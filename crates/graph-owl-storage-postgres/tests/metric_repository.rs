//! Epic 24 Slice E at the storage layer: FQN uniqueness, and the fields an
//! HTTP test cannot pin down as precisely as a direct repository test can.

mod common;

use chrono::Utc;
use graph_owl_core::metric::CalculationType;
use graph_owl_storage::{ConflictKind, MetricRecord, MetricUpdate, Storage, StorageError};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database, connection_string)
}

fn mock_metric(name: &str) -> MetricRecord {
    let now = Utc::now();
    MetricRecord {
        id: Uuid::new_v4(),
        name: name.to_string(),
        fully_qualified_name: format!("metric.{name}"),
        definition: format!("the meaning of {name}"),
        formula: None,
        unit: None,
        granularity: None,
        calculation_type: CalculationType::Simple,
        defined_by: None,
        source_assets: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn inserting_a_metric_persists_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = mock_metric("revenue");

    storage
        .insert_metric(metric.clone())
        .await
        .expect("insert should succeed");

    let found = storage
        .get_metric(metric.id)
        .await
        .expect("get_metric should succeed");
    assert_eq!(found, Some(metric));
}

#[tokio::test]
async fn a_duplicate_metric_fqn_is_rejected() {
    let (storage, _container, _connection_string) = test_storage().await;
    storage
        .insert_metric(mock_metric("revenue"))
        .await
        .expect("first insert should succeed");

    let result = storage.insert_metric(mock_metric("revenue")).await;

    assert!(matches!(
        result,
        Err(StorageError::Conflict {
            kind: ConflictKind::Fqn,
            ..
        })
    ));
}

// A non-uniqueness database error (the `CHECK (definition <> '')` constraint)
// must surface as `Unexpected`, not be reported as a `Conflict` too.
#[tokio::test]
async fn inserting_a_metric_with_a_blank_definition_is_rejected_as_unexpected() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = MetricRecord {
        definition: String::new(),
        ..mock_metric("revenue")
    };

    let result = storage.insert_metric(metric).await;

    assert!(matches!(result, Err(StorageError::Unexpected(_))));
}

#[tokio::test]
async fn a_metrics_sources_are_persisted_and_read_back() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = MetricRecord {
        source_assets: vec![
            "warehouse.public.orders".to_string(),
            "warehouse.public.refunds".to_string(),
        ],
        ..mock_metric("revenue")
    };

    storage
        .insert_metric(metric.clone())
        .await
        .expect("insert should succeed");

    let found = storage
        .get_metric(metric.id)
        .await
        .expect("get_metric should succeed")
        .expect("the metric should exist");
    assert_eq!(
        found.source_assets,
        vec![
            "warehouse.public.orders".to_string(),
            "warehouse.public.refunds".to_string()
        ]
    );
}

#[tokio::test]
async fn getting_a_nonexistent_metric_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let found = storage
        .get_metric(Uuid::new_v4())
        .await
        .expect("get_metric should succeed");

    assert_eq!(found, None);
}

#[tokio::test]
async fn listing_metrics_returns_every_persisted_metric() {
    let (storage, _container, _connection_string) = test_storage().await;
    storage
        .insert_metric(mock_metric("revenue"))
        .await
        .expect("insert should succeed");
    storage
        .insert_metric(mock_metric("churn"))
        .await
        .expect("insert should succeed");

    let page = storage
        .list_metrics(&graph_owl_core::page::PageRequest::new(None, None).expect("valid"))
        .await
        .expect("list_metrics should succeed");

    assert_eq!(page.data.len(), 2);
}

#[tokio::test]
async fn updating_a_metric_changes_only_the_provided_fields() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = mock_metric("revenue");
    storage
        .insert_metric(metric.clone())
        .await
        .expect("insert should succeed");

    let updated = storage
        .update_metric(
            metric.id,
            MetricUpdate {
                definition: Some("revised definition".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update_metric should succeed")
        .expect("the metric should exist");

    assert_eq!(updated.definition, "revised definition");
    assert_eq!(updated.name, metric.name);
    assert!(updated.updated_at >= metric.updated_at);
}

#[tokio::test]
async fn updating_a_nonexistent_metric_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let result = storage
        .update_metric(Uuid::new_v4(), MetricUpdate::default())
        .await
        .expect("update_metric should succeed");

    assert_eq!(result, None);
}

#[tokio::test]
async fn deleting_a_metric_removes_it() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = mock_metric("revenue");
    storage
        .insert_metric(metric.clone())
        .await
        .expect("insert should succeed");

    let deleted = storage
        .delete_metric(metric.id)
        .await
        .expect("delete_metric should succeed");

    assert!(deleted);
    assert_eq!(
        storage.get_metric(metric.id).await.expect("get_metric"),
        None
    );
}

