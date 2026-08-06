//! The query-execution port `RUN`/`PULL`/`DISCARD` call through — Epic 7b's
//! Cypher module, so `RUN` has **no second execution path** from the one
//! `POST /cypher` already uses.
//!
//! **Streamed, not collected.** A [`QueryEngine`] hands back a
//! [`RecordReceiver`] rather than a materialized `Vec` of rows — the
//! acceptance criterion this exists for is that a 100k-row result held under
//! `fetch_batch_size` 1000 must hold bounded server memory, and a `Vec`
//! large enough to hold every row is exactly the shape that criterion is
//! designed to catch.

use graph_owl_core::Principal;
use graph_owl_lpg::{LossyMapping, LpgEdge, LpgNode};

/// One value in a [`BoltRow`].
///
/// Not [`crate::packstream::BoltValue`] directly: this is the typed shape a
/// query result carries, before it is known which structure signature a
/// [`LpgNode`]/[`LpgEdge`] should be encoded as — that mapping is
/// `crate::messages`' job, kept separate so this port does not have to know
/// about the wire format at all.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Node(LpgNode),
    Relationship(LpgEdge),
}

impl RecordValue {
    /// Encode for the wire — `crate::messages` owns the LPG-to-structure
    /// mapping, since it also owns every other structure this server emits.
    #[must_use]
    pub fn into_bolt_value(self) -> crate::packstream::BoltValue {
        use crate::packstream::BoltValue;
        match self {
            RecordValue::Null => BoltValue::Null,
            RecordValue::Boolean(b) => BoltValue::Boolean(b),
            RecordValue::Integer(n) => BoltValue::Integer(n),
            RecordValue::Float(f) => BoltValue::Float(f),
            RecordValue::String(s) => BoltValue::String(s),
            RecordValue::Node(node) => crate::messages::bolt_node(&node),
            RecordValue::Relationship(edge) => crate::messages::bolt_relationship(&edge),
        }
    }
}

/// One row of a result, **in the order the query projected its variables** —
/// unlike SPARQL's `SparqlOutcome`, whose `BTreeMap` sorts them away before
/// a Bolt client would ever see them. A driver's `RETURN a, r, b` expects
/// `a`, `r`, `b` back in that order; alphabetising it would silently swap a
/// client's columns.
///
/// **Carries its own `lossy` mappings**, mirroring `graph_owl_api::CypherRow`
/// one layer down — a `QueryEngine` implementor is expected to forward
/// whatever its own projection discovered rather than drop it here, so
/// `PULL`'s `drain` (`crate::server`) has something to accumulate into its
/// `SUCCESS` summary. See Epic 7c decision 2.
#[derive(Debug, Clone, PartialEq)]
pub struct BoltRow {
    /// The bound values, in projection order.
    pub values: Vec<(String, RecordValue)>,
    /// What projecting this row's own values could not carry across.
    pub lossy: Vec<LossyMapping>,
}

/// What `RUN`'s `SUCCESS` reports before any row has arrived: the column
/// names, in projection order, so a driver can label rows as they stream in
/// rather than waiting for the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub fields: Vec<String>,
}

/// Why a query could not run, or failed partway through streaming.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    /// Refused before execution — includes decision 2's write-clause
    /// refusal, which already happens at Epic 7b's parse layer and needs no
    /// separate check here.
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Storage(String),
    #[error("query exceeded its time budget")]
    Timeout,
}

/// The receiving half of a streamed result.
///
/// Wraps any `Stream`, not a concrete channel — a `QueryEngine` implementor
/// (in practice, `graph-owl-server`'s adapter over `Catalog`) builds one by
/// mapping whatever streaming shape `graph-owl-api` exposes directly into
/// [`BoltRow`]s, with no extra forwarding channel in between. The bound
/// between "how many rows may run ahead of the connection pulling them" and
/// "how many rows `PULL`/`DISCARD` physically buffer" is still the one
/// number in `BoltLimits::fetch_batch_size`, enforced wherever the
/// implementor's own channel is sized — this type just does not have to
/// know what that channel is.
pub struct RecordReceiver(
    std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<BoltRow, QueryError>> + Send>>,
);

impl RecordReceiver {
    pub fn new(
        stream: impl tokio_stream::Stream<Item = Result<BoltRow, QueryError>> + Send + 'static,
    ) -> Self {
        Self(Box::pin(stream))
    }

    pub async fn recv(&mut self) -> Option<Result<BoltRow, QueryError>> {
        use tokio_stream::StreamExt;
        self.0.next().await
    }
}

/// Runs a query and streams its results, scoped to one principal.
///
/// **Authorization is not a separate check here.** Whatever implements this
/// trait is expected to route through the same compiled predicate SPARQL and
/// Cypher-over-HTTP already use — decision 5 — so a `RecordReceiver` never
/// yields a row this principal could not otherwise see.
#[async_trait::async_trait]
pub trait QueryEngine: Send + Sync {
    async fn run(
        &self,
        principal: &Principal,
        query: &str,
    ) -> Result<(RunOutcome, RecordReceiver), QueryError>;
}
