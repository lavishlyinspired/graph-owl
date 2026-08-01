//! Epic 18 Slice C against a real Postgres: mapping versioning is append-only.

mod common;

use graph_owl_storage::{Expression, Mapping, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use std::collections::BTreeMap;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn path(pointer: &str) -> Expression {
    Expression::Path {
        pointer: pointer.to_string(),
    }
}

fn mapping(name: &str) -> Mapping {
    Mapping {
        name: name.to_string(),
        version: 0, // ignored on write
        kind: path("/kind"),
        entity_name: path("/tableName"),
        parent_fqn: None,
        description: None,
        properties: BTreeMap::new(),
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn the_first_version_of_a_mapping_is_one() {
    let (storage, _database) = test_storage().await;

    let saved = storage
        .upsert_mapping(mapping("dbt-run-completed"))
        .await
        .expect("first version");

    assert_eq!(saved.version, 1);
    assert_eq!(saved.kind, path("/kind"));
}

#[tokio::test]
async fn a_second_write_to_the_same_name_is_version_two_not_an_overwrite() {
    let (storage, _database) = test_storage().await;
    storage
        .upsert_mapping(mapping("dbt-run-completed"))
        .await
        .expect("first version");

    let mut second = mapping("dbt-run-completed");
    second.entity_name = path("/renamedField");
    let saved = storage
        .upsert_mapping(second)
        .await
        .expect("second version");

    assert_eq!(saved.version, 2);

    let history = storage
        .list_mapping_versions("dbt-run-completed")
        .await
        .expect("history");
    assert_eq!(history.len(), 2, "the first version must still be a row");
    assert_eq!(history[0].version, 2, "newest first");
    assert_eq!(history[1].version, 1);
    assert_eq!(
        history[1].entity_name,
        path("/tableName"),
        "the old version's own rule is unchanged, not overwritten"
    );
}

#[tokio::test]
async fn get_mapping_returns_the_latest_version() {
    let (storage, _database) = test_storage().await;
    storage
        .upsert_mapping(mapping("dbt-run-completed"))
        .await
        .expect("first version");
    storage
        .upsert_mapping(mapping("dbt-run-completed"))
        .await
        .expect("second version");

    let latest = storage
        .get_mapping("dbt-run-completed")
        .await
        .expect("read")
        .expect("exists");
    assert_eq!(latest.version, 2);
}

#[tokio::test]
async fn different_mapping_names_version_independently() {
    let (storage, _database) = test_storage().await;
    storage
        .upsert_mapping(mapping("dbt-run-completed"))
        .await
        .expect("dbt v1");
    storage
        .upsert_mapping(mapping("dbt-run-completed"))
        .await
        .expect("dbt v2");
    storage
        .upsert_mapping(mapping("airflow-dag-completed"))
        .await
        .expect("airflow v1");

    let dbt = storage
        .get_mapping("dbt-run-completed")
        .await
        .expect("read")
        .expect("exists");
    let airflow = storage
        .get_mapping("airflow-dag-completed")
        .await
        .expect("read")
        .expect("exists");

    assert_eq!(dbt.version, 2);
    assert_eq!(
        airflow.version, 1,
        "a different mapping's history must not be shared"
    );
}

#[tokio::test]
async fn an_unregistered_name_reads_as_absent() {
    let (storage, _database) = test_storage().await;

    assert!(
        storage
            .get_mapping("no-such-mapping")
            .await
            .expect("read")
            .is_none()
    );
    assert!(
        storage
            .list_mapping_versions("no-such-mapping")
            .await
            .expect("read")
            .is_empty()
    );
}

/// Every expression variant round-trips through JSONB, including the
/// recursive ones — a shallow test could pass while `Box`/`Vec`/`BTreeMap`
/// nesting silently lost structure on the way through `serde_json`.
#[tokio::test]
async fn every_expression_variant_round_trips() {
    let (storage, _database) = test_storage().await;
    let mut m = mapping("dbt-run-completed");
    m.kind = Expression::Concat {
        parts: vec![
            Expression::Literal {
                value: "prefix-".to_string(),
            },
            Expression::Lowercase {
                of: Box::new(path("/kind")),
            },
        ],
    };
    m.parent_fqn = Some(Expression::Template {
        pattern: "{schema}.{table}".to_string(),
        bindings: BTreeMap::from([
            ("schema".to_string(), path("/schema")),
            ("table".to_string(), path("/tableName")),
        ]),
    });
    m.description = Some(Expression::Literal {
        value: "a fixed description".to_string(),
    });
    m.properties = BTreeMap::from([("rowCount".to_string(), path("/rows"))]);

    let saved = storage.upsert_mapping(m.clone()).await.expect("write");
    let read_back = storage
        .get_mapping("dbt-run-completed")
        .await
        .expect("read")
        .expect("exists");

    assert_eq!(read_back.kind, saved.kind);
    assert_eq!(read_back.parent_fqn, saved.parent_fqn);
    assert_eq!(read_back.description, saved.description);
    assert_eq!(read_back.properties, saved.properties);
}
