//! Inbound webhook events — Epic 18.
//!
//! Plain data, matching `resolution.rs`'s own precedent: the verification
//! logic (`graph-owl-connectors::webhook_signature`) and the dedup/mapping
//! decisions (Slices B, C) are pure functions elsewhere; this is the shape a
//! received event takes once some layer decides to record one.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where an inbound event sits in its own lifecycle.
///
/// Five states, not a bool, because "received but not yet mapped" and
/// "mapped but its apply failed" are different failure surfaces a dead-letter
/// view (Slice D) has to distinguish.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventState {
    /// Signature verified, persisted, not yet mapped.
    Received,
    /// Mapped to a draft, not yet applied.
    Mapped,
    /// Applied to the catalog.
    Applied,
    /// Mapping or validation failed; retained for the dead-letter queue.
    Failed,
    /// Recognized as a redelivery of an already-applied event; no effect.
    Duplicate,
}

/// One inbound webhook delivery, from signature verification onward.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundEvent {
    pub id: Uuid,
    pub endpoint: Uuid,
    /// The sender's own event identifier, when it provides one — the
    /// primary dedup key (Slice B). `None` falls back to a content hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_event_id: Option<String>,
    /// When the sender says the described state was true, for
    /// last-writer-wins ordering (Slice B) — distinct from `received_at`,
    /// which is only ever arrival order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_timestamp: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub raw: Vec<u8>,
    pub state: EventState,
    /// What [`dedup_key`] computed for this delivery — stored rather than
    /// recomputed, so a later replay (Slice D) compares against exactly the
    /// key a redelivery was judged against, not a value that could drift if
    /// the computation ever changes.
    pub dedup_key: String,
}

/// The dedup key a delivery is judged against: the sender's own event id
/// when it provides one, else a content hash of the raw bytes — Slice B's
/// "no sender id → content-hash dedup" criterion.
///
/// A prefix (`id:`/`hash:`) keeps the two spaces from colliding — a sender
/// whose event id happens to look like a hex digest must never be treated as
/// the same delivery as an unrelated payload that hashes to that string.
#[must_use]
pub fn dedup_key(sender_event_id: Option<&str>, raw: &[u8]) -> String {
    if let Some(id) = sender_event_id {
        format!("id:{id}")
    } else {
        use sha2::{Digest, Sha256};
        format!("hash:{:x}", Sha256::digest(raw))
    }
}

/// Where a candidate event's claimed state falls relative to what is already
/// current — Slice B's last-writer-wins comparison.
///
/// **Not yet wired against real entity state.** Nothing before Slice C's
/// mapping identifies *which* entity a payload describes, so there is no
/// "current state" to compare against outside a test. This function is the
/// pure decision Slice C wires up once that identity exists; building it now
/// means the comparison itself — the part a mutation can silently invert —
/// is proven before anything depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// At least as new as current state; safe to apply.
    Newer,
    /// Older than current state; applying it would revert fresh metadata to
    /// stale — must not overwrite.
    Older,
    /// No `sender_timestamp` to compare. Falls back to arrival order, and
    /// the caller must log a warning: an unordered claim about state is a
    /// real ambiguity, not a default silently assumed to be safe.
    Ambiguous,
}

/// Compares a candidate event's `sender_timestamp` to the timestamp already
/// current for whatever it describes.
#[must_use]
pub fn compare_timestamps(candidate: Option<DateTime<Utc>>, current: DateTime<Utc>) -> Freshness {
    match candidate {
        None => Freshness::Ambiguous,
        Some(t) if t >= current => Freshness::Newer,
        Some(_) => Freshness::Older,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_sender_event_id_produces_the_same_key() {
        assert_eq!(
            dedup_key(Some("evt-1"), b"payload-a"),
            dedup_key(Some("evt-1"), b"payload-b"),
            "the sender's own id is the key, regardless of content"
        );
    }

    #[test]
    fn different_sender_event_ids_produce_different_keys() {
        assert_ne!(
            dedup_key(Some("evt-1"), b"payload"),
            dedup_key(Some("evt-2"), b"payload")
        );
    }

    #[test]
    fn with_no_sender_id_the_same_content_produces_the_same_key() {
        assert_eq!(
            dedup_key(None, b"identical payload"),
            dedup_key(None, b"identical payload")
        );
    }

    #[test]
    fn with_no_sender_id_different_content_produces_different_keys() {
        assert_ne!(
            dedup_key(None, b"payload one"),
            dedup_key(None, b"payload two")
        );
    }

    #[test]
    fn a_sender_id_that_looks_like_a_hash_does_not_collide_with_the_hash_space() {
        let hash_key = dedup_key(None, b"some content");
        let hex_digest = hash_key.strip_prefix("hash:").expect("hash prefix");
        assert_ne!(
            dedup_key(Some(hex_digest), b"unrelated content"),
            hash_key,
            "an id-based key must never equal a hash-based key for the same string"
        );
    }

    fn t(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn a_candidate_at_or_after_current_is_newer() {
        assert_eq!(compare_timestamps(Some(t(20)), t(10)), Freshness::Newer);
        assert_eq!(compare_timestamps(Some(t(10)), t(10)), Freshness::Newer);
    }

    #[test]
    fn a_candidate_strictly_before_current_is_older() {
        assert_eq!(compare_timestamps(Some(t(5)), t(10)), Freshness::Older);
    }

    #[test]
    fn a_candidate_with_no_timestamp_is_ambiguous() {
        assert_eq!(compare_timestamps(None, t(10)), Freshness::Ambiguous);
    }
}
