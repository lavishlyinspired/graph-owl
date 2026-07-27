//! The frontier walk, as one recursive CTE.
//!
//! One statement per traversal rather than N round trips. That is the whole
//! reason this is a backend-shaped method instead of a portable loop over
//! `query_pattern`: a 3-hop walk over a wide schema is thousands of edges, and
//! a round trip per frontier expansion makes the network the bottleneck.

use async_trait::async_trait;
use graph_owl_core::flake::{Sid, namespace};
use graph_owl_traversal::{
    Bounds, Cycle, Direction, EdgeFilter, EdgeRef, Path, PathSet, Reached, Subgraph,
    TraversalEngine, TraversalError, TraversalResult, TruncationReason,
};
use sqlx::{QueryBuilder, Row};

use crate::PostgresTripleStore;

/// The predicates that reify a relationship. An edge is `entity → relationship
/// → entity`, so these two are what collapse the two stored hops into one
/// logical edge — and they are excluded from the direct-reference edges below,
/// or every relationship node would also appear as two edges of its own.
const FROM_ENTITY: &str = "fromEntity";
const TO_ENTITY: &str = "toEntity";

/// SQL producing the current, live flakes at an optional `as_of`.
///
/// Identical resolution to `query_pattern`'s: newest per fact, `op` filtered
/// *outside* the `DISTINCT ON` so a retraction supersedes rather than being
/// skipped over. Duplicated here as SQL text rather than shared with the
/// pattern builder because this one has to be embeddable inside a `WITH
/// RECURSIVE`, which the builder's shape does not allow.
fn push_live_flakes(builder: &mut QueryBuilder<'_, sqlx::Postgres>, as_of: Option<i64>) {
    builder.push(
        "live AS (
            SELECT * FROM (
                SELECT DISTINCT ON (namespace_s, sid_s, namespace_p, sid_p, value_type, value_key,
                                    cx_namespace, cx_id)
                       namespace_s, sid_s, namespace_p, sid_p, value_type, value_ref_ns,
                       value_ref_id, value_str, op
                FROM flakes
                WHERE (",
    );
    builder.push_bind(as_of);
    builder.push("::bigint IS NULL OR t <= ");
    builder.push_bind(as_of);
    builder.push(
        ")
                ORDER BY namespace_s, sid_s, namespace_p, sid_p, value_type, value_key,
                         cx_namespace, cx_id, t DESC
            ) newest WHERE op
        )",
    );
}

