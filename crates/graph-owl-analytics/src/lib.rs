//! Graph analytics: degree centrality, connected components, `PageRank`, and orphan detection —
//! Epic 38.
//!
//! **Pure, zero I/O.** Every function here takes an already-built [`GraphProjection`] or the
//! flakes to build one from, and returns a value. No storage port, no network, no clock read
//! beyond what a caller stamps on the result. This is what makes the crate exhaustively
//! mutation-testable and what makes the memory bound the caller's explicit decision — see
//! `plans/38-graph-analytics.md` for the four questions this answers and the one (ranking) it
//! deliberately still refuses.
//!
//! **Narrow on purpose.** Four algorithms, each tied to a named governance or operational
//! question. Not a graph data-science library — see `00e-crate-architecture.md`'s "Purity
//! boundary" entry for why this is the one place a caller-supplied projection earns a crate.

use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use std::collections::BTreeMap;

/// An index into [`GraphProjection::nodes`] — dense, zero-based, assigned by
/// [`project`] in a deterministic (sorted-`Sid`) order. Never the flake
/// store's own identity: that stays a [`Sid`], reachable via `nodes[id]`.
pub type NodeId = usize;

/// Directed adjacency as compressed sparse row, backed by
/// [`petgraph::csr::Csr`] — the storage and row layout are petgraph's own;
/// this wrapper adds only what petgraph's `Csr` does not expose itself: a
/// flat, contiguous edge-index space spanning every row (`row_offsets`,
/// derived once from petgraph's own [`petgraph::csr::Csr::out_degree`]),
/// which is what lets [`GraphProjection::weights`] stay a single flat `Vec`
/// aligned index-for-index with edges rather than one `Vec` per node.
///
/// Node and edge weights are left as `()`: `graph-owl-analytics` weighting
/// is a caller-supplied, post-projection concern
/// ([`GraphProjection::weights`]), not something the adjacency structure
/// itself carries — see `plans/00l-build-vs-adopt.md`'s petgraph entry for
/// why folding weights into petgraph's own per-edge slot was rejected
/// (nothing in this crate ever constructs a weighted [`Csr`] directly; only
/// tests mutate `weights` after the fact, which a per-edge weight slot
/// cannot express without rebuilding the structure).
#[derive(Debug, Clone)]
pub struct CsrGraph {
    inner: petgraph::csr::Csr<(), (), petgraph::Directed, NodeId>,
    /// `row_offsets[i]..row_offsets[i + 1]` is node `i`'s outgoing edges'
    /// position in the flat edge-index space — one more entry than there
    /// are nodes, the last being the total edge count. Prefix-summed once
    /// from `inner.out_degree`, not duplicated bookkeeping: petgraph's `Csr`
    /// keeps the equivalent internally but does not make it public.
    row_offsets: Vec<usize>,
}

impl CsrGraph {
    /// Build from a node count and edges already sorted by `(from, to)` —
    /// the same order [`project`]'s own `BTreeSet<(Sid, Sid)>` iterates in
    /// once mapped through a sort-order-preserving `Sid -> NodeId` index.
    /// `node_count` is taken explicitly, not inferred from the edges,
    /// because an isolated node (no edge at all — exactly what orphan
    /// detection needs to see) would otherwise vanish: petgraph's own
    /// `Csr::from_sorted_edges` sizes itself from the edges alone.
    fn from_sorted_edges(node_count: usize, edges: &[(NodeId, NodeId)]) -> Self {
        let mut inner = petgraph::csr::Csr::with_nodes(node_count);
        for &(from, to) in edges {
            inner.add_edge(from, to, ());
        }
        let mut row_offsets = Vec::with_capacity(node_count + 1);
        row_offsets.push(0);
        let mut acc = 0;
        for node in 0..node_count {
            acc += inner.out_degree(node);
            row_offsets.push(acc);
        }
        Self { inner, row_offsets }
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.inner.edge_count()
    }

    /// `node`'s outgoing neighbours, in the deterministic order [`project`]
    /// built them (sorted by the neighbour's own [`Sid`]).
    #[must_use]
    pub fn out_neighbours(&self, node: NodeId) -> &[NodeId] {
        self.inner.neighbors_slice(node)
    }

    /// The half-open range into the flat edge-index space — and, when
    /// present, a projection's own [`GraphProjection::weights`] — for
    /// `node`'s outgoing edges. Exposed so a caller can walk edges by index
    /// rather than only by neighbour, which is what reading a per-edge
    /// weight aligned to the same index space needs.
    #[must_use]
    pub fn out_edge_range(&self, node: NodeId) -> std::ops::Range<usize> {
        self.row_offsets[node]..self.row_offsets[node + 1]
    }

    /// The neighbour at `edge_index`, a flat index as returned by
    /// [`Self::out_edge_range`]. `row_offsets` is sorted, so the owning row
    /// is a binary search away; bounded graph sizes (`AnalyticsBudget`) keep
    /// this cheap relative to petgraph's own O(1) row-local indexing.
    #[must_use]
    pub fn target(&self, edge_index: usize) -> NodeId {
        let row = self.row_offsets.partition_point(|&x| x <= edge_index) - 1;
        self.inner.neighbors_slice(row)[edge_index - self.row_offsets[row]]
    }
}

/// Two [`CsrGraph`]s are equal when every node has the same outgoing
/// neighbours in the same order — `petgraph::csr::Csr` implements no
/// `PartialEq` of its own, and this is exactly the property `row_offsets`
/// plus `col_indices` equality expressed before the petgraph migration:
/// equal per-node neighbour lists imply equal degrees, and prefix sums of
/// equal degree sequences are themselves equal.
impl PartialEq for CsrGraph {
    fn eq(&self, other: &Self) -> bool {
        self.node_count() == other.node_count()
            && (0..self.node_count()).all(|n| self.out_neighbours(n) == other.out_neighbours(n))
    }
}

impl Eq for CsrGraph {}

/// A graph, projected from the catalog's flakes into the shape every
/// algorithm in this crate wants — decision 3 of `38-graph-analytics.md`:
/// the caller builds this once, every algorithm reads it, nothing here
/// touches storage again.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphProjection {
    /// Node `i`'s catalog identity — the projection's own answer to "which
    /// asset was this".
    pub nodes: Vec<Sid>,
    pub adjacency: CsrGraph,
    /// Per-edge confidence, aligned index-for-index with
    /// `adjacency`'s own `col_indices` — present only when the caller asked
    /// for a weighted projection.
    pub weights: Option<Vec<f32>>,
    /// The edge types [`project`] was asked to include. Carried on the
    /// projection itself, not just passed once to `project` and forgotten,
    /// so anything computed from it — [`connected_components`]'s own
    /// orphan/silo report most of all — can name what it actually
    /// considered. "Orphan" means nothing without saying which edges were
    /// looked at, and a report that does not name its filter produces
    /// confident wrong conclusions in a governance review.
    pub edge_types: Vec<Sid>,
}

