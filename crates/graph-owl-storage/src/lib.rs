use async_trait::async_trait;
use graph_owl_core::Table;
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
}
