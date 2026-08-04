//! Resource limits every connection is bound by. There is no way to raise
//! these from the client side — `BoltLimits` is constructed once, by the
//! composition root, and every connection inherits it as-is.

use std::time::Duration;

/// Refuses a connection past `max_connections` at accept time, bounds every
/// PackStream length-prefixed value at `max_message_bytes`, times a running
/// query out at `query_timeout`, and bounds `RUN`'s in-flight result rows at
/// `fetch_batch_size` — see [`crate::query::RecordReceiver`] for why that
/// last one is also the streaming channel's own capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoltLimits {
    pub max_connections: usize,
    pub max_message_bytes: usize,
    pub query_timeout: Duration,
    pub fetch_batch_size: usize,
}

impl Default for BoltLimits {
    fn default() -> Self {
        Self {
            // A handful of drivers per deployment, not a public-facing pool —
            // `00a`'s operational budget assumes a small default footprint,
            // and this is a second listening port on top of it.
            max_connections: 64,
            // 16 MiB: comfortably above any single property, well below a
            // careless client's ability to hold the whole process hostage
            // with one declared length.
            max_message_bytes: 16 * 1024 * 1024,
            query_timeout: Duration::from_secs(30),
            // Matches `graph-owl-api::SparqlBudget`'s reasoning at a smaller
            // scale: enough rows in flight that no realistic PULL stalls
            // waiting on the evaluator, small enough that a 100k-row result
            // never holds more than a page of it in memory at once.
            fetch_batch_size: 1000,
        }
    }
}
