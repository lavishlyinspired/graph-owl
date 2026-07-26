# Plan: Graph Traversal Engine (Epic 7a)

**Branch**: feat/engine-traversal
**Status**: Not started
**Depends on**: Epic 4 (triples, SPOT/POST/OPST indexes)
**Unblocks**: Epic 7 (property paths), Epic 29 (lineage), Epic 14 (subgraph for MCP)
**Crates**: **`graph-owl-traversal`** (new — pure graph algorithms over the `TripleStore` port)

## Goal

Graph algorithms the query language cannot express: shortest path, all paths, cycle detection, and subgraph extraction — with one bounded, cycle-safe traversal primitive shared by every consumer.

## Why this is a separate epic and a separate crate

I previously called traversal "a module in `graph-owl-query`, since property paths are a SPARQL feature". That was wrong, on two counts:

1. **SPARQL property paths answer reachability, not the other four questions.** `?a (dsc:feeds)+ ?b` tells you *whether* B is reachable from A. It cannot return *the shortest path*, *all paths*, *the cycles*, or *a bounded subgraph around a seed set*. Those are graph algorithms, not pattern matches.
2. **Repeated BGP evaluation for multi-hop degrades to O(n²).** Expressing a 3-hop lineage query as three joined patterns materializes the intermediate cross-products. A traversal engine walks the frontier instead.

