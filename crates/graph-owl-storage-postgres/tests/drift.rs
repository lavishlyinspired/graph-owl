//! Epic 20 x Epic 42 Slice D against a real Postgres: `push_drift` is
//! idempotent while an item is pending (partial unique index from V51), and
//! a decided item does not block a later, genuinely new occurrence of the
//! same drift.

mod common;

use chrono::Utc;
use graph_owl_core::drift::{DriftItem, DriftKind, DriftReportItem, DriftStatus};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{Asset, AssetKind};
use graph_owl_storage::{DriftFilter, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

async fn asset(storage: &PostgresStorage, name: &str) -> Uuid {
    let now = Utc::now();
    storage
        .upsert_asset(Asset {
            id: Uuid::new_v4(),
            kind: AssetKind::Service,
            name: name.to_string(),
            fully_qualified_name: name.to_string(),
            parent_id: None,
            description: None,
            properties: None,
            extension: None,
            owners: Vec::new(),
            version: EntityVersion::initial(),
            updated_by: "system".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            lifecycle: Default::default(),
            deprecation: None,
        })
        .await
        .expect("asset")
        .id
}

fn description_drift(fqn: &str) -> DriftReportItem {
    DriftReportItem {
        fully_qualified_name: fqn.to_string(),
        field: "description".to_string(),
        kind: DriftKind::Unapplied,
        live_value: Some("old description".to_string()),
        declared_value: Some("new description".to_string()),
    }
}

fn all_pending() -> DriftFilter {
    DriftFilter {
        limit: 50,
        ..Default::default()
    }
}

#[tokio::test]
async fn a_pushed_item_round_trips_with_the_asset_fqn_denormalized() {
    let (storage, _db) = test_storage().await;
    let id = asset(&storage, "orders").await;

    let pushed: DriftItem = storage
        .push_drift(id, description_drift("orders"))
        .await
        .expect("push");

    assert_eq!(pushed.asset_id, id);
    assert_eq!(pushed.fully_qualified_name, "orders");
    assert_eq!(pushed.field, "description");
    assert_eq!(pushed.kind, DriftKind::Unapplied);
    assert_eq!(pushed.live_value.as_deref(), Some("old description"));
    assert_eq!(pushed.declared_value.as_deref(), Some("new description"));
    assert_eq!(pushed.status, DriftStatus::Pending);

    let fetched = storage
        .get_drift_item(pushed.id)
        .await
        .expect("get")
        .expect("must exist");
    assert_eq!(fetched, pushed);
}

#[tokio::test]
async fn pushing_the_same_pending_field_twice_returns_the_original_item() {
    let (storage, _db) = test_storage().await;
    let id = asset(&storage, "orders").await;

    let first = storage
        .push_drift(id, description_drift("orders"))
        .await
        .expect("first push");
    // A different live/declared pair — simulating a second, independent
    // detection run over the same still-unresolved drift.
    let second = storage
        .push_drift(
            id,
            DriftReportItem {
                live_value: Some("old description v2".to_string()),
                ..description_drift("orders")
            },
        )
        .await
        .expect("second push");

    assert_eq!(
        second, first,
        "a second push for the same (asset, field) while pending must return the first item unchanged"
    );
}

#[tokio::test]
async fn a_decided_item_does_not_block_a_fresh_occurrence_of_the_same_drift() {
    let (storage, _db) = test_storage().await;
    let id = asset(&storage, "orders").await;

    let first = storage
        .push_drift(id, description_drift("orders"))
        .await
        .expect("first push");
    storage
        .decide_drift(
            first.id,
            DriftStatus::Ignored,
            "alice".to_string(),
            Utc::now(),
            Some("expected, not drift".to_string()),
        )
        .await
        .expect("ignore");

    let second = storage
        .push_drift(id, description_drift("orders"))
        .await
        .expect("second push, after resolution");

    assert_ne!(
        second.id, first.id,
        "a new pending row must be created once the previous one was decided"
    );
    assert_eq!(second.status, DriftStatus::Pending);

    let (pending, total) = storage
        .list_drift(&all_pending())
        .await
        .expect("pending list");
    assert_eq!(total, 1, "only the fresh occurrence is pending");
    assert_eq!(pending[0].id, second.id);
}

#[tokio::test]
async fn deciding_an_already_decided_item_leaves_it_unchanged() {
    let (storage, _db) = test_storage().await;
    let id = asset(&storage, "orders").await;
    let pushed = storage
        .push_drift(id, description_drift("orders"))
        .await
        .expect("push");

    storage
        .decide_drift(
            pushed.id,
            DriftStatus::Applied,
            "alice".to_string(),
            Utc::now(),
            None,
        )
        .await
        .expect("first decide");

    let second = storage
        .decide_drift(
            pushed.id,
            DriftStatus::Ignored,
            "bob".to_string(),
            Utc::now(),
            Some("bob's reason, must not overwrite alice's apply".to_string()),
        )
        .await
        .expect("second decide")
        .expect("item exists");

    assert_eq!(
        second.status,
        DriftStatus::Applied,
        "the first decision must stand; a second decide call must not flip it"
    );
    assert_eq!(second.decided_by.as_deref(), Some("alice"));
}

#[tokio::test]
async fn deciding_an_unknown_item_is_none() {
    let (storage, _db) = test_storage().await;
    let result = storage
        .decide_drift(
            Uuid::new_v4(),
            DriftStatus::Applied,
            "alice".to_string(),
            Utc::now(),
            None,
        )
        .await
        .expect("decide");
    assert_eq!(result, None);
}

#[tokio::test]
async fn listing_defaults_to_pending_and_is_filterable_by_status() {
    let (storage, _db) = test_storage().await;
    let id = asset(&storage, "orders").await;
    let pending_item = storage
        .push_drift(id, description_drift("orders"))
        .await
        .expect("push pending");
    let applied_item = storage
        .push_drift(
            id,
            DriftReportItem {
                field: "owner".to_string(),
                ..description_drift("orders")
            },
        )
        .await
        .expect("push to be applied");
    storage
        .decide_drift(
            applied_item.id,
            DriftStatus::Applied,
            "alice".to_string(),
            Utc::now(),
            None,
        )
        .await
        .expect("apply");

    let (default_page, default_total) = storage
        .list_drift(&all_pending())
        .await
        .expect("default list");
    assert_eq!(default_total, 1);
    assert_eq!(default_page[0].id, pending_item.id);

    let (applied_page, applied_total) = storage
        .list_drift(&DriftFilter {
            status: Some(DriftStatus::Applied),
            limit: 50,
            offset: 0,
        })
        .await
        .expect("applied list");
    assert_eq!(applied_total, 1);
    assert_eq!(applied_page[0].id, applied_item.id);
}