/// How large a projection (or a run over one) may grow before it refuses —
/// decision 4: **bounded by construction**. A metadata graph at this
/// project's target scale fits comfortably; the guard exists so the failure
/// at 100× is an error message, not an outage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalyticsBudget {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_iterations: usize,
}

/// A projection refused rather than silently sampled or swapped — decision
/// 4's "never silently sample" made structural: the caller gets a typed
/// refusal naming both numbers, not a truncated graph that looks whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
    #[error("projection would include {actual} nodes, budget permits {permitted}")]
    TooManyNodes { actual: usize, permitted: usize },
    #[error("projection would include {actual} edges, budget permits {permitted}")]
    TooManyEdges { actual: usize, permitted: usize },
}

/// The object of an edge-typed flake, when it has one. A literal object
/// cannot be a graph neighbour — there is nothing on the other end for an
/// edge to point to.
fn edge_target(flake: &Flake) -> Option<&Sid> {
    match &flake.o {
        FlakeValue::Ref(sid) => Some(sid),
        _ => None,
    }
}

/// Build a [`GraphProjection`] from a flake scan, keeping only the
/// requested edge types.
///
/// **`all_nodes` is what makes orphan detection possible at all.** A node
/// touched by no matching edge would otherwise never enter the projection
/// — it cannot appear as an edge endpoint if it has no edge — which would
/// make [`connected_components`]'s own size-1 "orphan" reading unable to
/// find the single most important case it exists for: an asset connected
/// to *nothing*. Every `Sid` in `all_nodes` gets a [`NodeId`] and a
/// (possibly empty) adjacency list; edge endpoints not already in
/// `all_nodes` are added the same way `project` always added them.
///
/// **Deterministic.** Nodes are numbered in sorted-`Sid` order and each
/// node's own adjacency list is sorted by neighbour `Sid` — the same input
/// always yields the same [`NodeId`] assignment and the same iteration
/// order, which is what makes a downstream result (`PageRank`'s own
/// convergence path, a components list's ordering) reproducible rather
/// than wobbling between runs on identical data because a `HashSet`
/// happened to iterate differently.
///
/// **Budget checked before allocation, not after.** A projection that
/// builds the oversized structure and only then reports it exceeded the
/// budget has already paid the cost the budget exists to avoid.
///
/// # Errors
///
/// [`ProjectionError::TooManyNodes`] or [`ProjectionError::TooManyEdges`] if
/// the projection would exceed `budget`.
pub fn project(
    all_nodes: &[Sid],
    flakes: &[Flake],
    edge_types: &[Sid],
    budget: &AnalyticsBudget,
) -> Result<GraphProjection, ProjectionError> {
    let edges: Vec<(&Sid, &Sid)> = flakes
        .iter()
        .filter(|f| f.op && edge_types.contains(&f.p))
        .filter_map(|f| edge_target(f).map(|o| (&f.s, o)))
        .collect();

    let mut node_set: std::collections::BTreeSet<Sid> = all_nodes.iter().cloned().collect();
    for (from, to) in &edges {
        node_set.insert((*from).clone());
        node_set.insert((*to).clone());
    }
    let node_count = node_set.len();
    if node_count > budget.max_nodes {
        return Err(ProjectionError::TooManyNodes {
            actual: node_count,
            permitted: budget.max_nodes,
        });
    }
    // Deduplicated: two flakes stating the identical edge (same s, p, o)
    // are one edge in the projection, not two — `edges` above has not been
    // deduplicated, so count against the *distinct* pairs, matching what
    // the CSR structure below actually stores.
    let mut distinct_edges: std::collections::BTreeSet<(Sid, Sid)> =
        std::collections::BTreeSet::new();
    for (from, to) in &edges {
        distinct_edges.insert(((*from).clone(), (*to).clone()));
    }
    let edge_count = distinct_edges.len();
    if edge_count > budget.max_edges {
        return Err(ProjectionError::TooManyEdges {
            actual: edge_count,
            permitted: budget.max_edges,
        });
    }

    let nodes: Vec<Sid> = node_set.into_iter().collect();
    let index_of: BTreeMap<&Sid, NodeId> = nodes.iter().enumerate().map(|(i, s)| (s, i)).collect();

    // `distinct_edges` is a `BTreeSet<(Sid, Sid)>`, so it already iterates
    // in sorted order; `index_of` is a monotonic, order-preserving map
    // (node ids are assigned in the same sorted-`Sid` order as `nodes`
    // itself), so mapping each pair through it preserves that order —
    // exactly the `(from, to)`-sorted input `CsrGraph::from_sorted_edges`
    // requires.
    let sorted_edges: Vec<(NodeId, NodeId)> = distinct_edges
        .iter()
        .map(|(from, to)| (index_of[from], index_of[to]))
        .collect();

    Ok(GraphProjection {
        adjacency: CsrGraph::from_sorted_edges(nodes.len(), &sorted_edges),
        nodes,
        weights: None,
        edge_types: edge_types.to_vec(),
    })
}

/// Which edges a degree count considers — the whole reason `Direction` is
/// its own type rather than a bare `bool`: "in" and "out" answer different
/// governance questions (who consumes this vs. who does this depend on),
/// and conflating them into one summed number erases exactly the
/// distinction a blast-radius reading needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
    Total,
}

/// One node's degree — a count, or a weighted sum when the projection
/// carries confidence weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Degree {
    pub node: NodeId,
    pub value: f64,
}

