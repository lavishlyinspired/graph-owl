use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::classification::{Classification, LabelState, LabelType, Tag, TagLabel};
use graph_owl_core::contract::{Contract, ContractBreach, ContractStatus, SchemaChange};
use graph_owl_core::contradiction::{Review, Verdict};
use graph_owl_core::custom_property::CustomProperty;
use graph_owl_core::domain::{DataProduct, Domain, DomainAssignment, domain_fqn};
use graph_owl_core::lifecycle::{Deprecation, LifecycleState};
use graph_owl_core::memory::{Authorship, LinkRelation, Memory, MemoryKind, MemoryLink};
use graph_owl_core::ownership::{EntityReference, OwnerKind, OwnerRef};
use graph_owl_core::usage::{UsageOperation, UsageRollup};
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    envelope::{ChangeDescription, EntityVersion, classify},
    page::{Cursor, Page, PageRequest},
};
use graph_owl_storage::{
    BreachReport, CertificationFilter, ColumnMapping, ConflictKind, DataProductUpdate,
    DiscardedClaimRecord, DomainDeletion, DomainHoldings, DomainUpdate, DriftFilter,
    ExtractionRunRecord, FollowOutcome, Holdings, IdempotencyClaim, IssueOutcome, LabelDecision,
    LabelOutcome, LifecycleOutcome, LineageReconciliation, MembershipRefusal, MemorySearchFilter,
    MemoryWrite, OwnersWrite, PrincipalDeletion, QueuedClaimRecord, ResultIngest, RetractOutcome,
    ReviewQueueFilter, SearchHit, SplitOutcome, Storage, StorageError, StoredCertification,
    StoredCertificationType, StoredContract, StoredTestCase, StoredTestDefinition,
    StoredTestResult, StoredUser, SupersedeOutcome, TagUsage, TestResultWrite, UpdateOutcome,
    UsageIngest, UsageWrite,
};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

/// Every column a `Memory` is rebuilt from, by id. Named once so the shape
/// cannot drift between the single read and the by-subject read.
const MEMORY_COLUMNS: &str = "SELECT id, kind, content, summary, author_kind, author_user_id,
            author_agent_id, author_model, confidence, as_of, supersedes, superseded_by,
            retracted_at, retraction_reason
     FROM memories WHERE id = $1";

/// The wire spelling of a kind.
///
/// A `match` rather than a `Serialize` round-trip through JSON: the column has a
/// `CHECK` listing these exact strings, so a rename that forgets the migration
/// has to fail to compile rather than fail at 3am on the first write.
const fn memory_kind_str(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Rationale => "rationale",
        MemoryKind::Incident => "incident",
        MemoryKind::Decision => "decision",
        MemoryKind::Caveat => "caveat",
        MemoryKind::Investigation => "investigation",
    }
}

/// One mapper for every team read.
///
/// Written when `parent_team_id` was added and three reads each built a `Team` by
/// hand: a new column added to two of them and missed in the third is a team that
/// reports a parent on one endpoint and none on another.
fn team_from_row(row: PgRow) -> graph_owl_storage::Team {
    graph_owl_storage::Team {
        id: row.get("id"),
        display_name: row.get("display_name"),
        description: row.get("description"),
        members: row.get("members"),
        // `get`, not `try_get(..).ok().flatten()`: a query that forgot the column
        // would then report every team as a root, which reads as real data. A
        // panic on a missing column is a bug found in the first test that runs it.
        parent_team_id: row.get("parent_team_id"),
    }
}

/// A principal as the `(user_id, team_id)` pair `asset_owners` stores.
///
/// One place, because getting the pair the wrong way round silently reassigns
/// ownership to a principal of the other kind that happens to share an id.
fn split(principal: &OwnerRef) -> (Option<&str>, Option<&str>) {
    match principal.kind {
        OwnerKind::User => (Some(principal.id.as_str()), None),
        OwnerKind::Team => (None, Some(principal.id.as_str())),
    }
}

const fn verdict_str(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Confirmed => "confirmed",
        Verdict::Dismissed => "dismissed",
    }
}

/// Rebuild a verdict from the column.
///
/// An unrecognised value is an error rather than a default. Defaulting to
/// `Dismissed` would silently hide a pair a reviewer confirmed; defaulting to
/// `Confirmed` would put their name against a judgement they did not make. The
/// `CHECK` means this can only happen across versions, which is exactly when a
/// loud failure is wanted.
fn verdict_from(value: &str) -> Result<Verdict, StorageError> {
    match value {
        "confirmed" => Ok(Verdict::Confirmed),
        "dismissed" => Ok(Verdict::Dismissed),
        other => Err(StorageError::Unexpected(format!(
            "unknown contradiction verdict in storage: {other}"
        ))),
    }
}

const fn relation_str(relation: LinkRelation) -> &'static str {
    match relation {
        LinkRelation::About => "about",
        LinkRelation::Affects => "affects",
        LinkRelation::Evidence => "evidence",
        LinkRelation::Follows => "follows",
        LinkRelation::Contradicts => "contradicts",
        LinkRelation::Mentions => "mentions",
    }
}

/// Rebuild a kind from the column.
///
/// An unrecognised value is an error rather than a default. A row written by a
/// newer version reading back as `Rationale` would silently reclassify somebody's
/// decision, and the `CHECK` means this can only happen across versions — which
/// is exactly when a loud failure is wanted.
fn memory_kind_from(value: &str) -> Result<MemoryKind, StorageError> {
    match value {
        "rationale" => Ok(MemoryKind::Rationale),
        "incident" => Ok(MemoryKind::Incident),
        "decision" => Ok(MemoryKind::Decision),
        "caveat" => Ok(MemoryKind::Caveat),
        "investigation" => Ok(MemoryKind::Investigation),
        other => Err(StorageError::Unexpected(format!(
            "unknown memory kind in storage: {other}"
        ))),
    }
}

fn relation_from(value: &str) -> Result<LinkRelation, StorageError> {
    match value {
        "about" => Ok(LinkRelation::About),
        "affects" => Ok(LinkRelation::Affects),
        "evidence" => Ok(LinkRelation::Evidence),
        "follows" => Ok(LinkRelation::Follows),
        "contradicts" => Ok(LinkRelation::Contradicts),
        "mentions" => Ok(LinkRelation::Mentions),
        other => Err(StorageError::Unexpected(format!(
            "unknown memory link relation in storage: {other}"
        ))),
    }
}

/// Write a memory's links, resolving each target to an asset or a memory.
///
/// Returns `Some((index, target))` for the **first** unresolvable link rather
/// than collecting them all: the client has to fix that one regardless, and
/// reporting four failures when the first is a typo in a copied id is noise.
///
/// Deliberately *not* a `MemoryWrite`: two callers need the same fact in two
/// different outcome shapes, and returning one of them from a shared helper
/// forced the other to unwrap a variant it could never see.
async fn insert_links(
    tx: &mut Transaction<'_, Postgres>,
    memory_id: Uuid,
    links: &[MemoryLink],
) -> Result<Option<(usize, Uuid)>, StorageError> {
    for (index, edge) in links.iter().enumerate() {
        // Which column the target belongs in is a question only the database can
        // answer, and asking it is not wasted work — Slice A requires an
        // unresolvable target to be reported as a client error naming the index,
        // so the lookup is the validation.
        let is_asset: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM assets WHERE id = $1)")
                .bind(edge.target)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let is_memory: bool = if is_asset {
            false
        } else {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM memories WHERE id = $1)")
                .bind(edge.target)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?
        };

        if !is_asset && !is_memory {
            return Ok(Some((index, edge.target)));
        }

        sqlx::query(
            "INSERT INTO memory_links (memory_id, relation, asset_target, memory_target)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(memory_id)
        .bind(relation_str(edge.relation))
        .bind(if is_asset { Some(edge.target) } else { None })
        .bind(if is_asset { None } else { Some(edge.target) })
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    }
    Ok(None)
}

