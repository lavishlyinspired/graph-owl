use chrono::Utc;
use graph_owl_core::Relationship;
use graph_owl_storage::{Storage, StorageError};
use graph_owl_storage_postgres::PostgresStorage;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
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

async fn test_storage() -> (PostgresStorage, ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .start()
        .await
        .expect("failed to start postgres container");
    let host = container.get_host().await.expect("failed to get host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get mapped port");
    let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");

    (storage, container, connection_string)
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

    assert!(matches!(result, Err(StorageError::Conflict(_))));
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
