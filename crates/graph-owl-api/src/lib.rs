use std::sync::Arc;

use chrono::Utc;
use graph_owl_core::Table;
use graph_owl_storage::{Storage, StorageError};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub fully_qualified_name: String,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct Catalog {
    storage: Arc<dyn Storage>,
}

impl Catalog {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails, e.g. a duplicate `fully_qualified_name`.
    pub async fn create_table(&self, request: CreateTable) -> Result<Table, StorageError> {
        let now = Utc::now();
        let table = Table {
            id: Uuid::new_v4(),
            name: request.name,
            fully_qualified_name: request.fully_qualified_name,
            description: request.description,
            created_at: now,
            updated_at: now,
        };
        self.storage.insert_table(table).await
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn get_table(&self, id: Uuid) -> Result<Option<Table>, StorageError> {
        self.storage.get_table(id).await
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn list_tables(&self) -> Result<Vec<Table>, StorageError> {
        self.storage.list_tables().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_storage::Storage;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct InMemoryStorage {
        inserted: Mutex<Vec<Table>>,
    }

    #[async_trait::async_trait]
    impl Storage for InMemoryStorage {
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

        async fn list_tables(&self) -> Result<Vec<Table>, StorageError> {
            Ok(self.inserted.lock().unwrap().clone())
        }
    }

    fn mock_create_table_request() -> CreateTable {
        CreateTable {
            name: "customers".to_string(),
            fully_qualified_name: "warehouse.public.customers".to_string(),
            description: None,
        }
    }

    #[tokio::test]
    async fn creating_a_table_assigns_matching_created_and_updated_timestamps() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let table = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        assert_eq!(table.name, "customers");
        assert_eq!(table.fully_qualified_name, "warehouse.public.customers");
        assert_eq!(table.created_at, table.updated_at);
    }

    #[tokio::test]
    async fn creating_two_tables_assigns_different_ids() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let first = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let second = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        assert_ne!(first.id, second.id);
    }

    #[tokio::test]
    async fn getting_a_table_by_id_returns_the_stored_table() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let found = catalog
            .get_table(created.id)
            .await
            .expect("get_table should succeed");

        assert_eq!(found, Some(created));
    }

    #[tokio::test]
    async fn getting_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let found = catalog
            .get_table(Uuid::new_v4())
            .await
            .expect("get_table should succeed");

        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn listing_tables_with_none_created_returns_an_empty_vec() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let tables = catalog
            .list_tables()
            .await
            .expect("list_tables should succeed");

        assert_eq!(tables, Vec::new());
    }

    #[tokio::test]
    async fn listing_tables_returns_all_created_tables() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let first = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let second = catalog
            .create_table(CreateTable {
                fully_qualified_name: "warehouse.public.orders".to_string(),
                ..mock_create_table_request()
            })
            .await
            .expect("create_table should succeed");

        let mut tables = catalog
            .list_tables()
            .await
            .expect("list_tables should succeed");
        tables.sort_by_key(|table| table.id);
        let mut expected = vec![first, second];
        expected.sort_by_key(|table| table.id);

        assert_eq!(tables, expected);
    }
}
