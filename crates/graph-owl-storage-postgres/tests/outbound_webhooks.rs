//! Epic 14 Slice F (decision 4.2) against a real Postgres.

mod common;

use graph_owl_storage::{OutboundWebhook, Storage};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn webhook(url: &str) -> OutboundWebhook {
    let now = chrono::Utc::now();
    OutboundWebhook {
        id: Uuid::new_v4(),
        url: url.to_string(),
        event_types: vec!["created".to_string(), "updated".to_string()],
        enabled: true,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn a_registered_webhook_round_trips_without_its_secret() {
    let (storage, _db) = test_storage().await;

    let written = storage
        .upsert_outbound_webhook(
            webhook("https://example.com/hooks/graph-owl"),
            Some(b"signing-secret"),
        )
        .await
        .expect("register");

    assert_eq!(written.url, "https://example.com/hooks/graph-owl");
    assert_eq!(
        written.event_types,
        vec!["created".to_string(), "updated".to_string()]
    );
    assert!(written.enabled);

    let fetched = storage
        .get_outbound_webhook(written.id)
        .await
        .expect("get")
        .expect("must exist");
    assert_eq!(fetched, written);
}

#[tokio::test]
async fn registering_a_new_webhook_without_a_secret_is_refused() {
    let (storage, _db) = test_storage().await;

    // Unlike an inbound endpoint or a stream subscription, an outbound
    // webhook this project has never seen before has no existing key for
    // `None` to fall back to — a signing key is not optional the way an
    // unauthenticated broker's credential is, so a first registration
    // without one must fail rather than silently create an unsigned,
    // unverifiable subscription.
    let result = storage
        .upsert_outbound_webhook(webhook("https://example.com/hooks/first"), None)
        .await;

    assert!(result.is_err(), "{result:?}");
}

#[tokio::test]
async fn the_secret_is_never_in_a_get_or_list_response() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_outbound_webhook(
            webhook("https://example.com/hooks/a"),
            Some(b"signing-secret"),
        )
        .await
        .expect("register");

    let fetched = storage
        .get_outbound_webhook(written.id)
        .await
        .expect("get")
        .expect("must exist");
    let fetched_string = format!("{fetched:?}");
    assert!(
        !fetched_string.contains("signing-secret"),
        "{fetched_string}"
    );

    let listed = storage.list_outbound_webhooks().await.expect("list");
    // A real, non-empty result naming the registered webhook — not just "the
    // secret string does not appear", which an empty list would also
    // satisfy for the wrong reason.
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, written.id);
    let listed_string = format!("{listed:?}");
    assert!(!listed_string.contains("signing-secret"), "{listed_string}");

    let json = serde_json::to_string(&listed).expect("serialize");
    assert!(!json.contains("signing-secret"));
}

#[tokio::test]
async fn the_secret_is_readable_only_through_its_own_method() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_outbound_webhook(
            webhook("https://example.com/hooks/a"),
            Some(b"signing-secret"),
        )
        .await
        .expect("register");

    let secret = storage
        .outbound_webhook_secret(written.id)
        .await
        .expect("read secret")
        .expect("must exist");
    assert_eq!(secret, b"signing-secret");
}

#[tokio::test]
async fn updating_a_webhook_without_a_new_secret_leaves_the_old_one_in_place() {
    let (storage, _db) = test_storage().await;
    let mut registered = webhook("https://example.com/hooks/a");
    let written = storage
        .upsert_outbound_webhook(registered.clone(), Some(b"original-secret"))
        .await
        .expect("register");

    registered.id = written.id;
    registered.enabled = false;
    let updated = storage
        .upsert_outbound_webhook(registered, None)
        .await
        .expect("update without a new secret");
    assert!(!updated.enabled);

    let secret = storage
        .outbound_webhook_secret(written.id)
        .await
        .expect("read secret")
        .expect("must still exist");
    assert_eq!(secret, b"original-secret");
}

/// Empty means "every kind" at the `graph_owl_events::webhook` layer — this
/// is only proving the array itself, including empty, round-trips through
/// storage faithfully, not deciding what empty means.
#[tokio::test]
async fn an_empty_event_types_list_round_trips_as_empty_not_null() {
    let (storage, _db) = test_storage().await;
    let mut every_kind = webhook("https://example.com/hooks/every-kind");
    every_kind.event_types = vec![];

    let written = storage
        .upsert_outbound_webhook(every_kind, Some(b"secret"))
        .await
        .expect("register");
    assert_eq!(written.event_types, Vec::<String>::new());

    let fetched = storage
        .get_outbound_webhook(written.id)
        .await
        .expect("get")
        .expect("must exist");
    assert_eq!(fetched.event_types, Vec::<String>::new());
}

#[tokio::test]
async fn enqueueing_a_delivery_is_visible_via_list_for_its_webhook() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_outbound_webhook(webhook("https://example.com/hooks/a"), Some(b"secret"))
        .await
        .expect("register");

    let payload = serde_json::json!({ "kind": "created", "entityId": "asset-1" });
    let enqueued = storage
        .enqueue_outbound_webhook_delivery(written.id, payload.clone())
        .await
        .expect("enqueue");

    assert_eq!(enqueued.webhook_id, written.id);
    assert_eq!(enqueued.payload, payload);
    assert_eq!(enqueued.attempt, 0);
    assert!(!enqueued.dead_lettered);

    let listed = storage
        .list_outbound_webhook_deliveries(written.id)
        .await
        .expect("list deliveries");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, enqueued.id);
    assert_eq!(listed[0].payload, payload);
}

#[tokio::test]
async fn deliveries_for_a_different_webhook_are_not_returned() {
    let (storage, _db) = test_storage().await;
    let a = storage
        .upsert_outbound_webhook(webhook("https://example.com/hooks/a"), Some(b"secret"))
        .await
        .expect("register a");
    let b = storage
        .upsert_outbound_webhook(webhook("https://example.com/hooks/b"), Some(b"secret"))
        .await
        .expect("register b");

    storage
        .enqueue_outbound_webhook_delivery(a.id, serde_json::json!({ "for": "a" }))
        .await
        .expect("enqueue for a");
    storage
        .enqueue_outbound_webhook_delivery(b.id, serde_json::json!({ "for": "b" }))
        .await
        .expect("enqueue for b");

    let listed_for_a = storage
        .list_outbound_webhook_deliveries(a.id)
        .await
        .expect("list for a");
    assert_eq!(listed_for_a.len(), 1);
    assert_eq!(listed_for_a[0].payload, serde_json::json!({ "for": "a" }));
}
