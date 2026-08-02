//! Kafka/Redpanda consumption — Epic 19 Slice A.
//!
//! Deliberately thin: this module owns talking to the broker (subscribe,
//! receive, commit, honour `start_position`) and knows nothing about
//! `Catalog` — `graph-owl-connectors` has no dependency on `graph-owl-api`,
//! the same boundary the Postgres source connector already respects.
//! `graph-owl-server` is where a received message becomes a mapped,
//! applied, resolved entity, the same composition-root role it already
//! plays for connector runs.

use rdkafka::ClientConfig;
use rdkafka::consumer::{Consumer, ConsumerContext, Rebalance, StreamConsumer};
use rdkafka::error::KafkaError;
use rdkafka::message::Message;
use rdkafka::topic_partition_list::{Offset, TopicPartitionList, TopicPartitionListElem};

use graph_owl_storage::StartPosition;

#[derive(Debug)]
pub enum StreamError {
    Connection(String),
    Receive(String),
    Commit(String),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamError::Connection(message) => write!(f, "connection failed: {message}"),
            StreamError::Receive(message) => write!(f, "receive failed: {message}"),
            StreamError::Commit(message) => write!(f, "commit failed: {message}"),
        }
    }
}

impl std::error::Error for StreamError {}

impl From<KafkaError> for StreamError {
    fn from(error: KafkaError) -> Self {
        StreamError::Connection(error.to_string())
    }
}

/// One received message, with enough to map, apply and later commit —
/// deliberately not `rdkafka`'s own borrowed message type, which cannot
/// outlive the poll that produced it and would leak that crate's lifetime
/// into every caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMessage {
    pub partition: i32,
    pub offset: i64,
    pub payload: Vec<u8>,
}

/// Applies `start_position` on a partition's **first-ever** assignment to
/// this consumer group — `enable.auto.commit` is off (Slice B: offsets
/// commit only after apply), so a partition with no committed offset is the
/// only signal available that this is genuinely the first assignment, not a
/// restart. `Earliest`/`Latest` are handled by `auto.offset.reset` before a
/// rebalance ever runs; this context exists for `Timestamp`/`Offset`, which
/// `auto.offset.reset` has no vocabulary for.
struct RebalanceHandler {
    start_position: StartPosition,
}

impl rdkafka::ClientContext for RebalanceHandler {}

impl ConsumerContext for RebalanceHandler {
    /// Epic 19 Slice E: "on revoke, in-flight work for revoked partitions
    /// completes and commits before release."
    ///
    /// Both halves are structural rather than coded here. **Completes**:
    /// this callback runs on the same thread that drives `recv`, so it
    /// cannot interleave with a message being applied — by the time it
    /// runs, `process_one_message` is between messages, never inside one.
    /// **Commits**: `commit_consumer_state` flushes whatever the last
    /// successful apply committed, so a partition is never released while
    /// this consumer still holds an acknowledged-but-unflushed position.
    ///
    /// An error is logged, not propagated — there is nothing to propagate
    /// to (librdkafka calls this, not us), and the failure is safe: an
    /// uncommitted offset is redelivered to whoever takes the partition
    /// next, which is decision 2's own backstop.
    fn pre_rebalance(
        &self,
        base_consumer: &rdkafka::consumer::BaseConsumer<Self>,
        rebalance: &Rebalance<'_>,
    ) {
        if let Rebalance::Revoke(revoked) = rebalance
            && let Err(error) =
                base_consumer.commit_consumer_state(rdkafka::consumer::CommitMode::Sync)
        {
            tracing::warn!(
                partitions = revoked.count(),
                %error,
                "committing before releasing revoked partitions failed; \
                 their uncommitted messages will be redelivered"
            );
        }
    }

