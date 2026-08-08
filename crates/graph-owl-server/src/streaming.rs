//! Epic 19 Slice A: the composition root for streaming subscriptions.
//!
//! `graph-owl-connectors::streaming` owns talking to the broker and knows
//! nothing about `Catalog`; `graph-owl-api`'s `apply_streamed_message` owns
//! mapping, applying and resolving one message and knows nothing about a
//! broker. This module is the only place that depends on both, the same
//! composition-root role this crate already plays for connector runs.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use graph_owl_api::Catalog;
use graph_owl_connectors::streaming::{KafkaConsumer, RawMessage, StreamError};
use graph_owl_storage::{BrokerConfig, StreamSubscription};
use uuid::Uuid;

/// Where one subscription's background consumer currently is — Epic 19
/// Slice C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumerState {
    Starting,
    Consuming,
    Failed,
}

/// What `/ready` and the lag gauge need to know about one running
/// consumer — the plan's own `ConsumerHealth` sketch, minus
/// `assigned_partitions`/`lag_per_partition`, which are reported straight
/// to Prometheus rather than duplicated here (a health snapshot that could
/// disagree with the metric it is describing is worse than one source).
#[derive(Debug, Clone)]
pub struct ConsumerHealth {
    pub state: ConsumerState,
    /// `None` until the first successful commit — a freshly started
    /// consumer that has not yet had the chance to apply anything is not
    /// "stalled", it just started. Distinguishing the two is the whole
    /// point of exposing this at all (Slice C's "a consumer that is alive
    /// but not progressing" criterion).
    pub last_commit: Option<chrono::DateTime<chrono::Utc>>,
}

/// Every running consumer's health, process-wide. A `LazyLock`, not
/// threaded through `Extension`/router state like `RateLimiter`: the
/// registry has to be reachable from `main.rs`'s startup routine (before
/// the router exists at all) and from every `run_consumer` task, and it is
/// inherently one-per-process state rather than one-per-request — the
/// `Extension` pattern exists to make per-request state reachable inside a
/// handler, which is not the problem here.
static CONSUMER_HEALTH: LazyLock<Mutex<HashMap<Uuid, ConsumerHealth>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn set_health(id: Uuid, health: ConsumerHealth) {
    CONSUMER_HEALTH.lock().unwrap().insert(id, health);
}

/// Marks a consumer failed **while preserving whatever `last_commit` it had**
/// — in a single lock acquisition.
///
/// One function rather than read-then-`set_health`, because the obvious
/// spelling of that deadlocks: a `CONSUMER_HEALTH.lock()` written as an
/// *argument* to `set_health` is a temporary whose guard lives until the end
/// of the enclosing statement, so `set_health`'s own `lock()` blocks on a
/// `std::sync::Mutex` that is not reentrant. It cost four test timeouts, and
/// in production it would have hung `/ready` for the whole server the first
/// time a broker went away.
fn mark_failed(id: Uuid) {
    let mut health = CONSUMER_HEALTH.lock().unwrap();
    health
        .entry(id)
        .and_modify(|existing| existing.state = ConsumerState::Failed)
        .or_insert(ConsumerHealth {
            state: ConsumerState::Failed,
            last_commit: None,
        });
}

/// Whether every registered consumer is healthy, and the failing ones by
/// subscription id — what `/ready` needs to fail readiness on a genuinely
/// broken consumer rather than staying silently green.
///
/// # Panics
///
/// Never under normal operation — only if the internal lock is poisoned by
/// an earlier panic elsewhere in this process, in which case the state it
/// protects cannot be trusted anyway.
#[must_use]
pub fn all_healthy() -> (bool, Vec<Uuid>) {
    let health = CONSUMER_HEALTH.lock().unwrap();
    let failed: Vec<Uuid> = health
        .iter()
        .filter(|(_, h)| h.state == ConsumerState::Failed)
        .map(|(id, _)| *id)
        .collect();
    (failed.is_empty(), failed)
}

/// `graph_owl_<subsystem>_<noun>_<unit>` — the observability contract's own
/// naming convention, matching `graph_owl_webhook_events_total`'s
/// precedent. Base unit: a count of messages, not a rate or a percentage.
const STREAM_CONSUMER_LAG: &str = "graph_owl_stream_consumer_lag";

