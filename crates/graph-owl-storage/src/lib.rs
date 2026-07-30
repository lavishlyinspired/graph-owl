use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    contradiction::Review,
    memory::Memory,
    ownership::{EntityReference, OwnerRef},
    page::{Page, PageRequest},
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

    // ---- asset hierarchy (Epic 2) ----

    /// Inserts, or updates in place if the FQN is already known.
    ///
    /// Upsert rather than insert because a connector re-run must converge
    /// (`15-connectors.md` decision 3): the second run over an unchanged source
    /// has to be a no-op, not a wall of conflicts.
    async fn upsert_asset(&self, asset: Asset) -> Result<Asset, StorageError>;
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

    async fn search_assets_visible(
        &self,
        query: &str,
        kind: Option<AssetKind>,
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
}
