mod common;

use chrono::Utc;
use graph_owl_core::Relationship;
use graph_owl_storage::{Storage, StorageError};
use graph_owl_storage_postgres::PostgresStorage;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use uuid::Uuid;

fn mock_relationship() -> Relationship {
    Relationship {
        id: Uuid::new_v4(),
        from_entity_type: "table".to_string(),
        from_entity_id: Uuid::new_v4(),
        relationship_type: "derived_from".to_string(),
        to_entity_type: "table".to_string(),
        to_entity_id: Uuid::new_v4(),
        created_at: Utc::now(),
    }
}

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;

    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");

    (storage, database, connection_string)
}

#[tokio::test]
async fn creating_a_relationship_persists_it_in_postgres() {
    let (storage, _container, connection_string) = test_storage().await;
    let relationship = mock_relationship();

    let created = storage
        .create_relationship(relationship.clone())
        .await
        .expect("create_relationship should succeed");

    assert_eq!(created, relationship);

    let verification_pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("failed to open verification connection");
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM entity_relationships WHERE id = $1")
        .bind(relationship.id)
        .fetch_one(&verification_pool)
        .await
        .expect("verification query should succeed");
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn creating_a_duplicate_relationship_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let first = mock_relationship();
    let second = Relationship {
        id: Uuid::new_v4(),
        ..first.clone()
    };

    storage
        .create_relationship(first)
        .await
        .expect("first create should succeed");
    let result = storage.create_relationship(second).await;

    assert!(matches!(result, Err(StorageError::Conflict { .. })));
}

#[tokio::test]
async fn creating_a_relationship_with_an_empty_relationship_type_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let relationship = Relationship {
        relationship_type: String::new(),
        ..mock_relationship()
    };

    let result = storage.create_relationship(relationship).await;

    assert!(matches!(result, Err(StorageError::Unexpected(_))));
}

#[tokio::test]
async fn listing_relationships_for_an_entity_with_none_returns_an_empty_vec() {
    let (storage, _container, _connection_string) = test_storage().await;

    let relationships = storage
        .list_relationships_for_entity("table", Uuid::new_v4())
        .await
        .expect("list_relationships_for_entity should succeed");

    assert_eq!(relationships, Vec::new());
}

#[tokio::test]
async fn listing_relationships_returns_ones_where_the_entity_is_the_from_side() {
    let (storage, _container, _connection_string) = test_storage().await;
    let relationship = mock_relationship();
    storage
        .create_relationship(relationship.clone())
        .await
        .expect("create_relationship should succeed");

    let found = storage
        .list_relationships_for_entity("table", relationship.from_entity_id)
        .await
        .expect("list_relationships_for_entity should succeed");

    assert_eq!(found, vec![relationship]);
}

#[tokio::test]
async fn listing_relationships_returns_ones_where_the_entity_is_the_to_side() {
    let (storage, _container, _connection_string) = test_storage().await;
    let relationship = mock_relationship();
    storage
        .create_relationship(relationship.clone())
        .await
        .expect("create_relationship should succeed");

    let found = storage
        .list_relationships_for_entity("table", relationship.to_entity_id)
        .await
        .expect("list_relationships_for_entity should succeed");

    assert_eq!(found, vec![relationship]);
}

#[tokio::test]
async fn listing_relationships_does_not_return_unrelated_entities_relationships() {
    let (storage, _container, _connection_string) = test_storage().await;
    let relationship = mock_relationship();
    storage
        .create_relationship(relationship)
        .await
        .expect("create_relationship should succeed");

    let found = storage
        .list_relationships_for_entity("table", Uuid::new_v4())
        .await
        .expect("list_relationships_for_entity should succeed");

    assert_eq!(found, Vec::new());
}

#[tokio::test]
async fn deleting_an_existing_relationship_removes_it_and_returns_true() {
    let (storage, _container, _connection_string) = test_storage().await;
    let relationship = mock_relationship();
    storage
        .create_relationship(relationship.clone())
        .await
        .expect("create_relationship should succeed");

    let deleted = storage
        .delete_relationship(relationship.id)
        .await
        .expect("delete_relationship should succeed");

    assert!(deleted);
    let found = storage
        .list_relationships_for_entity("table", relationship.from_entity_id)
        .await
        .expect("list_relationships_for_entity should succeed");
    assert_eq!(found, Vec::new());
}

#[tokio::test]
async fn deleting_a_nonexistent_relationship_returns_false() {
    let (storage, _container, _connection_string) = test_storage().await;

    let deleted = storage
        .delete_relationship(Uuid::new_v4())
        .await
        .expect("delete_relationship should succeed");

    assert!(!deleted);
}