/// SQL producing one row per **logical** edge.
///
/// Two sources, unioned:
///
/// 1. **Reified relationships** — joined across `fromEntity`/`toEntity` so the
///    two stored hops become one edge. Reporting them as two would double every
///    distance the product shows.
/// 2. **Direct references** — `dsc:parentTable`, `dsc:parentSchema`, `dsc:owner`
///    and so on. The hierarchy *is* graph structure, and a catalogue that has
///    not yet had lineage declared would otherwise traverse to nothing at all.
fn push_logical_edges(builder: &mut QueryBuilder<'_, sqlx::Postgres>, filter: &EdgeFilter) {
    builder.push(
        ", edges AS (
            SELECT f.value_ref_id AS from_id, t.value_ref_id AS to_id,
                   COALESCE(r.value_str, 'related') AS rel_type
            FROM live f
            JOIN live t ON t.sid_s = f.sid_s AND t.sid_p = '",
    );
    builder.push(TO_ENTITY);
    builder.push(
        "' LEFT JOIN live r ON r.sid_s = f.sid_s AND r.sid_p = 'relType'
            WHERE f.sid_p = '",
    );
    builder.push(FROM_ENTITY);
    builder.push(
        "'
          UNION ALL
            SELECT d.sid_s AS from_id, d.value_ref_id AS to_id, d.sid_p AS rel_type
            FROM live d
            WHERE d.value_type = 0
              AND d.sid_p NOT IN ('",
    );
    builder.push(FROM_ENTITY);
    builder.push("', '");
    builder.push(TO_ENTITY);
    builder.push(
        "')
              AND d.namespace_p = ",
    );
    builder.push_bind(i32::from(namespace::DSC));
    builder.push(")");

    if let Some(types) = &filter.relationship_types {
        // Applied to the edge set, before the walk — filtering after the walk
        // would return paths that travelled through edges the caller excluded.
        builder.push(", filtered AS (SELECT * FROM edges WHERE rel_type = ANY(");
        builder.push_bind(types.clone());
        builder.push("))");
    } else {
        builder.push(", filtered AS (SELECT * FROM edges)");
    }
}

/// The recursive frontier itself.
fn push_frontier(
    builder: &mut QueryBuilder<'_, sqlx::Postgres>,
    seeds: &[String],
    direction: Direction,
    max_hops: usize,
) {
    // `directed` normalises the walk so the recursive term never has to branch
    // on direction: `Both` unions each edge in each orientation once, which is
    // also what stops a bidirectional walk reporting every neighbour twice.
    builder.push(", directed AS (");
    let mut first = true;
    if direction.follows_outgoing() {
        builder.push("SELECT from_id AS src, to_id AS dst, rel_type FROM filtered");
        first = false;
    }
    if direction.follows_incoming() {
        if !first {
            builder.push(" UNION ");
        }
        builder.push("SELECT to_id AS src, from_id AS dst, rel_type FROM filtered");
    }
    builder.push(")");

    builder.push(
        ", frontier(node_id, depth, path) AS (
            SELECT s.id, 0, ARRAY[s.id] FROM UNNEST(",
    );
    builder.push_bind(seeds.to_vec());
    builder.push(
        "::text[]) AS s(id)
          UNION ALL
            SELECT d.dst, f.depth + 1, f.path || d.dst
            FROM frontier f
            JOIN directed d ON d.src = f.node_id
            WHERE f.depth < ",
    );
    // `<` with depth starting at 0: a max_hops of 2 admits depths 1 and 2 and
    // stops. `<=` would walk one hop further than asked.
    builder.push_bind(i64::try_from(max_hops).unwrap_or(i64::MAX));
    // Per-path cycle guard. A metadata graph really does contain cycles, and
    // without this the recursion does not terminate.
    builder.push(" AND NOT d.dst = ANY(f.path))");
}

#[async_trait]
impl TraversalEngine for PostgresTripleStore {
    async fn neighbours(
        &self,
        start: &Sid,
        direction: Direction,
        bounds: Bounds,
        filter: &EdgeFilter,
    ) -> Result<TraversalResult, TraversalError> {
        let seeds = vec![start.id.clone()];
        let mut builder = QueryBuilder::new("WITH RECURSIVE ");
        push_live_flakes(&mut builder, filter.as_of);
        push_logical_edges(&mut builder, filter);
        push_frontier(&mut builder, &seeds, direction, bounds.max_hops);
        // DISTINCT ON gives each node once at its shortest distance — a diamond
        // must report its far node once, not once per route to it.
        builder.push(
            " SELECT DISTINCT ON (node_id) node_id, depth FROM frontier
             ORDER BY node_id, depth LIMIT ",
        );
        // One over the budget, so "we hit the cap" is distinguishable from
        // "the answer happens to be exactly the cap".
        builder.push_bind(i64::try_from(bounds.max_nodes.saturating_add(1)).unwrap_or(i64::MAX));

        let rows = builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| TraversalError::Backend(e.to_string()))?;

        let over_budget = rows.len() > bounds.max_nodes;
        let mut reached: Vec<Reached> = rows
            .iter()
            .take(bounds.max_nodes)
            .map(|row| Reached {
                node: Sid::new(namespace::DSC, row.get::<String, _>("node_id")),
                distance: usize::try_from(row.get::<i32, _>("depth")).unwrap_or(0),
            })
            .collect();
        // Nearest first: a truncated list should keep what is most relevant,
        // and the caller reads it top-down.
        reached.sort_by_key(|r| r.distance);

        Ok(TraversalResult {
            reached,
            truncated: over_budget,
            truncation_reason: over_budget.then_some(TruncationReason::NodeBudget),
        })
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
            // A node reaches itself in zero edges. Walking would return the
            // same answer only if the node happens to sit on a cycle, which
            // would make "is this connected to itself" depend on graph shape
            // rather than on definition.
            return Ok(Some(Path {
                nodes: vec![from.clone()],
                length: 0,
            }));
        }

        let mut builder = QueryBuilder::new("WITH RECURSIVE ");
        push_live_flakes(&mut builder, filter.as_of);
        push_logical_edges(&mut builder, filter);
        push_frontier(
            &mut builder,
            std::slice::from_ref(&from.id),
            direction,
            bounds.max_hops,
        );
        // Shortest first, then lexicographically by route. The second key is
        // what makes equal-length alternatives resolve the same way on every
        // run — a tiebreak left to the planner is a result that changes
        // between calls for no reason the caller can see.
        builder.push(" SELECT path, depth FROM frontier WHERE node_id = ");
        builder.push_bind(to.id.clone());
        builder.push(" ORDER BY depth, path LIMIT 1");

        let row = builder
            .build()
            .fetch_optional(self.pool())
            .await
            .map_err(|e| TraversalError::Backend(e.to_string()))?;

        Ok(row.map(|row| {
            let path: Vec<String> = row.get("path");
            Path {
                length: path.len().saturating_sub(1),
                nodes: path
                    .into_iter()
                    .map(|id| Sid::new(namespace::DSC, id))
                    .collect(),
            }
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
        let mut builder = QueryBuilder::new("WITH RECURSIVE ");
        push_live_flakes(&mut builder, filter.as_of);
        push_logical_edges(&mut builder, filter);
        push_frontier(
            &mut builder,
            std::slice::from_ref(&from.id),
            direction,
            bounds.max_hops,
        );
        builder.push(" SELECT path, depth FROM frontier WHERE node_id = ");
        builder.push_bind(to.id.clone());
        builder.push(" ORDER BY depth, path LIMIT ");
        // One past the cap, so hitting it is distinguishable from landing on it.
        builder.push_bind(i64::try_from(max_paths.saturating_add(1)).unwrap_or(i64::MAX));

        let rows = builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| TraversalError::Backend(e.to_string()))?;

        let over_cap = rows.len() > max_paths;
        let paths = rows
            .iter()
            .take(max_paths)
            .map(|row| {
                let path: Vec<String> = row.get("path");
                Path {
                    length: path.len().saturating_sub(1),
                    nodes: path
                        .into_iter()
                        .map(|id| Sid::new(namespace::DSC, id))
                        .collect(),
                }
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
        // The frontier's per-path guard stops a walk *before* it revisits a
        // node, so a cycle never appears inside `frontier` itself. It is found
        // by taking each reached node and asking whether an edge runs from it
        // back to somewhere already on its own path — the closing edge the
        // guard refused to follow.
        let mut builder = QueryBuilder::new("WITH RECURSIVE ");
        push_live_flakes(&mut builder, filter.as_of);
        push_logical_edges(&mut builder, filter);
        // Outgoing only, and not a caller choice: a cycle is a route that
        // returns to its own start, and `Both` makes every edge bidirectional
        // — which would report every connected pair as a two-node cycle.
        push_frontier(
            &mut builder,
            std::slice::from_ref(&start.id),
            Direction::Outgoing,
            bounds.max_hops,
        );
        builder.push(
            " SELECT f.path, d.dst AS closes_at
             FROM frontier f
             JOIN directed d ON d.src = f.node_id
             WHERE d.dst = ANY(f.path)",
        );

        let rows = builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| TraversalError::Backend(e.to_string()))?;

        let mut cycles: Vec<Cycle> = Vec::new();
        for row in &rows {
            let path: Vec<String> = row.get("path");
            let closes_at: String = row.get("closes_at");
            // The loop is the tail of the path from where it closes onward.
            // The prefix leading into the cycle is not part of it.
            let Some(entry) = path.iter().position(|id| *id == closes_at) else {
                continue;
            };
            let loop_nodes: Vec<Sid> = path[entry..]
                .iter()
                .map(|id| Sid::new(namespace::DSC, id.clone()))
                .collect();
            let cycle = Cycle::normalised(loop_nodes);
            if !cycles.contains(&cycle) {
                cycles.push(cycle);
            }
        }
        Ok(cycles)
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
        let seed_ids: Vec<String> = seeds.iter().map(|s| s.id.clone()).collect();

        let mut builder = QueryBuilder::new("WITH RECURSIVE ");
        push_live_flakes(&mut builder, filter.as_of);
        push_logical_edges(&mut builder, filter);
        push_frontier(&mut builder, &seed_ids, direction, bounds.max_hops);
        builder.push(
            ", reachable AS (
                SELECT DISTINCT ON (node_id) node_id, depth FROM frontier
                ORDER BY node_id, depth LIMIT ",
        );
        builder.push_bind(i64::try_from(bounds.max_nodes.saturating_add(1)).unwrap_or(i64::MAX));
        builder.push(
            " )
             SELECT r.node_id, r.depth, e.to_id, e.rel_type
             FROM reachable r
             LEFT JOIN filtered e ON e.from_id = r.node_id
             ORDER BY r.depth, r.node_id",
        );

        let rows = builder
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(|e| TraversalError::Backend(e.to_string()))?;

        let mut nodes: Vec<Sid> = Vec::new();
        let mut depths: Vec<usize> = Vec::new();
        let mut edges: Vec<EdgeRef> = Vec::new();

        for row in &rows {
            let from = row.get::<String, _>("node_id");
            let depth = usize::try_from(row.get::<i32, _>("depth")).unwrap_or(0);
            let node = Sid::new(namespace::DSC, from.clone());
            if !nodes.contains(&node) {
                nodes.push(node.clone());
                depths.push(depth);
            }
            if let Some(to) = row.get::<Option<String>, _>("to_id") {
                let edge = EdgeRef {
                    from: Sid::new(namespace::DSC, from),
                    to: Sid::new(namespace::DSC, to),
                    relationship: row.get::<String, _>("rel_type"),
                };
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
        }

        // Farthest-first truncation. Dropping arbitrary nodes would punch holes
        // in the middle of the picture; dropping the outer ring keeps a
        // complete view of the neighbourhood the caller actually asked about.
        let over_budget = nodes.len() > bounds.max_nodes;
        if over_budget {
            let mut ordered: Vec<(Sid, usize)> = nodes.into_iter().zip(depths).collect();
            ordered.sort_by_key(|(_, depth)| *depth);
            ordered.truncate(bounds.max_nodes);
            nodes = ordered.into_iter().map(|(node, _)| node).collect();
        }

        Ok(Subgraph {
            nodes,
            edges,
            truncated: over_budget,
            truncation_reason: over_budget.then_some(TruncationReason::NodeBudget),
        }
        // Edges to nodes the budget dropped would dangle. Enforced by the type
        // rather than trusted to this function.
        .without_dangling_edges())
    }
}