    fn post_rebalance(
        &self,
        base_consumer: &rdkafka::consumer::BaseConsumer<Self>,
        rebalance: &Rebalance<'_>,
    ) {
        let Rebalance::Assign(assigned) = rebalance else {
            return;
        };
        // `seek` requires a partition that is already actively fetching,
        // which a just-assigned one is not yet — calling it here reliably
        // fails with "erroneous state" (found by an end-to-end test against
        // a real broker, not anticipated). The correct pattern is the one
        // librdkafka's own C examples use for a custom rebalance callback:
        // build the assignment you actually want and call `assign` with it,
        // rather than accepting the default assignment and moving it
        // afterwards.
        match self.start_position {
            // The default assignment `assigned` already *is* what
            // `auto.offset.reset` produces — nothing to override.
            StartPosition::Earliest | StartPosition::Latest => {}
            StartPosition::Offset { value } => {
                let mut wanted = TopicPartitionList::clone(assigned);
                for element in assigned.elements() {
                    if let Err(error) = wanted.set_partition_offset(
                        element.topic(),
                        element.partition(),
                        Offset::Offset(value),
                    ) {
                        tracing::error!(
                            topic = element.topic(),
                            partition = element.partition(),
                            %error,
                            "setting the configured start offset failed"
                        );
                    }
                }
                if let Err(error) = base_consumer.assign(&wanted) {
                    tracing::error!(%error, "assigning with the configured start offset failed");
                }
            }
            StartPosition::Timestamp { at } => {
                let mut lookup = TopicPartitionList::new();
                for element in assigned.elements() {
                    if let Err(error) = lookup.add_partition_offset(
                        element.topic(),
                        element.partition(),
                        Offset::Offset(at.timestamp_millis()),
                    ) {
                        tracing::error!(
                            topic = element.topic(),
                            partition = element.partition(),
                            %error,
                            "building the timestamp lookup for a rebalanced partition failed"
                        );
                    }
                }
                match base_consumer.offsets_for_times(lookup, std::time::Duration::from_secs(5)) {
                    Ok(resolved) => {
                        let mut wanted = TopicPartitionList::clone(assigned);
                        for element in resolved.elements() {
                            if let Err(error) = wanted.set_partition_offset(
                                element.topic(),
                                element.partition(),
                                element.offset(),
                            ) {
                                tracing::error!(
                                    topic = element.topic(),
                                    partition = element.partition(),
                                    %error,
                                    "setting the resolved start timestamp offset failed"
                                );
                            }
                        }
                        if let Err(error) = base_consumer.assign(&wanted) {
                            tracing::error!(
                                %error,
                                "assigning with the configured start timestamp failed"
                            );
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "resolving offsets for the configured start timestamp failed");
                    }
                }
            }
        }
    }
}

/// A live subscription to one Kafka/Redpanda topic.
pub struct KafkaConsumer {
    consumer: StreamConsumer<RebalanceHandler>,
}

impl KafkaConsumer {
    /// # Errors
    ///
    /// [`StreamError::Connection`] if the client cannot be built or the
    /// subscription cannot be registered.
    pub fn connect(
        bootstrap_servers: &str,
        topic: &str,
        consumer_group: &str,
        start_position: StartPosition,
    ) -> Result<Self, StreamError> {
        let auto_offset_reset = match start_position {
            StartPosition::Latest => "latest",
            // `Offset`/`Timestamp` still need a value here: a partition can
            // have no message at or after the requested position, and
            // `RebalanceHandler`'s seek is a no-op in that case, leaving
            // `auto.offset.reset` as what actually decides where consumption
            // starts. `earliest` never loses data in that fallback; `latest`
            // silently would.
            StartPosition::Earliest
            | StartPosition::Offset { .. }
            | StartPosition::Timestamp { .. } => "earliest",
        };
        let consumer: StreamConsumer<RebalanceHandler> = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("group.id", consumer_group)
            // Slice B's own decision: offsets commit only after apply
            // succeeds, which is meaningless if the client commits on its
            // own timer regardless of whether apply ran.
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", auto_offset_reset)
            // A broker address that resolves to both an A and an AAAA record
            // (any `localhost`-backed one, including every testcontainers
            // setup) leaves librdkafka free to try the IPv6 record first —
            // and a Docker port mapping is IPv4-only, so that attempt times
            // out before it falls back, surfacing as `BrokerTransportFailure`
            // rather than a clean, fast connection. Forcing v4 is correct
            // wherever the deployment is IPv4-only, which is every
            // environment this project runs in today.
            .set("broker.address.family", "v4")
            // Epic 19 Slice D's backpressure bound, made explicit rather
            // than inherited: librdkafka prefetches into an internal queue
            // and **pauses fetching when it fills** — that pause *is* the
            // "polling stops, memory does not grow" criterion, and this is
            // the knob that sizes the queue. 16 MB: `00a`'s idle-RSS budget
            // is 100 MB for the whole process, and a handful of concurrent
            // subscriptions each holding the (64 MB) default would blow it
            // on prefetch buffers alone; 16 MB keeps several subscriptions
            // inside the budget while still holding thousands of
            // typical-size metadata messages in flight.
            .set("queued.max.messages.kbytes", "16384")
            .create_with_context(RebalanceHandler { start_position })
            .map_err(|e| StreamError::Connection(e.to_string()))?;
        consumer
            .subscribe(&[topic])
            .map_err(|e| StreamError::Connection(e.to_string()))?;
        Ok(Self { consumer })
    }

