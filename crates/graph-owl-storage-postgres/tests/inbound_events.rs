//! Epic 18 Slice B against a real Postgres: dedup is atomic under real
//! concurrency, which nothing short of two real connections racing on the
//! same row can prove.

mod common;

use graph_owl_core::webhook::{EventState, InboundEvent};
use graph_owl_storage::{SignatureScheme, Storage, WebhookEndpoint};
use graph_owl_storage_postgres::PostgresStorage;
use std::sync::Arc;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn endpoint() -> WebhookEndpoint {
    let now = chrono::Utc::now();
    WebhookEndpoint {
        id: Uuid::new_v4(),
        path: "dbt".to_string(),
        source: "dbt-bot".to_string(),
        signature_scheme: SignatureScheme::HmacSha256 {
            header: "X-Signature".to_string(),
            prefix: "sha256=".to_string(),
        },
        mapping: "dbt-run-completed".to_string(),
        event_filter: vec!["run.completed".to_string()],
        enabled: true,
        has_secret: false,
        created_at: now,
        updated_at: now,
    }
}

fn event(endpoint_id: Uuid, dedup_key: &str) -> InboundEvent {
    InboundEvent {
        id: Uuid::new_v4(),
        endpoint: endpoint_id,
        sender_event_id: None,
        sender_timestamp: None,
        received_at: chrono::Utc::now(),
        raw: b"{}".to_vec(),
        state: EventState::Received,
        dedup_key: dedup_key.to_string(),
    }
}

#[tokio::test]
async fn a_second_delivery_with_the_same_dedup_key_is_recorded_as_duplicate() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");

    let first = storage
        .create_inbound_event(event(registered.id, "id:evt-1"))
        .await
        .expect("first delivery");
    assert_eq!(first.state, EventState::Received);

    let second = storage
        .create_inbound_event(event(registered.id, "id:evt-1"))
        .await
        .expect("second delivery");
    assert_eq!(second.state, EventState::Duplicate);
    assert_ne!(second.id, first.id, "each delivery keeps its own row");
}

#[tokio::test]
async fn different_dedup_keys_on_the_same_endpoint_do_not_collide() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");

    let first = storage
        .create_inbound_event(event(registered.id, "id:evt-1"))
        .await
        .expect("first delivery");
    let second = storage
        .create_inbound_event(event(registered.id, "id:evt-2"))
        .await
        .expect("second delivery");

    assert_eq!(first.state, EventState::Received);
    assert_eq!(second.state, EventState::Received);
}

/// The same `dedup_key` on two *different* endpoints must not collide either
/// — the marker's primary key is `(endpoint_id, dedup_key)`, not `dedup_key`
/// alone, because two unrelated sources are free to reuse the same event id.
#[tokio::test]
async fn the_same_dedup_key_on_different_endpoints_does_not_collide() {
    let (storage, _database) = test_storage().await;
    let mut second_endpoint = endpoint();
    second_endpoint.path = "airflow".to_string();
    second_endpoint.source = "airflow-bot".to_string();

    let first_endpoint = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register first");
    let second_endpoint = storage
        .upsert_webhook_endpoint(second_endpoint, Some(b"secret"))
        .await
        .expect("register second");

    let first = storage
        .create_inbound_event(event(first_endpoint.id, "id:evt-shared"))
        .await
        .expect("first delivery");
    let second = storage
        .create_inbound_event(event(second_endpoint.id, "id:evt-shared"))
        .await
        .expect("second delivery");

    assert_eq!(first.state, EventState::Received);
    assert_eq!(
        second.state,
        EventState::Received,
        "the same event id from a different source is not a redelivery"
    );
}

/// **The concurrency claim.** Two real connections racing to insert the same
/// `(endpoint, dedup_key)` must produce exactly one `Received` and one
/// `Duplicate` — never two `Received` (a duplicate effect) and never zero
/// (a delivery silently lost).
#[tokio::test]
async fn concurrent_duplicate_deliveries_produce_exactly_one_effect() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");
    let storage = Arc::new(storage);

    let first_storage = Arc::clone(&storage);
    let endpoint_id = registered.id;
    let first = tokio::spawn(async move {
        first_storage
            .create_inbound_event(event(endpoint_id, "id:racing-evt"))
            .await
            .expect("first delivery")
    });
    let second_storage = Arc::clone(&storage);
    let second = tokio::spawn(async move {
        second_storage
            .create_inbound_event(event(endpoint_id, "id:racing-evt"))
            .await
            .expect("second delivery")
    });

    let (first, second) = tokio::join!(first, second);
    let states = [first.expect("task").state, second.expect("task").state];
    let received_count = states
        .iter()
        .filter(|s| **s == EventState::Received)
        .count();
    let duplicate_count = states
        .iter()
        .filter(|s| **s == EventState::Duplicate)
        .count();
    assert_eq!(
        (received_count, duplicate_count),
        (1, 1),
        "exactly one concurrent delivery must win: {states:?}"
    );
}
