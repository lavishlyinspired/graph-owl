//! Graph traversal: neighbours and bounded subgraph extraction.
//!
//! Separate from `graph-owl-query` because these are graph algorithms, not
//! query-language features. A SPARQL property path answers *whether* B is
//! reachable from A; it cannot return the shortest path, the cycles, or a
//! bounded subgraph around a seed set. And expressing a 3-hop walk as three
//! joined patterns materialises the intermediate cross-products — a frontier
//! walk does not (`plans/07a-engine-traversal.md`).
//!
//! This crate is the port and the vocabulary. The frontier walk itself is a
//! recursive CTE and therefore lives in the Postgres adapter: one statement
//! per traversal beats N round trips by enough that it is worth being a
//! backend-shaped method rather than a portable-but-chatty one.

use async_trait::async_trait;
use graph_owl_core::flake::Sid;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TraversalError {
    #[error("traversal backend failed: {0}")]
    Backend(String),
}

/// Which way to walk.
///
/// `Incoming` is not a convenience — it is the direction that answers "what
/// feeds this table", and it is served by a different index (OPST) than
/// `Outgoing` (SPOT). Collapsing them would put half of every lineage question
/// on a sequential scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

impl Direction {
    #[must_use]
    pub fn follows_outgoing(self) -> bool {
        matches!(self, Direction::Outgoing | Direction::Both)
    }

    #[must_use]
    pub fn follows_incoming(self) -> bool {
        matches!(self, Direction::Incoming | Direction::Both)
    }
}

/// Why a traversal stopped early.
///
/// Truncation is always *reported*, never silent. A partial answer presented
/// as complete is the failure mode of every graph tool, and on a metadata
/// graph — which really does contain cycles — the alternative to bounding is
/// hanging in production rather than in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    MaxHops,
    NodeBudget,
}

/// One node the walk reached, and how far away it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Reached {
    pub node: Sid,
    /// Logical edges from the start, **not** stored hops. A relationship is
    /// reified as `entity → relationship → entity`, so one logical edge is two
    /// stored hops; reporting the stored count would double every distance in
    /// the product (`07a` decision 2).
    pub distance: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraversalResult {
    pub reached: Vec<Reached>,
    pub truncated: bool,
    pub truncation_reason: Option<TruncationReason>,
}

/// One logical edge, with what a renderer needs to draw it.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRef {
    pub from: Sid,
    pub to: Sid,
    /// `feeds`, `contains`, … Carried on the edge so a consumer can style or
    /// filter without resolving either endpoint.
    pub relationship: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Subgraph {
    pub nodes: Vec<Sid>,
    pub edges: Vec<EdgeRef>,
    pub truncated: bool,
    pub truncation_reason: Option<TruncationReason>,
}

impl Subgraph {
    /// Drops every edge with an endpoint outside the node set.
    ///
    /// Budget truncation removes nodes, and an edge left pointing at a removed
    /// node is a dangling edge — which a renderer either draws into empty space
    /// or crashes on. Internal consistency is the contract; enforcing it in one
    /// place means every construction path gets it rather than each remembering.
    #[must_use]
    pub fn without_dangling_edges(mut self) -> Self {
        let nodes = self.nodes.clone();
        self.edges
            .retain(|edge| nodes.contains(&edge.from) && nodes.contains(&edge.to));
        self
    }
}

/// Constraints applied to every algorithm.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EdgeFilter {
    /// Only these relationship types. `None` follows every edge.
    pub relationship_types: Option<Vec<String>>,
    /// Walk the graph as it stood at this transaction time. `None` is now —
    /// which is what makes time-travelling traversal fall out of Epic 4 rather
    /// than needing a mechanism of its own.
    pub as_of: Option<i64>,
}

/// How much of the graph a traversal may touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub max_hops: usize,
    /// The walk stops once this many nodes are reached, and says so. Without
    /// it a hub node in a real estate expands to the whole graph.
    pub max_nodes: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        // Two hops shows a table's schema and its sibling tables — the question
        // people actually open an explorer to answer. 200 nodes is roughly
        // where a force-directed layout stops being readable; past that the
        // picture is a hairball whether or not it is complete.
        Self {
            max_hops: 2,
            max_nodes: 200,
        }
    }
}