/// How often lag is re-measured — Slice C's own stall-detection criterion
/// needs this to run **independently of message processing**, since a
/// stalled consumer is, by definition, not triggering any other code path.
/// Five seconds: frequent enough that a scrape (Prometheus's own default
/// interval is 15s, per `10-operability.md`) always sees a fresh value,
/// infrequent enough that polling the broker for watermarks is not itself
/// a meaningful load.
const HEALTH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to wait after a failed receive before trying again, and the
/// ceiling that backoff climbs to.
///
/// **Without this the consume loop spins.** `recv` against an unreachable
/// broker fails *immediately*, so `loop { process_one_message() }` becomes a
/// tight loop burning a core per dead subscription — found when a test
/// against a deliberately-unreachable broker hung for 78 minutes at full
/// CPU rather than failing. Doubling from 100ms to 30s means a transient
/// blip costs almost nothing while a broker that is genuinely down is
/// retried twice a minute instead of millions of times.
const RECV_BACKOFF_INITIAL: std::time::Duration = std::time::Duration::from_millis(100);
const RECV_BACKOFF_MAX: std::time::Duration = std::time::Duration::from_secs(30);

/// Not this consumer's primary resilience mechanism — librdkafka already
/// retries many transient broker-communication failures internally before a
/// commit call ever returns an error to this code. This is a thin margin on
/// top of that, small enough to add no meaningful latency to the loop.
/// Beyond it, leaving the message uncommitted is the correct behaviour
/// (decision 2's own backstop: uncommitted means redelivered, never lost),
/// not a failure this loop should block indefinitely trying to avoid.
const COMMIT_RETRY_ATTEMPTS: u32 = 3;

/// Epic 19 Slice B: "a commit failure is retried and does not lose the
/// applied state." Immediate retry, no backoff — `COMMIT_RETRY_ATTEMPTS`'s
/// own reasoning is why neither is needed here.
fn commit_with_retry(
    consumer: &StreamConsumer,
    message: &RawMessage,
    topic: &str,
) -> Result<(), StreamError> {
    let mut last_error = None;
    for attempt in 0..COMMIT_RETRY_ATTEMPTS {
        match consumer.commit(message, topic) {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::warn!(
                    attempt,
                    partition = message.partition,
                    offset = message.offset,
                    %error,
                    "commit attempt failed"
                );
                last_error = Some(error);
            }
        }
    }
    Err(last_error.expect("the loop runs at least once, so an error was always recorded"))
}

/// Starts a background consumer for one subscription. Fire-and-forget, by
/// design: a durable subscription (decision 1) runs for the server's
/// lifetime, not for one request's — there is no caller left to hand a
/// result back to once this returns.
pub fn spawn_consumer(catalog: Catalog, subscription: StreamSubscription) {
    tokio::spawn(run_consumer(catalog, subscription));
}

/// Starts every currently-enabled subscription — called once at server
/// startup so a restart resumes consumption (from each subscription's last
/// committed offset) rather than silently going quiet until someone
/// re-registers it.
///
/// # Errors
///
/// `CatalogError` if listing subscriptions fails.
pub async fn spawn_enabled_subscriptions(
    catalog: &Catalog,
) -> Result<(), graph_owl_api::CatalogError> {
    for subscription in catalog.list_stream_subscriptions().await? {
        if subscription.enabled {
            spawn_consumer(catalog.clone(), subscription);
        }
    }
    Ok(())
}

/// One broker's consumer, behind the one interface the orchestration uses.
///
/// An enum rather than a trait object: the set is closed at two (the two
/// client crates `19-streaming.md` decision 6 adopted), both are
/// constructed in exactly one place, and a trait would need `async fn` in
/// trait objects for `recv` — real complexity bought for a third
/// implementation nobody has proposed.
pub enum StreamConsumer {
    Kafka(KafkaConsumer),
    /// Boxed: the Pulsar client is ~648 bytes against Kafka's ~24, and
    /// every `StreamConsumer` — including every Kafka one — would otherwise
    /// carry that footprint.
    Pulsar(Box<graph_owl_connectors::streaming_pulsar::PulsarConsumer>),
}

impl StreamConsumer {
    /// `pub` for the same reason `process_one_message` is: Slice B's
    /// kill-and-restart test needs to receive a message and deliberately
    /// *not* commit it, which no other entry point offers.
    ///
    /// # Errors
    ///
    /// [`StreamError::Receive`] if the underlying client reports one.
    pub async fn recv(&self) -> Result<RawMessage, StreamError> {
        match self {
            StreamConsumer::Kafka(c) => c.recv().await,
            StreamConsumer::Pulsar(c) => c.recv().await,
        }
    }

