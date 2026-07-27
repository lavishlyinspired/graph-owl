use async_trait::async_trait;
use graph_owl_authz::{AccessPredicate, Policy};
use graph_owl_core::{
    Asset, AssetKind, AssetUpdate, AssetVersion, Relationship, Table, TableUpdate,
    page::{Page, PageRequest},
};
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

#[async_trait]
pub trait Storage: Send + Sync {
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
    async fn update_asset(
        &self,
        id: Uuid,
        update: &AssetUpdate,
        updated_by: &str,
    ) -> Result<Option<Asset>, StorageError>;

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
