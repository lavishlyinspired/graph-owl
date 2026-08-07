//! In-process traversal — Epic 103, a second [`TraversalEngine`] implementation.
//!
//! One statement (`TripleStore::query_pattern`) fetches every current flake,
//! then the whole walk runs in memory over a graph built from it. Nothing
//! above the port changes: the explorer, the API and any future consumer keep
//! calling [`TraversalEngine`] exactly as they call the Postgres CTE adapter
//! — see `plans/103-in-process-traversal.md` for why this is a second
//! implementation of one trait rather than a rewrite, and for the honest
//! entry-condition measurement this epic's own plan records.
//!
//! **`DiGraph`, not `StableDiGraph`** (decision 2): the graph is built fresh
//! per call, from an already-filtered, already-authorized fact set, and
//! discarded. Holding one across calls would mean a single graph shared by
//! every caller at a single instant, which breaks `as_of` (whose instant?)
//! and authorization (whose view?) at once.

use async_trait::async_trait;
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use graph_owl_engine::TripleStore;
use graph_owl_traversal::{
    Bounds, Cycle, Direction, EdgeFilter, EdgeRef, Path, PathSet, Reached, Subgraph,
    TraversalEngine, TraversalError, TraversalResult, TruncationReason,
};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef as _;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

fn from_entity() -> Sid {
    Sid::dsc("fromEntity")
}
fn to_entity() -> Sid {
    Sid::dsc("toEntity")
}
fn rel_type_predicate() -> Sid {
    Sid::dsc("relType")
}
fn reasoning_graph() -> Sid {
    Sid::dsc("graph:reasoning")
}

fn obj_ref(flake: &Flake) -> Option<&Sid> {
    match &flake.o {
        FlakeValue::Ref(sid) => Some(sid),
        _ => None,
    }
}

/// One logical edge: a reified relationship (`entity -fromEntity-> rel
/// -toEntity-> entity`, collapsed) or a direct DSC-namespace reference.
/// Matches `graph-owl-engine-postgres::traversal`'s `edges` CTE exactly —
/// same two sources, same exclusions — since the whole point of a second
/// adapter is that it answers identically, not merely similarly.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LogicalEdge {
    from: Sid,
    to: Sid,
    rel_type: String,
    derived: bool,
}

/// Every logical edge in `flakes` — the in-memory equivalent of
/// `push_logical_edges`'s SQL. Deliberately reads `flakes` in one pass per
/// source rather than pre-indexing into a generic multimap: there are
/// exactly two sources, and a generic structure would hide that rather than
/// express it.
fn logical_edges(flakes: &[Flake]) -> Vec<LogicalEdge> {
    let from_p = from_entity();
    let to_p = to_entity();
    let relt_p = rel_type_predicate();
    let reasoning = reasoning_graph();

    let mut to_target_by_subject: HashMap<&Sid, &Sid> = HashMap::new();
    let mut rel_type_by_subject: HashMap<&Sid, &str> = HashMap::new();
    for flake in flakes {
        if flake.p == to_p {
            if let Some(target) = obj_ref(flake) {
                to_target_by_subject.insert(&flake.s, target);
            }
        } else if flake.p == relt_p
            && let FlakeValue::String(name) = &flake.o
        {
            rel_type_by_subject.insert(&flake.s, name.as_str());
        }
    }

    let mut edges = Vec::new();

    // Source 1: reified relationships.
    for flake in flakes {
        if flake.p != from_p {
            continue;
        }
        let Some(from_target) = obj_ref(flake) else {
            continue;
        };
        let Some(&to_target) = to_target_by_subject.get(&flake.s) else {
            continue;
        };
        let rel_type = rel_type_by_subject
            .get(&flake.s)
            .map_or_else(|| "related".to_string(), |name| (*name).to_string());
        edges.push(LogicalEdge {
            from: from_target.clone(),
            to: to_target.clone(),
            rel_type,
            derived: flake.cx.as_ref() == Some(&reasoning),
        });
    }

    // Source 2: direct DSC-namespace references, excluding fromEntity/toEntity
    // themselves (a relType flake's own value is a string, not a ref, so it is
    // already excluded by `obj_ref` without needing a name check).
    for flake in flakes {
        if flake.p == from_p || flake.p == to_p {
            continue;
        }
        if flake.p.namespace_code != namespace::DSC {
            continue;
        }
        let Some(target) = obj_ref(flake) else {
            continue;
        };
        edges.push(LogicalEdge {
            from: flake.s.clone(),
            to: target.clone(),
            rel_type: flake.p.id.clone(),
            derived: flake.cx.as_ref() == Some(&reasoning),
        });
    }

    edges
}