    /// Waits for the next message. **Genuinely idle, not a busy poll** —
    /// `StreamConsumer::recv` awaits on the underlying socket via `rdkafka`'s
    /// own tokio integration, so a subscription to a topic with nothing
    /// arriving costs no CPU while it waits (Slice A's own "an empty topic
    /// idles without spinning" criterion).
    ///
    /// # Errors
    ///
    /// [`StreamError::Receive`] if the underlying client reports an error.
    pub async fn recv(&self) -> Result<RawMessage, StreamError> {
        let message = self
            .consumer
            .recv()
            .await
            .map_err(|e| StreamError::Receive(e.to_string()))?;
        Ok(RawMessage {
            partition: message.partition(),
            offset: message.offset(),
            payload: message.payload().unwrap_or(&[]).to_vec(),
        })
    }

    /// Commits **this message's offset**, meaning "everything up to and
    /// including here is done" — called only after a caller's apply has
    /// succeeded (Slice B decision 2), never before.
    ///
    /// # Errors
    ///
    /// [`StreamError::Commit`] if the broker rejects the commit.
    pub fn commit(&self, message: &RawMessage, topic: &str) -> Result<(), StreamError> {
        let mut offsets = TopicPartitionList::new();
        offsets
            .add_partition_offset(topic, message.partition, Offset::Offset(message.offset + 1))
            .map_err(|e| StreamError::Commit(e.to_string()))?;
        self.consumer
            .commit(&offsets, rdkafka::consumer::CommitMode::Sync)
            .map_err(|e| StreamError::Commit(e.to_string()))
    }

    /// Partitions this consumer currently owns — Slice C's "assigned
    /// partitions reported" criterion.
    ///
    /// # Errors
    ///
    /// [`StreamError::Connection`] if the client cannot report assignment.
    pub fn assigned_partitions(&self) -> Result<Vec<i32>, StreamError> {
        Ok(self
            .consumer
            .assignment()
            .map_err(|e| StreamError::Connection(e.to_string()))?
            .elements()
            .iter()
            .map(TopicPartitionListElem::partition)
            .collect())
    }

    /// Lag per assigned partition of `topic` — **against the broker's own
    /// high-water mark** (`fetch_watermarks`), not estimated locally, per
    /// Slice C's own criterion: an estimate drawn from what this client has
    /// already fetched into a local buffer would under-report lag for
    /// exactly the stalled-consumer case the metric exists to catch.
    /// Committed offset, not in-memory position — lag is "how far behind
    /// what has actually been processed", and `position()` would read as
    /// caught up the moment a message is *fetched*, before it is applied.
    ///
    /// # Errors
    ///
    /// [`StreamError::Connection`] if assignment, committed offsets, or
    /// watermarks cannot be read.
    pub fn lag(&self, topic: &str) -> Result<std::collections::HashMap<i32, i64>, StreamError> {
        let assignment = self
            .consumer
            .assignment()
            .map_err(|e| StreamError::Connection(e.to_string()))?;
        let committed = self
            .consumer
            .committed(std::time::Duration::from_secs(5))
            .map_err(|e| StreamError::Connection(e.to_string()))?;

        let mut lag = std::collections::HashMap::new();
        for element in assignment.elements() {
            let partition = element.partition();
            let (_low, high) = self
                .consumer
                .fetch_watermarks(topic, partition, std::time::Duration::from_secs(5))
                .map_err(|e| StreamError::Connection(e.to_string()))?;
            // `Offset::Invalid` (nothing committed yet on this partition) —
            // the whole partition is outstanding, so the committed position
            // is its start, not zero lag.
            let committed_offset = committed
                .elements_for_topic(topic)
                .into_iter()
                .find(|e| e.partition() == partition)
                .and_then(|e| e.offset().to_raw())
                .filter(|offset| *offset >= 0)
                .unwrap_or(0);
            lag.insert(partition, (high - committed_offset).max(0));
        }
        Ok(lag)
    }
}
