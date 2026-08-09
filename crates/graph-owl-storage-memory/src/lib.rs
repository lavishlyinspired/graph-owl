//! In-memory `Storage` implementation, promoted from the integration-test fake.
//!
//! What an embedding consumer links instead of Postgres, and what makes the
//! `Storage` port provably a port rather than a Postgres-shaped interface
//! with one implementation (Epic 37c). Every field is a `Mutex`-guarded
//! `Vec` or `HashMap` — no cleverness, because the point of this backend is
//! to be obviously correct, not fast. It is not durable: nothing here
//! survives past the process, there is no cross-process transaction, and a
//! multi-threaded embedder gets safety from the same `Mutex`es a real
//! deployment would want replaced with something durable.
//!
//! An optional capacity bound is available via [`InMemoryStorage::bounded`]
//! for an embedder that wants a hard ceiling rather than unbounded growth;
//! [`InMemoryStorage::default`] has none, matching a test fixture's usual
//! expectation of "just work".

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    envelope::EntityVersion,
    page::{Cursor, Page, PageRequest},
};
use graph_owl_storage::{
    ConflictKind, DriftFilter, ReviewQueueFilter, SplitOutcome, Storage, StorageError, StoredUser,
    UpdateOutcome,
};
use uuid::Uuid;

/// An asset and everything beneath it. Used by the fake's cascade, which
/// must match Postgres's recursive CTE or a cascade bug passes here.
fn descendants(assets: &[Asset], root: Uuid) -> Vec<Uuid> {
    let mut found = vec![root];
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for child in assets.iter().filter(|a| a.parent_id == Some(parent)) {
            if !found.contains(&child.id) {
                found.push(child.id);
                frontier.push(child.id);
            }
        }
    }
    found
}
/// The fake's blocking-key computation (Epic 17 Slice B) — derived
/// on demand from the current `assets` list rather than a second store
/// kept in sync, since the fake already rebuilds every read from that
/// one list. Must derive identically to `PostgresStorage`'s stored keys,
/// so this calls the exact same pure functions rather than a
/// reimplementation that could quietly disagree.
fn asset_blocking_key_values(asset: &Asset, all: &[Asset]) -> [String; 4] {
    let column_hash = if asset.kind == AssetKind::Table {
        let columns: Vec<String> = all
            .iter()
            .filter(|a| a.parent_id == Some(asset.id) && a.kind == AssetKind::Column && !a.deleted)
            .map(|a| a.name.clone())
            .collect();
        graph_owl_core::blocking::column_hash_key(&columns)
    } else {
        graph_owl_core::blocking::column_hash_key(&[])
    };
    [
        graph_owl_core::blocking::normalized_fqn_key(&asset.fully_qualified_name),
        graph_owl_core::blocking::name_parent_key(
            &asset.name,
            graph_owl_core::fqn::parent(&asset.fully_qualified_name),
        ),
        graph_owl_core::blocking::soundex(&asset.name),
        column_hash,
    ]
}
/// The double's own evaluation of a custom-property filter.
///
/// **Written out rather than skipped**, because a double that ignored the
/// filter would report every asset for every query and make the facade's
/// composition tests pass against a filter that does nothing. It compares
/// JSON values the way `jsonb` does — numbers numerically, strings
/// lexicographically — which is what makes ISO-8601 dates order correctly on
/// both sides.
fn admits_extension(asset: &Asset, filters: &[graph_owl_storage::ExtensionFilter]) -> bool {
    use graph_owl_storage::ExtensionOp;
    filters.iter().all(|condition| {
        let Some(held) = asset
            .extension
            .as_ref()
            .and_then(|bag| bag.get(&condition.name))
        else {
            return false;
        };
        match condition.op {
            ExtensionOp::Eq => *held == condition.value,
            ExtensionOp::Gte | ExtensionOp::Lte => {
                let ordering = match (held.as_f64(), condition.value.as_f64()) {
                    (Some(a), Some(b)) => a.partial_cmp(&b),
                    _ => match (held.as_str(), condition.value.as_str()) {
                        (Some(a), Some(b)) => Some(a.cmp(b)),
                        _ => None,
                    },
                };
                matches!(
                    (ordering, condition.op),
                    (Some(std::cmp::Ordering::Less), ExtensionOp::Lte)
                        | (Some(std::cmp::Ordering::Greater), ExtensionOp::Gte)
                        | (Some(std::cmp::Ordering::Equal), _)
                )
            }
        }
    })
}
#[derive(Default)]
pub struct InMemoryStorage {
    assets: Mutex<Vec<Asset>>,
    versions: Mutex<Vec<AssetVersion>>,
    users: Mutex<Vec<StoredUser>>,
    pub policies: Mutex<Vec<Policy>>,
    /// `(role, policy name)` pairs — deliberately separate from `policies`'s
    /// own name-matching convention `policies_for_roles` already relies on,
    /// so adding a real write path here cannot change what that pre-existing
    /// read already does for tests written against it.
    role_policies: Mutex<Vec<(String, String)>>,
    inserted: Mutex<Vec<Table>>,
    relationships: Mutex<Vec<Relationship>>,
    /// The last validation pass, as the port stores it: the graph instant
    /// it ran against, and what it found.
    validation: Mutex<(i64, Vec<graph_owl_storage::ValidationFinding>)>,
    waivers: Mutex<Vec<graph_owl_storage::Waiver>>,
    assignments: Mutex<Vec<graph_owl_storage::Assignment>>,
    teams: Mutex<Vec<graph_owl_storage::Team>>,
    followers: Mutex<Vec<(Uuid, String)>>,
    #[allow(clippy::type_complexity)]
    idempotency: Mutex<Vec<(String, String, u16, serde_json::Value)>>,
    /// The config **and** its credential, so a test can prove the credential
    /// is kept and still never returned by the read path.
    #[allow(clippy::type_complexity)]
    connectors: Mutex<Vec<(graph_owl_storage::ConnectorConfig, Option<String>)>>,
    /// When armed, any relational write panics. Lets a test assert "this
    /// code path writes nothing" structurally instead of by reading it and
    /// believing what it says.
    writes_forbidden: std::sync::atomic::AtomicBool,
    /// How many times policies were read from storage. The decision cache
    /// is invisible in the *result* — a cached and an uncached predicate
    /// are the same predicate — so the only observable is whether the
    /// question reached storage at all.
    pub policy_reads: std::sync::atomic::AtomicUsize,
    source_hashes: Mutex<std::collections::HashMap<Uuid, Vec<u8>>>,
    /// Epic 32. Keyed by agent id, matching the table's one-grant-per-agent
    /// unique constraint — a double that allowed two grants would let a test
    /// pass against a shape production refuses.
    agent_grants: Mutex<std::collections::HashMap<String, graph_owl_authz::agent::AgentGrant>>,
    proposals: Mutex<Vec<graph_owl_authz::agent::Proposal>>,
    agent_activity: Mutex<Vec<graph_owl_authz::agent::AgentActivity>>,
    runs: Mutex<Vec<graph_owl_storage::ConnectorRun>>,
    lineage: Mutex<Vec<graph_owl_core::lineage::LineageEdge>>,
    memories: Mutex<Vec<graph_owl_core::memory::Memory>>,
    reviews: Mutex<Vec<graph_owl_core::contradiction::Review>>,
    /// `(asset, owners)` in submitted order — order is part of the contract,
    /// because validation reports failures by index.
    #[allow(clippy::type_complexity)]
    owners: Mutex<Vec<(Uuid, Vec<graph_owl_core::ownership::EntityReference>)>>,
    jobs: Mutex<Vec<graph_owl_storage::IngestJob>>,
    glossaries: Mutex<Vec<graph_owl_storage::Glossary>>,
    glossary_terms: Mutex<Vec<graph_owl_storage::GlossaryTermRecord>>,
    /// `(owner, relation)`, exactly the shape the port carries.
    #[allow(clippy::type_complexity)]
    term_relations: Mutex<Vec<(Uuid, graph_owl_core::glossary::SkosRelation)>>,
    term_reviewers: Mutex<Vec<(Uuid, Vec<String>)>>,
    /// `(term_id, target_fqn)`.
    term_attachments: Mutex<Vec<(Uuid, String)>>,
    metrics: Mutex<Vec<graph_owl_storage::MetricRecord>>,
    pub merge_records: Mutex<Vec<graph_owl_core::resolution::MergeRecord>>,
    pub resolution_queue: Mutex<Vec<graph_owl_core::resolution::ReviewQueueEntry>>,
    pub mention_resolutions: Mutex<Vec<graph_owl_core::resolution::MentionResolution>>,
    pub drift_reports: Mutex<Vec<graph_owl_core::drift::DriftItem>>,
    pub ontology_packs: Mutex<Vec<graph_owl_ontology::pack::OntologyPack>>,
    /// `(pack_id, term_id, source_iri)`.
    #[allow(clippy::type_complexity)]
    pack_terms: Mutex<Vec<(Uuid, Uuid, String)>>,
    pub pack_overrides: Mutex<Vec<graph_owl_ontology::pack::PackOverride>>,
    /// `(pack_id, turtle bytes)`.
    pack_source_turtle: Mutex<Vec<(Uuid, Vec<u8>)>>,
    pub threads: Mutex<Vec<graph_owl_core::collaboration::Thread>>,
    pub posts: Mutex<Vec<graph_owl_core::collaboration::Post>>,
    pub change_proposals: Mutex<Vec<graph_owl_core::collaboration::Proposal>>,
    pub announcements: Mutex<Vec<graph_owl_core::collaboration::Announcement>>,
    /// `(post_id, user_id, kind)`.
    #[allow(clippy::type_complexity)]
    reactions: Mutex<Vec<(Uuid, String, graph_owl_core::collaboration::ReactionKind)>>,
    /// `(endpoint, secret)` — the secret kept beside the public record so
    /// a test can prove it is kept and still never returned by the read
    /// path, matching the `connectors` field's own precedent.
    #[allow(clippy::type_complexity)]
    webhook_endpoints: Mutex<Vec<(graph_owl_storage::WebhookEndpoint, Option<Vec<u8>>)>>,
    inbound_events: Mutex<Vec<graph_owl_core::webhook::InboundEvent>>,
    /// `(subscription, secret)` — same reasoning as `webhook_endpoints`.
    #[allow(clippy::type_complexity)]
    stream_subscriptions: Mutex<Vec<(graph_owl_storage::StreamSubscription, Option<Vec<u8>>)>>,
    stream_dead_letters: Mutex<Vec<graph_owl_storage::StreamDeadLetter>>,
    /// `(webhook, secret)` — same reasoning as `webhook_endpoints`. Unlike
    /// that field, `secret` here starts required: `upsert_outbound_webhook`
    /// refuses a first registration with `None`, matching the real
    /// `NOT NULL` column this fake stands in for.
    #[allow(clippy::type_complexity)]
    outbound_webhooks: Mutex<Vec<(graph_owl_storage::OutboundWebhook, Vec<u8>)>>,
    outbound_webhook_deliveries: Mutex<Vec<graph_owl_storage::OutboundWebhookDelivery>>,
    mapping_versions: Mutex<Vec<graph_owl_storage::Mapping>>,
    entity_last_applied: Mutex<std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>>,
    custom_properties: Mutex<Vec<(Uuid, graph_owl_core::custom_property::CustomProperty)>>,
    domains: Mutex<Vec<graph_owl_core::domain::Domain>>,
    classifications: Mutex<Vec<graph_owl_core::classification::Classification>>,
    tags: Mutex<Vec<graph_owl_core::classification::Tag>>,
    labels: Mutex<Vec<graph_owl_core::classification::TagLabel>>,
    label_rejections: Mutex<Vec<(String, String)>>,
    certification_types: Mutex<Vec<graph_owl_storage::StoredCertificationType>>,
    certifications: Mutex<Vec<graph_owl_storage::StoredCertification>>,
    contracts: Mutex<Vec<graph_owl_core::contract::Contract>>,
    contract_breaches: Mutex<Vec<graph_owl_core::contract::ContractBreach>>,
    observations: Mutex<Vec<graph_owl_storage::UsageWrite>>,
    test_definitions: Mutex<Vec<graph_owl_storage::StoredTestDefinition>>,
    test_suites: Mutex<Vec<(Uuid, String, Option<String>)>>,
    test_cases: Mutex<Vec<graph_owl_storage::StoredTestCase>>,
    test_results: Mutex<Vec<graph_owl_storage::StoredTestResult>>,
    column_mappings: Mutex<Vec<(Uuid, graph_owl_storage::ColumnMapping)>>,
    data_products: Mutex<Vec<graph_owl_core::domain::DataProduct>>,
    product_members: Mutex<Vec<(Uuid, Uuid)>>,
    asset_domains: Mutex<Vec<(Uuid, Uuid)>>,
    extraction_runs: Mutex<Vec<graph_owl_storage::ExtractionRunRecord>>,
    extraction_claims: Mutex<Vec<graph_owl_storage::QueuedClaimRecord>>,
    extraction_discards: Mutex<Vec<graph_owl_storage::DiscardedClaimRecord>>,
    /// `None` (the `Default`) is unbounded — a test fixture's usual
    /// expectation. Set via [`InMemoryStorage::bounded`] for an embedder
    /// that wants a hard ceiling rather than unbounded growth.
    max_assets: Option<usize>,
}

impl InMemoryStorage {
    /// A store that refuses a *new* asset once it holds `max_assets`,
    /// rather than growing without limit.
    ///
    /// Assets are the one collection an embedder writes directly and
    /// repeatedly over a long-running process; everything else here
    /// (versions, relationships, findings, ...) is keyed off an asset
    /// that already exists, so bounding asset count is what actually
    /// stops an unbounded leak rather than bounding forty collections
    /// individually for a backend whose own docs already say "not for
    /// production volumes".
    ///
    /// Rejects rather than evicts: an entity silently vanishing from a
    /// catalog because a newer one displaced it is a correctness surprise
    /// no caller asked for. A full store is a `StorageError`, exactly
    /// like any other write refusal.
    #[must_use]
    pub fn bounded(max_assets: usize) -> Self {
        Self {
            max_assets: Some(max_assets),
            ..Self::default()
        }
    }

    pub fn forbid_writes(&self) {
        self.writes_forbidden
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Advance an asset's version because a label about it changed.
    ///
    /// A tag is not a column on the asset, so the ordinary update path
    /// never sees it — but a governance label appearing or vanishing is
    /// exactly what a consumer watches for, and one that left no version
    /// would be invisible to every one of them.
    fn bump_by_fqn(&self, target_fqn: &str, updated_by: &str) {
        let mut assets = self.assets.lock().expect("lock");
        if let Some(asset) = assets
            .iter_mut()
            .find(|a| a.fully_qualified_name == target_fqn)
        {
            asset.version = asset
                .version
                .bump(graph_owl_core::envelope::ChangeKind::Minor);
            asset.updated_by = updated_by.to_string();
            asset.updated_at = Utc::now();
        }
    }

    fn guard_write(&self, what: &str) {
        assert!(
            !self
                .writes_forbidden
                .load(std::sync::atomic::Ordering::SeqCst),
            "{what} wrote to relational storage while writes were forbidden"
        );
    }
}

#[async_trait::async_trait]
impl Storage for InMemoryStorage {
    // ---- Epic 31, and this double is deliberately as strict as the port ----
    //
    // Four times in this project a double has been looser than the port it
    // stands for, and each time the looseness hid a real defect until an
    // integration test found it. So: an unresolvable link target is rejected
    // here too, supersession writes both halves, and a dismissal is
    // normalised before it is stored.
    async fn save_memory(
        &self,
        memory: &graph_owl_core::memory::Memory,
    ) -> Result<graph_owl_storage::MemoryWrite, StorageError> {
        self.guard_write("save_memory");
        let mut held = self.memories.lock().expect("lock");
        if held.iter().any(|existing| existing.id == memory.id) {
            return Err(StorageError::Conflict {
                detail: format!("memory {} already exists", memory.id),
                existing_id: Some(memory.id),
                kind: graph_owl_storage::ConflictKind::MemoryExists,
            });
        }

        let assets = self.assets.lock().expect("lock");
        for (index, edge) in memory.links.iter().enumerate() {
            let known = assets.iter().any(|asset| asset.id == edge.target)
                || held.iter().any(|other| other.id == edge.target);
            if !known {
                return Ok(graph_owl_storage::MemoryWrite::UnknownLinkTarget {
                    index,
                    target: edge.target,
                });
            }
        }

        held.push(memory.clone());
        Ok(graph_owl_storage::MemoryWrite::Saved)
    }

    async fn find_memory(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::memory::Memory>, StorageError> {
        Ok(self
            .memories
            .lock()
            .expect("lock")
            .iter()
            .find(|memory| memory.id == id)
            .cloned())
    }

    async fn memories_about(
        &self,
        subject: Uuid,
        include_superseded: bool,
    ) -> Result<Vec<graph_owl_core::memory::Memory>, StorageError> {
        Ok(self
            .memories
            .lock()
            .expect("lock")
            .iter()
            .filter(|memory| memory.links.iter().any(|edge| edge.target == subject))
            .filter(|memory| include_superseded || memory.superseded_by.is_none())
            .cloned()
            .collect())
    }

    async fn supersede_memory(
        &self,
        original: Uuid,
        replacement: &graph_owl_core::memory::Memory,
    ) -> Result<graph_owl_storage::SupersedeOutcome, StorageError> {
        self.guard_write("supersede_memory");
        let mut held = self.memories.lock().expect("lock");
        let Some(position) = held.iter().position(|memory| memory.id == original) else {
            return Ok(graph_owl_storage::SupersedeOutcome::NotFound);
        };
        if let Some(current) = held[position].superseded_by {
            return Ok(graph_owl_storage::SupersedeOutcome::AlreadySuperseded { current });
        }

        let assets = self.assets.lock().expect("lock");
        for (index, edge) in replacement.links.iter().enumerate() {
            let known = assets.iter().any(|asset| asset.id == edge.target)
                || held.iter().any(|other| other.id == edge.target);
            if !known {
                return Ok(graph_owl_storage::SupersedeOutcome::UnknownLinkTarget {
                    index,
                    target: edge.target,
                });
            }
        }
        drop(assets);

        let mut correction = replacement.clone();
        correction.supersedes = Some(original);
        held[position].superseded_by = Some(correction.id);
        held.push(correction);
        Ok(graph_owl_storage::SupersedeOutcome::Superseded)
    }

    async fn retract_memory(
        &self,
        id: Uuid,
        reason: &str,
    ) -> Result<graph_owl_storage::RetractOutcome, StorageError> {
        self.guard_write("retract_memory");
        let mut held = self.memories.lock().expect("lock");
        let Some(memory) = held.iter_mut().find(|memory| memory.id == id) else {
            return Ok(graph_owl_storage::RetractOutcome::NotFound);
        };
        if memory.is_retracted() {
            return Ok(graph_owl_storage::RetractOutcome::AlreadyRetracted(
                memory.clone(),
            ));
        }
        memory.retracted_at = Some(chrono::Utc::now());
        memory.retraction_reason = Some(reason.to_string());
        Ok(graph_owl_storage::RetractOutcome::Retracted(memory.clone()))
    }

    async fn search_memories(
        &self,
        filter: &graph_owl_storage::MemorySearchFilter,
    ) -> Result<(Vec<graph_owl_core::memory::Memory>, i64), StorageError> {
        let matched: Vec<graph_owl_core::memory::Memory> = self
            .memories
            .lock()
            .expect("lock")
            .iter()
            .filter(|memory| {
                filter
                    .author
                    .as_deref()
                    .is_none_or(|author| match &memory.authorship {
                        graph_owl_core::memory::Authorship::Human { user_id } => user_id == author,
                        graph_owl_core::memory::Authorship::Agent { agent_id, .. } => {
                            agent_id == author
                        }
                    })
            })
            .filter(|memory| {
                filter
                    .min_confidence
                    .is_none_or(|min| memory.confidence >= min)
            })
            .filter(|memory| {
                filter
                    .max_confidence
                    .is_none_or(|max| memory.confidence <= max)
            })
            .filter(|memory| filter.since.is_none_or(|since| memory.as_of >= since))
            .filter(|memory| filter.until.is_none_or(|until| memory.as_of <= until))
            .filter(|memory| filter.include_superseded || memory.superseded_by.is_none())
            .filter(|memory| filter.include_retracted || !memory.is_retracted())
            .cloned()
            .collect();
        let total = i64::try_from(matched.len()).unwrap_or(i64::MAX);
        let page = matched
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit)
            .collect();
        Ok((page, total))
    }

    async fn review_contradiction(
        &self,
        review: graph_owl_core::contradiction::Review,
        _reviewed_by: &str,
        _note: Option<&str>,
    ) -> Result<(), StorageError> {
        self.guard_write("review_contradiction");
        let normalised = if review.a < review.b {
            review
        } else {
            graph_owl_core::contradiction::Review {
                a: review.b,
                b: review.a,
                verdict: review.verdict,
            }
        };
        let mut held = self.reviews.lock().expect("lock");
        // Upsert, as the port specifies: a reviewer changing their mind is one
        // pair with a new verdict. A double that appended would let a stale
        // verdict keep applying and would make the double looser than the
        // primary key it stands for.
        match held
            .iter_mut()
            .find(|existing| existing.a == normalised.a && existing.b == normalised.b)
        {
            Some(existing) => existing.verdict = normalised.verdict,
            None => held.push(normalised),
        }
        Ok(())
    }

    async fn contradiction_reviews(
        &self,
    ) -> Result<Vec<graph_owl_core::contradiction::Review>, StorageError> {
        Ok(self.reviews.lock().expect("lock").clone())
    }

    async fn claim_idempotency_key(
        &self,
        key: &str,
        request_hash: &str,
    ) -> Result<graph_owl_storage::IdempotencyClaim, StorageError> {
        let mut held = self.idempotency.lock().expect("lock");
        // The lock is this double's atomicity, standing in for the port's
        // `ON CONFLICT DO NOTHING`. A double that read, released, and wrote
        // would pass every single-threaded test and hide the exact race the
        // criterion is about.
        match held.iter().find(|(k, _, _, _)| k == key) {
            None => {
                held.push((
                    key.to_string(),
                    request_hash.to_string(),
                    0,
                    serde_json::Value::Null,
                ));
                Ok(graph_owl_storage::IdempotencyClaim::Claimed)
            }
            Some((_, stored, _, _)) if stored != request_hash => {
                Ok(graph_owl_storage::IdempotencyClaim::Mismatch)
            }
            Some((_, _, 0, _)) => Ok(graph_owl_storage::IdempotencyClaim::InFlight),
            Some((_, _, status, body)) => Ok(graph_owl_storage::IdempotencyClaim::Replay {
                status: *status,
                body: body.clone(),
            }),
        }
    }

    async fn record_idempotent_response(
        &self,
        key: &str,
        status: u16,
        body: &serde_json::Value,
    ) -> Result<(), StorageError> {
        let mut held = self.idempotency.lock().expect("lock");
        if let Some(entry) = held.iter_mut().find(|(k, _, _, _)| k == key) {
            entry.2 = status;
            entry.3 = body.clone();
        }
        Ok(())
    }

    // ---- Epic 16 Slice C, and as strict as the port ----

    async fn create_ingest_job(
        &self,
        job: &graph_owl_storage::IngestJob,
    ) -> Result<(), StorageError> {
        self.guard_write("create_ingest_job");
        self.jobs.lock().expect("lock").push(job.clone());
        Ok(())
    }

