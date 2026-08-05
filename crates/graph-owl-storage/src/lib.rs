use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    classification::{Classification, LabelState, LabelType, Tag, TagLabel},
    contract::{Contract, ContractBreach, ContractStatus, SchemaChange},
    contradiction::Review,
    custom_property::CustomProperty,
    domain::{DataProduct, Domain, DomainAssignment},
    lifecycle::{Deprecation, LifecycleState},
    memory::Memory,
    ownership::{EntityReference, OwnerRef},
    page::{Page, PageRequest},
    quality::TestStatus,
    usage::{Consumer, UsageOperation, UsageRollup},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A user as stored. Distinct from `Principal`, which is the request-scoped
/// view: this is the record, that is the claim about who is asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredUser {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub is_admin: bool,
    pub is_bot: bool,
    pub roles: Vec<String>,
}
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// A `fully_qualified_name` already exists.
    Fqn,
    /// The `(from, type, to)` relationship tuple already exists.
    RelationshipTuple,
    /// This finding is already waived. A second waiver would hide which reason
    /// is the live one, and "why is this accepted" is the question the record
    /// exists to answer.
    WaiverExists,
    /// This finding is already assigned. Two owners is no owner.
    AssignmentExists,
    /// An `Idempotency-Key` was reused for different content, or is in flight —
    /// Epic 16 Slice B.
    ///
    /// **The third variant added because the detail would otherwise be eaten.**
    /// `AppError`'s renderer returns a fixed sentence per `ConflictKind`, so
    /// borrowing an existing variant silently replaces whatever the facade wrote.
    /// That has now happened twice — Slice G lost its counts, this lost the key —
    /// which makes it a property of the design rather than a slip: **a conflict
    /// carrying its own detail needs its own kind.**
    IdempotencyConflict,
    /// A tag is still applied, or a classification still has tags — Epic 25.
    ///
    /// Its own kind because the response carries **counts by entity kind**, and
    /// a borrowed kind's canned sentence would replace them. That has now
    /// happened twice in this codebase, which makes it a property of the design
    /// rather than a slip.
    TagInUse,
    /// Another tag from the same mutually-exclusive classification is present —
    /// Epic 25 decision 4. Separate from `TagInUse` because the fix is
    /// different: one says remove the other tag, the other says the tag itself
    /// cannot go yet.
    TagExclusive,
    /// An asset is already assigned to a different domain — Epic 23 Slice B.
    ///
    /// Its own kind because the response has to name the *current* domain, and
    /// the server's canned sentence for a borrowed kind would replace it. That
    /// has now happened twice in this codebase, which makes it a property of the
    /// design: a conflict carrying its own detail needs its own kind.
    DomainAssigned,
    /// A domain still holds assets, products or child domains — Epic 23 Slice F.
    DomainInUse,
    /// An agent proposal was already decided — Epic 32 Slice B.
    ///
    /// Its own kind, following this enum's standing rule: the response has to
    /// name **which** decision already happened and by whom, and a borrowed
    /// kind's canned sentence would replace that. Deciding twice is a conflict
    /// rather than an update because two reviewers reaching opposite
    /// conclusions must not have the second silently win.
    ProposalDecided,
    /// This principal still owns assets or parents teams — Epic 11 Slice G.
    ///
    /// Its own variant because the response has to carry *counts by kind*, and
    /// reusing `AssignmentExists` meant the server's canned message for that kind
    /// replaced them: "this finding is already assigned" told a steward nothing
    /// about the 400 columns they were about to strand. Found by an HTTP test.
    PrincipalStillHolds,
    /// A memory with this id already exists. Its own variant rather than reusing
    /// one above, because this enum's whole purpose is that a client can act on
    /// the collision — and "your memory id collided" needs a different response
    /// from "that name is taken".
    MemoryExists,
    /// A glossary still has terms and the caller did not ask for a recursive
    /// delete — Epic 24 Slice A. Its own variant for the same reason
    /// `PrincipalStillHolds` has one: the term count is the actionable detail,
    /// and a canned per-kind sentence would eat it the way it twice ate
    /// others before this enum grew a rule about it.
    GlossaryHasTerms,
    /// A merge was already split — Epic 17 Slice E's `409`. Its own variant
    /// for the same reason every other conflict-with-detail here has one:
    /// the original split time is the actionable detail, and reusing a
    /// generic kind would eat it.
    MergeAlreadySplit,
    /// A review-queue entry was already confirmed or rejected — Epic 17
    /// Slice F. Its own variant so a second confirm cannot double-merge and
    /// a reject-after-confirm cannot silently do nothing.
    ReviewAlreadyDecided,
    /// A webhook endpoint's `path` is already registered to a different
    /// endpoint — Epic 18 Slice A. Its own variant because the path itself
    /// is the actionable detail: "conflict" alone does not tell a caller
    /// which URL segment to pick instead.
    WebhookPathExists,
    /// A `(topic, consumer_group)` pair is already registered as a different
    /// subscription — Epic 19 Slice A. Its own variant for the same reason
    /// as `WebhookPathExists`: the pair is the actionable detail a caller
    /// needs back, not a generic "conflict".
    StreamSubscriptionExists,
    /// A custom property with this name is already defined **on this entity
    /// type** — Epic 22 Slice A. Its own variant for the reason this enum has
    /// a rule about: the actionable detail is the *pair*, because the same
    /// name on a different type is allowed and a caller told only "conflict"
    /// cannot tell which of the two it needs to change.
    CustomPropertyExists,
}

#[derive(Debug, Error)]
pub enum StorageError {
    /// A uniqueness constraint rejected the write. `existing_id` names the row
    /// that was already there when the adapter can identify it, so a client can
    /// act on the collision instead of guessing what it hit.
    #[error("conflict: {detail}")]
    Conflict {
        detail: String,
        existing_id: Option<Uuid>,
        /// What kind of uniqueness was violated. A duplicate FQN and a
        /// duplicate relationship tuple need different client responses, so
        /// they must not share one error identity.
        kind: ConflictKind,
    },
    #[error("unexpected storage error: {0}")]
    Unexpected(String),
}

/// What an update did.
///
/// Three outcomes, not two: "the asset is gone" and "someone else edited it
/// first" need different fixes from the caller, and collapsing them into
/// `None` would make a lost update look like a deleted asset.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateOutcome {
    Updated(Box<Asset>),
    NotFound,
    /// The guard did not match. Carries what the version actually is, so the
    /// caller can show the reader what they were about to overwrite.
    VersionMismatch(EntityVersion),
}

/// What splitting a merge did (Epic 17 Slice E).
///
/// `AlreadySplit` is distinct from a plain error: splitting twice is a
/// **client** mistake with a specific fix (`409`, per the plan's own
/// acceptance criterion), not a backend failure, and it carries the original
/// `split_at` so the caller can report when the merge was undone rather than
/// just that it was.
#[derive(Debug, Clone, PartialEq)]
pub enum SplitOutcome {
    Split(Box<graph_owl_core::resolution::MergeRecord>),
    NotFound,
    AlreadySplit {
        split_at: chrono::DateTime<chrono::Utc>,
    },
}

/// What saving a memory did.
///
/// Not a `Result<(), StorageError>`: an unresolvable link is a **client**
/// mistake with a specific fix, and Slice A requires the response to name *which*
/// link is wrong. Folding it into `Unexpected(String)` would leave a client
/// parsing prose to find out which of four links to correct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryWrite {
    Saved,
    /// A link points at neither a known asset nor a known memory.
    ///
    /// `index` is the position in the submitted link list, because "one of your
    /// links is wrong" is not actionable when there are four of them.
    UnknownLinkTarget {
        index: usize,
        target: Uuid,
    },
}

/// What superseding a memory did.
///
/// Three outcomes, and the third is why this is not a `bool`: superseding a
/// memory that has *already* been corrected must name the current one, or a
/// client retrying has no way to find the right target and will keep hitting the
/// same wall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupersedeOutcome {
    Superseded,
    NotFound,
    /// Already corrected. Carries the id of the memory that corrected it.
    AlreadySuperseded {
        current: Uuid,
    },
    /// The correction's own link points at nothing known.
    ///
    /// Present for the same reason [`MemoryWrite::UnknownLinkTarget`] is: it is a
    /// **client** mistake with a specific fix. Leaving it out made the correction
    /// path return `500` for exactly the condition the create path returns `400`
    /// for — the same request body, a different status, decided only by which
    /// endpoint it was sent to.
    UnknownLinkTarget {
        index: usize,
        target: Uuid,
    },
}

/// What retracting a memory did.
///
/// Idempotency has the same shape as [`SupersedeOutcome`]'s and for the same
/// reason: a second retraction is not an error, but silently reporting
/// success a second time would hide that the reason given this time was
/// never recorded — so the *first* retraction's reason is what a caller gets
/// back either way.
#[derive(Debug, Clone, PartialEq)]
pub enum RetractOutcome {
    /// Retracted, returned in full.
    Retracted(Memory),
    NotFound,
    /// Already retracted. Carries the original retraction, not the reason
    /// this call supplied — a second call does not get to rewrite history.
    AlreadyRetracted(Memory),
}

/// What slice of memories a cross-entity search wants — Epic 41 Slice E.
///
/// Every field a filter, `None`/`false` meaning "do not narrow on this" —
/// the same shape [`ReviewQueueFilter`] uses, for the same reason: a search
/// with every filter absent is "everything", not an error.
#[derive(Debug, Clone, Default)]
pub struct MemorySearchFilter {
    /// A user id or agent id — whichever authored it. Matches either
    /// column, because a caller searching "what did Asha write" does not
    /// know or care which authorship shape backs the match.
    pub author: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_confidence: Option<f64>,
    /// `as_of >= since`.
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    /// `as_of <= until`.
    pub until: Option<chrono::DateTime<chrono::Utc>>,
    /// `false` — the default — excludes superseded memories, matching
    /// [`Storage::memories_about`]'s own default.
    pub include_superseded: bool,
    /// `false` — the default — excludes retracted memories. Administration
    /// is the one place a retracted memory needs to stay findable at all,
    /// which is why this defaults the other way from every other read.
    pub include_retracted: bool,
    pub limit: usize,
    pub offset: usize,
}

/// Whether a follow created an edge or found one already there.
///
/// Distinguished so a caller can report honestly, not so it can fail: both are
/// `200`. Slice F's idempotency is the point, and a double-follow that returned
/// `409` would make a retried request look like a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowOutcome {
    Followed,
    AlreadyFollowing,
}

/// What a principal holds, for the pre-delete check.
///
/// Counted **by kind**, because Slice G requires the refusal to report "how many
/// assets and of which types": "you still own 400 things" is not actionable, while
/// "1 service, 3 schemas, 396 columns" tells a steward whether to reassign the
/// service and let inheritance do the rest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holdings {
    pub owned_by_kind: Vec<(AssetKind, i64)>,
    /// Teams reporting into this one. Only ever non-empty for a team.
    pub child_teams: Vec<String>,
}

impl Holdings {
    /// Whether deleting this principal would strand anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.owned_by_kind.is_empty() && self.child_teams.is_empty()
    }

    #[must_use]
    pub fn owned_total(&self) -> i64 {
        self.owned_by_kind.iter().map(|(_, n)| n).sum()
    }
}

/// What deleting a principal did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrincipalDeletion {
    Deleted {
        reassigned: i64,
    },
    NotFound,
    /// It still holds things and no `reassignTo` was given.
    StillHolds(Box<Holdings>),
    /// The `reassignTo` target does not exist.
    UnknownTarget,
}

/// What deleting a domain found — Epic 23 Slice F.
///
/// Mirrors [`PrincipalDeletion`] deliberately: the two operations have the same
/// shape (a thing other things point at, deletable only when nothing does or
/// when a target is named) and giving them different shapes would mean two
/// almost-identical handlers a reader has to diff to trust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainDeletion {
    Deleted {
        /// Assets moved to the reassignment target.
        reassigned_assets: i64,
        /// Data products moved to it.
        reassigned_products: i64,
    },
    NotFound,
    /// It still holds things and no `reassignTo` was given.
    StillHolds(Box<DomainHoldings>),
    /// The `reassignTo` target does not exist.
    UnknownTarget,
    /// It has child domains, which must be handled explicitly — reassigning
    /// *assets* says nothing about where the sub-domains should go, and
    /// silently reparenting them to the target would restructure the
    /// accountability tree as a side effect of a delete.
    HasChildren {
        children: i64,
    },
}

/// What a domain still holds, by kind, so a `409` can say whether this is a
/// five-minute cleanup or a quarter's work.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DomainHoldings {
    /// Assets assigned **directly**. Inherited ones are not counted: they are
    /// not held by this domain, they are held by an ancestor of theirs that is,
    /// and deleting this domain does not orphan them.
    pub assets: i64,
    pub data_products: i64,
}

/// A change to a domain — absent means "not declared", the same PATCH rule the
/// rest of the envelope follows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DomainUpdate {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub domain_type: Option<Option<String>>,
    pub experts: Option<Vec<String>>,
    /// Reparenting. `Some(None)` promotes the domain to a root.
    pub parent_id: Option<Option<Uuid>>,
}

/// A change to a data product.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DataProductUpdate {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub purpose: Option<Option<String>>,
    pub domain_id: Option<Option<Uuid>>,
}

