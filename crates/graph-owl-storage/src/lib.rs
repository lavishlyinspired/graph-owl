use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
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
    async fn list_assets_visible(
        &self,
        kind: Option<AssetKind>,
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
}