/// A directed graph over [`LogicalEdge`]s, with `Sid <-> NodeIndex` lookups.
/// Built once per call and discarded — see the module doc for why holding
/// one across calls is refused rather than merely undesirable.
struct BuiltGraph {
    graph: DiGraph<Sid, LogicalEdgeData>,
    index_of: HashMap<Sid, NodeIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogicalEdgeData {
    rel_type: String,
    derived: bool,
}

impl BuiltGraph {
    /// `directed` normalises the walk so callers never branch on
    /// `Direction` themselves: `Outgoing` keeps each edge as stored,
    /// `Incoming` reverses it, `Both` adds both orientations of every edge
    /// once each — matching `push_frontier`'s own `directed` CTE exactly,
    /// including its `Both` semantics (each edge contributes one
    /// orientation per direction actually requested, never the same
    /// orientation twice).
    fn from_edges(edges: &[LogicalEdge], seeds: &[Sid], direction: Direction) -> Self {
        let mut graph = DiGraph::new();
        let mut index_of: HashMap<Sid, NodeIndex> = HashMap::new();

        let node_index = |graph: &mut DiGraph<Sid, LogicalEdgeData>,
                          index_of: &mut HashMap<Sid, NodeIndex>,
                          sid: &Sid|
         -> NodeIndex {
            if let Some(&idx) = index_of.get(sid) {
                idx
            } else {
                let idx = graph.add_node(sid.clone());
                index_of.insert(sid.clone(), idx);
                idx
            }
        };

        for seed in seeds {
            node_index(&mut graph, &mut index_of, seed);
        }

        for edge in edges {
            let data = LogicalEdgeData {
                rel_type: edge.rel_type.clone(),
                derived: edge.derived,
            };
            if direction.follows_outgoing() {
                let from = node_index(&mut graph, &mut index_of, &edge.from);
                let to = node_index(&mut graph, &mut index_of, &edge.to);
                graph.add_edge(from, to, data.clone());
            }
            if direction.follows_incoming() {
                let from = node_index(&mut graph, &mut index_of, &edge.to);
                let to = node_index(&mut graph, &mut index_of, &edge.from);
                graph.add_edge(from, to, data);
            }
        }

        Self { graph, index_of }
    }

    fn index(&self, sid: &Sid) -> Option<NodeIndex> {
        self.index_of.get(sid).copied()
    }
}

/// A second [`TraversalEngine`] implementation, in-process over petgraph.
pub struct InMemoryTraversalEngine {
    store: Arc<dyn TripleStore>,
}

impl InMemoryTraversalEngine {
    #[must_use]
    pub fn new(store: Arc<dyn TripleStore>) -> Self {
        Self { store }
    }