/// A memory's links, from whichever target column holds each one.
async fn read_links(pool: &PgPool, memory_id: Uuid) -> Result<Vec<MemoryLink>, StorageError> {
    let rows: Vec<(String, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT relation, asset_target, memory_target FROM memory_links
         WHERE memory_id = $1 ORDER BY relation, asset_target, memory_target",
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .map_err(|e| StorageError::Unexpected(e.to_string()))?;

    rows.into_iter()
        .map(|(relation, asset_target, memory_target)| {
            // The `CHECK` guarantees exactly one is set, so a row with neither is
            // a corrupt row and not a shape to paper over with a default.
            let target = asset_target.or(memory_target).ok_or_else(|| {
                StorageError::Unexpected(format!("memory link {memory_id} has no target"))
            })?;
            Ok(MemoryLink {
                relation: relation_from(&relation)?,
                target,
            })
        })
        .collect()
}

/// Rebuild a `Memory` from a row plus its links.
///
/// **Constructed field by field rather than through `Memory::new`.** The
/// constructor refuses a memory with no anchor, which is right on the way in and
/// wrong on the way out: a row that somehow lost its anchor must be *readable*
/// so somebody can see and fix it, not unreadable so it becomes invisible — and
/// hiding a row is the failure mode this whole epic is against.
fn memory_from_row(row: &PgRow, links: Vec<MemoryLink>) -> Result<Memory, StorageError> {
    let author_kind: String = row.get("author_kind");
    let authorship = match author_kind.as_str() {
        "human" => Authorship::Human {
            // `ON DELETE SET NULL` on the FK: losing the attribution is better
            // than losing the memory, so a deleted person reads back as an
            // unnamed human rather than as an error or as an agent.
            user_id: row
                .get::<Option<String>, _>("author_user_id")
                .unwrap_or_default(),
        },
        "agent" => Authorship::Agent {
            agent_id: row
                .get::<Option<String>, _>("author_agent_id")
                .unwrap_or_default(),
            model: row
                .get::<Option<String>, _>("author_model")
                .unwrap_or_default(),
        },
        other => {
            return Err(StorageError::Unexpected(format!(
                "unknown authorship kind in storage: {other}"
            )));
        }
    };

    Ok(Memory {
        id: row.get("id"),
        kind: memory_kind_from(&row.get::<String, _>("kind"))?,
        content: row.get("content"),
        summary: row.get("summary"),
        authorship,
        confidence: row.get("confidence"),
        links,
        as_of: row.get("as_of"),
        supersedes: row.get("supersedes"),
        superseded_by: row.get("superseded_by"),
        retracted_at: row.get("retracted_at"),
        retraction_reason: row.get("retraction_reason"),
    })
}

mod embedded {
    refinery::embed_migrations!("migrations");
}

const UNIQUE_VIOLATION: &str = "23505";

// Takes PgRow by value so it can be passed directly as a fn pointer to
// Option::map/Iterator::map at both call sites, instead of a wrapping closure.
#[allow(clippy::needless_pass_by_value)]
fn table_from_row(row: PgRow) -> Table {
    Table {
        id: row.get("id"),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        description: row.get("description"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn glossary_from_row(row: PgRow) -> graph_owl_storage::Glossary {
    graph_owl_storage::Glossary {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        fully_qualified_name: row.get("fully_qualified_name"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

const TERM_COLUMNS: &str = "id, glossary_id, name, fully_qualified_name, definition, status,
     synonyms, abbreviations, version_major, version_minor, created_at, updated_at";

#[allow(clippy::needless_pass_by_value)]
fn term_from_row(row: PgRow) -> graph_owl_storage::GlossaryTermRecord {
    graph_owl_storage::GlossaryTermRecord {
        id: row.get("id"),
        glossary_id: row.get("glossary_id"),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        definition: row.get("definition"),
        // The CHECK constraint pins the vocabulary; a value that somehow
        // fails to parse falls back to `Draft` rather than panicking a read.
        status: graph_owl_core::glossary::TermStatus::parse(row.get::<&str, _>("status"))
            .unwrap_or(graph_owl_core::glossary::TermStatus::Draft),
        synonyms: row.get("synonyms"),
        abbreviations: row.get("abbreviations"),
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(1),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(0),
        },
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// `source_assets` is a correlated subquery rather than a join, because a
/// `LEFT JOIN metric_sources` would need every other column in the `GROUP
/// BY` — the same reason `OWNERS_EXPR` is a subquery rather than a join.
const METRIC_SELECT: &str = "SELECT id, name, fully_qualified_name, definition, formula, unit,
     granularity, calculation_type, defined_by, created_at, updated_at,
     COALESCE(
         (SELECT ARRAY_AGG(source_fqn ORDER BY source_fqn)
            FROM metric_sources WHERE metric_id = metrics.id),
         '{}'
     ) AS source_assets
     FROM metrics";

#[allow(clippy::needless_pass_by_value)]
fn metric_from_row(row: PgRow) -> graph_owl_storage::MetricRecord {
    graph_owl_storage::MetricRecord {
        id: row.get("id"),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        definition: row.get("definition"),
        formula: row.get("formula"),
        unit: row.get("unit"),
        granularity: row.get("granularity"),
        // The CHECK constraint pins the vocabulary; a value that somehow
        // fails to parse falls back to `Simple` rather than panicking a read.
        calculation_type: graph_owl_core::metric::CalculationType::parse(
            row.get::<&str, _>("calculation_type"),
        )
        .unwrap_or(graph_owl_core::metric::CalculationType::Simple),
        defined_by: row.get("defined_by"),
        source_assets: row.get("source_assets"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

/// The wire spelling of a relation's kind — the `term_relations.kind` CHECK
/// vocabulary, distinct from [`SkosRelation::predicate`]'s `skos:` form.
const fn relation_kind_str(relation: &graph_owl_core::glossary::SkosRelation) -> &'static str {
    use graph_owl_core::glossary::SkosRelation;
    match relation {
        SkosRelation::Broader(_) => "broader",
        SkosRelation::Narrower(_) => "narrower",
        SkosRelation::Related(_) => "related",
        SkosRelation::ExactMatch(_) => "exactMatch",
        SkosRelation::CloseMatch(_) => "closeMatch",
    }
}

/// The inverse of [`relation_kind_str`]. `None` for a kind the CHECK
/// constraint would never let through — a row this crate wrote should never
/// fail this, so it is a defensive `Option` rather than a panic.
fn relation_from_kind(
    kind: &str,
    target: String,
) -> Option<graph_owl_core::glossary::SkosRelation> {
    use graph_owl_core::glossary::SkosRelation;
    match kind {
        "broader" => Some(SkosRelation::Broader(target)),
        "narrower" => Some(SkosRelation::Narrower(target)),
        "related" => Some(SkosRelation::Related(target)),
        "exactMatch" => Some(SkosRelation::ExactMatch(target)),
        "closeMatch" => Some(SkosRelation::CloseMatch(target)),
        _ => None,
    }
}

/// Splits a [`graph_owl_ontology::pack::Licence`] into `ontology_packs`'
/// four licence columns.
fn licence_columns(
    licence: &graph_owl_ontology::pack::Licence,
) -> (&'static str, &str, Option<&str>, Option<&str>) {
    use graph_owl_ontology::pack::Licence;
    match licence {
        Licence::Permissive { name } => ("permissive", name.as_str(), None, None),
        Licence::AttributionRequired { name, notice } => (
            "attributionRequired",
            name.as_str(),
            Some(notice.as_str()),
            None,
        ),
        Licence::LicenceRequired { name, contact } => (
            "licenceRequired",
            name.as_str(),
            None,
            Some(contact.as_str()),
        ),
    }
}

fn licence_from_columns(
    kind: &str,
    name: String,
    notice: Option<String>,
    contact: Option<String>,
) -> Result<graph_owl_ontology::pack::Licence, StorageError> {
    use graph_owl_ontology::pack::Licence;
    match kind {
        "permissive" => Ok(Licence::Permissive { name }),
        "attributionRequired" => Ok(Licence::AttributionRequired {
            name,
            notice: notice.unwrap_or_default(),
        }),
        "licenceRequired" => Ok(Licence::LicenceRequired {
            name,
            contact: contact.unwrap_or_default(),
        }),
        other => Err(StorageError::Unexpected(format!(
            "unknown licence kind '{other}' in ontology_packs"
        ))),
    }
}

fn pack_from_row(row: PgRow) -> Result<graph_owl_ontology::pack::OntologyPack, StorageError> {
    let licence = licence_from_columns(
        row.get::<&str, _>("licence_kind"),
        row.get("licence_name"),
        row.get("licence_notice"),
        row.get("licence_contact"),
    )?;
    Ok(graph_owl_ontology::pack::OntologyPack {
        id: row.get("id"),
        pack_id: row.get("pack_id"),
        version: row.get("version"),
        licence,
        source_url: row.get("source_url"),
        glossary_id: row.get("glossary_id"),
        term_count: usize::try_from(row.get::<i32, _>("term_count")).unwrap_or(0),
        imported_at: row.get("imported_at"),
    })
}

const fn override_kind_str(kind: graph_owl_ontology::pack::OverrideKind) -> &'static str {
    use graph_owl_ontology::pack::OverrideKind;
    match kind {
        OverrideKind::Redefine => "redefine",
        OverrideKind::Hide => "hide",
        OverrideKind::AddSynonym => "addSynonym",
        OverrideKind::AddRelation => "addRelation",
    }
}

fn override_kind_from_str(
    value: &str,
) -> Result<graph_owl_ontology::pack::OverrideKind, StorageError> {
    use graph_owl_ontology::pack::OverrideKind;
    match value {
        "redefine" => Ok(OverrideKind::Redefine),
        "hide" => Ok(OverrideKind::Hide),
        "addSynonym" => Ok(OverrideKind::AddSynonym),
        "addRelation" => Ok(OverrideKind::AddRelation),
        other => Err(StorageError::Unexpected(format!(
            "unknown override kind '{other}' in pack_overrides"
        ))),
    }
}

fn pack_override_from_row(
    row: PgRow,
) -> Result<graph_owl_ontology::pack::PackOverride, StorageError> {
    Ok(graph_owl_ontology::pack::PackOverride {
        id: row.get("id"),
        pack_id: row.get("pack_id"),
        term_path: row.get("term_path"),
        kind: override_kind_from_str(row.get::<&str, _>("kind"))?,
        payload: row.get("payload"),
    })
}

fn thread_from_row(row: PgRow) -> graph_owl_core::collaboration::Thread {
    graph_owl_core::collaboration::Thread {
        id: row.get("id"),
        about: row.get("about"),
        field: row.get("field"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        resolved: row.get("resolved"),
        resolved_by: row.get("resolved_by"),
        resolved_at: row.get("resolved_at"),
    }
}

fn post_from_row(row: PgRow) -> graph_owl_core::collaboration::Post {
    graph_owl_core::collaboration::Post {
        id: row.get("id"),
        thread_id: row.get("thread_id"),
        author: row.get("author"),
        message: row.get("message"),
        created_at: row.get("created_at"),
        edited_at: row.get("edited_at"),
        deleted: row.get("deleted"),
    }
}

// Named `change_proposal_*` rather than `proposal_*` — Epic 32 already
// defines `proposal_status_str`/`proposal_status_from_str` for
// `graph_owl_authz::agent::ProposalStatus` in this same module. Same
// collision as `Storage::insert_change_proposal` and friends; see that
// trait method's own comment.
fn change_proposal_status_str(
    status: graph_owl_core::collaboration::ProposalStatus,
) -> &'static str {
    status.as_str()
}

fn change_proposal_status_from_str(
    value: &str,
) -> Result<graph_owl_core::collaboration::ProposalStatus, StorageError> {
    use graph_owl_core::collaboration::ProposalStatus;
    match value {
        "pending" => Ok(ProposalStatus::Pending),
        "accepted" => Ok(ProposalStatus::Accepted),
        "rejected" => Ok(ProposalStatus::Rejected),
        other => Err(StorageError::Unexpected(format!(
            "unknown proposal status '{other}' in proposals"
        ))),
    }
}

fn change_proposal_from_row(
    row: PgRow,
) -> Result<graph_owl_core::collaboration::Proposal, StorageError> {
    Ok(graph_owl_core::collaboration::Proposal {
        id: row.get("id"),
        about: row.get("about"),
        field: row.get("field"),
        current_value: row.get("current_value"),
        proposed_value: row.get("proposed_value"),
        rationale: row.get("rationale"),
        status: change_proposal_status_from_str(row.get::<&str, _>("status"))?,
        proposed_by: row.get("proposed_by"),
        decided_by: row.get("decided_by"),
        decided_at: row.get("decided_at"),
        decision_reason: row.get("decision_reason"),
        created_at: row.get("created_at"),
    })
}

fn announcement_from_row(row: PgRow) -> graph_owl_core::collaboration::Announcement {
    graph_owl_core::collaboration::Announcement {
        id: row.get("id"),
        about: row.get("about"),
        message: row.get("message"),
        starts_at: row.get("starts_at"),
        ends_at: row.get("ends_at"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
    }
}

fn reaction_kind_str(kind: graph_owl_core::collaboration::ReactionKind) -> &'static str {
    kind.as_str()
}

fn reaction_kind_from_str(
    value: &str,
) -> Result<graph_owl_core::collaboration::ReactionKind, StorageError> {
    use graph_owl_core::collaboration::ReactionKind;
    match value {
        "helpful" => Ok(ReactionKind::Helpful),
        "agree" => Ok(ReactionKind::Agree),
        "disagree" => Ok(ReactionKind::Disagree),
        "question" => Ok(ReactionKind::Question),
        other => Err(StorageError::Unexpected(format!(
            "unknown reaction kind '{other}' in reactions"
        ))),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn relationship_from_row(row: PgRow) -> Relationship {
    Relationship {
        id: row.get("id"),
        from_entity_type: row.get("from_entity_type"),
        from_entity_id: row.get("from_entity_id"),
        relationship_type: row.get("relationship_type"),
        to_entity_type: row.get("to_entity_type"),
        to_entity_id: row.get("to_entity_id"),
        created_at: row.get("created_at"),
    }
}

pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    /// The principals allowed to issue a certification type.
    async fn read_issuers(&self, type_id: Uuid) -> Result<Vec<String>, StorageError> {
        sqlx::query_scalar(
            "SELECT principal_id FROM certification_type_issuers WHERE type_id = $1 ORDER BY principal_id",
        )
        .bind(type_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    /// Attach each certification's evidence.
    ///
    /// A second query per row rather than a join: evidence is a small list and
    /// a join would multiply the certification rows by it, which every caller
    /// would then have to un-multiply.
    async fn hydrate_certifications(
        &self,
        rows: &[PgRow],
    ) -> Result<Vec<StoredCertification>, StorageError> {
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: Uuid = row.get("id");
            let evidence: Vec<(String, String)> = sqlx::query_as(
                "SELECT kind, reference FROM certification_evidence
                  WHERE certification_id = $1 ORDER BY kind, reference",
            )
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            out.push(StoredCertification {
                id,
                target_fqn: row.get("target_fqn"),
                type_id: row.get("type_id"),
                type_name: row.get("type_name"),
                issuer: row.get("issuer"),
                criteria: row.get("criteria"),
                issued_at: row.get("issued_at"),
                expires_at: row.get("expires_at"),
                evidence,
            });
        }
        Ok(out)
    }

    /// Advance an asset's version because something *about* it changed.
    ///
    /// A tag is not a column on `assets`, so the ordinary update path never
    /// sees it — but a governance label appearing or vanishing is exactly the
    /// kind of change a consumer watches for, and one that left no version
    /// would be invisible to every one of them.
    async fn bump_asset_version(
        tx: &mut Transaction<'_, Postgres>,
        target_fqn: &str,
        updated_by: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE assets
                SET version_minor = version_minor + 1, updated_by = $2, updated_at = now()
              WHERE fully_qualified_name = $1",
        )
        .bind(target_fqn)
        .bind(updated_by)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    /// One case with its cadence resolved.
    async fn get_test_case(&self, id: Uuid) -> Result<Option<StoredTestCase>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {TEST_CASE_COLUMNS} FROM test_cases c
               LEFT JOIN test_definitions d ON d.id = c.definition_id
              WHERE c.id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.as_ref().map(test_case_from_row))
    }

    /// A contract row plus its consumers, guaranteed columns and SLAs.
    ///
    /// Three follow-up reads rather than a join: a join would multiply the
    /// contract row by each list, and every caller would then have to
    /// un-multiply it.
    async fn hydrate_contract(&self, row: &PgRow) -> Result<Contract, StorageError> {
        let id: Uuid = row.get("id");

        let consumers: Vec<String> = sqlx::query_scalar(
            "SELECT team_id FROM contract_consumers WHERE contract_id = $1 ORDER BY team_id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let column_rows = sqlx::query(
            "SELECT name, data_type, nullable FROM contract_columns
              WHERE contract_id = $1 ORDER BY name",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let sla_rows: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT definition FROM contract_slas WHERE contract_id = $1 ORDER BY id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Contract {
            id,
            name: row.get("name"),
            asset_fqn: row.get("asset_fqn"),
            producer: row.get("producer"),
            consumers,
            schema_guarantee: graph_owl_core::contract::SchemaGuarantee {
                required_columns: column_rows
                    .iter()
                    .map(|c| graph_owl_core::contract::ColumnGuarantee {
                        name: c.get("name"),
                        data_type: c.get("data_type"),
                        nullable: c.get("nullable"),
                    })
                    .collect(),
                allow_additional: row.get("allow_additional"),
            },
            slas: sla_rows
                .into_iter()
                .filter_map(|value| serde_json::from_value(value).ok())
                .collect(),
            compatibility: graph_owl_core::contract::CompatibilityMode::parse(
                row.get::<String, _>("compatibility").as_str(),
            )
            .unwrap_or(graph_owl_core::contract::CompatibilityMode::None),
            status: ContractStatus::parse(row.get::<String, _>("status").as_str())
                .unwrap_or(ContractStatus::Draft),
            version: EntityVersion {
                major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
                minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
            },
            updated_by: row.get("updated_by"),
            change_description: row
                .get::<Option<serde_json::Value>, _>("change_description")
                .and_then(|v| serde_json::from_value(v).ok()),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// The named experts on a domain, ordered as they were given.
    ///
    /// Order is preserved because it is meaningful — the first expert is
    /// usually the one to ask first — and a set would lose it.
    async fn read_experts(&self, domain: Uuid) -> Result<Vec<String>, StorageError> {
        sqlx::query_scalar(
            "SELECT user_id FROM domain_experts WHERE domain_id = $1 ORDER BY ordinal, user_id",
        )
        .bind(domain)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn read_experts_tx(
        tx: &mut Transaction<'_, Postgres>,
        domain: Uuid,
    ) -> Result<Vec<String>, StorageError> {
        sqlx::query_scalar(
            "SELECT user_id FROM domain_experts WHERE domain_id = $1 ORDER BY ordinal, user_id",
        )
        .bind(domain)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    /// **`ON CONFLICT DO NOTHING`, not an error.** Naming the same person twice
    /// in one request is a client's duplicate, not a state worth refusing — the
    /// answer they asked for is "this person is an expert", which is already
    /// true after the first row.
    async fn write_experts(
        tx: &mut Transaction<'_, Postgres>,
        domain: Uuid,
        experts: &[String],
    ) -> Result<(), StorageError> {
        for (ordinal, user) in experts.iter().enumerate() {
            sqlx::query(
                "INSERT INTO domain_experts (domain_id, user_id, ordinal)
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(domain)
            .bind(user)
            .bind(i32::try_from(ordinal).unwrap_or(i32::MAX))
            .execute(&mut **tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns `StorageError::Unexpected` if the connection or migrations fail.
    pub async fn connect(connection_string: &str) -> Result<Self, StorageError> {
        let pool = PgPool::connect(connection_string)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let (mut migration_client, connection) =
            tokio_postgres::connect(connection_string, tokio_postgres::NoTls)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        tokio::spawn(connection);

        embedded::migrations::runner()
            .run_async(&mut migration_client)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Self { pool })
    }

    /// The underlying pool, for tests that need to set up fixture data
    /// beneath what the `Storage` trait exposes (e.g. a bulk load for a
    /// query-plan assertion).
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Recomputes and upserts `asset`'s four blocking keys (Epic 17 Slice B).
    ///
    /// Called from `upsert_asset` itself rather than left for a caller to
    /// remember, so "computed on every write" and "recomputed on rename" are
    /// both true structurally: there is only one write path, and this is
    /// part of it.
    ///
    /// A written `Column` also refreshes its **parent**'s keys, one level
    /// up, because the parent table's column-hash key depends on its
    /// children — this is the one case a plain "recompute the row that was
    /// just written" would miss. The recursion stops there: a table has no
    /// column-hash dependency on *its* parent, so there is nothing further to
    /// refresh.
    async fn recompute_blocking_keys(&self, asset: &Asset) -> Result<(), StorageError> {
        let column_hash = if asset.kind == AssetKind::Table {
            let rows = sqlx::query(
                "SELECT name FROM assets WHERE parent_id = $1 AND kind = 'column' AND NOT deleted",
            )
            .bind(asset.id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            let columns: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();
            graph_owl_core::blocking::column_hash_key(&columns)
        } else {
            graph_owl_core::blocking::column_hash_key(&[])
        };

        let keys = [
            (
                "normalized_fqn",
                graph_owl_core::blocking::normalized_fqn_key(&asset.fully_qualified_name),
            ),
            (
                "name_parent",
                graph_owl_core::blocking::name_parent_key(
                    &asset.name,
                    graph_owl_core::fqn::parent(&asset.fully_qualified_name),
                ),
            ),
            (
                "soundex_name",
                graph_owl_core::blocking::soundex(&asset.name),
            ),
            ("column_hash", column_hash),
        ];

        for (key_type, key_value) in keys {
            sqlx::query(
                "INSERT INTO entity_blocking_keys (asset_id, key_type, key_value)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (asset_id, key_type) DO UPDATE SET key_value = EXCLUDED.key_value",
            )
            .bind(asset.id)
            .bind(key_type)
            .bind(key_value)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        if asset.kind == AssetKind::Column
            && let Some(parent_id) = asset.parent_id
            && let Some(parent) = self.get_asset(parent_id).await?
        {
            Box::pin(self.recompute_blocking_keys(&parent)).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl Storage for PostgresStorage {
    #[tracing::instrument(name = "storage.create_lineage_edge", skip_all)]
    async fn create_lineage_edge(
        &self,
        edge: &graph_owl_core::lineage::LineageEdge,
    ) -> Result<(), StorageError> {
        let result = sqlx::query(
            "INSERT INTO lineage_edges
                 (id, from_asset_id, to_asset_id, relationship, source, query, description,
                  created_by, pipeline_asset_id, openlineage_event_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(edge.id)
        .bind(edge.from_asset_id)
        .bind(edge.to_asset_id)
        .bind(edge.relationship.as_str())
        .bind(edge.details.source.as_str())
        .bind(&edge.details.query)
        .bind(&edge.details.description)
        .bind(&edge.created_by)
        .bind(edge.details.pipeline)
        .bind(&edge.details.openlineage_event_id)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                Err(StorageError::Conflict {
                    detail: format!(
                        "{} already {} {} according to {}",
                        edge.from_asset_id,
                        edge.relationship.as_str(),
                        edge.to_asset_id,
                        edge.details.source.as_str()
                    ),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                })
            }
            Err(e) => Err(StorageError::Unexpected(e.to_string())),
        }
    }

    #[tracing::instrument(name = "storage.delete_lineage_edge", skip_all)]
    async fn delete_lineage_edge(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::lineage::LineageEdge>, StorageError> {
        // `RETURNING`, so the caller can withdraw the matching triple from the
        // graph. A read followed by a delete races with a concurrent delete and
        // projects a retraction for an edge somebody else already removed.
        let row = sqlx::query(
            "DELETE FROM lineage_edges WHERE id = $1
             RETURNING id, from_asset_id, to_asset_id, relationship, source,
                       query, description, created_at, created_by, pipeline_asset_id,
                       openlineage_event_id",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        row.map(|row| {
            let relationship: String = row.get("relationship");
            let source: String = row.get("source");
            Ok(graph_owl_core::lineage::LineageEdge {
                id: row.get("id"),
                from_asset_id: row.get("from_asset_id"),
                to_asset_id: row.get("to_asset_id"),
                relationship: graph_owl_core::relationship_type::RelationshipType::parse(
                    &relationship,
                )
                .map_err(|e| StorageError::Unexpected(format!("unknown relationship {e:?}")))?,
                details: graph_owl_core::lineage::LineageDetails {
                    source: graph_owl_core::lineage::LineageSource::parse(&source).map_err(
                        |e| StorageError::Unexpected(format!("unknown lineage source {e}")),
                    )?,
                    query: row.get("query"),
                    description: row.get("description"),
                    pipeline: row.get("pipeline_asset_id"),
                    openlineage_event_id: row.get("openlineage_event_id"),
                },
                created_at: row.get("created_at"),
                created_by: row.get("created_by"),
            })
        })
        .transpose()
    }

    #[tracing::instrument(name = "storage.lineage_edges_touching", skip_all)]
    async fn lineage_edges_touching(
        &self,
        asset_ids: &[Uuid],
        limit: Option<i64>,
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError> {
        // Always bound, never a conditional `LIMIT` clause — `i64::MAX` for
        // `None` keeps the query text and bind-parameter shape identical
        // for every caller, which is what lets sqlx check it at one place
        // rather than two.
        let rows = sqlx::query(
            "SELECT id, from_asset_id, to_asset_id, relationship, source, query,
                    description, created_at, created_by, pipeline_asset_id,
                    openlineage_event_id
               FROM lineage_edges
              WHERE from_asset_id = ANY($1) OR to_asset_id = ANY($1)
              LIMIT $2",
        )
        .bind(asset_ids)
        .bind(limit.unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        rows.into_iter()
            .map(|row| {
                let relationship: String = row.get("relationship");
                let source: String = row.get("source");
                Ok(graph_owl_core::lineage::LineageEdge {
                    id: row.get("id"),
                    from_asset_id: row.get("from_asset_id"),
                    to_asset_id: row.get("to_asset_id"),
                    // A row whose vocabulary this build does not know is a
                    // storage error, not a silent skip: dropping it would make
                    // a lineage graph quietly incomplete, which is the one
                    // thing a lineage graph must never be.
                    relationship: graph_owl_core::relationship_type::RelationshipType::parse(
                        &relationship,
                    )
                    .map_err(|e| StorageError::Unexpected(format!("unknown relationship {e:?}")))?,
                    details: graph_owl_core::lineage::LineageDetails {
                        source: graph_owl_core::lineage::LineageSource::parse(&source).map_err(
                            |e| StorageError::Unexpected(format!("unknown lineage source {e}")),
                        )?,
                        query: row.get("query"),
                        description: row.get("description"),
                        pipeline: row.get("pipeline_asset_id"),
                        openlineage_event_id: row.get("openlineage_event_id"),
                    },
                    created_at: row.get("created_at"),
                    created_by: row.get("created_by"),
                })
            })
            .collect()
    }

    #[tracing::instrument(name = "storage.lineage_edges_by_pipeline", skip_all)]
    async fn lineage_edges_by_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, from_asset_id, to_asset_id, relationship, source, query,
                    description, created_at, created_by, pipeline_asset_id,
                    openlineage_event_id
               FROM lineage_edges
              WHERE pipeline_asset_id = $1",
        )
        .bind(pipeline_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        rows.into_iter()
            .map(|row| {
                let relationship: String = row.get("relationship");
                let source: String = row.get("source");
                Ok(graph_owl_core::lineage::LineageEdge {
                    id: row.get("id"),
                    from_asset_id: row.get("from_asset_id"),
                    to_asset_id: row.get("to_asset_id"),
                    relationship: graph_owl_core::relationship_type::RelationshipType::parse(
                        &relationship,
                    )
                    .map_err(|e| StorageError::Unexpected(format!("unknown relationship {e:?}")))?,
                    details: graph_owl_core::lineage::LineageDetails {
                        source: graph_owl_core::lineage::LineageSource::parse(&source).map_err(
                            |e| StorageError::Unexpected(format!("unknown lineage source {e}")),
                        )?,
                        query: row.get("query"),
                        description: row.get("description"),
                        pipeline: row.get("pipeline_asset_id"),
                        openlineage_event_id: row.get("openlineage_event_id"),
                    },
                    created_at: row.get("created_at"),
                    created_by: row.get("created_by"),
                })
            })
            .collect()
    }

    #[tracing::instrument(name = "storage.begin_run", skip_all)]
    async fn begin_run(&self, run: &graph_owl_storage::ConnectorRun) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO connector_runs
                 (id, connector, service_name, started_at, triggered_by)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(run.id)
        .bind(&run.connector)
        .bind(&run.service_name)
        .bind(run.started_at)
        .bind(&run.triggered_by)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.finish_run", skip_all)]
    async fn finish_run(&self, run: &graph_owl_storage::ConnectorRun) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE connector_runs
                SET finished_at = $2, created = $3, skipped = $4, failed = $5,
                    deleted = $6, failures = $7, refusal = $8
              WHERE id = $1",
        )
        .bind(run.id)
        .bind(run.finished_at)
        .bind(run.created)
        .bind(run.skipped)
        .bind(run.failed)
        .bind(run.deleted)
        .bind(&run.failures)
        .bind(&run.refusal)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.replace_validation_results", skip_all)]
    async fn replace_validation_results(
        &self,
        computed_at_t: i64,
        results: &[graph_owl_storage::ValidationFinding],
    ) -> Result<(), StorageError> {
        // One transaction, so a failed write leaves the previous results in
        // place. The alternative — delete, then fail to insert — empties the
        // queue and reads to a steward as "everything is fixed".
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query("DELETE FROM validation_results")
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for finding in results {
            sqlx::query(
                "INSERT INTO validation_results
                     (id, computed_at_t, shape, focus_node, path,
                      constraint_kind, severity, message, actual, suggestion)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(finding.id)
            .bind(computed_at_t)
            .bind(&finding.shape)
            .bind(&finding.focus_node)
            .bind(&finding.path)
            .bind(&finding.constraint_kind)
            .bind(&finding.severity)
            .bind(&finding.message)
            .bind(&finding.actual)
            .bind(&finding.suggestion)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        // A pass that found nothing still records *when* it ran. Without this
        // row an empty queue is ambiguous between "clean" and "never
        // validated", and those call for opposite reactions.
        if results.is_empty() {
            sqlx::query(
                "INSERT INTO validation_results
                     (id, computed_at_t, shape, focus_node, constraint_kind,
                      severity, message)
                 VALUES ($1, $2, '', '', '', 'marker', '')",
            )
            .bind(Uuid::new_v4())
            .bind(computed_at_t)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.validation_results", skip_all)]
    async fn validation_results(
        &self,
        filter: &graph_owl_storage::ValidationFilter,
    ) -> Result<(Vec<graph_owl_storage::ValidationFinding>, i64, usize), StorageError> {
        // The marker row is bookkeeping, never a finding — it exists so a clean
        // pass is distinguishable from no pass, and it must not appear in a
        // queue as a violation of nothing.
        let where_clause = "severity <> 'marker'
              AND ($1::TEXT IS NULL OR severity = $1)
              AND ($2::TEXT IS NULL OR shape = $2)
              AND ($3::TEXT IS NULL OR focus_node = $3)";

        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM validation_results WHERE {where_clause}"
        ))
        .bind(&filter.severity)
        .bind(&filter.shape)
        .bind(&filter.focus_node)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // `computed_at_t` comes from any row including the marker, so a clean
        // pass still reports its currency.
        let computed_at_t: Option<i64> =
            sqlx::query_scalar("SELECT MAX(computed_at_t) FROM validation_results")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let rows = sqlx::query(&format!(
            "SELECT id, shape, focus_node, path, constraint_kind, severity,
                    message, actual, suggestion
               FROM validation_results
              WHERE {where_clause}
              -- Worst first, then stable: a queue that reorders between polls
              -- cannot be worked from the top.
              ORDER BY CASE severity
                         WHEN 'violation' THEN 0
                         WHEN 'warning' THEN 1
                         ELSE 2
                       END,
                       focus_node, shape, constraint_kind
              LIMIT $4 OFFSET $5"
        ))
        .bind(&filter.severity)
        .bind(&filter.shape)
        .bind(&filter.focus_node)
        .bind(i64::try_from(filter.limit).unwrap_or(i64::MAX))
        .bind(i64::try_from(filter.offset).unwrap_or(0))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let findings = rows
            .into_iter()
            .map(|row| graph_owl_storage::ValidationFinding {
                id: row.get("id"),
                shape: row.get("shape"),
                focus_node: row.get("focus_node"),
                path: row.get("path"),
                constraint_kind: row.get("constraint_kind"),
                severity: row.get("severity"),
                message: row.get("message"),
                actual: row.get("actual"),
                suggestion: row.get("suggestion"),
            })
            .collect();

        Ok((
            findings,
            computed_at_t.unwrap_or(0),
            usize::try_from(total).unwrap_or(0),
        ))
    }

    #[tracing::instrument(name = "storage.waive_finding", skip_all)]
    async fn waive_finding(&self, waiver: &graph_owl_storage::Waiver) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO validation_waivers
                 (id, shape, focus_node, path, constraint_kind,
                  reason, waived_by, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(waiver.id)
        .bind(&waiver.shape)
        .bind(&waiver.focus_node)
        .bind(&waiver.path)
        .bind(&waiver.constraint_kind)
        .bind(&waiver.reason)
        .bind(&waiver.waived_by)
        .bind(waiver.expires_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| {
            // The unique index is what makes a second waiver impossible;
            // translating it here means the API can say so rather than
            // returning an opaque 500 for a condition a caller can fix.
            if e.as_database_error()
                .is_some_and(|db| db.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: "this finding is already waived".to_string(),
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::WaiverExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })
    }

    #[tracing::instrument(name = "storage.revoke_waiver", skip_all)]
    async fn revoke_waiver(&self, id: Uuid) -> Result<bool, StorageError> {
        sqlx::query("DELETE FROM validation_waivers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.waivers", skip_all)]
    async fn waivers(&self) -> Result<Vec<graph_owl_storage::Waiver>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, shape, focus_node, path, constraint_kind,
                    reason, waived_by, waived_at, expires_at
               FROM validation_waivers
              ORDER BY expires_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::Waiver {
                id: row.get("id"),
                shape: row.get("shape"),
                focus_node: row.get("focus_node"),
                path: row.get("path"),
                constraint_kind: row.get("constraint_kind"),
                reason: row.get("reason"),
                waived_by: row.get("waived_by"),
                waived_at: row.get("waived_at"),
                expires_at: row.get("expires_at"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.upsert_connector_config", skip_all)]
    async fn upsert_connector_config(
        &self,
        config: &graph_owl_storage::ConnectorConfig,
        secret: Option<&str>,
    ) -> Result<(), StorageError> {
        // `COALESCE($5, connector_configs.secret)` is what makes `None` mean
        // "leave it alone". An edit-then-save round trip cannot resend a
        // credential it was never given, and treating absent as "clear it"
        // would break a connector every time somebody renamed its service.
        sqlx::query(
            "INSERT INTO connector_configs (id, connector, service_name, settings, secret)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (connector, service_name) DO UPDATE
                SET settings   = EXCLUDED.settings,
                    secret     = COALESCE(EXCLUDED.secret, connector_configs.secret),
                    updated_at = now()",
        )
        .bind(config.id)
        .bind(&config.connector)
        .bind(&config.service_name)
        .bind(&config.settings)
        .bind(secret)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.connector_configs", skip_all)]
    async fn connector_configs(
        &self,
    ) -> Result<Vec<graph_owl_storage::ConnectorConfig>, StorageError> {
        // **`secret` is not in the SELECT.** The struct has no field for it, so
        // this could not compile if it were — but naming the columns rather than
        // `SELECT *` means a reviewer can see the omission is deliberate.
        let rows = sqlx::query(
            "SELECT id, connector, service_name, settings,
                    (secret IS NOT NULL) AS has_secret
               FROM connector_configs
              ORDER BY connector, service_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::ConnectorConfig {
                id: row.get("id"),
                connector: row.get("connector"),
                service_name: row.get("service_name"),
                settings: row.get("settings"),
                has_secret: row.get("has_secret"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.connector_secret", skip_all)]
    async fn connector_secret(&self, id: Uuid) -> Result<Option<String>, StorageError> {
        // The only place a credential is read. Deliberately its own method so a
        // reviewer auditing where secrets go has one signature to grep for.
        sqlx::query_scalar("SELECT secret FROM connector_configs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(Option::flatten)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.upsert_webhook_endpoint", skip_all)]
    async fn upsert_webhook_endpoint(
        &self,
        endpoint: graph_owl_storage::WebhookEndpoint,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::WebhookEndpoint, StorageError> {
        let (scheme, header, prefix) = scheme_columns(&endpoint.signature_scheme);
        let row = sqlx::query(
            "INSERT INTO webhook_endpoints
                 (id, path, source, scheme, scheme_header, scheme_prefix, mapping, event_filter, enabled, secret, rate_limit_per_minute)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                 path          = EXCLUDED.path,
                 source        = EXCLUDED.source,
                 scheme        = EXCLUDED.scheme,
                 scheme_header = EXCLUDED.scheme_header,
                 scheme_prefix = EXCLUDED.scheme_prefix,
                 mapping       = EXCLUDED.mapping,
                 event_filter  = EXCLUDED.event_filter,
                 enabled       = EXCLUDED.enabled,
                 -- `None` means leave an existing key alone — same reasoning
                 -- as `upsert_connector_config`.
                 secret        = COALESCE($10, webhook_endpoints.secret),
                 rate_limit_per_minute = EXCLUDED.rate_limit_per_minute,
                 updated_at    = now()
             RETURNING id, path, source, scheme, scheme_header, scheme_prefix,
                       mapping, event_filter, enabled, (secret IS NOT NULL) AS has_secret,
                       rate_limit_per_minute, created_at, updated_at",
        )
        .bind(endpoint.id)
        .bind(&endpoint.path)
        .bind(&endpoint.source)
        .bind(scheme)
        .bind(header)
        .bind(prefix)
        .bind(&endpoint.mapping)
        .bind(&endpoint.event_filter)
        .bind(endpoint.enabled)
        .bind(secret)
        .bind(endpoint.rate_limit_per_minute.map(|n| i32::try_from(n).unwrap_or(i32::MAX)))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                StorageError::Conflict {
                    detail: format!("path '{}' is already registered", endpoint.path),
                    existing_id: None,
                    kind: ConflictKind::WebhookPathExists,
                }
            }
            _ => StorageError::Unexpected(e.to_string()),
        })?;
        Ok(webhook_endpoint_from_row(row))
    }

    #[tracing::instrument(name = "storage.get_webhook_endpoint", skip_all)]
    async fn get_webhook_endpoint(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, StorageError> {
        let row = sqlx::query(
            "SELECT id, path, source, scheme, scheme_header, scheme_prefix,
                    mapping, event_filter, enabled, (secret IS NOT NULL) AS has_secret,
                    rate_limit_per_minute, created_at, updated_at
             FROM webhook_endpoints WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(webhook_endpoint_from_row))
    }

    #[tracing::instrument(name = "storage.get_webhook_endpoint_by_path", skip_all)]
    async fn get_webhook_endpoint_by_path(
        &self,
        path: &str,
    ) -> Result<Option<graph_owl_storage::WebhookEndpoint>, StorageError> {
        let row = sqlx::query(
            "SELECT id, path, source, scheme, scheme_header, scheme_prefix,
                    mapping, event_filter, enabled, (secret IS NOT NULL) AS has_secret,
                    rate_limit_per_minute, created_at, updated_at
             FROM webhook_endpoints WHERE path = $1",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(webhook_endpoint_from_row))
    }

    #[tracing::instrument(name = "storage.list_webhook_endpoints", skip_all)]
    async fn list_webhook_endpoints(
        &self,
    ) -> Result<Vec<graph_owl_storage::WebhookEndpoint>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, path, source, scheme, scheme_header, scheme_prefix,
                    mapping, event_filter, enabled, (secret IS NOT NULL) AS has_secret,
                    rate_limit_per_minute, created_at, updated_at
             FROM webhook_endpoints ORDER BY path",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(webhook_endpoint_from_row).collect())
    }

    #[tracing::instrument(name = "storage.webhook_secret", skip_all)]
    async fn webhook_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        // The only place key material is read. Deliberately its own method so
        // a reviewer auditing where secrets go has one signature to grep for.
        sqlx::query_scalar("SELECT secret FROM webhook_endpoints WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(Option::flatten)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.upsert_stream_subscription", skip_all)]
    async fn upsert_stream_subscription(
        &self,
        subscription: graph_owl_storage::StreamSubscription,
        secret: Option<&[u8]>,
    ) -> Result<graph_owl_storage::StreamSubscription, StorageError> {
        let (broker_kind, broker_address, broker_admin_url) = broker_columns(&subscription.broker);
        let (start_position, start_timestamp, start_offset) =
            start_position_columns(subscription.start_position);
        let row = sqlx::query(
            "INSERT INTO stream_subscriptions
                 (id, broker_kind, broker_address, broker_admin_url, topic, consumer_group,
                  mapping, start_position, start_timestamp, start_offset, max_in_flight,
                  poison_threshold, enabled, secret)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                 broker_kind      = EXCLUDED.broker_kind,
                 broker_address   = EXCLUDED.broker_address,
                 broker_admin_url = EXCLUDED.broker_admin_url,
                 topic            = EXCLUDED.topic,
                 consumer_group   = EXCLUDED.consumer_group,
                 mapping          = EXCLUDED.mapping,
                 start_position   = EXCLUDED.start_position,
                 start_timestamp  = EXCLUDED.start_timestamp,
                 start_offset     = EXCLUDED.start_offset,
                 max_in_flight    = EXCLUDED.max_in_flight,
                 poison_threshold = EXCLUDED.poison_threshold,
                 enabled          = EXCLUDED.enabled,
                 -- `None` means leave an existing credential alone — same
                 -- reasoning as `upsert_webhook_endpoint`.
                 secret           = COALESCE($14, stream_subscriptions.secret),
                 updated_at       = now()
             RETURNING id, broker_kind, broker_address, broker_admin_url, topic, consumer_group,
                       mapping, start_position, start_timestamp, start_offset, max_in_flight,
                       poison_threshold, enabled, (secret IS NOT NULL) AS has_secret,
                       created_at, updated_at",
        )
        .bind(subscription.id)
        .bind(broker_kind)
        .bind(broker_address)
        .bind(broker_admin_url)
        .bind(&subscription.topic)
        .bind(&subscription.consumer_group)
        .bind(&subscription.mapping)
        .bind(start_position)
        .bind(start_timestamp)
        .bind(start_offset)
        .bind(i32::try_from(subscription.max_in_flight).unwrap_or(i32::MAX))
        .bind(i32::try_from(subscription.poison_threshold).unwrap_or(i32::MAX))
        .bind(subscription.enabled)
        .bind(secret)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                StorageError::Conflict {
                    detail: format!(
                        "topic '{}' with consumer group '{}' is already registered",
                        subscription.topic, subscription.consumer_group
                    ),
                    existing_id: None,
                    kind: ConflictKind::StreamSubscriptionExists,
                }
            }
            _ => StorageError::Unexpected(e.to_string()),
        })?;
        Ok(stream_subscription_from_row(row))
    }

    #[tracing::instrument(name = "storage.get_stream_subscription", skip_all)]
    async fn get_stream_subscription(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StreamSubscription>, StorageError> {
        let row = sqlx::query(
            "SELECT id, broker_kind, broker_address, broker_admin_url, topic, consumer_group,
                    mapping, start_position, start_timestamp, start_offset, max_in_flight,
                    poison_threshold, enabled, (secret IS NOT NULL) AS has_secret,
                    created_at, updated_at
             FROM stream_subscriptions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(stream_subscription_from_row))
    }

    #[tracing::instrument(name = "storage.list_stream_subscriptions", skip_all)]
    async fn list_stream_subscriptions(
        &self,
    ) -> Result<Vec<graph_owl_storage::StreamSubscription>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, broker_kind, broker_address, broker_admin_url, topic, consumer_group,
                    mapping, start_position, start_timestamp, start_offset, max_in_flight,
                    poison_threshold, enabled, (secret IS NOT NULL) AS has_secret,
                    created_at, updated_at
             FROM stream_subscriptions ORDER BY topic, consumer_group",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(stream_subscription_from_row).collect())
    }

    #[tracing::instrument(name = "storage.stream_subscription_secret", skip_all)]
    async fn stream_subscription_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        // The only place credential material is read — same reasoning as
        // `webhook_secret`.
        sqlx::query_scalar("SELECT secret FROM stream_subscriptions WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(Option::flatten)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.create_stream_dead_letter", skip_all)]
    async fn create_stream_dead_letter(
        &self,
        letter: graph_owl_storage::StreamDeadLetter,
    ) -> Result<graph_owl_storage::StreamDeadLetter, StorageError> {
        let row = sqlx::query(
            "INSERT INTO stream_dead_letters
                 (id, subscription_id, topic, partition, kafka_offset, payload, reason)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, subscription_id, topic, partition, kafka_offset, payload, reason, created_at",
        )
        .bind(letter.id)
        .bind(letter.subscription_id)
        .bind(&letter.topic)
        .bind(letter.partition)
        .bind(letter.offset)
        .bind(&letter.payload)
        .bind(&letter.reason)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(stream_dead_letter_from_row(&row))
    }

    #[tracing::instrument(name = "storage.get_stream_dead_letter", skip_all)]
    async fn get_stream_dead_letter(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::StreamDeadLetter>, StorageError> {
        let row = sqlx::query(
            "SELECT id, subscription_id, topic, partition, kafka_offset, payload, reason, created_at
             FROM stream_dead_letters WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.as_ref().map(stream_dead_letter_from_row))
    }

    #[tracing::instrument(name = "storage.list_stream_dead_letters", skip_all)]
    async fn list_stream_dead_letters(
        &self,
        subscription: Option<Uuid>,
    ) -> Result<Vec<graph_owl_storage::StreamDeadLetter>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, subscription_id, topic, partition, kafka_offset, payload, reason, created_at
             FROM stream_dead_letters
             WHERE ($1::uuid IS NULL OR subscription_id = $1)
             ORDER BY created_at DESC",
        )
        .bind(subscription)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(stream_dead_letter_from_row).collect())
    }

    #[tracing::instrument(name = "storage.delete_stream_dead_letter", skip_all)]
    async fn delete_stream_dead_letter(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM stream_dead_letters WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    // The event row is inserted before the dedup marker — `first_event_id`
    // is a real foreign key, so the row it points to must already exist.
    // Both statements run in one transaction so "concurrent duplicate
    // deliveries produce one effect" is true under real concurrency, not
    // just in a single-threaded test: two transactions racing on the same
    // `(endpoint_id, dedup_key)` serialize on the marker table's primary
    // key, and only the one that wins keeps the caller's `state` — the
    // other's own row (already written) is updated to `Duplicate` before
    // either one commits.
    #[tracing::instrument(name = "storage.create_inbound_event", skip_all)]
    async fn create_inbound_event(
        &self,
        mut event: graph_owl_core::webhook::InboundEvent,
    ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO inbound_events
                 (id, endpoint_id, sender_event_id, sender_timestamp, received_at, raw, state, dedup_key)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(event.id)
        .bind(event.endpoint)
        .bind(&event.sender_event_id)
        .bind(event.sender_timestamp)
        .bind(event.received_at)
        .bind(&event.raw)
        .bind(event_state_str(event.state))
        .bind(&event.dedup_key)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let claimed = sqlx::query(
            "INSERT INTO inbound_event_dedup (endpoint_id, dedup_key, first_event_id)
                 VALUES ($1, $2, $3)
             ON CONFLICT (endpoint_id, dedup_key) DO NOTHING
             RETURNING first_event_id",
        )
        .bind(event.endpoint)
        .bind(&event.dedup_key)
        .bind(event.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if claimed.is_none() {
            event.state = graph_owl_core::webhook::EventState::Duplicate;
            sqlx::query("UPDATE inbound_events SET state = $1 WHERE id = $2")
                .bind(event_state_str(event.state))
                .bind(event.id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(event)
    }

    #[tracing::instrument(name = "storage.get_inbound_event", skip_all)]
    async fn get_inbound_event(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::webhook::InboundEvent>, StorageError> {
        let row = sqlx::query("SELECT * FROM inbound_events WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(inbound_event_from_row).transpose()
    }

    #[tracing::instrument(name = "storage.update_inbound_event_state", skip_all)]
    async fn update_inbound_event_state(
        &self,
        id: Uuid,
        state: graph_owl_core::webhook::EventState,
        reason: Option<&str>,
    ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
        let row = sqlx::query(
            "UPDATE inbound_events SET state = $1, reason = $2 WHERE id = $3 RETURNING *",
        )
        .bind(event_state_str(state))
        .bind(reason)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .ok_or_else(|| StorageError::Unexpected(format!("no inbound event {id}")))?;
        inbound_event_from_row(row)
    }

    #[tracing::instrument(name = "storage.list_dead_letters", skip_all)]
    async fn list_dead_letters(
        &self,
        filter: &graph_owl_storage::DeadLetterFilter,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError> {
        let limit = i64::try_from(filter.limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(filter.offset).unwrap_or(0);
        let rows = sqlx::query(
            "SELECT * FROM inbound_events
             WHERE state = 'failed'
               AND ($1::uuid IS NULL OR endpoint_id = $1)
               AND ($2::text IS NULL OR reason ILIKE '%' || $2 || '%')
             ORDER BY received_at DESC
             LIMIT $3 OFFSET $4",
        )
        .bind(filter.endpoint)
        .bind(&filter.reason_contains)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter().map(inbound_event_from_row).collect()
    }

    #[tracing::instrument(name = "storage.list_inbound_events_in_window", skip_all)]
    async fn list_inbound_events_in_window(
        &self,
        endpoint: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError> {
        // `COALESCE(sender_timestamp, received_at)` is the replay order —
        // an event without a sender timestamp falls back to arrival order,
        // same reasoning as `Freshness::Ambiguous`. The window itself is
        // bounded by `received_at`, which is always populated; a
        // `sender_timestamp` might fall outside `[since, until]` even
        // though the delivery itself landed inside it, and it is the
        // delivery a replay window is scoped to.
        let rows = sqlx::query(
            "SELECT * FROM inbound_events
             WHERE endpoint_id = $1 AND received_at >= $2 AND received_at <= $3
             ORDER BY COALESCE(sender_timestamp, received_at) ASC",
        )
        .bind(endpoint)
        .bind(since)
        .bind(until)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter().map(inbound_event_from_row).collect()
    }

    #[tracing::instrument(name = "storage.purge_dead_letters", skip_all)]
    async fn purge_dead_letters(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StorageError> {
        let result =
            sqlx::query("DELETE FROM inbound_events WHERE state = 'failed' AND received_at < $1")
                .bind(older_than)
                .execute(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected())
    }

    #[tracing::instrument(name = "storage.last_applied_timestamp", skip_all)]
    async fn last_applied_timestamp(
        &self,
        fully_qualified_name: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
        let row = sqlx::query(
            "SELECT sender_timestamp FROM entity_last_applied WHERE fully_qualified_name = $1",
        )
        .bind(fully_qualified_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(|row| row.get("sender_timestamp")))
    }

    #[tracing::instrument(name = "storage.record_applied_timestamp", skip_all)]
    async fn record_applied_timestamp(
        &self,
        fully_qualified_name: &str,
        sender_timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO entity_last_applied (fully_qualified_name, sender_timestamp)
                 VALUES ($1, $2)
             ON CONFLICT (fully_qualified_name) DO UPDATE SET
                 sender_timestamp = EXCLUDED.sender_timestamp,
                 updated_at = now()",
        )
        .bind(fully_qualified_name)
        .bind(sender_timestamp)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    // The next version is computed inside the `INSERT` itself, not read then
    // written: a subquery in the `SELECT` list is one round trip and one
    // statement, so there is no window between reading the current max and
    // writing the new row for a second writer to land in. Admin-only
    // configuration, not a hot path, so this is not wrapped in a
    // serializable transaction on top of that — a genuine simultaneous
    // double-register of the same mapping name is vanishingly unlikely, and
    // the failure mode if it ever happened is a `UNIQUE` violation, not a
    // silently wrong version.
    #[tracing::instrument(name = "storage.upsert_mapping", skip_all)]
    async fn upsert_mapping(
        &self,
        mapping: graph_owl_storage::Mapping,
    ) -> Result<graph_owl_storage::Mapping, StorageError> {
        let kind_expr = serde_json::to_value(&mapping.kind)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let name_expr = serde_json::to_value(&mapping.entity_name)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let parent_fqn_expr = mapping
            .parent_fqn
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let description_expr = mapping
            .description
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let properties_exprs = serde_json::to_value(&mapping.properties)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let row = sqlx::query(
            "INSERT INTO mapping_versions
                 (id, name, version, kind_expr, name_expr, parent_fqn_expr,
                  description_expr, properties_exprs)
             SELECT $1, $2,
                    COALESCE((SELECT MAX(version) FROM mapping_versions WHERE name = $2), 0) + 1,
                    $3, $4, $5, $6, $7
             RETURNING id, name, version, kind_expr, name_expr, parent_fqn_expr,
                       description_expr, properties_exprs, created_at",
        )
        .bind(Uuid::new_v4())
        .bind(&mapping.name)
        .bind(kind_expr)
        .bind(name_expr)
        .bind(parent_fqn_expr)
        .bind(description_expr)
        .bind(properties_exprs)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        mapping_from_row(row)
    }

    #[tracing::instrument(name = "storage.get_mapping", skip_all)]
    async fn get_mapping(
        &self,
        name: &str,
    ) -> Result<Option<graph_owl_storage::Mapping>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, version, kind_expr, name_expr, parent_fqn_expr,
                    description_expr, properties_exprs, created_at
             FROM mapping_versions WHERE name = $1 ORDER BY version DESC LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(mapping_from_row).transpose()
    }

    #[tracing::instrument(name = "storage.list_mapping_versions", skip_all)]
    async fn list_mapping_versions(
        &self,
        name: &str,
    ) -> Result<Vec<graph_owl_storage::Mapping>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, name, version, kind_expr, name_expr, parent_fqn_expr,
                    description_expr, properties_exprs, created_at
             FROM mapping_versions WHERE name = $1 ORDER BY version DESC",
        )
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter().map(mapping_from_row).collect()
    }

    #[tracing::instrument(name = "storage.upsert_team", skip_all)]
    // ---- Epic 31: organizational memory ----

    async fn save_memory(&self, memory: &Memory) -> Result<MemoryWrite, StorageError> {
        // One transaction: a memory whose row was written and whose links were
        // not is an **unanchored** memory — stored, permanently unretrievable,
        // and holding the id somebody was told the write succeeded under. The
        // domain refuses to construct one; the adapter must not create one by
        // failing halfway.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let (author_kind, author_user_id, author_agent_id, author_model) = match &memory.authorship
        {
            Authorship::Human { user_id } => ("human", Some(user_id.clone()), None, None),
            Authorship::Agent { agent_id, model } => {
                ("agent", None, Some(agent_id.clone()), Some(model.clone()))
            }
        };

        sqlx::query(
            "INSERT INTO memories
                (id, kind, content, summary, author_kind, author_user_id,
                 author_agent_id, author_model, confidence, as_of)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(memory.id)
        .bind(memory_kind_str(memory.kind))
        .bind(&memory.content)
        .bind(&memory.summary)
        .bind(author_kind)
        .bind(&author_user_id)
        .bind(&author_agent_id)
        .bind(&author_model)
        .bind(memory.confidence)
        .bind(memory.as_of)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!("memory {} already exists", memory.id),
                    existing_id: Some(memory.id),
                    kind: ConflictKind::MemoryExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        if let Some((index, target)) = insert_links(&mut tx, memory.id, &memory.links).await? {
            // Rolled back explicitly rather than by dropping `tx`: the caller is
            // getting `Ok(UnknownLinkTarget)`, and an implicit rollback on a
            // success-shaped return is the kind of thing a later reader assumes
            // did not happen.
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(MemoryWrite::UnknownLinkTarget { index, target });
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(MemoryWrite::Saved)
    }

    async fn find_memory(&self, id: Uuid) -> Result<Option<Memory>, StorageError> {
        let Some(row) = sqlx::query(MEMORY_COLUMNS)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?
        else {
            return Ok(None);
        };
        let links = read_links(&self.pool, id).await?;
        Ok(Some(memory_from_row(&row, links)?))
    }

    async fn memories_about(
        &self,
        subject: Uuid,
        include_superseded: bool,
    ) -> Result<Vec<Memory>, StorageError> {
        // The subject may be an asset or another memory, and the caller does not
        // know which — the domain's `MemoryLink` carries one id precisely because
        // it does not care. Matching either column keeps that true above the
        // adapter rather than pushing the split upward.
        let rows = sqlx::query(
            "SELECT m.id, m.kind, m.content, m.summary, m.author_kind, m.author_user_id,
                    m.author_agent_id, m.author_model, m.confidence, m.as_of,
                    m.supersedes, m.superseded_by, m.retracted_at, m.retraction_reason
             FROM memories m
             JOIN memory_links l ON l.memory_id = m.id
             WHERE (l.asset_target = $1 OR l.memory_target = $1)
               AND ($2 OR m.superseded_by IS NULL)
             GROUP BY m.id
             ORDER BY m.as_of DESC, m.id",
        )
        .bind(subject)
        .bind(include_superseded)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut memories = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let links = read_links(&self.pool, id).await?;
            memories.push(memory_from_row(row, links)?);
        }
        Ok(memories)
    }

    async fn supersede_memory(
        &self,
        original: Uuid,
        replacement: &Memory,
    ) -> Result<SupersedeOutcome, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // `FOR UPDATE`, so two concurrent corrections cannot both read "not yet
        // superseded" and both write themselves in. The loser gets
        // `AlreadySuperseded` naming the winner, which is exactly what it needs
        // to retry correctly.
        let existing: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT superseded_by FROM memories WHERE id = $1 FOR UPDATE")
                .bind(original)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some((superseded_by,)) = existing else {
            return Ok(SupersedeOutcome::NotFound);
        };
        if let Some(current) = superseded_by {
            return Ok(SupersedeOutcome::AlreadySuperseded { current });
        }

        let (author_kind, author_user_id, author_agent_id, author_model) =
            match &replacement.authorship {
                Authorship::Human { user_id } => ("human", Some(user_id.clone()), None, None),
                Authorship::Agent { agent_id, model } => {
                    ("agent", None, Some(agent_id.clone()), Some(model.clone()))
                }
            };

        sqlx::query(
            "INSERT INTO memories
                (id, kind, content, summary, author_kind, author_user_id,
                 author_agent_id, author_model, confidence, as_of, supersedes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(replacement.id)
        .bind(memory_kind_str(replacement.kind))
        .bind(&replacement.content)
        .bind(&replacement.summary)
        .bind(author_kind)
        .bind(&author_user_id)
        .bind(&author_agent_id)
        .bind(&author_model)
        .bind(replacement.confidence)
        .bind(replacement.as_of)
        .bind(original)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if let Some((index, target)) =
            insert_links(&mut tx, replacement.id, &replacement.links).await?
        {
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            // The same client-fixable condition the create path reports, reported
            // the same way. It previously became an `Unexpected` — a `500` for a
            // body that would have earned a `400` from `POST /memories`.
            return Ok(SupersedeOutcome::UnknownLinkTarget { index, target });
        }

        // The other half. Both or neither — a dangling pair reads as history and
        // is not.
        sqlx::query("UPDATE memories SET superseded_by = $2, updated_at = now() WHERE id = $1")
            .bind(original)
            .bind(replacement.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(SupersedeOutcome::Superseded)
    }

    async fn retract_memory(&self, id: Uuid, reason: &str) -> Result<RetractOutcome, StorageError> {
        let Some(row) = sqlx::query(MEMORY_COLUMNS)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?
        else {
            return Ok(RetractOutcome::NotFound);
        };
        let links = read_links(&self.pool, id).await?;
        let existing = memory_from_row(&row, links)?;
        if existing.is_retracted() {
            return Ok(RetractOutcome::AlreadyRetracted(existing));
        }

        sqlx::query(
            "UPDATE memories SET retracted_at = now(), retraction_reason = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .bind(reason)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let retracted = self.find_memory(id).await?.ok_or_else(|| {
            StorageError::Unexpected("memory vanished mid-retraction".to_string())
        })?;
        Ok(RetractOutcome::Retracted(retracted))
    }

    async fn search_memories(
        &self,
        filter: &MemorySearchFilter,
    ) -> Result<(Vec<Memory>, i64), StorageError> {
        let where_clause = "($1::text IS NULL OR author_user_id = $1 OR author_agent_id = $1)
              AND ($2::double precision IS NULL OR confidence >= $2)
              AND ($3::double precision IS NULL OR confidence <= $3)
              AND ($4::timestamptz IS NULL OR as_of >= $4)
              AND ($5::timestamptz IS NULL OR as_of <= $5)
              AND ($6 OR superseded_by IS NULL)
              AND ($7 OR retracted_at IS NULL)";

        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM memories WHERE {where_clause}"
        ))
        .bind(&filter.author)
        .bind(filter.min_confidence)
        .bind(filter.max_confidence)
        .bind(filter.since)
        .bind(filter.until)
        .bind(filter.include_superseded)
        .bind(filter.include_retracted)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let limit = i64::try_from(filter.limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(filter.offset).unwrap_or(0);
        let rows = sqlx::query(&format!(
            "SELECT id, kind, content, summary, author_kind, author_user_id,
                    author_agent_id, author_model, confidence, as_of,
                    supersedes, superseded_by, retracted_at, retraction_reason
             FROM memories
             WHERE {where_clause}
             ORDER BY as_of DESC, id
             LIMIT $8 OFFSET $9"
        ))
        .bind(&filter.author)
        .bind(filter.min_confidence)
        .bind(filter.max_confidence)
        .bind(filter.since)
        .bind(filter.until)
        .bind(filter.include_superseded)
        .bind(filter.include_retracted)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut memories = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let links = read_links(&self.pool, id).await?;
            memories.push(memory_from_row(row, links)?);
        }
        Ok((memories, total))
    }

    async fn review_contradiction(
        &self,
        review: Review,
        reviewed_by: &str,
        note: Option<&str>,
    ) -> Result<(), StorageError> {
        // Normalised before it is stored. The schema also enforces `a < b`, so
        // this is belt and braces — but the braces matter: the CHECK would turn a
        // reviewer's click into a 500 rather than quietly ordering it.
        let (a, b) = if review.a < review.b {
            (review.a, review.b)
        } else {
            (review.b, review.a)
        };

        sqlx::query(
            "INSERT INTO memory_contradiction_reviews (a, b, verdict, reviewed_by, note)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (a, b) DO UPDATE
                    SET verdict     = EXCLUDED.verdict,
                        reviewed_by = EXCLUDED.reviewed_by,
                        reviewed_at = now(),
                        note        = EXCLUDED.note",
        )
        .bind(a)
        .bind(b)
        .bind(verdict_str(review.verdict))
        .bind(reviewed_by)
        .bind(note)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn contradiction_reviews(&self) -> Result<Vec<Review>, StorageError> {
        let rows: Vec<(Uuid, Uuid, String)> =
            sqlx::query_as("SELECT a, b, verdict FROM memory_contradiction_reviews")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter()
            .map(|(a, b, verdict)| {
                Ok(Review {
                    a,
                    b,
                    verdict: verdict_from(&verdict)?,
                })
            })
            .collect()
    }

    // ---- Epic 11 Slice C: ownership ----

    async fn set_asset_owners(
        &self,
        asset_id: Uuid,
        owners: &[OwnerRef],
    ) -> Result<OwnersWrite, StorageError> {
        // One transaction: an asset whose old owners were deleted and whose new
        // ones failed to write is an asset that silently became unowned, and
        // "unowned" is a state the gap report acts on.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM assets WHERE id = $1)")
            .bind(asset_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if !exists {
            return Ok(OwnersWrite::NotFound);
        }

        // **Every principal is resolved before anything is written**, so a bad
        // owner at index 2 does not leave indexes 0 and 1 applied. Resolution also
        // produces the display name the read path needs, so the lookup is not
        // wasted work.
        let mut resolved = Vec::with_capacity(owners.len());
        for (index, owner) in owners.iter().enumerate() {
            let table = match owner.kind {
                OwnerKind::User => "SELECT display_name FROM users WHERE id = $1",
                OwnerKind::Team => "SELECT display_name FROM teams WHERE id = $1",
            };
            let display_name: Option<String> = sqlx::query_scalar(table)
                .bind(&owner.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            let Some(display_name) = display_name else {
                tx.rollback()
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
                return Ok(OwnersWrite::UnknownPrincipal {
                    index,
                    id: owner.id.clone(),
                });
            };
            resolved.push(EntityReference {
                id: owner.id.clone(),
                kind: owner.kind,
                display_name,
                // A write records ownership *here*, so what comes back is direct
                // by construction. Inheritance is a read-time projection only.
                inherited: false,
            });
        }

        sqlx::query("DELETE FROM asset_owners WHERE asset_id = $1")
            .bind(asset_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for (ordinal, owner) in owners.iter().enumerate() {
            let ordinal = i32::try_from(ordinal).map_err(|_| {
                StorageError::Unexpected("more owners than an asset can have".to_string())
            })?;
            let (user_id, team_id) = match owner.kind {
                OwnerKind::User => (Some(&owner.id), None),
                OwnerKind::Team => (None, Some(&owner.id)),
            };
            sqlx::query(
                "INSERT INTO asset_owners (asset_id, user_id, team_id, ordinal)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(asset_id)
            .bind(user_id)
            .bind(team_id)
            .bind(ordinal)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                // The unique indexes catch "the same principal twice", which is a
                // client mistake rather than an internal failure — but it is not
                // an *index* mistake, so it does not reuse `UnknownPrincipal`.
                if e.as_database_error()
                    .is_some_and(|d| d.is_unique_violation())
                {
                    StorageError::Conflict {
                        detail: format!("{} is listed as an owner more than once", owner.id),
                        existing_id: None,
                        kind: ConflictKind::AssignmentExists,
                    }
                } else {
                    StorageError::Unexpected(e.to_string())
                }
            })?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(OwnersWrite::Set(resolved))
    }

    async fn asset_owners(&self, asset_id: Uuid) -> Result<Vec<EntityReference>, StorageError> {
        // **The same projection the asset read uses**, deliberately. Two reads
        // that disagree about who owns a table is what a console shows a steward,
        // and the second implementation is where the disagreement comes from —
        // so there is only one, and inheritance is correct here for free.
        let owners: Option<serde_json::Value> = sqlx::query_scalar(&format!(
            "SELECT {OWNERS_EXPR} AS owners FROM assets WHERE assets.id = $1"
        ))
        .bind(asset_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // No asset is not an error here: the caller has already established the
        // asset exists, or is asking a question whose honest answer is "nobody".
        let Some(owners) = owners else {
            return Ok(Vec::new());
        };
        serde_json::from_value(owners).map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    // ---- Epic 11 Slices B, F, G ----

    async fn child_teams(&self, id: &str) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
        // Members aggregated in SQL, matching `teams()` and `find_team()` — a loop
        // asking per team would be one round trip per child for data one query
        // already returns.
        let rows = sqlx::query(
            "SELECT t.id, t.display_name, t.description, t.parent_team_id,
                    COALESCE(
                        ARRAY_AGG(m.user_id ORDER BY m.user_id)
                            FILTER (WHERE m.user_id IS NOT NULL),
                        '{}'
                    ) AS members
               FROM teams t
               LEFT JOIN team_members m ON m.team_id = t.id
              WHERE t.parent_team_id = $1
              GROUP BY t.id
              ORDER BY t.id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.into_iter().map(team_from_row).collect())
    }

    async fn would_cycle(&self, team: &str, parent: &str) -> Result<bool, StorageError> {
        // **Walk the proposed parent's ancestry, not just its immediate parent.**
        // Slice B's mutator watch is exactly this: a depth-1 check passes
        // `A parentOf B, B parentOf A` and lets `A → B → C → A` through, leaving an
        // ancestor walk that never terminates.
        //
        // `team = parent` is the depth-0 case and the database also refuses it, but
        // it is checked here so the caller gets a message rather than a constraint
        // violation.
        if team == parent {
            return Ok(true);
        }
        let closes: bool = sqlx::query_scalar(
            "WITH RECURSIVE ancestry (node) AS (
                     SELECT $2::text
                 UNION
                     SELECT t.parent_team_id FROM teams t
                       JOIN ancestry ON t.id = ancestry.node
                      WHERE t.parent_team_id IS NOT NULL
             )
             SELECT EXISTS (SELECT 1 FROM ancestry WHERE node = $1)",
        )
        .bind(team)
        .bind(parent)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(closes)
    }

    async fn follow_asset(
        &self,
        asset_id: Uuid,
        user_id: &str,
    ) -> Result<FollowOutcome, StorageError> {
        // `ON CONFLICT DO NOTHING` plus `RETURNING` is what makes idempotency
        // race-free: a read-then-write would let two concurrent follows both see
        // "not following" and one of them fail on the primary key.
        let inserted: Option<(Uuid,)> = sqlx::query_as(
            "INSERT INTO asset_followers (asset_id, user_id) VALUES ($1, $2)
             ON CONFLICT (asset_id, user_id) DO NOTHING
             RETURNING asset_id",
        )
        .bind(asset_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(if inserted.is_some() {
            FollowOutcome::Followed
        } else {
            FollowOutcome::AlreadyFollowing
        })
    }

    async fn unfollow_asset(&self, asset_id: Uuid, user_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM asset_followers WHERE asset_id = $1 AND user_id = $2")
            .bind(asset_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn assets_followed_by(
        &self,
        user_id: &str,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        // Keyset on `(fully_qualified_name, id)` like every other asset page, so a
        // follow list paginates the same way and the cursor is interchangeable.
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
              JOIN asset_followers f ON f.asset_id = assets.id
             WHERE f.user_id = $1
               AND NOT assets.deleted
               AND ($2::text IS NULL OR (fully_qualified_name, assets.id) > ($2, $3))
             ORDER BY fully_qualified_name, assets.id
             LIMIT $4"
        );
        let query = sqlx::query(&sql)
            .bind(user_id)
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.asset_page(query, page).await
    }

    async fn follower_count(&self, asset_id: Uuid) -> Result<i64, StorageError> {
        sqlx::query_scalar("SELECT count(*) FROM asset_followers WHERE asset_id = $1")
            .bind(asset_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn principal_holdings(&self, principal: &OwnerRef) -> Result<Holdings, StorageError> {
        let (user_id, team_id) = split(principal);
        // Counted by kind, because Slice G requires the refusal to say "how many
        // assets and of which types": "you own 400 things" is not actionable,
        // "1 service, 3 schemas, 396 columns" says reassign the service.
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT a.kind, count(*) FROM asset_owners o
               JOIN assets a ON a.id = o.asset_id
              WHERE (o.user_id = $1 OR o.team_id = $2)
              GROUP BY a.kind ORDER BY a.kind",
        )
        .bind(user_id)
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut owned_by_kind = Vec::with_capacity(rows.len());
        for (kind, count) in rows {
            owned_by_kind.push((
                AssetKind::parse(&kind)
                    .map_err(|_| StorageError::Unexpected(format!("unknown asset kind: {kind}")))?,
                count,
            ));
        }

        let child_teams = match principal.kind {
            OwnerKind::Team => {
                sqlx::query_scalar("SELECT id FROM teams WHERE parent_team_id = $1 ORDER BY id")
                    .bind(&principal.id)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?
            }
            // A user cannot parent a team, so the query would always be empty and
            // asking it would be a round trip that can only say "no".
            OwnerKind::User => Vec::new(),
        };

        Ok(Holdings {
            owned_by_kind,
            child_teams,
        })
    }

    async fn delete_principal(
        &self,
        principal: &OwnerRef,
        reassign_to: Option<&OwnerRef>,
    ) -> Result<PrincipalDeletion, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let table = match principal.kind {
            OwnerKind::User => "SELECT 1 FROM users WHERE id = $1",
            OwnerKind::Team => "SELECT 1 FROM teams WHERE id = $1",
        };
        let exists: Option<(i32,)> = sqlx::query_as(table)
            .bind(&principal.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if exists.is_none() {
            return Ok(PrincipalDeletion::NotFound);
        }

        let holdings = self.principal_holdings(principal).await?;
        let mut reassigned = 0_i64;

        if let Some(target) = reassign_to {
            let target_table = match target.kind {
                OwnerKind::User => "SELECT 1 FROM users WHERE id = $1",
                OwnerKind::Team => "SELECT 1 FROM teams WHERE id = $1",
            };
            let target_exists: Option<(i32,)> = sqlx::query_as(target_table)
                .bind(&target.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if target_exists.is_none() {
                return Ok(PrincipalDeletion::UnknownTarget);
            }

            let (from_user, from_team) = split(principal);
            let (to_user, to_team) = split(target);
            // **Reassign, then bump.** Slice G requires each affected asset's
            // version to move: ownership changing is a change somebody subscribed
            // to Minor bumps should see, and a silent transfer makes the audit
            // trail claim nothing happened.
            //
            // `ON CONFLICT DO NOTHING` on the update target: if the asset is
            // already owned by the destination, the transfer collapses to a
            // deletion rather than violating the identity index.
            let moved: Vec<(Uuid,)> = sqlx::query_as(
                "UPDATE asset_owners SET user_id = $3, team_id = $4
                  WHERE (user_id = $1 OR team_id = $2)
                    AND NOT EXISTS (
                          SELECT 1 FROM asset_owners existing
                           WHERE existing.asset_id = asset_owners.asset_id
                             AND (existing.user_id = $3 OR existing.team_id = $4))
                RETURNING asset_id",
            )
            .bind(from_user)
            .bind(from_team)
            .bind(to_user)
            .bind(to_team)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            reassigned = i64::try_from(moved.len()).unwrap_or(i64::MAX);

            for (asset_id,) in &moved {
                sqlx::query(
                    "UPDATE assets SET version_minor = version_minor + 1, updated_at = now()
                      WHERE id = $1",
                )
                .bind(asset_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            }

            // Child teams move with the ownership: a team being deleted cannot stay
            // the parent of anything, and `ON DELETE RESTRICT` would refuse the
            // delete otherwise.
            if matches!(principal.kind, OwnerKind::Team) && matches!(target.kind, OwnerKind::Team) {
                sqlx::query("UPDATE teams SET parent_team_id = $2 WHERE parent_team_id = $1")
                    .bind(&principal.id)
                    .bind(&target.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            }
        } else if !holdings.is_empty() {
            return Ok(PrincipalDeletion::StillHolds(Box::new(holdings)));
        }

        let delete = match principal.kind {
            OwnerKind::User => "DELETE FROM users WHERE id = $1",
            OwnerKind::Team => "DELETE FROM teams WHERE id = $1",
        };
        sqlx::query(delete)
            .bind(&principal.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(PrincipalDeletion::Deleted { reassigned })
    }

    // ---- Epic 16 Slice B ----

    async fn claim_idempotency_key(
        &self,
        key: &str,
        request_hash: &str,
    ) -> Result<IdempotencyClaim, StorageError> {
        // Swept here rather than by a background job: this project refuses a
        // scheduler (Epic 15 decision 5), and a table that only grows is a slow
        // leak nobody notices until it is large. Bounded work on a bounded index.
        sqlx::query("DELETE FROM idempotency_keys WHERE created_at < now() - interval '24 hours'")
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // **The insert is the claim.** `ON CONFLICT DO NOTHING ... RETURNING`
        // returns a row only to the caller that actually inserted, so two
        // concurrent identical requests cannot both believe they are first — a
        // read-then-write would let exactly that happen, which is the
        // concurrency criterion.
        let claimed: Option<(String,)> = sqlx::query_as(
            "INSERT INTO idempotency_keys (key, request_hash, status, body)
             VALUES ($1, $2, 0, '{}'::jsonb)
             ON CONFLICT (key) DO NOTHING
             RETURNING key",
        )
        .bind(key)
        .bind(request_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if claimed.is_some() {
            return Ok(IdempotencyClaim::Claimed);
        }

        let existing: Option<(String, i16, serde_json::Value)> = sqlx::query_as(
            "SELECT request_hash, status, body FROM idempotency_keys WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some((stored_hash, status, body)) = existing else {
            // Expired between the sweep and the read. Treating it as claimable
            // is right: the original is gone, so there is nothing to replay.
            return Ok(IdempotencyClaim::Claimed);
        };
        if stored_hash != request_hash {
            return Ok(IdempotencyClaim::Mismatch);
        }
        // `status = 0` is the placeholder the claim wrote: the first attempt owns
        // the key and has not answered yet.
        if status == 0 {
            return Ok(IdempotencyClaim::InFlight);
        }
        Ok(IdempotencyClaim::Replay {
            status: u16::try_from(status).unwrap_or(500),
            body,
        })
    }

    async fn record_idempotent_response(
        &self,
        key: &str,
        status: u16,
        body: &serde_json::Value,
    ) -> Result<(), StorageError> {
        sqlx::query("UPDATE idempotency_keys SET status = $2, body = $3 WHERE key = $1")
            .bind(key)
            .bind(i16::try_from(status).unwrap_or(500))
            .bind(body)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    // ---- Epic 16 Slice C ----

    async fn create_ingest_job(
        &self,
        job: &graph_owl_storage::IngestJob,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO ingest_jobs (id, format, state, submitted_by, started_at, heartbeat_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(job.id)
        .bind(&job.format)
        .bind(&job.state)
        .bind(&job.submitted_by)
        // Written from the job rather than defaulted to `now()`: the port hands
        // over a fully-formed row, and an adapter that quietly substituted its
        // own clock would make the type's `heartbeat_at` field a lie — and the
        // reaper's whole behaviour untestable without waiting five real minutes.
        .bind(job.started_at)
        .bind(job.heartbeat_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn ingest_job(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<graph_owl_storage::IngestJob>, StorageError> {
        type Row = (
            uuid::Uuid,
            String,
            String,
            i64,
            i64,
            i64,
            serde_json::Value,
            Option<String>,
            bool,
            String,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
            Option<chrono::DateTime<chrono::Utc>>,
        );

        let row: Option<Row> = sqlx::query_as(
            "SELECT id, format, state, rows_read, accepted, rejected, failures,
                    halt_reason, cancel_requested, submitted_by,
                    started_at, heartbeat_at, finished_at
               FROM ingest_jobs
              WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(|r| graph_owl_storage::IngestJob {
            id: r.0,
            format: r.1,
            state: r.2,
            rows_read: r.3,
            accepted: r.4,
            rejected: r.5,
            // A failure list that will not parse is reported as empty rather than
            // as an error: the counts are still true, and refusing to answer a
            // poll because one detail string is malformed would strand a client
            // with no way to learn anything at all.
            failures: serde_json::from_value(r.6).unwrap_or_default(),
            halt_reason: r.7,
            cancel_requested: r.8,
            submitted_by: r.9,
            started_at: r.10,
            heartbeat_at: r.11,
            finished_at: r.12,
        }))
    }

    async fn report_ingest_progress(
        &self,
        id: uuid::Uuid,
        progress: graph_owl_storage::IngestProgress,
        new_failures: &[graph_owl_storage::RowFailure],
    ) -> Result<bool, StorageError> {
        // Counts are **set**, not incremented: the worker holds the running
        // totals and one retried statement must not double them. Failures are
        // appended, because the worker only carries the chunk it just read —
        // holding every failure in memory to rewrite the whole list would undo
        // the memory bound this slice exists for.
        let appended = serde_json::to_value(new_failures)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let row: Option<(bool,)> = sqlx::query_as(
            "UPDATE ingest_jobs
                SET rows_read    = $2,
                    accepted     = $3,
                    rejected     = $4,
                    failures     = failures || $5::jsonb,
                    heartbeat_at = now(),
                    state        = 'running'
              WHERE id = $1
          RETURNING cancel_requested",
        )
        .bind(id)
        .bind(progress.rows_read)
        .bind(progress.accepted)
        .bind(progress.rejected)
        .bind(&appended)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // A job whose row has gone is a job nobody is waiting for, so stopping is
        // the right answer — the same one cancellation gives.
        Ok(row.is_none_or(|(cancelled,)| cancelled))
    }

    async fn finish_ingest_job(
        &self,
        id: uuid::Uuid,
        state: &str,
        halt_reason: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE ingest_jobs
                SET state = $2, halt_reason = $3, finished_at = now(), heartbeat_at = now()
              WHERE id = $1",
        )
        .bind(id)
        .bind(state)
        .bind(halt_reason)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn cancel_ingest_job(&self, id: uuid::Uuid) -> Result<bool, StorageError> {
        // `finished_at IS NULL` is the whole condition: cancelling a job that
        // already succeeded would rewrite a settled answer, and reporting success
        // for it would tell a client something stopped that had already finished.
        let row: Option<(uuid::Uuid,)> = sqlx::query_as(
            "UPDATE ingest_jobs
                SET cancel_requested = TRUE
              WHERE id = $1 AND finished_at IS NULL
          RETURNING id",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.is_some())
    }

    async fn reap_abandoned_ingest_jobs(
        &self,
        stale_after_seconds: i64,
    ) -> Result<u64, StorageError> {
        let reaped = sqlx::query(
            "UPDATE ingest_jobs
                SET state = 'failed',
                    halt_reason = 'abandoned: the worker stopped reporting',
                    finished_at = now()
              WHERE finished_at IS NULL
                AND heartbeat_at < now() - ($1::double precision * interval '1 second')",
        )
        .bind(f64::from(
            i32::try_from(stale_after_seconds.max(0)).unwrap_or(i32::MAX),
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(reaped.rows_affected())
    }

    async fn upsert_team(&self, team: &graph_owl_storage::Team) -> Result<(), StorageError> {
        // One transaction: a team whose row was written and whose membership
        // was not is a team that silently owns things on nobody's behalf.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO teams (id, display_name, description, parent_team_id)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE
                SET display_name   = EXCLUDED.display_name,
                    description    = EXCLUDED.description,
                    parent_team_id = EXCLUDED.parent_team_id",
        )
        .bind(&team.id)
        .bind(&team.display_name)
        .bind(&team.description)
        .bind(&team.parent_team_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // Replaced, not merged. A partial update cannot express "remove
        // everybody", and removal is the operation that has to work.
        sqlx::query("DELETE FROM team_members WHERE team_id = $1")
            .bind(&team.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for member in &team.members {
            sqlx::query("INSERT INTO team_members (team_id, user_id) VALUES ($1, $2)")
                .bind(&team.id)
                .bind(member)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    if e.as_database_error()
                        .is_some_and(|d| d.is_foreign_key_violation())
                    {
                        StorageError::Unexpected(format!(
                            "`{member}` is not a known user; a team member nobody \
                             can resolve is an owner who does not exist"
                        ))
                    } else {
                        StorageError::Unexpected(e.to_string())
                    }
                })?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.find_team", skip_all)]
    async fn find_team(&self, id: &str) -> Result<Option<graph_owl_storage::Team>, StorageError> {
        let row = sqlx::query(
            "SELECT t.id, t.display_name, t.description, t.parent_team_id,
                    COALESCE(
                        ARRAY_AGG(m.user_id ORDER BY m.user_id)
                            FILTER (WHERE m.user_id IS NOT NULL),
                        '{}'
                    ) AS members
               FROM teams t
               LEFT JOIN team_members m ON m.team_id = t.id
              WHERE t.id = $1
              GROUP BY t.id",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(team_from_row))
    }

    #[tracing::instrument(name = "storage.teams", skip_all)]
    async fn teams(&self) -> Result<Vec<graph_owl_storage::Team>, StorageError> {
        let rows = sqlx::query(
            "SELECT t.id, t.display_name, t.description, t.parent_team_id,
                    COALESCE(
                        ARRAY_AGG(m.user_id ORDER BY m.user_id)
                            FILTER (WHERE m.user_id IS NOT NULL),
                        '{}'
                    ) AS members
               FROM teams t
               LEFT JOIN team_members m ON m.team_id = t.id
              GROUP BY t.id
              ORDER BY t.id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.into_iter().map(team_from_row).collect())
    }

    #[tracing::instrument(name = "storage.assign_finding", skip_all)]
    async fn assign_finding(
        &self,
        assignment: &graph_owl_storage::Assignment,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO validation_assignments
                 (id, shape, focus_node, path, constraint_kind, assignee, assigned_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(assignment.id)
        .bind(&assignment.shape)
        .bind(&assignment.focus_node)
        .bind(&assignment.path)
        .bind(&assignment.constraint_kind)
        .bind(&assignment.assignee)
        .bind(&assignment.assigned_by)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| {
            let db = e.as_database_error();
            if db.is_some_and(|d| d.is_unique_violation()) {
                StorageError::Conflict {
                    detail: "this finding is already assigned".to_string(),
                    existing_id: None,
                    kind: graph_owl_storage::ConflictKind::AssignmentExists,
                }
            } else if db.is_some_and(|d| d.is_foreign_key_violation()) {
                // The FK is what makes "assign to a nickname" impossible. Said
                // plainly here so the API can explain it rather than returning
                // a 500 for something the caller can fix.
                StorageError::Unexpected(
                    "that assignee is not a known user; a finding assigned to a \
                     name nobody can resolve looks worked and is not"
                        .to_string(),
                )
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })
    }

    #[tracing::instrument(name = "storage.unassign_finding", skip_all)]
    async fn unassign_finding(&self, id: Uuid) -> Result<bool, StorageError> {
        sqlx::query("DELETE FROM validation_assignments WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.assignments", skip_all)]
    async fn assignments(&self) -> Result<Vec<graph_owl_storage::Assignment>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, shape, focus_node, path, constraint_kind,
                    assignee, assigned_by, assigned_at
               FROM validation_assignments",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::Assignment {
                id: row.get("id"),
                shape: row.get("shape"),
                focus_node: row.get("focus_node"),
                path: row.get("path"),
                constraint_kind: row.get("constraint_kind"),
                assignee: row.get("assignee"),
                assigned_by: row.get("assigned_by"),
                assigned_at: row.get("assigned_at"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.recent_runs", skip_all)]
    async fn recent_runs(
        &self,
        service_name: &str,
        limit: usize,
    ) -> Result<Vec<graph_owl_storage::ConnectorRun>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, connector, service_name, started_at, finished_at,
                    created, skipped, failed, deleted, failures, refusal, triggered_by
               FROM connector_runs
              WHERE ($1 = '' OR service_name = $1)
              ORDER BY started_at DESC
              LIMIT $2",
        )
        .bind(service_name)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| graph_owl_storage::ConnectorRun {
                id: row.get("id"),
                connector: row.get("connector"),
                service_name: row.get("service_name"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
                created: row.get("created"),
                skipped: row.get("skipped"),
                failed: row.get("failed"),
                deleted: row.get("deleted"),
                failures: row.get("failures"),
                refusal: row.get("refusal"),
                triggered_by: row.get("triggered_by"),
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.source_hashes", skip_all)]
    async fn source_hashes(
        &self,
        fqns: &[String],
    ) -> Result<std::collections::HashMap<String, Option<Vec<u8>>>, StorageError> {
        // Deleted rows are excluded deliberately. A tombstoned asset must look
        // *absent* to a re-run, so the record is created afresh rather than
        // compared against the fingerprint it had before it was deleted — and
        // `upsert_asset` is what refuses to resurrect the tombstone.
        let rows = sqlx::query(
            "SELECT fully_qualified_name, source_hash FROM assets
             WHERE NOT deleted AND fully_qualified_name = ANY($1)",
        )
        .bind(fqns)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("fully_qualified_name"),
                    row.get::<Option<Vec<u8>>, _>("source_hash"),
                )
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.set_source_hash", skip_all)]
    async fn set_source_hash(&self, id: Uuid, hash: &[u8]) -> Result<(), StorageError> {
        sqlx::query("UPDATE assets SET source_hash = $2 WHERE id = $1")
            .bind(id)
            .bind(hash)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    fn pool_stats(&self) -> Option<graph_owl_storage::PoolStats> {
        Some(graph_owl_storage::PoolStats {
            connections: self.pool.size(),
            // `num_idle` is a `usize` that cannot exceed `size`, which is a
            // `u32` — so the cast is lossless in every state a pool can reach.
            idle: u32::try_from(self.pool.num_idle()).unwrap_or(u32::MAX),
        })
    }

    async fn ping(&self) -> Result<(), StorageError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn insert_table(&self, table: Table) -> Result<Table, StorageError> {
        let result = sqlx::query(
            "INSERT INTO tables (id, name, fully_qualified_name, description, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(table.id)
        .bind(&table.name)
        .bind(&table.fully_qualified_name)
        .bind(&table.description)
        .bind(table.created_at)
        .bind(table.updated_at)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            return Err(match &e {
                sqlx::Error::Database(db_err)
                    if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) =>
                {
                    // Second query only on the error path: name the row that was
                    // already there so the caller can act on it.
                    let existing_id =
                        sqlx::query_scalar("SELECT id FROM tables WHERE fully_qualified_name = $1")
                            .bind(&table.fully_qualified_name)
                            .fetch_optional(&self.pool)
                            .await
                            .ok()
                            .flatten();
                    StorageError::Conflict {
                        detail: table.fully_qualified_name.clone(),
                        existing_id,
                        kind: ConflictKind::Fqn,
                    }
                }
                _ => StorageError::Unexpected(e.to_string()),
            });
        }

        Ok(table)
    }

    async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, fully_qualified_name, description, created_at, updated_at
             FROM tables WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(table_from_row))
    }

    async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
        // Overfetch by one: the extra row answers "is there a next page" without
        // a second COUNT, and is dropped before the page is returned.
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);

        // Keyset, not OFFSET. The row comparison `(fqn, id) > ($1, $2)` is a
        // single index-ordered seek and is stable under concurrent insert;
        // OFFSET re-counts from the start and shifts under any earlier insert.
        let rows = match &page.after {
            Some(cursor) => {
                sqlx::query(
                    "SELECT id, name, fully_qualified_name, description, created_at, updated_at
                     FROM tables
                     WHERE (fully_qualified_name, id) > ($1, $2)
                     ORDER BY fully_qualified_name, id
                     LIMIT $3",
                )
                .bind(&cursor.sort_key)
                .bind(cursor.id)
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT id, name, fully_qualified_name, description, created_at, updated_at
                     FROM tables
                     ORDER BY fully_qualified_name, id
                     LIMIT $1",
                )
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let tables: Vec<Table> = rows.into_iter().map(table_from_row).collect();
        Ok(Page::from_overfetch(tables, page.limit, |table| {
            Cursor::new(table.fully_qualified_name.clone(), table.id)
        }))
    }

    async fn update_table(
        &self,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, StorageError> {
        let row = sqlx::query(
            "UPDATE tables
             SET name = COALESCE($2, name),
                 description = COALESCE($3, description),
                 updated_at = now()
             WHERE id = $1
             RETURNING id, name, fully_qualified_name, description, created_at, updated_at",
        )
        .bind(id)
        .bind(&update.name)
        .bind(&update.description)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(table_from_row))
    }

    async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM tables WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn create_relationship(
        &self,
        relationship: Relationship,
    ) -> Result<Relationship, StorageError> {
        sqlx::query(
            "INSERT INTO entity_relationships
                (id, from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(relationship.id)
        .bind(&relationship.from_entity_type)
        .bind(relationship.from_entity_id)
        .bind(&relationship.relationship_type)
        .bind(&relationship.to_entity_type)
        .bind(relationship.to_entity_id)
        .bind(relationship.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) => {
                StorageError::Conflict {
                    detail: format!(
                        "{}:{} -{}-> {}:{}",
                        relationship.from_entity_type,
                        relationship.from_entity_id,
                        relationship.relationship_type,
                        relationship.to_entity_type,
                        relationship.to_entity_id
                    ),
                    existing_id: None,
                    kind: ConflictKind::RelationshipTuple,
                }
            }
            _ => StorageError::Unexpected(e.to_string()),
        })?;

        Ok(relationship)
    }

    async fn list_relationships_for_entity(
        &self,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<Vec<Relationship>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id, created_at
             FROM entity_relationships
             WHERE (from_entity_type = $1 AND from_entity_id = $2)
                OR (to_entity_type = $1 AND to_entity_id = $2)",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.into_iter().map(relationship_from_row).collect())
    }

    async fn get_relationship(&self, id: Uuid) -> Result<Option<Relationship>, StorageError> {
        sqlx::query("SELECT * FROM entity_relationships WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(|row| row.map(relationship_from_row))
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn list_relationships(
        &self,
        page: &PageRequest,
    ) -> Result<Page<Relationship>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let rows = sqlx::query(
            "SELECT id, from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id, created_at
             FROM entity_relationships
             WHERE $1::uuid IS NULL OR id > $1
             ORDER BY id
             LIMIT $2",
        )
        .bind(page.after.as_ref().map(|c| c.id))
        .bind(overfetch)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let relationships: Vec<Relationship> =
            rows.into_iter().map(relationship_from_row).collect();
        Ok(Page::from_overfetch(relationships, page.limit, |r| {
            Cursor::new(r.id.to_string(), r.id)
        }))
    }

    async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM entity_relationships WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    // ---- asset hierarchy ----

    #[tracing::instrument(name = "storage.upsert_asset", skip_all)]
    async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError> {
        // ON CONFLICT on the FQN, because the FQN *is* the identity: a
        // connector re-run supplies a fresh Uuid every time, and treating that
        // as a new entity would duplicate the whole warehouse nightly.
        // COALESCE on description keeps human curation: a source reporting
        // NULL means "I have nothing to say", not "blank what a person wrote"
        // (15-connectors.md decision 3).
        let row = sqlx::query(&format!(
            "INSERT INTO assets (id, kind, name, fully_qualified_name, parent_id, description,
                 properties, extension, version_major, version_minor, updated_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, COALESCE($11, '{{}}'::jsonb), 0, 1, $10, $8, $9)
             ON CONFLICT (fully_qualified_name) DO UPDATE SET
                 name = EXCLUDED.name,
                 parent_id = EXCLUDED.parent_id,
                 description = COALESCE(EXCLUDED.description, assets.description),
                 properties = COALESCE(EXCLUDED.properties, assets.properties),
                 -- **A connector cannot blank the organization\'s own fields.**
                 -- `properties` above is what the source reported and a re-run
                 -- may legitimately replace it; `extension` is what a person
                 -- curated, and a connector that sends none must leave it
                 -- alone. Without this guard the nightly run would wipe every
                 -- costCenter in the catalog, silently, on the first night.
                 extension = CASE
                     WHEN $11 IS NULL THEN assets.extension
                     ELSE EXCLUDED.extension
                 END,
                 updated_by = EXCLUDED.updated_by,
                 -- A re-ingest of a live asset does not resurrect a tombstone:
                 -- deletion is a governance decision and a connector must not
                 -- silently reverse it.
                 updated_at = now()
             RETURNING {ASSET_COLUMNS}"
        ))
        .bind(asset.id)
        .bind(asset.kind.as_str())
        .bind(&asset.name)
        .bind(&asset.fully_qualified_name)
        .bind(asset.parent_id)
        .bind(&asset.description)
        .bind(&asset.properties)
        .bind(asset.created_at)
        .bind(asset.updated_at)
        .bind(&asset.updated_by)
        .bind(asset.extension.clone().map(serde_json::Value::Object))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // `RETURNING` cannot carry the owners subquery — it has no `assets` alias
        // to correlate against — so this path reads them. One extra query on a
        // write, rather than a response that reports an owned asset as unowned.
        let mut written = asset_from_row(row);
        written.owners = self.asset_owners(written.id).await?;
        self.recompute_blocking_keys(&written).await?;
        Ok(written)
    }

    #[tracing::instrument(name = "storage.bump_version", skip_all)]
    async fn bump_version(
        &self,
        id: Uuid,
        next: graph_owl_core::envelope::EntityVersion,
        change_description: graph_owl_core::envelope::ChangeDescription,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let row = sqlx::query(&format!(
            "UPDATE assets SET version_major = $2, version_minor = $3, updated_by = $4,
                 change_description = $5, updated_at = now()
             WHERE id = $1
             RETURNING {ASSET_COLUMNS}"
        ))
        .bind(id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(updated_by)
        .bind(serde_json::to_value(&change_description).ok())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(row) = row else {
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(None);
        };
        let updated = asset_from_row(row);

        // Same history row `update_asset` writes, for the same reason: a
        // version bump with no entry in `asset_versions` is a version the
        // "History" tab cannot show.
        sqlx::query(
            "INSERT INTO asset_versions
                 (asset_id, version_major, version_minor, snapshot, change_description, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(serde_json::to_value(&updated).unwrap_or_default())
        .bind(serde_json::to_value(&change_description).ok())
        .bind(updated_by)
        .bind(updated.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut written = updated;
        written.owners = self.asset_owners(written.id).await?;
        Ok(Some(written))
    }

    #[tracing::instrument(name = "storage.get_asset", skip_all)]
    async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(asset_from_row))
    }

    #[tracing::instrument(name = "storage.get_asset_by_fqn", skip_all)]
    async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets WHERE fully_qualified_name = $1"
        ))
        .bind(fqn)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(asset_from_row))
    }

    async fn list_assets(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
             WHERE NOT deleted
               AND ($1::text IS NULL OR kind = $1)
               AND ($2::text IS NULL OR (fully_qualified_name, id) > ($2, $3))
             ORDER BY fully_qualified_name, id
             LIMIT $4"
        );
        let query = sqlx::query(&sql)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.asset_page(query, page).await
    }

    async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
             WHERE NOT deleted AND (($1::uuid IS NULL AND parent_id IS NULL) OR parent_id = $1)
             ORDER BY name"
        ))
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, StorageError> {
        // Recursive CTE walking parent_id upward, then reversed so callers get
        // root-first — which is the order a breadcrumb renders in.
        let recursive_columns = asset_columns_as("a");
        let rows = sqlx::query(&format!(
            // **Owners are projected once, outside the CTE.** Two reasons, and
            // both were live bugs: computing them in the non-recursive branch gave
            // it one more column than the recursive branch, which a `UNION ALL`
            // rejects outright; and `OWNERS_EXPR` correlates on `assets.id`, so
            // the outer `SELECT` needs an `assets` in scope — reading `FROM chain`
            // made the reference unresolvable and every ancestors request a 500.
            //
            // `chain AS assets` is what supplies that scope. Aliasing the CTE to
            // the table name the expression expects keeps the expression shared
            // rather than forking a second copy that takes a different alias.
            "WITH RECURSIVE chain AS (
                 SELECT {ASSET_COLUMNS}, 0 AS hops FROM assets WHERE id = $1
                 UNION ALL
                 SELECT {recursive_columns}, c.hops + 1
                 FROM assets a JOIN chain c ON a.id = c.parent_id
             )
             SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners
               FROM chain AS assets ORDER BY hops DESC"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    async fn search_assets(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let Some(terms) = graph_owl_search::tsquery(query) else {
            return Ok(Self::empty_ranked_page(page));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners, {RANK_KEY} AS sort_key
             FROM assets
             {POPULARITY_JOIN}, to_tsquery('english', $1) AS q (ts)
             WHERE NOT deleted
               AND assets.search_vector @@ q.ts
               AND ($2::text IS NULL OR kind = $2)
               AND ($3::text IS NULL OR ({RANK_KEY}, id) > ($3, $4))
             ORDER BY {RANK_KEY}, id
             LIMIT $5"
        );
        let q = sqlx::query(&sql)
            .bind(terms)
            .bind(kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.ranked_asset_page(q, page).await
    }

    async fn list_assets_under_fqn(&self, prefix: &str) -> Result<Vec<Asset>, StorageError> {
        // `fqn = prefix OR fqn LIKE prefix || '.%'` rather than a bare prefix
        // match: `hdfc-core` must not also match a service called
        // `hdfc-core-archive`, which a plain LIKE would sweep into the scope
        // and then delete.
        //
        // The empty prefix is special-cased to mean *everything*. Left to the
        // general form it becomes `fqn LIKE '.%'`, which is false for every
        // real FQN — so "no restriction" would silently return nothing.
        sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
             WHERE deleted = FALSE AND ($1 = ''
                                        OR fully_qualified_name = $1
                                        OR fully_qualified_name LIKE $1 || '.%')"
        ))
        .bind(prefix)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(asset_from_row).collect())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError> {
        let rows =
            sqlx::query("SELECT kind, count(*) AS n FROM assets WHERE NOT deleted GROUP BY kind")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                AssetKind::parse(row.get::<&str, _>("kind"))
                    .ok()
                    .map(|kind| (kind, row.get::<i64, _>("n")))
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.resolution_candidates", skip_all)]
    async fn resolution_candidates(&self, asset_id: Uuid) -> Result<Vec<Asset>, StorageError> {
        let rows = sqlx::query(&resolution_candidates_sql())
            .bind(asset_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    #[tracing::instrument(name = "storage.create_merge_record", skip_all)]
    async fn create_merge_record(
        &self,
        record: graph_owl_core::resolution::MergeRecord,
    ) -> Result<graph_owl_core::resolution::MergeRecord, StorageError> {
        let evidence = serde_json::to_value(&record.evidence)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let decided_by = serde_json::to_value(&record.decided_by)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        sqlx::query(
            "INSERT INTO merge_records (id, canonical_id, merged_id, evidence, confidence, decided_by, decided_at, merged_at_t, split_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(record.id)
        .bind(record.canonical)
        .bind(record.merged)
        .bind(evidence)
        .bind(record.confidence)
        .bind(decided_by)
        .bind(record.decided_at)
        .bind(record.merged_at_t)
        .bind(record.split_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(record)
    }

    #[tracing::instrument(name = "storage.get_merge_record", skip_all)]
    async fn get_merge_record(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::resolution::MergeRecord>, StorageError> {
        let row = sqlx::query("SELECT * FROM merge_records WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(merge_record_from_row).transpose()
    }

    #[tracing::instrument(name = "storage.split_merge_record", skip_all)]
    async fn split_merge_record(
        &self,
        id: Uuid,
        split_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SplitOutcome, StorageError> {
        // Atomic for the common path: the `WHERE split_at IS NULL` makes a
        // concurrent double-split lose the race at the database rather than
        // between a Rust-side read and write.
        let row = sqlx::query(
            "UPDATE merge_records SET split_at = $2 WHERE id = $1 AND split_at IS NULL RETURNING *",
        )
        .bind(id)
        .bind(split_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if let Some(row) = row {
            return Ok(SplitOutcome::Split(Box::new(merge_record_from_row(row)?)));
        }

        // Not updated: either the id is unknown, or it was already split.
        // Only this (rarer) path needs the extra read to tell the two apart.
        match self.get_merge_record(id).await? {
            None => Ok(SplitOutcome::NotFound),
            Some(existing) => Ok(SplitOutcome::AlreadySplit {
                split_at: existing
                    .split_at
                    .expect("not updated by the statement above because it is already split"),
            }),
        }
    }

    #[tracing::instrument(name = "storage.most_recent_split_between", skip_all)]
    async fn most_recent_split_between(
        &self,
        a: Uuid,
        b: Uuid,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
        sqlx::query_scalar(
            "SELECT MAX(split_at) FROM merge_records
             WHERE split_at IS NOT NULL
               AND ((canonical_id = $1 AND merged_id = $2) OR (canonical_id = $2 AND merged_id = $1))",
        )
        .bind(a)
        .bind(b)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    #[tracing::instrument(name = "storage.queue_for_review", skip_all)]
    async fn queue_for_review(
        &self,
        entry: graph_owl_core::resolution::ReviewQueueEntry,
    ) -> Result<graph_owl_core::resolution::ReviewQueueEntry, StorageError> {
        let evidence = serde_json::to_value(&entry.evidence)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let decided_by = entry
            .decided_by
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let row = sqlx::query(
            "INSERT INTO resolution_queue
                 (id, target_id, candidate_id, score, evidence, status, decided_by, decided_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (target_id, candidate_id) DO UPDATE SET
                 target_id = resolution_queue.target_id
             RETURNING *",
        )
        .bind(entry.id)
        .bind(entry.target)
        .bind(entry.candidate)
        .bind(entry.score)
        .bind(evidence)
        .bind(review_status_str(entry.status))
        .bind(decided_by)
        .bind(entry.decided_at)
        .bind(entry.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        review_queue_entry_from_row(row)
    }

    #[tracing::instrument(name = "storage.list_review_queue", skip_all)]
    async fn list_review_queue(
        &self,
        filter: &ReviewQueueFilter,
    ) -> Result<(Vec<graph_owl_core::resolution::ReviewQueueEntry>, i64), StorageError> {
        let status = filter.status.map_or("pending", review_status_str);
        let kind = filter.kind.map(AssetKind::as_str);

        let where_clause = "rq.status = $1
              AND ($2::text IS NULL OR a.kind = $2)
              AND ($3::double precision IS NULL OR rq.score >= $3)
              AND ($4::double precision IS NULL OR rq.score <= $4)";

        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM resolution_queue rq
             JOIN assets a ON a.id = rq.target_id
             WHERE {where_clause}"
        ))
        .bind(status)
        .bind(kind)
        .bind(filter.min_score)
        .bind(filter.max_score)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let limit = i64::try_from(filter.limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(filter.offset).unwrap_or(0);
        let rows = sqlx::query(&format!(
            "SELECT rq.* FROM resolution_queue rq
             JOIN assets a ON a.id = rq.target_id
             WHERE {where_clause}
             ORDER BY rq.score DESC, rq.id
             LIMIT $5 OFFSET $6"
        ))
        .bind(status)
        .bind(kind)
        .bind(filter.min_score)
        .bind(filter.max_score)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let entries = rows
            .into_iter()
            .map(review_queue_entry_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((entries, total))
    }

    #[tracing::instrument(name = "storage.get_review_queue_entry", skip_all)]
    async fn get_review_queue_entry(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError> {
        let row = sqlx::query("SELECT * FROM resolution_queue WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(review_queue_entry_from_row).transpose()
    }

    #[tracing::instrument(name = "storage.decide_review_queue_entry", skip_all)]
    async fn decide_review_queue_entry(
        &self,
        id: Uuid,
        status: graph_owl_core::resolution::ReviewStatus,
        decided_by: graph_owl_core::resolution::MergeDecidedBy,
        decided_at: chrono::DateTime<chrono::Utc>,
        reason: Option<String>,
    ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError> {
        let decided_by_json = serde_json::to_value(&decided_by)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        // Atomic and one-shot: the `WHERE status = 'pending'` is what stops a
        // second decide call from overwriting the first decision, without a
        // separate read-then-write race window.
        let row = sqlx::query(
            "UPDATE resolution_queue
             SET status = $2, decided_by = $3, decided_at = $4, reason = $5
             WHERE id = $1 AND status = 'pending'
             RETURNING *",
        )
        .bind(id)
        .bind(review_status_str(status))
        .bind(decided_by_json)
        .bind(decided_at)
        .bind(reason)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(review_queue_entry_from_row(row)?)),
            // Either unknown, or already decided — either way the caller gets
            // the current row (or nothing) rather than a write that silently
            // did not happen.
            None => self.get_review_queue_entry(id).await,
        }
    }

    #[tracing::instrument(name = "storage.push_drift", skip_all)]
    async fn push_drift(
        &self,
        asset_id: Uuid,
        item: graph_owl_core::drift::DriftReportItem,
    ) -> Result<graph_owl_core::drift::DriftItem, StorageError> {
        // The `ON CONFLICT` target names the partial unique index
        // (`WHERE status = 'pending'`) from V51, so a repeat push while an
        // item is still pending returns the existing row unchanged; once it
        // is applied or ignored, the index no longer covers it and a fresh
        // pending row is inserted for the next occurrence — a new instance
        // of the problem, not the same one still open.
        let row = sqlx::query(
            "WITH ins AS (
                 INSERT INTO drift_reports
                     (id, asset_id, field, kind, live_value, declared_value)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (asset_id, field) WHERE status = 'pending'
                 DO UPDATE SET asset_id = drift_reports.asset_id
                 RETURNING *
             )
             SELECT ins.*, a.fully_qualified_name
             FROM ins JOIN assets a ON a.id = ins.asset_id",
        )
        .bind(Uuid::new_v4())
        .bind(asset_id)
        .bind(&item.field)
        .bind(drift_kind_str(item.kind))
        .bind(&item.live_value)
        .bind(&item.declared_value)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        drift_item_from_row(row)
    }

    #[tracing::instrument(name = "storage.list_drift", skip_all)]
    async fn list_drift(
        &self,
        filter: &DriftFilter,
    ) -> Result<(Vec<graph_owl_core::drift::DriftItem>, i64), StorageError> {
        let status = filter.status.map_or("pending", drift_status_str);
        let limit = i64::try_from(filter.limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(filter.offset).unwrap_or(0);

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drift_reports WHERE status = $1")
            .bind(status)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let rows = sqlx::query(
            "SELECT dr.*, a.fully_qualified_name
             FROM drift_reports dr JOIN assets a ON a.id = dr.asset_id
             WHERE dr.status = $1
             ORDER BY dr.reported_at DESC, dr.id
             LIMIT $2 OFFSET $3",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let items = rows
            .into_iter()
            .map(drift_item_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((items, total))
    }

    #[tracing::instrument(name = "storage.get_drift_item", skip_all)]
    async fn get_drift_item(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::drift::DriftItem>, StorageError> {
        let row = sqlx::query(
            "SELECT dr.*, a.fully_qualified_name
             FROM drift_reports dr JOIN assets a ON a.id = dr.asset_id
             WHERE dr.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(drift_item_from_row).transpose()
    }

    #[tracing::instrument(name = "storage.decide_drift", skip_all)]
    async fn decide_drift(
        &self,
        id: Uuid,
        status: graph_owl_core::drift::DriftStatus,
        decided_by: String,
        decided_at: chrono::DateTime<chrono::Utc>,
        reason: Option<String>,
    ) -> Result<Option<graph_owl_core::drift::DriftItem>, StorageError> {
        // Same one-shot pattern as `decide_review_queue_entry`: the
        // `WHERE status = 'pending'` makes the write atomic against a second
        // decide call, with no read-then-write race window.
        let row = sqlx::query(
            "WITH upd AS (
                 UPDATE drift_reports
                 SET status = $2, decided_at = $3, decided_by = $4, reason = $5
                 WHERE id = $1 AND status = 'pending'
                 RETURNING *
             )
             SELECT upd.*, a.fully_qualified_name
             FROM upd JOIN assets a ON a.id = upd.asset_id",
        )
        .bind(id)
        .bind(drift_status_str(status))
        .bind(decided_at)
        .bind(&decided_by)
        .bind(&reason)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        match row {
            Some(row) => Ok(Some(drift_item_from_row(row)?)),
            None => self.get_drift_item(id).await,
        }
    }

    #[tracing::instrument(name = "storage.record_mention_resolution", skip_all)]
    async fn record_mention_resolution(
        &self,
        resolution: graph_owl_core::resolution::MentionResolution,
    ) -> Result<graph_owl_core::resolution::MentionResolution, StorageError> {
        sqlx::query(
            "INSERT INTO mention_resolutions (id, source_id, text, entity_id, confidence, resolved_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(resolution.id)
        .bind(resolution.source)
        .bind(&resolution.text)
        .bind(resolution.entity)
        .bind(resolution.confidence)
        .bind(resolution.resolved_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(resolution)
    }

    #[tracing::instrument(name = "storage.mention_resolutions_for_source", skip_all)]
    async fn mention_resolutions_for_source(
        &self,
        source: Uuid,
    ) -> Result<Vec<graph_owl_core::resolution::MentionResolution>, StorageError> {
        let rows = sqlx::query(
            "SELECT * FROM mention_resolutions WHERE source_id = $1 ORDER BY resolved_at DESC",
        )
        .bind(source)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(mention_resolution_from_row).collect())
    }

    // ---- envelope (Epic 3) ----

    #[tracing::instrument(name = "storage.update_asset", skip_all)]
    async fn update_asset(
        &self,
        id: Uuid,
        update: &AssetUpdate,
        updated_by: &str,
        expected_version: Option<EntityVersion>,
    ) -> Result<UpdateOutcome, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let before_row = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets WHERE id = $1 FOR UPDATE"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(before_row) = before_row else {
            return Ok(UpdateOutcome::NotFound);
        };
        let before = asset_from_row(before_row);

        // Compared under the row lock taken above, so no writer can slip
        // between the check and the write.
        if expected_version.is_some_and(|expected| before.version != expected) {
            return Ok(UpdateOutcome::VersionMismatch(before.version));
        }

        // Absent means "not declared"; explicit null means clear. Collapsing
        // them would let a connector's null description blank what a human
        // wrote (15-connectors.md decision 3).
        let mut after = before.clone();
        if let Some(description) = &update.description {
            after.description = description.clone();
        }
        // Merged per key against what is stored, so a patch naming one custom
        // property cannot clear another. The merge is the domain's
        // ([`AssetUpdate::merged_extension`]) rather than SQL's, because it is
        // the same rule the facade validates against — computing it twice in
        // two languages is how the validated bag and the written bag diverge.
        if let Some(merged) = update.merged_extension(before.extension.as_ref()) {
            after.extension = Some(merged);
        }
        // Phase 3 item 3.3. Re-derived from the *current* parent's FQN, read
        // fresh here under the row lock already held — the same reasoning
        // `update_domain` already uses, so a concurrent rename of the parent
        // cannot leave this asset's own FQN computed against a stale prefix.
        if let Some(name) = &update.name {
            after.name.clone_from(name);
            let parent_fqn: Option<String> = match before.parent_id {
                None => None,
                Some(parent_id) => {
                    sqlx::query_scalar("SELECT fully_qualified_name FROM assets WHERE id = $1")
                        .bind(parent_id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|e| StorageError::Unexpected(e.to_string()))?
                }
            };
            after.fully_qualified_name = match &parent_fqn {
                Some(parent) => graph_owl_core::fqn::child_of(parent, &after.name),
                None => graph_owl_core::fqn::derive(&[&after.name]),
            }
            // The facade already validated the raw segment before this was
            // ever called; a failure here would mean the parent's own FQN
            // changed shape between read and lock, not a client mistake.
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        let diff = ChangeDescription::between(
            &serde_json::to_value(&before).unwrap_or_default(),
            &serde_json::to_value(&after).unwrap_or_default(),
        );
        let kind = classify(&diff);
        if matches!(kind, graph_owl_core::envelope::ChangeKind::None) {
            // No version, no history row, no event. This is what makes a
            // connector re-run over an unchanged source observable.
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(UpdateOutcome::Updated(Box::new(before)));
        }

        let next = before.version.bump(kind);
        let updated_row = sqlx::query(&format!(
            "UPDATE assets SET name = $2, fully_qualified_name = $3, description = $4,
                 version_major = $5, version_minor = $6, updated_by = $7,
                 change_description = $8, extension = $9, updated_at = now()
             WHERE id = $1
             RETURNING {ASSET_COLUMNS}"
        ))
        .bind(id)
        .bind(&after.name)
        .bind(&after.fully_qualified_name)
        .bind(&after.description)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(updated_by)
        .bind(serde_json::to_value(&diff).ok())
        .bind(serde_json::to_value(after.extension.clone().unwrap_or_default()).unwrap_or_default())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!(
                        "an asset already exists at `{}`",
                        after.fully_qualified_name
                    ),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        // **The subtree's paths move with it**, in the same transaction — the
        // same guarantee `update_domain` already gives domains. A rename
        // that moved only its own path would leave every descendant
        // claiming to sit under a name that no longer exists.
        if before.fully_qualified_name != after.fully_qualified_name {
            sqlx::query(
                "WITH RECURSIVE subtree (id) AS (
                         SELECT id FROM assets WHERE parent_id = $1
                     UNION ALL
                         SELECT a.id FROM assets a JOIN subtree ON a.parent_id = subtree.id
                 )
                 UPDATE assets
                    SET fully_qualified_name = $3 || substring(fully_qualified_name from length($2) + 1)
                  WHERE id IN (SELECT id FROM subtree)",
            )
            .bind(id)
            .bind(&before.fully_qualified_name)
            .bind(&after.fully_qualified_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            // Phase 3 item 3.8, sequenced after this cascade landing in 3.3.
            // `lineage_column_mappings` stores column FQNs as plain TEXT with
            // no foreign key (by design — the same choice `lineage_edges.source`
            // makes), so a rename here has to reach it explicitly or a mapping
            // silently keeps citing an FQN nothing answers to any more.
            // `starts_with` rather than `LIKE`: an FQN segment routinely
            // contains `_`, which `LIKE` would read as a wildcard.
            for column in ["from_column_fqn", "to_column_fqn"] {
                sqlx::query(&format!(
                    "UPDATE lineage_column_mappings
                        SET {column} = $2 || substring({column} from length($1) + 1)
                      WHERE {column} = $1 OR starts_with({column}, $1 || '.')"
                ))
                .bind(&before.fully_qualified_name)
                .bind(&after.fully_qualified_name)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            }
        }

        let updated = asset_from_row(updated_row);

        // The snapshot is the state *after* the change, so replaying history
        // never requires applying diffs forward from the beginning.
        sqlx::query(
            "INSERT INTO asset_versions
                 (asset_id, version_major, version_minor, snapshot, change_description, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(serde_json::to_value(&updated).unwrap_or_default())
        .bind(serde_json::to_value(&diff).ok())
        .bind(updated_by)
        .bind(updated.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // After the commit, for the same reason as `upsert_asset`. Read outside
        // the transaction deliberately: this path never changes owners, so the
        // committed state is the right answer and reading inside would have seen
        // the same rows anyway.
        let mut updated = updated;
        updated.owners = self.asset_owners(updated.id).await?;
        Ok(UpdateOutcome::Updated(Box::new(updated)))
    }

    async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, StorageError> {
        let rows = sqlx::query(
            "SELECT version_major, version_minor, snapshot, change_description, updated_by, updated_at
             FROM asset_versions WHERE asset_id = $1
             ORDER BY version_major DESC, version_minor DESC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(AssetVersion {
                    version: EntityVersion {
                        major: u32::try_from(row.get::<i32, _>("version_major")).ok()?,
                        minor: u32::try_from(row.get::<i32, _>("version_minor")).ok()?,
                    },
                    snapshot: serde_json::from_value(row.get("snapshot")).ok()?,
                    change_description: row
                        .get::<Option<serde_json::Value>, _>("change_description")
                        .and_then(|v| serde_json::from_value(v).ok()),
                    updated_by: row.get("updated_by"),
                    updated_at: row.get("updated_at"),
                })
            })
            .collect())
    }

    #[tracing::instrument(name = "storage.soft_delete_asset", skip_all)]
    async fn soft_delete_asset(&self, id: Uuid, deleted_by: &str) -> Result<u64, StorageError> {
        // Cascades down the subtree: a live column under a tombstoned table is
        // reachable by search and addresses an asset that no longer exists.
        let result = sqlx::query(
            "WITH RECURSIVE subtree AS (
                 SELECT id FROM assets WHERE id = $1
                 UNION ALL
                 SELECT a.id FROM assets a JOIN subtree s ON a.parent_id = s.id
             )
             UPDATE assets SET deleted = TRUE, deleted_at = now(), updated_by = $2, updated_at = now()
             WHERE id IN (SELECT id FROM subtree) AND NOT deleted",
        )
        .bind(id)
        .bind(deleted_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn restore_asset(&self, id: Uuid, restored_by: &str) -> Result<u64, StorageError> {
        let result = sqlx::query(
            "WITH RECURSIVE subtree AS (
                 SELECT id FROM assets WHERE id = $1
                 UNION ALL
                 SELECT a.id FROM assets a JOIN subtree s ON a.parent_id = s.id
             )
             UPDATE assets SET deleted = FALSE, deleted_at = NULL, updated_by = $2, updated_at = now()
             WHERE id IN (SELECT id FROM subtree) AND deleted",
        )
        .bind(id)
        .bind(restored_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(result.rows_affected())
    }

    // ---- identity and policy (Epics 11-13) ----

    async fn find_user(&self, id: &str) -> Result<Option<StoredUser>, StorageError> {
        let row = sqlx::query(
            "SELECT u.id, u.display_name, u.email, u.is_admin, u.is_bot,
                    COALESCE(array_agg(r.role) FILTER (WHERE r.role IS NOT NULL), '{}') AS roles
             FROM users u LEFT JOIN user_roles r ON r.user_id = u.id
             WHERE u.id = $1
             GROUP BY u.id",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(|row| StoredUser {
            id: row.get("id"),
            display_name: row.get("display_name"),
            email: row.get("email"),
            is_admin: row.get("is_admin"),
            is_bot: row.get("is_bot"),
            roles: row.get("roles"),
        }))
    }

    async fn upsert_user(&self, user: &StoredUser) -> Result<(), StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO users (id, display_name, email, is_admin, is_bot)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
                 display_name = EXCLUDED.display_name,
                 email = EXCLUDED.email,
                 is_admin = EXCLUDED.is_admin,
                 is_bot = EXCLUDED.is_bot",
        )
        .bind(&user.id)
        .bind(&user.display_name)
        .bind(&user.email)
        .bind(user.is_admin)
        .bind(user.is_bot)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for role in &user.roles {
            sqlx::query("INSERT INTO roles (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(role)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            sqlx::query(
                "INSERT INTO user_roles (user_id, role) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(&user.id)
            .bind(role)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn policies_for_roles(&self, roles: &[String]) -> Result<Vec<Policy>, StorageError> {
        if roles.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT DISTINCT p.name, p.rules
             FROM policies p JOIN role_policies rp ON rp.policy = p.name
             WHERE rp.role = ANY($1)",
        )
        .bind(roles)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(Policy {
                    name: row.get("name"),
                    rules: serde_json::from_value(row.get("rules")).ok()?,
                })
            })
            .collect())
    }

    async fn upsert_policy(&self, policy: &Policy, roles: &[String]) -> Result<(), StorageError> {
        let rules = serde_json::to_value(&policy.rules)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO policies (name, rules) VALUES ($1, $2)
             ON CONFLICT (name) DO UPDATE SET rules = EXCLUDED.rules",
        )
        .bind(&policy.name)
        .bind(&rules)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // Full replace, not an addition: a role dropped from `roles` must
        // stop applying, which an INSERT-only upsert (like `upsert_user`'s
        // for `user_roles`) would silently fail to do.
        sqlx::query("DELETE FROM role_policies WHERE policy = $1")
            .bind(&policy.name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for role in roles {
            sqlx::query("INSERT INTO roles (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(role)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            sqlx::query("INSERT INTO role_policies (role, policy) VALUES ($1, $2)")
                .bind(role)
                .bind(&policy.name)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn list_policies(&self) -> Result<Vec<(Policy, Vec<String>)>, StorageError> {
        let rows = sqlx::query(
            "SELECT p.name, p.rules, rp.role
             FROM policies p LEFT JOIN role_policies rp ON rp.policy = p.name
             ORDER BY p.name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut policies: Vec<(Policy, Vec<String>)> = Vec::new();
        for row in rows {
            let name: String = row.get("name");
            let role: Option<String> = row.get("role");
            if let Some((_, roles)) = policies.iter_mut().find(|(p, _)| p.name == name) {
                roles.extend(role);
            } else {
                let rules = serde_json::from_value(row.get("rules"))
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
                policies.push((Policy { name, rules }, role.into_iter().collect()));
            }
        }
        Ok(policies)
    }

    async fn delete_policy(&self, name: &str) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM policies WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    #[tracing::instrument(name = "storage.list_assets_visible", skip_all)]
    async fn list_assets_visible(
        &self,
        filter: &graph_owl_storage::AssetFilter<'_>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            // Nothing visible. An empty page, not an error — "you may see
            // nothing here" is a legitimate answer, and 403 would leak that
            // something exists.
            return Ok(Page::from_overfetch(Vec::new(), page.limit, |a: &Asset| {
                Cursor::new(a.fully_qualified_name.clone(), a.id)
            }));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        // **The owner filter is written over `OWNERS_EXPR`, not as a second walk.**
        // The filter and the read path have to agree about who owns a thing, and
        // two copies of a nearest-owned-ancestor rule would agree right up until
        // somebody edited one — so there is one expression and both use it.
        //
        // `$7::text IS NULL` makes an absent filter a no-op rather than
        // match-nothing; without it, adding this parameter would have emptied
        // every existing list endpoint.
        //
        // **Computed once per row via `LEFT JOIN LATERAL`, not three times
        // textually** — Epic 37a Slice B's own measurement found the earlier,
        // textually-repeated form (once in the SELECT list, once inside the
        // `owner` filter's `EXISTS`, once inside `unowned`'s length check) at
        // 416ms p99 with `owner` filtering 60,246 rows, 2.8x over the 150ms
        // budget: Postgres does not recognize three copies of one recursive
        // CTE as the same computation, so it ran the five-level ancestry walk
        // up to three times per row. A `LATERAL` join runs it once and every
        // reference below reads the same result. This still walks every
        // non-deleted row once — the row-count-linear cost the code's own
        // prior comment already named as query-time's ceiling — that
        // ceiling is unchanged; only the constant factor is. If a future
        // measurement still misses budget at a larger target, the escape
        // hatch is the maintained effective-owner projection this comment
        // already flagged, not another constant-factor pass here.
        let extension = extension_clauses(filter.extension, 16);
        let tags_filter: Option<&[String]> = (!filter.tags.is_empty()).then_some(filter.tags);
        let tags = tags_expr(12);
        let certification = certification_expr(13, 14);
        let health = health_expr(15);
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, effective_owners.owners FROM assets
             LEFT JOIN LATERAL (SELECT {OWNERS_EXPR} AS owners) effective_owners ON true
             WHERE NOT deleted
               AND ($1::text IS NULL OR kind = $1)
               AND ($2::text IS NULL OR (fully_qualified_name, id) > ($2, $3))
               {VISIBILITY}
               AND ($7::text IS NULL OR EXISTS (
                     SELECT 1 FROM json_array_elements(effective_owners.owners) AS effective
                      WHERE effective->>'id' = $7))
               AND ($8::bool IS NOT TRUE OR json_array_length(effective_owners.owners) = 0)
               AND ($9::uuid IS NULL OR {DOMAIN_ID_EXPR} = $9)
               AND ($10::uuid IS NULL OR EXISTS (
                     SELECT 1 FROM data_product_assets m
                      WHERE m.asset_id = assets.id AND m.data_product_id = $10))
               AND ($11::text IS NULL OR lifecycle = $11)
               {tags}
               {certification}
               {health}
               {extension}
             ORDER BY fully_qualified_name, id
             LIMIT $4"
        );
        let mut query = sqlx::query(&sql)
            .bind(filter.kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch)
            .bind(&allow)
            .bind(&deny)
            .bind(filter.owner)
            .bind(filter.unowned)
            .bind(filter.domain)
            .bind(filter.data_product)
            .bind(filter.lifecycle.map(LifecycleState::as_str))
            .bind(tags_filter)
            .bind(filter.certification.map(CertificationFilter::as_str))
            .bind(graph_owl_core::lifecycle::DEFAULT_EXPIRY_WINDOW_DAYS as i32)
            .bind(filter.health.map(graph_owl_core::quality::Health::as_str));
        for condition in filter.extension {
            query = query.bind(&condition.name).bind(&condition.value);
        }
        self.asset_page(query, page).await
    }

    #[tracing::instrument(name = "storage.search_assets_visible", skip_all)]
    async fn search_assets_visible(
        &self,
        query: &str,
        filter: &graph_owl_storage::AssetFilter<'_>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<SearchHit>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Page::from_overfetch(
                Vec::new(),
                page.limit,
                |h: &SearchHit| Cursor::new(h.asset.fully_qualified_name.clone(), h.asset.id),
            ));
        };
        let Some(terms) = graph_owl_search::tsquery(query) else {
            return Ok(Self::empty_search_hit_page(page));
        };
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let extension = extension_clauses(filter.extension, 15);
        let tags_filter: Option<&[String]> = (!filter.tags.is_empty()).then_some(filter.tags);
        let tags = tags_expr(11);
        let certification = certification_expr(12, 13);
        let health = health_expr(14);
        // **`NULLIF(..., '')`, not the bare `ts_headline` result.** An asset
        // with no description makes `ts_headline` return `''`, not `NULL` —
        // `coalesce`d in for the same reason. A client that sees an empty
        // string cannot tell "matched, but there is nothing to excerpt" from
        // "field absent"; `NULL` collapses both into one honest answer, "no
        // snippet", the same way `Asset.description` itself is `Option`.
        // Postgres's own `MaxWords=35`/`MinWords=15` defaults are used as-is
        // rather than a locally invented number — they need no justification
        // this project would have to state, because they are not this
        // project's number to justify.
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners, {RANK_KEY} AS sort_key,
                    NULLIF(ts_headline('english', coalesce(description, ''), q.ts), '') AS snippet
             FROM assets
             {POPULARITY_JOIN}, to_tsquery('english', $1) AS q (ts)
             WHERE NOT deleted
               AND assets.search_vector @@ q.ts
               AND ($2::text IS NULL OR kind = $2)
               AND ($3::text IS NULL OR ({RANK_KEY}, id) > ($3, $4))
               {VISIBILITY_SEARCH}
               AND ($8::uuid IS NULL OR {DOMAIN_ID_EXPR} = $8)
               AND ($9::uuid IS NULL OR EXISTS (
                     SELECT 1 FROM data_product_assets m
                      WHERE m.asset_id = assets.id AND m.data_product_id = $9))
               AND ($10::text IS NULL OR lifecycle = $10)
               {tags}
               {certification}
               {health}
               {extension}
             ORDER BY {RANK_KEY}, id
             LIMIT $5"
        );
        let mut q = sqlx::query(&sql)
            .bind(terms)
            .bind(filter.kind.map(AssetKind::as_str))
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch)
            .bind(&allow)
            .bind(&deny)
            .bind(filter.domain)
            .bind(filter.data_product)
            .bind(filter.lifecycle.map(LifecycleState::as_str))
            .bind(tags_filter)
            .bind(filter.certification.map(CertificationFilter::as_str))
            .bind(graph_owl_core::lifecycle::DEFAULT_EXPIRY_WINDOW_DAYS as i32)
            .bind(filter.health.map(graph_owl_core::quality::Health::as_str));
        for condition in filter.extension {
            q = q.bind(&condition.name).bind(&condition.value);
        }
        self.ranked_search_hit_page(q, page).await
    }

    #[tracing::instrument(name = "storage.list_children_visible", skip_all)]
    async fn list_children_visible(
        &self,
        parent_id: Option<Uuid>,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
             WHERE NOT deleted
               AND (($1::uuid IS NULL AND parent_id IS NULL) OR parent_id = $1)
               AND (fully_qualified_name LIKE ANY($2))
               AND NOT (fully_qualified_name LIKE ANY($3))
             ORDER BY name"
        ))
        .bind(parent_id)
        .bind(&allow)
        .bind(&deny)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(asset_from_row).collect())
    }

    async fn count_documented_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<(i64, i64), StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok((0, 0));
        };
        // Both counts in one statement. Two queries could observe different
        // states and produce a coverage ratio above 1.
        //
        // `btrim(description) <> ''` rather than `IS NOT NULL`: whitespace is
        // not documentation, and counting it would make the number reward
        // someone typing a space into every field.
        let row = sqlx::query(
            "SELECT count(*) FILTER (WHERE description IS NOT NULL
                                       AND btrim(description) <> '') AS described,
                    count(*) AS total
             FROM assets
             WHERE NOT deleted
               AND (fully_qualified_name LIKE ANY($1))
               AND NOT (fully_qualified_name LIKE ANY($2))",
        )
        .bind(&allow)
        .bind(&deny)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok((row.get("described"), row.get("total")))
    }

    async fn recently_changed_visible(
        &self,
        limit: i64,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
             WHERE NOT deleted
               AND (fully_qualified_name LIKE ANY($1))
               AND NOT (fully_qualified_name LIKE ANY($2))
             ORDER BY updated_at DESC, id DESC
             LIMIT $3"
        ))
        .bind(&allow)
        .bind(&deny)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(asset_from_row).collect())
        .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn count_assets_by_kind_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<Vec<(AssetKind, i64)>, StorageError> {
        let Some((allow, deny)) = lower(predicate) else {
            return Ok(Vec::new());
        };
        // Counted through the same predicate as the rows. A total computed
        // before filtering says "47 results" above 12 rows, which leaks the
        // existence of 35 assets the reader may not see.
        let rows = sqlx::query(
            "SELECT kind, count(*) AS n FROM assets
             WHERE NOT deleted
               AND (fully_qualified_name LIKE ANY($1))
               AND NOT (fully_qualified_name LIKE ANY($2))
             GROUP BY kind",
        )
        .bind(&allow)
        .bind(&deny)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                AssetKind::parse(row.get::<&str, _>("kind"))
                    .ok()
                    .map(|kind| (kind, row.get::<i64, _>("n")))
            })
            .collect())
    }

    // ---- Epic 24 Slice A: glossary and terms ----

    async fn insert_glossary(
        &self,
        glossary: graph_owl_storage::Glossary,
    ) -> Result<graph_owl_storage::Glossary, StorageError> {
        let result = sqlx::query(
            "INSERT INTO glossaries (id, name, description, fully_qualified_name, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(glossary.id)
        .bind(&glossary.name)
        .bind(&glossary.description)
        .bind(&glossary.fully_qualified_name)
        .bind(glossary.created_at)
        .bind(glossary.updated_at)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            return Err(match &e {
                sqlx::Error::Database(db_err)
                    if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) =>
                {
                    let existing_id = sqlx::query_scalar(
                        "SELECT id FROM glossaries WHERE fully_qualified_name = $1",
                    )
                    .bind(&glossary.fully_qualified_name)
                    .fetch_optional(&self.pool)
                    .await
                    .ok()
                    .flatten();
                    StorageError::Conflict {
                        detail: glossary.fully_qualified_name.clone(),
                        existing_id,
                        kind: ConflictKind::Fqn,
                    }
                }
                _ => StorageError::Unexpected(e.to_string()),
            });
        }
        Ok(glossary)
    }

    async fn get_glossary(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::Glossary>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, description, fully_qualified_name, created_at, updated_at
             FROM glossaries WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(glossary_from_row))
    }

    async fn list_glossaries(&self) -> Result<Vec<graph_owl_storage::Glossary>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, name, description, fully_qualified_name, created_at, updated_at
             FROM glossaries ORDER BY fully_qualified_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(glossary_from_row).collect())
    }

    async fn delete_glossary(
        &self,
        id: Uuid,
        recursive: bool,
    ) -> Result<graph_owl_storage::GlossaryDeletion, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM glossaries WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if exists.is_none() {
            return Ok(graph_owl_storage::GlossaryDeletion::NotFound);
        }

        let term_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM glossary_terms WHERE glossary_id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // Refused rather than cascaded by default: a glossary with terms is
        // asked for explicitly, the same "unless recursive" contract as the
        // asset subtree delete (Epic 3).
        if term_count > 0 && !recursive {
            return Ok(graph_owl_storage::GlossaryDeletion::HasTerms { term_count });
        }

        // No `term_count > 0` guard: deleting zero rows is a correct no-op,
        // and a guard here would only be an optimisation with no correctness
        // value to test for.
        //
        // Every child table cascades from `glossary_terms` on its own FK, so
        // deleting the terms is enough to take the rest with them.
        sqlx::query("DELETE FROM glossary_terms WHERE glossary_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query("DELETE FROM glossaries WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(graph_owl_storage::GlossaryDeletion::Deleted)
    }

    async fn insert_term(
        &self,
        term: graph_owl_storage::GlossaryTermRecord,
    ) -> Result<graph_owl_storage::GlossaryTermRecord, StorageError> {
        let result = sqlx::query(
            "INSERT INTO glossary_terms
                (id, glossary_id, name, fully_qualified_name, definition, status,
                 synonyms, abbreviations, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(term.id)
        .bind(term.glossary_id)
        .bind(&term.name)
        .bind(&term.fully_qualified_name)
        .bind(&term.definition)
        .bind(term.status.as_str())
        .bind(&term.synonyms)
        .bind(&term.abbreviations)
        .bind(term.created_at)
        .bind(term.updated_at)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            return Err(match &e {
                sqlx::Error::Database(db_err)
                    if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) =>
                {
                    // Either the FQN or the scoped `(glossary_id, name)` pair
                    // collided; both are the same conflict from a caller's
                    // point of view — a term already exists at that address.
                    let existing_id = sqlx::query_scalar(
                        "SELECT id FROM glossary_terms WHERE fully_qualified_name = $1",
                    )
                    .bind(&term.fully_qualified_name)
                    .fetch_optional(&self.pool)
                    .await
                    .ok()
                    .flatten();
                    StorageError::Conflict {
                        detail: term.fully_qualified_name.clone(),
                        existing_id,
                        kind: ConflictKind::Fqn,
                    }
                }
                _ => StorageError::Unexpected(e.to_string()),
            });
        }
        Ok(term)
    }

    async fn get_term(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {TERM_COLUMNS} FROM glossary_terms WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(term_from_row))
    }

    async fn list_terms(
        &self,
        glossary_id: Uuid,
    ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {TERM_COLUMNS} FROM glossary_terms WHERE glossary_id = $1 ORDER BY name"
        ))
        .bind(glossary_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(term_from_row).collect())
    }

    async fn update_term(
        &self,
        id: Uuid,
        update: graph_owl_storage::GlossaryTermUpdate,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let row = sqlx::query(&format!(
            "UPDATE glossary_terms
             SET definition = COALESCE($2, definition),
                 synonyms = COALESCE($3, synonyms),
                 abbreviations = COALESCE($4, abbreviations),
                 updated_at = now()
             WHERE id = $1
             RETURNING {TERM_COLUMNS}"
        ))
        .bind(id)
        .bind(&update.definition)
        .bind(&update.synonyms)
        .bind(&update.abbreviations)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(term_from_row))
    }

    async fn delete_term(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM glossary_terms WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn search_terms(
        &self,
        query: &str,
    ) -> Result<Vec<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        // Matches the empty-query handling `search_assets` uses: `to_tsquery`
        // raises rather than returning zero rows, so an all-punctuation query
        // is answered without asking Postgres at all.
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(&format!(
            "SELECT {TERM_COLUMNS} FROM glossary_terms
             WHERE search_vector @@ websearch_to_tsquery('english', $1)
             ORDER BY ts_rank_cd(search_vector, websearch_to_tsquery('english', $1)) DESC, name"
        ))
        .bind(query)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(term_from_row).collect())
    }

    // ---- Epic 24 Slice B: SKOS relations ----

    async fn insert_term_relation(
        &self,
        term_id: Uuid,
        relation: graph_owl_core::glossary::SkosRelation,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO term_relations (term_id, kind, target)
             VALUES ($1, $2, $3)
             ON CONFLICT (term_id, kind, target) DO NOTHING",
        )
        .bind(term_id)
        .bind(relation_kind_str(&relation))
        .bind(relation.target())
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn delete_term_relation(
        &self,
        term_id: Uuid,
        relation: &graph_owl_core::glossary::SkosRelation,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "DELETE FROM term_relations WHERE term_id = $1 AND kind = $2 AND target = $3",
        )
        .bind(term_id)
        .bind(relation_kind_str(relation))
        .bind(relation.target())
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn term_relations_touching(
        &self,
        term_id: Uuid,
    ) -> Result<Vec<(String, graph_owl_core::glossary::SkosRelation)>, StorageError> {
        let id_text = term_id.to_string();
        let rows = sqlx::query(
            "SELECT term_id, kind, target FROM term_relations
             WHERE term_id = $1 OR target = $2",
        )
        .bind(term_id)
        .bind(&id_text)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let owner: Uuid = row.get("term_id");
                let kind: &str = row.get("kind");
                let target: String = row.get("target");
                relation_from_kind(kind, target).map(|relation| (owner.to_string(), relation))
            })
            .collect())
    }

    async fn broader_edges(&self) -> Result<Vec<(String, String)>, StorageError> {
        let rows = sqlx::query("SELECT term_id, target FROM term_relations WHERE kind = 'broader'")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let owner: Uuid = row.get("term_id");
                let target: String = row.get("target");
                (owner.to_string(), target)
            })
            .collect())
    }

    // ---- Epic 24 Slice C: review workflow ----

    async fn set_term_reviewers(
        &self,
        term_id: Uuid,
        reviewers: &[String],
    ) -> Result<(), StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // Replaced, not merged — same reason `upsert_team`'s membership is:
        // a partial update cannot express "nobody reviews this any more".
        sqlx::query("DELETE FROM term_reviewers WHERE term_id = $1")
            .bind(term_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for reviewer in reviewers {
            sqlx::query("INSERT INTO term_reviewers (term_id, user_id) VALUES ($1, $2)")
                .bind(term_id)
                .bind(reviewer)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    if e.as_database_error()
                        .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation)
                    {
                        StorageError::Unexpected(format!(
                            "`{reviewer}` is not a known user; a reviewer nobody can \
                             resolve is an approval nobody can act on"
                        ))
                    } else {
                        StorageError::Unexpected(e.to_string())
                    }
                })?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn term_reviewers(&self, term_id: Uuid) -> Result<Vec<String>, StorageError> {
        sqlx::query_scalar("SELECT user_id FROM term_reviewers WHERE term_id = $1 ORDER BY user_id")
            .bind(term_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn transition_term(
        &self,
        term_id: Uuid,
        from: graph_owl_core::glossary::TermStatus,
        to: graph_owl_core::glossary::TermStatus,
        actor: &str,
        reason: Option<String>,
        successor_term_id: Option<Uuid>,
    ) -> Result<Option<graph_owl_storage::GlossaryTermRecord>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let row = sqlx::query(&format!(
            "UPDATE glossary_terms
             SET status = $2,
                 deprecation_reason = $3,
                 successor_term_id = $4,
                 version_minor = version_minor + 1,
                 updated_at = now()
             WHERE id = $1
             RETURNING {TERM_COLUMNS}"
        ))
        .bind(term_id)
        .bind(to.as_str())
        .bind(&reason)
        .bind(successor_term_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };
        let updated = term_from_row(row);

        sqlx::query(
            "INSERT INTO term_transitions (id, term_id, from_status, to_status, actor, reason)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(term_id)
        .bind(from.as_str())
        .bind(to.as_str())
        .bind(actor)
        .bind(&reason)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Some(updated))
    }

    // ---- Epic 24 Slice D: terms attach to assets and columns ----

    async fn attach_term(
        &self,
        term_id: Uuid,
        target_fqn: &str,
        attached_by: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO term_attachments (term_id, target_fqn, attached_by)
             VALUES ($1, $2, $3)
             ON CONFLICT (term_id, target_fqn) DO NOTHING",
        )
        .bind(term_id)
        .bind(target_fqn)
        .bind(attached_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn detach_term(&self, term_id: Uuid, target_fqn: &str) -> Result<bool, StorageError> {
        let result =
            sqlx::query("DELETE FROM term_attachments WHERE term_id = $1 AND target_fqn = $2")
                .bind(term_id)
                .bind(target_fqn)
                .execute(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn term_usage(
        &self,
        term_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<String>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);

        // No per-row id of its own, so the term's own id fills the cursor's
        // tie-break slot — harmless, because `target_fqn` is already unique
        // within one term's attachments and a tie never occurs.
        let rows: Vec<String> = match &page.after {
            Some(cursor) => {
                sqlx::query_scalar(
                    "SELECT target_fqn FROM term_attachments
                     WHERE term_id = $1 AND target_fqn > $2
                     ORDER BY target_fqn
                     LIMIT $3",
                )
                .bind(term_id)
                .bind(&cursor.sort_key)
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query_scalar(
                    "SELECT target_fqn FROM term_attachments
                     WHERE term_id = $1
                     ORDER BY target_fqn
                     LIMIT $2",
                )
                .bind(term_id)
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Page::from_overfetch(rows, page.limit, |fqn: &String| {
            Cursor::new(fqn.clone(), term_id)
        }))
    }

    // ---- Epic 24 Slice E: Metric as a first-class entity ----

    async fn insert_metric(
        &self,
        metric: graph_owl_storage::MetricRecord,
    ) -> Result<graph_owl_storage::MetricRecord, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let result = sqlx::query(
            "INSERT INTO metrics
                (id, name, fully_qualified_name, definition, formula, unit, granularity,
                 calculation_type, defined_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(metric.id)
        .bind(&metric.name)
        .bind(&metric.fully_qualified_name)
        .bind(&metric.definition)
        .bind(&metric.formula)
        .bind(&metric.unit)
        .bind(&metric.granularity)
        .bind(metric.calculation_type.as_str())
        .bind(metric.defined_by)
        .bind(metric.created_at)
        .bind(metric.updated_at)
        .execute(&mut *tx)
        .await;

        if let Err(e) = result {
            return Err(match &e {
                sqlx::Error::Database(db_err)
                    if db_err.code().as_deref() == Some(UNIQUE_VIOLATION) =>
                {
                    let existing_id = sqlx::query_scalar(
                        "SELECT id FROM metrics WHERE fully_qualified_name = $1",
                    )
                    .bind(&metric.fully_qualified_name)
                    .fetch_optional(&self.pool)
                    .await
                    .ok()
                    .flatten();
                    StorageError::Conflict {
                        detail: metric.fully_qualified_name.clone(),
                        existing_id,
                        kind: ConflictKind::Fqn,
                    }
                }
                _ => StorageError::Unexpected(e.to_string()),
            });
        }

        for source in &metric.source_assets {
            sqlx::query("INSERT INTO metric_sources (metric_id, source_fqn) VALUES ($1, $2)")
                .bind(metric.id)
                .bind(source)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(metric)
    }

    async fn get_metric(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
        let row = sqlx::query(&format!("{METRIC_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(metric_from_row))
    }

    async fn list_metrics(
        &self,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_storage::MetricRecord>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);

        let rows = match &page.after {
            Some(cursor) => {
                sqlx::query(&format!(
                    "{METRIC_SELECT} WHERE (fully_qualified_name, id) > ($1, $2)
                     ORDER BY fully_qualified_name, id
                     LIMIT $3"
                ))
                .bind(&cursor.sort_key)
                .bind(cursor.id)
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(&format!(
                    "{METRIC_SELECT} ORDER BY fully_qualified_name, id LIMIT $1"
                ))
                .bind(overfetch)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let metrics: Vec<graph_owl_storage::MetricRecord> =
            rows.into_iter().map(metric_from_row).collect();
        Ok(Page::from_overfetch(metrics, page.limit, |metric| {
            Cursor::new(metric.fully_qualified_name.clone(), metric.id)
        }))
    }

    async fn update_metric(
        &self,
        id: Uuid,
        update: graph_owl_storage::MetricUpdate,
    ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
        let row = sqlx::query(
            "UPDATE metrics
             SET definition = COALESCE($2, definition),
                 formula = COALESCE($3, formula),
                 unit = COALESCE($4, unit),
                 granularity = COALESCE($5, granularity),
                 calculation_type = COALESCE($6, calculation_type),
                 updated_at = now()
             WHERE id = $1
             RETURNING id",
        )
        .bind(id)
        .bind(&update.definition)
        .bind(&update.formula)
        .bind(&update.unit)
        .bind(&update.granularity)
        .bind(
            update
                .calculation_type
                .map(graph_owl_core::metric::CalculationType::as_str),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        if row.is_none() {
            return Ok(None);
        }
        self.get_metric(id).await
    }

    async fn delete_metric(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM metrics WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn search_metrics(
        &self,
        query: &str,
    ) -> Result<Vec<graph_owl_storage::MetricRecord>, StorageError> {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        // Not `METRIC_SELECT`: the migration's `search_vector` is a
        // **generated** column, and Postgres generated columns cannot read
        // another table's row — so "searchable... by defining term" (Slice
        // E) has to reach the defining term's own `search_vector` through a
        // join at read time rather than through the metric's own column.
        let rows = sqlx::query(
            "SELECT metrics.id, metrics.name, metrics.fully_qualified_name, metrics.definition,
                    metrics.formula, metrics.unit, metrics.granularity, metrics.calculation_type,
                    metrics.defined_by, metrics.created_at, metrics.updated_at,
                    COALESCE(
                        (SELECT ARRAY_AGG(source_fqn ORDER BY source_fqn)
                           FROM metric_sources WHERE metric_id = metrics.id),
                        '{}'
                    ) AS source_assets
               FROM metrics
               LEFT JOIN glossary_terms term ON term.id = metrics.defined_by
              WHERE metrics.search_vector @@ websearch_to_tsquery('english', $1)
                 OR (term.id IS NOT NULL
                     AND term.search_vector @@ websearch_to_tsquery('english', $1))
              ORDER BY metrics.name",
        )
        .bind(query)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(metric_from_row).collect())
    }

    async fn update_metric_sources(
        &self,
        metric_id: Uuid,
        sources: &[String],
    ) -> Result<Option<graph_owl_storage::MetricRecord>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM metrics WHERE id = $1")
            .bind(metric_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if exists.is_none() {
            return Ok(None);
        }

        // Replaced wholesale rather than diffed here: the facade already
        // reconciled the *content* (dedup, self-reference excluded) via
        // `graph_owl_core::metric::reconcile_lineage` before calling this,
        // so this is a plain "make the stored set match" — every row here is
        // metric-declared, so there is no hand-drawn edge in this table for
        // a diff to protect.
        sqlx::query("DELETE FROM metric_sources WHERE metric_id = $1")
            .bind(metric_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for source in sources {
            sqlx::query("INSERT INTO metric_sources (metric_id, source_fqn) VALUES ($1, $2)")
                .bind(metric_id)
                .bind(source)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        let row = sqlx::query(&format!("{METRIC_SELECT} WHERE id = $1"))
            .bind(metric_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(metric_from_row))
    }

    // ---- Epic 21: extraction runs and the confirmation queue ----

    async fn find_extraction_run(
        &self,
        source_id: &str,
        fingerprint: &str,
        extractor: &str,
        version: &str,
    ) -> Result<Option<ExtractionRunRecord>, StorageError> {
        let row = sqlx::query(
            "SELECT id, source_id, source_fingerprint, extractor, extractor_version,
                    source_text, media_type, asserted, surfaced, discarded
             FROM extraction_runs
             WHERE source_id = $1 AND source_fingerprint = $2
               AND extractor = $3 AND extractor_version = $4
             ORDER BY started_at DESC
             LIMIT 1",
        )
        .bind(source_id)
        .bind(fingerprint)
        .bind(extractor)
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(|row| ExtractionRunRecord {
            id: row.get("id"),
            source_id: row.get("source_id"),
            source_fingerprint: row.get("source_fingerprint"),
            extractor: row.get("extractor"),
            extractor_version: row.get("extractor_version"),
            source_text: row.get("source_text"),
            media_type: row.get("media_type"),
            asserted: row.get("asserted"),
            surfaced: row.get("surfaced"),
            discarded: row.get("discarded"),
        }))
    }

    async fn find_extraction_run_by_id(
        &self,
        run_id: Uuid,
    ) -> Result<Option<ExtractionRunRecord>, StorageError> {
        let row = sqlx::query(
            "SELECT id, source_id, source_fingerprint, extractor, extractor_version,
                    source_text, media_type, asserted, surfaced, discarded
             FROM extraction_runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.map(|row| ExtractionRunRecord {
            id: row.get("id"),
            source_id: row.get("source_id"),
            source_fingerprint: row.get("source_fingerprint"),
            extractor: row.get("extractor"),
            extractor_version: row.get("extractor_version"),
            source_text: row.get("source_text"),
            media_type: row.get("media_type"),
            asserted: row.get("asserted"),
            surfaced: row.get("surfaced"),
            discarded: row.get("discarded"),
        }))
    }

    async fn save_extraction_run(
        &self,
        run: &ExtractionRunRecord,
        queued: &[QueuedClaimRecord],
        discarded: &[DiscardedClaimRecord],
    ) -> Result<(), StorageError> {
        // One transaction. Claims written without their run row would be
        // unattributable assertions that nothing can delete wholesale — the
        // exact property decision 0 buys by scoping extraction to a named
        // graph, lost to a partial write.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO extraction_runs
                (id, source_id, source_fingerprint, extractor, extractor_version,
                 source_text, media_type, asserted, surfaced, discarded)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(run.id)
        .bind(&run.source_id)
        .bind(&run.source_fingerprint)
        .bind(&run.extractor)
        .bind(&run.extractor_version)
        .bind(&run.source_text)
        .bind(&run.media_type)
        .bind(run.asserted)
        .bind(run.surfaced)
        .bind(run.discarded)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for claim in queued {
            sqlx::query(
                "INSERT INTO extraction_claims
                    (id, run_id, subject, predicate, object, confidence,
                     evidence_start, evidence_end, state)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(claim.id)
            .bind(run.id)
            .bind(&claim.subject)
            .bind(&claim.predicate)
            .bind(&claim.object)
            .bind(claim.confidence)
            .bind(claim.evidence_start)
            .bind(claim.evidence_end)
            .bind(&claim.state)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        for discard in discarded {
            sqlx::query(
                "INSERT INTO extraction_discards
                    (id, run_id, subject, predicate, object, confidence, reason)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(discard.id)
            .bind(run.id)
            .bind(&discard.subject)
            .bind(&discard.predicate)
            .bind(&discard.object)
            .bind(discard.confidence)
            .bind(&discard.reason)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn pending_extraction_claims(
        &self,
        limit: i64,
    ) -> Result<Vec<QueuedClaimRecord>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, run_id, subject, predicate, object, confidence,
                    evidence_start, evidence_end, state, decided_by, reason
             FROM extraction_claims
             WHERE state = 'pending'
             ORDER BY queued_at ASC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.iter().map(queued_claim_from_row).collect())
    }

    async fn decide_extraction_claim(
        &self,
        claim_id: Uuid,
        decision: graph_owl_core::extraction::ReviewDecision,
        decided_by: &str,
    ) -> Result<Option<QueuedClaimRecord>, StorageError> {
        use graph_owl_core::extraction::ReviewDecision;

        // `Accept` sends no correction, so `COALESCE` keeps the extractor's
        // own subject/predicate/object; `Edit` overwrites all three with the
        // reviewer's. Splitting subject/predicate/object into `Option`s
        // rather than writing two different SQL statements keeps this one
        // atomic `RETURNING` rather than a read-modify-write with a race
        // window between them.
        let (subject, predicate, object, reason) = match &decision {
            ReviewDecision::Accept => (None, None, None, None),
            ReviewDecision::Edit {
                subject,
                predicate,
                object,
            } => (
                Some(subject.as_str()),
                Some(predicate.as_str()),
                Some(object.as_str()),
                None,
            ),
            ReviewDecision::Reject { reason } => (None, None, None, Some(reason.as_str())),
        };

        // `RETURNING` rather than update-then-read: the read would be a second
        // statement against a row another reviewer may have decided in
        // between, and the answer this returns is what the caller is told
        // happened.
        let row = sqlx::query(
            "UPDATE extraction_claims
             SET state = $2, decided_at = now(), decided_by = $3, reason = $4,
                 subject = COALESCE($5, subject),
                 predicate = COALESCE($6, predicate),
                 object = COALESCE($7, object)
             WHERE id = $1
             RETURNING id, run_id, subject, predicate, object, confidence,
                       evidence_start, evidence_end, state, decided_by, reason",
        )
        .bind(claim_id)
        .bind(decision.state())
        .bind(decided_by)
        .bind(reason)
        .bind(subject)
        .bind(predicate)
        .bind(object)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.as_ref().map(queued_claim_from_row))
    }

    async fn rejected_assertions(&self) -> Result<Vec<(String, String, String)>, StorageError> {
        let rows = sqlx::query(
            "SELECT DISTINCT subject, predicate, object
             FROM extraction_claims WHERE state = 'rejected'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|row| (row.get("subject"), row.get("predicate"), row.get("object")))
            .collect())
    }

    // ---- Epic 22: organization-defined custom properties ----

    async fn define_custom_property(
        &self,
        id: Uuid,
        property: &CustomProperty,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO custom_properties
                (id, name, entity_type, property_type, description, constraints)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(&property.name)
        .bind(&property.entity_type)
        .bind(property.property_type.as_str())
        .bind(&property.description)
        .bind(serde_json::to_value(&property.constraints).unwrap_or_default())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                // The unique index is on `(entity_type, name)`, so this fires
                // only for a genuine collision on that pair — the same name on
                // a different type is a different property and inserts fine.
                StorageError::Conflict {
                    detail: format!(
                        "`{}` is already defined on `{}`",
                        property.name, property.entity_type
                    ),
                    existing_id: None,
                    kind: ConflictKind::CustomPropertyExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;
        Ok(())
    }

    async fn list_custom_properties(
        &self,
        entity_type: Option<&str>,
    ) -> Result<Vec<(Uuid, CustomProperty)>, StorageError> {
        // One query with a null-guard rather than two: `$1 IS NULL OR ...` lets
        // the filtered and unfiltered reads share a plan and a code path, and
        // the index on `entity_type` still applies when it is supplied.
        let rows = sqlx::query(
            "SELECT id, name, entity_type, property_type, description, constraints
             FROM custom_properties
             WHERE $1::text IS NULL OR entity_type = $1
             ORDER BY entity_type, name",
        )
        .bind(entity_type)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|row| (row.get("id"), custom_property_from_row(row)))
            .collect())
    }

    async fn get_custom_property(&self, id: Uuid) -> Result<Option<CustomProperty>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, entity_type, property_type, description, constraints
             FROM custom_properties WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.as_ref().map(custom_property_from_row))
    }

    async fn count_custom_property_values(
        &self,
        entity_type: &str,
        name: &str,
    ) -> Result<i64, StorageError> {
        // `? ` asks whether the key is present at all, which is the right
        // question: a property explicitly set to null has been cleared, and
        // counting it would refuse a delete over values nobody holds.
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM assets
             WHERE kind::text = $1 AND extension ? $2 AND extension -> $2 <> 'null'::jsonb",
        )
        .bind(entity_type)
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(count)
    }

    async fn delete_custom_property(&self, id: Uuid) -> Result<bool, StorageError> {
        let done = sqlx::query("DELETE FROM custom_properties WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    // ---- Epic 30: quality signals ----

    async fn create_test_definition(
        &self,
        id: Uuid,
        name: &str,
        test_type: &str,
        description: Option<&str>,
        expected_cadence: Option<&str>,
    ) -> Result<StoredTestDefinition, StorageError> {
        sqlx::query(
            "INSERT INTO test_definitions (id, name, test_type, description, expected_cadence)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(test_type)
        .bind(description)
        .bind(expected_cadence)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            conflict_or_unexpected(
                &e,
                format!("a test definition named `{name}` already exists"),
            )
        })?;

        Ok(StoredTestDefinition {
            id,
            name: name.to_string(),
            test_type: test_type.to_string(),
            description: description.map(str::to_string),
            expected_cadence: expected_cadence.map(str::to_string),
        })
    }

    async fn list_test_definitions(&self) -> Result<Vec<StoredTestDefinition>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, name, test_type, description, expected_cadence
               FROM test_definitions ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| StoredTestDefinition {
                id: row.get("id"),
                name: row.get("name"),
                test_type: row.get("test_type"),
                description: row.get("description"),
                expected_cadence: row.get("expected_cadence"),
            })
            .collect())
    }

    async fn set_definition_cadence(
        &self,
        id: Uuid,
        expected_cadence: Option<&str>,
    ) -> Result<Option<i64>, StorageError> {
        let done = sqlx::query(
            "UPDATE test_definitions SET expected_cadence = $2, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(expected_cadence)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if done.rows_affected() == 0 {
            return Ok(None);
        }
        // The cases that *inherit* — one row edited, N cases now resolving
        // differently, which is the whole point of the definition/case split.
        // Cases with their own cadence are deliberately not counted: they said
        // something different on purpose.
        let inherited: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM test_cases
              WHERE definition_id = $1 AND expected_cadence IS NULL",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Some(inherited))
    }

    async fn create_test_suite(
        &self,
        id: Uuid,
        name: &str,
        owner: Option<&str>,
        description: Option<&str>,
    ) -> Result<Option<Uuid>, StorageError> {
        if let Some(owner) = owner {
            let found: Option<String> = sqlx::query_scalar("SELECT id FROM teams WHERE id = $1")
                .bind(owner)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if found.is_none() {
                return Ok(None);
            }
        }
        sqlx::query(
            "INSERT INTO test_suites (id, name, owner, description) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(name)
        .bind(owner)
        .bind(description)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            conflict_or_unexpected(&e, format!("a test suite named `{name}` already exists"))
        })?;
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
    ) -> Result<Option<StoredTestCase>, StorageError> {
        // The target must be a live asset — a case on a name nothing resolves
        // to produces results nobody can navigate to.
        let live: Option<bool> =
            sqlx::query_scalar("SELECT deleted FROM assets WHERE fully_qualified_name = $1")
                .bind(target_fqn)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if live != Some(false) {
            return Ok(None);
        }
        if let Some(definition) = definition_id {
            let found: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM test_definitions WHERE id = $1")
                    .bind(definition)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if found.is_none() {
                return Ok(None);
            }
        }
        if let Some(suite) = suite_id {
            let found: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM test_suites WHERE id = $1")
                    .bind(suite)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if found.is_none() {
                return Ok(None);
            }
        }

        sqlx::query(
            "INSERT INTO test_cases
                 (id, name, target_fqn, test_type, description, definition_id, suite_id, expected_cadence)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(name)
        .bind(target_fqn)
        .bind(test_type)
        .bind(description)
        .bind(definition_id)
        .bind(suite_id)
        .bind(expected_cadence)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            conflict_or_unexpected(&e, format!("`{name}` is already a test case on `{target_fqn}`"))
        })?;

        self.get_test_case(id).await
    }

    async fn list_test_cases(
        &self,
        target_fqn: Option<&str>,
        suite_id: Option<Uuid>,
    ) -> Result<Vec<StoredTestCase>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {TEST_CASE_COLUMNS} FROM test_cases c
               LEFT JOIN test_definitions d ON d.id = c.definition_id
              WHERE ($1::text IS NULL OR c.target_fqn = $1)
                AND ($2::uuid IS NULL OR c.suite_id = $2)
              ORDER BY c.target_fqn, c.name"
        ))
        .bind(target_fqn)
        .bind(suite_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(test_case_from_row).collect())
    }

    async fn delete_test_case(&self, id: Uuid) -> Result<bool, StorageError> {
        // Results go with it by `ON DELETE CASCADE` — an observation about a
        // check nobody declared is unattributable.
        let done = sqlx::query("DELETE FROM test_cases WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn record_test_results(
        &self,
        batch: &[TestResultWrite],
    ) -> Result<ResultIngest, StorageError> {
        let mut ingest = ResultIngest::default();
        let now = chrono::Utc::now();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for result in batch {
            if result.observed_at > now {
                ingest.rejected += 1;
                continue;
            }
            let known: Option<Uuid> = sqlx::query_scalar("SELECT id FROM test_cases WHERE id = $1")
                .bind(result.case_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if known.is_none() {
                ingest.unknown_case += 1;
                continue;
            }

            // `ON CONFLICT DO NOTHING` against `(case, observed_at)`: a retried
            // push is normal and must not double-count.
            let inserted = sqlx::query(
                "INSERT INTO test_results (id, case_id, status, observed_at, message, metrics)
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(result.case_id)
            .bind(result.status.as_str())
            .bind(result.observed_at)
            .bind(&result.message)
            .bind(&result.metrics)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            if inserted.rows_affected() == 0 {
                ingest.duplicates += 1;
            } else {
                ingest.accepted += 1;
            }
        }

        // **No version bump and no change event** (decision 2). Deliberately
        // absent rather than forgotten: a nightly suite across ten thousand
        // tables would otherwise fill every history with observations, and the
        // version tracks descriptive change.
        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(ingest)
    }

    async fn test_results(
        &self,
        case_id: Uuid,
        limit: i64,
    ) -> Result<Vec<StoredTestResult>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, case_id, status, observed_at, message, metrics
               FROM test_results WHERE case_id = $1
              ORDER BY observed_at DESC LIMIT $2",
        )
        .bind(case_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(result_from_row).collect())
    }

    async fn latest_results_for(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<graph_owl_core::quality::LatestResult>, StorageError> {
        // `LEFT JOIN LATERAL` so a case with no results still produces a row —
        // a registered check that has never run is a *stale* case, not an
        // absent one, and an inner join would silently make it invisible.
        let rows = sqlx::query(
            "SELECT c.name,
                    coalesce(c.expected_cadence, d.expected_cadence) AS expected_cadence,
                    r.status, r.observed_at
               FROM test_cases c
               LEFT JOIN test_definitions d ON d.id = c.definition_id
               LEFT JOIN LATERAL (
                   SELECT status, observed_at FROM test_results
                    WHERE case_id = c.id ORDER BY observed_at DESC LIMIT 1
               ) r ON TRUE
              WHERE c.target_fqn = $1
              ORDER BY c.name",
        )
        .bind(target_fqn)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|row| graph_owl_core::quality::LatestResult {
                case_name: row.get("name"),
                status: row
                    .get::<Option<String>, _>("status")
                    .and_then(|raw| graph_owl_core::quality::TestStatus::parse(&raw).ok()),
                observed_at: row.get("observed_at"),
                // An unparseable cadence is treated as none rather than
                // panicking: it was validated on the way in, so reaching here
                // means a migration widened the column, and a read that dies is
                // worse than one that is conservative.
                cadence: row
                    .get::<Option<String>, _>("expected_cadence")
                    .and_then(|raw| graph_owl_core::quality::parse_cadence(&raw).ok()),
            })
            .collect())
    }

    async fn prune_test_results(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, StorageError> {
        // **The latest per case survives regardless of age**, or pruning would
        // blank the health signal it exists to support — and would do it worst
        // for exactly the infrequently-tested assets whose signal is scarcest.
        let pruned = sqlx::query(
            "DELETE FROM test_results r
              WHERE r.observed_at < $1
                AND r.id <> (SELECT keep.id FROM test_results keep
                              WHERE keep.case_id = r.case_id
                              ORDER BY keep.observed_at DESC, keep.id
                              LIMIT 1)",
        )
        .bind(before)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();
        Ok(i64::try_from(pruned).unwrap_or(i64::MAX))
    }

    // ---- Epic 29 Slices D and E ----

    async fn set_column_mappings(
        &self,
        edge_id: Uuid,
        mappings: &[ColumnMapping],
    ) -> Result<Option<i64>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM lineage_edges WHERE id = $1")
            .bind(edge_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if exists.is_none() {
            return Ok(None);
        }

        // Every named column has to resolve. A mapping to a column that does
        // not exist is a lineage claim nothing can render, and it would sit
        // there looking like coverage.
        for mapping in mappings {
            for fqn in [&mapping.from_column_fqn, &mapping.to_column_fqn] {
                let found: Option<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM assets
                      WHERE fully_qualified_name = $1 AND kind = 'column' AND NOT deleted",
                )
                .bind(fqn)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
                if found.is_none() {
                    return Ok(None);
                }
            }
        }

        sqlx::query("DELETE FROM lineage_column_mappings WHERE edge_id = $1")
            .bind(edge_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for mapping in mappings {
            sqlx::query(
                "INSERT INTO lineage_column_mappings
                     (edge_id, from_column_fqn, to_column_fqn, expression)
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            )
            .bind(edge_id)
            .bind(&mapping.from_column_fqn)
            .bind(&mapping.to_column_fqn)
            .bind(&mapping.expression)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Some(i64::try_from(mappings.len()).unwrap_or(i64::MAX)))
    }

    async fn column_mappings(&self, edge_id: Uuid) -> Result<Vec<ColumnMapping>, StorageError> {
        let rows = sqlx::query(
            "SELECT from_column_fqn, to_column_fqn, expression
               FROM lineage_column_mappings WHERE edge_id = $1
              ORDER BY to_column_fqn, from_column_fqn",
        )
        .bind(edge_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| ColumnMapping {
                from_column_fqn: row.get("from_column_fqn"),
                to_column_fqn: row.get("to_column_fqn"),
                expression: row.get("expression"),
            })
            .collect())
    }

    async fn reconcile_lineage(
        &self,
        source: &str,
        scope_prefix: &str,
        asserted: &[(Uuid, Uuid, String)],
        created_by: &str,
    ) -> Result<LineageReconciliation, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut report = LineageReconciliation::default();

        for (from, to, relationship) in asserted {
            let inserted = sqlx::query(
                "INSERT INTO lineage_edges
                     (id, from_asset_id, to_asset_id, relationship, source, created_by)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (from_asset_id, to_asset_id, relationship, source) DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(from)
            .bind(to)
            .bind(relationship)
            .bind(source)
            .bind(created_by)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if inserted.rows_affected() > 0 {
                report.added += 1;
            }
        }

        // **Scoped by source AND by prefix.** Source-blind replacement silently
        // deletes lineage a human curated, which is the failure this exists to
        // prevent. Scope-blind replacement deletes edges in schemas this run
        // never looked at — the same bug wearing a different hat.
        let keep: Vec<Uuid> = Vec::new();
        let removed = sqlx::query(
            "DELETE FROM lineage_edges e
              USING assets a
              WHERE e.from_asset_id = a.id
                AND e.source = $1
                AND (a.fully_qualified_name = $2 OR a.fully_qualified_name LIKE $2 || '.%')
                AND NOT EXISTS (
                    SELECT 1 FROM unnest($3::uuid[], $4::uuid[], $5::text[])
                              AS asserted(from_id, to_id, rel)
                     WHERE asserted.from_id = e.from_asset_id
                       AND asserted.to_id = e.to_asset_id
                       AND asserted.rel = e.relationship)",
        )
        .bind(source)
        .bind(scope_prefix)
        .bind(
            asserted
                .iter()
                .map(|(from, _, _)| *from)
                .collect::<Vec<_>>(),
        )
        .bind(asserted.iter().map(|(_, to, _)| *to).collect::<Vec<_>>())
        .bind(
            asserted
                .iter()
                .map(|(_, _, rel)| rel.clone())
                .collect::<Vec<_>>(),
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();
        let _ = keep;
        report.removed = i64::try_from(removed).unwrap_or(i64::MAX);

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(report)
    }

    // ---- Epic 27: data contracts ----

    async fn create_contract(
        &self,
        id: Uuid,
        contract: &Contract,
    ) -> Result<Option<Contract>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // Producer, consumers and asset all have to resolve. A contract whose
        // parties do not exist is a promise nobody is accountable for, which is
        // exactly what decision 1 makes it an entity to avoid.
        let producer: Option<String> = sqlx::query_scalar("SELECT id FROM teams WHERE id = $1")
            .bind(&contract.producer)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if producer.is_none() {
            return Ok(None);
        }
        let live_asset: Option<bool> =
            sqlx::query_scalar("SELECT deleted FROM assets WHERE fully_qualified_name = $1")
                .bind(&contract.asset_fqn)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if live_asset != Some(false) {
            return Ok(None);
        }
        for consumer in &contract.consumers {
            let found: Option<String> = sqlx::query_scalar("SELECT id FROM teams WHERE id = $1")
                .bind(consumer)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if found.is_none() {
                return Ok(None);
            }
        }

        sqlx::query(
            "INSERT INTO contracts
                 (id, name, asset_fqn, producer, compatibility, status, allow_additional, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(&contract.name)
        .bind(&contract.asset_fqn)
        .bind(&contract.producer)
        .bind(contract.compatibility.as_str())
        .bind(contract.status.as_str())
        .bind(contract.schema_guarantee.allow_additional)
        .bind(&contract.updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for consumer in &contract.consumers {
            sqlx::query(
                "INSERT INTO contract_consumers (contract_id, team_id) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(consumer)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }
        for column in &contract.schema_guarantee.required_columns {
            sqlx::query(
                "INSERT INTO contract_columns (contract_id, name, data_type, nullable)
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(&column.name)
            .bind(&column.data_type)
            .bind(column.nullable)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }
        for sla in &contract.slas {
            sqlx::query(
                "INSERT INTO contract_slas (id, contract_id, definition) VALUES ($1, $2, $3)",
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(serde_json::to_value(sla).unwrap_or_default())
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut created = contract.clone();
        created.id = id;
        Ok(Some(created))
    }

    async fn get_contract(&self, id: Uuid) -> Result<Option<StoredContract>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {CONTRACT_COLUMNS} FROM contracts WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some(row) = row else { return Ok(None) };

        let contract = self.hydrate_contract(&row).await?;
        let breaches = sqlx::query(
            "SELECT id, contract_id, column_name, detail, asset_version, detected_at
               FROM contract_breaches WHERE contract_id = $1 ORDER BY detected_at DESC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Some(StoredContract {
            contract,
            breaches: breaches.iter().map(breach_from_row).collect(),
        }))
    }

    async fn list_contracts(&self, asset_fqn: Option<&str>) -> Result<Vec<Contract>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {CONTRACT_COLUMNS} FROM contracts
              WHERE $1::text IS NULL OR asset_fqn = $1
              ORDER BY name, id"
        ))
        .bind(asset_fqn)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut contracts = Vec::with_capacity(rows.len());
        for row in &rows {
            contracts.push(self.hydrate_contract(row).await?);
        }
        Ok(contracts)
    }

    async fn set_contract_status(
        &self,
        id: Uuid,
        status: ContractStatus,
        updated_by: &str,
    ) -> Result<bool, StorageError> {
        let done = sqlx::query(
            "UPDATE contracts
                SET status = $2, version_minor = version_minor + 1,
                    updated_by = $3, updated_at = now()
              WHERE id = $1",
        )
        .bind(id)
        .bind(status.as_str())
        .bind(updated_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn evaluate_schema_change(
        &self,
        asset_fqn: &str,
        change: &SchemaChange,
        asset_version: &str,
    ) -> Result<Vec<BreachReport>, StorageError> {
        // Read the contracts *outside* the write transaction: the evaluation is
        // pure and the writes are per-breach, so holding a transaction open
        // across the whole set would serialise every schema change in the
        // estate behind the slowest contract.
        let contracts = self.list_contracts(Some(asset_fqn)).await?;

        let mut reports = Vec::new();
        for contract in contracts {
            if !contract.status.is_enforced() {
                continue;
            }
            let verdict = graph_owl_core::contract::check_compatibility(
                change,
                &contract.schema_guarantee,
                contract.compatibility,
            );
            let graph_owl_core::contract::Compatibility::Breach { column, detail } = verdict else {
                continue;
            };

            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            sqlx::query(
                "INSERT INTO contract_breaches
                     (id, contract_id, column_name, detail, asset_version)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(Uuid::new_v4())
            .bind(contract.id)
            .bind(&column)
            .bind(&detail)
            .bind(asset_version)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            sqlx::query(
                "UPDATE contracts SET status = 'violated',
                     version_minor = version_minor + 1, updated_at = now()
                  WHERE id = $1",
            )
            .bind(contract.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            tx.commit()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            reports.push(BreachReport {
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
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM contracts WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if exists.is_none() {
            return Ok(None);
        }

        let cleared = sqlx::query("DELETE FROM contract_breaches WHERE contract_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?
            .rows_affected();
        sqlx::query(
            "UPDATE contracts SET status = 'active',
                 version_minor = version_minor + 1, updated_by = $2, updated_at = now()
              WHERE id = $1 AND status = 'violated'",
        )
        .bind(id)
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Some(i64::try_from(cleared).unwrap_or(i64::MAX)))
    }

    // ---- Epic 28: usage and popularity ----

    async fn record_usage(&self, batch: &[UsageWrite]) -> Result<UsageIngest, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let mut ingest = UsageIngest::default();
        let now = chrono::Utc::now();

        for observation in batch {
            // An observation dated in the future is a clock problem, and storing
            // it would make every window computation wrong until it passed.
            if observation.occurred_at > now {
                ingest.rejected += 1;
                continue;
            }

            let inserted = sqlx::query(
                "INSERT INTO usage_observations
                     (id, asset_fqn, consumer_key, operation, occurred_at,
                      row_count, duration_ms, query_id, query_text)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(&observation.asset_fqn)
            .bind(observation.consumer.key())
            .bind(observation.operation.as_str())
            .bind(observation.occurred_at)
            .bind(observation.row_count)
            .bind(observation.duration_ms)
            .bind(&observation.query_id)
            .bind(&observation.query_text)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            if inserted.rows_affected() == 0 {
                ingest.duplicates += 1;
                continue;
            }

            // Folded in immediately rather than re-scanned later. A late
            // arrival for a past day lands on that day's row, which is what the
            // `(asset, consumer, day, operation)` key is for.
            sqlx::query(
                "INSERT INTO usage_rollups
                     (asset_fqn, consumer_key, day, operation, count, total_rows)
                 VALUES ($1, $2, $3::timestamptz::date, $4, 1, $5)
                 ON CONFLICT (asset_fqn, consumer_key, day, operation)
                 DO UPDATE SET count = usage_rollups.count + 1,
                               total_rows = coalesce(usage_rollups.total_rows, 0)
                                            + coalesce(EXCLUDED.total_rows, 0)",
            )
            .bind(&observation.asset_fqn)
            .bind(observation.consumer.key())
            .bind(observation.occurred_at)
            .bind(observation.operation.as_str())
            .bind(observation.row_count)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            // Kept separately so pruning cannot erase it — Slice E's criterion.
            // `GREATEST` because a batch may arrive out of order.
            sqlx::query(
                "INSERT INTO usage_last_accessed (asset_fqn, occurred_at) VALUES ($1, $2)
                 ON CONFLICT (asset_fqn)
                 DO UPDATE SET occurred_at = GREATEST(usage_last_accessed.occurred_at, EXCLUDED.occurred_at)",
            )
            .bind(&observation.asset_fqn)
            .bind(observation.occurred_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            let known: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM assets WHERE fully_qualified_name = $1")
                    .bind(&observation.asset_fqn)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if known.is_none() {
                ingest.unmatched += 1;
            }
            ingest.accepted += 1;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(ingest)
    }

    async fn usage_rollups(&self, asset_fqn: &str) -> Result<Vec<UsageRollup>, StorageError> {
        let rows = sqlx::query(
            "SELECT consumer_key, day, operation, count, total_rows
               FROM usage_rollups WHERE asset_fqn = $1 ORDER BY day DESC, consumer_key",
        )
        .bind(asset_fqn)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(rollup_from_row).collect())
    }

    async fn last_accessed(
        &self,
        asset_fqn: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError> {
        sqlx::query_scalar("SELECT occurred_at FROM usage_last_accessed WHERE asset_fqn = $1")
            .bind(asset_fqn)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn rebuild_usage_rollups(&self, asset_fqn: &str) -> Result<i64, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query("DELETE FROM usage_rollups WHERE asset_fqn = $1")
            .bind(asset_fqn)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rebuilt = sqlx::query(
            "INSERT INTO usage_rollups (asset_fqn, consumer_key, day, operation, count, total_rows)
             SELECT asset_fqn, consumer_key, occurred_at::date, operation,
                    COUNT(*), NULLIF(SUM(coalesce(row_count, 0)), 0)
               FROM usage_observations WHERE asset_fqn = $1
              GROUP BY asset_fqn, consumer_key, occurred_at::date, operation",
        )
        .bind(asset_fqn)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(i64::try_from(rebuilt).unwrap_or(i64::MAX))
    }

    async fn prune_usage(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, StorageError> {
        // **The most recent observation per asset survives**, whatever its age.
        // `last_accessed` lives in its own table for the same reason, but the
        // raw row is what a later rebuild or an audit would need, and deleting
        // the only evidence an asset was ever used is not pruning.
        let pruned = sqlx::query(
            "DELETE FROM usage_observations o
              WHERE o.occurred_at < $1
                AND o.id <> (SELECT keep.id FROM usage_observations keep
                              WHERE keep.asset_fqn = o.asset_fqn
                              ORDER BY keep.occurred_at DESC, keep.id
                              LIMIT 1)",
        )
        .bind(before)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();
        Ok(i64::try_from(pruned).unwrap_or(i64::MAX))
    }

    async fn resolve_usage_consumer(
        &self,
        identifier: &str,
        principal_id: &str,
    ) -> Result<i64, StorageError> {
        let opaque = format!("opaque:{identifier}");
        let principal = format!("principal:{principal_id}");

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query("UPDATE usage_observations SET consumer_key = $2 WHERE consumer_key = $1")
            .bind(&opaque)
            .bind(&principal)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // **Merge rather than rename**, because the principal may already have
        // a row for that day: two keys becoming one has to add, or resolving a
        // consumer would silently discard whichever half was already resolved.
        let moved = sqlx::query(
            "WITH moved AS (DELETE FROM usage_rollups WHERE consumer_key = $1 RETURNING *)
             INSERT INTO usage_rollups (asset_fqn, consumer_key, day, operation, count, total_rows)
             SELECT asset_fqn, $2, day, operation, count, total_rows FROM moved
             ON CONFLICT (asset_fqn, consumer_key, day, operation)
             DO UPDATE SET count = usage_rollups.count + EXCLUDED.count,
                           total_rows = coalesce(usage_rollups.total_rows, 0)
                                        + coalesce(EXCLUDED.total_rows, 0)",
        )
        .bind(&opaque)
        .bind(&principal)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(i64::try_from(moved).unwrap_or(i64::MAX))
    }

    // ---- Epic 25: tags and classifications ----

    async fn create_classification(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        mutually_exclusive: bool,
        updated_by: &str,
    ) -> Result<Classification, StorageError> {
        let row = sqlx::query(&format!(
            "INSERT INTO classifications (id, name, description, mutually_exclusive, updated_by)
             VALUES ($1, $2, $3, $4, $5) RETURNING {CLASSIFICATION_COLUMNS}"
        ))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(mutually_exclusive)
        .bind(updated_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            conflict_or_unexpected(
                &e,
                format!("a classification named `{name}` already exists"),
            )
        })?;
        Ok(classification_from_row(&row))
    }

    async fn get_classification(&self, id: Uuid) -> Result<Option<Classification>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {CLASSIFICATION_COLUMNS} FROM classifications WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.as_ref().map(classification_from_row))
    }

    async fn list_classifications(&self) -> Result<Vec<Classification>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {CLASSIFICATION_COLUMNS} FROM classifications ORDER BY name"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(classification_from_row).collect())
    }

    async fn delete_classification(
        &self,
        id: Uuid,
        recursive: bool,
    ) -> Result<Result<bool, i64>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let tags: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tags WHERE classification_id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if tags > 0 && !recursive {
            return Ok(Err(tags));
        }
        if recursive {
            // The labels go with the tags. `tag_labels` is `ON DELETE RESTRICT`
            // against `tags` deliberately — a governance label must not vanish
            // as a side effect — so a recursive delete clears them explicitly,
            // which is the caller having said so.
            sqlx::query(
                "DELETE FROM tag_labels WHERE tag_id IN (SELECT id FROM tags WHERE classification_id = $1)",
            )
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            sqlx::query("DELETE FROM tags WHERE classification_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }
        let done = sqlx::query("DELETE FROM classifications WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Ok(done.rows_affected() > 0))
    }

    async fn create_tag(
        &self,
        id: Uuid,
        classification_id: Uuid,
        name: &str,
        description: Option<&str>,
        updated_by: &str,
    ) -> Result<Option<Tag>, StorageError> {
        let classification: Option<String> =
            sqlx::query_scalar("SELECT name FROM classifications WHERE id = $1")
                .bind(classification_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some(classification) = classification else {
            return Ok(None);
        };
        let fqn = graph_owl_core::classification::tag_fqn(&classification, name);

        let row = sqlx::query(&format!(
            "INSERT INTO tags (id, name, classification_id, fully_qualified_name, description, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING {TAG_COLUMNS}"
        ))
        .bind(id)
        .bind(name)
        .bind(classification_id)
        .bind(&fqn)
        .bind(description)
        .bind(updated_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| conflict_or_unexpected(&e, format!("`{fqn}` already exists")))?;
        Ok(Some(tag_from_row(&row)))
    }

    async fn get_tag_by_fqn(&self, fqn: &str) -> Result<Option<Tag>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {TAG_COLUMNS} FROM tags WHERE fully_qualified_name = $1"
        ))
        .bind(fqn)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.as_ref().map(tag_from_row))
    }

    async fn list_tags(&self, classification_id: Option<Uuid>) -> Result<Vec<Tag>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {TAG_COLUMNS} FROM tags
              WHERE $1::uuid IS NULL OR classification_id = $1
              ORDER BY fully_qualified_name"
        ))
        .bind(classification_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(tag_from_row).collect())
    }

    async fn apply_tag(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        label_type: LabelType,
        state: LabelState,
        applied_by: &str,
    ) -> Result<LabelOutcome, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let tag: Option<(Uuid, Uuid, bool)> = sqlx::query_as(
            "SELECT t.id, t.classification_id, c.mutually_exclusive
               FROM tags t JOIN classifications c ON c.id = t.classification_id
              WHERE t.fully_qualified_name = $1",
        )
        .bind(tag_fqn)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some((tag_id, classification_id, exclusive)) = tag else {
            return Ok(LabelOutcome::NoSuchTag);
        };

        // The target must be a live asset. A label on a name nothing resolves
        // to is a governance claim about nothing, and it would sit there
        // looking like coverage.
        let target: Option<bool> =
            sqlx::query_scalar("SELECT deleted FROM assets WHERE fully_qualified_name = $1")
                .bind(target_fqn)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if target != Some(false) {
            return Ok(LabelOutcome::NoSuchTarget);
        }

        // **A human already said no.** Only automated re-proposals are dropped:
        // a person deliberately applying a tag that was once rejected is
        // changing their mind, which is allowed and is not the loop this ledger
        // exists to break.
        if matches!(label_type, LabelType::Automated | LabelType::Derived) {
            let rejected: Option<Uuid> = sqlx::query_scalar(
                "SELECT tag_id FROM tag_rejections WHERE tag_id = $1 AND target_fqn = $2",
            )
            .bind(tag_id)
            .bind(target_fqn)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if rejected.is_some() {
                return Ok(LabelOutcome::PreviouslyRejected);
            }
        }

        let already: Option<Uuid> = sqlx::query_scalar(
            "SELECT tag_id FROM tag_labels WHERE tag_id = $1 AND target_fqn = $2",
        )
        .bind(tag_id)
        .bind(target_fqn)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if already.is_some() {
            return Ok(LabelOutcome::AlreadyApplied);
        }

        if exclusive {
            let conflict: Option<String> = sqlx::query_scalar(
                "SELECT t.fully_qualified_name
                   FROM tag_labels l JOIN tags t ON t.id = l.tag_id
                  WHERE l.target_fqn = $1 AND t.classification_id = $2
                  LIMIT 1",
            )
            .bind(target_fqn)
            .bind(classification_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            if let Some(existing_tag_fqn) = conflict {
                return Ok(LabelOutcome::Conflicts { existing_tag_fqn });
            }
        }

        sqlx::query(
            "INSERT INTO tag_labels (tag_id, target_fqn, label_type, state, applied_by)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(tag_id)
        .bind(target_fqn)
        .bind(label_type.as_str())
        .bind(state.as_str())
        .bind(applied_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Self::bump_asset_version(&mut tx, target_fqn, applied_by).await?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(LabelOutcome::Applied)
    }

    async fn remove_tag(&self, tag_fqn: &str, target_fqn: &str) -> Result<bool, StorageError> {
        let done = sqlx::query(
            "DELETE FROM tag_labels
              WHERE tag_id = (SELECT id FROM tags WHERE fully_qualified_name = $1)
                AND target_fqn = $2",
        )
        .bind(tag_fqn)
        .bind(target_fqn)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn labels_on(&self, target_fqn: &str) -> Result<Vec<TagLabel>, StorageError> {
        let rows = sqlx::query(
            "SELECT t.fully_qualified_name AS tag_fqn, l.target_fqn, l.label_type, l.state,
                    l.applied_by, l.applied_at, l.confirmed_by
               FROM tag_labels l JOIN tags t ON t.id = l.tag_id
              WHERE l.target_fqn = $1
              ORDER BY l.applied_at DESC, t.fully_qualified_name",
        )
        .bind(target_fqn)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(label_from_row).collect())
    }

    async fn decide_label(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        confirmed: bool,
        decided_by: &str,
    ) -> Result<LabelDecision, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let found: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT l.tag_id, l.state
               FROM tag_labels l JOIN tags t ON t.id = l.tag_id
              WHERE t.fully_qualified_name = $1 AND l.target_fqn = $2
                FOR UPDATE OF l",
        )
        .bind(tag_fqn)
        .bind(target_fqn)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some((tag_id, state)) = found else {
            return Ok(LabelDecision::NoSuchLabel);
        };
        if confirmed && state == LabelState::Confirmed.as_str() {
            return Ok(LabelDecision::AlreadyConfirmed);
        }

        if confirmed {
            sqlx::query(
                "UPDATE tag_labels SET state = 'confirmed', confirmed_by = $3, confirmed_at = now()
                  WHERE tag_id = $1 AND target_fqn = $2",
            )
            .bind(tag_id)
            .bind(target_fqn)
            .bind(decided_by)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        } else {
            sqlx::query("DELETE FROM tag_labels WHERE tag_id = $1 AND target_fqn = $2")
                .bind(tag_id)
                .bind(target_fqn)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            // **Recorded, not merely removed.** A rejection that vanished would
            // be re-proposed by the next run of the same scanner, and a steward
            // would answer the same question forever.
            sqlx::query(
                "INSERT INTO tag_rejections (tag_id, target_fqn, rejected_by)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (tag_id, target_fqn)
                 DO UPDATE SET rejected_by = EXCLUDED.rejected_by, rejected_at = now()",
            )
            .bind(tag_id)
            .bind(target_fqn)
            .bind(decided_by)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        Self::bump_asset_version(&mut tx, target_fqn, decided_by).await?;
        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(LabelDecision::Decided)
    }

    async fn suggested_labels(&self, limit: i64) -> Result<Vec<TagLabel>, StorageError> {
        let rows = sqlx::query(
            "SELECT t.fully_qualified_name AS tag_fqn, l.target_fqn, l.label_type, l.state,
                    l.applied_by, l.applied_at, l.confirmed_by
               FROM tag_labels l JOIN tags t ON t.id = l.tag_id
              WHERE l.state = 'suggested'
              ORDER BY l.applied_at
              LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(label_from_row).collect())
    }

    async fn tag_usage(&self, tag_fqn: &str) -> Result<TagUsage, StorageError> {
        // **Live entities only.** A tombstoned column does not keep a
        // governance label alive, and counting it would refuse a delete over
        // data nobody can see.
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT a.kind::text, COUNT(*)
               FROM tag_labels l
               JOIN tags t ON t.id = l.tag_id
               JOIN assets a ON a.fully_qualified_name = l.target_fqn
              WHERE t.fully_qualified_name = $1 AND NOT a.deleted
              GROUP BY a.kind
              ORDER BY a.kind::text",
        )
        .bind(tag_fqn)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(TagUsage { by_kind: rows })
    }

    async fn delete_tag(
        &self,
        tag_fqn: &str,
        force: bool,
        updated_by: &str,
    ) -> Result<Option<i64>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let tag_id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM tags WHERE fully_qualified_name = $1")
                .bind(tag_fqn)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some(tag_id) = tag_id else {
            return Ok(None);
        };

        let mut removed = 0_i64;
        if force {
            // Each affected entity's version advances, so a label that
            // disappeared from a thousand columns is visible in each of their
            // histories rather than only in this one operation's response.
            let targets: Vec<String> =
                sqlx::query_scalar("SELECT target_fqn FROM tag_labels WHERE tag_id = $1")
                    .bind(tag_id)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            for target in &targets {
                Self::bump_asset_version(&mut tx, target, updated_by).await?;
            }
            removed = i64::try_from(targets.len()).unwrap_or(i64::MAX);
            sqlx::query("DELETE FROM tag_labels WHERE tag_id = $1")
                .bind(tag_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        sqlx::query("DELETE FROM tags WHERE id = $1")
            .bind(tag_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Some(removed))
    }

    async fn propagate_tag(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        recursive: bool,
        applied_by: &str,
    ) -> Result<i64, StorageError> {
        let children: Vec<String> = if recursive {
            sqlx::query_scalar(
                "SELECT fully_qualified_name FROM assets
                  WHERE NOT deleted
                    AND fully_qualified_name LIKE $1 || '.%'",
            )
            .bind(target_fqn)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?
        } else {
            sqlx::query_scalar(
                "SELECT child.fully_qualified_name FROM assets child
                   JOIN assets parent ON parent.id = child.parent_id
                  WHERE NOT child.deleted AND parent.fully_qualified_name = $1",
            )
            .bind(target_fqn)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?
        };

        let mut affected = 0_i64;
        for child in children {
            // Reuses `apply_tag`, so precedence, exclusivity and the rejection
            // ledger are all decided in one place. An `Applied` outcome is a
            // child that gained the label; everything else — already present,
            // conflicting, rejected — is deliberately not counted, because the
            // number reports what this call *did*.
            let existing = self.labels_on(&child).await?;
            let held = existing.iter().find(|l| l.tag_fqn == tag_fqn);
            if !graph_owl_core::classification::propagation_may_overwrite(held) {
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
                LabelOutcome::Applied
            ) {
                affected += 1;
            }
        }
        Ok(affected)
    }

    // ---- Epic 26: lifecycle and certification ----

    async fn set_lifecycle(
        &self,
        asset_id: Uuid,
        to: LifecycleState,
        deprecation: Option<&Deprecation>,
        updated_by: &str,
    ) -> Result<LifecycleOutcome, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let current: Option<String> =
            sqlx::query_scalar("SELECT lifecycle FROM assets WHERE id = $1 FOR UPDATE")
                .bind(asset_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some(current) = current else {
            return Ok(LifecycleOutcome::NotFound);
        };
        let from = LifecycleState::parse(&current).unwrap_or_default();
        if !graph_owl_core::lifecycle::can_transition(from, to) {
            return Ok(LifecycleOutcome::Illegal { from, to });
        }

        // The deprecation document and the state are written together, so they
        // cannot disagree — a `Deprecated` asset with no reason, or an `Active`
        // one still carrying a successor, are both states nothing should be
        // able to store.
        let row = sqlx::query(&format!(
            "UPDATE assets
                SET lifecycle = $2, deprecation = $3,
                    version_minor = version_minor + 1, updated_by = $4, updated_at = now()
              WHERE id = $1
              RETURNING {ASSET_COLUMNS}"
        ))
        .bind(asset_id)
        .bind(to.as_str())
        .bind(deprecation.and_then(|d| serde_json::to_value(d).ok()))
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut asset = asset_from_row(row);
        asset.owners = self.asset_owners(asset.id).await?;
        Ok(LifecycleOutcome::Moved(Box::new(asset)))
    }

    async fn terminal_successor(&self, fqn: &str) -> Result<Option<Asset>, StorageError> {
        // **Bounded and cycle-safe.** A successor loop is a configuration
        // mistake somebody will make, and an unbounded walk turns it into a
        // hung request rather than an answer. Ten hops is far past any real
        // chain — an estate that has deprecated the same thing ten times in
        // sequence has a bigger problem than this walk.
        const MAX_HOPS: usize = 10;
        let mut seen = std::collections::HashSet::new();
        let mut current = fqn.to_string();

        for _ in 0..MAX_HOPS {
            if !seen.insert(current.clone()) {
                return Ok(None);
            }
            let row = sqlx::query(&format!(
                "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
                  WHERE fully_qualified_name = $1"
            ))
            .bind(&current)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            let Some(row) = row else { return Ok(None) };
            let asset = asset_from_row(row);

            match asset.lifecycle {
                LifecycleState::Deprecated | LifecycleState::Retired => {
                    let Some(next) = asset
                        .deprecation
                        .as_ref()
                        .and_then(|d| d.successor_fqn.clone())
                    else {
                        // Dead end: deprecated with nowhere to go. `None`, not
                        // this asset — pointing a caller at a dead asset is the
                        // failure the successor field exists to prevent.
                        return Ok(None);
                    };
                    current = next;
                }
                _ => return Ok(Some(asset)),
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
        updated_by: &str,
    ) -> Result<StoredCertificationType, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query(
            "INSERT INTO certification_types
                 (id, name, description, default_validity_days, required_evidence, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(default_validity_days)
        .bind(required_evidence)
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            conflict_or_unexpected(
                &e,
                format!("a certification type named `{name}` already exists"),
            )
        })?;

        for issuer in authorized_issuers {
            sqlx::query(
                "INSERT INTO certification_type_issuers (type_id, principal_id)
                 VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(issuer)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(StoredCertificationType {
            id,
            name: name.to_string(),
            description: description.map(str::to_string),
            default_validity_days,
            required_evidence: required_evidence.to_vec(),
            authorized_issuers: authorized_issuers.to_vec(),
        })
    }

    async fn list_certification_types(&self) -> Result<Vec<StoredCertificationType>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, name, description, default_validity_days, required_evidence
               FROM certification_types ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut types = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            types.push(StoredCertificationType {
                id,
                name: row.get("name"),
                description: row.get("description"),
                default_validity_days: row.get("default_validity_days"),
                required_evidence: row.get("required_evidence"),
                authorized_issuers: self.read_issuers(id).await?,
            });
        }
        Ok(types)
    }

    async fn get_certification_type(
        &self,
        id: Uuid,
    ) -> Result<Option<StoredCertificationType>, StorageError> {
        let row = sqlx::query(
            "SELECT id, name, description, default_validity_days, required_evidence
               FROM certification_types WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(StoredCertificationType {
            id,
            name: row.get("name"),
            description: row.get("description"),
            default_validity_days: row.get("default_validity_days"),
            required_evidence: row.get("required_evidence"),
            authorized_issuers: self.read_issuers(id).await?,
        }))
    }

    async fn issue_certification(
        &self,
        id: Uuid,
        target_fqn: &str,
        type_id: Uuid,
        issuer: &str,
        criteria: Option<&str>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        evidence: &[(String, String)],
    ) -> Result<IssueOutcome, StorageError> {
        let Some(certification_type) = self.get_certification_type(type_id).await? else {
            return Ok(IssueOutcome::NoSuchType);
        };

        if !graph_owl_core::lifecycle::may_issue(&certification_type.authorized_issuers, issuer) {
            return Ok(IssueOutcome::NotAuthorized);
        }

        // **Re-checked at issuance, every time.** Slice E's renewal path goes
        // through here too, so a renewal whose evidence has since disappeared
        // fails — renewing on stale grounds is how certification decays into
        // theatre.
        let supplied: Vec<String> = evidence.iter().map(|(kind, _)| kind.clone()).collect();
        let missing = graph_owl_core::lifecycle::missing_evidence(
            &certification_type.required_evidence,
            &supplied,
        );
        if !missing.is_empty() {
            return Ok(IssueOutcome::MissingEvidence(missing));
        }

        let target: Option<bool> =
            sqlx::query_scalar("SELECT deleted FROM assets WHERE fully_qualified_name = $1")
                .bind(target_fqn)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if target != Some(false) {
            return Ok(IssueOutcome::NoSuchTarget);
        }

        let expires_at = expires_at.unwrap_or_else(|| {
            chrono::Utc::now()
                + chrono::Duration::days(i64::from(certification_type.default_validity_days))
        });

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // **Insert first, then supersede.** `superseded_by` is a
        // self-referential foreign key, so naming the new row before it exists
        // is refused by the database — and the two statements are in one
        // transaction, so no reader ever sees the moment when both are live.
        sqlx::query(
            "INSERT INTO certifications (id, target_fqn, type_id, issuer, criteria, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(target_fqn)
        .bind(type_id)
        .bind(issuer)
        .bind(criteria)
        .bind(expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // A second issuance of the same type supersedes rather than
        // accumulating, so "when does my Gold expire" has one answer. The
        // superseded row stays: who vouched for what and when is the point.
        sqlx::query(
            "UPDATE certifications SET superseded_by = $3
              WHERE target_fqn = $1 AND type_id = $2 AND superseded_by IS NULL
                AND id <> $3",
        )
        .bind(target_fqn)
        .bind(type_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        for (kind, reference) in evidence {
            sqlx::query(
                "INSERT INTO certification_evidence (certification_id, kind, reference)
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(kind)
            .bind(reference)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(IssueOutcome::Issued(Box::new(StoredCertification {
            id,
            target_fqn: target_fqn.to_string(),
            type_id,
            type_name: certification_type.name,
            issuer: issuer.to_string(),
            criteria: criteria.map(str::to_string),
            issued_at: chrono::Utc::now(),
            expires_at,
            evidence: evidence.to_vec(),
        })))
    }

    async fn certifications_on(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<StoredCertification>, StorageError> {
        let rows = sqlx::query(
            "SELECT c.id, c.target_fqn, c.type_id, t.name AS type_name, c.issuer, c.criteria,
                    c.issued_at, c.expires_at
               FROM certifications c JOIN certification_types t ON t.id = c.type_id
              WHERE c.target_fqn = $1 AND c.superseded_by IS NULL
              ORDER BY c.expires_at",
        )
        .bind(target_fqn)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        self.hydrate_certifications(&rows).await
    }

    async fn certifications_expiring_before(
        &self,
        instant: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<StoredCertification>, StorageError> {
        let rows = sqlx::query(
            "SELECT c.id, c.target_fqn, c.type_id, t.name AS type_name, c.issuer, c.criteria,
                    c.issued_at, c.expires_at
               FROM certifications c JOIN certification_types t ON t.id = c.type_id
              WHERE c.superseded_by IS NULL AND c.expires_at < $1
              ORDER BY c.expires_at",
        )
        .bind(instant)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        self.hydrate_certifications(&rows).await
    }

    // ---- Epic 23: domains and data products ----

    async fn create_domain(
        &self,
        id: Uuid,
        name: &str,
        parent_id: Option<Uuid>,
        description: Option<&str>,
        domain_type: Option<&str>,
        experts: &[String],
        updated_by: &str,
    ) -> Result<Option<Domain>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // The parent's path, read under the transaction, so a concurrent rename
        // cannot land between deriving the FQN and writing it.
        let parent_fqn: Option<String> = match parent_id {
            None => None,
            Some(parent) => {
                let found: Option<String> =
                    sqlx::query_scalar("SELECT fully_qualified_name FROM domains WHERE id = $1")
                        .bind(parent)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
                let Some(found) = found else {
                    return Ok(None);
                };
                Some(found)
            }
        };
        let fqn = domain_fqn(parent_fqn.as_deref(), name);

        let row = sqlx::query(&format!(
            "INSERT INTO domains (id, name, fully_qualified_name, parent_id, description,
                                  domain_type, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING {DOMAIN_COLUMNS}"
        ))
        .bind(id)
        .bind(name)
        .bind(&fqn)
        .bind(parent_id)
        .bind(description)
        .bind(domain_type)
        .bind(updated_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!("a domain already exists at `{fqn}`"),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        Self::write_experts(&mut tx, id, experts).await?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut domain = domain_from_row(&row);
        domain.experts = experts.to_vec();
        Ok(Some(domain))
    }

    async fn get_domain(&self, id: Uuid) -> Result<Option<Domain>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {DOMAIN_COLUMNS} FROM domains WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        match row {
            None => Ok(None),
            Some(row) => {
                let mut domain = domain_from_row(&row);
                domain.experts = self.read_experts(id).await?;
                Ok(Some(domain))
            }
        }
    }

    async fn get_domain_by_fqn(&self, fqn: &str) -> Result<Option<Domain>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {DOMAIN_COLUMNS} FROM domains WHERE fully_qualified_name = $1"
        ))
        .bind(fqn)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        match row {
            None => Ok(None),
            Some(row) => {
                let id: Uuid = row.get("id");
                let mut domain = domain_from_row(&row);
                domain.experts = self.read_experts(id).await?;
                Ok(Some(domain))
            }
        }
    }

    async fn list_domains(&self, page: &PageRequest) -> Result<Page<Domain>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let rows = sqlx::query(&format!(
            "SELECT {DOMAIN_COLUMNS} FROM domains
              WHERE NOT deleted
                AND ($1::text IS NULL OR (fully_qualified_name, id) > ($1, $2))
              ORDER BY fully_qualified_name, id
              LIMIT $3"
        ))
        .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
        .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
        .bind(overfetch)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut domains = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let mut domain = domain_from_row(row);
            domain.experts = self.read_experts(id).await?;
            domains.push(domain);
        }
        Ok(Page::from_overfetch(domains, page.limit, |d: &Domain| {
            Cursor::new(d.fully_qualified_name.clone(), d.id)
        }))
    }

    async fn update_domain(
        &self,
        id: Uuid,
        update: &DomainUpdate,
        updated_by: &str,
    ) -> Result<Option<Domain>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(before_row) = sqlx::query(&format!(
            "SELECT {DOMAIN_COLUMNS} FROM domains WHERE id = $1 FOR UPDATE"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        else {
            return Ok(None);
        };
        let mut before = domain_from_row(&before_row);
        before.experts = Self::read_experts_tx(&mut tx, id).await?;

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

        // Re-derived whenever the name or the parent moved, because the FQN is
        // a function of both. Deriving it from only one is how a reparented
        // domain keeps its old path.
        let parent_fqn: Option<String> = match after.parent_id {
            None => None,
            Some(parent) => {
                sqlx::query_scalar("SELECT fully_qualified_name FROM domains WHERE id = $1")
                    .bind(parent)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?
            }
        };
        after.fully_qualified_name = domain_fqn(parent_fqn.as_deref(), &after.name);

        let diff = ChangeDescription::between(
            &serde_json::to_value(&before).unwrap_or_default(),
            &serde_json::to_value(&after).unwrap_or_default(),
        );
        let kind = classify(&diff);
        if matches!(kind, graph_owl_core::envelope::ChangeKind::None) {
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(Some(before));
        }
        let next = before.version.bump(kind);

        let updated_row = sqlx::query(&format!(
            "UPDATE domains SET name = $2, fully_qualified_name = $3, parent_id = $4,
                 description = $5, domain_type = $6, version_major = $7, version_minor = $8,
                 updated_by = $9, change_description = $10, updated_at = now()
             WHERE id = $1
             RETURNING {DOMAIN_COLUMNS}"
        ))
        .bind(id)
        .bind(&after.name)
        .bind(&after.fully_qualified_name)
        .bind(after.parent_id)
        .bind(&after.description)
        .bind(&after.domain_type)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(updated_by)
        .bind(serde_json::to_value(&diff).ok())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!(
                        "a domain already exists at `{}`",
                        after.fully_qualified_name
                    ),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        if update.experts.is_some() {
            sqlx::query("DELETE FROM domain_experts WHERE domain_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            Self::write_experts(&mut tx, id, &after.experts).await?;
        }

        // **The subtree's paths move with it**, in the same transaction. A
        // rename that moved only its own path would leave every descendant
        // claiming to sit under a name that no longer exists, and every
        // FQN lookup below it would miss — with no error anywhere.
        if before.fully_qualified_name != after.fully_qualified_name {
            sqlx::query(
                "WITH RECURSIVE subtree (id) AS (
                         SELECT id FROM domains WHERE parent_id = $1
                     UNION ALL
                         SELECT d.id FROM domains d JOIN subtree ON d.parent_id = subtree.id
                 )
                 UPDATE domains
                    SET fully_qualified_name = $3 || substring(fully_qualified_name from length($2) + 1)
                  WHERE id IN (SELECT id FROM subtree)",
            )
            .bind(id)
            .bind(&before.fully_qualified_name)
            .bind(&after.fully_qualified_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        sqlx::query(
            "INSERT INTO domain_versions
                 (domain_id, version_major, version_minor, snapshot, change_description, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(serde_json::to_value(&after).unwrap_or_default())
        .bind(serde_json::to_value(&diff).ok())
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut result = domain_from_row(&updated_row);
        result.experts = after.experts;
        Ok(Some(result))
    }

    async fn domain_would_cycle(&self, domain: Uuid, parent: Uuid) -> Result<bool, StorageError> {
        // **Walk the proposed parent's whole ancestry.** A depth-1 check passes
        // `A → B → C → A` and leaves an ancestor walk that never terminates.
        // The depth-0 case is also a database constraint, but it is checked
        // here so the caller gets a sentence rather than a constraint violation.
        if domain == parent {
            return Ok(true);
        }
        let closes: bool = sqlx::query_scalar(
            "WITH RECURSIVE ancestry (node) AS (
                     SELECT $2::uuid
                 UNION
                     SELECT d.parent_id FROM domains d
                       JOIN ancestry ON d.id = ancestry.node
                      WHERE d.parent_id IS NOT NULL
             )
             SELECT EXISTS (SELECT 1 FROM ancestry WHERE node = $1)",
        )
        .bind(domain)
        .bind(parent)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(closes)
    }

    async fn child_domains(&self, parent: Option<Uuid>) -> Result<Vec<Domain>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {DOMAIN_COLUMNS} FROM domains
              WHERE NOT deleted
                AND ($1::uuid IS NULL AND parent_id IS NULL OR parent_id = $1)
              ORDER BY name, id"
        ))
        .bind(parent)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut children = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: Uuid = row.get("id");
            let mut domain = domain_from_row(row);
            domain.experts = self.read_experts(id).await?;
            children.push(domain);
        }
        Ok(children)
    }

    async fn assign_asset_domain(
        &self,
        asset_id: Uuid,
        domain_id: Option<Uuid>,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError> {
        // **A version bump, because an assignment is a change to the asset.**
        // The accountability for a table moving is exactly the kind of edit a
        // history exists to record, and one that left no version would be
        // invisible to every consumer watching for changes.
        let row = sqlx::query(&format!(
            "UPDATE assets
                SET domain_id = $2, version_minor = version_minor + 1,
                    updated_by = $3, updated_at = now()
              WHERE id = $1
              RETURNING {ASSET_COLUMNS}"
        ))
        .bind(asset_id)
        .bind(domain_id)
        .bind(updated_by)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(row) = row else { return Ok(None) };
        let mut asset = asset_from_row(row);
        asset.owners = self.asset_owners(asset.id).await?;
        Ok(Some(asset))
    }

    async fn resolve_asset_domain(
        &self,
        asset_id: Uuid,
    ) -> Result<Option<DomainAssignment>, StorageError> {
        let resolved: Option<serde_json::Value> = sqlx::query_scalar(&format!(
            "SELECT {DOMAIN_EXPR} FROM assets WHERE assets.id = $1"
        ))
        .bind(asset_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .flatten();

        Ok(resolved.and_then(|value| serde_json::from_value(value).ok()))
    }

    async fn count_assets_in_domain(&self, domain: Uuid) -> Result<i64, StorageError> {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM assets
              WHERE NOT deleted AND {DOMAIN_ID_EXPR} = $1"
        ))
        .bind(domain)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(count)
    }

    async fn delete_domain(
        &self,
        id: Uuid,
        reassign_to: Option<Uuid>,
        updated_by: &str,
    ) -> Result<DomainDeletion, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM domains WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if exists.is_none() {
            return Ok(DomainDeletion::NotFound);
        }

        // **Children first, and never reassigned implicitly.** Where the
        // *assets* go says nothing about where the sub-domains should go, and
        // reparenting them to the target would restructure the accountability
        // tree as a side effect of a delete.
        let children: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domains WHERE parent_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if children > 0 {
            return Ok(DomainDeletion::HasChildren { children });
        }

        // Direct assignments only. An asset that merely *inherits* this domain
        // is held by an ancestor of its own, and deleting this domain does not
        // orphan it — counting it would refuse a delete over data nobody holds.
        let assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets WHERE domain_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let products: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM data_products WHERE domain_id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(target) = reassign_to else {
            if assets > 0 || products > 0 {
                return Ok(DomainDeletion::StillHolds(Box::new(DomainHoldings {
                    assets,
                    data_products: products,
                })));
            }
            sqlx::query("DELETE FROM domains WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            tx.commit()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(DomainDeletion::Deleted {
                reassigned_assets: 0,
                reassigned_products: 0,
            });
        };

        let target_exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM domains WHERE id = $1 AND NOT deleted")
                .bind(target)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if target_exists.is_none() {
            return Ok(DomainDeletion::UnknownTarget);
        }

        // Transactional, so a failure halfway leaves neither a half-moved
        // estate nor a domain that was deleted while things still pointed at it.
        let moved_assets = sqlx::query(
            "UPDATE assets
                SET domain_id = $2, version_minor = version_minor + 1,
                    updated_by = $3, updated_at = now()
              WHERE domain_id = $1",
        )
        .bind(id)
        .bind(target)
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();

        let moved_products = sqlx::query(
            "UPDATE data_products
                SET domain_id = $2, version_minor = version_minor + 1,
                    updated_by = $3, updated_at = now()
              WHERE domain_id = $1",
        )
        .bind(id)
        .bind(target)
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        .rows_affected();

        sqlx::query("DELETE FROM domains WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(DomainDeletion::Deleted {
            reassigned_assets: i64::try_from(moved_assets).unwrap_or(i64::MAX),
            reassigned_products: i64::try_from(moved_products).unwrap_or(i64::MAX),
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
    ) -> Result<DataProduct, StorageError> {
        let row = sqlx::query(&format!(
            "INSERT INTO data_products
                 (id, name, fully_qualified_name, description, purpose, domain_id, updated_by)
             VALUES ($1, $2, $2, $3, $4, $5, $6)
             RETURNING {PRODUCT_COLUMNS}"
        ))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(purpose)
        .bind(domain_id)
        .bind(updated_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!("a data product named `{name}` already exists"),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;
        Ok(product_from_row(&row))
    }

    async fn get_data_product(&self, id: Uuid) -> Result<Option<DataProduct>, StorageError> {
        let row = sqlx::query(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM data_products WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.as_ref().map(product_from_row))
    }

    async fn list_data_products(
        &self,
        page: &PageRequest,
    ) -> Result<Page<DataProduct>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let rows = sqlx::query(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM data_products
              WHERE NOT deleted
                AND ($1::text IS NULL OR (fully_qualified_name, id) > ($1, $2))
              ORDER BY fully_qualified_name, id
              LIMIT $3"
        ))
        .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
        .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
        .bind(overfetch)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Page::from_overfetch(
            rows.iter().map(product_from_row).collect(),
            page.limit,
            |p: &DataProduct| Cursor::new(p.fully_qualified_name.clone(), p.id),
        ))
    }

    async fn update_data_product(
        &self,
        id: Uuid,
        update: &DataProductUpdate,
        updated_by: &str,
    ) -> Result<Option<DataProduct>, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let Some(before_row) = sqlx::query(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM data_products WHERE id = $1 FOR UPDATE"
        ))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?
        else {
            return Ok(None);
        };
        let before = product_from_row(&before_row);

        let mut after = before.clone();
        if let Some(name) = &update.name {
            after.name = name.clone();
            after.fully_qualified_name = name.clone();
        }
        if let Some(description) = &update.description {
            after.description = description.clone();
        }
        if let Some(purpose) = &update.purpose {
            after.purpose = purpose.clone();
        }
        if let Some(domain_id) = &update.domain_id {
            after.domain_id = *domain_id;
        }

        let diff = ChangeDescription::between(
            &serde_json::to_value(&before).unwrap_or_default(),
            &serde_json::to_value(&after).unwrap_or_default(),
        );
        let kind = classify(&diff);
        if matches!(kind, graph_owl_core::envelope::ChangeKind::None) {
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(Some(before));
        }
        let next = before.version.bump(kind);

        let updated_row = sqlx::query(&format!(
            "UPDATE data_products SET name = $2, fully_qualified_name = $3, description = $4,
                 purpose = $5, domain_id = $6, version_major = $7, version_minor = $8,
                 updated_by = $9, change_description = $10, updated_at = now()
             WHERE id = $1
             RETURNING {PRODUCT_COLUMNS}"
        ))
        .bind(id)
        .bind(&after.name)
        .bind(&after.fully_qualified_name)
        .bind(&after.description)
        .bind(&after.purpose)
        .bind(after.domain_id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(updated_by)
        .bind(serde_json::to_value(&diff).ok())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!("a data product named `{}` already exists", after.name),
                    existing_id: None,
                    kind: ConflictKind::Fqn,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        sqlx::query(
            "INSERT INTO data_product_versions
                 (data_product_id, version_major, version_minor, snapshot, change_description, updated_by, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
        .bind(serde_json::to_value(&after).unwrap_or_default())
        .bind(serde_json::to_value(&diff).ok())
        .bind(updated_by)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Some(product_from_row(&updated_row)))
    }

    async fn delete_data_product(&self, id: Uuid) -> Result<bool, StorageError> {
        // The membership edges go with it by `ON DELETE CASCADE`, and the
        // assets do not: a product is a *view* of things that exist
        // independently of it.
        let done = sqlx::query("DELETE FROM data_products WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn add_product_asset(
        &self,
        product_id: Uuid,
        asset_id: Uuid,
    ) -> Result<Result<(), MembershipRefusal>, StorageError> {
        let product: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM data_products WHERE id = $1 AND NOT deleted")
                .bind(product_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        if product.is_none() {
            return Ok(Err(MembershipRefusal::NoSuchProduct));
        }

        // **Tombstoned is a different refusal from absent.** A caller who sent
        // a typo'd id and one who sent a deleted asset are making different
        // mistakes, and "no such asset" for the second sends them looking for
        // the wrong thing.
        let asset: Option<bool> = sqlx::query_scalar("SELECT deleted FROM assets WHERE id = $1")
            .bind(asset_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        match asset {
            None => return Ok(Err(MembershipRefusal::NoSuchAsset)),
            Some(true) => return Ok(Err(MembershipRefusal::AssetDeleted)),
            Some(false) => {}
        }

        // `ON CONFLICT DO NOTHING` against the primary key is what makes this
        // idempotent without a read-then-write race: two concurrent adds both
        // see "not a member" and one of them would otherwise fail on the key.
        sqlx::query(
            "INSERT INTO data_product_assets (data_product_id, asset_id)
             VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(product_id)
        .bind(asset_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(Ok(()))
    }

    async fn remove_product_asset(
        &self,
        product_id: Uuid,
        asset_id: Uuid,
    ) -> Result<bool, StorageError> {
        let done = sqlx::query(
            "DELETE FROM data_product_assets WHERE data_product_id = $1 AND asset_id = $2",
        )
        .bind(product_id)
        .bind(asset_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn product_assets(
        &self,
        product_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let sql = format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
               JOIN data_product_assets m ON m.asset_id = assets.id
              WHERE m.data_product_id = $1
                AND NOT assets.deleted
                AND ($2::text IS NULL OR (fully_qualified_name, assets.id) > ($2, $3))
              ORDER BY fully_qualified_name, assets.id
              LIMIT $4"
        );
        let query = sqlx::query(&sql)
            .bind(product_id)
            .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
            .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
            .bind(overfetch);
        self.asset_page(query, page).await
    }

    async fn asset_products(&self, asset_id: Uuid) -> Result<Vec<DataProduct>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {PRODUCT_COLUMNS} FROM data_products p
               JOIN data_product_assets m ON m.data_product_id = p.id
              WHERE m.asset_id = $1 AND NOT p.deleted
              ORDER BY p.fully_qualified_name, p.id"
        ))
        .bind(asset_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.iter().map(product_from_row).collect())
    }

    async fn custom_property_values(
        &self,
        entity_type: &str,
        name: &str,
    ) -> Result<Vec<(Uuid, serde_json::Value)>, StorageError> {
        // The same `?` plus not-null test `count_custom_property_values` uses,
        // so the count and the list can never disagree about what "holds a
        // value" means — a `409` reporting three and a migration touching two
        // is a bug nobody would look for in two different `WHERE` clauses.
        let rows = sqlx::query(
            "SELECT id, extension -> $2 AS value FROM assets
             WHERE kind::text = $1 AND extension ? $2 AND extension -> $2 <> 'null'::jsonb
             ORDER BY id",
        )
        .bind(entity_type)
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows
            .iter()
            .map(|row| (row.get("id"), row.get::<serde_json::Value, _>("value")))
            .collect())
    }

    async fn update_custom_property(
        &self,
        id: Uuid,
        property: &CustomProperty,
        previous_name: &str,
    ) -> Result<bool, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let done = sqlx::query(
            "UPDATE custom_properties
                SET name = $2, property_type = $3, description = $4, constraints = $5
              WHERE id = $1",
        )
        .bind(id)
        .bind(&property.name)
        .bind(property.property_type.as_str())
        .bind(&property.description)
        .bind(serde_json::to_value(&property.constraints).unwrap_or_default())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!(
                        "`{}` is already defined on `{}`",
                        property.name, property.entity_type
                    ),
                    existing_id: None,
                    kind: ConflictKind::CustomPropertyExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;

        if done.rows_affected() == 0 {
            tx.rollback()
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            return Ok(false);
        }

        // The key migration, in the same transaction as the definition change.
        // `- old || jsonb_build_object(new, value)` in one expression, so no
        // row is ever observed holding neither key.
        if previous_name != property.name {
            sqlx::query(
                "UPDATE assets
                    SET extension = (extension - $2)
                                    || jsonb_build_object($3::text, extension -> $2)
                  WHERE kind::text = $1 AND extension ? $2",
            )
            .bind(&property.entity_type)
            .bind(previous_name)
            .bind(&property.name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(true)
    }

    #[tracing::instrument(name = "storage.insert_pack", skip_all)]
    async fn insert_pack(
        &self,
        pack: graph_owl_ontology::pack::OntologyPack,
        source_turtle: &[u8],
    ) -> Result<graph_owl_ontology::pack::OntologyPack, StorageError> {
        let (licence_kind, licence_name, licence_notice, licence_contact) =
            licence_columns(&pack.licence);
        let term_count = i32::try_from(pack.term_count).unwrap_or(i32::MAX);
        let row = sqlx::query(
            "INSERT INTO ontology_packs
                (id, pack_id, version, licence_kind, licence_name, licence_notice,
                 licence_contact, source_url, glossary_id, term_count, imported_at,
                 source_turtle)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             RETURNING *",
        )
        .bind(pack.id)
        .bind(&pack.pack_id)
        .bind(&pack.version)
        .bind(licence_kind)
        .bind(licence_name)
        .bind(licence_notice)
        .bind(licence_contact)
        .bind(&pack.source_url)
        .bind(pack.glossary_id)
        .bind(term_count)
        .bind(pack.imported_at)
        .bind(source_turtle)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if e.as_database_error()
                .is_some_and(|d| d.is_unique_violation())
            {
                StorageError::Conflict {
                    detail: format!(
                        "`{}` version `{}` is already imported",
                        pack.pack_id, pack.version
                    ),
                    existing_id: None,
                    kind: ConflictKind::PackVersionExists,
                }
            } else {
                StorageError::Unexpected(e.to_string())
            }
        })?;
        pack_from_row(row)
    }

    async fn get_pack_source_turtle(&self, pack_id: Uuid) -> Result<Option<Vec<u8>>, StorageError> {
        sqlx::query_scalar("SELECT source_turtle FROM ontology_packs WHERE id = $1")
            .bind(pack_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn update_pack_version(
        &self,
        id: Uuid,
        version: &str,
        term_count: usize,
        source_turtle: &[u8],
        imported_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StorageError> {
        let term_count = i32::try_from(term_count).unwrap_or(i32::MAX);
        sqlx::query(
            "UPDATE ontology_packs
             SET version = $2, term_count = $3, source_turtle = $4, imported_at = $5
             WHERE id = $1",
        )
        .bind(id)
        .bind(version)
        .bind(term_count)
        .bind(source_turtle)
        .bind(imported_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn get_pack(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_ontology::pack::OntologyPack>, StorageError> {
        let row = sqlx::query("SELECT * FROM ontology_packs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(pack_from_row).transpose()
    }

    async fn get_pack_by_id_and_version(
        &self,
        pack_id: &str,
        version: &str,
    ) -> Result<Option<graph_owl_ontology::pack::OntologyPack>, StorageError> {
        let row = sqlx::query("SELECT * FROM ontology_packs WHERE pack_id = $1 AND version = $2")
            .bind(pack_id)
            .bind(version)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(pack_from_row).transpose()
    }

    async fn list_packs(
        &self,
    ) -> Result<Vec<graph_owl_ontology::pack::OntologyPack>, StorageError> {
        let rows = sqlx::query("SELECT * FROM ontology_packs ORDER BY pack_id, version")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter().map(pack_from_row).collect()
    }

    async fn delete_pack(&self, id: Uuid) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM ontology_packs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn insert_pack_term(
        &self,
        pack_id: Uuid,
        term_id: Uuid,
        source_iri: &str,
    ) -> Result<(), StorageError> {
        sqlx::query("INSERT INTO pack_terms (pack_id, term_id, source_iri) VALUES ($1, $2, $3)")
            .bind(pack_id)
            .bind(term_id)
            .bind(source_iri)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn pack_terms(&self, pack_id: Uuid) -> Result<Vec<(String, Uuid)>, StorageError> {
        let rows = sqlx::query("SELECT source_iri, term_id FROM pack_terms WHERE pack_id = $1")
            .bind(pack_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get("source_iri"), row.get("term_id")))
            .collect())
    }

    async fn pack_term_by_iri(
        &self,
        pack_id: Uuid,
        source_iri: &str,
    ) -> Result<Option<Uuid>, StorageError> {
        sqlx::query_scalar("SELECT term_id FROM pack_terms WHERE pack_id = $1 AND source_iri = $2")
            .bind(pack_id)
            .bind(source_iri)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    async fn pack_attachment_counts(
        &self,
        pack_id: Uuid,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        let rows = sqlx::query(
            "SELECT pt.source_iri AS source_iri, COUNT(ta.target_fqn) AS attachment_count
             FROM pack_terms pt
             JOIN term_attachments ta ON ta.term_id = pt.term_id
             WHERE pt.pack_id = $1
             GROUP BY pt.source_iri",
        )
        .bind(pack_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get("source_iri"), row.get("attachment_count")))
            .collect())
    }

    async fn exact_match_targets_outside_pack(
        &self,
        pack_id: Uuid,
    ) -> Result<Vec<String>, StorageError> {
        let rows = sqlx::query(
            "SELECT tr.target AS target
             FROM term_relations tr
             JOIN pack_terms pt ON pt.term_id = tr.term_id
             WHERE tr.kind = 'exactMatch' AND pt.pack_id <> $1",
        )
        .bind(pack_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(|row| row.get("target")).collect())
    }

    async fn insert_pack_override(
        &self,
        override_: graph_owl_ontology::pack::PackOverride,
    ) -> Result<graph_owl_ontology::pack::PackOverride, StorageError> {
        let row = sqlx::query(
            "INSERT INTO pack_overrides (id, pack_id, term_path, kind, payload)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING *",
        )
        .bind(override_.id)
        .bind(override_.pack_id)
        .bind(&override_.term_path)
        .bind(override_kind_str(override_.kind))
        .bind(&override_.payload)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        pack_override_from_row(row)
    }

    async fn list_pack_overrides(
        &self,
        pack_id: Uuid,
    ) -> Result<Vec<graph_owl_ontology::pack::PackOverride>, StorageError> {
        let rows =
            sqlx::query("SELECT * FROM pack_overrides WHERE pack_id = $1 ORDER BY created_at")
                .bind(pack_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter().map(pack_override_from_row).collect()
    }

    async fn overrides_for_term_path(
        &self,
        pack_id: Uuid,
        term_path: &str,
    ) -> Result<Vec<graph_owl_ontology::pack::PackOverride>, StorageError> {
        let rows = sqlx::query(
            "SELECT * FROM pack_overrides
             WHERE pack_id = $1 AND term_path = $2
             ORDER BY created_at",
        )
        .bind(pack_id)
        .bind(term_path)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter().map(pack_override_from_row).collect()
    }

    async fn delete_pack_override(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM pack_overrides WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    // ---- Epic 35 Slice A: threads and replies ----

    async fn insert_thread(
        &self,
        thread: graph_owl_core::collaboration::Thread,
    ) -> Result<graph_owl_core::collaboration::Thread, StorageError> {
        let row = sqlx::query(
            "INSERT INTO threads (id, about, field, created_by, created_at, resolved, resolved_by, resolved_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING *",
        )
        .bind(thread.id)
        .bind(thread.about)
        .bind(&thread.field)
        .bind(&thread.created_by)
        .bind(thread.created_at)
        .bind(thread.resolved)
        .bind(&thread.resolved_by)
        .bind(thread.resolved_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(thread_from_row(row))
    }

    async fn get_thread(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Thread>, StorageError> {
        let row = sqlx::query("SELECT * FROM threads WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(thread_from_row))
    }

    async fn list_threads(
        &self,
        about: Uuid,
        resolved: Option<bool>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Thread>, i64), StorageError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(0);
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM threads WHERE about = $1 AND ($2::boolean IS NULL OR resolved = $2)",
        )
        .bind(about)
        .bind(resolved)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rows = sqlx::query(
            "SELECT * FROM threads
             WHERE about = $1 AND ($2::boolean IS NULL OR resolved = $2)
             ORDER BY created_at DESC, id
             LIMIT $3 OFFSET $4",
        )
        .bind(about)
        .bind(resolved)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok((rows.into_iter().map(thread_from_row).collect(), total))
    }

    async fn insert_post(
        &self,
        post: graph_owl_core::collaboration::Post,
    ) -> Result<graph_owl_core::collaboration::Post, StorageError> {
        let row = sqlx::query(
            "INSERT INTO posts (id, thread_id, author, message, created_at, edited_at, deleted)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING *",
        )
        .bind(post.id)
        .bind(post.thread_id)
        .bind(&post.author)
        .bind(&post.message)
        .bind(post.created_at)
        .bind(post.edited_at)
        .bind(post.deleted)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(post_from_row(row))
    }

    async fn get_post(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Post>, StorageError> {
        let row = sqlx::query("SELECT * FROM posts WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(post_from_row))
    }

    async fn list_posts(
        &self,
        thread_id: Uuid,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Post>, i64), StorageError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(0);
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM posts WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rows = sqlx::query(
            "SELECT * FROM posts WHERE thread_id = $1 ORDER BY created_at, id LIMIT $2 OFFSET $3",
        )
        .bind(thread_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok((rows.into_iter().map(post_from_row).collect(), total))
    }

    async fn update_post(
        &self,
        id: Uuid,
        message: &str,
        edited_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<graph_owl_core::collaboration::Post>, StorageError> {
        let row =
            sqlx::query("UPDATE posts SET message = $2, edited_at = $3 WHERE id = $1 RETURNING *")
                .bind(id)
                .bind(message)
                .bind(edited_at)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(post_from_row))
    }

    async fn delete_post(&self, id: Uuid) -> Result<bool, StorageError> {
        let result = sqlx::query("UPDATE posts SET deleted = TRUE WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    // ---- Epic 35 Slice B: threads resolve ----

    async fn resolve_thread(
        &self,
        id: Uuid,
        resolved_by: &str,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<graph_owl_core::collaboration::Thread>, StorageError> {
        let row = sqlx::query(
            "UPDATE threads SET resolved = TRUE, resolved_by = $2, resolved_at = $3
             WHERE id = $1
             RETURNING *",
        )
        .bind(id)
        .bind(resolved_by)
        .bind(at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(thread_from_row))
    }

    async fn reopen_thread(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Thread>, StorageError> {
        let row = sqlx::query(
            "UPDATE threads SET resolved = FALSE, resolved_by = NULL, resolved_at = NULL
             WHERE id = $1
             RETURNING *",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(row.map(thread_from_row))
    }

    async fn unresolved_thread_count(&self, about: Uuid) -> Result<i64, StorageError> {
        sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE about = $1 AND NOT resolved")
            .bind(about)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))
    }

    // ---- Epic 35 Slice C: change proposals ----

    async fn insert_change_proposal(
        &self,
        proposal: graph_owl_core::collaboration::Proposal,
    ) -> Result<graph_owl_core::collaboration::Proposal, StorageError> {
        let row = sqlx::query(
            "INSERT INTO proposals
                (id, about, field, current_value, proposed_value, rationale, status,
                 proposed_by, decided_by, decided_at, decision_reason, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             RETURNING *",
        )
        .bind(proposal.id)
        .bind(proposal.about)
        .bind(&proposal.field)
        .bind(&proposal.current_value)
        .bind(&proposal.proposed_value)
        .bind(&proposal.rationale)
        .bind(change_proposal_status_str(proposal.status))
        .bind(&proposal.proposed_by)
        .bind(&proposal.decided_by)
        .bind(proposal.decided_at)
        .bind(&proposal.decision_reason)
        .bind(proposal.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        change_proposal_from_row(row)
    }

    async fn get_change_proposal(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::collaboration::Proposal>, StorageError> {
        let row = sqlx::query("SELECT * FROM proposals WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        row.map(change_proposal_from_row).transpose()
    }

    async fn list_change_proposals_for_entity(
        &self,
        about: Uuid,
        status: Option<graph_owl_core::collaboration::ProposalStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Proposal>, i64), StorageError> {
        let status_str = status.map(change_proposal_status_str);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(0);
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM proposals WHERE about = $1 AND ($2::text IS NULL OR status = $2)",
        )
        .bind(about)
        .bind(status_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rows = sqlx::query(
            "SELECT * FROM proposals
             WHERE about = $1 AND ($2::text IS NULL OR status = $2)
             ORDER BY created_at DESC, id
             LIMIT $3 OFFSET $4",
        )
        .bind(about)
        .bind(status_str)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let proposals = rows
            .into_iter()
            .map(change_proposal_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((proposals, total))
    }

    async fn list_change_proposals_by_user(
        &self,
        proposed_by: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Proposal>, i64), StorageError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(0);
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM proposals WHERE proposed_by = $1")
                .bind(proposed_by)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rows = sqlx::query(
            "SELECT * FROM proposals WHERE proposed_by = $1 ORDER BY created_at DESC, id LIMIT $2 OFFSET $3",
        )
        .bind(proposed_by)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let proposals = rows
            .into_iter()
            .map(change_proposal_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((proposals, total))
    }

    async fn list_change_proposals(
        &self,
        status: Option<graph_owl_core::collaboration::ProposalStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Proposal>, i64), StorageError> {
        let status_str = status.map(change_proposal_status_str);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(0);
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM proposals WHERE ($1::text IS NULL OR status = $1)",
        )
        .bind(status_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rows = sqlx::query(
            "SELECT * FROM proposals
             WHERE ($1::text IS NULL OR status = $1)
             ORDER BY created_at DESC, id
             LIMIT $2 OFFSET $3",
        )
        .bind(status_str)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let proposals = rows
            .into_iter()
            .map(change_proposal_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((proposals, total))
    }

    async fn decide_change_proposal(
        &self,
        id: Uuid,
        status: graph_owl_core::collaboration::ProposalStatus,
        decided_by: &str,
        decided_at: chrono::DateTime<chrono::Utc>,
        decision_reason: Option<String>,
    ) -> Result<Option<graph_owl_core::collaboration::Proposal>, StorageError> {
        let row = sqlx::query(
            "UPDATE proposals
             SET status = $2, decided_by = $3, decided_at = $4, decision_reason = $5
             WHERE id = $1 AND status = 'pending'
             RETURNING *",
        )
        .bind(id)
        .bind(change_proposal_status_str(status))
        .bind(decided_by)
        .bind(decided_at)
        .bind(&decision_reason)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        match row {
            Some(row) => Ok(Some(change_proposal_from_row(row)?)),
            None => self.get_change_proposal(id).await,
        }
    }

    // ---- Epic 35 Slice D: announcements ----

    async fn insert_announcement(
        &self,
        announcement: graph_owl_core::collaboration::Announcement,
    ) -> Result<graph_owl_core::collaboration::Announcement, StorageError> {
        let row = sqlx::query(
            "INSERT INTO announcements (id, about, message, starts_at, ends_at, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING *",
        )
        .bind(announcement.id)
        .bind(announcement.about)
        .bind(&announcement.message)
        .bind(announcement.starts_at)
        .bind(announcement.ends_at)
        .bind(&announcement.created_by)
        .bind(announcement.created_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(announcement_from_row(row))
    }

    async fn active_announcements(
        &self,
        about_ids: &[Uuid],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<graph_owl_core::collaboration::Announcement>, StorageError> {
        let rows = sqlx::query(
            "SELECT * FROM announcements
             WHERE about = ANY($1) AND starts_at <= $2 AND ends_at > $2
             ORDER BY starts_at DESC",
        )
        .bind(about_ids)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows.into_iter().map(announcement_from_row).collect())
    }

    async fn list_announcements(
        &self,
        about: Uuid,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<graph_owl_core::collaboration::Announcement>, i64), StorageError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let offset = i64::try_from(offset).unwrap_or(0);
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM announcements WHERE about = $1")
            .bind(about)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let rows = sqlx::query(
            "SELECT * FROM announcements WHERE about = $1 ORDER BY starts_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(about)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok((rows.into_iter().map(announcement_from_row).collect(), total))
    }

    // ---- Epic 35 Slice E: reactions ----

    async fn has_reacted(
        &self,
        post_id: Uuid,
        user_id: &str,
        kind: graph_owl_core::collaboration::ReactionKind,
    ) -> Result<bool, StorageError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM reactions WHERE post_id = $1 AND user_id = $2 AND kind = $3",
        )
        .bind(post_id)
        .bind(user_id)
        .bind(reaction_kind_str(kind))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(count > 0)
    }

    async fn add_reaction(
        &self,
        post_id: Uuid,
        user_id: &str,
        kind: graph_owl_core::collaboration::ReactionKind,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO reactions (post_id, user_id, kind) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(post_id)
        .bind(user_id)
        .bind(reaction_kind_str(kind))
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn remove_reaction(
        &self,
        post_id: Uuid,
        user_id: &str,
        kind: graph_owl_core::collaboration::ReactionKind,
    ) -> Result<bool, StorageError> {
        let result =
            sqlx::query("DELETE FROM reactions WHERE post_id = $1 AND user_id = $2 AND kind = $3")
                .bind(post_id)
                .bind(user_id)
                .bind(reaction_kind_str(kind))
                .execute(&self.pool)
                .await
                .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }

    async fn reaction_counts(
        &self,
        post_id: Uuid,
    ) -> Result<Vec<(graph_owl_core::collaboration::ReactionKind, i64)>, StorageError> {
        let rows = sqlx::query(
            "SELECT kind, COUNT(*) AS reaction_count FROM reactions WHERE post_id = $1 GROUP BY kind",
        )
        .bind(post_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    reaction_kind_from_str(row.get::<&str, _>("kind"))?,
                    row.get("reaction_count"),
                ))
            })
            .collect()
    }

    // ---- Epic 35 Slice F: the activity feed ----

    async fn collaboration_activity_for_entity(
        &self,
        about: Uuid,
        limit: usize,
    ) -> Result<Vec<graph_owl_storage::ActivityRow>, StorageError> {
        use graph_owl_core::collaboration::ActivityKind;
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

        let mut items = Vec::new();

        let thread_rows = sqlx::query(
            "SELECT id, created_at, created_by, COALESCE(field, 'general') AS summary
             FROM threads WHERE about = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(about)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for row in thread_rows {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::ThreadStarted,
                occurred_at: row.get("created_at"),
                id: row.get("id"),
                actor: row.get("created_by"),
                summary: row.get("summary"),
            });
        }

        let resolved_rows = sqlx::query(
            "SELECT id, resolved_at, resolved_by
             FROM threads WHERE about = $1 AND resolved AND resolved_at IS NOT NULL
             ORDER BY resolved_at DESC LIMIT $2",
        )
        .bind(about)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for row in resolved_rows {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::ThreadResolved,
                occurred_at: row.get("resolved_at"),
                id: row.get("id"),
                actor: row
                    .get::<Option<String>, _>("resolved_by")
                    .unwrap_or_default(),
                summary: "resolved".to_string(),
            });
        }

        let post_rows = sqlx::query(
            "SELECT p.id AS id, p.created_at AS created_at, p.author AS author, p.message AS message
             FROM posts p JOIN threads t ON t.id = p.thread_id
             WHERE t.about = $1 AND NOT p.deleted
             ORDER BY p.created_at DESC LIMIT $2",
        )
        .bind(about)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for row in post_rows {
            let message: String = row.get("message");
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::PostAdded,
                occurred_at: row.get("created_at"),
                id: row.get("id"),
                actor: row.get("author"),
                summary: message.chars().take(120).collect(),
            });
        }

        let proposal_rows = sqlx::query(
            "SELECT id, created_at, proposed_by, field FROM proposals
             WHERE about = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(about)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for row in proposal_rows {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::ProposalCreated,
                occurred_at: row.get("created_at"),
                id: row.get("id"),
                actor: row.get("proposed_by"),
                summary: row.get("field"),
            });
        }

        let decided_rows = sqlx::query(
            "SELECT id, decided_at, decided_by, status FROM proposals
             WHERE about = $1 AND decided_at IS NOT NULL ORDER BY decided_at DESC LIMIT $2",
        )
        .bind(about)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for row in decided_rows {
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::ProposalDecided,
                occurred_at: row
                    .get::<Option<chrono::DateTime<chrono::Utc>>, _>("decided_at")
                    .unwrap_or_default(),
                id: row.get("id"),
                actor: row
                    .get::<Option<String>, _>("decided_by")
                    .unwrap_or_default(),
                summary: row.get::<&str, _>("status").to_string(),
            });
        }

        let announcement_rows = sqlx::query(
            "SELECT id, created_at, created_by, message FROM announcements
             WHERE about = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(about)
        .bind(limit_i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        for row in announcement_rows {
            let message: String = row.get("message");
            items.push(graph_owl_storage::ActivityRow {
                kind: ActivityKind::AnnouncementCreated,
                occurred_at: row.get("created_at"),
                id: row.get("id"),
                actor: row.get("created_by"),
                summary: message.chars().take(120).collect(),
            });
        }

        items.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at).then(b.id.cmp(&a.id)));
        items.truncate(limit);
        Ok(items)
    }

    async fn force_delete_custom_property(
        &self,
        id: Uuid,
        entity_type: &str,
        name: &str,
        updated_by: &str,
    ) -> Result<i64, StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        sqlx::query("DELETE FROM custom_properties WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        // **Row by row, deliberately, and this is the expensive choice.** One
        // `UPDATE ... SET extension = extension - $name` would strip every
        // value in a single statement and record none of it: no version bump,
        // no history row, no diff. An entity whose `costCenter` vanished has
        // changed, and a catalog that cannot say when is exactly the catalog
        // this epic exists to replace. Force-deleting a definition is a rare,
        // admin-only, deliberately-typed operation; paying per row for an
        // auditable one is the right side of that trade.
        let affected = sqlx::query(&format!(
            "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
              WHERE kind::text = $1 AND extension ? $2
              ORDER BY id
                FOR UPDATE"
        ))
        .bind(entity_type)
        .bind(name)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let mut changed = 0_i64;
        for row in affected {
            let before = asset_from_row(row);
            let mut after = before.clone();
            let mut bag = before.extension.clone().unwrap_or_default();
            bag.remove(name);
            after.extension = Some(bag.clone());

            let diff = graph_owl_core::envelope::ChangeDescription::between(
                &serde_json::to_value(&before).unwrap_or_default(),
                &serde_json::to_value(&after).unwrap_or_default(),
            );
            let kind = graph_owl_core::envelope::classify(&diff);
            if matches!(kind, graph_owl_core::envelope::ChangeKind::None) {
                continue;
            }
            let next = before.version.bump(kind);

            let updated_row = sqlx::query(&format!(
                "UPDATE assets SET extension = $2, version_major = $3, version_minor = $4,
                     updated_by = $5, change_description = $6, updated_at = now()
                 WHERE id = $1
                 RETURNING {ASSET_COLUMNS}"
            ))
            .bind(before.id)
            .bind(serde_json::to_value(&bag).unwrap_or_default())
            .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
            .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
            .bind(updated_by)
            .bind(serde_json::to_value(&diff).ok())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
            let updated = asset_from_row(updated_row);

            sqlx::query(
                "INSERT INTO asset_versions
                     (asset_id, version_major, version_minor, snapshot, change_description, updated_by, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT DO NOTHING",
            )
            .bind(before.id)
            .bind(i32::try_from(next.major).unwrap_or(i32::MAX))
            .bind(i32::try_from(next.minor).unwrap_or(i32::MAX))
            .bind(serde_json::to_value(&updated).unwrap_or_default())
            .bind(serde_json::to_value(&diff).ok())
            .bind(updated_by)
            .bind(updated.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

            changed += 1;
        }

        tx.commit()
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(changed)
    }

    async fn delete_extraction_run(&self, run_id: Uuid) -> Result<bool, StorageError> {
        // The claims and discards go with it by `ON DELETE CASCADE`, which is
        // what makes "a bad run is deletable wholesale" a schema guarantee
        // rather than a thing this method has to remember.
        let done = sqlx::query("DELETE FROM extraction_runs WHERE id = $1")
            .bind(run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    // ---- Epic 32: agent capabilities ----

    async fn upsert_agent_grant(
        &self,
        grant: &graph_owl_authz::agent::AgentGrant,
    ) -> Result<(), StorageError> {
        let capabilities: Vec<String> = grant
            .capabilities
            .iter()
            .map(|capability| capability.as_str().to_string())
            .collect();
        // One grant per agent (the table's unique constraint), so this is an
        // upsert rather than an insert: a second grant row would make "what may
        // this agent do" a union nobody wrote, and a revocation would have to
        // find every row to be a revocation at all.
        sqlx::query(
            "INSERT INTO agent_grants
                 (id, agent_id, capabilities, scope_fqn_prefix, max_writes,
                  window_seconds, expires_at, granted_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (agent_id) DO UPDATE SET
                 capabilities     = EXCLUDED.capabilities,
                 scope_fqn_prefix = EXCLUDED.scope_fqn_prefix,
                 max_writes       = EXCLUDED.max_writes,
                 window_seconds   = EXCLUDED.window_seconds,
                 expires_at       = EXCLUDED.expires_at,
                 granted_by       = EXCLUDED.granted_by,
                 updated_at       = now()",
        )
        .bind(grant.id)
        .bind(&grant.agent.id)
        .bind(&capabilities)
        .bind(grant.scope.as_ref().map(|scope| scope.fqn_prefix.clone()))
        .bind(i32::try_from(grant.rate_limit.max_writes).unwrap_or(i32::MAX))
        .bind(i32::try_from(grant.rate_limit.window_seconds).unwrap_or(i32::MAX))
        .bind(grant.expires_at)
        .bind(&grant.granted_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn agent_grant(
        &self,
        agent_id: &str,
    ) -> Result<Option<graph_owl_authz::agent::AgentGrant>, StorageError> {
        let row = sqlx::query(
            "SELECT g.id, g.agent_id, g.capabilities, g.scope_fqn_prefix,
                    g.max_writes, g.window_seconds, g.expires_at, g.granted_by,
                    g.created_at, g.updated_at, u.display_name
               FROM agent_grants g
               JOIN users u ON u.id = g.agent_id
              WHERE g.agent_id = $1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.as_ref().map(agent_grant_from_row))
    }

    async fn list_agent_grants(
        &self,
    ) -> Result<Vec<graph_owl_authz::agent::AgentGrant>, StorageError> {
        let rows = sqlx::query(
            "SELECT g.id, g.agent_id, g.capabilities, g.scope_fqn_prefix,
                    g.max_writes, g.window_seconds, g.expires_at, g.granted_by,
                    g.created_at, g.updated_at, u.display_name
               FROM agent_grants g
               JOIN users u ON u.id = g.agent_id
              ORDER BY g.agent_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(rows.iter().map(agent_grant_from_row).collect())
    }

    async fn revoke_agent_grant(&self, agent_id: &str) -> Result<bool, StorageError> {
        let done = sqlx::query("DELETE FROM agent_grants WHERE agent_id = $1")
            .bind(agent_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn create_proposal(
        &self,
        proposal: &graph_owl_authz::agent::Proposal,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO agent_proposals
                 (id, agent_id, target_fqn, capability, change, rationale,
                  confidence, status, base_major, base_minor)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'open', $8, $9)",
        )
        .bind(proposal.id)
        .bind(&proposal.proposed_by.id)
        .bind(&proposal.target_fqn)
        .bind(proposal.capability.as_str())
        .bind(&proposal.change)
        .bind(&proposal.rationale)
        .bind(proposal.confidence)
        .bind(i32::try_from(proposal.base_version.major).unwrap_or(i32::MAX))
        .bind(i32::try_from(proposal.base_version.minor).unwrap_or(i32::MAX))
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn get_proposal(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_authz::agent::Proposal>, StorageError> {
        let row = sqlx::query(
            "SELECT p.id, p.agent_id, p.target_fqn, p.capability, p.change,
                    p.rationale, p.confidence, p.status, p.base_major, p.base_minor,
                    p.decided_by, p.decided_at, p.created_at, u.display_name
               FROM agent_proposals p
               JOIN users u ON u.id = p.agent_id
              WHERE p.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(row.as_ref().map(proposal_from_row))
    }

    async fn list_proposals(
        &self,
        agent_id: Option<&str>,
        status: Option<graph_owl_authz::agent::ProposalStatus>,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_authz::agent::Proposal>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let rows = sqlx::query(
            "SELECT p.id, p.agent_id, p.target_fqn, p.capability, p.change,
                    p.rationale, p.confidence, p.status, p.base_major, p.base_minor,
                    p.decided_by, p.decided_at, p.created_at, u.display_name
               FROM agent_proposals p
               JOIN users u ON u.id = p.agent_id
              WHERE ($1::text IS NULL OR p.agent_id = $1)
                AND ($2::text IS NULL OR p.status = $2)
                AND ($3::text IS NULL OR (p.created_at::text, p.id) < ($3, $4))
              ORDER BY p.created_at DESC, p.id DESC
              LIMIT $5",
        )
        .bind(agent_id)
        .bind(status.map(proposal_status_str))
        .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
        .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
        .bind(overfetch)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Page::from_overfetch(
            rows.iter().map(proposal_from_row).collect(),
            page.limit,
            |p: &graph_owl_authz::agent::Proposal| Cursor::new(p.created_at.to_string(), p.id),
        ))
    }

    async fn decide_proposal(
        &self,
        id: Uuid,
        status: graph_owl_authz::agent::ProposalStatus,
        decided_by: &str,
    ) -> Result<bool, StorageError> {
        // `AND status = 'open'` is the whole guard: **deciding twice is a
        // conflict, not an update.** Two reviewers reaching opposite
        // conclusions must not have the second silently win, and without this
        // predicate the last writer would.
        let done = sqlx::query(
            "UPDATE agent_proposals
                SET status = $2, decided_by = $3, decided_at = now()
              WHERE id = $1 AND status = 'open'",
        )
        .bind(id)
        .bind(proposal_status_str(status))
        .bind(decided_by)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(done.rows_affected() > 0)
    }

    async fn record_agent_activity(
        &self,
        activity: &graph_owl_authz::agent::AgentActivity,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO agent_activity
                 (id, agent_id, capability, target_fqn, outcome, refusal, at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(activity.id)
        .bind(&activity.agent_id)
        .bind(activity.capability.as_str())
        .bind(&activity.target_fqn)
        .bind(activity_outcome_str(activity.outcome))
        .bind(activity.refusal.as_deref())
        .bind(activity.at)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(())
    }

    async fn agent_activity(
        &self,
        agent_id: &str,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_authz::agent::AgentActivity>, StorageError> {
        let overfetch = i64::try_from(page.limit)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        let rows = sqlx::query(
            "SELECT id, agent_id, capability, target_fqn, outcome, refusal, at
               FROM agent_activity
              WHERE agent_id = $1
                AND ($2::text IS NULL OR (at::text, id) < ($2, $3))
              ORDER BY at DESC, id DESC
              LIMIT $4",
        )
        .bind(agent_id)
        .bind(page.after.as_ref().map(|c| c.sort_key.clone()))
        .bind(page.after.as_ref().map_or_else(Uuid::nil, |c| c.id))
        .bind(overfetch)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        Ok(Page::from_overfetch(
            rows.iter().map(activity_from_row).collect(),
            page.limit,
            |a: &graph_owl_authz::agent::AgentActivity| Cursor::new(a.at.to_string(), a.id),
        ))
    }

    async fn agent_writes_in_window(
        &self,
        agent_id: &str,
        capability: graph_owl_authz::agent::AgentCapability,
        window_seconds: u32,
    ) -> Result<(u32, Option<u64>), StorageError> {
        // **Refusals do not consume budget.** An agent already being refused
        // must not have each refusal push its own recovery further away — that
        // turns a misconfiguration into a permanent lockout, and the refusal is
        // already recorded for the audit.
        let row = sqlx::query(
            "SELECT count(*) AS made,
                    EXTRACT(EPOCH FROM (now() - min(at)))::bigint AS oldest_age
               FROM agent_activity
              WHERE agent_id = $1
                AND capability = $2
                AND outcome <> 'refused'
                AND at > now() - make_interval(secs => $3::double precision)",
        )
        .bind(agent_id)
        .bind(capability.as_str())
        .bind(f64::from(window_seconds))
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let made: i64 = row.try_get("made").unwrap_or(0);
        let oldest: Option<i64> = row.try_get("oldest_age").unwrap_or(None);
        Ok((
            u32::try_from(made).unwrap_or(u32::MAX),
            oldest.and_then(|age| u64::try_from(age).ok()),
        ))
    }
}

fn custom_property_from_row(row: &PgRow) -> CustomProperty {
    let property_type: String = row.get("property_type");
    CustomProperty {
        name: row.get("name"),
        entity_type: row.get("entity_type"),
        // A stored type outside the supported set means the column was written
        // by something other than this code. Defaulting to `String` keeps the
        // read working rather than failing every list call, and the value is
        // still validated on every write.
        property_type: graph_owl_core::custom_property::PropertyType::parse(&property_type)
            .unwrap_or(graph_owl_core::custom_property::PropertyType::String),
        description: row.get("description"),
        constraints: serde_json::from_value(row.get("constraints")).unwrap_or_default(),
    }
}

fn queued_claim_from_row(row: &PgRow) -> QueuedClaimRecord {
    QueuedClaimRecord {
        id: row.get("id"),
        run_id: row.get("run_id"),
        subject: row.get("subject"),
        predicate: row.get("predicate"),
        object: row.get("object"),
        confidence: row.get("confidence"),
        evidence_start: row.get("evidence_start"),
        evidence_end: row.get("evidence_end"),
        state: row.get("state"),
        decided_by: row.get("decided_by"),
        reason: row.get("reason"),
    }
}

/// The one place `AccessPredicate` becomes SQL.
///
/// Returns `None` for "nothing visible", which callers must answer with an
/// empty result rather than a broader query — the alternative is a predicate
/// that silently matches everything.
fn lower(predicate: &AccessPredicate) -> Option<(Vec<String>, Vec<String>)> {
    match predicate {
        AccessPredicate::Nothing => None,
        // `%` matches every FQN. An empty deny array is correct rather than a
        // sentinel: `x LIKE ANY('{}')` is false, so `NOT (...)` is true and
        // every row passes the deny check. A NUL sentinel would have been both
        // unnecessary and rejected — Postgres text cannot contain NUL.
        AccessPredicate::All => Some((vec!["%".to_string()], Vec::new())),
        AccessPredicate::Fqn {
            allow_prefixes,
            deny_prefixes,
        } => Some((
            allow_prefixes.iter().map(|p| format!("{p}%")).collect(),
            deny_prefixes.iter().map(|p| format!("{p}%")).collect(),
        )),
    }
}

const VISIBILITY: &str =
    "AND (fully_qualified_name LIKE ANY($5)) AND NOT (fully_qualified_name LIKE ANY($6))";
const VISIBILITY_SEARCH: &str =
    "AND (fully_qualified_name LIKE ANY($6)) AND NOT (fully_qualified_name LIKE ANY($7))";

/// **AND across requested tags, table-level match includes a column's own
/// tag** — Epic 25, wired to `?tags=` Phase 2.1. `count(DISTINCT ...) =
/// array_length(...)` rather than one `EXISTS` per tag: the tag list is a
/// single bound array, not one placeholder per value, so its length is not
/// known when this string is built.
///
/// **`kind = 'table'` guards the prefix branch, and this is load-bearing, not
/// decorative.** A bare `target_fqn LIKE fully_qualified_name || '.%'` was
/// tried first and matches *any* descendant at *any* depth — which makes a
/// tag on a table's column also read as carried by that table's schema,
/// database and service, since a table's own FQN is itself a `LIKE` match
/// against its ancestors' prefixes. Gating on `kind = 'table'` restricts the
/// leniency to exactly the case the plan asks for (a column's tag counting
/// toward its own table), because a column's only possible parent kind in
/// this schema is `table` — every other kind falls through to the exact
/// match only.
fn tags_expr(param: usize) -> String {
    format!(
        "AND (${param}::text[] IS NULL OR (
               SELECT count(DISTINCT t.fully_qualified_name)
                 FROM tag_labels l JOIN tags t ON t.id = l.tag_id
                WHERE l.state = 'confirmed'
                  AND t.fully_qualified_name = ANY(${param})
                  AND (l.target_fqn = assets.fully_qualified_name
                       OR (assets.kind = 'table'
                           AND l.target_fqn LIKE assets.fully_qualified_name || '.%'))
             ) = array_length(${param}, 1))"
    )
}

/// **A computed status, pushed into SQL rather than duplicated as a stored
/// column** — Epic 26, wired to `?certification=` Phase 2.3. Matches "any
/// type", not one specific certification type: a target can hold several
/// certifications at once (Gold and a data-quality stamp, say), and asking
/// which one this filter means would need a `?certificationType=` parameter
/// nobody has asked for yet. `EXISTS`/`NOT EXISTS` per branch rather than
/// aggregating to one status avoids the ambiguity a mixed-status target
/// would otherwise force ("valid AND expired" has no single right answer).
/// `status_param` is reused across every `WHEN` arm — one bound value,
/// compared against each branch's own literal.
fn certification_expr(status_param: usize, window_param: usize) -> String {
    format!(
        "AND (${status_param}::text IS NULL OR (
               CASE ${status_param}
                 WHEN 'none' THEN NOT EXISTS (
                   SELECT 1 FROM certifications c
                    WHERE c.target_fqn = assets.fully_qualified_name
                      AND c.superseded_by IS NULL)
                 WHEN 'valid' THEN EXISTS (
                   SELECT 1 FROM certifications c
                    WHERE c.target_fqn = assets.fully_qualified_name
                      AND c.superseded_by IS NULL
                      AND c.expires_at > now() + make_interval(days => ${window_param}))
                 WHEN 'expiringSoon' THEN EXISTS (
                   SELECT 1 FROM certifications c
                    WHERE c.target_fqn = assets.fully_qualified_name
                      AND c.superseded_by IS NULL
                      AND c.expires_at > now()
                      AND c.expires_at <= now() + make_interval(days => ${window_param}))
                 WHEN 'expired' THEN EXISTS (
                   SELECT 1 FROM certifications c
                    WHERE c.target_fqn = assets.fully_qualified_name
                      AND c.superseded_by IS NULL
                      AND c.expires_at <= now())
                 ELSE FALSE
               END))"
    )
}

/// **The same computation `graph_owl_core::quality::health_of` runs in
/// Rust, pushed into SQL — Epic 30, decision 4.5, wired to `?health=`.**
/// Not a stored column, the identical "computation pushed into SQL rather
/// than a second, stored copy" reasoning `certification_expr` above already
/// uses.
///
/// **Precedence, matched exactly, including the subtle case:** a test case
/// that is *both* stale and failed counts toward staleness, never toward
/// failure — `health_of`'s own per-case loop checks `is_stale` before it
/// checks `status == Failed`. Getting this backwards here would make the
/// filter disagree with the read path on exactly the ambiguous case a
/// reviewer would reach for the filter to find. `bool_or(NOT is_stale AND
/// status = 'failed')` for "any real failure" and `bool_or(is_stale)` for
/// "any staleness" are aggregated once per asset via one grouped subquery,
/// then mapped through the identical four-way precedence
/// (`case_count = 0` → unknown, any failure → unhealthy, any staleness →
/// stale, else healthy) `health_of` itself uses.
///
/// **`expected_cadence::interval` is a safe cast, not a guess**: the column
/// only ever holds what `graph_owl_core::quality::parse_cadence` accepted
/// on the way in, which refuses `Y`/`M` designators specifically because
/// they are not fixed-length — the same property that makes every value
/// Postgres could find there a valid ISO 8601 interval literal (`'P1D'`,
/// `'P2W'`, verified directly: `'P2W'::interval` → `14 days`).
///
/// Kept as one `LEFT JOIN LATERAL` per case exactly matching
/// `latest_results_for`'s own shape (the trusted, existing single-asset
/// read path), rather than a second, differently-shaped query that could
/// silently answer a different question.
fn health_expr(status_param: usize) -> String {
    format!(
        "AND (${status_param}::text IS NULL OR (
               SELECT CASE
                        WHEN h.case_count = 0 THEN 'unknown'
                        WHEN h.any_failed THEN 'unhealthy'
                        WHEN h.any_stale THEN 'stale'
                        ELSE 'healthy'
                      END = ${status_param}
                 FROM (
                   SELECT count(*) AS case_count,
                          bool_or(NOT stale_calc.is_stale AND r.status = 'failed') AS any_failed,
                          bool_or(stale_calc.is_stale) AS any_stale
                     FROM test_cases c
                     LEFT JOIN test_definitions d ON d.id = c.definition_id
                     LEFT JOIN LATERAL (
                         SELECT status, observed_at FROM test_results
                          WHERE case_id = c.id ORDER BY observed_at DESC LIMIT 1
                     ) r ON TRUE
                     CROSS JOIN LATERAL (
                         SELECT (
                           r.status IS NULL OR r.observed_at IS NULL OR r.status = 'aborted'
                           OR (coalesce(c.expected_cadence, d.expected_cadence) IS NOT NULL
                               AND now() - r.observed_at
                                   > coalesce(c.expected_cadence, d.expected_cadence)::interval)
                         ) AS is_stale
                     ) stale_calc
                    WHERE c.target_fqn = assets.fully_qualified_name
                 ) h
             ))"
    )
}

/// The relevance score **and** the keyset cursor for a relevance-ordered page,
/// in one expression.
///
/// `ts_rank_cd` weighs *cover density* — how close the matched terms sit to one
/// another — which is what separates a table called `upi_transactions` from one
/// whose description happens to mention UPI and transactions ten lines apart.
/// Normalisation `32` is `rank / (rank + 1)`; bounded is the point, because an
/// unbounded rank cannot be encoded into a fixed-width sort key.
///
/// One constant rather than a score and a key derived from it: `ORDER BY`, the
/// keyset comparison and the emitted cursor must all be the same expression, and
/// three call sites deriving it separately is three chances for a page boundary
/// to drift.
///
/// `NNNN:fqn`, where `NNNN` is the rank **inverted** — `9999 - rank * 9999` — so
/// that descending relevance is *ascending* string order. Every other list in
/// this adapter paginates with `(sort_key, id) > ($n, $m)`, and inverting here
/// means relevance ordering reuses that comparison unchanged instead of needing
/// a second, differently-directed one.
///
/// Four digits because two documents whose normalised ranks differ by less than
/// 1/10000 are not meaningfully differently relevant to a person, and the FQN
/// suffix makes the ordering total regardless — so the digits only have to
/// separate results a reader could actually tell apart.
/// Epic 28's usage rollups, folded into search ranking — Phase 3 item 3.5.
/// Joined once and referenced from [`RANK_KEY`]; every query that uses
/// `RANK_KEY` must also carry this in its `FROM` clause, the same
/// obligation `{OWNERS_EXPR}`/`{DOMAIN_ID_EXPR}` already put on callers.
///
/// **The trailing-30-day read count, damped.** `recent / (recent + 10)`
/// saturates toward 1.0 rather than growing unbounded, so one asset with
/// ten thousand reads cannot swamp the term for everything else — the
/// same shrinkage idea `usage.rs`'s own `TREND_VOLUME_FLOOR` already uses
/// for a different noise problem, at a different constant because the
/// question here ("how much should this count move a rank") is not the
/// question there ("is this count large enough to call a trend at all").
const POPULARITY_JOIN: &str = "LEFT JOIN (
                 SELECT asset_fqn, SUM(count) AS recent
                   FROM usage_rollups
                  WHERE operation = 'read' AND day >= CURRENT_DATE - 30
                  GROUP BY asset_fqn
             ) pop ON pop.asset_fqn = assets.fully_qualified_name";

/// **A fixed weight, not a caller-exposed knob.** Exposing it would turn
/// "how should search rank" into a per-request decision no caller has the
/// information to make well, and Epic 37a's own search budgets are tuned
/// against one query shape, not an open set of them.
///
/// `ts_rank_cd`'s own normalised output (the `32` argument below) and
/// [`POPULARITY_JOIN`]'s damped count are both bounded in `[0, 1)`, so
/// `0.15` caps how much popularity can move a result within roughly one
/// relevance "notch" — real, but never enough for a wildly popular,
/// weakly-relevant result to outrank a strong lexical match. At weight
/// `0.0` the term would contribute exactly nothing; that case is proved
/// by a real before/after query comparison
/// (`popularity_weight_zero_would_reproduce_lexical_only_ordering_exactly`
/// in `crates/graph-owl-storage-postgres/tests/`), not merely reasoned
/// about, because floating-point claims are exactly the kind of thing
/// worth checking against the database rather than assumed.
const RANK_KEY: &str = "lpad((9999 - (LEAST(1.0,
                 ts_rank_cd(assets.search_vector, q.ts, 32)
                     + 0.15 * COALESCE(pop.recent, 0) / (COALESCE(pop.recent, 0) + 10.0)
             ) * 9999)::int)::text, 4, '0') || ':' || assets.fully_qualified_name";

/// The domain an asset falls under, as JSON, or `null`.
///
/// **The nearest assigned ancestor wins and the walk stops there** — the same
/// shape as [`OWNERS_EXPR`], and for the same reason. Accumulating every
/// assigned ancestor would answer "which domains is this under", a question
/// with several answers, which is the shared accountability decision 1 refuses.
///
/// Correlated on `assets.id`, so it composes into any query that already has
/// the `assets` row in scope without a second round trip per row.
const DOMAIN_EXPR: &str = "(WITH RECURSIVE ancestry (node, next_up, hops) AS (
            SELECT seed.id, seed.parent_id, 0 FROM assets seed WHERE seed.id = assets.id
        UNION ALL
            SELECT up.id, up.parent_id, ancestry.hops + 1
              FROM assets up JOIN ancestry ON up.id = ancestry.next_up
    ),
    nearest AS (
        SELECT a.domain_id, ancestry.hops
          FROM ancestry JOIN assets a ON a.id = ancestry.node
         WHERE a.domain_id IS NOT NULL
         ORDER BY ancestry.hops
         LIMIT 1
    )
    SELECT json_build_object(
        'id',                 d.id,
        'name',               d.name,
        'fullyQualifiedName', d.fully_qualified_name,
        'inherited',          nearest.hops > 0
    )
    FROM nearest JOIN domains d ON d.id = nearest.domain_id)";

/// Just the resolved domain id, for filtering. The same walk as
/// [`DOMAIN_EXPR`] without building the object — a filter that compared
/// against the JSON would have to parse it per row.
const DOMAIN_ID_EXPR: &str = "(WITH RECURSIVE ancestry (node, next_up, hops) AS (
            SELECT seed.id, seed.parent_id, 0 FROM assets seed WHERE seed.id = assets.id
        UNION ALL
            SELECT up.id, up.parent_id, ancestry.hops + 1
              FROM assets up JOIN ancestry ON up.id = ancestry.next_up
    )
    SELECT a.domain_id
      FROM ancestry JOIN assets a ON a.id = ancestry.node
     WHERE a.domain_id IS NOT NULL
     ORDER BY ancestry.hops
     LIMIT 1)";

const DOMAIN_COLUMNS: &str = "id, name, fully_qualified_name, parent_id, description, domain_type, \
     version_major, version_minor, updated_by, change_description, deleted, deleted_at, created_at, updated_at";

const PRODUCT_COLUMNS: &str = "id, name, fully_qualified_name, description, purpose, domain_id, \
     version_major, version_minor, updated_by, change_description, deleted, deleted_at, created_at, updated_at";

fn domain_from_row(row: &PgRow) -> Domain {
    Domain {
        id: row.get("id"),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        parent_id: row.get("parent_id"),
        description: row.get("description"),
        domain_type: row.get("domain_type"),
        // Filled by the caller, which is the only layer that knows whether the
        // query joined them. A `Vec::new()` here that silently meant "none"
        // would make a domain with experts look like one without.
        experts: Vec::new(),
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
        },
        updated_by: row.get("updated_by"),
        change_description: row
            .get::<Option<serde_json::Value>, _>("change_description")
            .and_then(|v| serde_json::from_value(v).ok()),
        deleted: row.get("deleted"),
        deleted_at: row.get("deleted_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn product_from_row(row: &PgRow) -> DataProduct {
    DataProduct {
        id: row.get("id"),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        description: row.get("description"),
        purpose: row.get("purpose"),
        domain_id: row.get("domain_id"),
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
        },
        updated_by: row.get("updated_by"),
        change_description: row
            .get::<Option<serde_json::Value>, _>("change_description")
            .and_then(|v| serde_json::from_value(v).ok()),
        deleted: row.get("deleted"),
        deleted_at: row.get("deleted_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

const CLASSIFICATION_COLUMNS: &str = "id, name, description, mutually_exclusive, \
     version_major, version_minor, updated_by, change_description, created_at, updated_at";

const TAG_COLUMNS: &str = "id, name, classification_id, fully_qualified_name, description, \
     version_major, version_minor, updated_by, created_at, updated_at";

fn classification_from_row(row: &PgRow) -> Classification {
    Classification {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        mutually_exclusive: row.get("mutually_exclusive"),
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
        },
        updated_by: row.get("updated_by"),
        change_description: row
            .get::<Option<serde_json::Value>, _>("change_description")
            .and_then(|v| serde_json::from_value(v).ok()),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn tag_from_row(row: &PgRow) -> Tag {
    Tag {
        id: row.get("id"),
        name: row.get("name"),
        classification_id: row.get("classification_id"),
        fully_qualified_name: row.get("fully_qualified_name"),
        description: row.get("description"),
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
        },
        updated_by: row.get("updated_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn label_from_row(row: &PgRow) -> TagLabel {
    TagLabel {
        tag_fqn: row.get("tag_fqn"),
        target_fqn: row.get("target_fqn"),
        // A stored value the enum has never heard of falls back rather than
        // panicking: the columns carry `CHECK`s, so this is only reachable by a
        // migration that widened one, and a read that dies is worse than one
        // that is conservative.
        label_type: LabelType::parse(row.get::<String, _>("label_type").as_str())
            .unwrap_or(LabelType::Manual),
        state: LabelState::parse(row.get::<String, _>("state").as_str())
            .unwrap_or(LabelState::Confirmed),
        applied_by: row.get("applied_by"),
        applied_at: row.get("applied_at"),
        confirmed_by: row.get("confirmed_by"),
    }
}

/// A unique violation becomes a named conflict; everything else stays
/// unexpected. Written once because five call sites needed the same three
/// lines, and five copies is five chances to map one of them wrong.
fn conflict_or_unexpected(error: &sqlx::Error, detail: String) -> StorageError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        StorageError::Conflict {
            detail,
            existing_id: None,
            kind: ConflictKind::Fqn,
        }
    } else {
        StorageError::Unexpected(error.to_string())
    }
}

const CONTRACT_COLUMNS: &str = "id, name, asset_fqn, producer, compatibility, status, \
     allow_additional, version_major, version_minor, updated_by, change_description, \
     created_at, updated_at";

const TEST_CASE_COLUMNS: &str = "c.id, c.name, c.target_fqn, c.test_type, c.description, \
     c.definition_id, c.suite_id, coalesce(c.expected_cadence, d.expected_cadence) \
     AS expected_cadence";

fn test_case_from_row(row: &PgRow) -> StoredTestCase {
    StoredTestCase {
        id: row.get("id"),
        name: row.get("name"),
        target_fqn: row.get("target_fqn"),
        test_type: row.get("test_type"),
        description: row.get("description"),
        definition_id: row.get("definition_id"),
        suite_id: row.get("suite_id"),
        expected_cadence: row.get("expected_cadence"),
    }
}

fn result_from_row(row: &PgRow) -> StoredTestResult {
    StoredTestResult {
        id: row.get("id"),
        case_id: row.get("case_id"),
        status: graph_owl_core::quality::TestStatus::parse(row.get::<String, _>("status").as_str())
            .unwrap_or(graph_owl_core::quality::TestStatus::Aborted),
        observed_at: row.get("observed_at"),
        message: row.get("message"),
        metrics: row.get("metrics"),
    }
}

fn breach_from_row(row: &PgRow) -> ContractBreach {
    ContractBreach {
        id: row.get("id"),
        contract_id: row.get("contract_id"),
        column: row.get("column_name"),
        detail: row.get("detail"),
        asset_version: row.get("asset_version"),
        detected_at: row.get("detected_at"),
    }
}

fn rollup_from_row(row: &PgRow) -> UsageRollup {
    UsageRollup {
        consumer_key: row.get("consumer_key"),
        day: row.get("day"),
        // A stored value the enum has never heard of falls back rather than
        // panicking — the column has a `CHECK`, so this is only reachable by a
        // migration that widened it.
        operation: UsageOperation::parse(row.get::<String, _>("operation").as_str())
            .unwrap_or(UsageOperation::Read),
        count: u64::try_from(row.get::<i64, _>("count")).unwrap_or(0),
        total_rows: row
            .get::<Option<i64>, _>("total_rows")
            .and_then(|n| u64::try_from(n).ok()),
    }
}

fn asset_from_row(row: PgRow) -> Asset {
    Asset {
        // A stored state the enum has never heard of falls back to `Active`
        // rather than panicking: the column has a `CHECK`, so this can only be
        // reached by a migration that widened it, and a read that dies is worse
        // than one that is conservative.
        lifecycle: graph_owl_core::lifecycle::LifecycleState::parse(
            row.get::<String, _>("lifecycle").as_str(),
        )
        .unwrap_or_default(),
        deprecation: row
            .get::<Option<serde_json::Value>, _>("deprecation")
            .and_then(|v| serde_json::from_value(v).ok()),
        version: EntityVersion {
            major: u32::try_from(row.get::<i32, _>("version_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("version_minor")).unwrap_or(1),
        },
        updated_by: row.get("updated_by"),
        change_description: row
            .get::<Option<serde_json::Value>, _>("change_description")
            .and_then(|v| serde_json::from_value(v).ok()),
        deleted: row.get("deleted"),
        deleted_at: row.get("deleted_at"),
        id: row.get("id"),
        kind: AssetKind::parse(row.get::<&str, _>("kind")).unwrap_or(AssetKind::Table),
        // `{}` on the wire is indistinguishable from absent, and both mean
        // "this entity holds no organization-defined values" — so an empty
        // bag is normalised to `None` rather than serialized as noise on
        // every asset in every list response.
        extension: row
            .get::<Option<serde_json::Value>, _>("extension")
            .and_then(|value| match value {
                serde_json::Value::Object(map) if !map.is_empty() => Some(map),
                _ => None,
            }),
        name: row.get("name"),
        fully_qualified_name: row.get("fully_qualified_name"),
        parent_id: row.get("parent_id"),
        description: row.get("description"),
        properties: row.get("properties"),
        // `try_get`, because the two `RETURNING` paths do not carry this column —
        // a correlated subquery in `RETURNING` cannot see `assets` under that
        // alias. Those paths read owners separately rather than silently
        // reporting none.
        owners: row
            .try_get::<serde_json::Value, _>("owners")
            .ok()
            .and_then(|raw| serde_json::from_value(raw).ok())
            .unwrap_or_default(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn merge_record_from_row(
    row: PgRow,
) -> Result<graph_owl_core::resolution::MergeRecord, StorageError> {
    Ok(graph_owl_core::resolution::MergeRecord {
        id: row.get("id"),
        canonical: row.get("canonical_id"),
        merged: row.get("merged_id"),
        evidence: serde_json::from_value(row.get("evidence"))
            .map_err(|e| StorageError::Unexpected(e.to_string()))?,
        confidence: row.get("confidence"),
        decided_by: serde_json::from_value(row.get("decided_by"))
            .map_err(|e| StorageError::Unexpected(e.to_string()))?,
        decided_at: row.get("decided_at"),
        merged_at_t: row.get("merged_at_t"),
        split_at: row.get("split_at"),
    })
}

/// The wire spelling of a review status.
///
/// A `match` rather than a `Serialize` round-trip, matching
/// [`memory_kind_str`]: the column has a `CHECK` listing these exact
/// strings, so a rename that forgets the migration fails to compile rather
/// than fails at 3am on the first write.
const fn review_status_str(status: graph_owl_core::resolution::ReviewStatus) -> &'static str {
    use graph_owl_core::resolution::ReviewStatus;
    match status {
        ReviewStatus::Pending => "pending",
        ReviewStatus::Confirmed => "confirmed",
        ReviewStatus::Rejected => "rejected",
    }
}

fn review_status_from_str(
    value: &str,
) -> Result<graph_owl_core::resolution::ReviewStatus, StorageError> {
    use graph_owl_core::resolution::ReviewStatus;
    match value {
        "pending" => Ok(ReviewStatus::Pending),
        "confirmed" => Ok(ReviewStatus::Confirmed),
        "rejected" => Ok(ReviewStatus::Rejected),
        other => Err(StorageError::Unexpected(format!(
            "unknown review status '{other}' in resolution_queue"
        ))),
    }
}

/// The wire spelling of a drift kind — matches the `CHECK` on `drift_reports.kind`.
const fn drift_kind_str(kind: graph_owl_core::drift::DriftKind) -> &'static str {
    use graph_owl_core::drift::DriftKind;
    match kind {
        DriftKind::LiveEdited => "live_edited",
        DriftKind::Unapplied => "unapplied",
    }
}

fn drift_kind_from_str(value: &str) -> Result<graph_owl_core::drift::DriftKind, StorageError> {
    use graph_owl_core::drift::DriftKind;
    match value {
        "live_edited" => Ok(DriftKind::LiveEdited),
        "unapplied" => Ok(DriftKind::Unapplied),
        other => Err(StorageError::Unexpected(format!(
            "unknown drift kind '{other}' in drift_reports"
        ))),
    }
}

/// The wire spelling of a drift status — matches the `CHECK` on `drift_reports.status`.
const fn drift_status_str(status: graph_owl_core::drift::DriftStatus) -> &'static str {
    use graph_owl_core::drift::DriftStatus;
    match status {
        DriftStatus::Pending => "pending",
        DriftStatus::Applied => "applied",
        DriftStatus::Ignored => "ignored",
    }
}

fn drift_status_from_str(value: &str) -> Result<graph_owl_core::drift::DriftStatus, StorageError> {
    use graph_owl_core::drift::DriftStatus;
    match value {
        "pending" => Ok(DriftStatus::Pending),
        "applied" => Ok(DriftStatus::Applied),
        "ignored" => Ok(DriftStatus::Ignored),
        other => Err(StorageError::Unexpected(format!(
            "unknown drift status '{other}' in drift_reports"
        ))),
    }
}

/// Requires the row to carry `fully_qualified_name` alongside `drift_reports`'
/// own columns — every caller joins `assets` for it, since the name is
/// denormalized at read time rather than stored redundantly.
fn drift_item_from_row(row: PgRow) -> Result<graph_owl_core::drift::DriftItem, StorageError> {
    Ok(graph_owl_core::drift::DriftItem {
        id: row.get("id"),
        asset_id: row.get("asset_id"),
        fully_qualified_name: row.get("fully_qualified_name"),
        field: row.get("field"),
        kind: drift_kind_from_str(row.get::<&str, _>("kind"))?,
        live_value: row.get("live_value"),
        declared_value: row.get("declared_value"),
        status: drift_status_from_str(row.get::<&str, _>("status"))?,
        reported_at: row.get("reported_at"),
        decided_at: row.get("decided_at"),
        decided_by: row.get("decided_by"),
        reason: row.get("reason"),
    })
}

/// Splits a `SignatureScheme` into the three columns it is stored across.
/// `prefix` is `None` for `Ed25519`, which carries no such label.
fn scheme_columns(
    scheme: &graph_owl_storage::SignatureScheme,
) -> (&'static str, &str, Option<&str>) {
    use graph_owl_storage::SignatureScheme;
    match scheme {
        SignatureScheme::HmacSha256 { header, prefix } => {
            ("hmac_sha256", header.as_str(), Some(prefix.as_str()))
        }
        SignatureScheme::Ed25519 { header } => ("ed25519", header.as_str(), None),
    }
}

fn webhook_endpoint_from_row(row: PgRow) -> graph_owl_storage::WebhookEndpoint {
    use graph_owl_storage::SignatureScheme;
    let scheme: &str = row.get("scheme");
    let header: String = row.get("scheme_header");
    let signature_scheme = if scheme == "ed25519" {
        SignatureScheme::Ed25519 { header }
    } else {
        SignatureScheme::HmacSha256 {
            header,
            prefix: row
                .get::<Option<String>, _>("scheme_prefix")
                .unwrap_or_default(),
        }
    };
    graph_owl_storage::WebhookEndpoint {
        id: row.get("id"),
        path: row.get("path"),
        source: row.get("source"),
        signature_scheme,
        mapping: row.get("mapping"),
        event_filter: row.get("event_filter"),
        enabled: row.get("enabled"),
        has_secret: row.get("has_secret"),
        rate_limit_per_minute: row
            .get::<Option<i32>, _>("rate_limit_per_minute")
            .map(|n| u32::try_from(n).unwrap_or(0)),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn broker_columns(broker: &graph_owl_storage::BrokerConfig) -> (&'static str, &str, Option<&str>) {
    use graph_owl_storage::BrokerConfig;
    match broker {
        BrokerConfig::KafkaProtocol { bootstrap_servers } => {
            ("kafka_protocol", bootstrap_servers.as_str(), None)
        }
        BrokerConfig::Pulsar {
            service_url,
            admin_url,
        } => ("pulsar", service_url.as_str(), admin_url.as_deref()),
    }
}

fn broker_config_from_columns(
    kind: &str,
    address: String,
    admin_url: Option<String>,
) -> graph_owl_storage::BrokerConfig {
    use graph_owl_storage::BrokerConfig;
    if kind == "pulsar" {
        BrokerConfig::Pulsar {
            service_url: address,
            admin_url,
        }
    } else {
        BrokerConfig::KafkaProtocol {
            bootstrap_servers: address,
        }
    }
}

fn start_position_columns(
    position: graph_owl_storage::StartPosition,
) -> (
    &'static str,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<i64>,
) {
    use graph_owl_storage::StartPosition;
    match position {
        StartPosition::Earliest => ("earliest", None, None),
        StartPosition::Latest => ("latest", None, None),
        StartPosition::Timestamp { at } => ("timestamp", Some(at), None),
        StartPosition::Offset { value } => ("offset", None, Some(value)),
    }
}

fn start_position_from_row(row: &PgRow) -> graph_owl_storage::StartPosition {
    use graph_owl_storage::StartPosition;
    let kind: &str = row.get("start_position");
    match kind {
        "latest" => StartPosition::Latest,
        "timestamp" => StartPosition::Timestamp {
            at: row.get("start_timestamp"),
        },
        "offset" => StartPosition::Offset {
            value: row.get("start_offset"),
        },
        _ => StartPosition::Earliest,
    }
}

fn stream_subscription_from_row(row: PgRow) -> graph_owl_storage::StreamSubscription {
    let broker_kind: String = row.get("broker_kind");
    let broker_address: String = row.get("broker_address");
    let broker_admin_url: Option<String> = row.get("broker_admin_url");
    let start_position = start_position_from_row(&row);
    graph_owl_storage::StreamSubscription {
        id: row.get("id"),
        broker: broker_config_from_columns(&broker_kind, broker_address, broker_admin_url),
        topic: row.get("topic"),
        consumer_group: row.get("consumer_group"),
        mapping: row.get("mapping"),
        start_position,
        max_in_flight: row
            .get::<i32, _>("max_in_flight")
            .try_into()
            .unwrap_or(usize::MAX),
        poison_threshold: row
            .get::<i32, _>("poison_threshold")
            .try_into()
            .unwrap_or(u32::MAX),
        has_secret: row.get("has_secret"),
        enabled: row.get("enabled"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn stream_dead_letter_from_row(row: &PgRow) -> graph_owl_storage::StreamDeadLetter {
    graph_owl_storage::StreamDeadLetter {
        id: row.get("id"),
        subscription_id: row.get("subscription_id"),
        topic: row.get("topic"),
        partition: row.get("partition"),
        offset: row.get("kafka_offset"),
        payload: row.get("payload"),
        reason: row.get("reason"),
        created_at: row.get("created_at"),
    }
}

const fn event_state_str(state: graph_owl_core::webhook::EventState) -> &'static str {
    use graph_owl_core::webhook::EventState;
    match state {
        EventState::Received => "received",
        EventState::Mapped => "mapped",
        EventState::Applied => "applied",
        EventState::Failed => "failed",
        EventState::Duplicate => "duplicate",
        EventState::Superseded => "superseded",
    }
}

fn event_state_from_str(value: &str) -> Result<graph_owl_core::webhook::EventState, StorageError> {
    use graph_owl_core::webhook::EventState;
    match value {
        "received" => Ok(EventState::Received),
        "mapped" => Ok(EventState::Mapped),
        "applied" => Ok(EventState::Applied),
        "failed" => Ok(EventState::Failed),
        "duplicate" => Ok(EventState::Duplicate),
        "superseded" => Ok(EventState::Superseded),
        other => Err(StorageError::Unexpected(format!(
            "unknown event state '{other}' in inbound_events"
        ))),
    }
}

fn inbound_event_from_row(
    row: PgRow,
) -> Result<graph_owl_core::webhook::InboundEvent, StorageError> {
    Ok(graph_owl_core::webhook::InboundEvent {
        id: row.get("id"),
        endpoint: row.get("endpoint_id"),
        sender_event_id: row.get("sender_event_id"),
        sender_timestamp: row.get("sender_timestamp"),
        received_at: row.get("received_at"),
        raw: row.get("raw"),
        state: event_state_from_str(row.get::<&str, _>("state"))?,
        dedup_key: row.get("dedup_key"),
        reason: row.get("reason"),
    })
}

fn mapping_from_row(row: PgRow) -> Result<graph_owl_storage::Mapping, StorageError> {
    let kind = serde_json::from_value(row.get("kind_expr"))
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    let entity_name = serde_json::from_value(row.get("name_expr"))
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    let parent_fqn = row
        .get::<Option<serde_json::Value>, _>("parent_fqn_expr")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    let description = row
        .get::<Option<serde_json::Value>, _>("description_expr")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;
    let properties = serde_json::from_value(row.get("properties_exprs"))
        .map_err(|e| StorageError::Unexpected(e.to_string()))?;

    Ok(graph_owl_storage::Mapping {
        name: row.get("name"),
        version: {
            let version: i32 = row.get("version");
            u32::try_from(version).map_err(|e| StorageError::Unexpected(e.to_string()))?
        },
        kind,
        entity_name,
        parent_fqn,
        description,
        properties,
        created_at: row.get("created_at"),
    })
}

fn review_queue_entry_from_row(
    row: PgRow,
) -> Result<graph_owl_core::resolution::ReviewQueueEntry, StorageError> {
    Ok(graph_owl_core::resolution::ReviewQueueEntry {
        id: row.get("id"),
        target: row.get("target_id"),
        candidate: row.get("candidate_id"),
        score: row.get("score"),
        evidence: serde_json::from_value(row.get("evidence"))
            .map_err(|e| StorageError::Unexpected(e.to_string()))?,
        status: review_status_from_str(row.get::<&str, _>("status"))?,
        decided_by: row
            .get::<Option<serde_json::Value>, _>("decided_by")
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| StorageError::Unexpected(e.to_string()))?,
        decided_at: row.get("decided_at"),
        reason: row.get("reason"),
        created_at: row.get("created_at"),
    })
}

fn mention_resolution_from_row(row: PgRow) -> graph_owl_core::resolution::MentionResolution {
    graph_owl_core::resolution::MentionResolution {
        id: row.get("id"),
        source: row.get("source_id"),
        text: row.get("text"),
        entity: row.get("entity_id"),
        confidence: row.get("confidence"),
        resolved_at: row.get("resolved_at"),
    }
}

/// Effective owners as a JSON array, aggregated **in SQL**.
///
/// A correlated subquery rather than a join, because a join multiplies asset rows
/// by owner count and every caller would then have to de-duplicate — and rather
/// than a second query per asset, because a list of 200 assets would become 201
/// round trips against Docker's mapped port at ~30ms each.
///
/// **Effective, not direct** (Epic 11 Slice D). An asset with no owner of its own
/// reports the nearest owned ancestor's owners, flagged `inherited`. The walk is
/// a recursive CTE per row; containment is at most five levels deep
/// (service → database → schema → table → column), so the recursion is bounded by
/// the domain rather than by a limit anybody has to remember to set.
///
/// `ORDER BY hops LIMIT 1` is what makes inheritance **stop at the nearest owned
/// ancestor** rather than accumulate up the chain: "who do I ask about this
/// table" has one answer, and a list that grows with tree depth answers "who
/// might conceivably care" instead.
///
/// `coalesce(..., '[]')` so an unowned asset yields an empty array rather than
/// `NULL`: the domain's `owners` is always a list, and the two must agree or the
/// version classifier sees a field appear and disappear.
///
/// Display names are joined here so a renamed team reads correctly everywhere,
/// and fall back to the id — an owner row can only exist for a live principal
/// (both columns are foreign keys), so the fallback is unreachable defence rather
/// than a real case.
const OWNERS_EXPR: &str = "(WITH RECURSIVE ancestry (node, next_up, hops) AS (
            SELECT seed.id, seed.parent_id, 0 FROM assets seed WHERE seed.id = assets.id
        UNION ALL
            SELECT up.id, up.parent_id, ancestry.hops + 1
              FROM assets up JOIN ancestry ON up.id = ancestry.next_up
    ),
    nearest AS (
        SELECT ancestry.node, ancestry.hops
          FROM ancestry
         WHERE EXISTS (SELECT 1 FROM asset_owners o WHERE o.asset_id = ancestry.node)
         ORDER BY ancestry.hops
         LIMIT 1
    )
    SELECT coalesce(json_agg(json_build_object(
        'id',          coalesce(o.user_id, o.team_id),
        'kind',        CASE WHEN o.user_id IS NOT NULL THEN 'user' ELSE 'team' END,
        'displayName', coalesce(u.display_name, t.display_name, o.user_id, o.team_id),
        'inherited',   nearest.hops > 0
    ) ORDER BY o.ordinal), '[]'::json)
    FROM nearest
    JOIN asset_owners o ON o.asset_id = nearest.node
    LEFT JOIN users u ON u.id = o.user_id
    LEFT JOIN teams t ON t.id = o.team_id)";

const ASSET_COLUMNS: &str = "id, kind, name, fully_qualified_name, parent_id, description, properties, extension, lifecycle, deprecation, version_major, version_minor, updated_by, change_description, deleted, deleted_at, created_at, updated_at";

/// `AND` clauses for a set of custom-property filters — Epic 22 Slice D.
///
/// Two placeholders per condition (the property name, then the value), starting
/// at `next`. **The name is bound, never interpolated**: it arrives from a query
/// string, and a name spliced into SQL is an injection whatever the facade
/// checked first.
///
/// Equality is written as **containment** (`@>`), not as `extension -> name =
/// value`. The two are equivalent here and only one is indexable: `jsonb_path_ops`
/// — the operator class the `assets_extension` index uses — supports `@>` and
/// nothing else. Written the other way this is a sequential scan of the whole
/// table on the most common filter there is.
///
/// The bounds are not index-backed, and cannot be by a general index: a btree on
/// `(extension -> 'retentionDays')` supports one property, so a generic range
/// index would mean an index per definition — which is precisely the per-property
/// migration this epic's decision 4 refuses. They run as a filter over whatever
/// the indexable predicates (`kind`, visibility, any equality filter) already
/// narrowed to. When one property becomes hot enough to matter, an expression
/// index on that one property is the escape hatch, and it needs no code change.
fn extension_clauses(filters: &[graph_owl_storage::ExtensionFilter], next: usize) -> String {
    use graph_owl_storage::ExtensionOp;
    let mut sql = String::new();
    for (offset, condition) in filters.iter().enumerate() {
        let name = next + offset * 2;
        let value = name + 1;
        let clause = match condition.op {
            ExtensionOp::Eq => {
                format!(" AND extension @> jsonb_build_object(${name}::text, ${value}::jsonb)")
            }
            // jsonb compares numbers numerically and strings lexicographically,
            // and a property has one declared type — so one expression serves
            // both the numeric and the date/timestamp criteria, ISO-8601 dates
            // ordering correctly as strings.
            ExtensionOp::Gte => format!(" AND (extension -> ${name}::text) >= ${value}::jsonb"),
            ExtensionOp::Lte => format!(" AND (extension -> ${name}::text) <= ${value}::jsonb"),
        };
        sql.push_str(&clause);
    }
    sql
}

/// [`ASSET_COLUMNS`] with every name qualified by a table alias.
///
/// **Derived rather than written out a second time.** A recursive CTE's two
/// branches must project identical column counts, and the recursive half needs
/// its columns qualified while the non-recursive half does not — so the
/// obvious thing is to hand-write the qualified list. That copy then drifts the
/// moment a column is added to the envelope, and Postgres rejects the `UNION
/// ALL` with an error naming neither the column nor the caller: every
/// `/ancestors` request becomes a 500. That has now happened twice, which makes
/// it a property of hand-copying rather than a slip.
fn asset_columns_as(alias: &str) -> String {
    ASSET_COLUMNS
        .split(", ")
        .map(|column| format!("{alias}.{column}"))
        .collect::<Vec<_>>()
        .join(", ")
}
/// The exact query `resolution_candidates` runs, built once so `EXPLAIN`ing
/// it (Epic 17 Slice B's index-scan acceptance criterion) explains the real
/// query rather than a lookalike — the same reasoning
/// `PostgresTripleStore::explain` documents for the engine's own plan
/// assertions.
fn resolution_candidates_sql() -> String {
    format!(
        "SELECT {ASSET_COLUMNS}, {OWNERS_EXPR} AS owners FROM assets
         WHERE NOT deleted
           AND id IN (
               SELECT DISTINCT ebk2.asset_id
               FROM entity_blocking_keys ebk1
               JOIN entity_blocking_keys ebk2
                 ON ebk2.key_type = ebk1.key_type AND ebk2.key_value = ebk1.key_value
               WHERE ebk1.asset_id = $1
                 AND ebk2.asset_id <> $1
                 AND ebk1.key_value <> ''
           )"
    )
}

impl PostgresStorage {
    /// The query plan `resolution_candidates` would use for `asset_id`, as
    /// plain text.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::Unexpected` if the plan cannot be produced.
    pub async fn explain_resolution_candidates(
        &self,
        asset_id: Uuid,
    ) -> Result<String, StorageError> {
        let rows = sqlx::query(&format!("EXPLAIN {}", resolution_candidates_sql()))
            .bind(asset_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|row| row.get::<String, _>(0))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    async fn asset_page(
        &self,
        query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let assets: Vec<Asset> = rows.into_iter().map(asset_from_row).collect();
        Ok(Page::from_overfetch(assets, page.limit, |asset| {
            Cursor::new(asset.fully_qualified_name.clone(), asset.id)
        }))
    }

    /// A relevance-ordered page, whose cursor is the rank key the query
    /// computed rather than the FQN.
    ///
    /// Separate from [`Self::asset_page`] because the cursor has to reproduce
    /// the ordering it came from. Reusing the FQN cursor here would page
    /// through a relevance-ordered result as though it were alphabetical, and
    /// the second page would silently skip and repeat rows.
    ///
    /// [`Self::asset_page`]: Self::asset_page
    async fn ranked_asset_page(
        &self,
        query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError> {
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let ranked: Vec<(Asset, String)> = rows
            .into_iter()
            .map(|row| {
                let key: String = row.get("sort_key");
                (asset_from_row(row), key)
            })
            .collect();
        let page = Page::from_overfetch(ranked, page.limit, |(asset, key)| {
            Cursor::new(key.clone(), asset.id)
        });
        Ok(Page {
            data: page.data.into_iter().map(|(asset, _)| asset).collect(),
            paging: page.paging,
        })
    }

    /// [`Self::ranked_asset_page`], for a query that also carries a snippet
    /// per row — kept separate rather than made generic, because the two
    /// only diverge in what one extra column the closure reads before
    /// [`asset_from_row`] consumes the [`PgRow`], and a generic over "what
    /// extra field" would cost more than it would save at two call sites.
    async fn ranked_search_hit_page(
        &self,
        query: sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>,
        page: &PageRequest,
    ) -> Result<Page<SearchHit>, StorageError> {
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;
        let ranked: Vec<(SearchHit, String)> = rows
            .into_iter()
            .map(|row| {
                let key: String = row.get("sort_key");
                let snippet: Option<String> = row.get("snippet");
                let asset = asset_from_row(row);
                (SearchHit { asset, snippet }, key)
            })
            .collect();
        let page = Page::from_overfetch(ranked, page.limit, |(hit, key)| {
            Cursor::new(key.clone(), hit.asset.id)
        });
        Ok(Page {
            data: page.data.into_iter().map(|(hit, _)| hit).collect(),
            paging: page.paging,
        })
    }

    /// A query with no searchable terms matches nothing.
    ///
    /// `to_tsquery('english', '')` raises a syntax error rather than returning
    /// a query that matches nothing, so an all-punctuation search has to be
    /// answered without asking Postgres. An empty result, not an error: the
    /// user typed something unusable, which is not a fault to report.
    fn empty_ranked_page(page: &PageRequest) -> Page<Asset> {
        Page::from_overfetch(Vec::new(), page.limit, |a: &Asset| {
            Cursor::new(a.fully_qualified_name.clone(), a.id)
        })
    }

    /// [`Self::empty_ranked_page`], for [`Self::search_assets_visible`]'s
    /// snippet-carrying result type.
    fn empty_search_hit_page(page: &PageRequest) -> Page<SearchHit> {
        Page::from_overfetch(Vec::new(), page.limit, |h: &SearchHit| {
            Cursor::new(h.asset.fully_qualified_name.clone(), h.asset.id)
        })
    }
}

// ---- Epic 32 row hydration ----

/// A capability name from the database back into the closed enum.
///
/// **`None` for anything unrecognised, and the caller drops it.** A capability
/// row that no longer maps to a variant is one that was *removed* — which is the
/// direction this set is expected to move — and the safe reading of a name this
/// build does not know is "not granted". Mapping it to a default would grant
/// something nobody asked for.
fn capability_from_str(name: &str) -> Option<graph_owl_authz::agent::AgentCapability> {
    graph_owl_authz::agent::AgentCapability::ALL
        .into_iter()
        .find(|capability| capability.as_str() == name)
}

fn proposal_status_str(status: graph_owl_authz::agent::ProposalStatus) -> &'static str {
    use graph_owl_authz::agent::ProposalStatus;
    match status {
        ProposalStatus::Open => "open",
        ProposalStatus::Accepted => "accepted",
        ProposalStatus::Rejected => "rejected",
        ProposalStatus::Superseded => "superseded",
    }
}

/// **An unrecognised status reads as `Superseded`, never as `Open`.**
///
/// `Open` would put a row nobody can act on into the reviewer's queue forever;
/// `Accepted` would claim a decision nobody made. `Superseded` is the one
/// variant that asserts nothing about what a human decided — it says the row
/// stopped being actionable, which is exactly what an unreadable status means.
fn proposal_status_from_str(name: &str) -> graph_owl_authz::agent::ProposalStatus {
    use graph_owl_authz::agent::ProposalStatus;
    match name {
        "open" => ProposalStatus::Open,
        "accepted" => ProposalStatus::Accepted,
        "rejected" => ProposalStatus::Rejected,
        _ => ProposalStatus::Superseded,
    }
}

fn activity_outcome_str(outcome: graph_owl_authz::agent::ActivityOutcome) -> &'static str {
    use graph_owl_authz::agent::ActivityOutcome;
    match outcome {
        ActivityOutcome::Applied => "applied",
        ActivityOutcome::Proposed => "proposed",
        ActivityOutcome::Refused => "refused",
    }
}

/// **An unrecognised outcome reads as `Refused`.**
///
/// The audit exists to show what an agent actually managed to do. Reading an
/// unknown row as `Applied` would credit it with a write nobody can confirm;
/// `Refused` under-claims, which is the safe direction for an audit log.
fn activity_outcome_from_str(name: &str) -> graph_owl_authz::agent::ActivityOutcome {
    use graph_owl_authz::agent::ActivityOutcome;
    match name {
        "applied" => ActivityOutcome::Applied,
        "proposed" => ActivityOutcome::Proposed,
        _ => ActivityOutcome::Refused,
    }
}

fn agent_grant_from_row(row: &PgRow) -> graph_owl_authz::agent::AgentGrant {
    let names: Vec<String> = row.try_get("capabilities").unwrap_or_default();
    graph_owl_authz::agent::AgentGrant {
        id: row.get("id"),
        agent: graph_owl_core::ownership::EntityReference {
            id: row.get("agent_id"),
            // An agent is a principal with `is_bot`, which is a `users` row —
            // so a bot is a `User` here, not a `Team`. The distinction that
            // matters for attribution is *which* principal, not its kind.
            kind: graph_owl_core::ownership::OwnerKind::User,
            display_name: row.get("display_name"),
            inherited: false,
        },
        capabilities: names
            .iter()
            .filter_map(|name| capability_from_str(name))
            .collect(),
        scope: row
            .get::<Option<String>, _>("scope_fqn_prefix")
            .map(|fqn_prefix| graph_owl_authz::agent::ScopeRef { fqn_prefix }),
        rate_limit: graph_owl_authz::agent::RateLimit {
            max_writes: u32::try_from(row.get::<i32, _>("max_writes")).unwrap_or(0),
            window_seconds: u32::try_from(row.get::<i32, _>("window_seconds")).unwrap_or(0),
        },
        expires_at: row.get("expires_at"),
        granted_by: row.get("granted_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn proposal_from_row(row: &PgRow) -> graph_owl_authz::agent::Proposal {
    graph_owl_authz::agent::Proposal {
        id: row.get("id"),
        proposed_by: graph_owl_core::ownership::EntityReference {
            id: row.get("agent_id"),
            kind: graph_owl_core::ownership::OwnerKind::User,
            display_name: row.get("display_name"),
            inherited: false,
        },
        target_fqn: row.get("target_fqn"),
        // A proposal whose capability this build no longer knows is not
        // actionable, and `LinkLineage` is the most conservative stand-in: it
        // always proposes and never applies, so nothing can be applied from a
        // row that could not be read.
        capability: capability_from_str(&row.get::<String, _>("capability"))
            .unwrap_or(graph_owl_authz::agent::AgentCapability::LinkLineage),
        change: row.get("change"),
        rationale: row.get("rationale"),
        confidence: row.get("confidence"),
        status: proposal_status_from_str(&row.get::<String, _>("status")),
        base_version: graph_owl_core::envelope::EntityVersion {
            major: u32::try_from(row.get::<i32, _>("base_major")).unwrap_or(0),
            minor: u32::try_from(row.get::<i32, _>("base_minor")).unwrap_or(0),
        },
        decided_by: row.get("decided_by"),
        decided_at: row.get("decided_at"),
        created_at: row.get("created_at"),
    }
}

fn activity_from_row(row: &PgRow) -> graph_owl_authz::agent::AgentActivity {
    graph_owl_authz::agent::AgentActivity {
        id: row.get("id"),
        agent_id: row.get("agent_id"),
        capability: capability_from_str(&row.get::<String, _>("capability"))
            .unwrap_or(graph_owl_authz::agent::AgentCapability::LinkLineage),
        target_fqn: row.get("target_fqn"),
        outcome: activity_outcome_from_str(&row.get::<String, _>("outcome")),
        refusal: row.get("refusal"),
        at: row.get("at"),
    }
}

#[cfg(test)]
mod extension_clause_tests {
    use super::extension_clauses;
    use graph_owl_storage::{ExtensionFilter, ExtensionOp};

    fn filter(name: &str, op: ExtensionOp) -> ExtensionFilter {
        ExtensionFilter {
            name: name.to_string(),
            op,
            value: serde_json::json!("x"),
        }
    }

    /// **The whole reason equality is written this way.** `jsonb_path_ops`
    /// supports `@>` and nothing else, so `extension -> name = value` is a
    /// sequential scan of the table on the most common filter there is — and
    /// both spellings return the same rows, so only this assertion can tell
    /// them apart.
    #[test]
    fn equality_is_containment_so_the_gin_index_can_serve_it() {
        let sql = extension_clauses(&[filter("costCenter", ExtensionOp::Eq)], 9);

        assert!(sql.contains("@>"), "{sql}");
        assert!(
            !sql.contains("extension -> $9"),
            "a direct comparison is unindexable here: {sql}"
        );
    }

    /// Placeholders advance by two per condition — one for the name, one for
    /// the value. Off by one and every filter past the first binds the previous
    /// filter's value, which returns wrong rows rather than failing.
    #[test]
    fn each_condition_consumes_two_placeholders_from_the_offset_given() {
        let sql = extension_clauses(
            &[
                filter("costCenter", ExtensionOp::Eq),
                filter("retentionDays", ExtensionOp::Gte),
            ],
            9,
        );

        assert!(sql.contains("$9"), "{sql}");
        assert!(sql.contains("$10"), "{sql}");
        assert!(sql.contains("$11"), "{sql}");
        assert!(sql.contains("$12"), "{sql}");
        assert!(!sql.contains("$13"), "{sql}");
    }

    /// The two bounds are different comparisons, and swapping them is a silent
    /// wrong answer rather than an error.
    #[test]
    fn the_bounds_compare_in_the_directions_they_are_named_for() {
        assert!(
            extension_clauses(&[filter("d", ExtensionOp::Gte)], 1).contains(">="),
            "gte must be >="
        );
        assert!(
            extension_clauses(&[filter("d", ExtensionOp::Lte)], 1).contains("<="),
            "lte must be <="
        );
    }

    /// **The property name is never interpolated.** It arrives from a query
    /// string, and a name spliced into SQL is an injection whatever the facade
    /// checked first.
    #[test]
    fn the_property_name_is_bound_not_written_into_the_sql() {
        let sql = extension_clauses(&[filter("'; DROP TABLE assets; --", ExtensionOp::Eq)], 9);

        assert!(!sql.contains("DROP TABLE"), "{sql}");
    }

    /// No filters, no clauses — an empty string, not a dangling `AND`.
    #[test]
    fn no_filters_produce_no_sql() {
        assert!(extension_clauses(&[], 9).is_empty());
    }
}