#[tokio::test]
async fn deleting_a_nonexistent_metric_returns_false() {
    let (storage, _container, _connection_string) = test_storage().await;

    let deleted = storage
        .delete_metric(Uuid::new_v4())
        .await
        .expect("delete_metric should succeed");

    assert!(!deleted);
}

#[tokio::test]
async fn a_metric_is_found_by_name() {
    let (storage, _container, _connection_string) = test_storage().await;
    storage
        .insert_metric(mock_metric("revenue"))
        .await
        .expect("insert should succeed");

    let hits = storage
        .search_metrics("revenue")
        .await
        .expect("search_metrics should succeed");

    assert_eq!(hits.len(), 1);
}

// **The negative that makes the positive above mean something.**
#[tokio::test]
async fn an_unrelated_word_does_not_match_a_metric() {
    let (storage, _container, _connection_string) = test_storage().await;
    storage
        .insert_metric(mock_metric("revenue"))
        .await
        .expect("insert should succeed");

    let hits = storage
        .search_metrics("zzzznomatch")
        .await
        .expect("search_metrics should succeed");

    assert!(hits.is_empty());
}

// **Searchable by defining term** (Slice E) — the generated `search_vector`
// column cannot reach across tables, so this exercises the join path
// specifically.
#[tokio::test]
async fn a_metric_is_found_by_its_defining_terms_name() {
    let (storage, _container, connection_string) = test_storage().await;
    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("connect");
    let glossary_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO glossaries (id, name, fully_qualified_name) VALUES ($1, 'Finance', 'Finance')",
    )
    .bind(glossary_id)
    .execute(&pool)
    .await
    .expect("seed glossary");
    let term_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO glossary_terms (id, glossary_id, name, fully_qualified_name, status)
         VALUES ($1, $2, 'Recognised Revenue', 'Finance.Recognised Revenue', 'approved')",
    )
    .bind(term_id)
    .bind(glossary_id)
    .execute(&pool)
    .await
    .expect("seed term");
    storage
        .insert_metric(MetricRecord {
            defined_by: Some(term_id),
            ..mock_metric("revenue")
        })
        .await
        .expect("insert should succeed");

    let hits = storage
        .search_metrics("Recognised")
        .await
        .expect("search_metrics should succeed");

    assert_eq!(hits.len(), 1);
}

// ---- Epic 24 Slice F: metric lineage reconciliation ----

#[tokio::test]
async fn updating_sources_replaces_them() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = MetricRecord {
        source_assets: vec!["warehouse.public.orders".to_string()],
        ..mock_metric("revenue")
    };
    storage
        .insert_metric(metric.clone())
        .await
        .expect("insert should succeed");

    let updated = storage
        .update_metric_sources(metric.id, &["warehouse.public.refunds".to_string()])
        .await
        .expect("update_metric_sources should succeed")
        .expect("the metric should exist");

    assert_eq!(
        updated.source_assets,
        vec!["warehouse.public.refunds".to_string()]
    );
}

#[tokio::test]
async fn clearing_sources_removes_them_all() {
    let (storage, _container, _connection_string) = test_storage().await;
    let metric = MetricRecord {
        source_assets: vec!["warehouse.public.orders".to_string()],
        ..mock_metric("revenue")
    };
    storage
        .insert_metric(metric.clone())
        .await
        .expect("insert should succeed");

    let updated = storage
        .update_metric_sources(metric.id, &[])
        .await
        .expect("update_metric_sources should succeed")
        .expect("the metric should exist");

    assert!(updated.source_assets.is_empty());
}

#[tokio::test]
async fn updating_sources_of_an_unknown_metric_returns_none() {
    let (storage, _container, _connection_string) = test_storage().await;

    let result = storage
        .update_metric_sources(Uuid::new_v4(), &["warehouse.public.orders".to_string()])
        .await
        .expect("update_metric_sources should succeed");

    assert_eq!(result, None);
}