    /// Fetches every current flake at `as_of`, then narrows to
    /// [`LogicalEdge`]s matching `filter.relationship_types`. One fetch,
    /// like the SQL's own `live` CTE, which scans the whole table rather
    /// than pushing predicate filters down into storage — the direct-
    /// reference edge source has no fixed predicate to filter on, so
    /// neither implementation can narrow the read.
    async fn fetch_edges(&self, filter: &EdgeFilter) -> Result<Vec<LogicalEdge>, TraversalError> {
        let pattern = TriplePattern {
            as_of: filter.as_of,
            ..Default::default()
        };
        let flakes = self
            .store
            .query_pattern(&pattern)
            .await
            .map_err(|e| TraversalError::Backend(e.to_string()))?;

        let mut edges = logical_edges(&flakes);
        if let Some(types) = &filter.relationship_types {
            edges.retain(|e| types.contains(&e.rel_type));
        }
        Ok(edges)
    }
}

/// Breadth-first, one node visited once at its true shortest distance.
///
/// **Equivalent to the SQL's per-path cycle guard plus `DISTINCT ON
/// (node_id) ORDER BY depth`, not merely similar to it.** The SQL walk
/// explores every path independently (guarded so a single path never
/// revisits its own nodes) and keeps only the shortest depth per node
/// afterward; a node reached again by a longer path carries no information
/// a shorter one has not already supplied, because BFS already explored
/// every neighbour of that node the first time it was dequeued. A global
/// visited set is the standard BFS shortest-path argument, not a shortcut —
/// see `plans/103-in-process-traversal.md`'s own "Resolved decisions" for
/// why `DiGraph` over a filtered, discarded fact set is the intended shape.
fn bfs(built: &BuiltGraph, start: NodeIndex, max_hops: usize) -> Vec<(NodeIndex, usize)> {
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut order: Vec<(NodeIndex, usize)> = Vec::new();
    let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();

    visited.insert(start);
    queue.push_back((start, 0));

    while let Some((node, depth)) = queue.pop_front() {
        order.push((node, depth));
        if depth >= max_hops {
            continue;
        }
        for neighbour in built.graph.neighbors(node) {
            if visited.insert(neighbour) {
                queue.push_back((neighbour, depth + 1));
            }
        }
    }

    order
}

#[async_trait]
impl TraversalEngine for InMemoryTraversalEngine {
    async fn neighbours(
        &self,
        start: &Sid,
        direction: Direction,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<TraversalResult, TraversalError> {
        let edges = self.fetch_edges(filter).await?;
        let built = BuiltGraph::from_edges(&edges, std::slice::from_ref(start), direction);
        let Some(start_idx) = built.index(start) else {
            return Ok(TraversalResult {
                reached: Vec::new(),
                truncated: false,
                truncation_reason: None,
            });
        };

        let order = bfs(&built, start_idx, bounds.max_hops);
        let over_budget = order.len() > bounds.max_nodes;
        let mut reached: Vec<Reached> = order
            .into_iter()
            .take(bounds.max_nodes)
            .map(|(idx, depth)| Reached {
                node: built.graph[idx].clone(),
                distance: depth,
            })
            .collect();
        reached.sort_by_key(|r| r.distance);

        Ok(TraversalResult {
            reached,
            truncated: over_budget,
            truncation_reason: over_budget.then_some(TruncationReason::NodeBudget),
        })
    }

    async fn subgraph(
        &self,
        seeds: &[Sid],
        direction: Direction,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<Subgraph, TraversalError> {
        if seeds.is_empty() {
            return Ok(Subgraph::default());
        }
        let edges = self.fetch_edges(filter).await?;
        let built = BuiltGraph::from_edges(&edges, seeds, direction);

        // Multi-source BFS: every seed starts at depth 0, and a node's
        // distance is the shortest hop count from *any* seed — matching the
        // SQL's own `UNNEST(seeds)` base case, one frontier row per seed.
        // `visited` is a membership set, not a depth lookup: the depth every
        // caller actually needs travels through `queue`/`order` instead, so
        // there is nothing here for a second `usize` to record.
        let mut visited: HashSet<NodeIndex> = HashSet::new();
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        for seed in seeds {
            if let Some(idx) = built.index(seed)
                && visited.insert(idx)
            {
                queue.push_back((idx, 0));
            }
        }
        let mut order: Vec<(NodeIndex, usize)> = Vec::new();
        while let Some((node, depth)) = queue.pop_front() {
            order.push((node, depth));
            if depth >= bounds.max_hops {
                continue;
            }
            for neighbour in built.graph.neighbors(node) {
                if visited.insert(neighbour) {
                    queue.push_back((neighbour, depth + 1));
                }
            }
        }

        // Farthest-first truncation: keep the smallest depths, matching the
        // Postgres adapter's own "dropping the outer ring keeps a complete
        // view of the neighbourhood" reasoning.
        let over_budget = order.len() > bounds.max_nodes;
        let mut kept = order;
        if over_budget {
            kept.sort_by_key(|(_, depth)| *depth);
            kept.truncate(bounds.max_nodes);
        }
        let kept_indices: HashSet<NodeIndex> = kept.iter().map(|(idx, _)| *idx).collect();

        let nodes: Vec<Sid> = kept
            .iter()
            .map(|(idx, _)| built.graph[*idx].clone())
            .collect();
        let mut edge_refs: Vec<EdgeRef> = Vec::new();
        for &(idx, _) in &kept {
            for edge in built.graph.edges(idx) {
                if !kept_indices.contains(&edge.target()) {
                    continue;
                }
                let data = edge.weight();
                let edge_ref = EdgeRef {
                    from: built.graph[idx].clone(),
                    to: built.graph[edge.target()].clone(),
                    relationship: data.rel_type.clone(),
                    derived: data.derived,
                };
                if !edge_refs.contains(&edge_ref) {
                    edge_refs.push(edge_ref);
                }
            }
        }

        Ok(Subgraph {
            nodes,
            edges: edge_refs,
            truncated: over_budget,
            truncation_reason: over_budget.then_some(TruncationReason::NodeBudget),
        }
        .without_dangling_edges())
    }

    async fn shortest_path(
        &self,
        from: &Sid,
        to: &Sid,
        direction: Direction,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<Option<Path>, TraversalError> {
        if from == to {
            return Ok(Some(Path {
                nodes: vec![from.clone()],
                length: 0,
            }));
        }

        let edges = self.fetch_edges(filter).await?;
        let built = BuiltGraph::from_edges(&edges, std::slice::from_ref(from), direction);
        let (Some(from_idx), Some(to_idx)) = (built.index(from), built.index(to)) else {
            return Ok(None);
        };

        // BFS with predecessor tracking, tie-broken lexicographically by
        // Sid at each step — matching `ORDER BY depth, path LIMIT 1`.
        // Neighbours are visited in `Sid`-sorted order so the *first*
        // shortest path BFS discovers is already the lexicographically
        // smallest one among equal-length routes.
        let mut visited: HashMap<NodeIndex, (usize, Option<NodeIndex>)> = HashMap::new();
        let mut queue: VecDeque<NodeIndex> = VecDeque::new();
        visited.insert(from_idx, (0, None));
        queue.push_back(from_idx);

        while let Some(node) = queue.pop_front() {
            let (depth, _) = visited[&node];
            if depth >= bounds.max_hops {
                continue;
            }
            let mut neighbours: Vec<NodeIndex> = built.graph.neighbors(node).collect();
            neighbours.sort_by(|a, b| built.graph[*a].cmp(&built.graph[*b]));
            for neighbour in neighbours {
                if let std::collections::hash_map::Entry::Vacant(entry) = visited.entry(neighbour) {
                    entry.insert((depth + 1, Some(node)));
                    queue.push_back(neighbour);
                }
            }
        }

        let Some(&(depth, _)) = visited.get(&to_idx) else {
            return Ok(None);
        };
        let mut nodes_idx = vec![to_idx];
        let mut current = to_idx;
        while let Some((_, Some(pred))) = visited.get(&current) {
            nodes_idx.push(*pred);
            current = *pred;
        }
        nodes_idx.reverse();

        Ok(Some(Path {
            nodes: nodes_idx
                .into_iter()
                .map(|idx| built.graph[idx].clone())
                .collect(),
            length: depth,
        }))
    }

    async fn all_paths(
        &self,
        from: &Sid,
        to: &Sid,
        direction: Direction,
        bounds: Bounds,
        max_paths: usize,
        filter: &EdgeFilter,
    ) -> Result<PathSet, TraversalError> {
        let edges = self.fetch_edges(filter).await?;
        let built = BuiltGraph::from_edges(&edges, std::slice::from_ref(from), direction);
        let (Some(from_idx), Some(to_idx)) = (built.index(from), built.index(to)) else {
            return Ok(PathSet::default());
        };

        // Exhaustive DFS, depth-bounded and path-guarded (a path may not
        // revisit its own nodes) — matching the SQL frontier's own
        // `path || d.dst` accumulation and `NOT d.dst = ANY(f.path)` guard
        // exactly, since `all_paths` (unlike `neighbours`/`subgraph`) needs
        // every distinct route, not only the shortest one.
        //
        // Deliberately NOT capped by `max_paths` during the walk itself: a
        // recursive CTE with an outer `ORDER BY ... LIMIT` still evaluates
        // its recursive term to completion before the limit applies, so the
        // SQL adapter finds every route within `max_hops` before sorting and
        // truncating. Stopping this DFS early once `max_paths` routes are
        // found would keep whichever routes the traversal order happened to
        // reach first — not the ones that sort smallest — and the two
        // adapters would disagree on *which* paths survive truncation.
        let mut found: Vec<(usize, Vec<NodeIndex>)> = Vec::new();
        let mut path = vec![from_idx];
        let mut on_path: HashSet<NodeIndex> = HashSet::from([from_idx]);
        walk_all_paths(
            &built,
            from_idx,
            to_idx,
            bounds.max_hops,
            &mut path,
            &mut on_path,
            &mut found,
        );

        found.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| {
                let a_sids: Vec<&Sid> = a.1.iter().map(|i| &built.graph[*i]).collect();
                let b_sids: Vec<&Sid> = b.1.iter().map(|i| &built.graph[*i]).collect();
                a_sids.cmp(&b_sids)
            })
        });

        let over_cap = found.len() > max_paths;
        let paths = found
            .into_iter()
            .take(max_paths)
            .map(|(length, idx_path)| Path {
                length,
                nodes: idx_path
                    .into_iter()
                    .map(|idx| built.graph[idx].clone())
                    .collect(),
            })
            .collect();

        Ok(PathSet {
            paths,
            truncated: over_cap,
            truncation_reason: over_cap.then_some(TruncationReason::NodeBudget),
        })
    }