/// Why an asset could not be added to a data product.
///
/// Two reasons rather than one boolean, because they need different fixes: a
/// caller who sent a typo'd id and a caller who sent a tombstoned asset are
/// making different mistakes, and "asset not found" for the second is a lie
/// that sends them looking for the wrong thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipRefusal {
    NoSuchProduct,
    NoSuchAsset,
    AssetDeleted,
}

/// What a tag still holds, by entity kind — Epic 25 Slice H.
///
/// **By kind, not a total.** "This tag is used 4,312 times" tells a steward
/// nothing about the shape of the cleanup; "1 service, 3 schemas, 4,308
/// columns" tells them it is a propagation to undo, not a curation to redo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagUsage {
    pub by_kind: Vec<(String, i64)>,
}

impl TagUsage {
    #[must_use]
    pub fn total(&self) -> i64 {
        self.by_kind.iter().map(|(_, count)| count).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// What applying a tag did — Epic 25 Slice B.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelOutcome {
    Applied,
    /// It was already there. **Not an error**: applying a tag twice is the
    /// state the caller asked for, and a `409` would make every retry fail.
    AlreadyApplied,
    NoSuchTag,
    NoSuchTarget,
    /// Another tag from the same exclusive classification is present, named so
    /// the caller can act.
    Conflicts {
        existing_tag_fqn: String,
    },
    /// A human already rejected this exact suggestion, so an automated
    /// re-proposal is dropped rather than asked again.
    PreviouslyRejected,
}

/// What deciding a suggested label did — Epic 25 Slice D.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelDecision {
    Decided,
    NoSuchLabel,
    /// Confirming something already confirmed. A distinct outcome because it
    /// means the caller's picture of the queue is stale, which is worth telling
    /// them rather than silently succeeding.
    AlreadyConfirmed,
}

/// A change to a lifecycle state — Epic 26 Slice A.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleOutcome {
    Moved(Box<Asset>),
    NotFound,
    /// The move is not in the state machine, reported with both ends so the
    /// message can name them.
    Illegal {
        from: LifecycleState,
        to: LifecycleState,
    },
}

/// What issuing a certification did — Epic 26 Slice C.
#[derive(Debug, Clone, PartialEq)]
pub enum IssueOutcome {
    Issued(Box<StoredCertification>),
    NoSuchType,
    NoSuchTarget,
    /// The principal is not on the type's allowlist.
    NotAuthorized,
    /// Required evidence was absent, **named** — a count would tell an issuer
    /// nothing they can act on.
    MissingEvidence(Vec<String>),
}

/// A stored certification, with its type's name resolved for display.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCertification {
    pub id: Uuid,
    pub target_fqn: String,
    pub type_id: Uuid,
    pub type_name: String,
    pub issuer: String,
    pub criteria: Option<String>,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub evidence: Vec<(String, String)>,
}

/// A certification type as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCertificationType {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub default_validity_days: i32,
    pub required_evidence: Vec<String>,
    pub authorized_issuers: Vec<String>,
}

/// An observation as it arrives, before storage.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageWrite {
    pub asset_fqn: String,
    pub consumer: Consumer,
    pub operation: UsageOperation,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub row_count: Option<i64>,
    pub duration_ms: Option<i64>,
    pub query_id: Option<String>,
    /// **Dropped at the boundary when the deployment has not opted in.** Not
    /// filtered on read — the difference between not storing data and
    /// storing-then-hiding it is the whole of decision 2.
    pub query_text: Option<String>,
}

/// What a batch of observations did.
///
/// **Unmatched is not rejected** (Slice A). An observation about a table nobody
/// has catalogued yet is still worth keeping — the connector may simply not have
/// run — and discarding it would throw away exactly the usage that tells you
/// something is missing from the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UsageIngest {
    pub accepted: i64,
    /// Accepted, but the asset is not in the catalog yet.
    pub unmatched: i64,
    /// Already seen, by `(asset, query_id)`.
    pub duplicates: i64,
    /// Refused: an observation dated in the future is a clock problem, and
    /// storing it would make every window computation wrong until it passed.
    pub rejected: i64,
}

/// A contract as stored, with its parties and promises resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredContract {
    pub contract: Contract,
    pub breaches: Vec<ContractBreach>,
}

/// What evaluating an asset change against its contracts concluded.
#[derive(Debug, Clone, PartialEq)]
pub struct BreachReport {
    pub contract_id: Uuid,
    pub contract_name: String,
    pub producer: String,
    pub consumers: Vec<String>,
    pub column: String,
    pub detail: String,
}

/// A test case as stored, with its cadence already resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTestCase {
    pub id: Uuid,
    pub name: String,
    pub target_fqn: String,
    pub test_type: String,
    pub description: Option<String>,
    pub definition_id: Option<Uuid>,
    pub suite_id: Option<Uuid>,
    /// **Resolved here, not at read time.** A case may override its
    /// definition's cadence; folding that once means every consumer of a case
    /// sees the cadence that actually applies rather than re-deriving it and
    /// occasionally getting it wrong.
    pub expected_cadence: Option<String>,
}

/// A reusable check template.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTestDefinition {
    pub id: Uuid,
    pub name: String,
    pub test_type: String,
    pub description: Option<String>,
    pub expected_cadence: Option<String>,
}

/// One observation.
#[derive(Debug, Clone, PartialEq)]
pub struct TestResultWrite {
    pub case_id: Uuid,
    pub status: TestStatus,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub message: Option<String>,
    pub metrics: Option<serde_json::Value>,
}

/// A stored observation.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTestResult {
    pub id: Uuid,
    pub case_id: Uuid,
    pub status: TestStatus,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub message: Option<String>,
    pub metrics: Option<serde_json::Value>,
}

/// What a batch of results did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResultIngest {
    pub accepted: i64,
    /// Already seen, by `(case, observed_at)`. **Not an error**: a retried push
    /// is normal, and the same check at the same instant is one observation
    /// however many times it arrives.
    pub duplicates: i64,
    /// Refused: an observation dated in the future is a clock problem.
    pub rejected: i64,
    /// The named case does not exist.
    pub unknown_case: i64,
}

/// A column-level mapping inside a lineage edge — Epic 29 Slice D.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMapping {
    pub from_column_fqn: String,
    pub to_column_fqn: String,
    pub expression: Option<String>,
}

/// What a source-scoped lineage reconciliation did — Epic 29 Slice E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineageReconciliation {
    pub added: i64,
    /// Edges this source had asserted before and no longer does. **Only this
    /// source's**: a manually curated edge is never in this count, which is the
    /// whole property the slice exists for.
    pub removed: i64,
}

/// What claiming an idempotency key found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyClaim {
    /// The key is new and this caller owns it. Process the request.
    Claimed,
    /// The same key and the same request. Replay the original answer verbatim
    /// rather than doing the work again.
    Replay {
        status: u16,
        body: serde_json::Value,
    },
    /// The same key, **different** content. A key identifies a request, not a
    /// slot, so this is a client bug worth reporting — serving the first
    /// response would silently drop a push the client believes succeeded.
    Mismatch,
    /// Claimed, but the first attempt has not recorded its answer yet. A
    /// concurrent duplicate, which must not be processed a second time and
    /// cannot be replayed either.
    InFlight,
}

/// What setting an asset's owners did.
///
/// Not a `Result<(), StorageError>`: an owner naming a principal that does not
/// exist is a **client** mistake with a specific fix, and Slice C requires the
/// response to name *which* owner — `owners[1].id`. Folding it into
/// `Unexpected(String)` would leave a client re-reading prose to find out which
/// of three owners to correct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnersWrite {
    /// The new owner list, resolved to display names.
    Set(Vec<EntityReference>),
    NotFound,
    /// No such user or team. `index` is the position in the submitted list.
    UnknownPrincipal {
        index: usize,
        id: String,
    },
}

/// One connector run, as history records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorRun {
    pub id: Uuid,
    pub connector: String,
    pub service_name: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// `None` means the run never reported back — a crash, not a fast success.
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created: i32,
    pub skipped: i32,
    pub failed: i32,
    pub deleted: i32,
    pub failures: serde_json::Value,
    /// Why deletion detection declined, when it did. A refusal is a successful
    /// run that deliberately did nothing.
    pub refusal: Option<String>,
    pub triggered_by: String,
}

/// One batch ingestion job — Epic 16 Slice C.
///
/// Decision 2: **batch is a job, not a request.** A 500k-row file cannot be
/// answered synchronously, so this row *is* the answer, polled until it settles.
///
/// `state` is a string rather than an enum because the vocabulary is decided in
/// `graph-owl-connectors`' `JobState` and this crate is the port: importing the
/// enum here would put a domain decision behind a storage boundary, and
/// duplicating it would give two definitions of `partial` that could drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestJob {
    pub id: Uuid,
    /// The format the upload declared — `jsonl` or `csv`.
    pub format: String,
    pub state: String,
    pub rows_read: i64,
    pub accepted: i64,
    pub rejected: i64,
    /// The per-row reasons, bounded by the error cap.
    ///
    /// **Not just the count.** A job that reports only a number tells a client
    /// something is wrong and nothing about what, at which point their only move
    /// is to re-send the file and hope.
    pub failures: Vec<RowFailure>,
    /// Why it stopped before the end of the file, when it did.
    pub halt_reason: Option<String>,
    pub cancel_requested: bool,
    pub submitted_by: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Last time the worker said it was alive. A stale one is how a crashed job
    /// becomes distinguishable from a slow one.
    pub heartbeat_at: chrono::DateTime<chrono::Utc>,
    /// `None` while it is still running.
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Why one row did not land.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowFailure {
    /// The line number in the submitted file, so a client can grep for it.
    pub row: u64,
    pub detail: String,
}

/// What a job has done so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IngestProgress {
    pub rows_read: i64,
    pub accepted: i64,
    pub rejected: i64,
}

/// One stored violation.
///
/// Flat strings rather than the validator's typed `Violation`: this crosses a
/// database boundary, and `Sid` and `FlakeValue` are domain types with no
/// storage encoding. Rendering here keeps the mapping in one place instead of
/// spreading a serialisation decision across the adapter and the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationFinding {
    pub id: Uuid,
    pub shape: String,
    pub focus_node: String,
    pub path: Option<String>,
    pub constraint_kind: String,
    pub severity: String,
    pub message: String,
    pub actual: Option<String>,
    pub suggestion: Option<serde_json::Value>,
}

/// A violation somebody accepted, on the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    pub id: Uuid,
    /// What the waiver is *about* — the same four fields that identify a
    /// finding. Not the finding's row id: results are replaced wholesale every
    /// pass and every row gets a fresh UUID, so a waiver keyed on one would
    /// survive until the next run and then point at nothing.
    pub shape: String,
    pub focus_node: String,
    pub path: Option<String>,
    pub constraint_kind: String,
    /// **Required.** A waiver without a reason is a violation deleted with
    /// extra steps.
    pub reason: String,
    pub waived_by: String,
    pub waived_at: chrono::DateTime<chrono::Utc>,
    /// **Required.** A permanent waiver is a rule switched off without being
    /// switched off — invisible in the shape and never reviewed again.
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// A configured connector, **without its credential**.
///
/// There is deliberately **no field for the secret**. A `redacted: bool` beside
/// the value, or a `secret: Option<String>` a handler is trusted to skip, is one
/// `Debug` derive or one `..` spread away from a password in a log line or a
/// response body. Making it unrepresentable is the only version of this rule
/// that cannot be got wrong later by somebody who does not know it exists.
///
/// The run path reads the credential through [`Storage::connector_secret`],
/// which is the single call site anybody has to review.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorConfig {
    pub id: Uuid,
    pub connector: String,
    pub service_name: String,
    pub settings: serde_json::Value,
    /// Whether a credential is stored — never which one. An operator has to be
    /// able to tell "configured" from "configured and unusable", and that
    /// question does not need the value to answer it.
    pub has_secret: bool,
}

/// How an inbound webhook's signature is verified — Epic 18 Slice A.
///
/// `header` is which request header carries the signature; `prefix` is
/// HMAC's sender-specific label before the hex digest (GitHub's `sha256=`,
/// for instance) — Ed25519 signatures carry no such label.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SignatureScheme {
    HmacSha256 { header: String, prefix: String },
    Ed25519 { header: String },
}

/// A registered webhook receiver, **without its secret**.
///
/// Same reasoning as [`ConnectorConfig`]: no field for the key material, so
/// a `Debug` derive or a `..` spread can never leak it into a log line or a
/// response body. The verification path reads it through
/// [`Storage::webhook_secret`], the one call site anybody has to review.
///
/// For `Ed25519`, what is stored is the sender's *public* verifying key —
/// not sensitive on its own, but kept behind the same seam as the HMAC case
/// rather than special-cased, so there is one rule to audit, not two.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookEndpoint {
    pub id: Uuid,
    /// The URL path segment this endpoint receives on; unique, so two
    /// senders cannot collide on one receiver.
    pub path: String,
    /// Which bot/source principal events from this endpoint are attributed
    /// to.
    pub source: String,
    pub signature_scheme: SignatureScheme,
    pub mapping: String,
    pub event_filter: Vec<String>,
    pub enabled: bool,
    pub has_secret: bool,
    /// Deliveries this endpoint accepts per minute; `None` means unlimited —
    /// Epic 18 Slice E. Per-endpoint, not a global default: `01-api-conventions.md`
    /// treats rate limiting as an ingress concern except for per-principal
    /// quotas, and a registered endpoint's own configured budget *is* that
    /// quota, set by whoever knows this specific sender's expected volume
    /// rather than a single number this crate would otherwise have to guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_minute: Option<u32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A closed expression for extracting or composing a value from a webhook
