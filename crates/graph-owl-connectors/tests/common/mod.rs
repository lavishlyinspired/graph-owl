//! One Kafka container per test binary — the same reasoning as the
//! Postgres tests' own setup, without the per-test database trick: Kafka
//! has no equivalent lightweight "create a fresh namespace" primitive, so
//! each test uses a uniquely named topic instead, which is isolation enough
//! without a second container.
//!
//! Deliberately a separate `testcontainers`/`testcontainers-modules` pin
//! from the workspace-wide one Postgres uses — see `19-streaming.md`
//! decision 6.

use testcontainers_modules::{
    kafka::apache::Kafka,
    testcontainers::{ContainerAsync, ImageExt, ReuseDirective, runners::AsyncRunner},
};

/// One name for the whole project, so every test binary and every run
/// attaches to the same broker instead of starting its own. Without this a
/// `OnceCell`-held container is never dropped and testcontainers never
/// reaps it — 11 leaked Kafka brokers accumulated in one session before
/// this was added, which is the exact failure CLAUDE.md already documents
/// for Postgres (146 containers, 3x slowdown). A *named* container cannot
/// accumulate: there is only ever the one.
///
///     docker rm -f graph-owl-kafka-tests
const SHARED_KAFKA_CONTAINER: &str = "graph-owl-kafka-tests";

static SHARED: tokio::sync::OnceCell<ContainerAsync<Kafka>> = tokio::sync::OnceCell::const_new();

/// The running container's bootstrap address, starting it on first call.
///
/// **The `apache` (`KRaft`) image, not the `confluent`/Zookeeper one this
/// module re-exports as its default.** Both patch `advertised.listeners` to
/// the real mapped port after start, but `apache`'s startup script blocks
/// on Kafka's own "started" log line before the container is considered up
/// — the confluent image's `.start()` returns as soon as the *process*
/// launches, before that reconfiguration has necessarily taken effect,
/// which produced exactly the symptom this comment is next to
/// (`MessageTimedOut` on the first produce after a fresh container).
pub async fn bootstrap_servers() -> String {
    let container = SHARED
        .get_or_init(|| async {
            Kafka::default()
                .with_container_name(SHARED_KAFKA_CONTAINER)
                .with_reuse(ReuseDirective::Always)
                .start()
                .await
                .expect("kafka container should start")
        })
        .await;
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(9092)
        .await
        .expect("container port");
    format!("{host}:{port}")
}

/// A topic name unique to this test, so tests never see each other's
/// messages even though they share one broker.
pub fn unique_topic() -> String {
    format!("test-{}", uuid::Uuid::new_v4())
}

/// A consumer group name unique to this test — group membership and
/// rebalance state persist at the broker across a group's lifetime, so
/// reusing one name across tests would let one test's leftover state (a
/// group generation mid-rebalance, an uncommitted assignment) leak into
/// the next.
pub fn unique_group() -> String {
    format!("test-group-{}", uuid::Uuid::new_v4())
}
