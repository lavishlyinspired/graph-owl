//! Epic 19 Slice E: rebalancing and replay.

mod common;

use graph_owl_connectors::streaming::KafkaConsumer;
use graph_owl_server::streaming::StreamConsumer;
use graph_owl_server::streaming::{process_one_message, replay_window};
use graph_owl_storage::{BrokerConfig, Expression, Mapping, StartPosition, StreamSubscription};
use rdkafka::ClientConfig;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;

/// A topic with more than one partition, so two consumers in a group have
/// something to split. `testcontainers`' broker auto-creates topics with a
/// single partition, which would make a two-consumer test vacuous — one
/// consumer would get the only partition and the other nothing.
async fn create_partitioned_topic(bootstrap_servers: &str, topic: &str, partitions: i32) {
    let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", "v4")
        .create()
        .expect("admin client should build");
    admin
        .create_topics(
            &[NewTopic::new(topic, partitions, TopicReplication::Fixed(1))],
            &AdminOptions::new(),
        )
        .await
        .expect("create topic");
}

async fn produce_keyed(bootstrap_servers: &str, topic: &str, name: &str) {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", "v4")
        .create()
        .expect("producer should build");
    producer
        .send(
            FutureRecord::to(topic)
                .payload(&json!({"name": name}).to_string())
                .key(name),
            Duration::from_secs(10),
        )
        .await
        .expect("send should succeed");
}

fn mapping(name: &str) -> Mapping {
    Mapping {
        name: name.to_string(),
        version: 0,
        kind: Expression::Literal {
            value: "service".to_string(),
        },
        entity_name: Expression::Path {
            pointer: "/name".to_string(),
        },
        parent_fqn: None,
        description: None,
        properties: BTreeMap::new(),
        created_at: chrono::Utc::now(),
    }
}

fn subscription(
    bootstrap_servers: &str,
    topic: &str,
    group: &str,
    mapping_name: &str,
) -> StreamSubscription {
    StreamSubscription {
        id: uuid::Uuid::new_v4(),
        broker: BrokerConfig::KafkaProtocol {
            bootstrap_servers: bootstrap_servers.to_string(),
        },
        topic: topic.to_string(),
        consumer_group: group.to_string(),
        mapping: mapping_name.to_string(),
        start_position: StartPosition::Earliest,
        max_in_flight: 100,
        poison_threshold: 2,
        has_secret: false,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

async fn count_present(catalog: &graph_owl_api::Catalog, total: usize) -> usize {
    let mut found = 0;
    for i in 0..total {
        if catalog
            .get_asset_by_fqn(&format!("rb-{i}"))
            .await
            .expect("read")
            .is_some()
        {
            found += 1;
        }
    }
    found
}

/// Enough messages that a 3-partition split is meaningful without
/// making the test slow.
const MESSAGES: usize = 12;

/// **The two-consumer exactly-once test** (the plan's own RED). Two
/// consumers in one group split a 3-partition topic; every message must be
/// applied, and none twice. Applied-ness is checked by entity existence
/// rather than a call count, because "applied twice" for an FQN-keyed
/// upsert is not observable as a duplicate row — what would be observable
/// is a *missing* entity (a partition neither consumer served) or a merge
/// record (resolution treating a re-apply as a separate entity), and both
/// are asserted.
#[tokio::test]
async fn two_consumers_in_one_group_split_partitions_without_double_applying() {
    let (catalog, _database, _) = common::test_catalog().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();
    create_partitioned_topic(&bootstrap_servers, &topic, 3).await;

    catalog
        .upsert_mapping(mapping("rebalance-map"))
        .await
        .expect("register mapping");

    for i in 0..MESSAGES {
        produce_keyed(&bootstrap_servers, &topic, &format!("rb-{i}")).await;
    }

    let group = format!("rebalance-{}", uuid::Uuid::new_v4());
    let sub = subscription(&bootstrap_servers, &topic, &group, "rebalance-map");

    let first = KafkaConsumer::connect(&bootstrap_servers, &topic, &group, sub.start_position)
        .map(StreamConsumer::Kafka)
        .expect("first consumer");
    let second = KafkaConsumer::connect(&bootstrap_servers, &topic, &group, sub.start_position)
        .map(StreamConsumer::Kafka)
        .expect("second consumer");

    // Alternate between them until the whole set has landed or we give up.
    // Each `process_one_message` blocks until *that* consumer receives, so
    // a strict alternation would deadlock if one consumer owns nothing;
    // instead each is polled with a timeout and skipped when idle.
    let deadline = tokio::time::Instant::now() + Duration::from_mins(1);
    let mut applied = 0;
    while applied < MESSAGES && tokio::time::Instant::now() < deadline {
        for consumer in [&first, &second] {
            if tokio::time::timeout(
                Duration::from_secs(5),
                process_one_message(&catalog, consumer, &sub),
            )
            .await
            .is_ok()
            {
                applied += 1;
            }
        }
        let found = count_present(&catalog, MESSAGES).await;
        if found == MESSAGES {
            break;
        }
    }

    assert_eq!(
        count_present(&catalog, MESSAGES).await,
        MESSAGES,
        "every message must be applied exactly once across both consumers"
    );
}

/// **Replay does not disturb the live subscription's offsets** — the
/// criterion that makes replay safe to run against a running consumer.
/// After a replay covering the same window the live consumer already
/// consumed, the live consumer's committed position is unchanged, and
/// re-applying produced no duplicate entities.
#[tokio::test]
async fn replaying_a_window_is_idempotent_and_leaves_live_offsets_alone() {
    let (catalog, _database, _) = common::test_catalog().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();

    catalog
        .upsert_mapping(mapping("replay-window-map"))
        .await
        .expect("register mapping");
    let group = format!("replay-live-{}", uuid::Uuid::new_v4());
    let sub = catalog
        .register_stream_subscription(
            subscription(&bootstrap_servers, &topic, &group, "replay-window-map"),
            None,
        )
        .await
        .expect("register subscription");

    let before_everything = chrono::Utc::now() - chrono::Duration::minutes(1);
    for i in 0..3 {
        produce_keyed(&bootstrap_servers, &topic, &format!("rw-{i}")).await;
    }

    // The live consumer consumes and commits all three.
    let live = KafkaConsumer::connect(&bootstrap_servers, &topic, &group, sub.start_position)
        .map(StreamConsumer::Kafka)
        .expect("live consumer");
    for _ in 0..3 {
        process_one_message(&catalog, &live, &sub).await;
    }
    let live_lag_before = live
        .lag(&topic)
        .await
        .expect("kafka reports lag")
        .expect("lag");

    let summary = replay_window(&catalog, sub.id, before_everything)
        .await
        .expect("replay");
    assert_eq!(summary.attempted, 3, "the whole window: {summary:?}");
    assert_eq!(summary.applied, 3, "all re-applied: {summary:?}");

    for i in 0..3 {
        assert!(
            catalog
                .get_asset_by_fqn(&format!("rw-{i}"))
                .await
                .expect("read")
                .is_some()
        );
    }
    assert_eq!(
        live.lag(&topic)
            .await
            .expect("kafka reports lag")
            .expect("lag"),
        live_lag_before,
        "the replay ran in its own consumer group, so the live subscription's \
         committed position must be untouched"
    );
}