    fn commit(&self, message: &RawMessage, topic: &str) -> Result<(), StreamError> {
        match self {
            StreamConsumer::Kafka(c) => c.commit(message, topic),
            StreamConsumer::Pulsar(c) => c.commit(message, topic),
        }
    }

    /// `None` for Pulsar when no admin REST URL was configured: lag there is
    /// *subscription backlog*, which lives on a separate HTTP surface from
    /// the binary protocol this consumer speaks, not derivable from it.
    /// Reporting a fabricated zero would be worse than reporting nothing —
    /// see `19-streaming.md` Slice F. **Wired for real 8 August 2026**
    /// (`plans/EPIC-COMPLETION-PLAN.md` Phase 2.8): when a broker's
    /// `admin_url` is configured, this fetches real backlog from it.
    ///
    /// Async, unlike Kafka's own `lag` — `rdkafka`'s watermark query is
    /// synchronous, but Pulsar's is an HTTP round trip, and the two live
    /// behind one method because [`report_lag_periodically`] polls both the
    /// same way.
    ///
    /// # Errors
    ///
    /// [`StreamError::Connection`] if watermarks cannot be read, or the
    /// admin REST call fails.
    #[must_use]
    pub async fn lag(&self, topic: &str) -> Option<Result<HashMap<i32, i64>, StreamError>> {
        match self {
            StreamConsumer::Kafka(c) => Some(c.lag(topic)),
            StreamConsumer::Pulsar(c) => c.lag(topic).await,
        }
    }
}

async fn run_consumer(catalog: Catalog, subscription: StreamSubscription) {
    set_health(
        subscription.id,
        ConsumerHealth {
            state: ConsumerState::Starting,
            last_commit: None,
        },
    );
    let connected = match &subscription.broker {
        BrokerConfig::KafkaProtocol { bootstrap_servers } => KafkaConsumer::connect(
            bootstrap_servers,
            &subscription.topic,
            &subscription.consumer_group,
            subscription.start_position,
        )
        .map(StreamConsumer::Kafka),
        BrokerConfig::Pulsar {
            service_url,
            admin_url,
        } => graph_owl_connectors::streaming_pulsar::PulsarConsumer::connect(
            service_url,
            &subscription.topic,
            // Pulsar's subscription name is what owns cursor position,
            // the same role a Kafka consumer group plays — so the same
            // configured field feeds both.
            &subscription.consumer_group,
            admin_url.as_deref(),
        )
        .await
        .map(|c| StreamConsumer::Pulsar(Box::new(c))),
    };
    let consumer = match connected {
        Ok(consumer) => Arc::new(consumer),
        Err(error) => {
            tracing::error!(
                topic = %subscription.topic,
                consumer_group = %subscription.consumer_group,
                %error,
                "failed to connect the stream consumer"
            );
            set_health(
                subscription.id,
                ConsumerHealth {
                    state: ConsumerState::Failed,
                    last_commit: None,
                },
            );
            return;
        }
    };
    set_health(
        subscription.id,
        ConsumerHealth {
            state: ConsumerState::Consuming,
            last_commit: None,
        },
    );
    tokio::spawn(report_lag_periodically(
        Arc::clone(&consumer),
        subscription.topic.clone(),
    ));

    let mut backoff = RECV_BACKOFF_INITIAL;
    loop {
        if process_one_message(&catalog, &consumer, &subscription).await {
            backoff = RECV_BACKOFF_INITIAL;
        } else {
            // Nothing received — a broker that is down, or a subscription
            // whose partitions were revoked. Wait before retrying, or this
            // loop spins at full CPU for as long as the condition lasts.
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(RECV_BACKOFF_MAX);
        }
    }
}

/// Runs independently of message processing — Slice C's own stall-detection
/// criterion needs lag to keep updating even when a consumer is alive but
/// applying nothing, which by definition never touches
/// `process_one_message`.
async fn report_lag_periodically(consumer: Arc<StreamConsumer>, topic: String) {
    let mut interval = tokio::time::interval(HEALTH_POLL_INTERVAL);
    loop {
        interval.tick().await;
        let Some(measured) = consumer.lag(&topic).await else {
            // Pulsar with no admin URL configured: no lag surface. Stop
            // polling rather than spin re-discovering that every interval.
            return;
        };
        match measured {
            Ok(lag_per_partition) => {
                for (partition, lag) in lag_per_partition {
                    // Lag beyond 2^53 messages is not a number any dashboard
                    // renders meaningfully, and a Prometheus gauge is f64 by
                    // definition — the precision limit is the metric format's
                    // own contract, not a lossy choice made here.
                    #[allow(clippy::cast_precision_loss)]
                    let lag = lag as f64;
                    metrics::gauge!(
                        STREAM_CONSUMER_LAG,
                        "topic" => topic.clone(),
                        "partition" => partition.to_string()
                    )
                    .set(lag);
                }
            }
            Err(error) => {
                tracing::warn!(topic = %topic, %error, "failed to read consumer lag");
            }
        }
    }
}

