use async_trait::async_trait;
use graph_owl_core::{
    Asset, AssetKind, Relationship, Table, TableUpdate,
    page::{Page, PageRequest},
};
use thiserror::Error;
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
    async fn count_assets_by_kind(&self) -> Result<Vec<(AssetKind, i64)>, StorageError>;
}