/// payload — Epic 18 Slice C.
///
/// Five variants and no more, deliberately: a general scripting language can
/// hang the receiver forever, and every variant here recurses into a
/// strictly smaller, owned sub-expression, so evaluation terminates by
/// construction — there is no "repeat" or "while" construct to misuse into
/// one that does not.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Expression {
    /// A JSON Pointer (RFC 6901) into the payload, e.g. `/run/id`.
    Path { pointer: String },
    /// A fixed string, independent of the payload.
    Literal { value: String },
    /// The string results of each sub-expression, joined with nothing
    /// between them.
    Concat { parts: Vec<Expression> },
    /// The lowercased string result of one sub-expression.
    Lowercase { of: Box<Expression> },
    /// `pattern` with each `{name}` replaced by `bindings[name]`'s evaluated
    /// result, exactly once. Never re-scanned: a bound value that itself
    /// contains `{...}` must not trigger a second substitution pass, which
    /// is the shape an unbounded template evaluator's loop risk takes.
    Template {
        pattern: String,
        bindings: std::collections::BTreeMap<String, Expression>,
    },
}

/// A registered webhook payload-to-draft mapping, versioned — Epic 18 Slice C.
///
/// Every update is a new version, never an overwrite: "mappings are
/// versioned so a fix is auditable" means the old rule stays readable next
/// to the new one, not merely that a counter increments somewhere nobody can
/// see the history of.
///
/// `kind` and `entity_name` are required — every entity-draft path in this
/// codebase needs both (see `graph_owl_connectors::batch::RowDraft`, the
/// batch-ingestion equivalent this mirrors). `parent_fqn`/`description` are
/// optional expressions, and `properties` builds a free-form object the same
/// way an unrecognised batch column does.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub name: String,
    pub version: u32,
    pub kind: Expression,
    pub entity_name: Expression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_fqn: Option<Expression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Expression>,
    #[serde(default)]
    pub properties: std::collections::BTreeMap<String, Expression>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A group of people who own things together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub id: String,
    pub display_name: String,
    /// `None` is a real state — usually somebody created the team in a hurry.
    /// Requiring one would get it filled with the word "team".
    pub description: Option<String>,
    /// Ordered by id, so two reads of an unchanged team compare equal.
    pub members: Vec<String>,
    /// The team this one reports into — Epic 11 Slice B's nesting half.
    ///
    /// At most one, which is what makes this a hierarchy rather than a graph. A
    /// cycle at **any** depth is refused: `A → B → C → A` is as much a cycle as
    /// `A → A`, and a check that only compared immediate parents would pass the
    /// deep case while leaving an ancestor walk that never terminates.
    pub parent_team_id: Option<String>,
}

/// Who is fixing a violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub id: Uuid,
    /// The finding's identity, not its row id — same reason as [`Waiver`].
    pub shape: String,
    pub focus_node: String,
    pub path: Option<String>,
    pub constraint_kind: String,
    /// A `users.id`. **Not free text**: an assignment to a name nobody can
    /// resolve is a queue row that looks worked and is not.
    pub assignee: String,
    pub assigned_by: String,
    pub assigned_at: chrono::DateTime<chrono::Utc>,
}

/// What a caller wants from a list of assets.
///
/// A struct rather than a growing positional tail: `list_assets_visible(kind,
/// owner, unowned, page, predicate)` puts an `Option<&str>` next to a `bool` next
/// to two references, which is a signature that gets called wrong and still
/// compiles. Named fields also give the mutually-exclusive combination one place
/// to be rejected.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssetFilter<'a> {
    pub kind: Option<AssetKind>,
    /// Matches **effective** ownership — direct *and* inherited, using the same
    /// nearest-owned-ancestor rule the read path uses (Epic 11 Slice E). Matching
    /// only direct ownership would make "show me everything my team owns" answer
    /// "the four things somebody remembered to tag".
    ///
    /// An id matching no principal yields an **empty page, not an error**: a
    /// filter is a question, and "nothing" is a valid answer to it.
    pub owner: Option<&'a str>,
    /// Only assets with **no effective owner anywhere up their chain** — the
    /// ownership-gap report.
    ///
    /// This is the query Slice D's `inherited` flag exists to make answerable. It
    /// is deliberately not spelled `owner=none`: a sentinel would collide with a
    /// principal actually called `none`, and "no owner" is a different *kind* of
    /// question from "this owner" rather than a special value of it.
    pub unowned: bool,
    /// Organization-defined fields to match — Epic 22 Slice D.
    ///
    /// Repeated filters are **AND**, matching the conventions doc's rule for
    /// repeated parameters. Two bounds on one property is how a range is
    /// expressed, and it falls out of that rule rather than needing its own.
    pub extension: &'a [ExtensionFilter],
    /// Assets whose resolved domain is this one — Epic 23 Slice E.
    ///
    /// **Direct *and* inherited.** "Show me everything in the payments domain"
    /// is the query this epic exists for, and answering it with only the
    /// handful of assets somebody assigned by hand would report a governed
    /// estate as almost empty — the exact opposite of the truth, and the more
    /// dangerous direction to be wrong in.
    pub domain: Option<Uuid>,
    /// Assets belonging to this data product.
    pub data_product: Option<Uuid>,
}

/// One condition on a custom property's value — Epic 22 Slice D.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionFilter {
    pub name: String,
    pub op: ExtensionOp,
    /// The value as JSON, already coerced to the property's declared type by
    /// the facade. **Coerced there, not here**: a query string carries only
    /// text, and deciding that `30` means the number thirty rather than the
    /// string "30" needs the definition — which storage does not have and the
    /// facade already read to reject undefined names.
    pub value: serde_json::Value,
}

/// How a value is compared.
///
/// Three, not a general expression language. Equality answers "which tables are
/// in this cost centre" and the two bounds answer "which ones outlive ninety
/// days"; anything past that is search's job, not a list endpoint's
/// (`00d-api-conventions.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionOp {
    Eq,
    Gte,
    Lte,
}

/// What slice of the dead-letter queue a caller wants — Epic 18 Slice D.
/// Always scoped to `Failed` events; a steward triages by which endpoint is
/// misbehaving and what it's failing with, not by state, which this filter
/// does not need to name.
#[derive(Debug, Clone, Default)]
pub struct DeadLetterFilter {
    pub endpoint: Option<Uuid>,
    /// Substring match against `reason` — a steward searching for "every
    /// event this shape rejected" does not know the exact message, only
    /// the shape's name.
    pub reason_contains: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

/// What slice of the queue a caller wants.
#[derive(Debug, Clone, Default)]
pub struct ValidationFilter {
    pub severity: Option<String>,
    pub shape: Option<String>,
    pub focus_node: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

/// What slice of the review queue a caller wants — Epic 17 Slice F.
#[derive(Debug, Clone, Default)]
pub struct ReviewQueueFilter {
    /// `None` — the common case for the working queue — means "pending
    /// only"; every other status has to be asked for explicitly, or a
    /// decided entry no one is acting on would clutter the queue forever.
    pub status: Option<graph_owl_core::resolution::ReviewStatus>,
    /// The **target** asset's kind. A steward triages by what kind of thing
    /// is duplicated, not by what it might be a duplicate of.
    pub kind: Option<AssetKind>,
    pub min_score: Option<f64>,
    pub max_score: Option<f64>,
    pub limit: usize,
    pub offset: usize,
}

/// Connection-pool occupancy, for the operational gauge.
///
/// `idle` rather than `in_use`, because that is what a pool can report without
/// racing itself: connections move between the two constantly, and a pool that
/// counted both separately would publish a pair that does not sum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolStats {
    /// Connections the pool currently holds, idle or not.
    pub connections: u32,
    pub idle: u32,
}

/// A named vocabulary of business terms — Epic 24 Slice A.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glossary {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// Derived with `fqn::derive`, same as every other addressable entity.
    /// Terms below it derive theirs with `fqn::child_of`.
    pub fully_qualified_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A term as stored. The review workflow (Slice C) and SKOS relations (Slice
/// B) wire onto this by id; this type only carries what Slice A's CRUD needs.
#[derive(Debug, Clone, PartialEq)]
pub struct GlossaryTermRecord {
    pub id: Uuid,
    pub glossary_id: Uuid,
    pub name: String,
    /// `{glossary}.{term}` — **globally unique as an FQN**, even though `name`
    /// is only unique *within* its glossary (decision 1: "Customer" in Finance
    /// and "Customer" in Support are different terms with different
    /// addresses).
    pub fully_qualified_name: String,
    pub definition: String,
    pub status: graph_owl_core::glossary::TermStatus,
    pub synonyms: Vec<String>,
    pub abbreviations: Vec<String>,
    /// Bumped by Slice C's review workflow — a workflow move, not a field
    /// edit, so it starts at `1.0` (the migration's default) rather than
    /// [`EntityVersion::initial`]'s `0.1`; the two entities version for
    /// different reasons and there is no shared "first version" to agree on.
    pub version: EntityVersion,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A field-level change to a term. `None` means "leave alone" — the same
/// partial-update shape as [`graph_owl_core::TableUpdate`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlossaryTermUpdate {
    pub definition: Option<String>,
    pub synonyms: Option<Vec<String>>,
    pub abbreviations: Option<Vec<String>>,
}

/// `Metric` as stored — Epic 24 Slice E.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRecord {
    pub id: Uuid,
    pub name: String,
    /// **Namespaced away from tables** (decision — Slice E). A metric called
    /// `revenue` and a table called `revenue` are different things, and a
    /// shared FQN space would make one of them unaddressable.
    pub fully_qualified_name: String,
    pub definition: String,
    /// Prose, never evaluated (`graph_owl_core::metric` decision 3).
    pub formula: Option<String>,
    pub unit: Option<String>,
    pub granularity: Option<String>,
    pub calculation_type: graph_owl_core::metric::CalculationType,
    /// The glossary term that defines this metric. Must be `Approved` at
    /// creation; enforced by the facade, where the term's status can be read.
    pub defined_by: Option<Uuid>,
    /// What this metric claims as its sources. Slice F derives `derivedFrom`
    /// edges from this list rather than requiring both, which would invite
    /// the two to diverge.
    pub source_assets: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A field-level change to a metric. `None` means "leave alone". Does not
/// carry `source_assets` or `defined_by` — those go through their own path
/// because changing them means reconciling lineage (Slice F) or re-checking
/// the defining term's status, not merely replacing a value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricUpdate {
    pub definition: Option<String>,
    pub formula: Option<String>,
    pub unit: Option<String>,
    pub granularity: Option<String>,
    pub calculation_type: Option<graph_owl_core::metric::CalculationType>,
}

/// What deleting a glossary did.
///
/// Not a `bool`: "it still has terms" needs a count a caller can report, and
/// folding that into `StorageError::Conflict` would make the facade's 409
/// detail the adapter's job to word rather than the facade's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlossaryDeletion {
    Deleted,
    NotFound,
    /// Refused: it still has terms and the caller did not ask for a recursive
    /// delete.
    HasTerms {
        term_count: i64,
    },
}

/// A message broker's connection details — Epic 19.
///
/// Two variants, deliberately, matching the two client crates actually
/// adopted (`19-streaming.md` decision 6): Kafka and Pulsar do not share a
/// wire protocol, so there is no third "generic" variant that would almost
/// fit either. Redpanda needs no variant of its own — it speaks the Kafka
/// protocol, so `KafkaProtocol` already covers it.
///
/// `rename_all_fields`, not just `rename_all`: the latter renames a tagged
/// enum's *variant names*, not the fields inside them — a documented gotcha
/// from Epic 18's `Authorship`, which shipped `agent_id` on the wire because
/// only `rename_all` was set. `bootstrap_servers` is this type's first
/// multi-word variant field, so it is the first place that bug would have
/// resurfaced silently.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum BrokerConfig {
    KafkaProtocol { bootstrap_servers: String },
    Pulsar { service_url: String },
}

/// Where a new subscription starts reading from — Epic 19.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum StartPosition {
    Earliest,
    Latest,
    Timestamp { at: chrono::DateTime<chrono::Utc> },
    Offset { value: i64 },
}

