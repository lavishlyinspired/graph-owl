//! Pulsar consumption — Epic 19 Slice F.
//!
//! Mirrors [`crate::streaming::KafkaConsumer`]'s shape (connect / recv /
//! commit) so `graph-owl-server`'s orchestration is one code path over two
//! brokers rather than two pipelines. What it deliberately does **not** do
//! is imitate Kafka's vocabulary: Pulsar's equivalents are named for what
//! they actually are, and where no equivalent exists that is stated rather
//! than faked.

use std::collections::HashMap;

use futures::TryStreamExt;
use pulsar::message::proto::command_subscribe::SubType;
use pulsar::{Consumer, Pulsar, TokioExecutor};

use crate::streaming::{RawMessage, StreamError};

/// A live subscription to one Pulsar topic.
///
/// **`Key_Shared`, not `Shared` or `Failover`** — the closest equivalent to
/// a Kafka consumer group: `Shared` round-robins with no ordering guarantee
/// (Kafka guarantees per-partition order, which the whole out-of-order
/// design in Epic 18 relies on), and `Failover` idles standbys instead of
/// splitting work. `Key_Shared` routes messages with the same key to the
/// same consumer consistently, which is Kafka's partition-key behaviour
/// under a different name.
pub struct PulsarConsumer {
    consumer: tokio::sync::Mutex<Consumer<Vec<u8>, TokioExecutor>>,
    /// The subscription name — Pulsar's own cursor-owning concept, the same
    /// role a Kafka consumer group plays. Stored rather than re-threaded
    /// through every call: [`Self::lag`] needs it to address the admin
    /// REST API's per-subscription backlog, and it is otherwise only ever
    /// used once, at [`Self::connect`] time.
    subscription: String,
    /// The admin REST API's own base URL — Epic 19 Slice F, completed 8
    /// August 2026. `None` when a deployment has not configured it, in
    /// which case [`Self::lag`] reports nothing, exactly as before this was
    /// wired: subscription backlog lives on a separate HTTP surface from
    /// the binary protocol `service_url` speaks, so it cannot be derived
    /// from that URL (see `graph_owl_storage::BrokerConfig::Pulsar`).
    admin_url: Option<String>,
}

impl PulsarConsumer {
    /// # Errors
    ///
    /// [`StreamError::Connection`] if the client or the subscription cannot
    /// be built.
    pub async fn connect(
        service_url: &str,
        topic: &str,
        subscription: &str,
        admin_url: Option<&str>,
    ) -> Result<Self, StreamError> {
        let pulsar: Pulsar<_> = Pulsar::builder(service_url, TokioExecutor)
            .build()
            .await
            .map_err(|e| StreamError::Connection(e.to_string()))?;
        let consumer = pulsar
            .consumer()
            .with_topic(topic)
            .with_subscription(subscription)
            .with_subscription_type(SubType::KeyShared)
            .build()
            .await
            .map_err(|e| StreamError::Connection(e.to_string()))?;
        Ok(Self {
            consumer: tokio::sync::Mutex::new(consumer),
            subscription: subscription.to_string(),
            admin_url: admin_url.map(ToString::to_string),
        })
    }

    /// Waits for the next message.
    ///
    /// `partition`/`offset` on the returned [`RawMessage`] are **Pulsar's
    /// message id**, not Kafka coordinates: `partition` is the topic
    /// partition index (`-1` on a non-partitioned topic, which is what a
    /// plain `persistent://.../topic` is), and `offset` is the entry id.
    /// They exist so one `RawMessage` type serves both brokers; only
    /// [`Self::commit`] interprets them, and only for logging.
    ///
    /// # Errors
    ///
    /// [`StreamError::Receive`] if the client reports an error or the
    /// stream ends.
    pub async fn recv(&self) -> Result<RawMessage, StreamError> {
        let mut consumer = self.consumer.lock().await;
        let message = consumer
            .try_next()
            .await
            .map_err(|e| StreamError::Receive(e.to_string()))?
            .ok_or_else(|| StreamError::Receive("the consumer stream ended".to_string()))?;
        let id = message.message_id();
        let raw = RawMessage {
            partition: id.partition.unwrap_or(-1),
            offset: id.entry_id.try_into().unwrap_or(i64::MAX),
            payload: message.payload.data.clone(),
        };
        // Acked here rather than in `commit`, because Pulsar's ack takes the
        // *message* and `commit`'s cross-broker signature only carries the
        // coordinates — see `commit`'s own note for why that is safe.
        self.pending_ack(&mut consumer, &message).await;
        Ok(raw)
    }

