use async_trait::async_trait;
use graph_owl_core::{Relationship, Table, TableUpdate};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("conflict: {0}")]
    Conflict(String),
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
}
