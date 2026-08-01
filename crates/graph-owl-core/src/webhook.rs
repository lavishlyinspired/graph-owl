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
}