/// Degree centrality over `g`, in `dir`.
///
/// **Every node scores, including zero.** A node with no matching edges is
/// absent from no result set here — `MinCount` is the tool for requiring
/// an edge; a degree function that silently dropped zero-degree nodes
/// would make "not scored" indistinguishable from "scored zero", which are
/// different findings for a governance reader ("nothing feeds this" vs.
/// "we don't know").
///
/// **Weighted when `g.weights` is present**, summing each edge's own
/// weight instead of counting it as 1 — the same index space
/// [`CsrGraph::out_edge_range`] documents.
///
/// **Deterministic order.** Results are returned indexed by [`NodeId`] —
/// itself assigned by [`project`] in sorted-`Sid` order — so two runs over
/// the same projection tie-break identically without this function needing
/// its own comparator.
// Both `out` and `inn` are updated per iteration, keyed by the same
// index and by `g.adjacency.target(edge)` respectively — an iterator
// adapter over one of them cannot express updating the other, which is
// what makes the plain range loop clippy suggests replacing genuinely the
// clearest form here, not an oversight.
#[allow(clippy::needless_range_loop)]
#[must_use]
pub fn degree_centrality(g: &GraphProjection, dir: Direction) -> Vec<Degree> {
    let n = g.adjacency.node_count();
    let mut out = vec![0.0_f64; n];
    let mut inn = vec![0.0_f64; n];
    for node in 0..n {
        for edge in g.adjacency.out_edge_range(node) {
            let weight = g.weights.as_ref().map_or(1.0, |ws| f64::from(ws[edge]));
            out[node] += weight;
            inn[g.adjacency.target(edge)] += weight;
        }
    }
    (0..n)
        .map(|node| Degree {
            node,
            value: match dir {
                Direction::In => inn[node],
                Direction::Out => out[node],
                Direction::Total => inn[node] + out[node],
            },
        })
        .collect()
}

/// Weakly-connected components — union-find with path compression, edges
/// treated as undirected for connectivity even though [`GraphProjection`]
/// stores them directed. Ids are `NodeId`s of each component's own smallest
/// member, which is what makes them **stable across runs**: re-running
/// union-find over the identical, deterministically-numbered projection
/// always picks the same representative.
#[derive(Debug, Clone, PartialEq)]
pub struct Components {
    /// The edge types this pass actually considered — copied from
    /// [`GraphProjection::edge_types`], not re-derived, so a caller who
    /// only has this result (not the projection it came from) can still
    /// name the filter. **Orphan and silo findings are meaningless without
    /// this**: an entity connected only by a `hasOwner` edge is genuinely
    /// an orphan under a lineage-only projection and genuinely is not
    /// under a broader one, and the two must never be presented as the
    /// same finding.
    pub filter_edge_types: Vec<Sid>,
    /// Node `i`'s component id.
    pub component_of: Vec<NodeId>,
    /// Component id to member count, so a size distribution does not need
    /// a second pass over `component_of`.
    pub sizes: BTreeMap<NodeId, usize>,
}

impl Components {
    /// Nodes whose component has exactly one member — orphaned under
    /// `filter_edge_types` specifically, not orphaned in some
    /// filter-independent sense that does not exist.
    #[must_use]
    pub fn orphans(&self) -> Vec<NodeId> {
        (0..self.component_of.len())
            .filter(|&n| self.sizes[&self.component_of[n]] == 1)
            .collect()
    }
}

struct UnionFind {
    parent: Vec<NodeId>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    /// Path-compressed: every node visited on the way to the root is
    /// re-pointed straight at it, so a later `find` over the same subtree
    /// is O(1) rather than repeating the walk — the difference union-find
    /// scale assertions exist to catch, per the plan's own mutator watch
    /// ("union without path compression passes correctness").
    fn find(&mut self, node: NodeId) -> NodeId {
        if self.parent[node] != node {
            self.parent[node] = self.find(self.parent[node]);
        }
        self.parent[node]
    }

    fn union(&mut self, a: NodeId, b: NodeId) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Smaller root wins — arbitrary but deterministic, which is
            // the only property component ids need to be stable across
            // runs over the identical projection.
            //
            // `<` vs `<=` is a provably equivalent mutant here, not a
            // missing test: this branch is only ever reached with
            // `ra != rb` (the guard above), so the two operators compute
            // identically on every call this line can ever see. Confirmed
            // by mutation testing — `<=` is the one survivor left after
            // `>`, `==`, and the reachable-branch geometry are all covered.
            let (keep, drop) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[drop] = keep;
        }
    }
}

/// Directed cycles in `g` — every set of nodes each of which can reach every
/// other **following edge direction**.
///
/// **This is the one structural finding a per-edge rule cannot make, and
/// [`connected_components`] cannot make it either.** That pass is *weakly*
/// connected: it ignores direction, so `a → b → c → a` and `a → b → c` are
/// indistinguishable — both one component. The difference is the entire
/// finding, because value returning to where it started is a different claim
/// from ordinary supply passing along a chain.
///
/// Returned as the strongly-connected components of size ≥ 2. Size 1 is
/// excluded deliberately: every node is trivially strongly connected to
/// itself, so including singletons would report every node in the graph. A
/// **self-loop** is excluded for a different and more considered reason — a
/// node pointing at itself is a data-quality problem, not a cycle among
/// distinct parties, and reporting it as one fills the finding with noise
/// nobody can act on.
///
/// Iterative Tarjan rather than the recursive formulation, because the
/// recursion depth is the length of the longest path and a projection is
/// caller-supplied: a deep chain would overflow the stack on data rather than
/// on a bug.
///
/// # Panics
///
/// Never. Every index pushed to the stacks comes from `0..node_count` and
/// every lookup is bounds-checked by construction.
#[must_use]
pub fn cycles(g: &GraphProjection) -> Vec<Vec<NodeId>> {
    let n = g.adjacency.node_count();
    let unvisited = usize::MAX;

    let mut index_of = vec![unvisited; n];
    let mut low_link = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<NodeId> = Vec::new();
    let mut next_index = 0usize;
    let mut found: Vec<Vec<NodeId>> = Vec::new();

    // (node, how many of its out-edges have been consumed) — the explicit
    // frame the recursive formulation would keep on the call stack.
    let mut work: Vec<(NodeId, usize)> = Vec::new();

    for root in 0..n {
        if index_of[root] != unvisited {
            continue;
        }
        work.push((root, 0));

        while let Some(&mut (v, ref mut edge_cursor)) = work.last_mut() {
            if *edge_cursor == 0 {
                index_of[v] = next_index;
                low_link[v] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[v] = true;
            }

            let neighbours = g.adjacency.out_neighbours(v);
            if *edge_cursor < neighbours.len() {
                let w = neighbours[*edge_cursor];
                *edge_cursor += 1;
                if index_of[w] == unvisited {
                    work.push((w, 0));
                } else if on_stack[w] {
                    low_link[v] = low_link[v].min(index_of[w]);
                }
                continue;
            }

            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low_link[parent] = low_link[parent].min(low_link[v]);
            }

            if low_link[v] == index_of[v] {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                // Size 1 is every node trivially; a singleton is only a cycle
                // if it points at itself, and a self-loop is excluded above.
                if component.len() > 1 {
                    component.sort_unstable();
                    found.push(component);
                }
            }
        }
    }

    found.sort_unstable();
    found
}

