use chrono::Utc;
use graph_owl_core::Table;
use graph_owl_storage::{Storage, StorageError};
use graph_owl_storage_postgres::PostgresStorage;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use uuid::Uuid;

fn mock_table() -> Table {
    let now = Utc::now();
    Table {
        id: Uuid::new_v4(),
        name: "customers".to_string(),
        fully_qualified_name: "warehouse.public.customers".to_string(),
        description: None,
        created_at: now,
        updated_at: now,
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
async fn inserting_a_table_persists_it_in_postgres() {
    let (storage, _container, connection_string) = test_storage().await;
    let table = mock_table();

    let inserted = storage
        .insert_table(table.clone())
        .await
        .expect("insert should succeed");

    assert_eq!(inserted.id, table.id);
    assert_eq!(inserted.fully_qualified_name, table.fully_qualified_name);

    let verification_pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("failed to open verification connection");
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM tables WHERE id = $1")
        .bind(table.id)
        .fetch_one(&verification_pool)
        .await
        .expect("verification query should succeed");
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn inserting_a_duplicate_fully_qualified_name_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let first = mock_table();
    let second = Table {
        id: Uuid::new_v4(),
        fully_qualified_name: first.fully_qualified_name.clone(),
        ..mock_table()
    };

    storage
        .insert_table(first)
        .await
        .expect("first insert should succeed");
    let result = storage.insert_table(second).await;

    assert!(matches!(result, Err(StorageError::Conflict(_))));
}

#[tokio::test]
async fn inserting_a_table_with_an_empty_name_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let table = Table {
        name: String::new(),
        ..mock_table()
    };

    let result = storage.insert_table(table).await;

    assert!(matches!(result, Err(StorageError::Unexpected(_))));
}

#[tokio::test]
async fn getting_a_table_by_id_returns_the_persisted_table() {
    let (storage, _container, _connection_string) = test_storage().await;
    let table = mock_table();
    storage
        .insert_table(table.clone())
        .await
        .expect("insert should succeed");

    let found = storage
        .get_table(table.id)
        .await
        .expect("get_table should succeed");

    assert_eq!(found, Some(table));
}

#[tokio::test]
async fn getting_a_nonexistent_table_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let found = storage
        .get_table(Uuid::new_v4())
        .await
        .expect("get_table should succeed");

    assert_eq!(found, None);
}
