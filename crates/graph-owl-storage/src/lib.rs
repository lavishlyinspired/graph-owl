use async_trait::async_trait;
use graph_owl_core::{Relationship, Table, TableUpdate};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    /// A uniqueness constraint rejected the write. `existing_id` names the row
    /// that was already there when the adapter can identify it, so a client can
    /// act on the collision instead of guessing what it hit.
    #[error("conflict: {detail}")]
    Conflict {
        detail: String,
        existing_id: Option<Uuid>,
    },
    #[error("unexpected storage error: {0}")]
    Unexpected(String),
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn insert_table(&self, table: Table) -> Result<Table, StorageError>;
    async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError>;
    async fn list_tables(&self) -> Result<Vec<Table>, StorageError>;
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
}