/// Weakly-connected components over `g`, considering every edge
/// [`GraphProjection::edge_types`] included regardless of direction.
///
/// # Panics
///
/// Never, but see [`Components::orphans`] for the invariant that makes its
/// own lookups panic-free: every `component_of` entry is a key `sizes`
/// necessarily set, since a node's own union-find pass always registers
/// its final root as its component.
#[must_use]
pub fn connected_components(g: &GraphProjection) -> Components {
    let n = g.adjacency.node_count();
    let mut uf = UnionFind::new(n);
    for node in 0..n {
        for edge in g.adjacency.out_edge_range(node) {
            uf.union(node, g.adjacency.target(edge));
        }
    }

    let component_of: Vec<NodeId> = (0..n).map(|node| uf.find(node)).collect();
    let mut sizes: BTreeMap<NodeId, usize> = BTreeMap::new();
    for &root in &component_of {
        *sizes.entry(root).or_insert(0) += 1;
    }

    Components {
        filter_edge_types: g.edge_types.clone(),
        component_of,
        sizes,
    }
}

/// `PageRank`'s own tunables — decision 6: **on probation**, and the exit
/// criterion (Slice E's bake-off against Epic 28's usage signals) is
/// written down precisely so decision 1 stays a real narrowing rather than
/// scope creep with a nicer vocabulary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageRankConfig {
    pub damping: f32,
    pub tolerance: f32,
    pub max_iterations: usize,
}

/// One node's `PageRank` score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rank {
    pub node: NodeId,
    pub score: f64,
}

/// What one `PageRank` run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRankResult {
    pub scores: Vec<Rank>,
    /// `false` means `max_iterations` was reached before the delta between
    /// successive passes fell under `tolerance` — **never silently
    /// reported as converged**. Iteration exhaustion is the mutant that
    /// turns a wrong answer into a confident one, per the plan's own
    /// mutator watch, which is exactly why this field exists rather than
    /// the caller inferring convergence from iteration count alone.
    pub converged: bool,
}

