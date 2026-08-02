//! Pulsar consumption — Epic 19 Slice F.
//!
//! Mirrors [`crate::streaming::KafkaConsumer`]'s shape (connect / recv /
//! commit) so `graph-owl-server`'s orchestration is one code path over two
//! brokers rather than two pipelines. What it deliberately does **not** do
//! is imitate Kafka's vocabulary: Pulsar's equivalents are named for what
//! they actually are, and where no equivalent exists that is stated rather
//! than faked.

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
}
