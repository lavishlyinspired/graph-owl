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
        reason: None,
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

// ---- Epic 18 Slice D: dead-letter and replay ----

#[tokio::test]
async fn moving_an_event_to_failed_records_the_reason() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");
    let created = storage
        .create_inbound_event(event(registered.id, "id:evt-1"))
        .await
        .expect("create");

    let failed = storage
        .update_inbound_event_state(
            created.id,
            EventState::Failed,
            Some("mapping `dbt-run-completed` field `kind`: nothing at the path"),
        )
        .await
        .expect("update");

    assert_eq!(failed.state, EventState::Failed);
    assert_eq!(
        failed.reason.as_deref(),
        Some("mapping `dbt-run-completed` field `kind`: nothing at the path")
    );
}

#[tokio::test]
async fn a_later_transition_clears_a_stale_reason() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");
    let created = storage
        .create_inbound_event(event(registered.id, "id:evt-1"))
        .await
        .expect("create");
    storage
        .update_inbound_event_state(created.id, EventState::Failed, Some("first attempt failed"))
        .await
        .expect("fail");

    let applied = storage
        .update_inbound_event_state(created.id, EventState::Applied, None)
        .await
        .expect("succeed on replay");

    assert_eq!(applied.state, EventState::Applied);
    assert_eq!(
        applied.reason, None,
        "a successful replay must not leave a stale failure reason behind"
    );
}

#[tokio::test]
async fn the_dead_letter_queue_lists_only_failed_events() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");
    let received = storage
        .create_inbound_event(event(registered.id, "id:received"))
        .await
        .expect("create");
    let failed = storage
        .create_inbound_event(event(registered.id, "id:failed"))
        .await
        .expect("create");
    storage
        .update_inbound_event_state(failed.id, EventState::Failed, Some("bad kind"))
        .await
        .expect("fail");

    let dlq = storage
        .list_dead_letters(&graph_owl_storage::DeadLetterFilter {
            limit: 50,
            ..Default::default()
        })
        .await
        .expect("dlq");

    let ids: Vec<_> = dlq.iter().map(|e| e.id).collect();
    assert!(ids.contains(&failed.id), "{ids:?}");
    assert!(
        !ids.contains(&received.id),
        "a Received event is not dead-lettered: {ids:?}"
    );
}

#[tokio::test]
async fn the_dead_letter_queue_filters_by_endpoint_and_reason() {
    let (storage, _database) = test_storage().await;
    let mut other = endpoint();
    other.path = "airflow".to_string();
    let e1 = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register e1");
    let e2 = storage
        .upsert_webhook_endpoint(other, Some(b"secret"))
        .await
        .expect("register e2");

    let a = storage
        .create_inbound_event(event(e1.id, "id:a"))
        .await
        .expect("create");
    storage
        .update_inbound_event_state(
            a.id,
            EventState::Failed,
            Some("shape `TableNeedsOwner` failed"),
        )
        .await
        .expect("fail a");
    let b = storage
        .create_inbound_event(event(e1.id, "id:b"))
        .await
        .expect("create");
    storage
        .update_inbound_event_state(b.id, EventState::Failed, Some("missing field kind"))
        .await
        .expect("fail b");
    let c = storage
        .create_inbound_event(event(e2.id, "id:c"))
        .await
        .expect("create");
    storage
        .update_inbound_event_state(
            c.id,
            EventState::Failed,
            Some("shape `TableNeedsOwner` failed"),
        )
        .await
        .expect("fail c");

    let by_endpoint = storage
        .list_dead_letters(&graph_owl_storage::DeadLetterFilter {
            endpoint: Some(e1.id),
            limit: 50,
            ..Default::default()
        })
        .await
        .expect("filtered");
    let ids: Vec<_> = by_endpoint.iter().map(|e| e.id).collect();
    assert!(
        ids.contains(&a.id) && ids.contains(&b.id) && !ids.contains(&c.id),
        "{ids:?}"
    );

    let by_reason = storage
        .list_dead_letters(&graph_owl_storage::DeadLetterFilter {
            reason_contains: Some("TableNeedsOwner".to_string()),
            limit: 50,
            ..Default::default()
        })
        .await
        .expect("filtered");
    let ids: Vec<_> = by_reason.iter().map(|e| e.id).collect();
    assert!(
        ids.contains(&a.id) && !ids.contains(&b.id) && ids.contains(&c.id),
        "{ids:?}"
    );
}

#[tokio::test]
async fn the_replay_window_is_bounded_by_arrival_and_ordered_by_sender_timestamp() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");

    let mut early = event(registered.id, "id:early");
    early.sender_timestamp = Some(chrono::Utc::now() - chrono::Duration::hours(2));
    let early = storage
        .create_inbound_event(early)
        .await
        .expect("create early");

    let mut late = event(registered.id, "id:late");
    late.sender_timestamp = Some(chrono::Utc::now() - chrono::Duration::hours(1));
    let late = storage
        .create_inbound_event(late)
        .await
        .expect("create late");

    // Delivered out of order relative to `sender_timestamp`: `late` was
    // recorded first, `early` second, but replay must still visit `early`
    // first.
    let window = storage
        .list_inbound_events_in_window(
            registered.id,
            chrono::Utc::now() - chrono::Duration::days(1),
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .await
        .expect("window");

    let ids: Vec<_> = window.iter().map(|e| e.id).collect();
    let early_pos = ids
        .iter()
        .position(|id| *id == early.id)
        .expect("early present");
    let late_pos = ids
        .iter()
        .position(|id| *id == late.id)
        .expect("late present");
    assert!(
        early_pos < late_pos,
        "sender_timestamp order must win over arrival order: {ids:?}"
    );
}