    async fn ingest_job(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::IngestJob>, StorageError> {
        Ok(self
            .jobs
            .lock()
            .expect("lock")
            .iter()
            .find(|job| job.id == id)
            .cloned())
    }

    async fn report_ingest_progress(
        &self,
        id: Uuid,
        progress: graph_owl_storage::IngestProgress,
        new_failures: &[graph_owl_storage::RowFailure],
    ) -> Result<bool, StorageError> {
        self.guard_write("report_ingest_progress");
        let mut held = self.jobs.lock().expect("lock");
        let Some(job) = held.iter_mut().find(|job| job.id == id) else {
            // The port stops a worker whose row has gone, and so does this:
            // a double that kept going would hide the case where it matters.
            return Ok(true);
        };
        job.rows_read = progress.rows_read;
        job.accepted = progress.accepted;
        job.rejected = progress.rejected;
        // Appended, exactly as the SQL does — a double that replaced would
        // pass a test that the real adapter would double-count.
        job.failures.extend_from_slice(new_failures);
        job.heartbeat_at = chrono::Utc::now();
        job.state = "running".to_string();
        Ok(job.cancel_requested)
    }

    async fn finish_ingest_job(
        &self,
        id: Uuid,
        state: &str,
        halt_reason: Option<&str>,
    ) -> Result<(), StorageError> {
        self.guard_write("finish_ingest_job");
        let mut held = self.jobs.lock().expect("lock");
        if let Some(job) = held.iter_mut().find(|job| job.id == id) {
            job.state = state.to_string();
            job.halt_reason = halt_reason.map(ToString::to_string);
            job.finished_at = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn cancel_ingest_job(&self, id: Uuid) -> Result<bool, StorageError> {
        self.guard_write("cancel_ingest_job");
        let mut held = self.jobs.lock().expect("lock");
        let Some(job) = held
            .iter_mut()
            .find(|job| job.id == id && job.finished_at.is_none())
        else {
            return Ok(false);
        };
        job.cancel_requested = true;
        Ok(true)
    }

    async fn reap_abandoned_ingest_jobs(
        &self,
        stale_after_seconds: i64,
    ) -> Result<u64, StorageError> {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(stale_after_seconds);
        let mut held = self.jobs.lock().expect("lock");
        let mut reaped = 0;
        for job in held
            .iter_mut()
            .filter(|job| job.finished_at.is_none() && job.heartbeat_at < cutoff)
        {
            job.state = "failed".to_string();
            job.halt_reason = Some("abandoned: the worker stopped reporting".to_string());
            job.finished_at = Some(chrono::Utc::now());
            reaped += 1;
        }
        Ok(reaped)
    }

    async fn child_teams(&self, id: &str) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
        Ok(self
            .teams
            .lock()
            .expect("lock")
            .iter()
            .filter(|team| team.parent_team_id.as_deref() == Some(id))
            .cloned()
            .collect())
    }

    async fn would_cycle(&self, team: &str, parent: &str) -> Result<bool, StorageError> {
        if team == parent {
            return Ok(true);
        }
        // Walks ancestry, as the port specifies. A double that compared only
        // the immediate parent would pass every depth-1 test and let
        // `A → B → C → A` through — which is Slice B's named mutator watch.
        let teams = self.teams.lock().expect("lock");
        let mut node = Some(parent.to_string());
        let mut seen = 0;
        while let Some(current) = node {
            if current == team {
                return Ok(true);
            }
            // Guard against an already-corrupt chain rather than looping
            // forever while deciding whether a loop would be created.
            seen += 1;
            if seen > teams.len() {
                return Ok(true);
            }
            node = teams
                .iter()
                .find(|candidate| candidate.id == current)
                .and_then(|candidate| candidate.parent_team_id.clone());
        }
        Ok(false)
    }

    async fn follow_asset(
        &self,
        asset_id: Uuid,
        user_id: &str,
    ) -> Result<graph_owl_storage::FollowOutcome, StorageError> {
        self.guard_write("follow_asset");
        let mut held = self.followers.lock().expect("lock");
        let edge = (asset_id, user_id.to_string());
        if held.contains(&edge) {
            return Ok(graph_owl_storage::FollowOutcome::AlreadyFollowing);
        }
        held.push(edge);
        Ok(graph_owl_storage::FollowOutcome::Followed)
    }

    async fn unfollow_asset(&self, asset_id: Uuid, user_id: &str) -> Result<(), StorageError> {
        self.guard_write("unfollow_asset");
        self.followers
            .lock()
            .expect("lock")
            .retain(|(id, who)| !(*id == asset_id && who == user_id));
        Ok(())
    }

    async fn assets_followed_by(
        &self,
        user_id: &str,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let followed: Vec<Uuid> = self
            .followers
            .lock()
            .expect("lock")
            .iter()
            .filter(|(_, who)| who == user_id)
            .map(|(id, _)| *id)
            .collect();
        let assets: Vec<Asset> = self
            .assets
            .lock()
            .expect("lock")
            .iter()
            .filter(|asset| followed.contains(&asset.id) && !asset.deleted)
            .cloned()
            .collect();
        Ok(Page::from_overfetch(assets, page.limit, |a: &Asset| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        }))
    }

    async fn follower_count(&self, asset_id: Uuid) -> Result<i64, StorageError> {
        Ok(i64::try_from(
            self.followers
                .lock()
                .expect("lock")
                .iter()
                .filter(|(id, _)| *id == asset_id)
                .count(),
        )
        .unwrap_or(i64::MAX))
    }

    async fn principal_holdings(
        &self,
        principal: &graph_owl_core::ownership::OwnerRef,
    ) -> Result<graph_owl_storage::Holdings, StorageError> {
        let owners = self.owners.lock().expect("lock");
        let assets = self.assets.lock().expect("lock");
        // Keyed on the wire string because `AssetKind` is deliberately not
        // `Ord` — deriving an ordering on a domain enum to satisfy a test
        // double would put an arbitrary rank on the hierarchy.
        let mut by_kind: std::collections::BTreeMap<&'static str, (AssetKind, i64)> =
            std::collections::BTreeMap::new();
        for (asset_id, list) in owners.iter() {
            if list
                .iter()
                .any(|o| o.id == principal.id && o.kind == principal.kind)
                && let Some(asset) = assets.iter().find(|a| a.id == *asset_id)
            {
                by_kind
                    .entry(asset.kind.as_str())
                    .or_insert((asset.kind, 0))
                    .1 += 1;
            }
        }
        drop(owners);
        drop(assets);
        let child_teams = match principal.kind {
            graph_owl_core::ownership::OwnerKind::Team => self
                .teams
                .lock()
                .expect("lock")
                .iter()
                .filter(|t| t.parent_team_id.as_deref() == Some(principal.id.as_str()))
                .map(|t| t.id.clone())
                .collect(),
            graph_owl_core::ownership::OwnerKind::User => Vec::new(),
        };
        Ok(graph_owl_storage::Holdings {
            owned_by_kind: by_kind.into_values().collect(),
            child_teams,
        })
    }

    async fn delete_principal(
        &self,
        principal: &graph_owl_core::ownership::OwnerRef,
        reassign_to: Option<&graph_owl_core::ownership::OwnerRef>,
    ) -> Result<graph_owl_storage::PrincipalDeletion, StorageError> {
        self.guard_write("delete_principal");
        let known = match principal.kind {
            graph_owl_core::ownership::OwnerKind::User => self
                .users
                .lock()
                .expect("lock")
                .iter()
                .any(|u| u.id == principal.id),
            graph_owl_core::ownership::OwnerKind::Team => self
                .teams
                .lock()
                .expect("lock")
                .iter()
                .any(|t| t.id == principal.id),
        };
        if !known {
            return Ok(graph_owl_storage::PrincipalDeletion::NotFound);
        }

        let holdings = self.principal_holdings(principal).await?;
        let mut reassigned = 0_i64;
        if let Some(target) = reassign_to {
            let target_known = match target.kind {
                graph_owl_core::ownership::OwnerKind::User => self
                    .users
                    .lock()
                    .expect("lock")
                    .iter()
                    .any(|u| u.id == target.id),
                graph_owl_core::ownership::OwnerKind::Team => self
                    .teams
                    .lock()
                    .expect("lock")
                    .iter()
                    .any(|t| t.id == target.id),
            };
            if !target_known {
                return Ok(graph_owl_storage::PrincipalDeletion::UnknownTarget);
            }
            let mut owners = self.owners.lock().expect("lock");
            let mut assets = self.assets.lock().expect("lock");
            for (asset_id, list) in owners.iter_mut() {
                let mut moved = false;
                for entry in list.iter_mut() {
                    if entry.id == principal.id && entry.kind == principal.kind {
                        entry.id = target.id.clone();
                        entry.kind = target.kind;
                        moved = true;
                    }
                }
                if moved {
                    reassigned += 1;
                    // The version bump the port promises; without it a transfer
                    // is invisible to anyone subscribed to Minor changes.
                    if let Some(asset) = assets.iter_mut().find(|a| a.id == *asset_id) {
                        asset.version.minor += 1;
                    }
                }
            }
        } else if !holdings.is_empty() {
            return Ok(graph_owl_storage::PrincipalDeletion::StillHolds(Box::new(
                holdings,
            )));
        }

        match principal.kind {
            graph_owl_core::ownership::OwnerKind::User => self
                .users
                .lock()
                .expect("lock")
                .retain(|u| u.id != principal.id),
            graph_owl_core::ownership::OwnerKind::Team => self
                .teams
                .lock()
                .expect("lock")
                .retain(|t| t.id != principal.id),
        }
        Ok(graph_owl_storage::PrincipalDeletion::Deleted { reassigned })
    }

    async fn set_asset_owners(
        &self,
        asset_id: Uuid,
        owners: &[graph_owl_core::ownership::OwnerRef],
    ) -> Result<graph_owl_storage::OwnersWrite, StorageError> {
        self.guard_write("set_asset_owners");
        let assets = self.assets.lock().expect("lock");
        if !assets.iter().any(|asset| asset.id == asset_id) {
            return Ok(graph_owl_storage::OwnersWrite::NotFound);
        }
        drop(assets);

        // As strict as the port: every principal is resolved before anything
        // is written, so a bad owner at index 2 does not leave 0 and 1
        // applied. A double that skipped this would hide the exact bug the
        // index-naming error exists to report.
        let users = self.users.lock().expect("lock");
        let teams = self.teams.lock().expect("lock");
        let mut resolved = Vec::with_capacity(owners.len());
        for (index, owner) in owners.iter().enumerate() {
            let found = match owner.kind {
                graph_owl_core::ownership::OwnerKind::User => users
                    .iter()
                    .find(|user| user.id == owner.id)
                    .map(|user| user.display_name.clone()),
                graph_owl_core::ownership::OwnerKind::Team => teams
                    .iter()
                    .find(|team| team.id == owner.id)
                    .map(|team| team.display_name.clone()),
            };
            let Some(display_name) = found else {
                return Ok(graph_owl_storage::OwnersWrite::UnknownPrincipal {
                    index,
                    id: owner.id.clone(),
                });
            };
            resolved.push(graph_owl_core::ownership::EntityReference {
                id: owner.id.clone(),
                kind: owner.kind,
                display_name,
                inherited: false,
            });
        }
        drop(users);
        drop(teams);

        let mut held = self.owners.lock().expect("lock");
        held.retain(|(id, _)| *id != asset_id);
        held.push((asset_id, resolved.clone()));
        Ok(graph_owl_storage::OwnersWrite::Set(resolved))
    }

    async fn asset_owners(
        &self,
        asset_id: Uuid,
    ) -> Result<Vec<graph_owl_core::ownership::EntityReference>, StorageError> {
        Ok(self
            .owners
            .lock()
            .expect("lock")
            .iter()
            .find(|(id, _)| *id == asset_id)
            .map(|(_, owners)| owners.clone())
            .unwrap_or_default())
    }

    async fn upsert_connector_config(
        &self,
        config: &graph_owl_storage::ConnectorConfig,
        secret: Option<&str>,
    ) -> Result<(), StorageError> {
        let mut held = self.connectors.lock().expect("lock");
        // `None` leaves an existing credential alone, exactly as the
        // adapter's `COALESCE` does. A double that cleared it would let a
        // facade pass here and silently break connectors in production.
        let existing = held
            .iter()
            .find(|(c, _)| c.connector == config.connector && c.service_name == config.service_name)
            .and_then(|(_, s): &(graph_owl_storage::ConnectorConfig, Option<String>)| s.clone());
        held.retain(|(c, _)| {
            !(c.connector == config.connector && c.service_name == config.service_name)
        });
        let kept = secret.map(ToString::to_string).or(existing);
        let mut stored = config.clone();
        stored.has_secret = kept.is_some();
        held.push((stored, kept));
        Ok(())
    }

    async fn connector_configs(
        &self,
    ) -> Result<Vec<graph_owl_storage::ConnectorConfig>, StorageError> {
        Ok(self
            .connectors
            .lock()
            .expect("lock")
            .iter()
            .map(|(c, _)| c.clone())
            .collect())
    }

    async fn connector_secret(&self, id: Uuid) -> Result<Option<String>, StorageError> {
        Ok(self
            .connectors
            .lock()
            .expect("lock")
            .iter()
            .find(|(c, _)| c.id == id)
            .and_then(|(_, s)| s.clone()))
    }

    async fn upsert_webhook_endpoint(
        &self,
        endpoint: graph_owl_storage::WebhookEndpoint,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::WebhookEndpoint, StorageError> {
        let mut held = self.webhook_endpoints.lock().unwrap();
        if held
            .iter()
            .any(|(e, _)| e.path == endpoint.path && e.id != endpoint.id)
        {
            return Err(StorageError::Conflict {
                detail: format!("path '{}' is already registered", endpoint.path),
                existing_id: None,
                kind: ConflictKind::WebhookPathExists,
            });
        }
        // `None` leaves an existing key alone, matching
        // `upsert_connector_config`'s own fake exactly.
        let existing_secret = held
            .iter()
            .find(|(e, _)| e.id == endpoint.id)
            .and_then(|(_, s)| s.clone());
        held.retain(|(e, _)| e.id != endpoint.id);
        let kept = secret.map(<[u8]>::to_vec).or(existing_secret);
        let mut stored = endpoint;
        stored.has_secret = kept.is_some();
        held.push((stored.clone(), kept));
        Ok(stored)
    }

    async fn get_webhook_endpoint(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, StorageError> {
        Ok(self
            .webhook_endpoints
            .lock()
            .unwrap()
            .iter()
            .find(|(e, _)| e.id == id)
            .map(|(e, _)| e.clone()))
    }

    async fn get_webhook_endpoint_by_path(
        &self,
        path: &str,
    ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, StorageError> {
        Ok(self
            .webhook_endpoints
            .lock()
            .unwrap()
            .iter()
            .find(|(e, _)| e.path == path)
            .map(|(e, _)| e.clone()))
    }

    async fn list_webhook_endpoints(
        &self,
    ) -> Result<Vec<graph_owl_storage::WebhookEndpoint>, StorageError> {
        Ok(self
            .webhook_endpoints
            .lock()
            .unwrap()
            .iter()
            .map(|(e, _)| e.clone())
            .collect())
    }

    async fn webhook_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self
            .webhook_endpoints
            .lock()
            .unwrap()
            .iter()
            .find(|(e, _)| e.id == id)
            .and_then(|(_, s)| s.clone()))
    }

    async fn upsert_stream_subscription(
        &self,
        subscription: graph_owl_storage::StreamSubscription,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::StreamSubscription, StorageError> {
        let mut held = self.stream_subscriptions.lock().unwrap();
        if held.iter().any(|(s, _)| {
            s.topic == subscription.topic
                && s.consumer_group == subscription.consumer_group
                && s.id != subscription.id
        }) {
            return Err(StorageError::Conflict {
                detail: format!(
                    "topic '{}' with consumer group '{}' is already registered",
                    subscription.topic, subscription.consumer_group
                ),
                existing_id: None,
                kind: ConflictKind::StreamSubscriptionExists,
            });
        }
        let existing_secret = held
            .iter()
            .find(|(s, _)| s.id == subscription.id)
            .and_then(|(_, s)| s.clone());
        held.retain(|(s, _)| s.id != subscription.id);
        let kept = secret.map(<[u8]>::to_vec).or(existing_secret);
        let mut stored = subscription;
        stored.has_secret = kept.is_some();
        held.push((stored.clone(), kept));
        Ok(stored)
    }

    async fn get_stream_subscription(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StreamSubscription>, StorageError> {
        Ok(self
            .stream_subscriptions
            .lock()
            .unwrap()
            .iter()
            .find(|(s, _)| s.id == id)
            .map(|(s, _)| s.clone()))
    }

    async fn list_stream_subscriptions(
        &self,
    ) -> Result<Vec<graph_owl_storage::StreamSubscription>, StorageError> {
        Ok(self
            .stream_subscriptions
            .lock()
            .unwrap()
            .iter()
            .map(|(s, _)| s.clone())
            .collect())
    }

    async fn stream_subscription_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self
            .stream_subscriptions
            .lock()
            .unwrap()
            .iter()
            .find(|(s, _)| s.id == id)
            .and_then(|(_, s)| s.clone()))
    }

    async fn create_stream_dead_letter(
        &self,
        letter: graph_owl_storage::StreamDeadLetter,
    ) -> Result<graph_owl_storage::StreamDeadLetter, StorageError> {
        self.stream_dead_letters
            .lock()
            .unwrap()
            .push(letter.clone());
        Ok(letter)
    }

    async fn get_stream_dead_letter(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StreamDeadLetter>, StorageError> {
        Ok(self
            .stream_dead_letters
            .lock()
            .unwrap()
            .iter()
            .find(|l| l.id == id)
            .cloned())
    }

    async fn list_stream_dead_letters(
        &self,
        subscription: Option<Uuid>,
    ) -> Result<Vec<graph_owl_storage::StreamDeadLetter>, StorageError> {
        let mut letters: Vec<_> = self
            .stream_dead_letters
            .lock()
            .unwrap()
            .iter()
            .filter(|l| subscription.is_none_or(|s| l.subscription_id == s))
            .cloned()
            .collect();
        letters.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(letters)
    }

    async fn delete_stream_dead_letter(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut held = self.stream_dead_letters.lock().unwrap();
        let before = held.len();
        held.retain(|l| l.id != id);
        Ok(held.len() < before)
    }

    async fn upsert_outbound_webhook(
        &self,
        webhook: graph_owl_storage::OutboundWebhook,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::OutboundWebhook, StorageError> {
        let mut held = self.outbound_webhooks.lock().unwrap();
        let existing_secret = held
            .iter()
            .find(|(w, _)| w.id == webhook.id)
            .map(|(_, s)| s.clone());
        let Some(kept) = secret.map(<[u8]>::to_vec).or(existing_secret) else {
            return Err(StorageError::Unexpected(
                "an outbound webhook requires a signing secret on first registration".to_string(),
            ));
        };
        held.retain(|(w, _)| w.id != webhook.id);
        held.push((webhook.clone(), kept));
        Ok(webhook)
    }

    async fn get_outbound_webhook(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::OutboundWebhook>, StorageError> {
        Ok(self
            .outbound_webhooks
            .lock()
            .unwrap()
            .iter()
            .find(|(w, _)| w.id == id)
            .map(|(w, _)| w.clone()))
    }

    async fn list_outbound_webhooks(
        &self,
    ) -> Result<Vec<graph_owl_storage::OutboundWebhook>, StorageError> {
        Ok(self
            .outbound_webhooks
            .lock()
            .unwrap()
            .iter()
            .map(|(w, _)| w.clone())
            .collect())
    }

    async fn outbound_webhook_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self
            .outbound_webhooks
            .lock()
            .unwrap()
            .iter()
            .find(|(w, _)| w.id == id)
            .map(|(_, s)| s.clone()))
    }

    async fn enqueue_outbound_webhook_delivery(
        &self,
        webhook_id: Uuid,
        payload: serde_json::Value,
    ) -> Result<graph_owl_storage::OutboundWebhookDelivery, StorageError> {
        let delivery = graph_owl_storage::OutboundWebhookDelivery {
            id: Uuid::new_v4(),
            webhook_id,
            payload,
            attempt: 0,
            next_attempt_at: chrono::Utc::now(),
            last_error: None,
            dead_lettered: false,
            created_at: chrono::Utc::now(),
        };
        self.outbound_webhook_deliveries
            .lock()
            .unwrap()
            .push(delivery.clone());
        Ok(delivery)
    }

    async fn list_outbound_webhook_deliveries(
        &self,
        webhook_id: Uuid,
    ) -> Result<Vec<graph_owl_storage::OutboundWebhookDelivery>, StorageError> {
        Ok(self
            .outbound_webhook_deliveries
            .lock()
            .unwrap()
            .iter()
            .filter(|d| d.webhook_id == webhook_id)
            .cloned()
            .collect())
    }

    async fn create_inbound_event(
        &self,
        mut event: graph_owl_core::webhook::InboundEvent,
    ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
        let mut held = self.inbound_events.lock().unwrap();
        // Mirrors the Postgres impl's dedup marker: the lock held across
        // the check and the push is what makes this atomic for a single
        // process, the same way `(endpoint_id, dedup_key)`'s primary key
        // makes it atomic across connections.
        if held
            .iter()
            .any(|e| e.endpoint == event.endpoint && e.dedup_key == event.dedup_key)
        {
            event.state = graph_owl_core::webhook::EventState::Duplicate;
        }
        held.push(event.clone());
        Ok(event)
    }

    async fn get_inbound_event(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::webhook::InboundEvent>, StorageError> {
        Ok(self
            .inbound_events
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.id == id)
            .cloned())
    }

    async fn update_inbound_event_state(
        &self,
        id: Uuid,
        state: graph_owl_core::webhook::EventState,
        reason: Option<&str>,
    ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
        let mut held = self.inbound_events.lock().unwrap();
        let event = held
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| StorageError::Unexpected(format!("no inbound event {id}")))?;
        event.state = state;
        event.reason = reason.map(str::to_string);
        Ok(event.clone())
    }

    async fn list_dead_letters(
        &self,
        filter: &graph_owl_storage::DeadLetterFilter,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError> {
        let mut matching: Vec<_> = self
            .inbound_events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.state == graph_owl_core::webhook::EventState::Failed)
            .filter(|e| {
                filter
                    .endpoint
                    .is_none_or(|endpoint| e.endpoint == endpoint)
            })
            .filter(|e| {
                filter.reason_contains.as_ref().is_none_or(|needle| {
                    e.reason
                        .as_ref()
                        .is_some_and(|reason| reason.contains(needle.as_str()))
                })
            })
            .cloned()
            .collect();
        matching.sort_by_key(|e| std::cmp::Reverse(e.received_at));
        Ok(matching
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit)
            .collect())
    }

    async fn list_inbound_events_in_window(
        &self,
        endpoint: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError> {
        let mut matching: Vec<_> = self
            .inbound_events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.endpoint == endpoint && e.received_at >= since && e.received_at <= until)
            .cloned()
            .collect();
        matching.sort_by_key(|e| e.sender_timestamp.unwrap_or(e.received_at));
        Ok(matching)
    }

    async fn purge_dead_letters(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StorageError> {
        let mut held = self.inbound_events.lock().unwrap();
        let before = held.len();
        held.retain(|e| {
            !(e.state == graph_owl_core::webhook::EventState::Failed && e.received_at < older_than)
        });
        Ok((before - held.len()) as u64)
    }

    async fn last_applied_timestamp(
        &self,
        fully_qualified_name: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
        Ok(self
            .entity_last_applied
            .lock()
            .unwrap()
            .get(fully_qualified_name)
            .copied())
    }

    async fn record_applied_timestamp(
        &self,
        fully_qualified_name: &str,
        sender_timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StorageError> {
        self.entity_last_applied
            .lock()
            .unwrap()
            .insert(fully_qualified_name.to_string(), sender_timestamp);
        Ok(())
    }

    async fn upsert_mapping(
        &self,
        mut mapping: graph_owl_storage::Mapping,
    ) -> Result<graph_owl_storage::Mapping, StorageError> {
        self.guard_write("upsert_mapping");
        let mut held = self.mapping_versions.lock().unwrap();
        mapping.version = held
            .iter()
            .filter(|m| m.name == mapping.name)
            .map(|m| m.version)
            .max()
            .unwrap_or(0)
            + 1;
        mapping.created_at = chrono::Utc::now();
        held.push(mapping.clone());
        Ok(mapping)
    }

    async fn get_mapping(
        &self,
        name: &str,
    ) -> Result<Option<graph_owl_storage::Mapping>, StorageError> {
        Ok(self
            .mapping_versions
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.name == name)
            .max_by_key(|m| m.version)
            .cloned())
    }

    async fn list_mapping_versions(
        &self,
        name: &str,
    ) -> Result<Vec<graph_owl_storage::Mapping>, StorageError> {
        let mut versions: Vec<_> = self
            .mapping_versions
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect();
        versions.sort_by_key(|m| std::cmp::Reverse(m.version));
        Ok(versions)
    }

    async fn upsert_team(&self, team: &graph_owl_storage::Team) -> Result<(), StorageError> {
        // The real adapter's foreign key refuses an unknown member. A
        // looser double would let a facade skipping the check pass here
        // and fail against Postgres.
        let users = self.users.lock().expect("lock");
        for member in &team.members {
            if users.iter().all(|u| &u.id != member) {
                return Err(StorageError::Unexpected(format!(
                    "`{member}` is not a known user"
                )));
            }
        }
        drop(users);
        let mut teams = self.teams.lock().expect("lock");
        teams.retain(|t: &graph_owl_storage::Team| t.id != team.id);
        let mut stored = team.clone();
        // Ordered, as the adapter's `ARRAY_AGG ... ORDER BY` guarantees, so
        // two reads of an unchanged team compare equal.
        stored.members.sort();
        teams.push(stored);
        Ok(())
    }

    async fn find_team(&self, id: &str) -> Result<Option<graph_owl_storage::Team>, StorageError> {
        Ok(self
            .teams
            .lock()
            .expect("lock")
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn teams(&self) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
        let mut teams = self.teams.lock().expect("lock").clone();
        teams.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(teams)
    }

    async fn assign_finding(
        &self,
        assignment: &graph_owl_storage::Assignment,
    ) -> Result<(), StorageError> {
        // The real adapter's foreign key is what refuses an unknown
        // assignee. A double that accepted one would let a facade skipping
        // the check pass here and fail against Postgres.
        if self
            .users
            .lock()
            .expect("lock")
            .iter()
            .all(|u| u.id != assignment.assignee)
        {
            return Err(StorageError::Unexpected(
                "that assignee is not a known user".to_string(),
            ));
        }
        let mut held = self.assignments.lock().expect("lock");
        let identity = |a: &graph_owl_storage::Assignment| {
            (
                a.shape.clone(),
                a.focus_node.clone(),
                a.path.clone().unwrap_or_default(),
                a.constraint_kind.clone(),
            )
        };
        if held.iter().any(|a| identity(a) == identity(assignment)) {
            return Err(StorageError::Conflict {
                detail: "this finding is already assigned".to_string(),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::AssignmentExists,
            });
        }
        held.push(assignment.clone());
        Ok(())
    }

    async fn unassign_finding(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut held = self.assignments.lock().expect("lock");
        let before = held.len();
        held.retain(|a| a.id != id);
        Ok(held.len() < before)
    }

    async fn assignments(&self) -> Result<Vec<graph_owl_storage::Assignment>, StorageError> {
        Ok(self.assignments.lock().expect("lock").clone())
    }

    async fn waive_finding(&self, waiver: &graph_owl_storage::Waiver) -> Result<(), StorageError> {
        let mut waivers = self.waivers.lock().expect("lock");
        // The same uniqueness the unique index enforces. A double that
        // accepted a second waiver would let a facade that never checks
        // pass here and conflict against Postgres.
        let identity = |w: &graph_owl_storage::Waiver| {
            (
                w.shape.clone(),
                w.focus_node.clone(),
                w.path.clone().unwrap_or_default(),
                w.constraint_kind.clone(),
            )
        };
        if waivers
            .iter()
            .any(|held| identity(held) == identity(waiver))
        {
            return Err(StorageError::Conflict {
                detail: "this finding is already waived".to_string(),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::WaiverExists,
            });
        }
        waivers.push(waiver.clone());
        Ok(())
    }

    async fn revoke_waiver(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut waivers = self.waivers.lock().expect("lock");
        let before = waivers.len();
        waivers.retain(|w| w.id != id);
        Ok(waivers.len() < before)
    }

    async fn waivers(&self) -> Result<Vec<graph_owl_storage::Waiver>, StorageError> {
        Ok(self.waivers.lock().expect("lock").clone())
    }

    /// Wholesale replacement, same as the real one: a fixed violation must
    /// vanish rather than linger until something deletes it.
    async fn replace_validation_results(
        &self,
        computed_at_t: i64,
        results: &[graph_owl_storage::ValidationFinding],
    ) -> Result<(), StorageError> {
        let mut stored = self.validation.lock().expect("lock");
        *stored = (computed_at_t, results.to_vec());
        Ok(())
    }

    /// Filtered and ordered the way the adapter orders — a double that
    /// returned everything unsorted would let a facade that ignores the
    /// filter pass here and fail against Postgres.
    async fn validation_results(
        &self,
        filter: &graph_owl_storage::ValidationFilter,
    ) -> Result<(Vec<graph_owl_storage::ValidationFinding>, i64, usize), StorageError> {
        let (computed_at_t, all) = self.validation.lock().expect("lock").clone();
        let matching: Vec<graph_owl_storage::ValidationFinding> = all
            .into_iter()
            .filter(|f| {
                filter.severity.as_ref().is_none_or(|s| &f.severity == s)
                    && filter.shape.as_ref().is_none_or(|s| &f.shape == s)
                    && filter
                        .focus_node
                        .as_ref()
                        .is_none_or(|n| &f.focus_node == n)
            })
            .collect();
        let total = matching.len();
        let page = matching
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit)
            .collect();
        Ok((page, computed_at_t, total))
    }

    async fn ping(&self) -> Result<(), StorageError> {
        Ok(())
    }

    // The fake honours the same identity rule as Postgres: the FQN is the
    // identity, so a re-upsert converges instead of duplicating.
    async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError> {
        self.guard_write("upsert_asset");
        let mut assets = self.assets.lock().unwrap();
        if let Some(existing) = assets
            .iter_mut()
            .find(|a| a.fully_qualified_name == asset.fully_qualified_name)
        {
            existing.name = asset.name;
            existing.parent_id = asset.parent_id;
            existing.description = asset.description.or(existing.description.clone());
            existing.properties = asset.properties.or(existing.properties.clone());
            existing.updated_at = asset.updated_at;
            return Ok(existing.clone());
        }
        if let Some(max) = self.max_assets
            && assets.len() >= max
        {
            return Err(StorageError::Unexpected(format!(
                "in-memory store is bounded at {max} assets and is full"
            )));
        }
        assets.push(asset.clone());
        Ok(asset)
    }

    async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError> {
        Ok(self
            .assets
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.id == id)
            .cloned())
    }

    async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
        Ok(self
            .assets
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.fully_qualified_name == fqn)
            .cloned())
    }

