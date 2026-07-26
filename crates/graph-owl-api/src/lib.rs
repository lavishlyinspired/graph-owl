use std::sync::Arc;

use chrono::Utc;
use graph_owl_core::{
    Relationship, Table, TableUpdate,
    page::{Cursor, Page, PageRequest},
};
use graph_owl_storage::{Storage, StorageError};
use serde::Deserialize;
use uuid::Uuid;

pub mod validation;
use validation::{FieldError, FieldPath, ValidateBody, optional_string, require_non_empty_string};

#[derive(Debug, Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub fully_qualified_name: String,
    pub description: Option<String>,
}

impl ValidateBody for CreateTable {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        require_non_empty_string(
            value,
            &FieldPath::root().key("fully_qualified_name"),
            &mut errors,
        );
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateRelationship {
    pub to_table_id: Uuid,
    pub relationship_type: String,
}

/// PATCH semantics: every field is optional, so absence is never an error.
/// But a field the client *did* send must still be usable — `name: ""` is a
/// request to blank a required value, not a no-op.
impl ValidateBody for TableUpdate {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        if value.get("name").is_some_and(|v| !v.is_null()) {
            require_non_empty_string(value, &FieldPath::root().key("name"), &mut errors);
        }
        optional_string(value, &FieldPath::root().key("description"), &mut errors);
        errors
    }
}

impl ValidateBody for CreateRelationship {
    fn validate_body(value: &serde_json::Value) -> Vec<FieldError> {
        let mut errors = Vec::new();
        require_non_empty_string(value, &FieldPath::root().key("to_table_id"), &mut errors);
        require_non_empty_string(
            value,
            &FieldPath::root().key("relationship_type"),
            &mut errors,
        );
        errors
    }
}

#[derive(Debug)]
pub enum CreateRelationshipError {
    InvalidRelationshipType,
    TableNotFound,
    Storage(StorageError),
}

impl From<StorageError> for CreateRelationshipError {
    fn from(error: StorageError) -> Self {
        CreateRelationshipError::Storage(error)
    }
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
    pub async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
        self.storage.list_tables(page).await
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn update_table(
        &self,
        id: Uuid,
        update: TableUpdate,
    ) -> Result<Option<Table>, StorageError> {
        self.storage.update_table(id, update).await
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
        self.storage.delete_table(id).await
    }

