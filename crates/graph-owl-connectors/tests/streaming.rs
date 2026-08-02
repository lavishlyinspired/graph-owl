//! Epic 19 Slice A against a real Kafka broker — the low-level consumer,
//! not yet wired to `Catalog` (that orchestration lives in
//! `graph-owl-server`, which is the only crate that depends on both this
//! one and the facade).

mod common;

use graph_owl_connectors::streaming::KafkaConsumer;
use graph_owl_storage::StartPosition;
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;

async fn produce(bootstrap_servers: &str, topic: &str, payload: &str) {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .create()
        .expect("producer should build");
    producer
        .send(
            FutureRecord::to(topic).payload(payload).key("test-key"),
            Duration::from_secs(10),
        )
        .await
        .expect("send should succeed");
}

#[tokio::test]
async fn a_produced_message_is_received() {
    let bootstrap_servers = common::bootstrap_servers().await;
    let topic = common::unique_topic();
    produce(&bootstrap_servers, &topic, "hello").await;

    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &common::unique_group(),
        StartPosition::Earliest,
    )
    .expect("connect");

    let received = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
        .await
        .expect("should not time out")
        .expect("recv should succeed");
    assert_eq!(received.payload, b"hello");
}

/// **The idle test.** A consumer subscribed to a topic that never receives
/// anything must not spin: `recv()` awaits the underlying socket rather
/// than polling in a tight loop. Measured directly, not inferred: process
/// CPU time before and after a multi-second wait must stay near zero.
#[tokio::test]
async fn an_idle_consumer_does_not_spin_cpu() {
    let bootstrap_servers = common::bootstrap_servers().await;
    let topic = common::unique_topic();
    // Registering the topic (by producing nothing to it directly, but
    // connecting) is enough — Kafka auto-creates topics on first reference
    // by default, which is what `subscribe` triggers.
    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &common::unique_group(),
        StartPosition::Latest,
    )
    .expect("connect");

    let cpu_before = cpu_time_ms();
    let result = tokio::time::timeout(Duration::from_secs(3), consumer.recv()).await;
    let cpu_after = cpu_time_ms();

    // A real message is the only failure this test is about. A transient
    // receive error (e.g. while a freshly auto-created topic's metadata is
    // still propagating) racing the 3s window is not the thing being
    // tested here and must not be misread as "a message arrived".
    assert!(
        !matches!(result, Ok(Ok(_))),
        "an empty topic should not deliver a real message: {result:?}"
    );
    let spent = cpu_after - cpu_before;
    assert!(
        spent < 500,
        "an idle consumer should not burn CPU polling: {spent}ms of process CPU time over a 3s wait"
    );
}

fn cpu_time_ms() -> i64 {
    // `getrusage` gives the whole process's CPU time, which is what proves
    // "not spinning" regardless of which thread `recv()` happens to run on.
    use std::mem::MaybeUninit;
    unsafe {
        let mut usage = MaybeUninit::<libc::rusage>::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr());
        let usage = usage.assume_init();
        let user_ms = usage.ru_utime.tv_sec * 1000 + i64::from(usage.ru_utime.tv_usec) / 1000;
        let sys_ms = usage.ru_stime.tv_sec * 1000 + i64::from(usage.ru_stime.tv_usec) / 1000;
        user_ms + sys_ms
    }
}

/// `StartPosition::Offset` — a message produced *before* the requested
/// offset must not be delivered; the point of the offset is to skip it.
#[tokio::test]
async fn start_position_offset_skips_earlier_messages() {
    let bootstrap_servers = common::bootstrap_servers().await;
    let topic = common::unique_topic();
    produce(&bootstrap_servers, &topic, "skip-me").await;
    produce(&bootstrap_servers, &topic, "read-me").await;

    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &common::unique_group(),
        StartPosition::Offset { value: 1 },
    )
    .expect("connect");

    let received = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
        .await
        .expect("should not time out")
        .expect("recv should succeed");
    assert_eq!(received.payload, b"read-me");
}

/// `StartPosition::Timestamp` — the resolve-via-`offsets_for_times` path,
/// distinct code from `Offset`'s direct seek and not exercised by any test
/// above it. A message produced *before* the requested timestamp must not
/// be delivered.
#[tokio::test]
async fn start_position_timestamp_skips_earlier_messages() {
    let bootstrap_servers = common::bootstrap_servers().await;
    let topic = common::unique_topic();
    produce(&bootstrap_servers, &topic, "skip-me").await;

    // A real gap, not a race against clock resolution: the cutoff is well
    // after the first message and well before the second.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let cutoff = chrono::Utc::now();
    tokio::time::sleep(Duration::from_millis(500)).await;
    produce(&bootstrap_servers, &topic, "read-me").await;

    let consumer = KafkaConsumer::connect(
        &bootstrap_servers,
        &topic,
        &common::unique_group(),
        StartPosition::Timestamp { at: cutoff },
    )
    .expect("connect");

    let received = tokio::time::timeout(Duration::from_secs(30), consumer.recv())
        .await
        .expect("should not time out")
        .expect("recv should succeed");
    assert_eq!(received.payload, b"read-me");
}