/// Power iteration with damping, over `g`.
///
/// **Dangling nodes are handled explicitly, not silently dropped.** A node
/// with no outgoing edges would otherwise leak its rank mass out of the
/// system entirely — invisible on a small test graph, where the leak is a
/// rounding error, and a systemic skew on a real one, where every dangling
/// node quietly drains the total. Each pass sums the score sitting on
/// dangling nodes and redistributes it evenly across every node, which is
/// what keeps `PageRank` a genuine probability distribution rather than a
/// slowly-shrinking one.
///
/// **Weighted when `g.weights` is present**, distributing a node's score
/// across its out-edges in proportion to each edge's own weight rather
/// than uniformly.
// Node counts here are bounded by `AnalyticsBudget::max_nodes`, orders of
// magnitude below `f64`'s 52-bit mantissa — the same reasoning
// `graph-owl-constraint::shapes::as_number` already applies to this exact
// lint.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn pagerank(g: &GraphProjection, cfg: &PageRankConfig) -> PageRankResult {
    let n = g.adjacency.node_count();
    if n == 0 {
        return PageRankResult {
            scores: Vec::new(),
            converged: true,
        };
    }
    let n_f = n as f64;
    let damping = f64::from(cfg.damping);
    let base = (1.0 - damping) / n_f;

    let out_weight_total: Vec<f64> = (0..n)
        .map(|node| {
            g.weights.as_ref().map_or_else(
                || g.adjacency.out_neighbours(node).len() as f64,
                |ws| {
                    g.adjacency
                        .out_edge_range(node)
                        .map(|e| f64::from(ws[e]))
                        .sum()
                },
            )
        })
        .collect();

    let mut scores = vec![1.0 / n_f; n];
    let mut converged = false;
    for _ in 0..cfg.max_iterations {
        let dangling_mass: f64 = (0..n)
            .filter(|&node| out_weight_total[node] <= 0.0)
            .map(|node| scores[node])
            .sum();
        let mut next = vec![base + damping * dangling_mass / n_f; n];
        for node in 0..n {
            if out_weight_total[node] <= 0.0 {
                continue;
            }
            for edge in g.adjacency.out_edge_range(node) {
                let weight = g.weights.as_ref().map_or(1.0, |ws| f64::from(ws[edge]));
                let target = g.adjacency.target(edge);
                next[target] += damping * scores[node] * weight / out_weight_total[node];
            }
        }

        let delta: f64 = scores.iter().zip(&next).map(|(a, b)| (a - b).abs()).sum();
        scores = next;
        if delta < f64::from(cfg.tolerance) {
            converged = true;
            break;
        }
    }

    PageRankResult {
        scores: (0..n)
            .map(|node| Rank {
                node,
                score: scores[node],
            })
            .collect(),
        converged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::namespace;

    fn a(id: &str) -> Sid {
        Sid::dsc(id)
    }

    fn feeds() -> Sid {
        Sid::new(namespace::DSC, "feeds")
    }

    fn other() -> Sid {
        Sid::new(namespace::DSC, "other")
    }

    fn edge(from: &str, to: &str, p: &Sid) -> Flake {
        Flake::assert(a(from), p.clone(), FlakeValue::Ref(a(to)), 1)
    }

    fn budget() -> AnalyticsBudget {
        AnalyticsBudget {
            max_nodes: 100,
            max_edges: 100,
            max_iterations: 20,
        }
    }

    #[test]
    fn an_empty_graph_projects_without_error() {
        let projection = project(&[], &[], &[feeds()], &budget()).expect("an empty graph is valid");
        assert_eq!(projection.nodes.len(), 0);
        assert_eq!(projection.adjacency.node_count(), 0);
        assert_eq!(projection.adjacency.edge_count(), 0);
    }

    #[test]
    fn only_the_requested_edge_types_are_included() {
        let flakes = vec![edge("a", "b", &feeds()), edge("c", "d", &other())];
        let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

        assert_eq!(
            projection.nodes,
            vec![a("a"), a("b")],
            "{:?}",
            projection.nodes
        );
        assert_eq!(projection.adjacency.edge_count(), 1);
    }

    #[test]
    fn a_graph_exceeding_max_nodes_refuses_and_names_both_counts() {
        let flakes = vec![edge("a", "b", &feeds()), edge("c", "d", &feeds())];
        let tight = AnalyticsBudget {
            max_nodes: 2,
            ..budget()
        };

        let err = project(&[], &flakes, &[feeds()], &tight).expect_err("4 nodes exceeds 2");

        assert_eq!(
            err,
            ProjectionError::TooManyNodes {
                actual: 4,
                permitted: 2
            }
        );
    }

    #[test]
    fn a_graph_exceeding_max_edges_refuses_and_names_both_counts() {
        let flakes = vec![
            edge("a", "b", &feeds()),
            edge("a", "c", &feeds()),
            edge("a", "d", &feeds()),
        ];
        let tight = AnalyticsBudget {
            max_edges: 1,
            ..budget()
        };

        let err = project(&[], &flakes, &[feeds()], &tight).expect_err("3 edges exceeds 1");

        assert_eq!(
            err,
            ProjectionError::TooManyEdges {
                actual: 3,
                permitted: 1
            }
        );
    }

    /// Landing exactly on the budget is not a refusal — a `>` boundary
    /// mistakenly written `>=` would refuse a graph that fits exactly,
    /// which is the kind of off-by-one a budget check gets wrong silently
    /// (a report that never triggers here just looks like "budgets are
    /// generous today").
    #[test]
    fn landing_exactly_on_max_nodes_is_not_refused() {
        let flakes = vec![edge("a", "b", &feeds())];
        let exact = AnalyticsBudget {
            max_nodes: 2,
            ..budget()
        };

        let projection = project(&[], &flakes, &[feeds()], &exact);

        assert!(projection.is_ok(), "{projection:?}");
    }

    #[test]
    fn landing_exactly_on_max_edges_is_not_refused() {
        let flakes = vec![edge("a", "b", &feeds())];
        let exact = AnalyticsBudget {
            max_edges: 1,
            ..budget()
        };

        let projection = project(&[], &flakes, &[feeds()], &exact);

        assert!(projection.is_ok(), "{projection:?}");
    }

    /// **The determinism test.** Non-deterministic node ordering makes a
    /// downstream algorithm's results wobble between runs on identical
    /// data, which reads as a bug in the algorithm and is actually a bug in
    /// the projection — the plan's own named RED case for this slice.
    #[test]
    fn the_same_input_produces_the_same_node_ordering_every_time() {
        let flakes = vec![
            edge("zebra", "apple", &feeds()),
            edge("mango", "zebra", &feeds()),
            edge("apple", "mango", &feeds()),
        ];

        let first = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
        let second = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

        assert_eq!(first.nodes, second.nodes);
        assert_eq!(first.adjacency, second.adjacency);
        // And sorted, specifically — not merely "consistent with itself".
        assert_eq!(first.nodes, vec![a("apple"), a("mango"), a("zebra")]);
    }

    /// Two flakes stating the identical edge collapse to one — a duplicate
    /// assertion (or a re-run that lands the same fact twice) must not
    /// inflate degree or edge counts.
    #[test]
    fn a_duplicated_edge_counts_once() {
        let flakes = vec![
            edge("a", "b", &feeds()),
            Flake::assert(a("a"), feeds(), FlakeValue::Ref(a("b")), 2),
        ];

        let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

        assert_eq!(projection.adjacency.edge_count(), 1);
    }

    /// A retracted edge must not appear in the projection — it is not a
    /// live fact, and analytics over a graph that includes withdrawn
    /// history would answer a different question than "what is connected
    /// now".
    #[test]
    fn a_retracted_edge_is_not_projected() {
        let flakes = vec![Flake {
            op: false,
            ..edge("a", "b", &feeds())
        }];

        let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

        assert_eq!(projection.nodes.len(), 0, "{:?}", projection.nodes);
    }

    #[test]
    fn out_neighbours_reflects_only_outgoing_edges() {
        let flakes = vec![edge("a", "b", &feeds()), edge("b", "a", &feeds())];
        let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

        let a_id = projection.nodes.iter().position(|s| *s == a("a")).unwrap();
        let b_id = projection.nodes.iter().position(|s| *s == a("b")).unwrap();

        assert_eq!(projection.adjacency.out_neighbours(a_id), &[b_id]);
        assert_eq!(projection.adjacency.out_neighbours(b_id), &[a_id]);
    }

    /// **The negative case for `CsrGraph`'s `PartialEq`.** Every other test
    /// asserting on `.adjacency` compares two structurally identical graphs
    /// (`assert_eq!`) — never two different ones — which cannot catch `eq`
    /// mutated to always return `true`, nor its `&&` mutated to `||`: with
    /// the empty graph on the left, `(0..0).all(..)` is vacuously `true`
    /// regardless of the right side, so only `&&` correctly falls through
    /// to `node_count() == node_count()` being `false`.
    #[test]
    fn an_empty_adjacency_is_not_equal_to_a_non_empty_one() {
        let empty = project(&[], &[], &[feeds()], &budget()).expect("within budget");
        let non_empty = project(&[], &[edge("a", "b", &feeds())], &[feeds()], &budget())
            .expect("within budget");

        assert_ne!(empty.adjacency, non_empty.adjacency);
    }

    // Every comparison in this module is against an unweighted degree —
    // a whole-number sum of exact `1.0`s, never subject to accumulated
    // rounding — so exact equality is the correct assertion, not a
    // shortcut standing in for a tolerance check.
    #[allow(clippy::float_cmp)]
    mod degree_centrality_tests {
        use super::*;

        fn id_of(projection: &GraphProjection, sid: &Sid) -> NodeId {
            projection.nodes.iter().position(|s| s == sid).unwrap()
        }

        fn value_for(scores: &[Degree], node: NodeId) -> f64 {
            scores.iter().find(|d| d.node == node).unwrap().value
        }

        /// **The direction test, the plan's own named RED case.** A table
        /// with many downstream consumers and one upstream source must not
        /// score the same as its inverse — summing in and out erases the
        /// exact distinction a blast-radius reading needs.
        #[test]
        fn direction_is_not_conflated() {
            let mut flakes = vec![edge("source", "hub", &feeds())];
            for i in 0..5 {
                flakes.push(edge("hub", &format!("consumer{i}"), &feeds()));
            }
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let hub = id_of(&projection, &a("hub"));

            let in_scores = degree_centrality(&projection, Direction::In);
            let out_scores = degree_centrality(&projection, Direction::Out);

            assert_eq!(
                value_for(&in_scores, hub),
                1.0,
                "hub has one upstream source"
            );
            assert_eq!(
                value_for(&out_scores, hub),
                5.0,
                "hub has five downstream consumers"
            );
            assert_ne!(
                value_for(&in_scores, hub),
                value_for(&out_scores, hub),
                "a heavily-consumed node and a heavily-consuming one are different findings"
            );
        }

        #[test]
        fn total_sums_in_and_out() {
            let flakes = vec![
                edge("source", "hub", &feeds()),
                edge("hub", "sink", &feeds()),
            ];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let hub = id_of(&projection, &a("hub"));

            let total = degree_centrality(&projection, Direction::Total);

            assert_eq!(value_for(&total, hub), 2.0);
        }

        /// A node with no matching edges scores 0, and is *present* in the
        /// result — not silently dropped, which would make "scored zero"
        /// indistinguishable from "not scored at all".
        #[test]
        fn a_node_with_no_edges_scores_zero_not_absent() {
            let flakes = vec![
                edge("a", "b", &feeds()),
                edge("isolated_source", "isolated_sink", &other()),
            ];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

            let scores = degree_centrality(&projection, Direction::Total);

            assert_eq!(scores.len(), projection.nodes.len());
            assert!(scores.iter().all(|d| d.value >= 0.0));
        }

        /// The weighted variant sums each edge's own weight instead of
        /// counting 1 per edge.
        #[test]
        fn the_weighted_variant_sums_edge_weights_not_edge_counts() {
            let flakes = vec![edge("a", "hub", &feeds()), edge("b", "hub", &feeds())];
            let mut projection =
                project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            // Aligned to `col_indices`' own order: both edges point at
            // `hub`, weighted 0.9 and 0.1.
            projection.weights = Some(vec![0.9, 0.1]);
            let hub = id_of(&projection, &a("hub"));

            let scores = degree_centrality(&projection, Direction::In);

            // Tolerance sized for `f32` precision (~1e-7), not `f64::EPSILON`
            // — the weights are stored as `f32` and widened to `f64` for the
            // sum, so the accumulated rounding is `f32`-sized, not `f64`-sized.
            assert!((value_for(&scores, hub) - 1.0).abs() < 1e-6, "{scores:?}");
        }

        /// Two runs over the same projection produce identical results in
        /// identical order — nothing here depends on hash-map iteration.
        #[test]
        fn results_are_returned_in_deterministic_node_order() {
            let flakes = vec![
                edge("zebra", "hub", &feeds()),
                edge("apple", "hub", &feeds()),
                edge("mango", "hub", &feeds()),
            ];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

            let first = degree_centrality(&projection, Direction::In);
            let second = degree_centrality(&projection, Direction::In);

            assert_eq!(first, second);
            assert_eq!(
                first.iter().map(|d| d.node).collect::<Vec<_>>(),
                (0..projection.nodes.len()).collect::<Vec<_>>()
            );
        }
    }

    mod connected_components_tests {
        use super::*;

        fn id_of(projection: &GraphProjection, sid: &Sid) -> NodeId {
            projection.nodes.iter().position(|s| s == sid).unwrap()
        }

        #[test]
        fn an_empty_graph_yields_no_components() {
            let projection = project(&[], &[], &[feeds()], &budget()).expect("within budget");
            let components = connected_components(&projection);
            assert!(components.sizes.is_empty(), "{:?}", components.sizes);
        }

        #[test]
        fn a_fully_connected_graph_yields_one_component() {
            let flakes = vec![edge("a", "b", &feeds()), edge("b", "c", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

            let components = connected_components(&projection);

            assert_eq!(components.sizes.len(), 1, "{:?}", components.sizes);
            assert_eq!(*components.sizes.values().next().unwrap(), 3);
        }

        /// **Weakly connected**: an edge's direction does not matter for
        /// connectivity, only for degree. `a -> b` and `c -> b` put `a`
        /// and `c` in the same component even though neither points at the
        /// other directly.
        #[test]
        fn connectivity_ignores_edge_direction() {
            let flakes = vec![edge("a", "hub", &feeds()), edge("c", "hub", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let a_id = id_of(&projection, &a("a"));
            let c_id = id_of(&projection, &a("c"));

            let components = connected_components(&projection);

            assert_eq!(components.component_of[a_id], components.component_of[c_id]);
        }

        /// Size-1 components are the orphan set — the reason `Components`
        /// exposes `orphans()` rather than making a caller re-derive it
        /// from `sizes`.
        #[test]
        fn size_one_components_are_the_orphan_set() {
            // `isolated` is fed in through `all_nodes`, not through any
            // edge — it has no `feeds` edge at all, which is exactly the
            // case a real orphan is: an asset with no matching edge, not
            // merely a lightly-connected one.
            let flakes = vec![edge("a", "b", &feeds())];
            let projection =
                project(&[a("isolated")], &flakes, &[feeds()], &budget()).expect("within budget");
            let isolated_id = id_of(&projection, &a("isolated"));

            let components = connected_components(&projection);
            let orphans = components.orphans();

            assert_eq!(orphans, vec![isolated_id], "{orphans:?}");
        }

        /// **The filter-visibility test, the plan's own named RED case.**
        /// An entity connected only by `hasOwner` is an orphan under a
        /// lineage-only projection — but the result must *say* that is
        /// what it checked, not report "orphan" as though it were a
        /// filter-independent fact.
        #[test]
        fn the_result_names_the_filter_it_was_computed_under() {
            let owner_edge = Sid::new(namespace::DSC, "hasOwner");
            let flakes = vec![edge("table", "person", &owner_edge)];
            let projection = project(&[], &flakes, &[feeds()], &budget())
                .expect("within budget — hasOwner excluded");

            let components = connected_components(&projection);

            assert_eq!(components.filter_edge_types, vec![feeds()]);
            // Under a lineage-only (`feeds`) projection, `table` and
            // `person` never appear at all — the hasOwner edge was
            // filtered out at the projection step, before components ever
            // ran.
            assert!(projection.nodes.is_empty(), "{:?}", projection.nodes);
        }

        /// Components are stable across runs over the identical
        /// projection — deterministic ids, not merely a deterministic
        /// partition.
        #[test]
        fn component_ids_are_stable_across_runs() {
            let flakes = vec![edge("a", "b", &feeds()), edge("c", "d", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

            let first = connected_components(&projection);
            let second = connected_components(&projection);

            assert_eq!(first, second);
        }

        /// The specific winning id, not just "some consistent one" — union
        /// picks the *smaller* of the two roots, and a test that only
        /// checks two runs agree with each other cannot tell "smaller
        /// wins" apart from "larger wins" or "whichever was `find`-ed
        /// first", since all three are equally self-consistent.
        #[test]
        fn union_keeps_the_smaller_root_as_the_component_id() {
            // Sorted-`Sid` order numbers "a" before "z", so `a`'s NodeId
            // is smaller — the union must keep it as the representative
            // regardless of which side of the edge it appears on.
            let flakes = vec![edge("z", "a", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let a_id = id_of(&projection, &a("a"));
            let z_id = id_of(&projection, &a("z"));

            let components = connected_components(&projection);

            assert_eq!(components.component_of[a_id], a_id.min(z_id));
            assert_eq!(components.component_of[z_id], a_id.min(z_id));
        }

        /// **The complementary geometry.** `union` is only ever entered
        /// with `ra != rb`, so `ra < rb` and `ra == rb` coincide only by
        /// accident when the *first*-processed root happens to be the
        /// larger one — the test above unions `z` before `a`, so a `<`
        /// mutated to `==` (always false here, since the roots always
        /// differ) still lands on the same else-branch as the real
        /// comparison and passes undetected. Reversing which side is
        /// visited first (`a` before `z`, so `ra < rb` is genuinely `true`
        /// at the call site) is what a `==` mutant gets wrong: it takes
        /// the else-branch regardless, keeping the *larger* root.
        #[test]
        fn union_keeps_the_smaller_root_when_the_smaller_side_is_visited_first() {
            let flakes = vec![edge("a", "z", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let a_id = id_of(&projection, &a("a"));
            let z_id = id_of(&projection, &a("z"));

            let components = connected_components(&projection);

            assert_eq!(components.component_of[a_id], a_id.min(z_id));
            assert_eq!(components.component_of[z_id], a_id.min(z_id));
        }
    }

    mod pagerank_tests {
        use super::*;

        fn cfg() -> PageRankConfig {
            PageRankConfig {
                damping: 0.85,
                tolerance: 0.000_01,
                max_iterations: 100,
            }
        }

        fn score_for(scores: &[Rank], node: NodeId) -> f64 {
            scores.iter().find(|r| r.node == node).unwrap().score
        }

        #[test]
        fn an_empty_graph_converges_trivially() {
            let projection = project(&[], &[], &[feeds()], &budget()).expect("within budget");
            let result = pagerank(&projection, &cfg());
            assert!(result.converged);
            assert!(result.scores.is_empty());
        }

        /// **The reference case.** A directed cycle is symmetric under
        /// rotation, so its stationary distribution is provably uniform —
        /// derived from the algorithm's own definition, not copied from
        /// any external implementation's fixture. A three-node cycle must
        /// converge to `[1/3, 1/3, 1/3]` within tolerance.
        #[test]
        fn a_directed_cycle_converges_to_the_uniform_distribution() {
            let flakes = vec![
                edge("a", "b", &feeds()),
                edge("b", "c", &feeds()),
                edge("c", "a", &feeds()),
            ];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

            let result = pagerank(&projection, &cfg());

            assert!(result.converged, "{result:?}");
            for rank in &result.scores {
                assert!(
                    (rank.score - 1.0 / 3.0).abs() < 0.001,
                    "node {} scored {}, expected ~1/3: {:?}",
                    rank.node,
                    rank.score,
                    result.scores
                );
            }
        }

        /// **The dangling-node test, the plan's own named RED case.** A
        /// node with no outgoing edges must not leak rank mass — the sum
        /// over every score must stay 1 within floating-point tolerance
        /// even when a dangling node is present, which is invisible on a
        /// graph too small to notice the leak and skews everything on a
        /// real one.
        #[test]
        fn scores_sum_to_one_even_with_a_dangling_node() {
            let flakes = vec![edge("a", "sink", &feeds())]; // `sink` has no outgoing edges
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");

            let result = pagerank(&projection, &cfg());

            let total: f64 = result.scores.iter().map(|r| r.score).sum();
            assert!(
                (total - 1.0).abs() < 0.001,
                "rank mass leaked: sum was {total}, {:?}",
                result.scores
            );
        }

        /// `sink` (dangling) must actually receive the redistributed mass,
        /// not merely fail to break the sum-to-one invariant above — a
        /// dangling node's own score must be non-trivial, since it is
        /// receiving `a`'s full rank plus its share of every dangling
        /// redistribution.
        #[test]
        fn a_dangling_node_receives_the_incoming_rank_pointed_at_it() {
            let flakes = vec![edge("a", "sink", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let sink = projection
                .nodes
                .iter()
                .position(|s| *s == a("sink"))
                .unwrap();

            let result = pagerank(&projection, &cfg());

            assert!(score_for(&result.scores, sink) > 0.4, "{:?}", result.scores);
        }

        /// `max_iterations` reached before the delta falls under tolerance
        /// must report `converged: false` — never silently reported as
        /// converged, the mutant the plan's own mutator watch names
        /// explicitly.
        #[test]
        fn hitting_max_iterations_without_convergence_reports_it_honestly() {
            let flakes = vec![
                edge("a", "b", &feeds()),
                edge("b", "c", &feeds()),
                edge("c", "a", &feeds()),
            ];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let starved = PageRankConfig {
                damping: 0.85,
                tolerance: 0.0, // unreachable — forces exhaustion
                max_iterations: 2,
            };

            let result = pagerank(&projection, &starved);

            assert!(!result.converged, "{result:?}");
            assert_eq!(result.scores.len(), 3);
        }

        /// The weighted variant distributes a node's score in proportion
        /// to each out-edge's own weight, not uniformly.
        #[test]
        fn the_weighted_variant_favours_the_heavier_edge() {
            let flakes = vec![
                edge("hub", "strong", &feeds()),
                edge("hub", "weak", &feeds()),
            ];
            let mut projection =
                project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let strong = projection
                .nodes
                .iter()
                .position(|s| *s == a("strong"))
                .unwrap();
            let weak = projection
                .nodes
                .iter()
                .position(|s| *s == a("weak"))
                .unwrap();
            // Aligned to `col_indices`' own order (sorted by target Sid):
            // "strong" < "weak" alphabetically.
            projection.weights = Some(vec![0.9, 0.1]);

            let result = pagerank(&projection, &cfg());

            assert!(
                score_for(&result.scores, strong) > score_for(&result.scores, weak),
                "{:?}",
                result.scores
            );
        }

        /// **The normalization test.** An unweighted hub with *two*
        /// out-edges must divide its score between them, not distribute it
        /// unchanged to each (a `/` mutated to `*` or `%` at the
        /// normalization step) — every earlier fixture in this module
        /// happens to have `out_weight_total == 1.0` for the node under
        /// test, where dividing, multiplying and (for values under 1)
        /// taking the remainder by 1.0 all coincide, so none of them can
        /// tell the operators apart. One iteration only, so the expected
        /// value is the closed-form first pass rather than a converged
        /// fixed point — computed here from the algorithm's own
        /// definition, not transcribed from any external source.
        #[test]
        fn an_unweighted_hub_with_two_out_edges_splits_its_score_between_them() {
            let flakes = vec![edge("hub", "a", &feeds()), edge("hub", "b", &feeds())];
            let projection = project(&[], &flakes, &[feeds()], &budget()).expect("within budget");
            let a_id = projection.nodes.iter().position(|s| *s == a("a")).unwrap();
            let b_id = projection.nodes.iter().position(|s| *s == a("b")).unwrap();
            let one_pass = PageRankConfig {
                damping: 0.85,
                tolerance: 0.0,
                max_iterations: 1,
            };

            let result = pagerank(&projection, &one_pass);

            // By symmetry `a` and `b` must score identically.
            assert!(
                (score_for(&result.scores, a_id) - score_for(&result.scores, b_id)).abs() < 1e-9,
                "{:?}",
                result.scores
            );

            let n = 3.0_f64;
            // Widened from `f32` exactly the way `pagerank` itself widens
            // `cfg.damping` — a literal `0.85_f64` here would not equal
            // `f64::from(0.85_f32)`, since `0.85` has no exact `f32`
            // representation, and comparing against the wrong widening
            // would need a tolerance too loose to still catch the mutants
            // this test exists for.
            let damping = f64::from(one_pass.damping);
            let initial = 1.0 / n;
            let dangling_mass = initial * 2.0; // `a` and `b` are both dangling
            let base = (1.0 - damping) / n + damping * dangling_mass / n;
            // hub's own score (`initial`), split across its two out-edges —
            // the `/ 2.0` this test exists to pin.
            let expected_a = base + damping * initial * 1.0 / 2.0;

            assert!(
                (score_for(&result.scores, a_id) - expected_a).abs() < 1e-9,
                "expected {expected_a}, got {:?}",
                result.scores
            );
        }
    }
    /// **Cycles — the one structural finding a per-edge rule cannot make.**
    ///
    /// A weakly-connected component cannot answer this: it ignores direction,
    /// so `a -> b -> c -> a` and `a -> b -> c` are both one component. The
    /// distinction is the whole finding, because a cycle among trading parties
    /// means value returning to where it started and a chain means ordinary
    /// supply.
    mod cycle_detection {
        use super::*;

        fn ring() -> Vec<Flake> {
            vec![
                edge("a", "b", &feeds()),
                edge("b", "c", &feeds()),
                edge("c", "a", &feeds()),
            ]
        }

        #[test]
        fn a_three_node_ring_is_reported_as_one_cycle() {
            let projection = project(&[], &ring(), &[feeds()], &budget()).expect("within budget");

            let found = cycles(&projection);

            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].len(), 3, "{found:?}");
        }

        #[test]
        fn breaking_the_ring_with_one_edge_stops_it_being_a_cycle() {
            // The plan's own mutator, as a test. A weakly-connected component
            // would survive this — all three nodes stay in one component — so
            // a test that only asserted "one component" would prove nothing.
            let mut broken = ring();
            broken.pop();

            let found = cycles(&project(&[], &broken, &[feeds()], &budget()).expect("ok"));

            assert!(found.is_empty(), "{found:?}");
        }

        #[test]
        fn a_chain_of_any_length_is_never_a_cycle() {
            let chain = vec![
                edge("a", "b", &feeds()),
                edge("b", "c", &feeds()),
                edge("c", "d", &feeds()),
                edge("d", "e", &feeds()),
            ];

            assert!(cycles(&project(&[], &chain, &[feeds()], &budget()).expect("ok")).is_empty());
        }

        #[test]
        fn a_two_node_cycle_counts() {
            // Reciprocal trading between exactly two parties is the smallest
            // real instance of the pattern, and excluding it would miss the
            // most common one.
            let pair = vec![edge("a", "b", &feeds()), edge("b", "a", &feeds())];

            let found = cycles(&project(&[], &pair, &[feeds()], &budget()).expect("ok"));

            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].len(), 2);
        }

        #[test]
        fn a_self_loop_is_not_reported_as_a_cycle() {
            // A node pointing at itself is a data-quality problem, not a ring
            // of trading parties, and reporting it as one would fill the
            // finding with noise nobody can act on.
            let loops = vec![edge("a", "a", &feeds())];

            assert!(cycles(&project(&[], &loops, &[feeds()], &budget()).expect("ok")).is_empty());
        }

        #[test]
        fn two_separate_rings_are_two_findings_not_one() {
            let mut two = ring();
            two.extend([
                edge("x", "y", &feeds()),
                edge("y", "z", &feeds()),
                edge("z", "x", &feeds()),
            ]);

            let found = cycles(&project(&[], &two, &[feeds()], &budget()).expect("ok"));

            assert_eq!(found.len(), 2, "{found:?}");
        }

        #[test]
        fn a_ring_with_an_unconnected_node_beside_it_reports_only_the_ring() {
            // The plan's own scenario: an unconnected party of larger value
            // must not dilute or join the finding.
            let mut with_outsider = ring();
            with_outsider.push(edge("lonely", "nowhere", &feeds()));

            let found = cycles(&project(&[], &with_outsider, &[feeds()], &budget()).expect("ok"));

            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].len(), 3);
        }

        #[test]
        fn a_ring_reachable_only_through_a_chain_is_still_found() {
            // `entry -> a -> b -> c -> a`. The chain is not part of the cycle
            // and must not be reported as though it were — a finding that
            // swept in every upstream party would be unactionable.
            let mut tailed = ring();
            tailed.push(edge("entry", "a", &feeds()));

            let found = cycles(&project(&[], &tailed, &[feeds()], &budget()).expect("ok"));

            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].len(), 3, "{found:?}");
        }
    }
}