    /// # Errors
    ///
    /// Returns `CreateRelationshipError::InvalidRelationshipType` if `relationship_type` is
    /// empty, `CreateRelationshipError::TableNotFound` if either table doesn't exist, or
    /// `CreateRelationshipError::Storage` if the underlying storage fails (e.g. a duplicate
    /// relationship).
    pub async fn create_relationship(
        &self,
        from_table_id: Uuid,
        request: CreateRelationship,
    ) -> Result<Relationship, CreateRelationshipError> {
        if request.relationship_type.is_empty() {
            return Err(CreateRelationshipError::InvalidRelationshipType);
        }

        if self.storage.get_table(from_table_id).await?.is_none() {
            return Err(CreateRelationshipError::TableNotFound);
        }
        if self.storage.get_table(request.to_table_id).await?.is_none() {
            return Err(CreateRelationshipError::TableNotFound);
        }

        let relationship = Relationship {
            id: Uuid::new_v4(),
            from_entity_type: "table".to_string(),
            from_entity_id: from_table_id,
            relationship_type: request.relationship_type,
            to_entity_type: "table".to_string(),
            to_entity_id: request.to_table_id,
            created_at: Utc::now(),
        };

        Ok(self.storage.create_relationship(relationship).await?)
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails. Returns `Ok(None)` if the table
    /// itself doesn't exist.
    pub async fn list_relationships_for_table(
        &self,
        table_id: Uuid,
    ) -> Result<Option<Vec<Relationship>>, StorageError> {
        if self.storage.get_table(table_id).await?.is_none() {
            return Ok(None);
        }

        let relationships = self
            .storage
            .list_relationships_for_entity("table", table_id)
            .await?;
        Ok(Some(relationships))
    }

    /// # Errors
    ///
    /// Returns an error if the underlying storage fails.
    pub async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
        self.storage.delete_relationship(id).await
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
        relationships: Mutex<Vec<Relationship>>,
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

        async fn list_tables(&self, page: &PageRequest) -> Result<Page<Table>, StorageError> {
            // The fake honours the same ordering and keyset contract as the
            // Postgres adapter. A fake that returns insertion order would let
            // a pagination bug pass here and fail only against a real database,
            // which is the whole failure mode a port is supposed to prevent.
            let mut tables = self.inserted.lock().unwrap().clone();
            tables.sort_by(|a, b| {
                a.fully_qualified_name
                    .cmp(&b.fully_qualified_name)
                    .then(a.id.cmp(&b.id))
            });
            if let Some(cursor) = &page.after {
                tables.retain(|t| {
                    (t.fully_qualified_name.as_str(), t.id) > (cursor.sort_key.as_str(), cursor.id)
                });
            }
            tables.truncate(page.limit + 1);
            Ok(Page::from_overfetch(tables, page.limit, |t| {
                Cursor::new(t.fully_qualified_name.clone(), t.id)
            }))
        }

        async fn update_table(
            &self,
            id: Uuid,
            update: TableUpdate,
        ) -> Result<Option<Table>, StorageError> {
            let mut inserted = self.inserted.lock().unwrap();
            let Some(table) = inserted.iter_mut().find(|table| table.id == id) else {
                return Ok(None);
            };
            if let Some(name) = update.name {
                table.name = name;
            }
            if let Some(description) = update.description {
                table.description = Some(description);
            }
            table.updated_at = Utc::now();
            Ok(Some(table.clone()))
        }

        async fn delete_table(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut inserted = self.inserted.lock().unwrap();
            let original_len = inserted.len();
            inserted.retain(|table| table.id != id);
            Ok(inserted.len() != original_len)
        }

        async fn create_relationship(
            &self,
            relationship: Relationship,
        ) -> Result<Relationship, StorageError> {
            self.relationships
                .lock()
                .unwrap()
                .push(relationship.clone());
            Ok(relationship)
        }

        async fn list_relationships_for_entity(
            &self,
            entity_type: &str,
            entity_id: Uuid,
        ) -> Result<Vec<Relationship>, StorageError> {
            Ok(self
                .relationships
                .lock()
                .unwrap()
                .iter()
                .filter(|relationship| {
                    (relationship.from_entity_type == entity_type
                        && relationship.from_entity_id == entity_id)
                        || (relationship.to_entity_type == entity_type
                            && relationship.to_entity_id == entity_id)
                })
                .cloned()
                .collect())
        }

        async fn delete_relationship(&self, id: Uuid) -> Result<bool, StorageError> {
            let mut relationships = self.relationships.lock().unwrap();
            let original_len = relationships.len();
            relationships.retain(|relationship| relationship.id != id);
            Ok(relationships.len() != original_len)
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

        let page = catalog
            .list_tables(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_tables should succeed");

        assert_eq!(page.data, Vec::new());
        assert_eq!(page.paging.after, None);
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

        let page = catalog
            .list_tables(&PageRequest::new(None, None).expect("valid"))
            .await
            .expect("list_tables should succeed");

        // Sorted by FQN, so the order is the contract's, not insertion order.
        let mut expected = vec![first, second];
        expected.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));
        assert_eq!(page.data, expected);
        assert_eq!(page.paging.after, None, "both rows fit in one page");
    }

    #[tokio::test]
    async fn updating_a_table_changes_only_the_provided_fields() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let updated = catalog
            .update_table(
                created.id,
                TableUpdate {
                    name: None,
                    description: Some("a new description".to_string()),
                },
            )
            .await
            .expect("update_table should succeed")
            .expect("table should exist");