/// A durable subscription to a broker topic, **without its credentials** —
/// Epic 19. Same reasoning as [`WebhookEndpoint`]: no field for SASL/token
/// material, so a `Debug` derive or a response body can never leak it. When a
/// broker needs one, it is readable through exactly one method
/// ([`Storage::stream_subscription_secret`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSubscription {
    pub id: Uuid,
    pub broker: BrokerConfig,
    pub topic: String,
    pub consumer_group: String,
    /// The declarative mapping (Epic 18 Slice C) messages on this topic are
    /// run through — reused wholesale rather than reinvented: the payload
    /// shapes and duplication problem are identical to a webhook's, and only
    /// the transport differs.
    pub mapping: String,
    pub start_position: StartPosition,
    pub max_in_flight: usize,
    pub poison_threshold: u32,
    pub has_secret: bool,
    /// Same reasoning as [`WebhookEndpoint::enabled`]: pausing consumption
    /// without losing the registration (and its committed offsets) needs a
    /// flag, not a delete-and-recreate.
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A streamed message that failed `poison_threshold` apply attempts —
/// Epic 19 Slice D. Kept with its raw payload so a replay after a mapping
/// fix re-applies exactly what the broker delivered, not a reconstruction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamDeadLetter {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub payload: Vec<u8>,
    pub reason: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait Storage: Send + Sync {
    /// Connection-pool occupancy, if this backend has a pool.
    ///
    /// `None` by default, and that is not the same as zero: a backend without a
    /// pool has nothing to report, and publishing `0` would show an operator a
    /// permanently empty pool rather than the absence of one.
    fn pool_stats(&self) -> Option<PoolStats> {
        None
    }

    /// Cheapest round trip that proves the backing store answers.
    async fn ping(&self) -> Result<(), StorageError>;

    async fn insert_table(&self, table: Table) -> Result<Table, StorageError>;
    async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError>;
    async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError>;
    async fn update_table(
        &self,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, StorageError>;
    async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError>;
    async fn create_relationship(
        &self,
        relationship: Relationship,
    ) -> Result<Relationship, StorageError>;
    async fn list_relationships_for_entity(
        &self,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<Vec<Relationship>, StorageError>;
    /// One relationship by id, or `None`.
    ///
    /// Needed by the graph projection on the delete path: a retraction must
    /// name the exact facts it withdraws, and once the row is deleted there is
    /// nothing left to name them from.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn get_relationship(&self, id: Uuid) -> Result<Option<Relationship>, StorageError>;

    async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Every relationship in the catalog, paginated — Epic 37b's own export
    /// primitive. `list_relationships_for_entity` cannot serve this: it needs
    /// an entity to start from, and a full-catalog export has none. Sorted by
    /// id, which is enough for a stable, deterministic cursor — relationships
    /// have no name of their own the way an asset's FQN gives it one.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn list_relationships(
        &self,
        page: &PageRequest,
    ) -> Result<Page<Relationship>, StorageError>;

    // ---- asset hierarchy (Epic 2) ----

    /// Inserts, or updates in place if the FQN is already known.
    ///
    /// Upsert rather than insert because a connector re-run must converge
    /// (`15-connectors.md` decision 3): the second run over an unchanged source
    /// has to be a no-op, not a wall of conflicts.
    async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError>;

    /// Bumps an asset straight to a computed version, with no diff of its
    /// own fields to derive one from — Epic 34 Slice B. `upsert_asset`
    /// cannot do this: neither backend's upsert touches the version columns
    /// on its update path (only a fresh insert sets them), because ordinary
    /// upserts are connector syncs that must converge silently, not edits.
    /// This exists for the one case that is a real, human-relevant version
    /// event without being an edit to the asset's own fields: a child of it
    /// (a topic's schema field, taggable and deletable on its own) was
    /// removed, and "a schema field removal is Major" needs to land on the
    /// *topic*, which `ChangeDescription::between`'s field-diff can never see
    /// happen since nothing about the topic's own row changed.
    async fn bump_version(
        &self,
        id: Uuid,
        next: graph_owl_core::envelope::EntityVersion,
        change_description: graph_owl_core::envelope::ChangeDescription,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError>;

    async fn get_asset(&self, id: Uuid) -> Result<Option<Asset>, StorageError>;
    async fn get_asset_by_fqn(&self, fqn: &str) -> Result<Option<Asset>, StorageError>;
    async fn list_assets(
        &self,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError>;
    /// Direct children, name-ordered. `None` parent lists roots.
    async fn list_children(&self, parent_id: Option<Uuid>) -> Result<Vec<Asset>, StorageError>;
    /// Root-to-self chain, for breadcrumbs and for cascade decisions.
    async fn ancestors_of(&self, id: Uuid) -> Result<Vec<Asset>, StorageError>;
    /// Case-insensitive substring match over name and FQN.
    async fn search_assets(
        &self,
        query: &str,
        kind: Option<AssetKind>,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError>;
    /// Every **live** asset whose FQN is `prefix` or sits beneath it.
    ///
    /// An **empty prefix means every asset**. Without that case the natural
    /// reading — "no prefix, no restriction" — silently matches nothing,
    /// because `fqn LIKE '.%'` is false for every real FQN. That is exactly the
    /// bug it caused: `projection_drift` scanned an empty set and reported no
    /// drift, which is the most dangerous possible answer from a drift
    /// detector.
    ///
    /// Unpaged on purpose: the caller is a connector run reconciling its whole
    /// scope, and a paged answer would let an asset slip between pages while
    /// the run writes — which would then read as "the source no longer reports
    /// it" and tombstone something that exists.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn list_assets_under_fqn(&self, prefix: &str) -> Result<Vec<Asset>, StorageError>;

    async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError>;

    /// Entities sharing at least one blocking key with `asset_id` (Epic 17
    /// Slice B) — the candidate set a resolver scores, never the full table.
    /// Blocking keys are computed and kept current by `upsert_asset` itself,
    /// so every write (including a rename) is reflected here without a
    /// separate call.
    ///
    /// Excludes `asset_id` itself and any tombstoned asset. Empty when
    /// nothing shares a key with it — that is a normal outcome, not an
    /// error: an isolated entity has no duplicates to find.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn resolution_candidates(&self, asset_id: Uuid) -> Result<Vec<Asset>, StorageError>;

    /// Writes a merge (Epic 17 Slice D). The caller has already retracted
    /// the merged entity's flakes and asserted `sameAs` — this only records
    /// the decision, which is what makes it reviewable and splittable later.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn create_merge_record(
        &self,
        record: graph_owl_core::resolution::MergeRecord,
    ) -> Result<graph_owl_core::resolution::MergeRecord, StorageError>;

    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn get_merge_record(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::resolution::MergeRecord>, StorageError>;

    /// Marks a merge split at `split_at`, without deleting the record
    /// (Slice E decision: a split is a fact about the merge, not its
    /// erasure).
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn split_merge_record(
        &self,
        id: Uuid,
        split_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SplitOutcome, StorageError>;

    /// The most recent split between this pair, in either role — the
    /// cooldown check Slice E's acceptance criteria require, so auto-merge
    /// does not immediately re-merge what a human just split.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn most_recent_split_between(
        &self,
        a: Uuid,
        b: Uuid,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError>;

    /// Queues a candidate pair for review (Epic 17 Slice F), or does nothing
    /// if the pair is already queued.
    ///
    /// **Idempotent by design, not merely by accident**: an existing entry —
    /// pending, confirmed, or rejected — is returned unchanged. This is the
    /// entire mechanism behind "a rejection is not re-queued on the next
    /// re-ingestion of the same draft": there is nothing that resets it,
    /// because the write that would have re-created it saw the row already
    /// there and stopped.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn queue_for_review(
        &self,
        entry: graph_owl_core::resolution::ReviewQueueEntry,
    ) -> Result<graph_owl_core::resolution::ReviewQueueEntry, StorageError>;

    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn list_review_queue(
        &self,
        filter: &ReviewQueueFilter,
    ) -> Result<(Vec<graph_owl_core::resolution::ReviewQueueEntry>, i64), StorageError>;

    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn get_review_queue_entry(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError>;

    /// Decides a pending entry. A entry that is already decided is returned
    /// unchanged rather than overwritten — a decision, once made, does not
    /// flip back and forth because a client called this twice.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn decide_review_queue_entry(
        &self,
        id: Uuid,
        status: graph_owl_core::resolution::ReviewStatus,
        decided_by: graph_owl_core::resolution::MergeDecidedBy,
        decided_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<graph_owl_core::resolution::ReviewQueueEntry>, StorageError>;

    /// Persists a mention resolution (Epic 17 Slice G). Never a merge — a
    /// mention links text to an entity, and this is the record of that link,
    /// not of any identity claim.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn record_mention_resolution(
        &self,
        resolution: graph_owl_core::resolution::MentionResolution,
    ) -> Result<graph_owl_core::resolution::MentionResolution, StorageError>;

    /// Every mention resolved from one source (e.g. a memory), most recent
    /// first.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn mention_resolutions_for_source(
        &self,
        source: Uuid,
    ) -> Result<Vec<graph_owl_core::resolution::MentionResolution>, StorageError>;

    // ---- envelope (Epic 3) ----

    /// Applies a partial update, computing the diff and advancing the version.
    ///
    /// Returns `Ok(None)` if the asset does not exist. A no-op update returns
    /// the asset unchanged at its current version — that is what makes a
    /// connector's convergence observable rather than merely claimed.
    /// Apply a partial update, optionally guarded by the version the caller
    /// believed it was editing.
    ///
    /// The guard must be evaluated under the same lock as the write. The
    /// Postgres adapter already reads `FOR UPDATE` inside the transaction that
    /// writes, so comparing there is atomic; an implementation that compared
    /// outside a lock would reintroduce exactly the race the precondition
    /// exists to close.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn update_asset(
        &self,
        id: Uuid,
        update: &AssetUpdate,
        updated_by: &str,
        expected_version: Option<EntityVersion>,
    ) -> Result<UpdateOutcome, StorageError>;

    async fn asset_versions(&self, id: Uuid) -> Result<Vec<AssetVersion>, StorageError>;

    /// Tombstones the asset and everything beneath it. Returns the count.
    async fn soft_delete_asset(&self, id: Uuid, deleted_by: &str) -> Result<u64, StorageError>;

    /// Lifts the tombstone from the asset and its subtree.
    async fn restore_asset(&self, id: Uuid, restored_by: &str) -> Result<u64, StorageError>;

    // ---- identity and policy (Epics 11-13) ----

    /// Looks up a user with their roles. `None` means unknown, which the
    /// facade turns into an auto-provision (`12-13-security.md` decision 7).
    async fn find_user(&self, id: &str) -> Result<Option<StoredUser>, StorageError>;
    async fn upsert_user(&self, user: &StoredUser) -> Result<(), StorageError>;
    /// Every policy attached to any of these roles, deduplicated.
    async fn policies_for_roles(&self, roles: &[String]) -> Result<Vec<Policy>, StorageError>;

    /// Creates or replaces a policy, and replaces which roles it applies to.
    ///
    /// `roles` is the complete set going forward, not an addition to whatever
    /// was there before — the same replace-the-whole-set semantics
    /// `Catalog::set_user_roles` uses for a user's roles. A caller that wants
    /// to add one role to an existing policy reads its current roles first.
    async fn upsert_policy(&self, policy: &Policy, roles: &[String]) -> Result<(), StorageError>;

    /// Every stored policy, with the roles it currently applies to.
    async fn list_policies(&self) -> Result<Vec<(Policy, Vec<String>)>, StorageError>;

    /// Removes a policy and its role attachments. Returns whether one existed.
    async fn delete_policy(&self, name: &str) -> Result<bool, StorageError>;

    /// Assert a lineage edge.
    ///
    /// # Errors
    /// `Conflict` when the same `(from, to, relationship, source)` already
    /// exists — the same pair from a *different* source is a distinct fact and
    /// is accepted.
    async fn create_lineage_edge(
        &self,
        edge: &graph_owl_core::lineage::LineageEdge,
    ) -> Result<(), StorageError>;

    /// Delete an edge and return what was deleted.
    ///
    /// The edge rather than a boolean, because the caller has to mirror the
    /// removal into the graph and needs the endpoints to name the triple. One
    /// `DELETE ... RETURNING` rather than a read followed by a delete: the
    /// two-statement version races with a concurrent delete and projects a
    /// retraction for an edge somebody else already removed.
    async fn delete_lineage_edge(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::lineage::LineageEdge>, StorageError>;

    /// Every edge touching any of these assets, in one round trip.
    ///
    /// A walk asks for one level at a time and would otherwise make one query
    /// per node per level — the shape that turns a five-deep lineage graph into
    /// hundreds of queries.
    async fn lineage_edges_touching(
        &self,
        asset_ids: &[Uuid],
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError>;

    /// Every edge naming this asset as the pipeline that moved the data —
    /// Epic 34 Slice C. What `Catalog::soft_delete_asset`'s force-guard
    /// checks: a pipeline referenced here resists deletion, because removing
    /// it would leave the edges it explains attributed to nothing.
    async fn lineage_edges_by_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<graph_owl_core::lineage::LineageEdge>, StorageError>;

    /// Replace the stored validation results with a fresh pass.
    ///
    /// **Wholesale.** A violation that has been fixed must vanish, and merging
    /// would leave it standing until something thought to delete it — a queue
    /// that only ever grows is a queue nobody works. `computed_at_t` is the
    /// graph instant the pass ran against, so a stale report is visibly stale
    /// rather than silently so.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the replacement fails; it is one transaction, so a
    /// failure leaves the previous results in place rather than an empty queue.
    async fn replace_validation_results(
        &self,
        computed_at_t: i64,
        results: &[ValidationFinding],
    ) -> Result<(), StorageError>;

    /// The current queue, worst first.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn validation_results(
        &self,
        filter: &ValidationFilter,
    ) -> Result<(Vec<ValidationFinding>, i64, usize), StorageError>;

    /// Record that somebody accepted a violation.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] if this finding is already waived — a second
    /// waiver would hide which reason is the live one.
    async fn waive_finding(&self, waiver: &Waiver) -> Result<(), StorageError>;

    /// Withdraw a waiver, putting the finding back in the queue.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the delete fails. A waiver that was not there is
    /// `Ok(false)`, not an error: revoking twice is the same intent twice.
    async fn revoke_waiver(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Every waiver, expired ones included.
    ///
    /// **Expiry is evaluated by the reader, not filtered here.** A queue that
    /// silently dropped expired waivers would make a finding reappear with no
    /// explanation of where its acceptance went.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn waivers(&self) -> Result<Vec<Waiver>, StorageError>;

    /// Save a connector configuration.
    ///
    /// `secret` is `None` to **leave an existing credential alone** — an
    /// edit-then-save round trip cannot resend what it was never given, and
    /// treating absent as "clear it" would silently break a connector every
    /// time somebody renamed its service.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails.
    async fn upsert_connector_config(
        &self,
        config: &ConnectorConfig,
        secret: Option<&str>,
    ) -> Result<(), StorageError>;

    /// Every configuration, without credentials.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn connector_configs(&self) -> Result<Vec<ConnectorConfig>, StorageError>;

    /// The stored credential, for the run path only.
    ///
    /// **The one call site that sees a secret**, which is the point of it being
    /// a separate method: a reviewer auditing where credentials go has one
    /// signature to grep for rather than every read of a config.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn connector_secret(&self, id: Uuid) -> Result<Option<String>, StorageError>;

    /// Registers or updates a webhook endpoint.
    ///
    /// `secret` is `None` to **leave an existing key alone** — same reasoning
    /// as [`Self::upsert_connector_config`]: an edit-then-save round trip
    /// cannot resend a secret it was never given back.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] if `path` is already registered to a
    /// different endpoint; [`StorageError::Unexpected`] if the write fails.
    async fn upsert_webhook_endpoint(
        &self,
        endpoint: WebhookEndpoint,
        secret: Option<&[u8]>,
    ) -> Result<WebhookEndpoint, StorageError>;

    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn get_webhook_endpoint(&self, id: Uuid)
    -> Result<Option<WebhookEndpoint>, StorageError>;

    /// The endpoint a request arrived on, by its URL path — what the
    /// receiver handler looks up before it can even attempt verification.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn get_webhook_endpoint_by_path(
        &self,
        path: &str,
    ) -> Result<Option<WebhookEndpoint>, StorageError>;

    /// Every registered endpoint, without secrets.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn list_webhook_endpoints(&self) -> Result<Vec<WebhookEndpoint>, StorageError>;

    /// The stored key material, for the verification path only.
    ///
    /// **The one call site that sees a secret** — the point of it being
    /// separate from [`Self::get_webhook_endpoint`], the same reasoning as
    /// [`Self::connector_secret`].
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn webhook_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError>;

    /// Registers or updates a streaming subscription — Epic 19.
    ///
    /// `secret` is `None` to **leave an existing credential alone** — same
    /// reasoning as [`Self::upsert_webhook_endpoint`].
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] if `(topic, consumer_group)` is already
    /// registered to a different subscription;
    /// [`StorageError::Unexpected`] if the write fails.
    async fn upsert_stream_subscription(
        &self,
        subscription: StreamSubscription,
        secret: Option<&[u8]>,
    ) -> Result<StreamSubscription, StorageError>;

    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn get_stream_subscription(
        &self,
        id: Uuid,
    ) -> Result<Option<StreamSubscription>, StorageError>;

    /// Every registered subscription, without secrets.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn list_stream_subscriptions(&self) -> Result<Vec<StreamSubscription>, StorageError>;

    /// The stored credential, for the consume path only.
    ///
    /// **The one call site that sees a secret** — same reasoning as
    /// [`Self::webhook_secret`].
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn stream_subscription_secret(&self, id: Uuid) -> Result<Option<Vec<u8>>, StorageError>;

    /// Persists a poisoned streamed message — Epic 19 Slice D.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails.
    async fn create_stream_dead_letter(
        &self,
        letter: StreamDeadLetter,
    ) -> Result<StreamDeadLetter, StorageError>;

    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn get_stream_dead_letter(
        &self,
        id: Uuid,
    ) -> Result<Option<StreamDeadLetter>, StorageError>;

    /// Dead letters, newest first, optionally scoped to one subscription.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn list_stream_dead_letters(
        &self,
        subscription: Option<Uuid>,
    ) -> Result<Vec<StreamDeadLetter>, StorageError>;

    /// Removes a dead letter — the successful end of a replay. Returns
    /// whether a row existed to remove.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the delete fails.
    async fn delete_stream_dead_letter(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Persists a received (already signature-verified) inbound event.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails.
    async fn create_inbound_event(
        &self,
        event: graph_owl_core::webhook::InboundEvent,
    ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError>;

    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn get_inbound_event(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_core::webhook::InboundEvent>, StorageError>;

    /// Moves an inbound event to a new state — Epic 18 Slice D's processing
    /// pipeline. `reason` is written alongside every transition (`Some` only
    /// for `Failed`, `None` otherwise), never left stale from an earlier
    /// attempt: a replay that succeeds clears whatever reason a prior
    /// failure left behind.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails, or the id names no event.
    async fn update_inbound_event_state(
        &self,
        id: Uuid,
        state: graph_owl_core::webhook::EventState,
        reason: Option<&str>,
    ) -> Result<graph_owl_core::webhook::InboundEvent, StorageError>;

    /// The dead-letter queue, filtered — Epic 18 Slice D.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn list_dead_letters(
        &self,
        filter: &DeadLetterFilter,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError>;

    /// Every event for `endpoint` received between `since` and `until`
    /// (inclusive), ordered for replay: by `sender_timestamp` where an event
    /// has one, falling back to arrival order (`received_at`) for the ones
    /// that do not — the same fallback [`graph_owl_core::webhook::Freshness::Ambiguous`]
    /// names.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn list_inbound_events_in_window(
        &self,
        endpoint: Uuid,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<graph_owl_core::webhook::InboundEvent>, StorageError>;

    /// Deletes dead-lettered events older than `older_than` — Slice D's
    /// "DLQ retention is bounded and configurable" criterion. The bound is
    /// the caller's to configure (a runbook, an admin call, a schedule);
    /// this is the mechanism, not a policy this crate decides on its own.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails.
    async fn purge_dead_letters(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, StorageError>;

    /// The `sender_timestamp` most recently and successfully applied for
    /// this entity, if anything has been — the high-water mark
    /// `graph_owl_core::webhook::compare_timestamps` checks a candidate
    /// against before `process_inbound_event` overwrites it.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn last_applied_timestamp(
        &self,
        fully_qualified_name: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError>;

    /// Records that `sender_timestamp` is the newest applied so far for this
    /// entity. Always an upsert to the newer value — callers only reach
    /// this after confirming the candidate is not older than what is
    /// already there.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails.
    async fn record_applied_timestamp(
        &self,
        fully_qualified_name: &str,
        sender_timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StorageError>;

    /// Records a new version of a mapping — Epic 18 Slice C. `version` and
    /// `created_at` on the argument are ignored; the real values (the
    /// existing max version for this name, plus one, and the write's own
    /// timestamp) come back on the returned value. Never an update in
    /// place: every call adds a version, so a fix is auditable against what
    /// it replaced.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails.
    async fn upsert_mapping(&self, mapping: Mapping) -> Result<Mapping, StorageError>;

    /// The latest version of a mapping by name.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn get_mapping(&self, name: &str) -> Result<Option<Mapping>, StorageError>;

    /// Every version of a mapping, newest first — the audit trail "mappings
    /// are versioned" exists for.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn list_mapping_versions(&self, name: &str) -> Result<Vec<Mapping>, StorageError>;

    /// Create or update a team, replacing its membership.
    ///
    /// Membership is **replaced, not merged**: a partial update cannot express
    /// "remove everybody", and removal is the operation that has to work — a
    /// team somebody has left is an owner who no longer exists.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the write fails, including when a named member is
    /// not a known user.
    async fn upsert_team(&self, team: &Team) -> Result<(), StorageError>;

    /// One team, with its members.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn find_team(&self, id: &str) -> Result<Option<Team>, StorageError>;

    /// Every team, by id.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn teams(&self) -> Result<Vec<Team>, StorageError>;

    /// Put a finding on somebody's plate.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] if it is already assigned — two owners is no
    /// owner. [`StorageError`] if the assignee is not a known user.
    async fn assign_finding(&self, assignment: &Assignment) -> Result<(), StorageError>;

    /// Take a finding off somebody's plate.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the delete fails. Unassigning something that was not
    /// assigned is `Ok(false)`, not an error.
    async fn unassign_finding(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Every assignment.
    ///
    /// # Errors
    ///
    /// [`StorageError`] if the read fails.
    async fn assignments(&self) -> Result<Vec<Assignment>, StorageError>;

    /// Open a run row before the work starts.
    ///
    /// Written *before* rather than after, so a run that dies mid-flight leaves
    /// a row with no `finished_at` instead of leaving nothing. A history that
    /// only records completions cannot show a crash, which is the failure it is
    /// most needed for.
    async fn begin_run(&self, run: &ConnectorRun) -> Result<(), StorageError>;

    /// Close it with what actually happened.
    async fn finish_run(&self, run: &ConnectorRun) -> Result<(), StorageError>;

    /// Recent runs for a service, newest first.
    async fn recent_runs(
        &self,
        service_name: &str,
        limit: usize,
    ) -> Result<Vec<ConnectorRun>, StorageError>;

    /// Fingerprints for the FQNs a run is about to write.
    ///
    /// One round trip for the whole batch, not one per record: the point of
    /// fingerprinting is to make an unchanged re-run cheap, and a per-record
    /// lookup would replace the write it saves with a read.
    ///
    /// FQNs absent from the result do not exist; present-with-`None` exist
    /// without a fingerprint. Those are different answers — see `Existing` in
    /// `graph-owl-connectors`.
    async fn source_hashes(
        &self,
        fqns: &[String],
    ) -> Result<std::collections::HashMap<String, Option<Vec<u8>>>, StorageError>;

    /// Record what the source said, so the next run can compare against it.
    async fn set_source_hash(&self, id: Uuid, hash: &[u8]) -> Result<(), StorageError>;

    /// Lists assets visible under `predicate`.
    ///
    /// Separate from `list_assets` so the unfiltered path cannot be reached by
    /// accident from a request handler — a filtered call site that forgets the
    /// predicate is a leak, and this makes forgetting a compile error.
    /// Lists assets matching `filter`, visible under `predicate`.
    async fn list_assets_visible(
        &self,
        filter: &AssetFilter<'_>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<Asset>, StorageError>;

    /// Takes the whole [`AssetFilter`] rather than a bare `kind`, so a custom
    /// property filter narrows a search exactly as it narrows a list. Two
    /// endpoints that filtered differently would be two endpoints a client has
    /// to learn separately, and the one that got it wrong would be the one that
    /// silently returned more.
    async fn search_assets_visible(
        &self,
        query: &str,
        filter: &AssetFilter<'_>,
        page: &PageRequest,
        predicate: &AccessPredicate,
    ) -> Result<Page<Asset>, StorageError>;

    async fn list_children_visible(
        &self,
        parent_id: Option<Uuid>,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError>;

    /// How many visible assets carry a **non-empty** description, and how many
    /// there are in total.
    ///
    /// Non-empty, not non-null: a description of `"   "` is not documentation,
    /// and counting it would make the coverage number reward whitespace.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn count_documented_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<(i64, i64), StorageError>;

    /// The most recently changed visible assets, newest first.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the query fails.
    async fn recently_changed_visible(
        &self,
        limit: i64,
        predicate: &AccessPredicate,
    ) -> Result<Vec<Asset>, StorageError>;

    async fn count_assets_by_kind_visible(
        &self,
        predicate: &AccessPredicate,
    ) -> Result<Vec<(AssetKind, i64)>, StorageError>;

    // ---- Epic 31: organizational memory ----

    /// Store a memory and its links.
    ///
    /// Links are resolved to an asset or another memory here, because the
    /// schema keeps them in separate foreign-key columns — a polymorphic target
    /// could not be a foreign key, and a deleted asset that goes on being named
    /// as a subject is the silent rot this schema refuses everywhere else.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] if the id already exists.
    /// [`StorageError::Unexpected`] if the write fails.
    async fn save_memory(&self, memory: &Memory) -> Result<MemoryWrite, StorageError>;

    /// One memory, with its links, **whether or not it has been superseded**.
    ///
    /// A superseded memory stays readable: the record of what people believed
    /// before they were corrected is most of the reason to keep a record.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn find_memory(&self, id: Uuid) -> Result<Option<Memory>, StorageError>;

    /// Every memory linked to this subject, by any relation.
    ///
    /// **By any relation, not only `About`** — ranking needs the weak links too,
    /// since it is what distinguishes "about this table" from "mentions it".
    /// Filtering here would hard-code a relevance decision into a read.
    ///
    /// `include_superseded` defaults the retrieval contract: current only, with
    /// history available on request.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn memories_about(
        &self,
        subject: Uuid,
        include_superseded: bool,
    ) -> Result<Vec<Memory>, StorageError>;

    /// Replace a memory with a correction, marking both sides in one
    /// transaction.
    ///
    /// **Both halves or neither.** Supersession is two rows, and a half-written
    /// pair is a chain that reads as history but is not — the dangling case is
    /// real enough that contradiction detection has a test for it.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn supersede_memory(
        &self,
        original: Uuid,
        replacement: &Memory,
    ) -> Result<SupersedeOutcome, StorageError>;

    /// Mark a memory as no longer believed, without replacing it.
    ///
    /// Never a delete — the retracted row stays readable, matching every
    /// other retraction in this schema.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn retract_memory(&self, id: Uuid, reason: &str) -> Result<RetractOutcome, StorageError>;

    /// Every memory matching a cross-entity search, and the total before
    /// paging — the same `(rows, total)` shape [`Self::list_review_queue`]
    /// returns, for the same reason: a filtered count a page cannot show
    /// answers "is there more" without a second round trip.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn search_memories(
        &self,
        filter: &MemorySearchFilter,
    ) -> Result<(Vec<Memory>, i64), StorageError>;

    /// Record what a human decided about a candidate contradiction.
    ///
    /// **Upsert, not insert.** A reviewer changing their mind is one pair with a
    /// new verdict, not a second row — and a duplicate-key failure on a second
    /// click is a `500` for a person doing something reasonable.
    ///
    /// The pair is normalised before it is stored, and the schema enforces it: a
    /// verdict recorded in the other order would silently stop applying, the
    /// queue would reopen or downgrade the pair on its own, and it would be
    /// unreproducible because it depends on load order.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails, including when either
    /// memory or the reviewing user is unknown.
    async fn review_contradiction(
        &self,
        review: Review,
        reviewed_by: &str,
        note: Option<&str>,
    ) -> Result<(), StorageError>;

    // ---- Epic 11 Slices B, F, G: nesting, following, and safe deletion ----

    /// Teams reporting directly into this one.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn child_teams(&self, id: &str) -> Result<Vec<Team>, StorageError>;

    /// Whether making `parent` the parent of `team` would close a cycle.
    ///
    /// Checked by walking `parent`'s ancestry, so a cycle at **any** depth is
    /// caught. Slice B's own mutator watch is exactly this: "a check that only
    /// compares immediate parent passes depth-1 and fails depth-3".
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn would_cycle(&self, team: &str, parent: &str) -> Result<bool, StorageError>;

    /// Record that a user follows an asset.
    ///
    /// **Idempotent.** Following something you already follow is not an error, it
    /// is the state you asked for — Slice F requires a second follow to be a `200`
    /// with one edge rather than a `409`.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails, including when the asset
    /// or the user is unknown.
    async fn follow_asset(
        &self,
        asset_id: Uuid,
        user_id: &str,
    ) -> Result<FollowOutcome, StorageError>;

    /// Stop following. Unfollowing something not followed is also not an error.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn unfollow_asset(&self, asset_id: Uuid, user_id: &str) -> Result<(), StorageError>;

    /// What this user follows, newest first.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn assets_followed_by(
        &self,
        user_id: &str,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError>;

    /// How many people follow this asset.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn follower_count(&self, asset_id: Uuid) -> Result<i64, StorageError>;

    /// What a principal still holds, so deletion can refuse with a reason.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn principal_holdings(&self, principal: &OwnerRef) -> Result<Holdings, StorageError>;

    /// Delete a principal, optionally transferring what it owns first.
    ///
    /// **One transaction.** A reassignment that moved half the assets and then
    /// failed to delete the principal would leave ownership half-moved with no
    /// record of which half — Slice G's mutator watch is precisely
    /// "non-transactional reassignment".
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn delete_principal(
        &self,
        principal: &OwnerRef,
        reassign_to: Option<&OwnerRef>,
    ) -> Result<PrincipalDeletion, StorageError>;

    // ---- Epic 16 Slice B: idempotency ----

    /// Claim `key` for a request, or report what was already answered under it.
    ///
    /// **Atomic.** Slice B requires that "concurrent identical requests produce
    /// one effect, not two", and a read-then-write would let two callers both see
    /// an unclaimed key. The insert *is* the claim.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn claim_idempotency_key(
        &self,
        key: &str,
        request_hash: &str,
    ) -> Result<IdempotencyClaim, StorageError>;

    /// Record the answer a claimed key produced, so a replay can return it.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn record_idempotent_response(
        &self,
        key: &str,
        status: u16,
        body: &serde_json::Value,
    ) -> Result<(), StorageError>;

    // ---- Epic 16 Slice C: batch jobs ----

    /// Record a job that is about to start.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn create_ingest_job(&self, job: &IngestJob) -> Result<(), StorageError>;

    /// The job as it stands, or `None` if no such job.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn ingest_job(&self, id: Uuid) -> Result<Option<IngestJob>, StorageError>;

    /// Report progress **and learn whether to stop**, in one round trip.
    ///
    /// The two are deliberately the same call. A worker that heartbeats and then
    /// separately asks "was I cancelled?" does twice the work to answer a
    /// question the first statement already had the row for, and the window
    /// between the two is exactly where a cancelled job processes one more chunk.
    ///
    /// Returns whether cancellation has been requested.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn report_ingest_progress(
        &self,
        id: Uuid,
        progress: IngestProgress,
        new_failures: &[RowFailure],
    ) -> Result<bool, StorageError>;

    /// Close a job out with its verdict.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn finish_ingest_job(
        &self,
        id: Uuid,
        state: &str,
        halt_reason: Option<&str>,
    ) -> Result<(), StorageError>;

    /// Ask an in-flight job to stop. Returns `false` if it had already finished.
    ///
    /// **A request, not an order.** The worker is the only thing that can stop
    /// cleanly and report what landed; killing it from here would leave the
    /// counts describing a moment nobody observed.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn cancel_ingest_job(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Fail every job that stopped reporting, returning how many were reaped.
    ///
    /// A process that dies mid-job leaves a row saying `running` forever, and a
    /// client polling it waits for an answer that will never come. The heartbeat
    /// is what makes "stopped reporting" observable without a scheduler.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn reap_abandoned_ingest_jobs(
        &self,
        stale_after_seconds: i64,
    ) -> Result<u64, StorageError>;

    /// Every verdict, so detection can skip what a human closed and upgrade what
    /// they confirmed.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn contradiction_reviews(&self) -> Result<Vec<Review>, StorageError>;

    // ---- Epic 11 Slice C: ownership ----

    /// Replace an asset's owners, preserving submitted order.
    ///
    /// **Replace, not merge.** A partial update cannot express "this asset has no
    /// owner any more", and dropping the last owner is the operation that has to
    /// work — an unowned asset is a real, reportable state, and the ownership-gap
    /// report is only meaningful if it can be reached.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the write fails.
    async fn set_asset_owners(
        &self,
        asset_id: Uuid,
        owners: &[OwnerRef],
    ) -> Result<OwnersWrite, StorageError>;

    /// An asset's owners, in submitted order, with display names resolved.
    ///
    /// Resolved at read time rather than stored, so a renamed team shows its new
    /// name everywhere instead of whatever it was called when ownership was
    /// assigned.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unexpected`] if the read fails.
    async fn asset_owners(&self, asset_id: Uuid) -> Result<Vec<EntityReference>, StorageError>;

    // ---- Epic 24 Slice A: glossary and terms ----

    /// # Errors
    /// [`StorageError::Conflict`] (`kind: Fqn`) if the FQN is already taken.
    async fn insert_glossary(&self, glossary: Glossary) -> Result<Glossary, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_glossary(&self, id: Uuid) -> Result<Option<Glossary>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_glossaries(&self) -> Result<Vec<Glossary>, StorageError>;

    /// Delete a glossary. `recursive` deletes its terms first rather than
    /// refusing — see [`GlossaryDeletion::HasTerms`].
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn delete_glossary(
        &self,
        id: Uuid,
        recursive: bool,
    ) -> Result<GlossaryDeletion, StorageError>;

    /// # Errors
    /// [`StorageError::Conflict`] (`kind: Fqn`) if the FQN is already taken,
    /// which covers both a duplicate name within the glossary and a
    /// cross-glossary FQN collision.
    async fn insert_term(
        &self,
        term: GlossaryTermRecord,
    ) -> Result<GlossaryTermRecord, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_term(&self, id: Uuid) -> Result<Option<GlossaryTermRecord>, StorageError>;

    /// Every term in one glossary.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_terms(&self, glossary_id: Uuid) -> Result<Vec<GlossaryTermRecord>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn update_term(
        &self,
        id: Uuid,
        update: GlossaryTermUpdate,
    ) -> Result<Option<GlossaryTermRecord>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn delete_term(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Full-text search over name, synonyms, abbreviations and definition —
    /// the same weighting the migration's `search_vector` encodes.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn search_terms(&self, query: &str) -> Result<Vec<GlossaryTermRecord>, StorageError>;

    // ---- Epic 24 Slice B: SKOS relations ----

    /// Assert one relation, owned by `term_id`.
    ///
    /// **Idempotent** — asserting the same relation twice is the same fact,
    /// not a conflict, so a repeat is a no-op success rather than an error.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn insert_term_relation(
        &self,
        term_id: Uuid,
        relation: graph_owl_core::glossary::SkosRelation,
    ) -> Result<(), StorageError>;

    /// Retract a relation `term_id` owns. Returns `false` if no such row
    /// exists — including when `relation` is one only visible on `term_id`
    /// as a *derived* inverse (e.g. `narrower`), which was never a row to
    /// delete in the first place.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn delete_term_relation(
        &self,
        term_id: Uuid,
        relation: &graph_owl_core::glossary::SkosRelation,
    ) -> Result<bool, StorageError>;

    /// Every stored relation with `term_id` on **either** end — what it
    /// declared, and what points at it — keyed by the declaring term's id.
    /// This is exactly the `stored` shape
    /// [`graph_owl_core::glossary::visible_relations`] takes to compute the
    /// full picture including derived inverses.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn term_relations_touching(
        &self,
        term_id: Uuid,
    ) -> Result<Vec<(String, graph_owl_core::glossary::SkosRelation)>, StorageError>;

    /// Every stored `broader` edge, as `(child, parent)` pairs — what
    /// [`graph_owl_core::glossary::would_cycle`] walks before a new one is
    /// accepted.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn broader_edges(&self) -> Result<Vec<(String, String)>, StorageError>;

    // ---- Epic 24 Slice C: review workflow ----

    /// Replace a term's assigned reviewers. Replace, not merge — same reason
    /// as `Team`'s membership replace: a partial update cannot express
    /// "nobody reviews this any more".
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn set_term_reviewers(
        &self,
        term_id: Uuid,
        reviewers: &[String],
    ) -> Result<(), StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn term_reviewers(&self, term_id: Uuid) -> Result<Vec<String>, StorageError>;

    /// Move a term to `to`, recording who did it, why, and bumping its
    /// version. `None` if the term does not exist.
    ///
    /// **Records the transition and writes the new status atomically** — a
    /// status change with no account of how it got there is the one thing a
    /// reviewer is for.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn transition_term(
        &self,
        term_id: Uuid,
        from: graph_owl_core::glossary::TermStatus,
        to: graph_owl_core::glossary::TermStatus,
        actor: &str,
        reason: Option<String>,
        successor_term_id: Option<Uuid>,
    ) -> Result<Option<GlossaryTermRecord>, StorageError>;

    // ---- Epic 24 Slice D: terms attach to assets and columns ----

    /// Attach a term to `target_fqn` — an asset or an individual column,
    /// both addressed by FQN since a column has no row of its own until
    /// Epic 22. **Idempotent**: re-attaching what is already attached is the
    /// same fact, not a conflict.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn attach_term(
        &self,
        term_id: Uuid,
        target_fqn: &str,
        attached_by: &str,
    ) -> Result<(), StorageError>;

    /// Detach a term from `target_fqn`. `false` if no such attachment
    /// exists.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn detach_term(&self, term_id: Uuid, target_fqn: &str) -> Result<bool, StorageError>;

    /// Every asset or column this term is attached to, paginated.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn term_usage(
        &self,
        term_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<String>, StorageError>;

    // ---- Epic 24 Slice E: Metric as a first-class entity ----

    /// # Errors
    /// [`StorageError::Conflict`] (`kind: Fqn`) if the FQN is already taken.
    async fn insert_metric(&self, metric: MetricRecord) -> Result<MetricRecord, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_metric(&self, id: Uuid) -> Result<Option<MetricRecord>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_metrics(&self, page: &PageRequest) -> Result<Page<MetricRecord>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn update_metric(
        &self,
        id: Uuid,
        update: MetricUpdate,
    ) -> Result<Option<MetricRecord>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn delete_metric(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Full-text search over name, definition and defining term — the same
    /// weighting the migration's `search_vector` encodes.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn search_metrics(&self, query: &str) -> Result<Vec<MetricRecord>, StorageError>;

    // ---- Epic 24 Slice F: metric lineage reconciliation ----

    /// Replace what a metric declares as its sources.
    ///
    /// **Scoped to `metric_sources`, not `lineage_edges`.** A metric is not
    /// an asset — `lineage_edges.to_asset_id` has a hard FK to `assets(id)`
    /// — so this reconciles the metric's own claim about its sources; it
    /// does not yet create a graph-traversable lineage edge. That needs a
    /// schema decision (give `Metric` an `AssetKind`, or widen
    /// `lineage_edges`' endpoint type) that is bigger than this slice.
    ///
    /// `None` if the metric does not exist.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn update_metric_sources(
        &self,
        metric_id: Uuid,
        sources: &[String],
    ) -> Result<Option<MetricRecord>, StorageError>;

    // ---- Epic 21: extraction runs and the confirmation queue ----

    /// Whether this exact document has already been through this exact
    /// extractor.
    ///
    /// **All three parts of the identity, because any one alone is wrong**: a
    /// better extractor should re-read old documents, an edited document
    /// should be re-read by the same extractor, and neither changing means
    /// there is nothing new to learn.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn find_extraction_run(
        &self,
        source_id: &str,
        fingerprint: &str,
        extractor: &str,
        version: &str,
    ) -> Result<Option<ExtractionRunRecord>, StorageError>;

    /// One run by id, for resolving a queued claim's evidence against the
    /// source text as the parser produced it.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn find_extraction_run_by_id(
        &self,
        run_id: Uuid,
    ) -> Result<Option<ExtractionRunRecord>, StorageError>;

    /// Persist a whole run — the run row, its queued claims, and its
    /// discards — in one transaction.
    ///
    /// **One transaction, because a partial run is worse than no run.** Claims
    /// written without their run row would be unattributable assertions that
    /// nothing can delete wholesale, which is the exact property decision 0
    /// buys by putting extraction in a named graph.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn save_extraction_run(
        &self,
        run: &ExtractionRunRecord,
        queued: &[QueuedClaimRecord],
        discarded: &[DiscardedClaimRecord],
    ) -> Result<(), StorageError>;

    /// Claims awaiting a human, oldest first.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn pending_extraction_claims(
        &self,
        limit: i64,
    ) -> Result<Vec<QueuedClaimRecord>, StorageError>;

    /// Record a reviewer's decision. `None` if the claim does not exist.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn decide_extraction_claim(
        &self,
        claim_id: Uuid,
        confirmed: bool,
        decided_by: &str,
    ) -> Result<Option<QueuedClaimRecord>, StorageError>;

    /// Assertions a human has already rejected, so a later run does not
    /// re-queue them.
    ///
    /// **Keyed on the assertion, not on the run**, because the re-ingestion
    /// that would re-propose a rejected claim is by definition a *different*
    /// run — matching on run id would make this never fire.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn rejected_assertions(&self) -> Result<Vec<(String, String, String)>, StorageError>;

    /// Delete a run and everything it produced. `false` if it did not exist.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn delete_extraction_run(&self, run_id: Uuid) -> Result<bool, StorageError>;

    // ---- Epic 22: organization-defined custom properties ----

    /// Define a property on an entity type.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the name is already defined **on that
    /// type** — the same name on a different type is a different property and
    /// is allowed.
    /// [`StorageError::Unexpected`] if the write fails.
    async fn define_custom_property(
        &self,
        id: Uuid,
        property: &CustomProperty,
    ) -> Result<(), StorageError>;

    /// Every definition, or only those for one entity type.
    ///
    /// **Unfiltered is the common call**, because validating a write needs the
    /// definitions for that entity's type and nothing else knows the type until
    /// the entity is in hand.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_custom_properties(
        &self,
        entity_type: Option<&str>,
    ) -> Result<Vec<(Uuid, CustomProperty)>, StorageError>;

    /// One definition by id.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_custom_property(&self, id: Uuid) -> Result<Option<CustomProperty>, StorageError>;

    /// How many entities currently hold a value for this property.
    ///
    /// **The number a `409` reports.** "Cannot delete, values exist" tells an
    /// operator nothing about whether this is a five-minute cleanup or a
    /// quarter's work; the count does.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn count_custom_property_values(
        &self,
        entity_type: &str,
        name: &str,
    ) -> Result<i64, StorageError>;

    /// Delete a definition. `false` if it did not exist.
    ///
    /// **Does not touch values** — the facade refuses the delete while any
    /// exist (decision 5), so a cascade here would turn a guarded operation
    /// into a silent one if that check were ever bypassed.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn delete_custom_property(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Every value currently held for one property, with the entity holding it.
    ///
    /// **What a guarded definition change is checked against.** Deciding
    /// whether a narrowed constraint orphans data by reasoning about the
    /// *shape* of the change — is this a widening or a narrowing? — needs a
    /// classification table that has to be right for every combination of
    /// bound, type and enum. Reading the values and re-running the write-path
    /// validator over them needs no table at all, and it cannot disagree with
    /// what a write would do, because it is the same function.
    ///
    /// Unpaged: an admin operation over one property's values, and a paged
    /// answer would let a value slip between pages while the check runs —
    /// producing an "allowed" verdict for a change that orphans it.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn custom_property_values(
        &self,
        entity_type: &str,
        name: &str,
    ) -> Result<Vec<(Uuid, serde_json::Value)>, StorageError>;

    /// Replace a definition in place. `false` if it did not exist.
    ///
    /// A change of `name` **migrates every existing value to the new key in
    /// the same transaction**. Doing it in two statements would leave a window
    /// in which the definition names a key no entity holds, and every read in
    /// that window reports the organization's field as unset.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the new name is already defined on that
    /// entity type. [`StorageError::Unexpected`] if the write fails.
    async fn update_custom_property(
        &self,
        id: Uuid,
        property: &CustomProperty,
        previous_name: &str,
    ) -> Result<bool, StorageError>;

    // ---- Epic 30: quality signals ----

    /// Register a reusable check template.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken.
    async fn create_test_definition(
        &self,
        id: Uuid,
        name: &str,
        test_type: &str,
        description: Option<&str>,
        expected_cadence: Option<&str>,
    ) -> Result<StoredTestDefinition, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_test_definitions(&self) -> Result<Vec<StoredTestDefinition>, StorageError>;

    /// Change a definition's cadence, and with it every case that has not
    /// overridden it.
    ///
    /// **This is the whole point of decision 3a.** Without the split, changing
    /// "freshness within 24 hours" to 12 means editing eight hundred rows; with
    /// it, the cases that inherited the cadence follow automatically and the
    /// ones that deliberately differ do not. Returns how many cases now resolve
    /// differently.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn set_definition_cadence(
        &self,
        id: Uuid,
        expected_cadence: Option<&str>,
    ) -> Result<Option<i64>, StorageError>;

    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken on that target.
    async fn create_test_suite(
        &self,
        id: Uuid,
        name: &str,
        owner: Option<&str>,
        description: Option<&str>,
    ) -> Result<Option<Uuid>, StorageError>;

    /// Register a case against an asset or a column. `None` when the target
    /// does not resolve, or when a named definition or suite does not exist.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken on that target.
    #[allow(clippy::too_many_arguments)]
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
    ) -> Result<Option<StoredTestCase>, StorageError>;

    /// Cases on a target, or in a suite, or all of them.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_test_cases(
        &self,
        target_fqn: Option<&str>,
        suite_id: Option<Uuid>,
    ) -> Result<Vec<StoredTestCase>, StorageError>;

    /// Delete a case and its results. `false` if it did not exist.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn delete_test_case(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Record a batch of observations.
    ///
    /// **Never bumps the entity version and emits no change event** (decision
    /// 2): a nightly suite across ten thousand tables would otherwise inflate
    /// every history with observations rather than changes, and the version
    /// tracks *descriptive* change.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn record_test_results(
        &self,
        batch: &[TestResultWrite],
    ) -> Result<ResultIngest, StorageError>;

    /// A case's results, newest first.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn test_results(
        &self,
        case_id: Uuid,
        limit: i64,
    ) -> Result<Vec<StoredTestResult>, StorageError>;

    /// The latest result per case for a target, which is what health is
    /// computed from.
    ///
    /// **Returns a row per case even when it has never run**, because a
    /// registered case with no results is a *stale* case rather than an absent
    /// one — somebody declared the check and it has produced nothing, which is
    /// worth saying.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn latest_results_for(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<graph_owl_core::quality::LatestResult>, StorageError>;

    /// Delete results older than `before`, keeping the most recent per case.
    ///
    /// **The latest survives regardless of age.** Pruning it would blank the
    /// health signal pruning exists to support, and would do it worst for
    /// exactly the infrequently-tested assets whose signal is scarcest.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn prune_test_results(
        &self,
        before: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, StorageError>;

    // ---- Epic 29 Slices D and E: column lineage and reconciliation ----

    /// Attach column-level mappings to an edge, replacing what was there.
    ///
    /// `None` when the edge does not exist, or when a named column does not.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn set_column_mappings(
        &self,
        edge_id: Uuid,
        mappings: &[ColumnMapping],
    ) -> Result<Option<i64>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn column_mappings(&self, edge_id: Uuid) -> Result<Vec<ColumnMapping>, StorageError>;

    /// Replace the edge set one source asserted **within an enumerated scope**.
    ///
    /// **Scoped by source and by prefix, and both halves matter.** Source-blind
    /// replacement silently deletes lineage a human curated — the failure this
    /// exists to prevent. Scope-blind replacement deletes edges in schemas the
    /// run never looked at, which is the same bug wearing a different hat.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if any part of the transaction fails.
    async fn reconcile_lineage(
        &self,
        source: &str,
        scope_prefix: &str,
        asserted: &[(Uuid, Uuid, String)],
        created_by: &str,
    ) -> Result<LineageReconciliation, StorageError>;

    // ---- Epic 27: data contracts ----

    /// Create a contract. `None` when the producer, a consumer or the asset
    /// does not resolve — said with an `Option` rather than a new error
    /// variant, the same shape `create_domain` uses.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn create_contract(
        &self,
        id: Uuid,
        contract: &Contract,
    ) -> Result<Option<Contract>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_contract(&self, id: Uuid) -> Result<Option<StoredContract>, StorageError>;

    /// Contracts on an asset, or every contract when `asset_fqn` is `None`.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_contracts(&self, asset_fqn: Option<&str>) -> Result<Vec<Contract>, StorageError>;

    /// Move a contract's status. `false` if it does not exist.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn set_contract_status(
        &self,
        id: Uuid,
        status: ContractStatus,
        updated_by: &str,
    ) -> Result<bool, StorageError>;

    /// Evaluate a schema change against every **enforced** contract on the
    /// asset, recording each breach and marking those contracts violated.
    ///
    /// **The change itself is never blocked** (decision 3): graph-owl observes
    /// metadata and cannot stop a warehouse `ALTER TABLE`, so refusing here
    /// would be making a promise it has no way to keep. Returns what broke, for
    /// the caller to announce.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if any part of the transaction fails.
    async fn evaluate_schema_change(
        &self,
        asset_fqn: &str,
        change: &SchemaChange,
        asset_version: &str,
    ) -> Result<Vec<BreachReport>, StorageError>;

    /// Clear a contract's breaches and return it to `Active`.
    ///
    /// **Explicit, never automatic.** A later compatible change does not clear
    /// an earlier breach — the incident happened, and silent clearing would let
    /// a producer break something on Monday and look clean on Tuesday.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn clear_contract_breaches(
        &self,
        id: Uuid,
        updated_by: &str,
    ) -> Result<Option<i64>, StorageError>;

    // ---- Epic 28: usage and popularity ----

    /// Record a batch of observations and fold them into the daily rollups.
    ///
    /// **Rollups are updated incrementally here, not rebuilt.** Re-scanning raw
    /// rows at warehouse scale is the thing decision 4 prunes them to avoid.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn record_usage(&self, batch: &[UsageWrite]) -> Result<UsageIngest, StorageError>;

    /// Daily rollups for an asset, newest day first.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn usage_rollups(&self, asset_fqn: &str) -> Result<Vec<UsageRollup>, StorageError>;

    /// When an asset was last used, kept separately so pruning cannot erase it.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn last_accessed(
        &self,
        asset_fqn: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, StorageError>;

    /// Rebuild an asset's rollups from its raw observations.
    ///
    /// **Exists to be compared against the incremental path**, which is the
    /// only way to know that path is correct — Slice B's equivalence test. Not
    /// a repair tool: after pruning the raw rows are gone and a rebuild would
    /// produce *less* than the truth, which is why nothing calls it in
    /// production.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read or write fails.
    async fn rebuild_usage_rollups(&self, asset_fqn: &str) -> Result<i64, StorageError>;

    /// Delete raw observations older than `before`, keeping the most recent one
    /// per asset.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn prune_usage(&self, before: chrono::DateTime<chrono::Utc>)
    -> Result<i64, StorageError>;

    /// Re-key observations and rollups from an opaque identifier to a
    /// principal.
    ///
    /// **Retroactive by design** (Slice D): creating a matching `User` later
    /// should reclassify the history rather than starting a second count beside
    /// it. Returns how many rollup rows moved.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn resolve_usage_consumer(
        &self,
        identifier: &str,
        principal_id: &str,
    ) -> Result<i64, StorageError>;

    // ---- Epic 25: tags and classifications ----

    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken.
    async fn create_classification(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        mutually_exclusive: bool,
        updated_by: &str,
    ) -> Result<Classification, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_classification(&self, id: Uuid) -> Result<Option<Classification>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_classifications(&self) -> Result<Vec<Classification>, StorageError>;

    /// Delete a classification. `Err(count)` when it still has tags and
    /// `recursive` was not given — the number, because "it has tags" tells an
    /// operator nothing about the size of the cleanup.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn delete_classification(
        &self,
        id: Uuid,
        recursive: bool,
    ) -> Result<Result<bool, i64>, StorageError>;

    /// Create a tag under a classification. `None` when the classification does
    /// not exist.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken **on that
    /// classification** — the same name under a different one is a different
    /// tag and inserts fine.
    async fn create_tag(
        &self,
        id: Uuid,
        classification_id: Uuid,
        name: &str,
        description: Option<&str>,
        updated_by: &str,
    ) -> Result<Option<Tag>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_tag_by_fqn(&self, fqn: &str) -> Result<Option<Tag>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_tags(&self, classification_id: Option<Uuid>) -> Result<Vec<Tag>, StorageError>;

    /// Apply a tag to something, with provenance.
    ///
    /// Enforces exclusivity, idempotence and the rejection ledger — all three
    /// here rather than in the facade, because they are one decision about one
    /// row and splitting them would open a read-then-write race on every one.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn apply_tag(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        label_type: LabelType,
        state: LabelState,
        applied_by: &str,
    ) -> Result<LabelOutcome, StorageError>;

    /// Remove a label. `false` if it was not there.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn remove_tag(&self, tag_fqn: &str, target_fqn: &str) -> Result<bool, StorageError>;

    /// Every label on a target, newest first.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn labels_on(&self, target_fqn: &str) -> Result<Vec<TagLabel>, StorageError>;

    /// Confirm or reject a suggested label.
    ///
    /// **A rejection is recorded, not merely removed.** A rejection that
    /// vanished would be re-proposed by the next run of the same scanner, and a
    /// steward would answer the same question forever.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn decide_label(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        confirmed: bool,
        decided_by: &str,
    ) -> Result<LabelDecision, StorageError>;

    /// Targets carrying a suggested label — the steward triage queue.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn suggested_labels(&self, limit: i64) -> Result<Vec<TagLabel>, StorageError>;

    /// How many live entities carry a tag, by kind.
    ///
    /// **Soft-deleted entities do not count.** A tombstoned column does not
    /// keep a governance label alive, and counting it would refuse a delete
    /// over data nobody can see.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn tag_usage(&self, tag_fqn: &str) -> Result<TagUsage, StorageError>;

    /// Delete a tag and, when forced, every label of it — transactionally,
    /// bumping each affected entity's version. Returns how many labels went.
    ///
    /// `None` when the tag does not exist.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if any part of the transaction fails.
    async fn delete_tag(
        &self,
        tag_fqn: &str,
        force: bool,
        updated_by: &str,
    ) -> Result<Option<i64>, StorageError>;

    /// Apply a tag to a target's children, respecting precedence.
    ///
    /// Returns how many children gained or kept a label because of this call.
    /// **A manual label on a child is never downgraded** — a steward's
    /// deliberate choice survives, and relabelling it `Propagated` would also
    /// be a lie about where it came from.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn propagate_tag(
        &self,
        tag_fqn: &str,
        target_fqn: &str,
        recursive: bool,
        applied_by: &str,
    ) -> Result<i64, StorageError>;

    // ---- Epic 26: lifecycle and certification ----

    /// Move an asset's lifecycle state, refusing a move the machine forbids.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn set_lifecycle(
        &self,
        asset_id: Uuid,
        to: LifecycleState,
        deprecation: Option<&Deprecation>,
        updated_by: &str,
    ) -> Result<LifecycleOutcome, StorageError>;

    /// Follow a deprecation chain to the first asset that is not itself
    /// deprecated or retired.
    ///
    /// **Bounded and cycle-safe**: a successor loop is a configuration mistake
    /// somebody will make, and an unbounded walk turns it into a hung request.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn terminal_successor(&self, fqn: &str) -> Result<Option<Asset>, StorageError>;

    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken.
    #[allow(clippy::too_many_arguments)]
    async fn create_certification_type(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        default_validity_days: i32,
        required_evidence: &[String],
        authorized_issuers: &[String],
        updated_by: &str,
    ) -> Result<StoredCertificationType, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_certification_types(&self) -> Result<Vec<StoredCertificationType>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_certification_type(
        &self,
        id: Uuid,
    ) -> Result<Option<StoredCertificationType>, StorageError>;

    /// Issue a certification, enforcing the issuer allowlist and the required
    /// evidence.
    ///
    /// A second issuance of the same type on the same target **supersedes**
    /// rather than accumulating, so "when does my Gold expire" has one answer —
    /// and the superseded row stays, because who vouched for what and when is
    /// the point of having certification at all.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    #[allow(clippy::too_many_arguments)]
    async fn issue_certification(
        &self,
        id: Uuid,
        target_fqn: &str,
        type_id: Uuid,
        issuer: &str,
        criteria: Option<&str>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        evidence: &[(String, String)],
    ) -> Result<IssueOutcome, StorageError>;

    /// Live certifications on a target — superseded ones excluded.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn certifications_on(
        &self,
        target_fqn: &str,
    ) -> Result<Vec<StoredCertification>, StorageError>;

    /// Live certifications expiring before an instant — the recertification
    /// queue.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn certifications_expiring_before(
        &self,
        instant: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<StoredCertification>, StorageError>;

    // ---- Epic 23: domains and data products ----

    /// Create a domain. The FQN is derived by the adapter from the parent
    /// chain, so a caller cannot make the path and the parent disagree.
    ///
    /// `None` means `parent_id` named a domain that does not exist — said with
    /// an `Option` rather than a new `StorageError` variant, because that is
    /// the shape every other lookup in this port already uses and a variant
    /// would ripple through every adapter for one case.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the derived FQN is taken.
    #[allow(clippy::too_many_arguments)]
    async fn create_domain(
        &self,
        id: Uuid,
        name: &str,
        parent_id: Option<Uuid>,
        description: Option<&str>,
        domain_type: Option<&str>,
        experts: &[String],
        updated_by: &str,
    ) -> Result<Option<Domain>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_domain(&self, id: Uuid) -> Result<Option<Domain>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_domain_by_fqn(&self, fqn: &str) -> Result<Option<Domain>, StorageError>;

    /// Live domains, name-ordered.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_domains(&self, page: &PageRequest) -> Result<Page<Domain>, StorageError>;

    /// Apply a partial update, advancing the version by the size of the change.
    ///
    /// **Renaming re-derives the whole subtree's FQNs**, transactionally. A
    /// rename that moved only its own path would leave every descendant
    /// claiming to sit under a domain that no longer has that name, and every
    /// FQN lookup below it would then miss.
    ///
    /// # Errors
    /// [`StorageError::Conflict`] if the new path is taken, or if reparenting
    /// would close a cycle.
    async fn update_domain(
        &self,
        id: Uuid,
        update: &DomainUpdate,
        updated_by: &str,
    ) -> Result<Option<Domain>, StorageError>;

    /// Whether making `parent` the parent of `domain` would close a cycle.
    ///
    /// **Walks the proposed parent's whole ancestry, not its immediate
    /// parent.** A depth-1 check passes `A → B → C → A` and leaves an ancestor
    /// walk that never terminates.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn domain_would_cycle(&self, domain: Uuid, parent: Uuid) -> Result<bool, StorageError>;

    /// Direct children of `parent`, or the roots when `None`.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn child_domains(&self, parent: Option<Uuid>) -> Result<Vec<Domain>, StorageError>;

    /// Assign an asset to a domain directly, or clear the assignment.
    ///
    /// Returns the asset as it stands after the write, or `NotFound`. Clearing
    /// does not make the asset domainless — it makes it *inherit* again, which
    /// is a different and usually better answer.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn assign_asset_domain(
        &self,
        asset_id: Uuid,
        domain_id: Option<Uuid>,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError>;

    /// The domain an asset falls under, directly or by inheritance.
    ///
    /// **The nearest assigned ancestor wins, and the walk stops there.**
    /// Accumulating every assigned ancestor would answer "which domains is this
    /// under" — a question with several answers, which is exactly the shared
    /// accountability decision 1 refuses.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn resolve_asset_domain(
        &self,
        asset_id: Uuid,
    ) -> Result<Option<DomainAssignment>, StorageError>;

    /// How many live assets resolve to `domain`, directly or by inheritance.
    ///
    /// The number a reassignment reports. Counting only direct assignments
    /// would tell an operator a database moved one asset when it moved five
    /// thousand.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn count_assets_in_domain(&self, domain: Uuid) -> Result<i64, StorageError>;

    /// Delete a domain, refusing while it holds things unless a target is
    /// named.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if any part of the transaction fails.
    async fn delete_domain(
        &self,
        id: Uuid,
        reassign_to: Option<Uuid>,
        updated_by: &str,
    ) -> Result<DomainDeletion, StorageError>;

    /// # Errors
    /// [`StorageError::Conflict`] if the name is taken.
    async fn create_data_product(
        &self,
        id: Uuid,
        name: &str,
        description: Option<&str>,
        purpose: Option<&str>,
        domain_id: Option<Uuid>,
        updated_by: &str,
    ) -> Result<DataProduct, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_data_product(&self, id: Uuid) -> Result<Option<DataProduct>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_data_products(
        &self,
        page: &PageRequest,
    ) -> Result<Page<DataProduct>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn update_data_product(
        &self,
        id: Uuid,
        update: &DataProductUpdate,
        updated_by: &str,
    ) -> Result<Option<DataProduct>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn delete_data_product(&self, id: Uuid) -> Result<bool, StorageError>;

    /// Add an asset to a product. Idempotent: adding it twice is one edge.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn add_product_asset(
        &self,
        product_id: Uuid,
        asset_id: Uuid,
    ) -> Result<Result<(), MembershipRefusal>, StorageError>;

    /// Remove an asset from a product. **Never deletes the asset** — a product
    /// is a view of things that exist independently of it.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn remove_product_asset(
        &self,
        product_id: Uuid,
        asset_id: Uuid,
    ) -> Result<bool, StorageError>;

    /// The assets in a product, paginated.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn product_assets(
        &self,
        product_id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Asset>, StorageError>;

    /// The products an asset belongs to.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn asset_products(&self, asset_id: Uuid) -> Result<Vec<DataProduct>, StorageError>;

    /// Delete a definition **and every value of it**, transactionally, bumping
    /// the version of each entity that held one. Returns how many entities
    /// changed.
    ///
    /// The version bump is what makes this honest rather than merely tidy: an
    /// entity whose `costCenter` vanished has changed, and a history that does
    /// not say so leaves a consumer unable to explain when the field stopped
    /// being there.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if any part of the transaction fails.
    async fn force_delete_custom_property(
        &self,
        id: Uuid,
        entity_type: &str,
        name: &str,
        updated_by: &str,
    ) -> Result<i64, StorageError>;

    // ---- Epic 32: agent capabilities ----

    /// Write or replace an agent's grant. **Human-managed only** — no MCP tool
    /// reaches this, and `graph-owl-core::agent::authorize_forbidden` refuses
    /// grant management unconditionally, so the absence is enforced twice.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn upsert_agent_grant(
        &self,
        grant: &graph_owl_authz::agent::AgentGrant,
    ) -> Result<(), StorageError>;

    /// `None` when this agent has no grant, which refuses everything.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn agent_grant(
        &self,
        agent_id: &str,
    ) -> Result<Option<graph_owl_authz::agent::AgentGrant>, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_agent_grants(
        &self,
    ) -> Result<Vec<graph_owl_authz::agent::AgentGrant>, StorageError>;

    /// `false` when there was no grant to revoke.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the delete fails.
    async fn revoke_agent_grant(&self, agent_id: &str) -> Result<bool, StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn create_proposal(
        &self,
        proposal: &graph_owl_authz::agent::Proposal,
    ) -> Result<(), StorageError>;

    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn get_proposal(
        &self,
        id: Uuid,
    ) -> Result<Option<graph_owl_authz::agent::Proposal>, StorageError>;

    /// An agent's proposals, newest first — **so a steward can review an
    /// agent's track record** rather than only its individual suggestions.
    /// `None` for `agent_id` lists everyone's.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn list_proposals(
        &self,
        agent_id: Option<&str>,
        status: Option<graph_owl_authz::agent::ProposalStatus>,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_authz::agent::Proposal>, StorageError>;

    /// Record a decision. `false` when the proposal does not exist or was
    /// already decided — **deciding twice is a conflict, not an update**: two
    /// reviewers reaching opposite conclusions must not have the second
    /// silently win.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn decide_proposal(
        &self,
        id: Uuid,
        status: graph_owl_authz::agent::ProposalStatus,
        decided_by: &str,
    ) -> Result<bool, StorageError>;

    /// Append one line to the agent's history — **including refusals**.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the write fails.
    async fn record_agent_activity(
        &self,
        activity: &graph_owl_authz::agent::AgentActivity,
    ) -> Result<(), StorageError>;

    /// One agent's history, newest first.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn agent_activity(
        &self,
        agent_id: &str,
        page: &PageRequest,
    ) -> Result<Page<graph_owl_authz::agent::AgentActivity>, StorageError>;

    /// How many writes of one capability this agent made inside the window, and
    /// how long ago the oldest of them was.
    ///
    /// **This is what makes the rate limit survive a restart.** An in-process
    /// counter resets on deploy, which is precisely when a runaway agent would
    /// get its budget back.
    ///
    /// Returns `(count, oldest_age_seconds)`.
    ///
    /// # Errors
    /// [`StorageError::Unexpected`] if the read fails.
    async fn agent_writes_in_window(
        &self,
        agent_id: &str,
        capability: graph_owl_authz::agent::AgentCapability,
        window_seconds: u32,
    ) -> Result<(u32, Option<u64>), StorageError>;
}

/// A stored extraction run.
///
/// Carries `source_text` because every evidence span is an offset into *that
/// string*. Resolving spans against a re-read of the original document would
/// drift silently the moment anyone edits it, and a reviewer shown the current
/// text of an edited sentence is being shown something the extractor never saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionRunRecord {
    pub id: Uuid,
    pub source_id: String,
    pub source_fingerprint: String,
    pub extractor: String,
    pub extractor_version: String,
    pub source_text: String,
    pub media_type: String,
    pub asserted: i32,
    pub surfaced: i32,
    pub discarded: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueuedClaimRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f64,
    pub evidence_start: i32,
    pub evidence_end: i32,
    pub state: String,
    pub decided_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscardedClaimRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f64,
    pub reason: String,
}