    async fn list_assets(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let mut assets: Vec<Asset> = self
            .assets
            .lock()
            .unwrap()
            .iter()
            .filter(|a| kind.is_none_or(|k| a.kind == k))
            .cloned()
            .collect();
        assets.sort_by(|a, b| {
            a.fully_qualified_name
                .cmp(&b.fully_qualified_name)
                .then(a.id.cmp(&b.id))
        });
        if let Some(cursor) = &page.after {
            assets.retain(|a| {
                (a.fully_qualified_name.as_str(), a.id) > (cursor.sort_key.as_str(), cursor.id)
            });
        }
        assets.truncate(page.limit + 1);
        Ok(Page::from_overfetch(assets, page.limit, |a| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        }))
    }

    async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, StorageError> {
        let mut children: Vec<Asset> = self
            .assets
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.parent_id == parent_id)
            .cloned()
            .collect();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(children)
    }

    async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, StorageError> {
        let assets = self.assets.lock().unwrap().clone();
        let mut chain = Vec::new();
        let mut current = assets.iter().find(|a| a.id == id).cloned();
        while let Some(asset) = current {
            current = asset
                .parent_id
                .and_then(|pid| assets.iter().find(|a| a.id == pid).cloned());
            chain.push(asset);
        }
        chain.reverse();
        Ok(chain)
    }

    async fn search_assets(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let needle = query.to_lowercase();
        let mut assets: Vec<Asset> = self
            .assets
            .lock()
            .unwrap()
            .iter()
            .filter(|a| {
                (a.name.to_lowercase().contains(&needle)
                    || a.fully_qualified_name.to_lowercase().contains(&needle)
                    // Epic 34 Slice A: the weight-D component the real
                    // Postgres `search_vector` gained for the same reason —
                    // a dashboard's own row carries its charts' names so it
                    // is findable by them without a per-row search join.
                    || a.properties
                        .as_ref()
                        .and_then(|p| p.get("chartNames"))
                        .and_then(|v| v.as_str())
                        .is_some_and(|names| names.to_lowercase().contains(&needle))
                    // Phase 3 item 3.7: the same treatment for a table's
                    // columns — see `Catalog::sync_table_column_names`.
                    || a.properties
                        .as_ref()
                        .and_then(|p| p.get("columnNames"))
                        .and_then(|v| v.as_str())
                        .is_some_and(|names| names.to_lowercase().contains(&needle)))
                    && kind.is_none_or(|k| a.kind == k)
            })
            .cloned()
            .collect();
        assets.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        assets.truncate(page.limit + 1);
        Ok(Page::from_overfetch(assets, page.limit, |a| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        }))
    }

    async fn list_assets_under_fqn(&self, prefix: &str) -> Result<Vec<Asset>, StorageError> {
        // Same boundary rule as Postgres: `hdfc-core` must not sweep in
        // `hdfc-core-archive`. A fake that matched more loosely would let a
        // scope bug pass here and fail only against the real database.
        let assets = self.assets.lock().unwrap();
        Ok(assets
            .iter()
            .filter(|a| {
                !a.deleted
                    && (prefix.is_empty()
                        || a.fully_qualified_name == prefix
                        || a.fully_qualified_name.starts_with(&format!("{prefix}.")))
            })
            .cloned()
            .collect())
    }

    async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError> {
        let assets = self.assets.lock().unwrap();
        Ok(AssetKind::ALL
            .into_iter()
            .map(|kind| {
                (
                    kind,
                    i64::try_from(assets.iter().filter(|a| a.kind == kind).count())
                        .unwrap_or(i64::MAX),
                )
            })
            .filter(|(_, n)| *n > 0)
            .collect())
    }

    async fn resolution_candidates(&self, asset_id: Uuid) -> Result<Vec<Asset>, StorageError> {
        let assets = self.assets.lock().unwrap();
        let Some(target) = assets.iter().find(|a| a.id == asset_id) else {
            return Ok(Vec::new());
        };
        let target_keys = asset_blocking_key_values(target, &assets);
        Ok(assets
            .iter()
            .filter(|a| a.id != asset_id && !a.deleted)
            .filter(|a| {
                let keys = asset_blocking_key_values(a, &assets);
                target_keys
                    .iter()
                    .any(|k| !k.is_empty() && keys.contains(k))
            })
            .cloned()
            .collect())
    }

    async fn create_merge_record(
        &self,
        record: graph_owl_core::resolution::MergeRecord,
    ) -> Result<graph_owl_core::resolution::MergeRecord, StorageError> {
        self.guard_write("create_merge_record");
        self.merge_records.lock().unwrap().push(record.clone());
        Ok(record)
    }

    async fn get_merge_record(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::resolution::MergeRecord>, StorageError> {
        Ok(self
            .merge_records
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.id == id)
            .cloned())
    }

    async fn split_merge_record(
        &self,
        id: Uuid,
        split_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SplitOutcome, StorageError> {
        self.guard_write("split_merge_record");
        let mut records = self.merge_records.lock().unwrap();
        let Some(record) = records.iter_mut().find(|r| r.id == id) else {
            return Ok(SplitOutcome::NotFound);
        };
        if let Some(already) = record.split_at {
            return Ok(SplitOutcome::AlreadySplit { split_at: already });
        }
        record.split_at = Some(split_at);
        Ok(SplitOutcome::Split(Box::new(record.clone())))
    }

    async fn most_recent_split_between(
        &self,
        a: Uuid,
        b: Uuid,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
        Ok(self
            .merge_records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| (r.canonical == a && r.merged == b) || (r.canonical == b && r.merged == a))
            .filter_map(|r| r.split_at)
            .max())
    }

    async fn queue_for_review(
        &self,
        entry: graph_owl_core::resolution::ReviewQueueEntry,
    ) -> Result<graph_owl_core::resolution::ReviewQueueEntry, StorageError> {
        let mut queue = self.resolution_queue.lock().unwrap();
        if let Some(existing) = queue
            .iter()
            .find(|e| e.target == entry.target && e.candidate == entry.candidate)
        {
            return Ok(existing.clone());
        }
        queue.push(entry.clone());
        Ok(entry)
    }

    async fn list_review_queue(
        &self,
        filter: &ReviewQueueFilter,
    ) -> Result<(Vec<graph_owl_core::resolution::ReviewQueueEntry>, i64), StorageError> {
        use graph_owl_core::resolution::ReviewStatus;
        let assets = self.assets.lock().unwrap();
        let status = filter.status.unwrap_or(ReviewStatus::Pending);
        let matching: Vec<_> = self
            .resolution_queue
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.status == status)
            .filter(|e| {
                filter.kind.is_none_or(|kind| {
                    assets
                        .iter()
                        .find(|a| a.id == e.target)
                        .is_some_and(|a| a.kind == kind)
                })
            })
            .filter(|e| filter.min_score.is_none_or(|min| e.score >= min))
            .filter(|e| filter.max_score.is_none_or(|max| e.score <= max))
            .cloned()
            .collect();
        let mut sorted = matching;
        sorted.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        let total = i64::try_from(sorted.len()).unwrap_or(i64::MAX);
        let page = sorted
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit)
            .collect();
        Ok((page, total))
    }

    async fn get_review_queue_entry(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError> {
        Ok(self
            .resolution_queue
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.id == id)
            .cloned())
    }

    async fn decide_review_queue_entry(
        &self,
        id: Uuid,
        status: graph_owl_core::resolution::ReviewStatus,
        decided_by: graph_owl_core::resolution::MergeDecidedBy,
        decided_at: chrono::DateTime<chrono::Utc>,
        reason: Option<String>,
    ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError> {
        use graph_owl_core::resolution::ReviewStatus;
        let mut queue = self.resolution_queue.lock().unwrap();
        let Some(entry) = queue.iter_mut().find(|e| e.id == id) else {
            return Ok(None);
        };
        if entry.status != ReviewStatus::Pending {
            return Ok(Some(entry.clone()));
        }
        entry.status = status;
        entry.decided_by = Some(decided_by);
        entry.decided_at = Some(decided_at);
        entry.reason = reason;
        Ok(Some(entry.clone()))
    }

    async fn push_drift(
        &self,
        asset_id: Uuid,
        item: graph_owl_core::drift::DriftReportItem,
    ) -> Result<graph_owl_core::drift::DriftItem, StorageError> {
        use graph_owl_core::drift::DriftStatus;
        let mut reports = self.drift_reports.lock().unwrap();
        if let Some(existing) = reports.iter().find(|d| {
            d.asset_id == asset_id && d.field == item.field && d.status == DriftStatus::Pending
        }) {
            return Ok(existing.clone());
        }
        let fully_qualified_name = self
            .assets
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.id == asset_id)
            .map(|a| a.fully_qualified_name.clone())
            .unwrap_or_default();
        let created = graph_owl_core::drift::DriftItem {
            id: Uuid::new_v4(),
            asset_id,
            fully_qualified_name,
            field: item.field,
            kind: item.kind,
            live_value: item.live_value,
            declared_value: item.declared_value,
            status: DriftStatus::Pending,
            reported_at: chrono::Utc::now(),
            decided_at: None,
            decided_by: None,
            reason: None,
        };
        reports.push(created.clone());
        Ok(created)
    }

    async fn list_drift(
        &self,
        filter: &DriftFilter,
    ) -> Result<(Vec<graph_owl_core::drift::DriftItem>, i64), StorageError> {
        use graph_owl_core::drift::DriftStatus;
        let status = filter.status.unwrap_or(DriftStatus::Pending);
        let mut matching: Vec<_> = self
            .drift_reports
            .lock()
            .unwrap()
            .iter()
            .filter(|d| d.status == status)
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.reported_at.cmp(&a.reported_at).then(a.id.cmp(&b.id)));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit)
            .collect();
        Ok((page, total))
    }

    async fn get_drift_item(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::drift::DriftItem>, StorageError> {
        Ok(self
            .drift_reports
            .lock()
            .unwrap()
            .iter()
            .find(|d| d.id == id)
            .cloned())
    }

    async fn decide_drift(
        &self,
        id: Uuid,
        status: graph_owl_core::drift::DriftStatus,
        decided_by: String,
        decided_at: chrono::DateTime<chrono::Utc>,
        reason: Option<String>,
    ) -> Result<Option<graph_owl_core::drift::DriftItem>, StorageError> {
        use graph_owl_core::drift::DriftStatus;
        let mut reports = self.drift_reports.lock().unwrap();
        let Some(item) = reports.iter_mut().find(|d| d.id == id) else {
            return Ok(None);
        };
        if item.status != DriftStatus::Pending {
            return Ok(Some(item.clone()));
        }
        item.status = status;
        item.decided_by = Some(decided_by);
        item.decided_at = Some(decided_at);
        item.reason = reason;
        Ok(Some(item.clone()))
    }

    async fn record_mention_resolution(
        &self,
        resolution: graph_owl_core::resolution::MentionResolution,
    ) -> Result<graph_owl_core::resolution::MentionResolution, StorageError> {
        self.mention_resolutions
            .lock()
            .unwrap()
            .push(resolution.clone());
        Ok(resolution)
    }

    async fn mention_resolutions_for_source(
        &self,
        source: Uuid,
    ) -> Result<Vec<graph_owl_core::resolution::MentionResolution>, StorageError> {
        let mut found: Vec<_> = self
            .mention_resolutions
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.source == source)
            .cloned()
            .collect();
        found.sort_by_key(|b| std::cmp::Reverse(b.resolved_at));
        Ok(found)
    }

    // The fake honours the same envelope contract as Postgres, including
    // the no-op rule: a fake that always bumped would let a version-inflation
    // bug pass here and fail only against a real database.
    async fn update_asset(
        &self,
        id: Uuid,
        update: &AssetUpdate,
        updated_by: &str,
        expected_version: Option<EntityVersion>,
    ) -> Result<UpdateOutcome, StorageError> {
        use graph_owl_core::envelope::{ChangeDescription, ChangeKind, classify};
        self.guard_write("update_asset");
        let mut assets = self.assets.lock().unwrap();
        let Some(before) = assets.iter().find(|a| a.id == id).cloned() else {
            return Ok(UpdateOutcome::NotFound);
        };
        // The fake enforces the precondition too. One that ignored it would
        // let a lost-update bug pass here and fail only against Postgres.
        if expected_version.is_some_and(|expected| before.version != expected) {
            return Ok(UpdateOutcome::VersionMismatch(before.version));
        }
        let mut after = before.clone();
        if let Some(description) = &update.description {
            after.description = description.clone();
        }
        // Phase 3 item 3.3 — the same subtree-cascade guarantee the Postgres
        // adapter gives, over a plain `Vec` instead of a recursive CTE.
        // Computed here, before taking a mutable borrow of `existing` below,
        // since scanning `assets` for the parent's row and a mutable borrow
        // of one of its own elements cannot coexist.
        if let Some(name) = &update.name {
            after.name.clone_from(name);
            let parent_fqn = after
                .parent_id
                .and_then(|parent_id| assets.iter().find(|a| a.id == parent_id))
                .map(|parent| parent.fully_qualified_name.clone());
            after.fully_qualified_name = match &parent_fqn {
                Some(parent) => graph_owl_core::fqn::child_of(parent, &after.name),
                None => graph_owl_core::fqn::derive(&[&after.name]),
            }
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if after.fully_qualified_name != before.fully_qualified_name
                && assets
                    .iter()
                    .any(|a| a.id != id && a.fully_qualified_name == after.fully_qualified_name)
            {
                return Err(StorageError::Conflict {
                    detail: format!(
                        "an asset already exists at `{}`",
                        after.fully_qualified_name
                    ),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                });
            }
        }
        let diff = ChangeDescription::between(
            &serde_json::to_value(&before).unwrap_or_default(),
            &serde_json::to_value(&after).unwrap_or_default(),
        );
        let kind = classify(&diff);
        if matches!(kind, ChangeKind::None) {
            return Ok(UpdateOutcome::Updated(Box::new(before)));
        }
        after.version = before.version.bump(kind);
        after.updated_by = updated_by.to_string();
        after.change_description = Some(diff.clone());
        after.updated_at = Utc::now();

        // **The subtree's paths move with it** — every descendant's own
        // `fully_qualified_name` gets the old prefix swapped for the new
        // one, transitively, matching `update_domain`'s own cascade.
        if before.fully_qualified_name != after.fully_qualified_name {
            let old_prefix = before.fully_qualified_name.clone();
            let new_prefix = after.fully_qualified_name.clone();
            let mut descendants: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
            loop {
                let mut grew = false;
                for asset in assets.iter() {
                    let parent_is_self_or_descendant = asset.parent_id == Some(id)
                        || asset.parent_id.is_some_and(|p| descendants.contains(&p));
                    if parent_is_self_or_descendant && !descendants.contains(&asset.id) {
                        descendants.insert(asset.id);
                        grew = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            for asset in assets.iter_mut() {
                if descendants.contains(&asset.id) {
                    asset.fully_qualified_name = format!(
                        "{new_prefix}{}",
                        &asset.fully_qualified_name[old_prefix.len()..]
                    );
                }
            }

            // Phase 3 item 3.8, sequenced after this cascade landing in 3.3 —
            // the same reasoning as the Postgres adapter's own cascade: a
            // column mapping stores its FQNs as plain text, so a rename has
            // to reach it explicitly or it keeps citing an FQN that moved.
            let renames_column_fqn = |fqn: &str| -> Option<String> {
                if fqn == old_prefix {
                    Some(new_prefix.clone())
                } else if let Some(rest) = fqn.strip_prefix(&format!("{old_prefix}.")) {
                    Some(format!("{new_prefix}.{rest}"))
                } else {
                    None
                }
            };
            for (_, mapping) in self.column_mappings.lock().unwrap().iter_mut() {
                if let Some(renamed) = renames_column_fqn(&mapping.from_column_fqn) {
                    mapping.from_column_fqn = renamed;
                }
                if let Some(renamed) = renames_column_fqn(&mapping.to_column_fqn) {
                    mapping.to_column_fqn = renamed;
                }
            }
        }

        let existing = assets.iter_mut().find(|a| a.id == id).expect("just read");
        *existing = after.clone();
        self.versions.lock().unwrap().push(AssetVersion {
            version: after.version,
            snapshot: after.clone(),
            change_description: Some(diff),
            updated_by: updated_by.to_string(),
            updated_at: after.updated_at,
        });
        Ok(UpdateOutcome::Updated(Box::new(after)))
    }

    async fn bump_version(
        &self,
        id: Uuid,
        next: EntityVersion,
        change_description: graph_owl_core::envelope::ChangeDescription,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError> {
        self.guard_write("bump_version");
        let mut assets = self.assets.lock().unwrap();
        let Some(existing) = assets.iter_mut().find(|a| a.id == id) else {
            return Ok(None);
        };
        existing.version = next;
        existing.updated_by = updated_by.to_string();
        existing.change_description = Some(change_description.clone());
        existing.updated_at = Utc::now();
        let after = existing.clone();
        self.versions.lock().unwrap().push(AssetVersion {
            version: after.version,
            snapshot: after.clone(),
            change_description: Some(change_description),
            updated_by: updated_by.to_string(),
            updated_at: after.updated_at,
        });
        Ok(Some(after))
    }

    async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, StorageError> {
        let mut versions: Vec<AssetVersion> = self
            .versions
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.snapshot.id == id)
            .cloned()
            .collect();
        versions.sort_by_key(|version| std::cmp::Reverse(version.version));
        Ok(versions)
    }

    async fn soft_delete_asset(&self, id: Uuid, deleted_by: &str) -> Result<u64, StorageError> {
        self.guard_write("soft_delete_asset");
        let mut assets = self.assets.lock().unwrap();
        let subtree = descendants(&assets, id);
        let mut affected = 0;
        for asset in assets
            .iter_mut()
            .filter(|a| subtree.contains(&a.id) && !a.deleted)
        {
            asset.deleted = true;
            asset.deleted_at = Some(Utc::now());
            asset.updated_by = deleted_by.to_string();
            affected += 1;
        }
        Ok(affected)
    }

    async fn restore_asset(&self, id: Uuid, restored_by: &str) -> Result<u64, StorageError> {
        let mut assets = self.assets.lock().unwrap();
        let subtree = descendants(&assets, id);
        let mut affected = 0;
        for asset in assets
            .iter_mut()
            .filter(|a| subtree.contains(&a.id) && a.deleted)
        {
            asset.deleted = false;
            asset.deleted_at = None;
            asset.updated_by = restored_by.to_string();
            affected += 1;
        }
        Ok(affected)
    }

    // The fake applies the *same* AccessPredicate::admits used by the real
    // adapter's reference semantics, so a lowering bug shows as a
    // disagreement rather than passing here and failing in Postgres.
    async fn find_user(&self, id: &str) -> Result<Option<StoredUser>, StorageError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|u| u.id == id)
            .cloned())
    }

    async fn upsert_user(&self, user: &StoredUser) -> Result<(), StorageError> {
        let mut users = self.users.lock().unwrap();
        if let Some(existing) = users.iter_mut().find(|u| u.id == user.id) {
            *existing = user.clone();
        } else {
            users.push(user.clone());
        }
        Ok(())
    }

    /// **Honours `roles`**, because the port does.
    ///
    /// The real adapter joins `role_policies` on the roles it was given, so
    /// a principal holding no matching role gets no policy. A double that
    /// returned every policy regardless made role-scoped authorization
    /// unobservable in this crate — a decision cache keyed on the wrong
    /// thing would have passed every test here.
    ///
    /// A policy is attached to the role of the same name. That is the
    /// fake's convention, not the schema's, and it is enough to model the
    /// one property that matters: different roles resolve different
    /// policies.
    async fn create_lineage_edge(
        &self,
        edge: &graph_owl_core::lineage::LineageEdge,
    ) -> Result<(), StorageError> {
        let mut edges = self.lineage.lock().unwrap();
        // Honours the port's uniqueness: `(from, to, relationship, source)`,
        // not the triple. A double unique on the triple alone would make the
        // "two sources coexist" test pass for the wrong reason.
        if edges.iter().any(|existing| {
            existing.from_asset_id == edge.from_asset_id
                && existing.to_asset_id == edge.to_asset_id
                && existing.relationship == edge.relationship
                && existing.details.source == edge.details.source
        }) {
            return Err(StorageError::Conflict {
                detail: "that source has already asserted this edge".to_string(),
                existing_id: None,
                kind: ConflictKind::Fqn,
            });
        }
        edges.push(edge.clone());
        Ok(())
    }

    /// Returns what was deleted, as the port specifies — the caller needs
    /// the endpoints to withdraw the matching triple from the graph.
    async fn delete_lineage_edge(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::lineage::LineageEdge>, StorageError> {
        let mut edges = self.lineage.lock().unwrap();
        let removed = edges.iter().find(|edge| edge.id == id).cloned();
        edges.retain(|edge| edge.id != id);
        Ok(removed)
    }

    async fn lineage_edges_touching(
        &self,
        asset_ids: &[Uuid],
        limit: Option<i64>,
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError> {
        let matched = self
            .lineage
            .lock()
            .unwrap()
            .iter()
            .filter(|edge| {
                asset_ids.contains(&edge.from_asset_id) || asset_ids.contains(&edge.to_asset_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        Ok(match limit {
            Some(limit) => {
                let limit = usize::try_from(limit).unwrap_or(usize::MAX);
                matched.into_iter().take(limit).collect()
            }
            None => matched,
        })
    }

    async fn lineage_edges_by_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError> {
        Ok(self
            .lineage
            .lock()
            .unwrap()
            .iter()
            .filter(|edge| edge.details.pipeline == Some(pipeline_id))
            .cloned()
            .collect())
    }

    async fn begin_run(&self, run: &graph_owl_storage::ConnectorRun) -> Result<(), StorageError> {
        self.runs.lock().unwrap().push(run.clone());
        Ok(())
    }

    /// Replaces the open row rather than appending a second one — a run is
    /// one row that gains an ending, not two rows that must be correlated.
    async fn finish_run(&self, run: &graph_owl_storage::ConnectorRun) -> Result<(), StorageError> {
        let mut runs = self.runs.lock().unwrap();
        if let Some(open) = runs.iter_mut().find(|r| r.id == run.id) {
            *open = run.clone();
        }
        Ok(())
    }

    async fn recent_runs(
        &self,
        service_name: &str,
        limit: usize,
    ) -> Result<Vec<graph_owl_storage::ConnectorRun>, StorageError> {
        let mut runs: Vec<_> = self
            .runs
            .lock()
            .unwrap()
            .iter()
            .filter(|r| service_name.is_empty() || r.service_name == service_name)
            .cloned()
            .collect();
        runs.sort_by_key(|run| std::cmp::Reverse(run.started_at));
        runs.truncate(limit);
        Ok(runs)
    }

    /// Honours the port's distinction between the two absences: an FQN
    /// missing from the map does not exist, and one present with `None`
    /// exists without a fingerprint. A double that returned an empty map
    /// for both would make every re-run look like a first run.
    async fn source_hashes(
        &self,
        fqns: &[String],
    ) -> Result<std::collections::HashMap<String, Option<Vec<u8>>>, StorageError> {
        Ok(self
            .assets
            .lock()
            .unwrap()
            .iter()
            .filter(|asset| !asset.deleted)
            .filter(|asset| fqns.contains(&asset.fully_qualified_name))
            .map(|asset| {
                (
                    asset.fully_qualified_name.clone(),
                    self.source_hashes.lock().unwrap().get(&asset.id).cloned(),
                )
            })
            .collect())
    }

    async fn set_source_hash(&self, id: Uuid, hash: &[u8]) -> Result<(), StorageError> {
        self.source_hashes.lock().unwrap().insert(id, hash.to_vec());
        Ok(())
    }

    async fn upsert_policy(&self, policy: &Policy, roles: &[String]) -> Result<(), StorageError> {
        let mut policies = self.policies.lock().unwrap();
        if let Some(existing) = policies.iter_mut().find(|p| p.name == policy.name) {
            *existing = policy.clone();
        } else {
            policies.push(policy.clone());
        }
        drop(policies);

        let mut attachments = self.role_policies.lock().unwrap();
        attachments.retain(|(_, p)| p != &policy.name);
        attachments.extend(roles.iter().map(|role| (role.clone(), policy.name.clone())));
        Ok(())
    }

    async fn list_policies(&self) -> Result<Vec<(Policy, Vec<String>)>, StorageError> {
        let attachments = self.role_policies.lock().unwrap();
        Ok(self
            .policies
            .lock()
            .unwrap()
            .iter()
            .map(|policy| {
                let roles = attachments
                    .iter()
                    .filter(|(_, p)| p == &policy.name)
                    .map(|(role, _)| role.clone())
                    .collect();
                (policy.clone(), roles)
            })
            .collect())
    }

    async fn delete_policy(&self, name: &str) -> Result<bool, StorageError> {
        let mut policies = self.policies.lock().unwrap();
        let before = policies.len();
        policies.retain(|p| p.name != name);
        let removed = policies.len() != before;
        drop(policies);
        self.role_policies
            .lock()
            .unwrap()
            .retain(|(_, p)| p != name);
        Ok(removed)
    }

    async fn policies_for_roles(&self, roles: &[String]) -> Result<Vec<Policy>, StorageError> {
        self.policy_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Epic 34 Slice F found this checking `roles.contains(&policy.name)`
        // — a policy's own name treated as if it were a role — which only
        // ever worked by coincidence, when a test happened to give a policy
        // and the role that carries it the same name. The real join is
        // through `role_policies`, the same attachment table
        // `upsert_policy`/`list_policies`/`delete_policy` already maintain,
        // matching `graph-owl-storage-postgres`'s `policies p JOIN
        // role_policies rp ON rp.policy = p.name WHERE rp.role = ANY($1)`.
        let attached: std::collections::HashSet<String> = self
            .role_policies
            .lock()
            .unwrap()
            .iter()
            .filter(|(role, _)| roles.contains(role))
            .map(|(_, policy)| policy.clone())
            .collect();
        Ok(self
            .policies
            .lock()
            .unwrap()
            .iter()
            .filter(|policy| attached.contains(&policy.name))
            .cloned()
            .collect())
    }

    async fn list_assets_visible(
        &self,
        filter: &graph_owl_storage::AssetFilter<'_>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<Asset>, StorageError> {
        let owner = filter.owner;
        let all = self.list_assets(filter.kind, page).await?;
        // **Effective ownership, walked, as the port specifies.** A double
        // that ignored `owner` — or matched only direct ownership — would
        // pass every facade test while the real adapter did something else,
        // which is the failure mode this project has hit four times.
        let effective = |asset: &Asset| -> Vec<String> {
            let assets = self.assets.lock().expect("lock");
            let owners = self.owners.lock().expect("lock");
            let mut node = Some(asset.id);
            while let Some(current) = node {
                if let Some((_, found)) = owners.iter().find(|(id, _)| *id == current)
                    && !found.is_empty()
                {
                    // Stops at the nearest owned ancestor rather than
                    // accumulating up the chain: "who do I ask" has one
                    // answer.
                    return found.iter().map(|o| o.id.clone()).collect();
                }
                node = assets
                    .iter()
                    .find(|candidate| candidate.id == current)
                    .and_then(|candidate| candidate.parent_id);
            }
            Vec::new()
        };
        let visible: Vec<Asset> = all
            .data
            .into_iter()
            .filter(|a| predicate.admits(&a.fully_qualified_name))
            .filter(|a| match owner {
                // Absent means unfiltered, not match-nothing.
                None => true,
                Some(wanted) => effective(a).iter().any(|id| id == wanted),
            })
            .filter(|a| admits_extension(a, filter.extension))
            .collect();
        Ok(Page::from_overfetch(visible, page.limit, |a: &Asset| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        }))
    }

    async fn search_assets_visible(
        &self,
        query: &str,
        filter: &graph_owl_storage::AssetFilter<'_>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<graph_owl_storage::SearchHit>, StorageError> {
        let all = self.search_assets(query, filter.kind, page).await?;
        // `snippet: None` throughout — this fake has no text-search engine to
        // excerpt from, the same reason it already leaves `domain`/
        // `data_product`/`lifecycle` unfiltered rather than reimplementing
        // Postgres's own query planner in-process.
        let visible: Vec<graph_owl_storage::SearchHit> = all
            .data
            .into_iter()
            .filter(|a| predicate.admits(&a.fully_qualified_name))
            .filter(|a| admits_extension(a, filter.extension))
            .map(|asset| graph_owl_storage::SearchHit {
                asset,
                snippet: None,
            })
            .collect();
        Ok(Page::from_overfetch(visible, page.limit, |h| {
            Cursor::new(h.asset.fully_qualified_name.clone(), h.asset.id)
        }))
    }

    async fn list_children_visible(
        &self,
        parent_id: Option<Uuid>,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        Ok(self
            .list_children(parent_id)
            .await?
            .into_iter()
            .filter(|a| predicate.admits(&a.fully_qualified_name))
            .collect())
    }

    async fn count_documented_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<(i64, i64), StorageError> {
        let assets = self.assets.lock().unwrap();
        let visible: Vec<_> = assets
            .iter()
            .filter(|a| !a.deleted && predicate.admits(&a.fully_qualified_name))
            .collect();
        let described = visible
            .iter()
            .filter(|a| {
                a.description
                    .as_deref()
                    .is_some_and(|d| !d.trim().is_empty())
            })
            .count();
        Ok((
            i64::try_from(described).unwrap_or(i64::MAX),
            i64::try_from(visible.len()).unwrap_or(i64::MAX),
        ))
    }

    async fn recently_changed_visible(
        &self,
        limit: i64,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        let assets = self.assets.lock().unwrap();
        let mut visible: Vec<Asset> = assets
            .iter()
            .filter(|a| !a.deleted && predicate.admits(&a.fully_qualified_name))
            .cloned()
            .collect();
        visible.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        visible.truncate(usize::try_from(limit).unwrap_or(0));
        Ok(visible)
    }

    async fn count_assets_by_kind_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<Vec<(AssetKind, i64)>, StorageError> {
        let assets = self.assets.lock().unwrap();
        Ok(AssetKind::ALL
            .into_iter()
            .map(|kind| {
                let n = assets
                    .iter()
                    .filter(|a| a.kind == kind && predicate.admits(&a.fully_qualified_name))
                    .count();
                (kind, i64::try_from(n).unwrap_or(i64::MAX))
            })
            .filter(|(_, n)| *n > 0)
            .collect())
    }

    async fn insert_table(&self, table: Table) -> Result<Table, StorageError> {
        self.inserted.lock().unwrap().push(table.clone());
        Ok(table)
    }

    async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError> {
        Ok(self
            .inserted
            .lock()
            .unwrap()
            .iter()
            .find(|table| table.id == id)
            .cloned())
    }

    async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
        // The fake honours the same ordering and keyset contract as the
        // Postgres adapter. A fake that returns insertion order would let
        // a pagination bug pass here and fail only against a real database,
        // which is the whole failure mode a port is supposed to prevent.
        let mut tables = self.inserted.lock().unwrap().clone();
        tables.sort_by(|a, b| {
            a.fully_qualified_name
                .cmp(&b.fully_qualified_name)
                .then(a.id.cmp(&b.id))
        });
        if let Some(cursor) = &page.after {
            tables.retain(|t| {
                (t.fully_qualified_name.as_str(), t.id) > (cursor.sort_key.as_str(), cursor.id)
            });
        }
        tables.truncate(page.limit + 1);
        Ok(Page::from_overfetch(tables, page.limit, |t| {
            Cursor::new(t.fully_qualified_name.clone(), t.id)
        }))
    }

    async fn update_table(
        &self,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, StorageError> {
        let mut inserted = self.inserted.lock().unwrap();
        let Some(table) = inserted.iter_mut().find(|table| table.id == id) else {
            return Ok(None);
        };
        if let Some(name) = update.name {
            table.name = name;
        }
        if let Some(description) = update.description {
            table.description = Some(description);
        }
        table.updated_at = Utc::now();
        Ok(Some(table.clone()))
    }

    async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut inserted = self.inserted.lock().unwrap();
        let original_len = inserted.len();
        inserted.retain(|table| table.id != id);
        Ok(inserted.len() != original_len)
    }

    async fn create_relationship(
        &self,
        relationship: Relationship,
    ) -> Result<Relationship, StorageError> {
        self.relationships
            .lock()
            .unwrap()
            .push(relationship.clone());
        Ok(relationship)
    }

    async fn list_relationships_for_entity(
        &self,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<Vec<Relationship>, StorageError> {
        Ok(self
            .relationships
            .lock()
            .unwrap()
            .iter()
            .filter(|relationship| {
                (relationship.from_entity_type == entity_type
                    && relationship.from_entity_id == entity_id)
                    || (relationship.to_entity_type == entity_type
                        && relationship.to_entity_id == entity_id)
            })
            .cloned()
            .collect())
    }

    async fn get_relationship(&self, id: Uuid) -> Result<Option<Relationship>, StorageError> {
        let relationships = self.relationships.lock().unwrap();
        Ok(relationships.iter().find(|r| r.id == id).cloned())
    }

    async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut relationships = self.relationships.lock().unwrap();
        let original_len = relationships.len();
        relationships.retain(|relationship| relationship.id != id);
        Ok(relationships.len() != original_len)
    }

    async fn list_relationships(
        &self,
        page: &graph_owl_core::page::PageRequest,
    ) -> Result<graph_owl_core::page::Page<Relationship>, StorageError> {
        use graph_owl_core::page::{Cursor, Page};

        let mut relationships: Vec<Relationship> =
            self.relationships.lock().unwrap().iter().cloned().collect();
        relationships.sort_by_key(|a| a.id.to_string());
        if let Some(cursor) = &page.after {
            relationships.retain(|r| r.id.to_string().as_str() > cursor.sort_key.as_str());
        }
        relationships.truncate(page.limit + 1);
        Ok(Page::from_overfetch(relationships, page.limit, |r| {
            Cursor::new(r.id.to_string(), r.id)
        }))
    }

    // ---- Epic 24 Slice A: glossary and terms ----

    async fn insert_glossary(
        &self,
        glossary: graph_owl_storage::Glossary,
    ) -> Result<graph_owl_storage::Glossary, StorageError> {
        let mut held = self.glossaries.lock().unwrap();
        if held
            .iter()
            .any(|g| g.fully_qualified_name == glossary.fully_qualified_name)
        {
            return Err(StorageError::Conflict {
                detail: glossary.fully_qualified_name.clone(),
                existing_id: None,
                kind: ConflictKind::Fqn,
            });
        }
        held.push(glossary.clone());
        Ok(glossary)
    }

    async fn get_glossary(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::Glossary>, StorageError> {
        Ok(self
            .glossaries
            .lock()
            .unwrap()
            .iter()
            .find(|g| g.id == id)
            .cloned())
    }

    async fn list_glossaries(&self) -> Result<Vec<graph_owl_storage::Glossary>, StorageError> {
        let mut glossaries = self.glossaries.lock().unwrap().clone();
        glossaries.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        Ok(glossaries)
    }

    async fn delete_glossary(
        &self,
        id: Uuid,
        recursive: bool,
    ) -> Result<graph_owl_storage::GlossaryDeletion, StorageError> {
        let mut glossaries = self.glossaries.lock().unwrap();
        if !glossaries.iter().any(|g| g.id == id) {
            return Ok(graph_owl_storage::GlossaryDeletion::NotFound);
        }
        let mut terms = self.glossary_terms.lock().unwrap();
        let term_count =
            i64::try_from(terms.iter().filter(|t| t.glossary_id == id).count()).unwrap_or(i64::MAX);
        if term_count > 0 && !recursive {
            return Ok(graph_owl_storage::GlossaryDeletion::HasTerms { term_count });
        }
        terms.retain(|t| t.glossary_id != id);
        glossaries.retain(|g| g.id != id);
        Ok(graph_owl_storage::GlossaryDeletion::Deleted)
    }

    async fn insert_term(
        &self,
        term: graph_owl_storage::GlossaryTermRecord,
    ) -> Result<graph_owl_storage::GlossaryTermRecord, StorageError> {
        let mut held = self.glossary_terms.lock().unwrap();
        if held
            .iter()
            .any(|t| t.fully_qualified_name == term.fully_qualified_name)
        {
            return Err(StorageError::Conflict {
                detail: term.fully_qualified_name.clone(),
                existing_id: None,
                kind: ConflictKind::Fqn,
            });
        }
        held.push(term.clone());
        Ok(term)
    }

    async fn get_term(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        Ok(self
            .glossary_terms
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn list_terms(
        &self,
        glossary_id: Uuid,
    ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let mut terms: Vec<_> = self
            .glossary_terms
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.glossary_id == glossary_id)
            .cloned()
            .collect();
        terms.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(terms)
    }

    async fn update_term(
        &self,
        id: Uuid,
        update: graph_owl_storage::GlossaryTermUpdate,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let mut terms = self.glossary_terms.lock().unwrap();
        let Some(term) = terms.iter_mut().find(|t| t.id == id) else {
            return Ok(None);
        };
        if let Some(definition) = update.definition {
            term.definition = definition;
        }
        if let Some(synonyms) = update.synonyms {
            term.synonyms = synonyms;
        }
        if let Some(abbreviations) = update.abbreviations {
            term.abbreviations = abbreviations;
        }
        term.updated_at = Utc::now();
        Ok(Some(term.clone()))
    }

    async fn delete_term(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut terms = self.glossary_terms.lock().unwrap();
        let original_len = terms.len();
        terms.retain(|t| t.id != id);
        Ok(terms.len() != original_len)
    }

    async fn search_terms(
        &self,
        query: &str,
    ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        // A substring match over the same fields the migration's
        // `search_vector` indexes. Not ranked the way Postgres's
        // `ts_rank_cd` is — this fake only has to prove the facade wires
        // the query through, not reproduce full-text relevance.
        let needle = query.to_lowercase();
        Ok(self
            .glossary_terms
            .lock()
            .unwrap()
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&needle)
                    || t.definition.to_lowercase().contains(&needle)
                    || t.synonyms
                        .iter()
                        .any(|s| s.to_lowercase().contains(&needle))
                    || t.abbreviations
                        .iter()
                        .any(|a| a.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect())
    }

    // ---- Epic 24 Slice B: SKOS relations ----

    async fn insert_term_relation(
        &self,
        term_id: Uuid,
        relation: graph_owl_core::glossary::SkosRelation,
    ) -> Result<(), StorageError> {
        let mut held = self.term_relations.lock().unwrap();
        if !held.iter().any(|(id, r)| *id == term_id && *r == relation) {
            held.push((term_id, relation));
        }
        Ok(())
    }

    async fn delete_term_relation(
        &self,
        term_id: Uuid,
        relation: &graph_owl_core::glossary::SkosRelation,
    ) -> Result<bool, StorageError> {
        let mut held = self.term_relations.lock().unwrap();
        let original_len = held.len();
        held.retain(|(id, r)| !(*id == term_id && r == relation));
        Ok(held.len() != original_len)
    }

    async fn term_relations_touching(
        &self,
        term_id: Uuid,
    ) -> Result<Vec<(String, graph_owl_core::glossary::SkosRelation)>, StorageError> {
        use graph_owl_core::glossary::SkosRelation;
        let id_text = term_id.to_string();
        Ok(self
            .term_relations
            .lock()
            .unwrap()
            .iter()
            .filter(|(owner, relation)| *owner == term_id || relation.target() == id_text)
            .map(|(owner, relation): &(Uuid, SkosRelation)| (owner.to_string(), relation.clone()))
            .collect())
    }

    async fn broader_edges(&self) -> Result<Vec<(String, String)>, StorageError> {
        use graph_owl_core::glossary::SkosRelation;
        Ok(self
            .term_relations
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(owner, relation)| match relation {
                SkosRelation::Broader(target) => Some((owner.to_string(), target.clone())),
                _ => None,
            })
            .collect())
    }

    // ---- Epic 24 Slice C: review workflow ----

    async fn set_term_reviewers(
        &self,
        term_id: Uuid,
        reviewers: &[String],
    ) -> Result<(), StorageError> {
        let mut held = self.term_reviewers.lock().unwrap();
        held.retain(|(id, _)| *id != term_id);
        held.push((term_id, reviewers.to_vec()));
        Ok(())
    }

    async fn term_reviewers(&self, term_id: Uuid) -> Result<Vec<String>, StorageError> {
        Ok(self
            .term_reviewers
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| *id == term_id)
            .map(|(_, reviewers)| reviewers.clone())
            .unwrap_or_default())
    }

    async fn transition_term(
        &self,
        term_id: Uuid,
        _from: graph_owl_core::glossary::TermStatus,
        to: graph_owl_core::glossary::TermStatus,
        _actor: &str,
        _reason: Option<String>,
        _successor_term_id: Option<Uuid>,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let mut terms = self.glossary_terms.lock().unwrap();
        let Some(term) = terms.iter_mut().find(|t| t.id == term_id) else {
            return Ok(None);
        };
        term.status = to;
        term.version.minor += 1;
        term.updated_at = Utc::now();
        Ok(Some(term.clone()))
    }

    // ---- Epic 24 Slice D: terms attach to assets and columns ----

    async fn attach_term(
        &self,
        term_id: Uuid,
        target_fqn: &str,
        _attached_by: &str,
    ) -> Result<(), StorageError> {
        let mut held = self.term_attachments.lock().unwrap();
        if !held
            .iter()
            .any(|(id, fqn)| *id == term_id && fqn == target_fqn)
        {
            held.push((term_id, target_fqn.to_string()));
        }
        Ok(())
    }

    async fn detach_term(&self, term_id: Uuid, target_fqn: &str) -> Result<bool, StorageError> {
        let mut held = self.term_attachments.lock().unwrap();
        let original_len = held.len();
        held.retain(|(id, fqn)| !(*id == term_id && fqn == target_fqn));
        Ok(held.len() != original_len)
    }

    async fn term_usage(
        &self,
        term_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<String>, StorageError> {
        let mut fqns: Vec<String> = self
            .term_attachments
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| *id == term_id)
            .map(|(_, fqn)| fqn.clone())
            .collect();
        fqns.sort();
        if let Some(cursor) = &page.after {
            fqns.retain(|fqn| fqn.as_str() > cursor.sort_key.as_str());
        }
        fqns.truncate(page.limit + 1);
        Ok(Page::from_overfetch(fqns, page.limit, |fqn: &String| {
            Cursor::new(fqn.clone(), term_id)
        }))
    }

    // ---- Epic 24 Slice E: Metric as a first-class entity ----

    async fn insert_metric(
        &self,
        metric: graph_owl_storage::MetricRecord,
    ) -> Result<graph_owl_storage::MetricRecord, StorageError> {
        let mut held = self.metrics.lock().unwrap();
        if held
            .iter()
            .any(|m| m.fully_qualified_name == metric.fully_qualified_name)
        {
            return Err(StorageError::Conflict {
                detail: metric.fully_qualified_name.clone(),
                existing_id: None,
                kind: ConflictKind::Fqn,
            });
        }
        held.push(metric.clone());
        Ok(metric)
    }

    async fn get_metric(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
        Ok(self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.id == id)
            .cloned())
    }

    async fn list_metrics(
        &self,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_storage::MetricRecord>, StorageError> {
        let mut metrics = self.metrics.lock().unwrap().clone();
        metrics.sort_by(|a, b| {
            a.fully_qualified_name
                .cmp(&b.fully_qualified_name)
                .then(a.id.cmp(&b.id))
        });
        if let Some(cursor) = &page.after {
            metrics.retain(|m| {
                (m.fully_qualified_name.as_str(), m.id) > (cursor.sort_key.as_str(), cursor.id)
            });
        }
        metrics.truncate(page.limit + 1);
        Ok(Page::from_overfetch(metrics, page.limit, |m| {
            Cursor::new(m.fully_qualified_name.clone(), m.id)
        }))
    }

    async fn update_metric(
        &self,
        id: Uuid,
        update: graph_owl_storage::MetricUpdate,
    ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
        let mut metrics = self.metrics.lock().unwrap();
        let Some(metric) = metrics.iter_mut().find(|m| m.id == id) else {
            return Ok(None);
        };
        if let Some(definition) = update.definition {
            metric.definition = definition;
        }
        if let Some(formula) = update.formula {
            metric.formula = Some(formula);
        }
        if let Some(unit) = update.unit {
            metric.unit = Some(unit);
        }
        if let Some(granularity) = update.granularity {
            metric.granularity = Some(granularity);
        }
        if let Some(calculation_type) = update.calculation_type {
            metric.calculation_type = calculation_type;
        }
        metric.updated_at = Utc::now();
        Ok(Some(metric.clone()))
    }

    async fn delete_metric(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut metrics = self.metrics.lock().unwrap();
        let original_len = metrics.len();
        metrics.retain(|m| m.id != id);
        Ok(metrics.len() != original_len)
    }

    async fn search_metrics(
        &self,
        query: &str,
    ) -> Result<Vec<graph_owl_storage::MetricRecord>, StorageError> {
        let needle = query.to_lowercase();
        let terms = self.glossary_terms.lock().unwrap();
        Ok(self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .filter(|m| {
                m.name.to_lowercase().contains(&needle)
                    || m.definition.to_lowercase().contains(&needle)
                    || m.defined_by.is_some_and(|term_id| {
                        terms
                            .iter()
                            .any(|t| t.id == term_id && t.name.to_lowercase().contains(&needle))
                    })
            })
            .cloned()
            .collect())
    }

    // ---- Epic 24 Slice F: metric lineage reconciliation ----

    async fn update_metric_sources(
        &self,
        metric_id: Uuid,
        sources: &[String],
    ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
        let mut metrics = self.metrics.lock().unwrap();
        let Some(metric) = metrics.iter_mut().find(|m| m.id == metric_id) else {
            return Ok(None);
        };
        metric.source_assets = sources.to_vec();
        metric.updated_at = Utc::now();
        Ok(Some(metric.clone()))
    }

    // ---- Epic 21, and again as strict as the port ----
    //
    // In particular the cascade is real here: deleting a run drops its
    // claims and discards, because "a bad run is deletable wholesale" is
    // the property the whole named-graph decision buys, and a double that
    // left orphans behind would let a broken cascade pass every test that
    // does not use Postgres.

    async fn find_extraction_run(
        &self,
        source_id: &str,
        fingerprint: &str,
        extractor: &str,
        version: &str,
    ) -> Result<Option<graph_owl_storage::ExtractionRunRecord>, StorageError> {
        let runs = self.extraction_runs.lock().unwrap();
        Ok(runs
            .iter()
            .find(|run| {
                run.source_id == source_id
                    && run.source_fingerprint == fingerprint
                    && run.extractor == extractor
                    && run.extractor_version == version
            })
            .cloned())
    }

    // ---- Epic 22, and as strict as the port ----
    //
    // Uniqueness is scoped to the entity type here too. A double with
    // *global* uniqueness would refuse a definition the real index accepts,
    // so decision 2 would look enforced in unit tests and be untested where
    // it actually lives.

    async fn define_custom_property(
        &self,
        id: Uuid,
        property: &graph_owl_core::custom_property::CustomProperty,
    ) -> Result<(), StorageError> {
        self.guard_write("define_custom_property");
        let mut held = self.custom_properties.lock().unwrap();
        if held.iter().any(|(_, existing)| {
            existing.name == property.name && existing.entity_type == property.entity_type
        }) {
            return Err(StorageError::Conflict {
                detail: format!(
                    "`{}` is already defined on `{}`",
                    property.name, property.entity_type
                ),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::CustomPropertyExists,
            });
        }
        held.push((id, property.clone()));
        Ok(())
    }

    async fn list_custom_properties(
        &self,
        entity_type: Option<&str>,
    ) -> Result<Vec<(Uuid, graph_owl_core::custom_property::CustomProperty)>, StorageError> {
        let held = self.custom_properties.lock().unwrap();
        Ok(held
            .iter()
            .filter(|(_, property)| entity_type.is_none_or(|wanted| property.entity_type == wanted))
            .cloned()
            .collect())
    }

    async fn get_custom_property(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::custom_property::CustomProperty>, StorageError> {
        let held = self.custom_properties.lock().unwrap();
        Ok(held
            .iter()
            .find(|(held_id, _)| *held_id == id)
            .map(|(_, property)| property.clone()))
    }

    async fn count_custom_property_values(
        &self,
        entity_type: &str,
        name: &str,
    ) -> Result<i64, StorageError> {
        let assets = self.assets.lock().expect("lock");
        // A property explicitly set to null has been *cleared*, so it does
        // not count — counting it would refuse a delete over values nobody
        // holds, which is the kind of guard that teaches people to force.
        let count = assets
            .iter()
            .filter(|asset| {
                asset.kind.as_str() == entity_type
                    && asset
                        .extension
                        .as_ref()
                        .and_then(|bag| bag.get(name))
                        .is_some_and(|value| !value.is_null())
            })
            .count();
        Ok(i64::try_from(count).unwrap_or(i64::MAX))
    }

    async fn delete_custom_property(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut held = self.custom_properties.lock().unwrap();
        let before = held.len();
        held.retain(|(held_id, _)| *held_id != id);
        Ok(held.len() < before)
    }

    // ---- Epics 29 (D, E) and 30, and as strict as the port ----

    async fn create_test_definition(
        &self,
        id: Uuid,
        name: &str,
        test_type: &str,
        description: Option<&str>,
        expected_cadence: Option<&str>,
    ) -> Result<graph_owl_storage::StoredTestDefinition, StorageError> {
        self.guard_write("create_test_definition");
        let mut held = self.test_definitions.lock().unwrap();
        if held.iter().any(|d| d.name == name) {
            return Err(StorageError::Conflict {
                detail: format!("a test definition named `{name}` already exists"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let created = graph_owl_storage::StoredTestDefinition {
            id,
            name: name.to_string(),
            test_type: test_type.to_string(),
            description: description.map(str::to_string),
            expected_cadence: expected_cadence.map(str::to_string),
        };
        held.push(created.clone());
        Ok(created)
    }

    async fn list_test_definitions(
        &self,
    ) -> Result<Vec<graph_owl_storage::StoredTestDefinition>, StorageError> {
        Ok(self.test_definitions.lock().unwrap().clone())
    }

    async fn set_definition_cadence(
        &self,
        id: Uuid,
        expected_cadence: Option<&str>,
    ) -> Result<Option<i64>, StorageError> {
        self.guard_write("set_definition_cadence");
        {
            let mut held = self.test_definitions.lock().unwrap();
            let Some(definition) = held.iter_mut().find(|d| d.id == id) else {
                return Ok(None);
            };
            definition.expected_cadence = expected_cadence.map(str::to_string);
        }
        // Only the cases that *inherit*. One with its own cadence said
        // something different on purpose, and counting it would report a
        // change that did not happen.
        let cases = self.test_cases.lock().unwrap();
        Ok(Some(
            i64::try_from(cases.iter().filter(|c| c.definition_id == Some(id)).count())
                .unwrap_or(i64::MAX),
        ))
    }

    async fn create_test_suite(
        &self,
        id: Uuid,
        name: &str,
        owner: Option<&str>,
        description: Option<&str>,
    ) -> Result<Option<Uuid>, StorageError> {
        self.guard_write("create_test_suite");
        let _ = description;
        if let Some(owner) = owner {
            let teams = self.teams.lock().unwrap();
            if !teams.iter().any(|t| t.id == owner) {
                return Ok(None);
            }
        }
        let mut held = self.test_suites.lock().unwrap();
        if held.iter().any(|(_, existing, _)| existing == name) {
            return Err(StorageError::Conflict {
                detail: format!("a test suite named `{name}` already exists"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        held.push((id, name.to_string(), owner.map(str::to_string)));
        Ok(Some(id))
    }

    async fn create_test_case(
        &self,
        id: Uuid,
        name: &str,
        target_fqn: &str,
        test_type: &str,
        description: Option<&str>,
        definition_id: Option<Uuid>,
        suite_id: Option<Uuid>,
        expected_cadence: Option<&str>,
    ) -> Result<Option<graph_owl_storage::StoredTestCase>, StorageError> {
        self.guard_write("create_test_case");
        {
            let assets = self.assets.lock().expect("lock");
            if !assets
                .iter()
                .any(|a| a.fully_qualified_name == target_fqn && !a.deleted)
            {
                return Ok(None);
            }
        }
        let inherited = match definition_id {
            None => None,
            Some(definition) => {
                let held = self.test_definitions.lock().unwrap();
                match held.iter().find(|d| d.id == definition) {
                    None => return Ok(None),
                    Some(found) => found.expected_cadence.clone(),
                }
            }
        };
        if let Some(suite) = suite_id {
            let held = self.test_suites.lock().unwrap();
            if !held.iter().any(|(existing, _, _)| *existing == suite) {
                return Ok(None);
            }
        }

        let mut cases = self.test_cases.lock().unwrap();
        if cases
            .iter()
            .any(|c| c.target_fqn == target_fqn && c.name == name)
        {
            return Err(StorageError::Conflict {
                detail: format!("`{name}` is already a test case on `{target_fqn}`"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let created = graph_owl_storage::StoredTestCase {
            id,
            name: name.to_string(),
            target_fqn: target_fqn.to_string(),
            test_type: test_type.to_string(),
            description: description.map(str::to_string),
            definition_id,
            suite_id,
            // Resolved here exactly as the SQL `coalesce` does: the case's
            // own cadence wins, the definition's is the fallback.
            expected_cadence: expected_cadence.map(str::to_string).or(inherited),
        };
        cases.push(created.clone());
        Ok(Some(created))
    }

    async fn list_test_cases(
        &self,
        target_fqn: Option<&str>,
        suite_id: Option<Uuid>,
    ) -> Result<Vec<graph_owl_storage::StoredTestCase>, StorageError> {
        let cases = self.test_cases.lock().unwrap();
        Ok(cases
            .iter()
            .filter(|c| target_fqn.is_none_or(|fqn| c.target_fqn == fqn))
            .filter(|c| suite_id.is_none_or(|suite| c.suite_id == Some(suite)))
            .cloned()
            .collect())
    }

    async fn delete_test_case(&self, id: Uuid) -> Result<bool, StorageError> {
        self.guard_write("delete_test_case");
        let mut cases = self.test_cases.lock().unwrap();
        let before = cases.len();
        cases.retain(|c| c.id != id);
        self.test_results
            .lock()
            .unwrap()
            .retain(|r| r.case_id != id);
        Ok(cases.len() < before)
    }

    async fn record_test_results(
        &self,
        batch: &[graph_owl_storage::TestResultWrite],
    ) -> Result<graph_owl_storage::ResultIngest, StorageError> {
        self.guard_write("record_test_results");
        let mut ingest = graph_owl_storage::ResultIngest::default();
        let now = Utc::now();
        let known: Vec<Uuid> = self
            .test_cases
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.id)
            .collect();
        let mut held = self.test_results.lock().unwrap();

        for result in batch {
            if result.observed_at > now {
                ingest.rejected += 1;
                continue;
            }
            if !known.contains(&result.case_id) {
                ingest.unknown_case += 1;
                continue;
            }
            // The same `(case, observed_at)` dedup the unique index
            // enforces — a retried push must not double-count.
            if held
                .iter()
                .any(|r| r.case_id == result.case_id && r.observed_at == result.observed_at)
            {
                ingest.duplicates += 1;
                continue;
            }
            held.push(graph_owl_storage::StoredTestResult {
                id: Uuid::new_v4(),
                case_id: result.case_id,
                status: result.status,
                observed_at: result.observed_at,
                message: result.message.clone(),
                metrics: result.metrics.clone(),
            });
            ingest.accepted += 1;
        }
        // No version bump, deliberately — decision 2.
        Ok(ingest)
    }

    async fn test_results(
        &self,
        case_id: Uuid,
        limit: i64,
    ) -> Result<Vec<graph_owl_storage::StoredTestResult>, StorageError> {
        let held = self.test_results.lock().unwrap();
        let mut found: Vec<graph_owl_storage::StoredTestResult> = held
            .iter()
            .filter(|r| r.case_id == case_id)
            .cloned()
            .collect();
        found.sort_by_key(|b| std::cmp::Reverse(b.observed_at));
        found.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(found)
    }

    async fn latest_results_for(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<graph_owl_core::quality::LatestResult>, StorageError> {
        let cases = self.test_cases.lock().unwrap();
        let results = self.test_results.lock().unwrap();
        Ok(cases
            .iter()
            .filter(|c| c.target_fqn == target_fqn)
            .map(|case| {
                // A case with no results still produces a row — it is a
                // *stale* case, not an absent one, and dropping it would
                // make a declared check that never ran invisible.
                let latest = results
                    .iter()
                    .filter(|r| r.case_id == case.id)
                    .max_by_key(|r| r.observed_at);
                graph_owl_core::quality::LatestResult {
                    case_name: case.name.clone(),
                    status: latest.map(|r| r.status),
                    observed_at: latest.map(|r| r.observed_at),
                    cadence: case
                        .expected_cadence
                        .as_deref()
                        .and_then(|raw| graph_owl_core::quality::parse_cadence(raw).ok()),
                }
            })
            .collect())
    }

    async fn prune_test_results(&self, before: DateTime<Utc>) -> Result<i64, StorageError> {
        self.guard_write("prune_test_results");
        let mut held = self.test_results.lock().unwrap();
        // The latest per case survives regardless of age.
        let mut keep: std::collections::BTreeMap<Uuid, DateTime<Utc>> =
            std::collections::BTreeMap::new();
        for result in held.iter() {
            keep.entry(result.case_id)
                .and_modify(|newest| {
                    if result.observed_at > *newest {
                        *newest = result.observed_at;
                    }
                })
                .or_insert(result.observed_at);
        }
        let count_before = held.len();
        held.retain(|r| r.observed_at >= before || keep.get(&r.case_id) == Some(&r.observed_at));
        Ok(i64::try_from(count_before - held.len()).unwrap_or(i64::MAX))
    }

    async fn set_column_mappings(
        &self,
        edge_id: Uuid,
        mappings: &[graph_owl_storage::ColumnMapping],
    ) -> Result<Option<i64>, StorageError> {
        self.guard_write("set_column_mappings");
        {
            let edges = self.lineage.lock().unwrap();
            if !edges.iter().any(|e| e.id == edge_id) {
                return Ok(None);
            }
        }
        {
            // Every named column has to resolve, as the real adapter
            // requires — a double that skipped it would let the facade's
            // tests pass against mappings nothing can render.
            let assets = self.assets.lock().expect("lock");
            for mapping in mappings {
                for fqn in [&mapping.from_column_fqn, &mapping.to_column_fqn] {
                    if !assets.iter().any(|a| {
                        a.fully_qualified_name == *fqn && a.kind == AssetKind::Column && !a.deleted
                    }) {
                        return Ok(None);
                    }
                }
            }
        }
        let mut held = self.column_mappings.lock().unwrap();
        held.retain(|(edge, _)| *edge != edge_id);
        for mapping in mappings {
            held.push((edge_id, mapping.clone()));
        }
        Ok(Some(i64::try_from(mappings.len()).unwrap_or(i64::MAX)))
    }

    async fn column_mappings(
        &self,
        edge_id: Uuid,
    ) -> Result<Vec<graph_owl_storage::ColumnMapping>, StorageError> {
        Ok(self
            .column_mappings
            .lock()
            .unwrap()
            .iter()
            .filter(|(edge, _)| *edge == edge_id)
            .map(|(_, mapping)| mapping.clone())
            .collect())
    }

    async fn reconcile_lineage(
        &self,
        source: &str,
        scope_prefix: &str,
        asserted: &[(Uuid, Uuid, String)],
        created_by: &str,
    ) -> Result<graph_owl_storage::LineageReconciliation, StorageError> {
        self.guard_write("reconcile_lineage");
        let mut report = graph_owl_storage::LineageReconciliation::default();

        let in_scope: Vec<Uuid> = {
            let assets = self.assets.lock().expect("lock");
            assets
                .iter()
                .filter(|a| {
                    a.fully_qualified_name == scope_prefix
                        || a.fully_qualified_name
                            .starts_with(&format!("{scope_prefix}."))
                })
                .map(|a| a.id)
                .collect()
        };

        let mut edges = self.lineage.lock().unwrap();
        for (from, to, relationship) in asserted {
            let already = edges.iter().any(|e| {
                e.from_asset_id == *from
                    && e.to_asset_id == *to
                    && e.relationship.as_str() == relationship
                    && e.details.source.as_str() == source
            });
            if already {
                continue;
            }
            edges.push(graph_owl_core::lineage::LineageEdge {
                id: Uuid::new_v4(),
                from_asset_id: *from,
                to_asset_id: *to,
                relationship: graph_owl_core::relationship_type::RelationshipType::parse(
                    relationship,
                )
                .unwrap_or(graph_owl_core::relationship_type::RelationshipType::Feeds),
                details: graph_owl_core::lineage::LineageDetails {
                    source: graph_owl_core::lineage::LineageSource::parse(source)
                        .unwrap_or(graph_owl_core::lineage::LineageSource::Connector),
                    query: None,
                    description: None,
                    pipeline: None,
                    openlineage_event_id: None,
                },
                created_at: Utc::now(),
                created_by: created_by.to_string(),
            });
            report.added += 1;
        }

        // **Scoped by source and by prefix.** A manually curated edge is
        // never touched — that is the property the slice exists for, and a
        // double that dropped either half would let a source-blind
        // implementation pass every facade test.
        let before = edges.len();
        edges.retain(|e| {
            let this_source = e.details.source.as_str() == source;
            let in_this_scope = in_scope.contains(&e.from_asset_id);
            let still_asserted = asserted.iter().any(|(from, to, relationship)| {
                e.from_asset_id == *from
                    && e.to_asset_id == *to
                    && e.relationship.as_str() == relationship
            });
            !(this_source && in_this_scope && !still_asserted)
        });
        report.removed = i64::try_from(before - edges.len()).unwrap_or(i64::MAX);
        Ok(report)
    }

    // ---- Epics 27 and 28, and as strict as the port ----
    //
    // The **rollups are derived from the stored observations on read**
    // here, which is deliberately *not* what Postgres does — it accumulates
    // incrementally. That difference is the point: if the two ever disagree
    // the equivalence test catches it, and a double that copied the
    // incremental path would only prove the same code twice.

    async fn create_contract(
        &self,
        id: Uuid,
        contract: &graph_owl_core::contract::Contract,
    ) -> Result<Option<graph_owl_core::contract::Contract>, StorageError> {
        self.guard_write("create_contract");
        // Parties and asset must resolve, exactly as the real adapter
        // requires — a double that skipped it would let the facade's tests
        // pass against contracts nobody is accountable for.
        let teams = self.teams.lock().unwrap();
        if !teams.iter().any(|t| t.id == contract.producer) {
            return Ok(None);
        }
        for consumer in &contract.consumers {
            if !teams.iter().any(|t| t.id == *consumer) {
                return Ok(None);
            }
        }
        drop(teams);
        {
            let assets = self.assets.lock().expect("lock");
            if !assets
                .iter()
                .any(|a| a.fully_qualified_name == contract.asset_fqn && !a.deleted)
            {
                return Ok(None);
            }
        }
        let mut created = contract.clone();
        created.id = id;
        self.contracts.lock().unwrap().push(created.clone());
        Ok(Some(created))
    }

    async fn get_contract(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StoredContract>, StorageError> {
        let contracts = self.contracts.lock().unwrap();
        let Some(contract) = contracts.iter().find(|c| c.id == id).cloned() else {
            return Ok(None);
        };
        let breaches = self
            .contract_breaches
            .lock()
            .unwrap()
            .iter()
            .filter(|b| b.contract_id == id)
            .cloned()
            .collect();
        Ok(Some(graph_owl_storage::StoredContract {
            contract,
            breaches,
        }))
    }

    async fn list_contracts(
        &self,
        asset_fqn: Option<&str>,
    ) -> Result<Vec<graph_owl_core::contract::Contract>, StorageError> {
        let contracts = self.contracts.lock().unwrap();
        Ok(contracts
            .iter()
            .filter(|c| asset_fqn.is_none_or(|fqn| c.asset_fqn == fqn))
            .cloned()
            .collect())
    }

    async fn set_contract_status(
        &self,
        id: Uuid,
        status: graph_owl_core::contract::ContractStatus,
        updated_by: &str,
    ) -> Result<bool, StorageError> {
        self.guard_write("set_contract_status");
        let mut contracts = self.contracts.lock().unwrap();
        let Some(contract) = contracts.iter_mut().find(|c| c.id == id) else {
            return Ok(false);
        };
        contract.status = status;
        contract.updated_by = updated_by.to_string();
        contract.updated_at = Utc::now();
        Ok(true)
    }

    async fn evaluate_schema_change(
        &self,
        asset_fqn: &str,
        change: &graph_owl_core::contract::SchemaChange,
        asset_version: &str,
    ) -> Result<Vec<graph_owl_storage::BreachReport>, StorageError> {
        use graph_owl_core::contract::Compatibility;
        self.guard_write("evaluate_schema_change");

        let candidates: Vec<graph_owl_core::contract::Contract> = self
            .contracts
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.asset_fqn == asset_fqn && c.status.is_enforced())
            .cloned()
            .collect();

        let mut reports = Vec::new();
        for contract in candidates {
            let Compatibility::Breach { column, detail } =
                graph_owl_core::contract::check_compatibility(
                    change,
                    &contract.schema_guarantee,
                    contract.compatibility,
                )
            else {
                continue;
            };
            self.contract_breaches
                .lock()
                .unwrap()
                .push(graph_owl_core::contract::ContractBreach {
                    id: Uuid::new_v4(),
                    contract_id: contract.id,
                    column: column.clone(),
                    detail: detail.clone(),
                    asset_version: asset_version.to_string(),
                    detected_at: Utc::now(),
                });
            if let Some(held) = self
                .contracts
                .lock()
                .unwrap()
                .iter_mut()
                .find(|c| c.id == contract.id)
            {
                held.status = graph_owl_core::contract::ContractStatus::Violated;
            }
            reports.push(graph_owl_storage::BreachReport {
                contract_id: contract.id,
                contract_name: contract.name.clone(),
                producer: contract.producer.clone(),
                consumers: contract.consumers.clone(),
                column,
                detail,
            });
        }
        Ok(reports)
    }

    async fn clear_contract_breaches(
        &self,
        id: Uuid,
        updated_by: &str,
    ) -> Result<Option<i64>, StorageError> {
        self.guard_write("clear_contract_breaches");
        if !self.contracts.lock().unwrap().iter().any(|c| c.id == id) {
            return Ok(None);
        }
        let mut breaches = self.contract_breaches.lock().unwrap();
        let before = breaches.len();
        breaches.retain(|b| b.contract_id != id);
        let cleared = before - breaches.len();
        drop(breaches);

        if let Some(contract) = self
            .contracts
            .lock()
            .unwrap()
            .iter_mut()
            .find(|c| c.id == id)
            && contract.status == graph_owl_core::contract::ContractStatus::Violated
        {
            contract.status = graph_owl_core::contract::ContractStatus::Active;
            contract.updated_by = updated_by.to_string();
        }
        Ok(Some(i64::try_from(cleared).unwrap_or(i64::MAX)))
    }

    async fn record_usage(
        &self,
        batch: &[graph_owl_storage::UsageWrite],
    ) -> Result<graph_owl_storage::UsageIngest, StorageError> {
        self.guard_write("record_usage");
        let mut ingest = graph_owl_storage::UsageIngest::default();
        let now = Utc::now();
        let mut held = self.observations.lock().unwrap();

        for observation in batch {
            if observation.occurred_at > now {
                ingest.rejected += 1;
                continue;
            }
            // The same `(asset, query_id)` dedup key the partial unique
            // index enforces — and only when a `query_id` is present, or an
            // engine that supplies none would have every observation after
            // the first counted as a duplicate.
            if observation.query_id.is_some()
                && held.iter().any(|existing| {
                    existing.asset_fqn == observation.asset_fqn
                        && existing.query_id == observation.query_id
                })
            {
                ingest.duplicates += 1;
                continue;
            }
            let known = {
                let assets = self.assets.lock().expect("lock");
                assets
                    .iter()
                    .any(|a| a.fully_qualified_name == observation.asset_fqn)
            };
            if !known {
                ingest.unmatched += 1;
            }
            held.push(observation.clone());
            ingest.accepted += 1;
        }
        Ok(ingest)
    }

    async fn usage_rollups(
        &self,
        asset_fqn: &str,
    ) -> Result<Vec<graph_owl_core::usage::UsageRollup>, StorageError> {
        let held = self.observations.lock().unwrap();
        let mut grouped: std::collections::BTreeMap<
            (
                String,
                chrono::NaiveDate,
                graph_owl_core::usage::UsageOperation,
            ),
            (u64, u64),
        > = std::collections::BTreeMap::new();
        for observation in held.iter().filter(|o| o.asset_fqn == asset_fqn) {
            let entry = grouped
                .entry((
                    observation.consumer.key(),
                    observation.occurred_at.date_naive(),
                    observation.operation,
                ))
                .or_default();
            entry.0 += 1;
            entry.1 += u64::try_from(observation.row_count.unwrap_or(0)).unwrap_or(0);
        }
        Ok(grouped
            .into_iter()
            .map(|((consumer_key, day, operation), (count, rows))| {
                graph_owl_core::usage::UsageRollup {
                    consumer_key,
                    day,
                    operation,
                    count,
                    total_rows: (rows > 0).then_some(rows),
                }
            })
            .collect())
    }

    async fn last_accessed(&self, asset_fqn: &str) -> Result<Option<DateTime<Utc>>, StorageError> {
        let held = self.observations.lock().unwrap();
        Ok(held
            .iter()
            .filter(|o| o.asset_fqn == asset_fqn)
            .map(|o| o.occurred_at)
            .max())
    }

    async fn rebuild_usage_rollups(&self, asset_fqn: &str) -> Result<i64, StorageError> {
        // Derived on read here, so a rebuild is by construction identical
        // to the incremental answer — which is exactly what the equivalence
        // test asserts of the *real* adapter, where they are two paths.
        Ok(i64::try_from(self.usage_rollups(asset_fqn).await?.len()).unwrap_or(i64::MAX))
    }

    async fn prune_usage(&self, before: DateTime<Utc>) -> Result<i64, StorageError> {
        self.guard_write("prune_usage");
        let mut held = self.observations.lock().unwrap();
        // The most recent per asset survives, whatever its age — deleting
        // the only evidence an asset was ever used is not pruning.
        let mut keep: std::collections::BTreeMap<String, DateTime<Utc>> =
            std::collections::BTreeMap::new();
        for observation in held.iter() {
            keep.entry(observation.asset_fqn.clone())
                .and_modify(|newest| {
                    if observation.occurred_at > *newest {
                        *newest = observation.occurred_at;
                    }
                })
                .or_insert(observation.occurred_at);
        }
        let count_before = held.len();
        held.retain(|o| o.occurred_at >= before || keep.get(&o.asset_fqn) == Some(&o.occurred_at));
        Ok(i64::try_from(count_before - held.len()).unwrap_or(i64::MAX))
    }

    async fn resolve_usage_consumer(
        &self,
        identifier: &str,
        principal_id: &str,
    ) -> Result<i64, StorageError> {
        self.guard_write("resolve_usage_consumer");
        let mut held = self.observations.lock().unwrap();
        let mut moved = 0_i64;
        for observation in held.iter_mut() {
            let matches = observation.consumer
                == (graph_owl_core::usage::Consumer::Opaque {
                    identifier: identifier.to_string(),
                });
            if matches {
                observation.consumer = graph_owl_core::usage::Consumer::Principal {
                    id: principal_id.to_string(),
                };
                moved += 1;
            }
        }
        Ok(moved)
    }

    // ---- Epics 25 and 26, and as strict as the port ----
    //
    // In particular **exclusivity, idempotence and the rejection ledger are
    // all real here**. A double that skipped any of them would make the
    // facade's tests pass against a rule that does not exist in Postgres,
    // which is the failure mode this project has hit before.

    async fn create_classification(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        mutually_exclusive: bool,
        updated_by: &str,
    ) -> Result<graph_owl_core::classification::Classification, StorageError> {
        self.guard_write("create_classification");
        let mut held = self.classifications.lock().unwrap();
        if held.iter().any(|c| c.name == name) {
            return Err(StorageError::Conflict {
                detail: format!("a classification named `{name}` already exists"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let now = Utc::now();
        let created = graph_owl_core::classification::Classification {
            id,
            name: name.to_string(),
            description: description.map(str::to_string),
            mutually_exclusive,
            version: EntityVersion::initial(),
            updated_by: updated_by.to_string(),
            change_description: None,
            created_at: now,
            updated_at: now,
        };
        held.push(created.clone());
        Ok(created)
    }

    async fn get_classification(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::classification::Classification>, StorageError> {
        Ok(self
            .classifications
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .cloned())
    }

    async fn list_classifications(
        &self,
    ) -> Result<Vec<graph_owl_core::classification::Classification>, StorageError> {
        Ok(self.classifications.lock().unwrap().clone())
    }

    async fn delete_classification(
        &self,
        id: Uuid,
        recursive: bool,
    ) -> Result<Result<bool, i64>, StorageError> {
        self.guard_write("delete_classification");
        let tag_fqns: Vec<String> = {
            let tags = self.tags.lock().unwrap();
            tags.iter()
                .filter(|t| t.classification_id == id)
                .map(|t| t.fully_qualified_name.clone())
                .collect()
        };
        if !tag_fqns.is_empty() && !recursive {
            return Ok(Err(i64::try_from(tag_fqns.len()).unwrap_or(i64::MAX)));
        }
        if recursive {
            self.labels
                .lock()
                .unwrap()
                .retain(|l| !tag_fqns.contains(&l.tag_fqn));
            self.tags
                .lock()
                .unwrap()
                .retain(|t| t.classification_id != id);
        }
        let mut held = self.classifications.lock().unwrap();
        let before = held.len();
        held.retain(|c| c.id != id);
        Ok(Ok(held.len() < before))
    }

    async fn create_tag(
        &self,
        id: Uuid,
        classification_id: Uuid,
        name: &str,
        description: Option<&str>,
        updated_by: &str,
    ) -> Result<Option<graph_owl_core::classification::Tag>, StorageError> {
        self.guard_write("create_tag");
        let classification = {
            let held = self.classifications.lock().unwrap();
            held.iter()
                .find(|c| c.id == classification_id)
                .map(|c| c.name.clone())
        };
        let Some(classification) = classification else {
            return Ok(None);
        };
        let fqn = graph_owl_core::classification::tag_fqn(&classification, name);

        let mut tags = self.tags.lock().unwrap();
        // **Scoped to the classification**, exactly as the real unique
        // index is. A globally-scoped double would refuse `Tier.Gold`
        // beside `SupportPlan.Gold`, which the database accepts — so the
        // rule would look enforced here and be untested where it lives.
        if tags
            .iter()
            .any(|t| t.classification_id == classification_id && t.name == name)
        {
            return Err(StorageError::Conflict {
                detail: format!("`{fqn}` already exists"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let now = Utc::now();
        let created = graph_owl_core::classification::Tag {
            id,
            name: name.to_string(),
            classification_id,
            fully_qualified_name: fqn,
            description: description.map(str::to_string),
            version: EntityVersion::initial(),
            updated_by: updated_by.to_string(),
            created_at: now,
            updated_at: now,
        };
        tags.push(created.clone());
        Ok(Some(created))
    }

    async fn get_tag_by_fqn(
        &self,
        fqn: &str,
    ) -> Result<Option<graph_owl_core::classification::Tag>, StorageError> {
        Ok(self
            .tags
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.fully_qualified_name == fqn)
            .cloned())
    }

    async fn list_tags(
        &self,
        classification_id: Option<Uuid>,
    ) -> Result<Vec<graph_owl_core::classification::Tag>, StorageError> {
        let tags = self.tags.lock().unwrap();
        Ok(tags
            .iter()
            .filter(|t| classification_id.is_none_or(|id| t.classification_id == id))
            .cloned()
            .collect())
    }

    async fn apply_tag(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        label_type: graph_owl_core::classification::LabelType,
        state: graph_owl_core::classification::LabelState,
        applied_by: &str,
    ) -> Result<graph_owl_storage::LabelOutcome, StorageError> {
        use graph_owl_core::classification::LabelType;
        use graph_owl_storage::LabelOutcome;
        self.guard_write("apply_tag");

        let tag = {
            let tags = self.tags.lock().unwrap();
            tags.iter()
                .find(|t| t.fully_qualified_name == tag_fqn)
                .cloned()
        };
        let Some(tag) = tag else {
            return Ok(LabelOutcome::NoSuchTag);
        };
        let live = {
            let assets = self.assets.lock().expect("lock");
            assets
                .iter()
                .any(|a| a.fully_qualified_name == target_fqn && !a.deleted)
        };
        if !live {
            return Ok(LabelOutcome::NoSuchTarget);
        }
        if matches!(label_type, LabelType::Automated | LabelType::Derived)
            && self
                .label_rejections
                .lock()
                .unwrap()
                .contains(&(tag_fqn.to_string(), target_fqn.to_string()))
        {
            return Ok(LabelOutcome::PreviouslyRejected);
        }

        let existing: Vec<graph_owl_core::classification::TagLabel> = self
            .labels
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.target_fqn == target_fqn)
            .cloned()
            .collect();
        if existing.iter().any(|l| l.tag_fqn == tag_fqn) {
            return Ok(LabelOutcome::AlreadyApplied);
        }

        let exclusive = {
            let held = self.classifications.lock().unwrap();
            held.iter()
                .find(|c| c.id == tag.classification_id)
                .is_some_and(|c| c.mutually_exclusive)
        };
        if let Some(blocker) =
            graph_owl_core::classification::conflicting_label(&existing, tag_fqn, exclusive)
        {
            return Ok(LabelOutcome::Conflicts {
                existing_tag_fqn: blocker.tag_fqn.clone(),
            });
        }

        self.labels
            .lock()
            .unwrap()
            .push(graph_owl_core::classification::TagLabel {
                tag_fqn: tag_fqn.to_string(),
                target_fqn: target_fqn.to_string(),
                label_type,
                state,
                applied_by: applied_by.to_string(),
                applied_at: Utc::now(),
                confirmed_by: None,
            });
        self.bump_by_fqn(target_fqn, applied_by);
        Ok(LabelOutcome::Applied)
    }

    async fn remove_tag(&self, tag_fqn: &str, target_fqn: &str) -> Result<bool, StorageError> {
        let mut labels = self.labels.lock().unwrap();
        let before = labels.len();
        labels.retain(|l| !(l.tag_fqn == tag_fqn && l.target_fqn == target_fqn));
        Ok(labels.len() < before)
    }

    async fn labels_on(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<graph_owl_core::classification::TagLabel>, StorageError> {
        Ok(self
            .labels
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.target_fqn == target_fqn)
            .cloned()
            .collect())
    }

    async fn decide_label(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        confirmed: bool,
        decided_by: &str,
    ) -> Result<graph_owl_storage::LabelDecision, StorageError> {
        use graph_owl_core::classification::LabelState;
        use graph_owl_storage::LabelDecision;
        self.guard_write("decide_label");

        let mut labels = self.labels.lock().unwrap();
        let Some(index) = labels
            .iter()
            .position(|l| l.tag_fqn == tag_fqn && l.target_fqn == target_fqn)
        else {
            return Ok(LabelDecision::NoSuchLabel);
        };
        if confirmed && labels[index].state == LabelState::Confirmed {
            return Ok(LabelDecision::AlreadyConfirmed);
        }
        if confirmed {
            labels[index].state = LabelState::Confirmed;
            labels[index].confirmed_by = Some(decided_by.to_string());
        } else {
            labels.remove(index);
            // **Recorded, not merely removed** — the half a double is most
            // tempted to skip, and the one that makes a rejection stick.
            let mut rejections = self.label_rejections.lock().unwrap();
            let key = (tag_fqn.to_string(), target_fqn.to_string());
            if !rejections.contains(&key) {
                rejections.push(key);
            }
        }
        drop(labels);
        self.bump_by_fqn(target_fqn, decided_by);
        Ok(LabelDecision::Decided)
    }

    async fn suggested_labels(
        &self,
        limit: i64,
    ) -> Result<Vec<graph_owl_core::classification::TagLabel>, StorageError> {
        use graph_owl_core::classification::LabelState;
        let labels = self.labels.lock().unwrap();
        Ok(labels
            .iter()
            .filter(|l| l.state == LabelState::Suggested)
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .cloned()
            .collect())
    }

    async fn tag_usage(&self, tag_fqn: &str) -> Result<graph_owl_storage::TagUsage, StorageError> {
        let targets: Vec<String> = self
            .labels
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.tag_fqn == tag_fqn)
            .map(|l| l.target_fqn.clone())
            .collect();
        let assets = self.assets.lock().expect("lock");
        let mut by_kind: std::collections::BTreeMap<String, i64> =
            std::collections::BTreeMap::new();
        for asset in assets
            .iter()
            // Live only: a tombstoned column does not keep a governance
            // label alive.
            .filter(|a| !a.deleted && targets.contains(&a.fully_qualified_name))
        {
            *by_kind.entry(asset.kind.as_str().to_string()).or_default() += 1;
        }
        Ok(graph_owl_storage::TagUsage {
            by_kind: by_kind.into_iter().collect(),
        })
    }

    async fn delete_tag(
        &self,
        tag_fqn: &str,
        force: bool,
        updated_by: &str,
    ) -> Result<Option<i64>, StorageError> {
        self.guard_write("delete_tag");
        if !self
            .tags
            .lock()
            .unwrap()
            .iter()
            .any(|t| t.fully_qualified_name == tag_fqn)
        {
            return Ok(None);
        }
        let mut removed = 0_i64;
        if force {
            let targets: Vec<String> = self
                .labels
                .lock()
                .unwrap()
                .iter()
                .filter(|l| l.tag_fqn == tag_fqn)
                .map(|l| l.target_fqn.clone())
                .collect();
            for target in &targets {
                self.bump_by_fqn(target, updated_by);
            }
            removed = i64::try_from(targets.len()).unwrap_or(i64::MAX);
            self.labels.lock().unwrap().retain(|l| l.tag_fqn != tag_fqn);
        }
        self.tags
            .lock()
            .unwrap()
            .retain(|t| t.fully_qualified_name != tag_fqn);
        Ok(Some(removed))
    }

    async fn propagate_tag(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        recursive: bool,
        applied_by: &str,
    ) -> Result<i64, StorageError> {
        use graph_owl_core::classification::{LabelState, LabelType};
        let children: Vec<String> = {
            let assets = self.assets.lock().expect("lock");
            let parent_id = assets
                .iter()
                .find(|a| a.fully_qualified_name == target_fqn)
                .map(|a| a.id);
            assets
                .iter()
                .filter(|a| {
                    !a.deleted
                        && if recursive {
                            a.fully_qualified_name
                                .starts_with(&format!("{target_fqn}."))
                        } else {
                            parent_id.is_some_and(|id| a.parent_id == Some(id))
                        }
                })
                .map(|a| a.fully_qualified_name.clone())
                .collect()
        };

        let mut affected = 0_i64;
        for child in children {
            let held = self
                .labels_on(&child)
                .await?
                .into_iter()
                .find(|l| l.tag_fqn == tag_fqn);
            if !graph_owl_core::classification::propagation_may_overwrite(held.as_ref()) {
                continue;
            }
            if held.is_some() {
                self.remove_tag(tag_fqn, &child).await?;
            }
            if matches!(
                self.apply_tag(
                    tag_fqn,
                    &child,
                    LabelType::Propagated,
                    LabelState::Confirmed,
                    applied_by,
                )
                .await?,
                graph_owl_storage::LabelOutcome::Applied
            ) {
                affected += 1;
            }
        }
        Ok(affected)
    }

    async fn set_lifecycle(
        &self,
        asset_id: Uuid,
        to: graph_owl_core::lifecycle::LifecycleState,
        deprecation: Option<&graph_owl_core::lifecycle::Deprecation>,
        updated_by: &str,
    ) -> Result<graph_owl_storage::LifecycleOutcome, StorageError> {
        use graph_owl_storage::LifecycleOutcome;
        self.guard_write("set_lifecycle");
        let mut assets = self.assets.lock().expect("lock");
        let Some(asset) = assets.iter_mut().find(|a| a.id == asset_id) else {
            return Ok(LifecycleOutcome::NotFound);
        };
        let from = asset.lifecycle;
        if !graph_owl_core::lifecycle::can_transition(from, to) {
            return Ok(LifecycleOutcome::Illegal { from, to });
        }
        asset.lifecycle = to;
        asset.deprecation = deprecation.cloned();
        asset.version = asset
            .version
            .bump(graph_owl_core::envelope::ChangeKind::Minor);
        asset.updated_by = updated_by.to_string();
        asset.updated_at = Utc::now();
        Ok(LifecycleOutcome::Moved(Box::new(asset.clone())))
    }

    async fn terminal_successor(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
        use graph_owl_core::lifecycle::LifecycleState;
        const MAX_HOPS: usize = 10;
        let assets = self.assets.lock().expect("lock");
        let mut seen = std::collections::HashSet::new();
        let mut current = fqn.to_string();
        for _ in 0..MAX_HOPS {
            if !seen.insert(current.clone()) {
                return Ok(None);
            }
            let Some(asset) = assets.iter().find(|a| a.fully_qualified_name == current) else {
                return Ok(None);
            };
            match asset.lifecycle {
                LifecycleState::Deprecated | LifecycleState::Retired => {
                    let Some(next) = asset
                        .deprecation
                        .as_ref()
                        .and_then(|d| d.successor_fqn.clone())
                    else {
                        return Ok(None);
                    };
                    current = next;
                }
                _ => return Ok(Some(asset.clone())),
            }
        }
        Ok(None)
    }

    async fn create_certification_type(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        default_validity_days: i32,
        required_evidence: &[String],
        authorized_issuers: &[String],
        _updated_by: &str,
    ) -> Result<graph_owl_storage::StoredCertificationType, StorageError> {
        self.guard_write("create_certification_type");
        let mut types = self.certification_types.lock().unwrap();
        if types.iter().any(|t| t.name == name) {
            return Err(StorageError::Conflict {
                detail: format!("a certification type named `{name}` already exists"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let created = graph_owl_storage::StoredCertificationType {
            id,
            name: name.to_string(),
            description: description.map(str::to_string),
            default_validity_days,
            required_evidence: required_evidence.to_vec(),
            authorized_issuers: authorized_issuers.to_vec(),
        };
        types.push(created.clone());
        Ok(created)
    }

    async fn list_certification_types(
        &self,
    ) -> Result<Vec<graph_owl_storage::StoredCertificationType>, StorageError> {
        Ok(self.certification_types.lock().unwrap().clone())
    }

    async fn get_certification_type(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StoredCertificationType>, StorageError> {
        Ok(self
            .certification_types
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn issue_certification(
        &self,
        id: Uuid,
        target_fqn: &str,
        type_id: Uuid,
        issuer: &str,
        criteria: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        evidence: &[(String, String)],
    ) -> Result<graph_owl_storage::IssueOutcome, StorageError> {
        use graph_owl_storage::IssueOutcome;
        self.guard_write("issue_certification");

        let Some(certification_type) = self.get_certification_type(type_id).await? else {
            return Ok(IssueOutcome::NoSuchType);
        };
        if !graph_owl_core::lifecycle::may_issue(&certification_type.authorized_issuers, issuer) {
            return Ok(IssueOutcome::NotAuthorized);
        }
        // **Enforced here too**, so a renewal that lost its evidence fails
        // in the double exactly as it does in Postgres.
        let supplied: Vec<String> = evidence.iter().map(|(kind, _)| kind.clone()).collect();
        let missing = graph_owl_core::lifecycle::missing_evidence(
            &certification_type.required_evidence,
            &supplied,
        );
        if !missing.is_empty() {
            return Ok(IssueOutcome::MissingEvidence(missing));
        }
        let live = {
            let assets = self.assets.lock().expect("lock");
            assets
                .iter()
                .any(|a| a.fully_qualified_name == target_fqn && !a.deleted)
        };
        if !live {
            return Ok(IssueOutcome::NoSuchTarget);
        }

        let expires_at = expires_at.unwrap_or_else(|| {
            Utc::now() + chrono::Duration::days(i64::from(certification_type.default_validity_days))
        });
        let mut held = self.certifications.lock().unwrap();
        // Supersedes rather than accumulating: one live answer per (target,
        // type).
        held.retain(|c| !(c.target_fqn == target_fqn && c.type_id == type_id));
        let issued = graph_owl_storage::StoredCertification {
            id,
            target_fqn: target_fqn.to_string(),
            type_id,
            type_name: certification_type.name,
            issuer: issuer.to_string(),
            criteria: criteria.map(str::to_string),
            issued_at: Utc::now(),
            expires_at,
            evidence: evidence.to_vec(),
        };
        held.push(issued.clone());
        Ok(IssueOutcome::Issued(Box::new(issued)))
    }

    async fn certifications_on(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<graph_owl_storage::StoredCertification>, StorageError> {
        Ok(self
            .certifications
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.target_fqn == target_fqn)
            .cloned()
            .collect())
    }

    async fn certifications_expiring_before(
        &self,
        instant: DateTime<Utc>,
    ) -> Result<Vec<graph_owl_storage::StoredCertification>, StorageError> {
        Ok(self
            .certifications
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.expires_at < instant)
            .cloned()
            .collect())
    }

    // ---- Epic 23, and as strict as the port ----
    //
    // In particular the **inheritance walk is real** here. A double that
    // answered only direct assignments would make every multi-hop test pass
    // against a resolver that does not walk, and the walk is the whole of
    // decision 2 — which is what makes adoption possible at all.

    async fn create_domain(
        &self,
        id: Uuid,
        name: &str,
        parent_id: Option<Uuid>,
        description: Option<&str>,
        domain_type: Option<&str>,
        experts: &[String],
        updated_by: &str,
    ) -> Result<Option<graph_owl_core::domain::Domain>, StorageError> {
        self.guard_write("create_domain");
        let mut domains = self.domains.lock().unwrap();
        let parent_fqn = match parent_id {
            None => None,
            Some(parent) => match domains.iter().find(|d| d.id == parent) {
                None => return Ok(None),
                Some(found) => Some(found.fully_qualified_name.clone()),
            },
        };
        let fqn = graph_owl_core::domain::domain_fqn(parent_fqn.as_deref(), name);
        if domains.iter().any(|d| d.fully_qualified_name == fqn) {
            return Err(StorageError::Conflict {
                detail: format!("a domain already exists at `{fqn}`"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let now = Utc::now();
        let domain = graph_owl_core::domain::Domain {
            id,
            name: name.to_string(),
            fully_qualified_name: fqn,
            parent_id,
            description: description.map(str::to_string),
            domain_type: domain_type.map(str::to_string),
            experts: experts.to_vec(),
            version: EntityVersion::initial(),
            updated_by: updated_by.to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        domains.push(domain.clone());
        Ok(Some(domain))
    }

    async fn get_domain(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::domain::Domain>, StorageError> {
        let domains = self.domains.lock().unwrap();
        Ok(domains.iter().find(|d| d.id == id).cloned())
    }

    async fn get_domain_by_fqn(
        &self,
        fqn: &str,
    ) -> Result<Option<graph_owl_core::domain::Domain>, StorageError> {
        let domains = self.domains.lock().unwrap();
        Ok(domains
            .iter()
            .find(|d| d.fully_qualified_name == fqn)
            .cloned())
    }

    async fn list_domains(
        &self,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_core::domain::Domain>, StorageError> {
        let domains = self.domains.lock().unwrap();
        let mut live: Vec<graph_owl_core::domain::Domain> =
            domains.iter().filter(|d| !d.deleted).cloned().collect();
        live.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        Ok(Page::from_overfetch(
            live,
            page.limit,
            |d: &graph_owl_core::domain::Domain| Cursor::new(d.fully_qualified_name.clone(), d.id),
        ))
    }

    async fn update_domain(
        &self,
        id: Uuid,
        update: &graph_owl_storage::DomainUpdate,
        updated_by: &str,
    ) -> Result<Option<graph_owl_core::domain::Domain>, StorageError> {
        self.guard_write("update_domain");
        let mut domains = self.domains.lock().unwrap();
        let Some(index) = domains.iter().position(|d| d.id == id) else {
            return Ok(None);
        };
        let before = domains[index].clone();
        let mut after = before.clone();
        if let Some(name) = &update.name {
            after.name = name.clone();
        }
        if let Some(description) = &update.description {
            after.description = description.clone();
        }
        if let Some(domain_type) = &update.domain_type {
            after.domain_type = domain_type.clone();
        }
        if let Some(experts) = &update.experts {
            after.experts = experts.clone();
        }
        if let Some(parent_id) = &update.parent_id {
            after.parent_id = *parent_id;
        }
        let parent_fqn = after.parent_id.and_then(|parent| {
            domains
                .iter()
                .find(|d| d.id == parent)
                .map(|d| d.fully_qualified_name.clone())
        });
        after.fully_qualified_name =
            graph_owl_core::domain::domain_fqn(parent_fqn.as_deref(), &after.name);

        if after == before {
            return Ok(Some(before));
        }
        after.version = before
            .version
            .bump(graph_owl_core::envelope::ChangeKind::Minor);
        after.updated_by = updated_by.to_string();
        after.updated_at = Utc::now();

        // The subtree's paths move with it, exactly as the SQL adapter does
        // — a double that skipped it would let a rename pass every test
        // here while orphaning every descendant path in Postgres.
        let (old_prefix, new_prefix) = (
            before.fully_qualified_name.clone(),
            after.fully_qualified_name.clone(),
        );
        domains[index] = after.clone();
        if old_prefix != new_prefix {
            let moved: Vec<Uuid> = domains
                .iter()
                .filter(|d| {
                    d.id != id
                        && d.fully_qualified_name
                            .starts_with(&format!("{old_prefix}."))
                })
                .map(|d| d.id)
                .collect();
            for child in moved {
                if let Some(slot) = domains.iter_mut().find(|d| d.id == child) {
                    slot.fully_qualified_name = format!(
                        "{new_prefix}{}",
                        &slot.fully_qualified_name[old_prefix.len()..]
                    );
                }
            }
        }
        Ok(Some(after))
    }

    async fn domain_would_cycle(&self, domain: Uuid, parent: Uuid) -> Result<bool, StorageError> {
        if domain == parent {
            return Ok(true);
        }
        // The **whole** ancestry, not the immediate parent: a depth-1 check
        // passes `A → B → C → A` and leaves an ancestor walk that never
        // terminates, which is a hung request rather than an error.
        let domains = self.domains.lock().unwrap();
        let mut node = Some(parent);
        while let Some(current) = node {
            if current == domain {
                return Ok(true);
            }
            node = domains
                .iter()
                .find(|d| d.id == current)
                .and_then(|d| d.parent_id);
        }
        Ok(false)
    }

    async fn child_domains(
        &self,
        parent: Option<Uuid>,
    ) -> Result<Vec<graph_owl_core::domain::Domain>, StorageError> {
        let domains = self.domains.lock().unwrap();
        Ok(domains
            .iter()
            .filter(|d| !d.deleted && d.parent_id == parent)
            .cloned()
            .collect())
    }

    async fn assign_asset_domain(
        &self,
        asset_id: Uuid,
        domain_id: Option<Uuid>,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError> {
        self.guard_write("assign_asset_domain");
        let mut assets = self.assets.lock().expect("lock");
        let Some(asset) = assets.iter_mut().find(|a| a.id == asset_id) else {
            return Ok(None);
        };
        asset.version = asset
            .version
            .bump(graph_owl_core::envelope::ChangeKind::Minor);
        asset.updated_by = updated_by.to_string();
        asset.updated_at = Utc::now();
        let updated = asset.clone();
        drop(assets);

        let mut links = self.asset_domains.lock().unwrap();
        links.retain(|(a, _)| *a != asset_id);
        if let Some(domain) = domain_id {
            links.push((asset_id, domain));
        }
        Ok(Some(updated))
    }

    async fn resolve_asset_domain(
        &self,
        asset_id: Uuid,
    ) -> Result<Option<graph_owl_core::domain::DomainAssignment>, StorageError> {
        let assets = self.assets.lock().expect("lock");
        let links = self.asset_domains.lock().unwrap();
        let domains = self.domains.lock().unwrap();

        // **Stops at the nearest assigned ancestor.** Accumulating every
        // assigned ancestor would answer "which domains is this under" — a
        // question with several answers, which is the shared accountability
        // decision 1 refuses.
        let mut node = Some(asset_id);
        let mut hops = 0_usize;
        while let Some(current) = node {
            if let Some((_, domain_id)) = links.iter().find(|(a, _)| *a == current) {
                return Ok(domains.iter().find(|d| d.id == *domain_id).map(|d| {
                    graph_owl_core::domain::DomainAssignment {
                        id: d.id,
                        name: d.name.clone(),
                        fully_qualified_name: d.fully_qualified_name.clone(),
                        inherited: hops > 0,
                    }
                }));
            }
            node = assets
                .iter()
                .find(|a| a.id == current)
                .and_then(|a| a.parent_id);
            hops += 1;
        }
        Ok(None)
    }

    async fn count_assets_in_domain(&self, domain: Uuid) -> Result<i64, StorageError> {
        let ids: Vec<Uuid> = {
            let assets = self.assets.lock().expect("lock");
            assets.iter().filter(|a| !a.deleted).map(|a| a.id).collect()
        };
        let mut total = 0_i64;
        for id in ids {
            if self
                .resolve_asset_domain(id)
                .await?
                .is_some_and(|a| a.id == domain)
            {
                total += 1;
            }
        }
        Ok(total)
    }

    async fn delete_domain(
        &self,
        id: Uuid,
        reassign_to: Option<Uuid>,
        _updated_by: &str,
    ) -> Result<graph_owl_storage::DomainDeletion, StorageError> {
        use graph_owl_storage::{DomainDeletion, DomainHoldings};
        self.guard_write("delete_domain");
        let children = {
            let domains = self.domains.lock().unwrap();
            if !domains.iter().any(|d| d.id == id) {
                return Ok(DomainDeletion::NotFound);
            }
            i64::try_from(domains.iter().filter(|d| d.parent_id == Some(id)).count())
                .unwrap_or(i64::MAX)
        };
        if children > 0 {
            return Ok(DomainDeletion::HasChildren { children });
        }

        let held_assets: Vec<Uuid> = {
            let links = self.asset_domains.lock().unwrap();
            links
                .iter()
                .filter(|(_, d)| *d == id)
                .map(|(a, _)| *a)
                .collect()
        };
        let held_products: Vec<Uuid> = {
            let products = self.data_products.lock().unwrap();
            products
                .iter()
                .filter(|p| p.domain_id == Some(id))
                .map(|p| p.id)
                .collect()
        };

        let Some(target) = reassign_to else {
            if !held_assets.is_empty() || !held_products.is_empty() {
                return Ok(DomainDeletion::StillHolds(Box::new(DomainHoldings {
                    assets: i64::try_from(held_assets.len()).unwrap_or(i64::MAX),
                    data_products: i64::try_from(held_products.len()).unwrap_or(i64::MAX),
                })));
            }
            self.domains.lock().unwrap().retain(|d| d.id != id);
            return Ok(DomainDeletion::Deleted {
                reassigned_assets: 0,
                reassigned_products: 0,
            });
        };

        if !self
            .domains
            .lock()
            .unwrap()
            .iter()
            .any(|d| d.id == target && !d.deleted)
        {
            return Ok(DomainDeletion::UnknownTarget);
        }

        {
            let mut links = self.asset_domains.lock().unwrap();
            for link in links.iter_mut().filter(|(_, d)| *d == id) {
                link.1 = target;
            }
        }
        {
            let mut products = self.data_products.lock().unwrap();
            for product in products.iter_mut().filter(|p| p.domain_id == Some(id)) {
                product.domain_id = Some(target);
            }
        }
        self.domains.lock().unwrap().retain(|d| d.id != id);

        Ok(DomainDeletion::Deleted {
            reassigned_assets: i64::try_from(held_assets.len()).unwrap_or(i64::MAX),
            reassigned_products: i64::try_from(held_products.len()).unwrap_or(i64::MAX),
        })
    }

    async fn create_data_product(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        purpose: Option<&str>,
        domain_id: Option<Uuid>,
        updated_by: &str,
    ) -> Result<graph_owl_core::domain::DataProduct, StorageError> {
        self.guard_write("create_data_product");
        let mut products = self.data_products.lock().unwrap();
        if products.iter().any(|p| p.name == name) {
            return Err(StorageError::Conflict {
                detail: format!("a data product named `{name}` already exists"),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::Fqn,
            });
        }
        let now = Utc::now();
        let product = graph_owl_core::domain::DataProduct {
            id,
            name: name.to_string(),
            fully_qualified_name: name.to_string(),
            description: description.map(str::to_string),
            purpose: purpose.map(str::to_string),
            domain_id,
            version: EntityVersion::initial(),
            updated_by: updated_by.to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        products.push(product.clone());
        Ok(product)
    }

    async fn get_data_product(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::domain::DataProduct>, StorageError> {
        let products = self.data_products.lock().unwrap();
        Ok(products.iter().find(|p| p.id == id).cloned())
    }

    async fn list_data_products(
        &self,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_core::domain::DataProduct>, StorageError> {
        let products = self.data_products.lock().unwrap();
        let mut live: Vec<graph_owl_core::domain::DataProduct> =
            products.iter().filter(|p| !p.deleted).cloned().collect();
        live.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        Ok(Page::from_overfetch(
            live,
            page.limit,
            |p: &graph_owl_core::domain::DataProduct| {
                Cursor::new(p.fully_qualified_name.clone(), p.id)
            },
        ))
    }

    async fn update_data_product(
        &self,
        id: Uuid,
        update: &graph_owl_storage::DataProductUpdate,
        updated_by: &str,
    ) -> Result<Option<graph_owl_core::domain::DataProduct>, StorageError> {
        self.guard_write("update_data_product");
        let mut products = self.data_products.lock().unwrap();
        let Some(product) = products.iter_mut().find(|p| p.id == id) else {
            return Ok(None);
        };
        if let Some(name) = &update.name {
            product.name = name.clone();
            product.fully_qualified_name = name.clone();
        }
        if let Some(description) = &update.description {
            product.description = description.clone();
        }
        if let Some(purpose) = &update.purpose {
            product.purpose = purpose.clone();
        }
        if let Some(domain_id) = &update.domain_id {
            product.domain_id = *domain_id;
        }
        product.version = product
            .version
            .bump(graph_owl_core::envelope::ChangeKind::Minor);
        product.updated_by = updated_by.to_string();
        product.updated_at = Utc::now();
        Ok(Some(product.clone()))
    }

    async fn delete_data_product(&self, id: Uuid) -> Result<bool, StorageError> {
        self.guard_write("delete_data_product");
        let mut products = self.data_products.lock().unwrap();
        let before = products.len();
        products.retain(|p| p.id != id);
        // The membership edges go with it; the assets do not.
        self.product_members
            .lock()
            .unwrap()
            .retain(|(p, _)| *p != id);
        Ok(products.len() < before)
    }

    async fn add_product_asset(
        &self,
        product_id: Uuid,
        asset_id: Uuid,
    ) -> Result<Result<(), graph_owl_storage::MembershipRefusal>, StorageError> {
        use graph_owl_storage::MembershipRefusal;
        self.guard_write("add_product_asset");
        if !self
            .data_products
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.id == product_id && !p.deleted)
        {
            return Ok(Err(MembershipRefusal::NoSuchProduct));
        }
        let deleted = {
            let assets = self.assets.lock().expect("lock");
            match assets.iter().find(|a| a.id == asset_id) {
                None => return Ok(Err(MembershipRefusal::NoSuchAsset)),
                Some(asset) => asset.deleted,
            }
        };
        if deleted {
            return Ok(Err(MembershipRefusal::AssetDeleted));
        }
        let mut members = self.product_members.lock().unwrap();
        // Idempotent: adding twice is one edge, which is the state the
        // caller asked for rather than an error.
        if !members.contains(&(product_id, asset_id)) {
            members.push((product_id, asset_id));
        }
        Ok(Ok(()))
    }

    async fn remove_product_asset(
        &self,
        product_id: Uuid,
        asset_id: Uuid,
    ) -> Result<bool, StorageError> {
        self.guard_write("remove_product_asset");
        let mut members = self.product_members.lock().unwrap();
        let before = members.len();
        members.retain(|edge| *edge != (product_id, asset_id));
        Ok(members.len() < before)
    }

    async fn product_assets(
        &self,
        product_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let members: Vec<Uuid> = self
            .product_members
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| *p == product_id)
            .map(|(_, a)| *a)
            .collect();
        let assets = self.assets.lock().expect("lock");
        let mut found: Vec<Asset> = assets
            .iter()
            .filter(|a| !a.deleted && members.contains(&a.id))
            .cloned()
            .collect();
        found.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        Ok(Page::from_overfetch(found, page.limit, |a: &Asset| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        }))
    }

    async fn asset_products(
        &self,
        asset_id: Uuid,
    ) -> Result<Vec<graph_owl_core::domain::DataProduct>, StorageError> {
        let members: Vec<Uuid> = self
            .product_members
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, a)| *a == asset_id)
            .map(|(p, _)| *p)
            .collect();
        let products = self.data_products.lock().unwrap();
        Ok(products
            .iter()
            .filter(|p| !p.deleted && members.contains(&p.id))
            .cloned()
            .collect())
    }

    async fn custom_property_values(
        &self,
        entity_type: &str,
        name: &str,
    ) -> Result<Vec<(Uuid, serde_json::Value)>, StorageError> {
        // The same "holds a value" test `count_custom_property_values`
        // uses. A double where the count and the list disagreed would
        // report a `409` for three values and migrate two, and the SQL
        // impl's own agreement would then be untested from here.
        let assets = self.assets.lock().expect("lock");
        Ok(assets
            .iter()
            .filter(|asset| asset.kind.as_str() == entity_type)
            .filter_map(|asset| {
                let value = asset.extension.as_ref()?.get(name)?;
                (!value.is_null()).then(|| (asset.id, value.clone()))
            })
            .collect())
    }

    async fn insert_pack(
        &self,
        pack: graph_owl_ontology::pack::OntologyPack,
        source_turtle: &[u8],
    ) -> Result<graph_owl_ontology::pack::OntologyPack, StorageError> {
        let mut held = self.ontology_packs.lock().unwrap();
        if held
            .iter()
            .any(|p| p.pack_id == pack.pack_id && p.version == pack.version)
        {
            return Err(StorageError::Conflict {
                detail: format!(
                    "`{}` version `{}` is already imported",
                    pack.pack_id, pack.version
                ),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::PackVersionExists,
            });
        }
        held.push(pack.clone());
        drop(held);
        self.pack_source_turtle
            .lock()
            .unwrap()
            .push((pack.id, source_turtle.to_vec()));
        Ok(pack)
    }

    async fn get_pack_source_turtle(&self, pack_id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self
            .pack_source_turtle
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| *id == pack_id)
            .map(|(_, bytes)| bytes.clone()))
    }

    async fn update_pack_version(
        &self,
        id: Uuid,
        version: &str,
        term_count: usize,
        source_turtle: &[u8],
        imported_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StorageError> {
        if let Some(pack) = self
            .ontology_packs
            .lock()
            .unwrap()
            .iter_mut()
            .find(|p| p.id == id)
        {
            pack.version = version.to_string();
            pack.term_count = term_count;
            pack.imported_at = imported_at;
        }
        let mut turtle = self.pack_source_turtle.lock().unwrap();
        if let Some(entry) = turtle.iter_mut().find(|(pid, _)| *pid == id) {
            entry.1 = source_turtle.to_vec();
        } else {
            turtle.push((id, source_turtle.to_vec()));
        }
        Ok(())
    }

    async fn get_pack(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_ontology::pack::OntologyPack>, StorageError> {
        Ok(self
            .ontology_packs
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    async fn get_pack_by_id_and_version(
        &self,
        pack_id: &str,
        version: &str,
    ) -> Result<Option<graph_owl_ontology::pack::OntologyPack>, StorageError> {
        Ok(self
            .ontology_packs
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.pack_id == pack_id && p.version == version)
            .cloned())
    }

    async fn list_packs(
        &self,
    ) -> Result<Vec<graph_owl_ontology::pack::OntologyPack>, StorageError> {
        let mut packs = self.ontology_packs.lock().unwrap().clone();
        packs.sort_by(|a, b| (&a.pack_id, &a.version).cmp(&(&b.pack_id, &b.version)));
        Ok(packs)
    }

    async fn delete_pack(&self, id: Uuid) -> Result<(), StorageError> {
        self.ontology_packs.lock().unwrap().retain(|p| p.id != id);
        Ok(())
    }

    async fn insert_pack_term(
        &self,
        pack_id: Uuid,
        term_id: Uuid,
        source_iri: &str,
    ) -> Result<(), StorageError> {
        self.pack_terms
            .lock()
            .unwrap()
            .push((pack_id, term_id, source_iri.to_string()));
        Ok(())
    }

    async fn pack_terms(&self, pack_id: Uuid) -> Result<Vec<(String, Uuid)>, StorageError> {
        Ok(self
            .pack_terms
            .lock()
            .unwrap()
            .iter()
            .filter(|(pid, _, _)| *pid == pack_id)
            .map(|(_, term_id, iri)| (iri.clone(), *term_id))
            .collect())
    }

    async fn pack_term_by_iri(
        &self,
        pack_id: Uuid,
        source_iri: &str,
    ) -> Result<Option<Uuid>, StorageError> {
        Ok(self
            .pack_terms
            .lock()
            .unwrap()
            .iter()
            .find(|(pid, _, iri)| *pid == pack_id && iri == source_iri)
            .map(|(_, term_id, _)| *term_id))
    }

    async fn pack_attachment_counts(
        &self,
        pack_id: Uuid,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        let pack_terms = self.pack_terms.lock().unwrap();
        let attachments = self.term_attachments.lock().unwrap();
        Ok(pack_terms
            .iter()
            .filter(|(pid, _, _)| *pid == pack_id)
            .filter_map(|(_, term_id, iri)| {
                let count = attachments
                    .iter()
                    .filter(|(attached_term, _)| attached_term == term_id)
                    .count();
                (count > 0).then(|| (iri.clone(), i64::try_from(count).unwrap_or(i64::MAX)))
            })
            .collect())
    }

    async fn exact_match_targets_outside_pack(
        &self,
        pack_id: Uuid,
    ) -> Result<Vec<String>, StorageError> {
        use graph_owl_core::glossary::SkosRelation;
        let pack_terms = self.pack_terms.lock().unwrap();
        let other_term_ids: std::collections::HashSet<Uuid> = pack_terms
            .iter()
            .filter(|(pid, _, _)| *pid != pack_id)
            .map(|(_, term_id, _)| *term_id)
            .collect();
        Ok(self
            .term_relations
            .lock()
            .unwrap()
            .iter()
            .filter(|(owner, _)| other_term_ids.contains(owner))
            .filter_map(|(_, relation)| match relation {
                SkosRelation::ExactMatch(target) => Some(target.clone()),
                _ => None,
            })
            .collect())
    }

    async fn insert_pack_override(
        &self,
        override_: graph_owl_ontology::pack::PackOverride,
    ) -> Result<graph_owl_ontology::pack::PackOverride, StorageError> {
        self.pack_overrides.lock().unwrap().push(override_.clone());
        Ok(override_)
    }

    async fn list_pack_overrides(
        &self,
        pack_id: Uuid,
    ) -> Result<Vec<graph_owl_ontology::pack::PackOverride>, StorageError> {
        Ok(self
            .pack_overrides
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.pack_id == pack_id)
            .cloned()
            .collect())
    }

    async fn overrides_for_term_path(
        &self,
        pack_id: Uuid,
        term_path: &str,
    ) -> Result<Vec<graph_owl_ontology::pack::PackOverride>, StorageError> {
        Ok(self
            .pack_overrides
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.pack_id == pack_id && o.term_path == term_path)
            .cloned()
            .collect())
    }

    async fn delete_pack_override(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut held = self.pack_overrides.lock().unwrap();
        let before = held.len();
        held.retain(|o| o.id != id);
        Ok(held.len() < before)
    }

    async fn insert_thread(
        &self,
        thread: graph_owl_core::collaboration::Thread,
    ) -> Result<graph_owl_core::collaboration::Thread, StorageError> {
        self.threads.lock().unwrap().push(thread.clone());
        Ok(thread)
    }

    async fn get_thread(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Thread>, StorageError> {
        Ok(self
            .threads
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn list_threads(
        &self,
        about: Uuid,
        resolved: Option<bool>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Thread>, i64), StorageError> {
        let mut matching: Vec<_> = self
            .threads
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.about == about)
            .filter(|t| resolved.is_none_or(|want| t.resolved == want))
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    async fn insert_post(
        &self,
        post: graph_owl_core::collaboration::Post,
    ) -> Result<graph_owl_core::collaboration::Post, StorageError> {
        self.posts.lock().unwrap().push(post.clone());
        Ok(post)
    }

    async fn get_post(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Post>, StorageError> {
        Ok(self
            .posts
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    async fn list_posts(
        &self,
        thread_id: Uuid,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Post>, i64), StorageError> {
        let mut matching: Vec<_> = self
            .posts
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.thread_id == thread_id)
            .cloned()
            .collect();
        matching.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    async fn update_post(
        &self,
        id: Uuid,
        message: &str,
        edited_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<graph_owl_core::collaboration::Post>, StorageError> {
        let mut held = self.posts.lock().unwrap();
        let Some(post) = held.iter_mut().find(|p| p.id == id) else {
            return Ok(None);
        };
        post.message = message.to_string();
        post.edited_at = Some(edited_at);
        Ok(Some(post.clone()))
    }

    async fn delete_post(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut held = self.posts.lock().unwrap();
        let Some(post) = held.iter_mut().find(|p| p.id == id) else {
            return Ok(false);
        };
        post.deleted = true;
        Ok(true)
    }

    async fn resolve_thread(
        &self,
        id: Uuid,
        resolved_by: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<graph_owl_core::collaboration::Thread>, StorageError> {
        let mut held = self.threads.lock().unwrap();
        let Some(thread) = held.iter_mut().find(|t| t.id == id) else {
            return Ok(None);
        };
        thread.resolved = true;
        thread.resolved_by = Some(resolved_by.to_string());
        thread.resolved_at = Some(at);
        Ok(Some(thread.clone()))
    }

    async fn reopen_thread(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Thread>, StorageError> {
        let mut held = self.threads.lock().unwrap();
        let Some(thread) = held.iter_mut().find(|t| t.id == id) else {
            return Ok(None);
        };
        thread.resolved = false;
        thread.resolved_by = None;
        thread.resolved_at = None;
        Ok(Some(thread.clone()))
    }

    async fn unresolved_thread_count(&self, about: Uuid) -> Result<i64, StorageError> {
        let count = self
            .threads
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.about == about && !t.resolved)
            .count();
        Ok(i64::try_from(count).unwrap_or(i64::MAX))
    }

    async fn insert_change_proposal(
        &self,
        proposal: graph_owl_core::collaboration::Proposal,
    ) -> Result<graph_owl_core::collaboration::Proposal, StorageError> {
        self.change_proposals.lock().unwrap().push(proposal.clone());
        Ok(proposal)
    }

    async fn get_change_proposal(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Proposal>, StorageError> {
        Ok(self
            .change_proposals
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == id)
            .cloned())
    }

    async fn list_change_proposals_for_entity(
        &self,
        about: Uuid,
        status: Option<graph_owl_core::collaboration::ProposalStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Proposal>, i64), StorageError> {
        let mut matching: Vec<_> = self
            .change_proposals
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.about == about)
            .filter(|p| status.is_none_or(|want| p.status == want))
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    async fn list_change_proposals_by_user(
        &self,
        proposed_by: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Proposal>, i64), StorageError> {
        let mut matching: Vec<_> = self
            .change_proposals
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.proposed_by == proposed_by)
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    async fn list_change_proposals(
        &self,
        status: Option<graph_owl_core::collaboration::ProposalStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Proposal>, i64), StorageError> {
        let mut matching: Vec<_> = self
            .change_proposals
            .lock()
            .unwrap()
            .iter()
            .filter(|p| status.is_none_or(|want| p.status == want))
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    async fn decide_change_proposal(
        &self,
        id: Uuid,
        status: graph_owl_core::collaboration::ProposalStatus,
        decided_by: &str,
        decided_at: chrono::DateTime<chrono::Utc>,
        decision_reason: Option<String>,
    ) -> Result<Option<graph_owl_core::collaboration::Proposal>, StorageError> {
        use graph_owl_core::collaboration::ProposalStatus;
        let mut held = self.change_proposals.lock().unwrap();
        let Some(proposal) = held.iter_mut().find(|p| p.id == id) else {
            return Ok(None);
        };
        if proposal.status != ProposalStatus::Pending {
            return Ok(Some(proposal.clone()));
        }
        proposal.status = status;
        proposal.decided_by = Some(decided_by.to_string());
        proposal.decided_at = Some(decided_at);
        proposal.decision_reason = decision_reason;
        Ok(Some(proposal.clone()))
    }

    async fn insert_announcement(
        &self,
        announcement: graph_owl_core::collaboration::Announcement,
    ) -> Result<graph_owl_core::collaboration::Announcement, StorageError> {
        self.announcements
            .lock()
            .unwrap()
            .push(announcement.clone());
        Ok(announcement)
    }

    async fn active_announcements(
        &self,
        about_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<graph_owl_core::collaboration::Announcement>, StorageError> {
        Ok(self
            .announcements
            .lock()
            .unwrap()
            .iter()
            .filter(|a| about_ids.contains(&a.about))
            .filter(|a| graph_owl_core::collaboration::is_active(now, a.starts_at, a.ends_at))
            .cloned()
            .collect())
    }

    async fn list_announcements(
        &self,
        about: Uuid,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Announcement>, i64), StorageError> {
        let mut matching: Vec<_> = self
            .announcements
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.about == about)
            .cloned()
            .collect();
        matching.sort_by_key(|b| std::cmp::Reverse(b.starts_at));
        let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
        let page = matching.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    async fn has_reacted(
        &self,
        post_id: Uuid,
        user_id: &str,
        kind: graph_owl_core::collaboration::ReactionKind,
    ) -> Result<bool, StorageError> {
        Ok(self
            .reactions
            .lock()
            .unwrap()
            .iter()
            .any(|(p, u, k)| *p == post_id && u == user_id && *k == kind))
    }

    async fn add_reaction(
        &self,
        post_id: Uuid,
        user_id: &str,
        kind: graph_owl_core::collaboration::ReactionKind,
    ) -> Result<(), StorageError> {
        let mut held = self.reactions.lock().unwrap();
        if !held
            .iter()
            .any(|(p, u, k)| *p == post_id && u == user_id && *k == kind)
        {
            held.push((post_id, user_id.to_string(), kind));
        }
        Ok(())
    }

    async fn remove_reaction(
        &self,
        post_id: Uuid,
        user_id: &str,
        kind: graph_owl_core::collaboration::ReactionKind,
    ) -> Result<bool, StorageError> {
        let mut held = self.reactions.lock().unwrap();
        let before = held.len();
        held.retain(|(p, u, k)| !(*p == post_id && u == user_id && *k == kind));
        Ok(held.len() < before)
    }

    async fn reaction_counts(
        &self,
        post_id: Uuid,
    ) -> Result<Vec<(graph_owl_core::collaboration::ReactionKind, i64)>, StorageError> {
        use graph_owl_core::collaboration::ReactionKind;
        let held = self.reactions.lock().unwrap();
        let mut counts: Vec<(ReactionKind, i64)> = Vec::new();
        for (_, _, kind) in held.iter().filter(|(p, _, _)| *p == post_id) {
            if let Some(entry) = counts.iter_mut().find(|(k, _)| k == kind) {
                entry.1 += 1;
            } else {
                counts.push((*kind, 1));
            }
        }
        Ok(counts)
    }

    async fn collaboration_activity_for_entity(
        &self,
        about: Uuid,
        limit: usize,
    ) -> Result<Vec<graph_owl_storage::ActivityRow>, StorageError> {
        use graph_owl_core::collaboration::ActivityKind;
        let mut items = Vec::new();

        let threads = self.threads.lock().unwrap();
        for thread in threads.iter().filter(|t| t.about == about) {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::ThreadStarted,
                occurred_at: thread.created_at,
                id: thread.id,
                actor: thread.created_by.clone(),
                summary: thread
                    .field
                    .clone()
                    .unwrap_or_else(|| "general".to_string()),
            });
            if let (true, Some(resolved_at)) = (thread.resolved, thread.resolved_at) {
                items.push(graph_owl_storage::ActivityRow {
                    kind: ActivityKind::ThreadResolved,
                    occurred_at: resolved_at,
                    id: thread.id,
                    actor: thread.resolved_by.clone().unwrap_or_default(),
                    summary: "resolved".to_string(),
                });
            }
        }
        let thread_ids: std::collections::HashSet<Uuid> = threads
            .iter()
            .filter(|t| t.about == about)
            .map(|t| t.id)
            .collect();
        drop(threads);

        for post in self
            .posts
            .lock()
            .unwrap()
            .iter()
            .filter(|p| thread_ids.contains(&p.thread_id) && !p.deleted)
        {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::PostAdded,
                occurred_at: post.created_at,
                id: post.id,
                actor: post.author.clone(),
                summary: post.message.chars().take(120).collect(),
            });
        }

        for proposal in self
            .change_proposals
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.about == about)
        {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::ProposalCreated,
                occurred_at: proposal.created_at,
                id: proposal.id,
                actor: proposal.proposed_by.clone(),
                summary: proposal.field.clone(),
            });
            if let Some(decided_at) = proposal.decided_at {
                items.push(graph_owl_storage::ActivityRow {
                    kind: ActivityKind::ProposalDecided,
                    occurred_at: decided_at,
                    id: proposal.id,
                    actor: proposal.decided_by.clone().unwrap_or_default(),
                    summary: proposal.status.as_str().to_string(),
                });
            }
        }

        for announcement in self
            .announcements
            .lock()
            .unwrap()
            .iter()
            .filter(|a| a.about == about)
        {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::AnnouncementCreated,
                occurred_at: announcement.created_at,
                id: announcement.id,
                actor: announcement.created_by.clone(),
                summary: announcement.message.chars().take(120).collect(),
            });
        }

        items.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at).then(b.id.cmp(&a.id)));
        items.truncate(limit);
        Ok(items)
    }

    async fn update_custom_property(
        &self,
        id: Uuid,
        property: &graph_owl_core::custom_property::CustomProperty,
        previous_name: &str,
    ) -> Result<bool, StorageError> {
        self.guard_write("update_custom_property");
        let mut held = self.custom_properties.lock().unwrap();
        if held.iter().any(|(other, existing)| {
            *other != id
                && existing.name == property.name
                && existing.entity_type == property.entity_type
        }) {
            return Err(StorageError::Conflict {
                detail: format!(
                    "`{}` is already defined on `{}`",
                    property.name, property.entity_type
                ),
                existing_id: None,
                kind: graph_owl_storage::ConflictKind::CustomPropertyExists,
            });
        }
        let Some(slot) = held.iter_mut().find(|(held_id, _)| *held_id == id) else {
            return Ok(false);
        };
        slot.1 = property.clone();
        drop(held);

        // The key migration, which is the half a double is most tempted to
        // skip — and skipping it would let a rename pass every test here
        // while orphaning every value in Postgres.
        if previous_name != property.name {
            let mut assets = self.assets.lock().expect("lock");
            for asset in assets
                .iter_mut()
                .filter(|asset| asset.kind.as_str() == property.entity_type)
            {
                if let Some(bag) = asset.extension.as_mut()
                    && let Some(value) = bag.remove(previous_name)
                {
                    bag.insert(property.name.clone(), value);
                }
            }
        }
        Ok(true)
    }

    async fn force_delete_custom_property(
        &self,
        id: Uuid,
        entity_type: &str,
        name: &str,
        updated_by: &str,
    ) -> Result<i64, StorageError> {
        self.guard_write("force_delete_custom_property");
        {
            let mut held = self.custom_properties.lock().unwrap();
            held.retain(|(held_id, _)| *held_id != id);
        }
        let mut assets = self.assets.lock().expect("lock");
        let mut changed = 0_i64;
        for asset in assets
            .iter_mut()
            .filter(|asset| asset.kind.as_str() == entity_type)
        {
            let removed = asset
                .extension
                .as_mut()
                .is_some_and(|bag| bag.remove(name).is_some());
            if removed {
                // The version bump is the point of the operation being
                // transactional rather than a bulk strip, so a double that
                // dropped it would make the auditability untestable here.
                asset.version = asset
                    .version
                    .bump(graph_owl_core::envelope::ChangeKind::Minor);
                asset.updated_by = updated_by.to_string();
                asset.updated_at = Utc::now();
                changed += 1;
            }
        }
        Ok(changed)
    }

    async fn find_extraction_run_by_id(
        &self,
        run_id: Uuid,
    ) -> Result<Option<graph_owl_storage::ExtractionRunRecord>, StorageError> {
        let runs = self.extraction_runs.lock().unwrap();
        Ok(runs.iter().find(|run| run.id == run_id).cloned())
    }

    async fn save_extraction_run(
        &self,
        run: &graph_owl_storage::ExtractionRunRecord,
        queued: &[graph_owl_storage::QueuedClaimRecord],
        discarded: &[graph_owl_storage::DiscardedClaimRecord],
    ) -> Result<(), StorageError> {
        self.guard_write("save_extraction_run");
        self.extraction_runs.lock().unwrap().push(run.clone());
        self.extraction_claims
            .lock()
            .unwrap()
            .extend(queued.iter().cloned());
        self.extraction_discards
            .lock()
            .unwrap()
            .extend(discarded.iter().cloned());
        Ok(())
    }

    async fn pending_extraction_claims(
        &self,
        limit: i64,
    ) -> Result<Vec<graph_owl_storage::QueuedClaimRecord>, StorageError> {
        let claims = self.extraction_claims.lock().unwrap();
        Ok(claims
            .iter()
            .filter(|claim| claim.state == "pending")
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .cloned()
            .collect())
    }

    async fn decide_extraction_claim(
        &self,
        claim_id: Uuid,
        decision: graph_owl_core::extraction::ReviewDecision,
        decided_by: &str,
    ) -> Result<Option<graph_owl_storage::QueuedClaimRecord>, StorageError> {
        use graph_owl_core::extraction::ReviewDecision;
        let mut claims = self.extraction_claims.lock().unwrap();
        let Some(claim) = claims.iter_mut().find(|claim| claim.id == claim_id) else {
            return Ok(None);
        };
        claim.state = decision.state().to_string();
        claim.decided_by = Some(decided_by.to_string());
        match decision {
            ReviewDecision::Accept => {}
            ReviewDecision::Edit {
                subject,
                predicate,
                object,
            } => {
                claim.subject = subject;
                claim.predicate = predicate;
                claim.object = object;
            }
            ReviewDecision::Reject { reason } => claim.reason = Some(reason),
        }
        Ok(Some(claim.clone()))
    }

    async fn rejected_assertions(&self) -> Result<Vec<(String, String, String)>, StorageError> {
        let claims = self.extraction_claims.lock().unwrap();
        Ok(claims
            .iter()
            .filter(|claim| claim.state == "rejected")
            .map(|claim| {
                (
                    claim.subject.clone(),
                    claim.predicate.clone(),
                    claim.object.clone(),
                )
            })
            .collect())
    }

    async fn delete_extraction_run(&self, run_id: Uuid) -> Result<bool, StorageError> {
        let mut runs = self.extraction_runs.lock().unwrap();
        let before = runs.len();
        runs.retain(|run| run.id != run_id);
        self.extraction_claims
            .lock()
            .unwrap()
            .retain(|claim| claim.run_id != run_id);
        self.extraction_discards
            .lock()
            .unwrap()
            .retain(|discard| discard.run_id != run_id);
        Ok(runs.len() < before)
    }

    // ---- Epic 32: agent capabilities ----
    //
    // The facade tests that exercise the gate use this double, so these are
    // real in-memory implementations rather than `todo!()` — a gate whose
    // storage panics is a gate no test can reach.

    async fn upsert_agent_grant(
        &self,
        grant: &graph_owl_authz::agent::AgentGrant,
    ) -> Result<(), StorageError> {
        self.agent_grants
            .lock()
            .expect("lock")
            .insert(grant.agent.id.clone(), grant.clone());
        Ok(())
    }

    async fn agent_grant(
        &self,
        agent_id: &str,
    ) -> Result<Option<graph_owl_authz::agent::AgentGrant>, StorageError> {
        Ok(self
            .agent_grants
            .lock()
            .expect("lock")
            .get(agent_id)
            .cloned())
    }

    async fn list_agent_grants(
        &self,
    ) -> Result<Vec<graph_owl_authz::agent::AgentGrant>, StorageError> {
        Ok(self
            .agent_grants
            .lock()
            .expect("lock")
            .values()
            .cloned()
            .collect())
    }

    async fn revoke_agent_grant(&self, agent_id: &str) -> Result<bool, StorageError> {
        Ok(self
            .agent_grants
            .lock()
            .expect("lock")
            .remove(agent_id)
            .is_some())
    }

    async fn create_proposal(
        &self,
        proposal: &graph_owl_authz::agent::Proposal,
    ) -> Result<(), StorageError> {
        self.proposals.lock().expect("lock").push(proposal.clone());
        Ok(())
    }

    async fn get_proposal(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_authz::agent::Proposal>, StorageError> {
        Ok(self
            .proposals
            .lock()
            .expect("lock")
            .iter()
            .find(|proposal| proposal.id == id)
            .cloned())
    }

    async fn list_proposals(
        &self,
        agent_id: Option<&str>,
        status: Option<graph_owl_authz::agent::ProposalStatus>,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_authz::agent::Proposal>, StorageError> {
        let mut found: Vec<graph_owl_authz::agent::Proposal> = self
            .proposals
            .lock()
            .expect("lock")
            .iter()
            .filter(|proposal| agent_id.is_none_or(|id| proposal.proposed_by.id == id))
            .filter(|proposal| status.is_none_or(|want| proposal.status == want))
            .cloned()
            .collect();
        found.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(Page::from_overfetch(found, page.limit, |p| {
            Cursor::new(p.created_at.to_string(), p.id)
        }))
    }

    async fn decide_proposal(
        &self,
        id: Uuid,
        status: graph_owl_authz::agent::ProposalStatus,
        decided_by: &str,
    ) -> Result<bool, StorageError> {
        let mut proposals = self.proposals.lock().expect("lock");
        let Some(proposal) = proposals.iter_mut().find(|proposal| proposal.id == id) else {
            return Ok(false);
        };
        // The same guard the SQL has: deciding twice is a conflict, not an
        // update, so a double-decide must be observably refused here too or
        // the double keeps a bug from ever reaching a test.
        if proposal.status != graph_owl_authz::agent::ProposalStatus::Open {
            return Ok(false);
        }
        proposal.status = status;
        proposal.decided_by = Some(decided_by.to_string());
        proposal.decided_at = Some(Utc::now());
        Ok(true)
    }

    async fn record_agent_activity(
        &self,
        activity: &graph_owl_authz::agent::AgentActivity,
    ) -> Result<(), StorageError> {
        self.agent_activity
            .lock()
            .expect("lock")
            .push(activity.clone());
        Ok(())
    }

    async fn agent_activity(
        &self,
        agent_id: &str,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_authz::agent::AgentActivity>, StorageError> {
        let mut found: Vec<graph_owl_authz::agent::AgentActivity> = self
            .agent_activity
            .lock()
            .expect("lock")
            .iter()
            .filter(|activity| activity.agent_id == agent_id)
            .cloned()
            .collect();
        found.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
        Ok(Page::from_overfetch(found, page.limit, |a| {
            Cursor::new(a.at.to_string(), a.id)
        }))
    }

    async fn agent_writes_in_window(
        &self,
        agent_id: &str,
        capability: graph_owl_authz::agent::AgentCapability,
        window_seconds: u32,
    ) -> Result<(u32, Option<u64>), StorageError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(i64::from(window_seconds));
        let activity = self.agent_activity.lock().expect("lock");
        // Refusals do not consume budget — the same rule the SQL applies,
        // stated here too so the double cannot disagree with production
        // about the one thing this function decides.
        let inside: Vec<&graph_owl_authz::agent::AgentActivity> = activity
            .iter()
            .filter(|a| {
                a.agent_id == agent_id
                    && a.capability == capability
                    && a.outcome != graph_owl_authz::agent::ActivityOutcome::Refused
                    && a.at > cutoff
            })
            .collect();
        let oldest = inside
            .iter()
            .map(|a| a.at)
            .min()
            .and_then(|at| u64::try_from((Utc::now() - at).num_seconds()).ok());
        Ok((u32::try_from(inside.len()).unwrap_or(u32::MAX), oldest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn asset(name: &str, fqn: &str) -> Asset {
        let now = Utc::now();
        Asset {
            id: Uuid::new_v4(),
            kind: AssetKind::Service,
            name: name.to_string(),
            fully_qualified_name: fqn.to_string(),
            parent_id: None,
            description: None,
            properties: None,
            owners: Vec::new(),
            version: EntityVersion::initial(),
            updated_by: "system".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            extension: None,
            lifecycle: graph_owl_core::lifecycle::LifecycleState::default(),
            deprecation: None,
        }
    }

    mod capacity {
        use super::*;

        #[tokio::test]
        async fn an_unbounded_store_admits_as_many_assets_as_written() {
            let storage = InMemoryStorage::default();

            for n in 0..50 {
                storage
                    .upsert_asset(asset(&format!("svc{n}"), &format!("svc.{n}")))
                    .await
                    .expect("unbounded store should never refuse a new asset");
            }
        }

        #[tokio::test]
        async fn a_bounded_store_admits_new_assets_up_to_its_limit() {
            let storage = InMemoryStorage::bounded(2);

            storage
                .upsert_asset(asset("a", "svc.a"))
                .await
                .expect("first of two should be admitted");
            storage
                .upsert_asset(asset("b", "svc.b"))
                .await
                .expect("second of two should be admitted");
        }

        #[tokio::test]
        async fn a_bounded_store_refuses_a_new_asset_once_full() {
            let storage = InMemoryStorage::bounded(2);
            storage
                .upsert_asset(asset("a", "svc.a"))
                .await
                .expect("first should be admitted");
            storage
                .upsert_asset(asset("b", "svc.b"))
                .await
                .expect("second should be admitted");

            let refused = storage.upsert_asset(asset("c", "svc.c")).await;

            assert!(
                refused.is_err(),
                "a third asset must be refused once the store holds its limit"
            );
        }

        #[tokio::test]
        async fn a_bounded_store_still_admits_an_update_to_an_existing_asset_once_full() {
            // The bound caps *new* assets, not writes in general — an update
            // to an asset already held does not grow the collection, so
            // refusing it would make a full store unable to correct a typo
            // in the one row it already has.
            let storage = InMemoryStorage::bounded(1);
            let original = storage
                .upsert_asset(asset("a", "svc.a"))
                .await
                .expect("first should be admitted");

            let mut renamed = original.clone();
            renamed.name = "a-renamed".to_string();
            let updated = storage
                .upsert_asset(renamed)
                .await
                .expect("updating the one asset already held must not be refused");

            assert_eq!(updated.name, "a-renamed");
        }
    }

    mod concurrency {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn concurrent_writers_all_land_without_losing_or_corrupting_any() {
            let storage = Arc::new(InMemoryStorage::default());
            let writers = (0..100).map(|n| {
                let storage = Arc::clone(&storage);
                tokio::spawn(async move {
                    storage
                        .upsert_asset(asset(&format!("svc{n}"), &format!("svc.{n}")))
                        .await
                        .expect("a concurrent write should never be lost or fail")
                })
            });

            for writer in writers {
                writer.await.expect("writer task should not panic");
            }

            let page = PageRequest::new(Some(100), None).expect("a page wide enough for all 100");
            let all = storage
                .list_assets(None, &page)
                .await
                .expect("list_assets should succeed");
            let distinct: std::collections::HashSet<_> = all.data.iter().map(|a| a.id).collect();

            assert_eq!(
                distinct.len(),
                100,
                "every one of 100 concurrent writers should have landed exactly once"
            );
        }
    }

    mod relationship_listing {
        //! Epic 37b's own export primitive — `list_relationships_for_entity`
        //! needs a starting entity, and a full-catalog export has none.

        use super::*;

        fn relationship(relationship_type: &str) -> Relationship {
            Relationship {
                id: Uuid::new_v4(),
                from_entity_type: "table".to_string(),
                from_entity_id: Uuid::new_v4(),
                relationship_type: relationship_type.to_string(),
                to_entity_type: "table".to_string(),
                to_entity_id: Uuid::new_v4(),
                created_at: Utc::now(),
            }
        }

        #[tokio::test]
        async fn every_relationship_is_returned_across_pages_with_none_repeated_or_dropped() {
            let storage = InMemoryStorage::default();
            let mut created = Vec::new();
            for n in 0..25 {
                let r = storage
                    .create_relationship(relationship(&format!("feeds{n}")))
                    .await
                    .expect("create should succeed");
                created.push(r.id);
            }

            let mut seen = std::collections::HashSet::new();
            let mut after: Option<String> = None;
            loop {
                let page = PageRequest::new(Some(10), after.as_deref()).expect("valid page");
                let result = storage
                    .list_relationships(&page)
                    .await
                    .expect("list should succeed");
                for r in &result.data {
                    assert!(seen.insert(r.id), "{} returned twice", r.id);
                }
                match result.paging.after {
                    Some(next) => after = Some(next),
                    None => break,
                }
            }

            assert_eq!(
                seen,
                created.into_iter().collect(),
                "every created relationship should appear exactly once across all pages"
            );
        }

        #[tokio::test]
        async fn an_empty_store_returns_an_empty_last_page() {
            let storage = InMemoryStorage::default();
            let page = PageRequest::new(None, None).expect("valid page");

            let result = storage
                .list_relationships(&page)
                .await
                .expect("list should succeed");

            assert!(result.data.is_empty());
            assert_eq!(result.paging.after, None);
        }
    }
}