    async fn pending_ack(
        &self,
        consumer: &mut Consumer<Vec<u8>, TokioExecutor>,
        message: &pulsar::consumer::Message<Vec<u8>>,
    ) {
        // Cumulative-ack semantics are deliberately not used: a single ack
        // marks exactly this message, so an unacked one is redelivered on
        // its own rather than dragging everything after it back with it.
        if let Err(error) = consumer.ack(message).await {
            tracing::warn!(%error, "acking a Pulsar message failed; it will be redelivered");
        }
    }

    /// **A no-op for Pulsar, deliberately** — and the one place the
    /// two-broker abstraction does not line up.
    ///
    /// Kafka's commit is a separate call the orchestration makes only after
    /// a successful apply (decision 2). Pulsar's ack takes the message
    /// value itself, which cannot survive being handed back as bare
    /// coordinates, so the ack happens in [`Self::recv`] instead. The
    /// consequence is honest and worth stating: **Pulsar is
    /// at-most-once-per-delivery where Kafka is at-least-once** — a crash
    /// between ack and apply loses that message rather than redelivering
    /// it. Closing that gap means holding the un-acked message across the
    /// apply, which needs a different orchestration shape than the one
    /// Slices A–E built for Kafka; it is recorded in `19-streaming.md` as
    /// the known gap rather than silently papered over here.
    ///
    /// # Errors
    ///
    /// Never — the signature matches [`crate::streaming::KafkaConsumer::commit`]
    /// so one orchestration serves both.
    #[allow(clippy::unnecessary_wraps, clippy::unused_self)]
    pub fn commit(&self, _message: &RawMessage, _topic: &str) -> Result<(), StreamError> {
        Ok(())
    }

    /// Subscription backlog, from the admin REST API — `None` when this
    /// consumer's broker was not configured with one.
    ///
    /// A single `HashMap` entry at key `-1`, matching Kafka's own
    /// `lag()`'s per-partition shape: Pulsar's backlog is a property of
    /// the *subscription* on the topic as a whole, not something this API
    /// breaks out by partition (`msgBacklog` in the stats response, itself
    /// unpartitioned for a non-partitioned topic — the only kind this
    /// project's own `topic` configuration produces).
    ///
    /// # Errors
    ///
    /// [`StreamError::Connection`] if the request fails, cannot be parsed,
    /// or the subscription named at connect time is not present in the
    /// response — the last of which means the subscription has not been
    /// created on the broker yet (Pulsar creates it lazily, on first
    /// consumer connection).
    pub async fn lag(&self, topic: &str) -> Option<Result<HashMap<i32, i64>, StreamError>> {
        let admin_url = self.admin_url.as_deref()?;
        Some(fetch_backlog(admin_url, topic, &self.subscription).await)
    }
}

/// A free function, not a method — the only state it needs is three
/// strings, and keeping it that way lets it be tested against a real local
/// admin-REST double without a live Pulsar broker to construct a
/// [`PulsarConsumer`] against.
async fn fetch_backlog(
    admin_url: &str,
    topic: &str,
    subscription: &str,
) -> Result<HashMap<i32, i64>, StreamError> {
    let (tenant, namespace, short_name) = parse_topic(topic);
    let url = format!(
        "{}/admin/v2/persistent/{tenant}/{namespace}/{short_name}/stats",
        admin_url.trim_end_matches('/')
    );
    let response = reqwest::get(&url)
        .await
        .map_err(|e| StreamError::Connection(e.to_string()))?
        .error_for_status()
        .map_err(|e| StreamError::Connection(e.to_string()))?;
    let stats: TopicStats = response
        .json()
        .await
        .map_err(|e| StreamError::Connection(e.to_string()))?;
    let backlog = stats
        .subscriptions
        .get(subscription)
        .ok_or_else(|| {
            StreamError::Connection(format!(
                "subscription `{subscription}` is not in {url}'s response yet — Pulsar \
                 creates a subscription on first connection, so this is expected briefly \
                 after a fresh registration"
            ))
        })?
        .msg_backlog;
    // `-1`, not the partition index this crate's own `RawMessage` uses for
    // a non-partitioned topic: that `-1` names *a message's own origin*,
    // and reusing it here would make an unrelated convention look
    // load-bearing. This key means "the whole topic", the only grouping
    // Pulsar's own backlog concept has for one it did not partition.
    Ok(HashMap::from([(-1, backlog)]))
}

/// The Pulsar admin REST API's own JSON shape for `GET .../stats`, reduced
/// to the one field [`PulsarConsumer::lag`] reads — confirmed against the
/// official Apache Pulsar documentation (`admin-api-topics`), 8 August
/// 2026: `subscriptions` is an object keyed by subscription name, and each
/// entry's `msgBacklog` is the count of messages in backlog for it.
#[derive(serde::Deserialize)]
struct TopicStats {
    subscriptions: HashMap<String, SubscriptionStats>,
}

