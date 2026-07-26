use chrono::Utc;
use graph_owl_core::{Table, TableUpdate};
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

#[tokio::test]
async fn listing_tables_with_no_rows_returns_an_empty_vec() {
    let (storage, _container, _connection_string) = test_storage().await;

    let tables = storage
        .list_tables()
        .await
        .expect("list_tables should succeed");

    assert_eq!(tables, Vec::new());
}

#[tokio::test]
async fn listing_tables_returns_all_persisted_tables() {
    let (storage, _container, _connection_string) = test_storage().await;
    let first = mock_table();
    let second = Table {
        id: Uuid::new_v4(),
        name: "orders".to_string(),
        fully_qualified_name: "warehouse.public.orders".to_string(),
        ..mock_table()
    };
    storage
        .insert_table(first.clone())
        .await
        .expect("insert should succeed");
    storage
        .insert_table(second.clone())
        .await
        .expect("insert should succeed");

    let mut tables = storage
        .list_tables()
        .await
        .expect("list_tables should succeed");
    tables.sort_by_key(|table| table.id);
    let mut expected = vec![first, second];
    expected.sort_by_key(|table| table.id);

    assert_eq!(tables, expected);
}

#[tokio::test]
async fn updating_a_table_changes_only_the_provided_fields() {
    let (storage, _container, _connection_string) = test_storage().await;
    let table = mock_table();
    storage
        .insert_table(table.clone())
        .await
        .expect("insert should succeed");

    let updated = storage
        .update_table(
            table.id,
            TableUpdate {
                name: None,
                description: Some("a new description".to_string()),
            },
        )
        .await
        .expect("update_table should succeed")
        .expect("table should exist");

    assert_eq!(updated.id, table.id);
    assert_eq!(updated.name, table.name);
    assert_eq!(updated.fully_qualified_name, table.fully_qualified_name);
    assert_eq!(updated.description, Some("a new description".to_string()));
    assert_eq!(updated.created_at, table.created_at);
    assert!(updated.updated_at > table.updated_at);
}

#[tokio::test]
async fn updating_a_nonexistent_table_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let result = storage
        .update_table(
            Uuid::new_v4(),
            TableUpdate {
                name: Some("new name".to_string()),
                description: None,
            },
        )
        .await
        .expect("update_table should succeed");

    assert_eq!(result, None);
}