        assert_eq!(updated.name, created.name);
        assert_eq!(updated.description, Some("a new description".to_string()));
        assert_eq!(updated.created_at, created.created_at);
    }

    #[tokio::test]
    async fn updating_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .update_table(Uuid::new_v4(), TableUpdate::default())
            .await
            .expect("update_table should succeed");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deleting_an_existing_table_removes_it_and_returns_true() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let created = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let deleted = catalog
            .delete_table(created.id)
            .await
            .expect("delete_table should succeed");

        assert!(deleted);
        let found = catalog
            .get_table(created.id)
            .await
            .expect("get_table should succeed");
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn deleting_a_nonexistent_table_returns_false() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let deleted = catalog
            .delete_table(Uuid::new_v4())
            .await
            .expect("delete_table should succeed");

        assert!(!deleted);
    }

    #[tokio::test]
    async fn creating_a_relationship_between_two_existing_tables_succeeds() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let relationship = catalog
            .create_relationship(
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derived_from".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        assert_eq!(relationship.from_entity_type, "table");
        assert_eq!(relationship.from_entity_id, from.id);
        assert_eq!(relationship.to_entity_type, "table");
        assert_eq!(relationship.to_entity_id, to.id);
        assert_eq!(relationship.relationship_type, "derived_from");
    }

    #[tokio::test]
    async fn creating_a_relationship_from_a_nonexistent_table_returns_table_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let to = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                Uuid::new_v4(),
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derived_from".to_string(),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(CreateRelationshipError::TableNotFound)
        ));
    }

    #[tokio::test]
    async fn creating_a_relationship_to_a_nonexistent_table_returns_table_not_found() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                from.id,
                CreateRelationship {
                    to_table_id: Uuid::new_v4(),
                    relationship_type: "derived_from".to_string(),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(CreateRelationshipError::TableNotFound)
        ));
    }

    #[tokio::test]
    async fn creating_a_relationship_with_empty_relationship_type_returns_invalid_relationship_type()
     {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let result = catalog
            .create_relationship(
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: String::new(),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(CreateRelationshipError::InvalidRelationshipType)
        ));
    }

    #[tokio::test]
    async fn listing_relationships_for_a_table_with_none_returns_an_empty_vec() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let table = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");

        let relationships = catalog
            .list_relationships_for_table(table.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");

        assert_eq!(relationships, Vec::new());
    }

    #[tokio::test]
    async fn listing_relationships_for_a_table_returns_relationships_from_either_side() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let orders = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let customers = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let archive = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        catalog
            .create_relationship(
                orders.id,
                CreateRelationship {
                    to_table_id: customers.id,
                    relationship_type: "derived_from".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");
        catalog
            .create_relationship(
                archive.id,
                CreateRelationship {
                    to_table_id: orders.id,
                    relationship_type: "derived_from".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        let relationships = catalog
            .list_relationships_for_table(orders.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");

        assert_eq!(relationships.len(), 2);
    }

    #[tokio::test]
    async fn listing_relationships_for_a_nonexistent_table_returns_none() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let result = catalog
            .list_relationships_for_table(Uuid::new_v4())
            .await
            .expect("list_relationships_for_table should succeed");

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn deleting_an_existing_relationship_removes_it_and_returns_true() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));
        let from = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let to = catalog
            .create_table(mock_create_table_request())
            .await
            .expect("create_table should succeed");
        let relationship = catalog
            .create_relationship(
                from.id,
                CreateRelationship {
                    to_table_id: to.id,
                    relationship_type: "derived_from".to_string(),
                },
            )
            .await
            .expect("create_relationship should succeed");

        let deleted = catalog
            .delete_relationship(relationship.id)
            .await
            .expect("delete_relationship should succeed");

        assert!(deleted);
        let remaining = catalog
            .list_relationships_for_table(from.id)
            .await
            .expect("list_relationships_for_table should succeed")
            .expect("table should exist");
        assert_eq!(remaining, Vec::new());
    }

    #[tokio::test]
    async fn deleting_a_nonexistent_relationship_returns_false() {
        let catalog = Catalog::new(Arc::new(InMemoryStorage::default()));

        let deleted = catalog
            .delete_relationship(Uuid::new_v4())
            .await
            .expect("delete_relationship should succeed");

        assert!(!deleted);
    }
}