#[tokio::test]
async fn an_event_with_no_sender_timestamp_falls_back_to_arrival_order_in_the_window() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");

    let first = storage
        .create_inbound_event(event(registered.id, "id:first"))
        .await
        .expect("create first");
    let second = storage
        .create_inbound_event(event(registered.id, "id:second"))
        .await
        .expect("create second");

    let window = storage
        .list_inbound_events_in_window(
            registered.id,
            chrono::Utc::now() - chrono::Duration::days(1),
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .await
        .expect("window");

    let ids: Vec<_> = window.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![first.id, second.id]);
}

#[tokio::test]
async fn purging_removes_only_old_failed_events() {
    let (storage, _database) = test_storage().await;
    let registered = storage
        .upsert_webhook_endpoint(endpoint(), Some(b"secret"))
        .await
        .expect("register");

    let mut old = event(registered.id, "id:old-failed");
    old.received_at = chrono::Utc::now() - chrono::Duration::days(30);
    let old_failed = storage.create_inbound_event(old).await.expect("create");
    storage
        .update_inbound_event_state(old_failed.id, EventState::Failed, Some("old"))
        .await
        .expect("fail");
    let recent_failed = storage
        .create_inbound_event(event(registered.id, "id:recent-failed"))
        .await
        .expect("create");
    storage
        .update_inbound_event_state(recent_failed.id, EventState::Failed, Some("recent"))
        .await
        .expect("fail");
    let received = storage
        .create_inbound_event(event(registered.id, "id:still-received"))
        .await
        .expect("create");

    // Purge everything failed before a week ago — `recent_failed` and
    // `received` were created just now, so this cutoff must not catch
    // either, only the genuinely old row.
    let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
    let purged = storage.purge_dead_letters(cutoff).await.expect("purge");
    assert_eq!(purged, 1, "only the row older than the cutoff is removed");

    assert!(
        storage
            .get_inbound_event(old_failed.id)
            .await
            .expect("read")
            .is_none(),
        "the old failed row must be gone"
    );
    assert!(
        storage
            .get_inbound_event(recent_failed.id)
            .await
            .expect("read")
            .is_some(),
        "a failed row newer than the cutoff must survive"
    );
    assert!(
        storage
            .get_inbound_event(received.id)
            .await
            .expect("read")
            .is_some(),
        "purge must never remove a non-Failed row regardless of age"
    );
}

// ---- out-of-order protection: entity_last_applied ----

#[tokio::test]
async fn an_entity_with_nothing_applied_yet_has_no_recorded_timestamp() {
    let (storage, _database) = test_storage().await;
    assert!(
        storage
            .last_applied_timestamp("svc.orders")
            .await
            .expect("read")
            .is_none()
    );
}

#[tokio::test]
async fn recording_a_timestamp_makes_it_readable_back() {
    let (storage, _database) = test_storage().await;
    let ts = chrono::Utc::now() - chrono::Duration::hours(1);
    storage
        .record_applied_timestamp("svc.orders", ts)
        .await
        .expect("record");

    let read_back = storage
        .last_applied_timestamp("svc.orders")
        .await
        .expect("read")
        .expect("recorded");
    // Postgres TIMESTAMPTZ is microsecond precision; chrono::Utc::now() is
    // nanosecond — compare to the microsecond, not for exact equality.
    assert!(
        (read_back - ts)
            .num_microseconds()
            .unwrap_or(i64::MAX)
            .abs()
            < 1_000
    );
}

#[tokio::test]
async fn recording_again_overwrites_with_the_newer_value() {
    let (storage, _database) = test_storage().await;
    let first = chrono::Utc::now() - chrono::Duration::hours(2);
    let second = chrono::Utc::now() - chrono::Duration::hours(1);
    storage
        .record_applied_timestamp("svc.orders", first)
        .await
        .expect("record first");
    storage
        .record_applied_timestamp("svc.orders", second)
        .await
        .expect("record second");

    let read_back = storage
        .last_applied_timestamp("svc.orders")
        .await
        .expect("read")
        .expect("recorded");
    assert!(
        (read_back - second).num_milliseconds().abs() < 1_000,
        "the newer recording must win, not the first"
    );
}

#[tokio::test]
async fn different_entities_track_independently() {
    let (storage, _database) = test_storage().await;
    let ts = chrono::Utc::now();
    storage
        .record_applied_timestamp("svc.orders", ts)
        .await
        .expect("record orders");

    assert!(
        storage
            .last_applied_timestamp("svc.customers")
            .await
            .expect("read")
            .is_none(),
        "a different entity's high-water mark must not leak across"
    );
}
