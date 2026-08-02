//! Epic 19 Slice D: poison messages are quarantined, never retried forever.

mod common;

use graph_owl_connectors::streaming::KafkaConsumer;
use graph_owl_server::streaming::StreamConsumer;
use graph_owl_server::streaming::process_one_message;
use graph_owl_storage::{BrokerConfig, Expression, Mapping, StartPosition, StreamSubscription};
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;

async fn produce_raw(bootstrap_servers: &str, topic: &str, payload: &str) {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", "v4")
        .create()
        .expect("producer should build");
    producer
        .send(
            FutureRecord::to(topic).payload(payload).key("k"),
            Duration::from_secs(10),
        )
        .await
        .expect("send should succeed");
}

fn name_mapping(name: &str, pointer: &str) -> Mapping {
    Mapping {
        name: name.to_string(),
        version: 0,
        kind: Expression::Literal {
            value: "service".to_string(),
        },
        entity_name: Expression::Path {
            pointer: pointer.to_string(),
        },
        parent_fqn: None,
        description: None,
        properties: BTreeMap::new(),
        created_at: chrono::Utc::now(),
    }
}

fn subscription(bootstrap_servers: &str, topic: &str, mapping: &str) -> StreamSubscription {
    StreamSubscription {
        id: uuid::Uuid::new_v4(),
        broker: BrokerConfig::KafkaProtocol {
            bootstrap_servers: bootstrap_servers.to_string(),
        },
        topic: topic.to_string(),
        consumer_group: format!("poison-{}", uuid::Uuid::new_v4()),
        mapping: mapping.to_string(),
        start_position: StartPosition::Earliest,
        max_in_flight: 100,
        poison_threshold: 2,
        has_secret: false,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// **The starvation test** (the plan's own RED): one permanently-bad
/// message among good ones. The good ones *after* it must still be applied
/// — a blocking retry would starve them — and the bad one must land in the
/// DLQ with its raw payload and the error, not vanish.
#[tokio::test]
async fn a_poison_message_is_quarantined_and_the_ones_behind_it_still_apply() {
    let (catalog, _database, _) = common::test_catalog().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();

    catalog
        .upsert_mapping(name_mapping("poison-map", "/name"))
        .await
        .expect("register mapping");
    let sub = catalog
        .register_stream_subscription(subscription(&bootstrap_servers, &topic, "poison-map"), None)
        .await
        .expect("register subscription");

    produce_raw(
        &bootstrap_servers,
        &topic,
        &json!({"name": "good-0"}).to_string(),
    )
    .await;
    // Valid JSON, but nothing at `/name` — a deterministic mapping failure,
    // poisonous on every attempt.
    produce_raw(
        &bootstrap_servers,
        &topic,
        &json!({"title": "poison"}).to_string(),
    )
    .await;
    produce_raw(
        &bootstrap_servers,
        &topic,
        &json!({"name": "good-1"}).to_string(),
    )
    .await;

    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &sub.consumer_group,
        sub.start_position,
    )
    .map(StreamConsumer::Kafka)
    .expect("connect");
    for _ in 0..3 {
        process_one_message(&catalog, &consumer, &sub).await;
    }

    for name in ["good-0", "good-1"] {
        assert!(
            catalog
                .get_asset_by_fqn(name)
                .await
                .expect("read")
                .is_some(),
            "`{name}` must have been applied despite the poison message before/between"
        );
    }

    let letters = catalog
        .stream_dead_letters(Some(sub.id))
        .await
        .expect("list dead letters");
    assert_eq!(letters.len(), 1, "exactly the poison message: {letters:?}");
    assert!(
        letters[0].reason.contains("name"),
        "the reason must name what failed: {}",
        letters[0].reason
    );
    assert_eq!(
        letters[0].payload,
        json!({"title": "poison"}).to_string().into_bytes(),
        "the raw payload must be preserved for replay"
    );
}

/// **Replay after a mapping fix.** The mapping is repaired (a new version
/// reading `/title` instead of `/name`), the letter replayed, and the
/// entity appears — with the letter gone from the queue.
#[tokio::test]
async fn a_dead_letter_is_replayable_after_a_mapping_fix() {
    let (catalog, _database, _) = common::test_catalog().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();

    catalog
        .upsert_mapping(name_mapping("replay-map", "/name"))
        .await
        .expect("register mapping");
    let sub = catalog
        .register_stream_subscription(subscription(&bootstrap_servers, &topic, "replay-map"), None)
        .await
        .expect("register subscription");

    produce_raw(
        &bootstrap_servers,
        &topic,
        &json!({"title": "renamed-entity"}).to_string(),
    )
    .await;

    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &sub.consumer_group,
        sub.start_position,
    )
    .map(StreamConsumer::Kafka)
    .expect("connect");
    process_one_message(&catalog, &consumer, &sub).await;

    let letters = catalog
        .stream_dead_letters(Some(sub.id))
        .await
        .expect("list");
    assert_eq!(letters.len(), 1, "{letters:?}");

    // The fix: a new version of the same mapping, now reading `/title` —
    // mappings are append-only versioned (Epic 18 Slice C), and
    // `get_mapping` returns the latest.
    catalog
        .upsert_mapping(name_mapping("replay-map", "/title"))
        .await
        .expect("register the fixed mapping");

    catalog
        .replay_stream_dead_letter(letters[0].id)
        .await
        .expect("replay should now succeed");

    assert!(
        catalog
            .get_asset_by_fqn("renamed-entity")
            .await
            .expect("read")
            .is_some(),
        "the replayed entity must now exist"
    );
    assert!(
        catalog
            .stream_dead_letters(Some(sub.id))
            .await
            .expect("list")
            .is_empty(),
        "a successful replay removes the letter"
    );
}

/// A replay against a mapping that is *still* broken fails, and the letter
/// stays — replay is not a delete.
#[tokio::test]
async fn a_failed_replay_keeps_the_letter() {
    let (catalog, _database, _) = common::test_catalog().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();

    catalog
        .upsert_mapping(name_mapping("still-broken", "/name"))
        .await
        .expect("register mapping");
    let sub = catalog
        .register_stream_subscription(
            subscription(&bootstrap_servers, &topic, "still-broken"),
            None,
        )
        .await
        .expect("register subscription");

    produce_raw(
        &bootstrap_servers,
        &topic,
        &json!({"title": "x"}).to_string(),
    )
    .await;
    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &sub.consumer_group,
        sub.start_position,
    )
    .map(StreamConsumer::Kafka)
    .expect("connect");
    process_one_message(&catalog, &consumer, &sub).await;

    let letters = catalog
        .stream_dead_letters(Some(sub.id))
        .await
        .expect("list");
    assert_eq!(letters.len(), 1);

    let result = catalog.replay_stream_dead_letter(letters[0].id).await;
    assert!(result.is_err(), "an unfixed mapping must still fail");
    assert_eq!(
        catalog
            .stream_dead_letters(Some(sub.id))
            .await
            .expect("list")
            .len(),
        1,
        "the letter must survive a failed replay"
    );
}