    async fn detect_cycles(
        &self,
        start: &Sid,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<Vec<Cycle>, TraversalError> {
        // Outgoing only, not a caller choice — `Both` would make every edge
        // bidirectional, reporting every connected pair as a two-node
        // cycle. Matches the Postgres adapter's own forced `Direction::Outgoing`.
        let edges = self.fetch_edges(filter).await?;
        let built =
            BuiltGraph::from_edges(&edges, std::slice::from_ref(start), Direction::Outgoing);
        let Some(start_idx) = built.index(start) else {
            return Ok(Vec::new());
        };

        // Same path-guarded walk `all_paths` uses, but every step also
        // checks whether an edge closes back onto the *current* path —
        // the closing edge the guard itself refused to follow. Matches the
        // Postgres adapter's own "find frontier nodes with an edge back to
        // somewhere already on their own path" reasoning exactly.
        let mut cycles: Vec<Cycle> = Vec::new();
        let mut path = vec![start_idx];
        let mut on_path: HashSet<NodeIndex> = HashSet::from([start_idx]);
        find_cycles(
            &built,
            bounds.max_hops,
            &mut path,
            &mut on_path,
            &mut cycles,
        );

        Ok(cycles)
    }
}

fn walk_all_paths(
    built: &BuiltGraph,
    current: NodeIndex,
    target: NodeIndex,
    max_hops: usize,
    path: &mut Vec<NodeIndex>,
    on_path: &mut HashSet<NodeIndex>,
    found: &mut Vec<(usize, Vec<NodeIndex>)>,
) {
    // No `path.len() > 1` guard: `current` can only equal `target` at
    // `path.len() == 1` when `from == to` (the `on_path` guard below stops
    // any longer route from closing back onto the start), and the SQL
    // adapter's own frontier includes that depth-0 base-case row too — a
    // node reaches itself in zero hops there as much as here.
    if current == target {
        found.push((path.len() - 1, path.clone()));
        return;
    }
    if path.len() > max_hops {
        return;
    }
    for edge in built.graph.edges(current) {
        let next = edge.target();
        if on_path.contains(&next) {
            continue;
        }
        path.push(next);
        on_path.insert(next);
        walk_all_paths(built, next, target, max_hops, path, on_path, found);
        on_path.remove(&next);
        path.pop();
    }
}

fn find_cycles(
    built: &BuiltGraph,
    max_hops: usize,
    path: &mut Vec<NodeIndex>,
    on_path: &mut HashSet<NodeIndex>,
    cycles: &mut Vec<Cycle>,
) {
    let current = *path.last().expect("path is never empty");
    if path.len() > max_hops {
        return;
    }
    for edge in built.graph.edges(current) {
        let next = edge.target();
        if on_path.contains(&next) {
            // Closing edge: the loop is the path's own tail from where it
            // closes onward, not the whole walk that led into it.
            let entry = path
                .iter()
                .position(|idx| *idx == next)
                .expect("on_path implies present");
            let loop_nodes: Vec<Sid> = path[entry..]
                .iter()
                .map(|idx| built.graph[*idx].clone())
                .collect();
            let cycle = Cycle::normalised(loop_nodes);
            if !cycles.contains(&cycle) {
                cycles.push(cycle);
            }
            continue;
        }
        path.push(next);
        on_path.insert(next);
        find_cycles(built, max_hops, path, on_path, cycles);
        on_path.remove(&next);
        path.pop();
    }
}