#[derive(serde::Deserialize)]
struct SubscriptionStats {
    #[serde(rename = "msgBacklog")]
    msg_backlog: i64,
}

/// `topic` into `(tenant, namespace, short_name)` for the admin REST path.
///
/// **`public`/`default` for a short name, not invented defaults** — Pulsar's
/// own documented behaviour for an unqualified topic name (verified against
/// the official docs, 8 August 2026): every topic this project's own
/// `StreamSubscription::topic` configures is a short name (see
/// `graph-owl-server/tests/streaming.rs`'s own fixtures), so this is the
/// common case, not a fallback for a case that should not arise. A fully
/// qualified `persistent://tenant/namespace/name` is still accepted, for a
/// deployment that names one that way.
fn parse_topic(topic: &str) -> (&str, &str, &str) {
    let without_scheme = topic
        .strip_prefix("persistent://")
        .or_else(|| topic.strip_prefix("non-persistent://"))
        .unwrap_or(topic);
    let mut parts = without_scheme.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(tenant), Some(namespace), Some(name)) => (tenant, namespace, name),
        _ => ("public", "default", without_scheme),
    }
}

#[cfg(test)]
mod parse_topic_tests {
    use super::parse_topic;

    #[test]
    fn a_short_name_defaults_to_public_default() {
        assert_eq!(parse_topic("dbt.runs"), ("public", "default", "dbt.runs"));
    }

    #[test]
    fn a_fully_qualified_topic_is_split_into_its_three_parts() {
        assert_eq!(
            parse_topic("persistent://acme/prod/dbt.runs"),
            ("acme", "prod", "dbt.runs")
        );
    }

    #[test]
    fn a_non_persistent_topic_is_recognised_too() {
        assert_eq!(
            parse_topic("non-persistent://acme/prod/dbt.runs"),
            ("acme", "prod", "dbt.runs")
        );
    }
}

/// **Epic 19 Slice F, completed 8 August 2026** (`plans/EPIC-COMPLETION-PLAN.md`
/// Phase 2.8). Against a real local admin-REST double, not a mock library —
/// the same "a real local server, not a stub" reasoning Epic 101's
/// federation tests already established for SPARQL `SERVICE`.
#[cfg(test)]
mod fetch_backlog_tests {
    use super::fetch_backlog;
    use axum::{Json, Router, routing::get};
    use serde_json::{Value, json};

    /// A one-route admin-REST double answering every `.../stats` request
    /// with the same fixed body, on an OS-assigned port so tests never
    /// collide.
    async fn admin_double(response: Value) -> String {
        let app = Router::new().route(
            "/admin/v2/persistent/{tenant}/{namespace}/{topic}/stats",
            get(move || {
                let response = response.clone();
                async move { Json(response) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind an ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn the_named_subscriptions_backlog_is_read() {
        let admin_url = admin_double(json!({
            "subscriptions": { "graph-owl": { "msgBacklog": 42 } }
        }))
        .await;

        let lag = fetch_backlog(&admin_url, "dbt.runs", "graph-owl")
            .await
            .expect("lag");

        assert_eq!(lag.get(&-1), Some(&42));
    }

    /// The negative: a subscription this call did *not* name must not leak
    /// its own backlog into the answer — `-1` always means the one this
    /// consumer connected as, never whichever key happened to be first.
    #[tokio::test]
    async fn a_different_subscriptions_backlog_is_not_returned() {
        let admin_url = admin_double(json!({
            "subscriptions": {
                "someone-elses-subscription": { "msgBacklog": 999 },
                "graph-owl": { "msgBacklog": 7 }
            }
        }))
        .await;

        let lag = fetch_backlog(&admin_url, "dbt.runs", "graph-owl")
            .await
            .expect("lag");

        assert_eq!(lag.get(&-1), Some(&7), "{lag:?}");
    }

    /// A subscription absent from the response — the lazily-created-on-first-
    /// connect case `fetch_backlog`'s own doc names — is a named error, not
    /// a panic or a silent zero.
    #[tokio::test]
    async fn a_subscription_absent_from_the_response_is_a_named_error() {
        let admin_url = admin_double(json!({
            "subscriptions": { "someone-elses-subscription": { "msgBacklog": 1 } }
        }))
        .await;

        let error = fetch_backlog(&admin_url, "dbt.runs", "graph-owl")
            .await
            .expect_err("the named subscription is not in the response");

        assert!(
            error.to_string().contains("graph-owl"),
            "the error must name the subscription it looked for: {error}"
        );
    }
}