#[async_trait]
pub trait TraversalEngine: Send + Sync {
    /// Every node within `bounds.max_hops` of `start`, each once, at its
    /// shortest distance.
    ///
    /// # Errors
    ///
    /// [`TraversalError::Backend`] if the walk fails.
    async fn neighbours(
        &self,
        start: &Sid,
        direction: Direction,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<TraversalResult, TraversalError>;

    /// The nodes and edges within `bounds.max_hops` of any seed, merged.
    ///
    /// Overlapping seed neighbourhoods produce one subgraph, not several: the
    /// consumer is drawing a picture, and the same node appearing twice is a
    /// rendering bug rather than extra information.
    ///
    /// # Errors
    ///
    /// [`TraversalError::Backend`] if the walk fails.
    async fn subgraph(
        &self,
        seeds: &[Sid],
        direction: Direction,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<Subgraph, TraversalError>;
}

#[cfg(test)]
mod direction_tests {
    use super::*;

    #[test]
    fn each_direction_follows_exactly_what_it_names() {
        assert!(Direction::Outgoing.follows_outgoing());
        assert!(!Direction::Outgoing.follows_incoming());

        assert!(Direction::Incoming.follows_incoming());
        assert!(!Direction::Incoming.follows_outgoing());

        assert!(Direction::Both.follows_outgoing());
        assert!(Direction::Both.follows_incoming());
    }
}

#[cfg(test)]
mod subgraph_tests {
    use super::*;

    fn sid(id: &str) -> Sid {
        Sid::dsc(id)
    }

    fn edge(from: &str, to: &str) -> EdgeRef {
        EdgeRef {
            from: sid(from),
            to: sid(to),
            relationship: "contains".to_string(),
        }
    }

    /// A subgraph whose budget dropped a node must not keep the edges that
    /// pointed at it. A renderer given a dangling edge draws it into empty
    /// space or crashes; either way the truncation has produced something
    /// worse than a smaller picture.
    #[test]
    fn edges_to_dropped_nodes_are_removed() {
        let graph = Subgraph {
            nodes: vec![sid("a"), sid("b")],
            edges: vec![edge("a", "b"), edge("b", "gone"), edge("gone", "a")],
            truncated: true,
            truncation_reason: Some(TruncationReason::NodeBudget),
        }
        .without_dangling_edges();

        assert_eq!(graph.edges, vec![edge("a", "b")]);
        assert!(
            graph.truncated,
            "pruning must not clear the truncation flag"
        );
    }

    #[test]
    fn a_consistent_subgraph_is_left_alone() {
        let graph = Subgraph {
            nodes: vec![sid("a"), sid("b")],
            edges: vec![edge("a", "b")],
            ..Subgraph::default()
        };
        assert_eq!(graph.clone().without_dangling_edges(), graph);
    }

    #[test]
    fn an_empty_subgraph_survives_pruning() {
        assert_eq!(
            Subgraph::default().without_dangling_edges(),
            Subgraph::default()
        );
    }

    /// A self-loop is a legitimate edge — a table feeding itself through a
    /// recursive view — and both its endpoints are the same present node.
    #[test]
    fn a_self_loop_is_kept_when_its_node_is_present() {
        let graph = Subgraph {
            nodes: vec![sid("a")],
            edges: vec![edge("a", "a")],
            ..Subgraph::default()
        };
        assert_eq!(graph.without_dangling_edges().edges.len(), 1);
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    /// Every default here is a number someone has to justify. Two hops is the
    /// question people open an explorer to ask; the node cap is where a
    /// force-directed layout stops being readable.
    #[test]
    fn the_defaults_are_bounded_and_small() {
        let bounds = Bounds::default();
        assert_eq!(bounds.max_hops, 2);
        assert_eq!(bounds.max_nodes, 200);
    }

    /// An unbounded traversal on a metadata graph — which really does contain
    /// cycles — hangs in production rather than in tests.
    #[test]
    fn a_default_traversal_cannot_be_unbounded() {
        let bounds = Bounds::default();
        assert!(bounds.max_hops > 0 && bounds.max_nodes > 0);
    }
}