/// What replaying a window did — Epic 19 Slice E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamReplaySummary {
    pub attempted: u32,
    pub applied: u32,
    pub failed: u32,
}

/// How long a replay waits between messages before concluding the window is
/// exhausted. A replay reads a *finite* historical range, so it needs a way
/// to know it has reached the end — Kafka offers no end-of-window signal,
/// only "nothing has arrived yet". Three seconds is far longer than a broker
/// takes to serve an already-buffered historical message.
const REPLAY_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// How long to wait for the **first** message, which is a different question
/// from the one above and was originally, wrongly, given the same answer.
///
/// A replay runs in its own consumer group (that is what keeps it from
/// disturbing live offsets), so before any message can arrive it must join
/// that group, be assigned partitions, and have `offsets_for_times` resolve
/// the timestamp. That handshake routinely takes several seconds against a
/// real broker — longer than the idle timeout — so sharing one constant made
/// every replay report `attempted: 0` and call it an empty window. Thirty
/// seconds is generous for the handshake and still bounded.
const REPLAY_FIRST_MESSAGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Re-processes a subscription's messages from `since` — Epic 19 Slice E.
///
/// **Runs as its own consumer group**, which is the whole reason replay is
/// safe to run against a live subscription: group membership is what owns
/// committed offsets in Kafka, so a replay in a *different* group cannot
/// move the live consumer's position, and the live consumer keeps
/// delivering while the replay runs. The criterion "replay does not disturb
/// the live subscription's offsets" is therefore structural, not a rule
/// this function has to remember to follow.
///
/// Idempotent for the same reason a redelivery is (decision 2): apply is
/// FQN-keyed upsert plus Epic 17 resolution, so re-applying a message that
/// already landed converges rather than duplicating.
///
/// # Errors
///
/// `CatalogError::NotFound` if no such subscription. A broker that cannot
/// be reached is reported as `Internal`.
pub async fn replay_window(
    catalog: &Catalog,
    subscription_id: Uuid,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<StreamReplaySummary, graph_owl_api::CatalogError> {
    let subscription = catalog
        .stream_subscription(subscription_id)
        .await?
        .ok_or(graph_owl_api::CatalogError::NotFound)?;
    let BrokerConfig::KafkaProtocol { bootstrap_servers } = &subscription.broker else {
        return Err(graph_owl_api::CatalogError::Storage(
            graph_owl_storage::StorageError::Unexpected(
                "replay is only implemented for Kafka-protocol brokers (Slice F covers Pulsar)"
                    .to_string(),
            ),
        ));
    };

    let consumer = KafkaConsumer::connect(
        bootstrap_servers,
        &subscription.topic,
        // A fresh group per replay — never `subscription.consumer_group`.
        &format!("graph-owl-replay-{}", Uuid::new_v4()),
        graph_owl_storage::StartPosition::Timestamp { at: since },
    )
    .map(StreamConsumer::Kafka)
    .map_err(|error| {
        graph_owl_api::CatalogError::Storage(graph_owl_storage::StorageError::Unexpected(format!(
            "failed to connect the replay consumer: {error}"
        )))
    })?;

    let mut summary = StreamReplaySummary::default();
    loop {
        // The first wait covers the group-join handshake; every later one is
        // asking the genuinely different question "is there more?".
        let patience = if summary.attempted == 0 {
            REPLAY_FIRST_MESSAGE_TIMEOUT
        } else {
            REPLAY_IDLE_TIMEOUT
        };
        let Ok(received) = tokio::time::timeout(patience, consumer.recv()).await else {
            break; // the window is exhausted
        };
        let Ok(message) = received else {
            continue;
        };
        summary.attempted += 1;
        let applied = match serde_json::from_slice::<serde_json::Value>(&message.payload) {
            Ok(payload) => catalog
                .apply_streamed_message(&subscription.mapping, &payload)
                .await
                .is_ok(),
            Err(_) => false,
        };
        if applied {
            summary.applied += 1;
        } else {
            summary.failed += 1;
        }
    }
    Ok(summary)
}

/// Receives, maps, applies and (only on success) commits exactly one
/// message. Extracted out of `run_consumer`'s loop body and made `pub` for
/// Slice B's kill-and-restart test, which needs to drive a controlled
/// number of iterations and then stop — an infinite loop offers no such
/// point to a caller from outside the module.
///
/// # Panics
///
/// Never on a message-level failure — a bad payload, a mapping rejection or
/// a commit failure is logged and this returns, leaving the offset
/// uncommitted so the message is redelivered (decision 2). A `recv` failure
/// is the one case with no message to log against.
pub async fn process_one_message(
    catalog: &Catalog,
    consumer: &StreamConsumer,
    subscription: &StreamSubscription,
) -> bool {
    let message = match consumer.recv().await {
        Ok(message) => message,
        Err(error) => {
            tracing::error!(topic = %subscription.topic, %error, "receive failed");
            // A broker a consumer cannot reach fails here, on the first
            // `recv` — `KafkaConsumer::connect` itself succeeds even against
            // an unreachable broker (subscribing only registers interest;
            // rdkafka's connection attempts happen in the background and
            // surface here, not at construction). `/ready`'s "a failed
            // consumer" criterion is checked against this state, not
            // against whether `connect` returned `Ok`.
            mark_failed(subscription.id);
            return false;
        }
    };

    // Slice D: `poison_threshold` attempts, then quarantine and advance.
    // One in-place retry loop rather than relying on broker redelivery,
    // because within a running consumer librdkafka's position has already
    // moved past a received message — redelivery only happens on restart,
    // which would turn "retry this message" into "block until someone
    // bounces the server". A parse failure is deterministic and fails each
    // attempt trivially; a transient apply failure (storage blip) gets a
    // real second chance. Uniform loop, no special cases to reason about.
    let mut last_failure = String::new();
    let mut applied = false;
    for _ in 0..subscription.poison_threshold.max(1) {
        let result = match serde_json::from_slice::<serde_json::Value>(&message.payload) {
            Ok(payload) => catalog
                .apply_streamed_message(&subscription.mapping, &payload)
                .await
                .map_err(|error| format!("{error:?}")),
            Err(error) => Err(format!("payload is not valid JSON: {error}")),
        };
        match result {
            Ok(()) => {
                applied = true;
                break;
            }
            Err(failure) => last_failure = failure,
        }
    }

    if !applied {
        // Quarantined, and the consumer advances (the commit below) — the
        // criterion: one bad message must not starve everything behind it.
        // The commit is correct even though apply failed, because the
        // message is no longer "unprocessed": it is preserved, in full, in
        // the dead-letter queue, replayable after a fix.
        tracing::error!(
            topic = %subscription.topic,
            partition = message.partition,
            offset = message.offset,
            failure = %last_failure,
            "message quarantined after exhausting poison_threshold attempts"
        );
        if let Err(error) = catalog
            .record_stream_dead_letter(graph_owl_storage::StreamDeadLetter {
                id: Uuid::new_v4(),
                subscription_id: subscription.id,
                topic: subscription.topic.clone(),
                partition: message.partition,
                offset: message.offset,
                payload: message.payload.clone(),
                reason: last_failure,
                created_at: chrono::Utc::now(),
            })
            .await
        {
            // The DLQ write itself failed — do NOT commit, or the message
            // would be lost entirely (neither applied nor preserved). Left
            // uncommitted for redelivery, decision 2's backstop.
            tracing::error!(
                topic = %subscription.topic,
                offset = message.offset,
                "failed to record the dead letter; leaving the offset uncommitted: {error:?}"
            );
            return true;
        }
    }

    // Committed only now — decision 2. A crash between apply and commit
    // reprocesses this message on restart; a crash before apply never
    // advanced the offset at all. Either way nothing is lost: Epic 18's
    // dedup makes a reprocessed apply harmless, and a quarantined message
    // is preserved in the DLQ before its offset moves.
    match commit_with_retry(consumer, &message, &subscription.topic) {
        Ok(()) => set_health(
            subscription.id,
            ConsumerHealth {
                state: ConsumerState::Consuming,
                last_commit: Some(chrono::Utc::now()),
            },
        ),
        Err(error) => {
            // Left uncommitted after exhausting retries — still safe
            // (decision 2's own backstop: an uncommitted offset is
            // redelivered, never lost), just not retried forever. A broker
            // down long enough to exhaust these must not block this
            // consumer from ever reaching its next message.
            tracing::error!(
                topic = %subscription.topic,
                partition = message.partition,
                offset = message.offset,
                %error,
                "commit failed after retrying"
            );
        }
    }
    true
}