It earns a crate rather than a module because it is **pure algorithms over the `TripleStore` trait** with no parser dependency, and it has four independent consumers (query, lineage, MCP, reasoning's `sameAs` closure). A module inside `graph-owl-query` would force every traversal consumer to depend on the SPARQL parser.

## Resolved decisions

1. **One traversal primitive, four algorithms on top.** A single bounded, visited-set BFS frontier walk; `neighbors`, `shortest_path`, `all_paths`, `detect_cycles`, and `subgraph` are all expressed over it. Two implementations of cycle detection is a latent divergence — this epic exists partly to prevent that.
2. **Reified edges mean two hops per logical edge.** `entity → relationship → entity`. The traversal engine hides this; callers think in logical edges. Getting it wrong doubles every reported distance.
3. **Every traversal is bounded, always.** `max_hops` and a node budget, with `truncated` reported. Real metadata graphs contain cycles; an unbounded walk hangs in production, not in tests.
4. **Postgres recursive CTE for the frontier walk**, not N round trips. One statement per traversal using SPOT for outgoing and OPST for incoming. This is a Postgres-shaped method and is recorded as such.
5. **`all_paths` is exponential and capped hard.** Path enumeration between two nodes in a dense graph explodes. Default cap 100 paths, and exceeding it reports truncation rather than running to completion.

## Implementation reference

```rust
#[async_trait]
pub trait TraversalEngine: Send + Sync {
    async fn neighbors(&self, start: &Sid, dir: Direction, max_hops: usize)
        -> Result<TraversalResult, TraversalError>;

    async fn shortest_path(&self, from: &Sid, to: &Sid, dir: Direction)
        -> Result<Option<Path>, TraversalError>;

    async fn all_paths(&self, from: &Sid, to: &Sid, max_hops: usize, max_paths: usize)
        -> Result<PathSet, TraversalError>;

    async fn detect_cycles(&self, start: &Sid, max_hops: usize)
        -> Result<Vec<Cycle>, TraversalError>;

    async fn subgraph(&self, seeds: &[Sid], max_hops: usize, budget: NodeBudget)
        -> Result<Subgraph, TraversalError>;
}

pub enum Direction { Outgoing, Incoming, Both }

pub struct TraversalResult {
    pub reached: Vec<Reached>,     // entity + distance + one representative path
    pub truncated: bool,
    pub truncation_reason: Option<TruncationReason>,  // MaxHops | NodeBudget | PathCap
}

pub struct Path { pub nodes: Vec<Sid>, pub edges: Vec<Sid>, pub length: usize }

pub struct Subgraph {
    pub nodes: Vec<Sid>,
    pub edges: Vec<EdgeRef>,       // relationship Sid + endpoints + type
    pub truncated: bool,
}

pub struct EdgeFilter {            // applies to every algorithm
    pub relationship_types: Option<Vec<RelationshipType>>,
    pub min_confidence: Option<f64>,
    pub as_of: Option<i64>,        // time-travel, per Epic 4
}
```

### The frontier walk

```sql
WITH RECURSIVE frontier(node_ns, node_id, depth, path) AS (
    SELECT $1, $2, 0, ARRAY[$2]
  UNION ALL
    SELECT e.to_ns, e.to_id, f.depth + 1, f.path || e.to_id
    FROM frontier f
    JOIN logical_edges e ON (e.from_ns, e.from_id) = (f.node_ns, f.node_id)
    WHERE f.depth < $3
      AND NOT e.to_id = ANY(f.path)        -- cycle guard, per-path
)
SELECT DISTINCT ON (node_ns, node_id) * FROM frontier ORDER BY node_ns, node_id, depth
```

`logical_edges` is a view over `flakes` collapsing the reified two-hop into one logical edge — defined once, so decision 2 is enforced in the schema rather than in every caller.

`NOT ... = ANY(path)` is a **per-path** cycle guard, which is what `all_paths` needs. `DISTINCT ON` then gives the shortest distance per node, which is what `neighbors` needs. Using a global visited set instead would break `all_paths`; using only per-path guards would make `neighbors` exponential. Both are required, at different stages.

## Acceptance criteria

- [ ] All five algorithms implemented over one shared frontier primitive.
- [ ] Reified two-hop edges are hidden — reported distances count logical edges.
- [ ] Every algorithm terminates on a cyclic graph.
- [ ] `max_hops`, node budget, and path cap all report `truncated` with a reason.
- [ ] `EdgeFilter` applies uniformly, including `as_of` time-travel.
- [ ] The frontier walk is one Postgres statement, not N round trips.
- [ ] Epic 7's property paths and Epic 29's lineage both consume this — asserted structurally.
- [ ] Traversal latency meets the Epic 37a budgets on a 100k-entity graph.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`, `testing`, `mutation-testing`, `refactoring` loaded first.

### Slice A: The logical-edge view and frontier primitive

**Acceptance criteria**: a `logical_edges` view collapsing reified relationships into one row per logical edge, with type and confidence; the frontier CTE walks it in one statement; a 5-deep chain returns depths 1–5, not 2–10 (decision 2); `Direction::Incoming` uses OPST; `Both` unions without duplicating; query plan asserts index use, not a sequential scan.
**RED**: The depth test is the specification for decision 2 — a 5-edge chain must report distance 5. An `Incoming` plan test asserting OPST. Mutator watch: counting reified hops must fail the depth test (reporting 10); a `Both` implementation that unions without dedup must fail the duplicate assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: `neighbors` and bounded reachability

**Acceptance criteria**: returns each reachable node once with its shortest distance and one representative path; `max_hops` bounds exactly (5 returns 5 hops, not 6); a diamond returns the far node once at the shorter distance; a cycle terminates; node budget exceeded → `truncated` with `NodeBudget`; an isolated node returns itself at depth 0 with no error.
**RED**: Diamond test asserting one entry at the shorter distance. Off-by-one boundary at `max_hops`. Cycle test with an explicit timeout. Mutator watch: `<` vs `<=` on depth must fail the boundary; a missing `DISTINCT ON` must fail the diamond.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: `shortest_path`

**Acceptance criteria**: returns the node and edge sequence, not just length; unreachable → `None`, not an error; equal-length alternatives resolve deterministically (documented tiebreak); a self-path returns length 0; respects `EdgeFilter` — a filtered-out edge is not used even if it would be shorter.
**RED**: The filter test is the important one: a short path through a low-confidence edge must be excluded when `min_confidence` rules it out, and the longer permitted path returned instead. Determinism test running twice. Mutator watch: applying the filter after path selection must fail it — that returns a path the caller explicitly excluded.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: `all_paths` and `detect_cycles`

**Acceptance criteria**: `all_paths` enumerates distinct paths with the per-path cycle guard; `max_paths` caps hard and reports `PathCap` truncation; a dense graph does not run to completion before capping; `detect_cycles` returns each distinct cycle once, normalized so rotations are not reported as separate cycles; a DAG returns no cycles; a self-loop is a cycle of length 1.
**RED**: A dense-graph test with a low `max_paths` asserting it returns promptly and flags `PathCap` — an uncapped enumeration is the hang risk here. A cycle-normalization test asserting A→B→C→A and B→C→A→B are one cycle. Mutator watch: no path cap must exceed a test timeout; unnormalized cycles must fail the rotation test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: `subgraph`

**Acceptance criteria**: takes a seed set, returns nodes and edges within `max_hops` of any seed; overlapping seed neighbourhoods produce one merged subgraph with no duplicate nodes or edges; node budget truncates by dropping the *farthest* nodes first, not arbitrarily; edges to dropped nodes are omitted so the result is internally consistent; an empty seed set returns an empty subgraph.
**RED**: The consistency test: after budget truncation, every edge's endpoints must be present in the node set — a subgraph with dangling edges is unusable by Epic 14's MCP consumer. A farthest-first truncation test. Mutator watch: retaining edges to dropped nodes must fail the consistency test; arbitrary truncation must fail the farthest-first assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Consumers share one implementation

**Acceptance criteria**: Epic 7's `p+`/`p*` property-path evaluation calls this engine rather than its own BFS; Epic 29's lineage traversal calls it; Epic 14's `explain_lineage` and subgraph tools call it; asserted **structurally** — a test enumerating traversal implementations in the workspace and asserting there is exactly one; `EdgeFilter`'s `as_of` gives time-travelling traversal for free.
**RED**: The single-implementation test — a grep-or-AST check failing if a second BFS with a visited set appears anywhere. This is the guard against the divergence decision 1 exists to prevent. An `as_of` traversal test asserting historical topology. Mutator watch: a duplicated traversal must fail the structural test.
**REFACTOR**: this slice's job is deleting the duplicate implementations Epics 7 and 29 would otherwise have grown.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Graph analytics** — PageRank, centrality, community detection → **not planned**; see `ROADMAP.md`'s not-doing table. Traversal answers "what is connected to what"; analytics answers "what is important", and Epic 28's usage signals answer that better and cheaper for a metadata graph.
- **Weighted shortest path** (Dijkstra over edge confidence) → unweighted BFS is what lineage needs; add if a weighted question appears.
- **Bidirectional search** for `shortest_path` → an optimization; revisit if Epic 37a shows the single-ended walk is the bottleneck.
- **Graph-native storage for traversal** → Epic 4's Oxigraph note; the same trigger applies.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Every traversal test carries an explicit timeout** so a termination regression fails CI rather than wedging it.
5. `graph-owl-traversal` has **zero I/O dependencies beyond the `TripleStore` port** — asserted.
6. Exactly one traversal implementation exists in the workspace (Slice F).
