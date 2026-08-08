//! Epic 19 Slice A against a real Postgres.

mod common;

use graph_owl_storage::{BrokerConfig, StartPosition, Storage, StreamSubscription};
use graph_owl_storage_postgres::PostgresStorage;
use uuid::Uuid;

async fn test_storage() -> (PostgresStorage, common::TestDb) {
    let (database, connection_string) = common::fresh_database().await;
    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    (storage, database)
}

fn subscription(topic: &str, consumer_group: &str) -> StreamSubscription {
    let now = chrono::Utc::now();
    StreamSubscription {
        id: Uuid::new_v4(),
        broker: BrokerConfig::KafkaProtocol {
            bootstrap_servers: "localhost:9092".to_string(),
        },
        topic: topic.to_string(),
        consumer_group: consumer_group.to_string(),
        mapping: "dbt-run-completed".to_string(),
        start_position: StartPosition::Earliest,
        max_in_flight: 100,
        poison_threshold: 3,
        has_secret: false,
        enabled: true,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn a_registered_subscription_round_trips_without_its_secret() {
    let (storage, _db) = test_storage().await;

    let written = storage
        .upsert_stream_subscription(subscription("dbt.runs", "graph-owl"), Some(b"sasl-secret"))
        .await
        .expect("register");

    assert!(written.has_secret);
    let fetched = storage
        .get_stream_subscription(written.id)
        .await
        .expect("get")
        .expect("must exist");
    assert_eq!(fetched, written);
    assert_eq!(
        fetched.broker,
        BrokerConfig::KafkaProtocol {
            bootstrap_servers: "localhost:9092".to_string()
        }
    );
}

#[tokio::test]
async fn the_secret_is_never_in_a_get_or_list_response() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_stream_subscription(subscription("dbt.runs", "graph-owl"), Some(b"sasl-secret"))
        .await
        .expect("register");

    let fetched = storage
        .get_stream_subscription(written.id)
        .await
        .expect("get")
        .expect("must exist");
    let fetched_string = format!("{fetched:?}");
    assert!(!fetched_string.contains("sasl-secret"), "{fetched_string}");

    let listed = storage.list_stream_subscriptions().await.expect("list");
    let listed_string = format!("{listed:?}");
    assert!(!listed_string.contains("sasl-secret"), "{listed_string}");
}

#[tokio::test]
async fn the_secret_is_readable_only_through_its_own_method() {
    let (storage, _db) = test_storage().await;
    let written = storage
        .upsert_stream_subscription(subscription("dbt.runs", "graph-owl"), Some(b"sasl-secret"))
        .await
        .expect("register");

    let secret = storage
        .stream_subscription_secret(written.id)
        .await
        .expect("read secret")
        .expect("must exist");
    assert_eq!(secret, b"sasl-secret");
}

#[tokio::test]
async fn a_second_subscription_cannot_reuse_a_registered_topic_and_group() {
    let (storage, _db) = test_storage().await;
    storage
        .upsert_stream_subscription(subscription("dbt.runs", "graph-owl"), None)
        .await
        .expect("register");

    let mut duplicate = subscription("dbt.runs", "graph-owl");
    duplicate.id = Uuid::new_v4();
    let result = storage.upsert_stream_subscription(duplicate, None).await;

    assert!(matches!(
        result,
        Err(graph_owl_storage::StorageError::Conflict {
            kind: graph_owl_storage::ConflictKind::StreamSubscriptionExists,
            ..
        })
    ));
}

/// A different `consumer_group` on the *same* topic is a distinct
/// subscription, not a conflict — multiple independent consumers reading
/// the same topic is the normal case, not the collision this constraint
/// exists to catch.
#[tokio::test]
async fn a_different_consumer_group_on_the_same_topic_is_not_a_conflict() {
    let (storage, _db) = test_storage().await;
    storage
        .upsert_stream_subscription(subscription("dbt.runs", "graph-owl"), None)
        .await
        .expect("register");

    let result = storage
        .upsert_stream_subscription(subscription("dbt.runs", "a-different-group"), None)
        .await;

    assert!(result.is_ok(), "{result:?}");
}

#[tokio::test]
async fn updating_a_subscription_without_a_new_secret_leaves_the_old_one_in_place() {
    let (storage, _db) = test_storage().await;
    let mut registered = subscription("dbt.runs", "graph-owl");
    let written = storage
        .upsert_stream_subscription(registered.clone(), Some(b"original-secret"))
        .await
        .expect("register");

    registered.id = written.id;
    registered.max_in_flight = 200;
    storage
        .upsert_stream_subscription(registered, None)
        .await
        .expect("update without a new secret");

    let secret = storage
        .stream_subscription_secret(written.id)
        .await
        .expect("read secret")
        .expect("must exist");
    assert_eq!(secret, b"original-secret");
}

#[tokio::test]
async fn every_start_position_variant_round_trips() {
    let (storage, _db) = test_storage().await;

    for (topic, position) in [
        ("t1", StartPosition::Earliest),
        ("t2", StartPosition::Latest),
        (
            "t3",
            StartPosition::Timestamp {
                at: chrono::Utc::now(),
            },
        ),
        ("t4", StartPosition::Offset { value: 42 }),
    ] {
        let mut candidate = subscription(topic, "graph-owl");
        candidate.start_position = position;
        let written = storage
            .upsert_stream_subscription(candidate.clone(), None)
            .await
            .expect("register");
        assert_eq!(written.start_position, candidate.start_position, "{topic}");
    }
}

#[tokio::test]
async fn a_pulsar_broker_round_trips() {
    let (storage, _db) = test_storage().await;
    let mut candidate = subscription("dbt.runs", "graph-owl");
    candidate.broker = BrokerConfig::Pulsar {
        service_url: "pulsar://localhost:6650".to_string(),
        admin_url: None,
    };

    let written = storage
        .upsert_stream_subscription(candidate.clone(), None)
        .await
        .expect("register");
    assert_eq!(written.broker, candidate.broker);
}

/// **Epic 19 Slice F, completed 8 August 2026.** `admin_url` is a second
/// column, not a reuse of `broker_address` — it lives on a separate HTTP
/// surface from the binary protocol `service_url` speaks, sometimes on a
/// different host entirely, so it has to round-trip independently.
#[tokio::test]
async fn a_pulsar_brokers_admin_url_round_trips_too() {
    let (storage, _db) = test_storage().await;
    let mut candidate = subscription("dbt.runs", "graph-owl");
    candidate.broker = BrokerConfig::Pulsar {
        service_url: "pulsar://localhost:6650".to_string(),
        admin_url: Some("http://localhost:8080".to_string()),
    };

    let written = storage
        .upsert_stream_subscription(candidate.clone(), None)
        .await
        .expect("register");
    assert_eq!(written.broker, candidate.broker);

    let fetched = storage
        .get_stream_subscription(written.id)
        .await
        .expect("get")
        .expect("found");
    assert_eq!(fetched.broker, candidate.broker);
}
