# Plan: Graph Query — SPARQL Subset (Epic 7) ★

**Branch**: feat/engine-query
**Status**: Not started
**Depends on**: Epic 4 (triples), Epic 13 (authorization to compile into queries), **Epic 7a** (property-path evaluation calls `graph-owl-traversal`; this crate does not implement its own BFS — decision 2a)
**Crate**: `graph-owl-query` (pure parse + plan; execution via `TripleStore`)

## Goal

Multi-hop query the REST surface fundamentally cannot express. "Every table feeding a certified metric, owned by a team outside the finance domain, with a failing quality test" is one query here and impossible there.

## Resolved decisions

0. **Full SPARQL 1.1 is the target. The subset is a delivery order, not a scope.**

   This reverses the framing the plan opened with. It previously described a
   permanent subset "sized to metadata"; it is now a **complete implementation
   delivered in stages**, and the difference is not cosmetic:

   - A permanent subset means every unimplemented construct is a **decision to
     defend**, forever, to every user who hits one.
   - A staged complete implementation means every unimplemented construct is a
     **queue position**, and the honest answer is "not yet, and here is where it
     sits" rather than "no, and here is why you do not really want it".

   Two things make this affordable now. Parsing is free and total (decision 8
   below), so no construct costs language work. And the algebra is a closed set:
   "full SPARQL" is not an open-ended commitment but a finite list of node types
   with a completion table.

   What does *not* change: each stage ships tested, authorization-filtered and
   budgeted. A fast wrong answer is not progress toward a complete right one.

1. **The delivery order, sized to metadata.** BGP, `FILTER`, `OPTIONAL`, `UNION`, `MINUS`, `(NOT) EXISTS`, property paths, `ORDER BY`, `LIMIT`/`OFFSET`, `DISTINCT`, `SELECT`/`ASK`/`CONSTRUCT`. Later stages: aggregates, `GROUP BY`, `HAVING` and subqueries (v2); `SERVICE` (**Epic 101**); entailment regimes (v3, and they need Epics 6/98/99 to entail anything against). Every one is scheduled — none is refused. A reference implementation's SPARQL layer is ~29,000 lines; the subset that answers metadata questions is a fraction.
2. **Property paths are in scope, not deferred.** They are the reason to have SPARQL at all — `?t (dsc:feeds)+ ?u` is the lineage query, and without them REST endpoints are strictly better.
2a. **Traversal moved to Epic 7a, which therefore ships *before* this epic** despite sorting after it — see `ROADMAP.md`'s build-order table. Property paths (`p+`, `p*`) call `graph-owl-traversal`; this crate does not implement its own BFS. `shortest_path`, `all_paths`, `detect_cycles`, and `subgraph` are not expressible as property paths and live there.
3. **BGP matching is homomorphism-based, per spec.** Variables may bind to the same node. This is *not* subgraph isomorphism; getting it wrong produces subtly missing results.
4. **Authorization compiles into the query**, never post-filters. Post-filtering breaks `LIMIT` (a page of 25 becomes 3) and leaks existence through counts.
5. **Filter pushdown to the index scan.** `FILTER(?conf < 0.5)` must reach the scan, not materialize a million bindings and discard them. Orders of magnitude, verified in reference implementations.
6. **Cypher is a module lowering onto the same plan** — not a second engine. **No longer deferred**: Epic 7b (`07b-engine-cypher.md`) is scheduled, because Epic 7d's Bolt server cannot exist without it.
7. **The target is SPARQL 1.1, and 1.2 changes nothing yet.** SPARQL 1.2 Federated Query is at Candidate Recommendation (7 April 2026); the Query Language, Protocol and Entailment documents are Working Drafts. Since federation is explicitly out of scope for this subset, the one part of 1.2 that is nearly stable is the one part this epic does not implement — so 1.1 is the target and there is no churn to accept. The 1.2 change that *would* matter is triple-term patterns, and it arrives with Epic 9's decision on emitting `rdf:reifies` (`00k-standards-conformance.md`).
8. **Parse everything; evaluate a subset. Do not write the parser.**

   This reverses the earlier plan, which had Slice A hand-writing a
   recursive-descent parser for the subset. That is the wrong split, and the
   reason is what it does to a client.

   A hand-written subset parser rejects unsupported SPARQL **at the door, as a
   syntax error**. A tool connects, sends a perfectly valid standard query, and
   is told its syntax is wrong. That is a lie, and it is the worst possible
   first impression: the user concludes the endpoint is broken rather than
   partial.

   `spargebra` (Apache-2.0/MIT, the parser Oxigraph is built on, published
   standalone) parses **full SPARQL 1.1 Query and Update** — and SPARQL 1.2
   behind a feature flag — and emits the standard SPARQL 1.1 Query Algebra. It
   does no evaluation, which is exactly the division of labour this project
   wants: parsing is the commodity, evaluation over flakes is the part with
   value in it.

   So:

   ```
   query → spargebra (full SPARQL 1.1) → algebra → OUR planner → OUR executor
                                             │
                                             └─ unsupported node → precise error
                                                naming the construct
   ```

   Three consequences, all improvements:

   - **Every valid SPARQL query parses.** An unsupported one gets "MINUS is not
     supported yet", naming the algebra node — not "syntax error at 14".
   - **The subset stops being all-or-nothing.** Full parsing arrives on day one;
     evaluation grows node by node without ever touching the front end. "Do we
     support SPARQL" becomes a table of algebra nodes rather than a yes/no.
   - **The planner works on the standard algebra**, so the optimizer literature
     applies directly and a future contributor reads familiar shapes.

   The licence is permissive, so `00i` permits it. What this project does *not*
   take is the evaluator — the flake store, four index orderings and the
   compiled access predicate are the engine, and no external library knows about
   any of them.

9. **Adopt the evaluator too, not only the parser — `spareval` calls back into
   a dataset we implement.**

   Decision 8 stopped at "do not write the parser". That was half the finding.
   The Oxigraph ecosystem is four separable, permissively-licensed crates:

   ```
   text → spargebra → algebra → sparopt → optimized
                                              │
                                              ▼
                                        spareval ──calls──▶ QueryableDataset
                                              │              (OURS, over flakes)
                                              ▼
                                        sparesults → wire format
   ```

   `spareval` owns no store. It evaluates against a **`QueryableDataset` trait
   the caller implements** — so the scan, and everything that lives in it, stays
   ours:

   - **Index selection** across the four orderings — the pattern-to-index
     decision is inside our scan, where it belongs.
   - **`as_of`** — the dataset is constructed at a transaction time and only
     ever exposes that resolved state.
   - **The access predicate** — applied inside the scan, so the evaluator only
     ever receives permitted rows. Post-filtering, the leak Demo 2 exists to
     close, is structurally impossible here.
   - **`SERVICE`** — `spareval` takes a `ServiceHandler`, which is where Epic
     101's allow-list, timeout and outbound filtering live.

   This is the largest single reduction in scope in this plan. Joins, expression
   evaluation, aggregates, property paths and result serialisation are adopted;
   what remains is the mapping onto flakes, which is the part no library could
   have supplied.

   **Three things to verify before committing**, and they are the reason this is
   a decision rather than a foregone conclusion:

   1. **Budgets.** Nothing surveyed bounds anything. A `QueryableDataset` can
      count its own scans, but whether exceeding a limit yields clean truncation
      or a mid-query error needs testing — `00a`'s budget is not optional, and
      an unbounded query is worse than a missing feature.
   2. **Freshness stamping** (Epic 4 decision 8) wraps the result rather than
      living inside evaluation. Fine, but it must not be forgotten.
   3. **Whether `sparopt`'s generic rewrites fight our index selection.** A
      generic optimizer may reorder patterns in a way that is worse given which
      orderings exist. Measure before trusting it; it is separable.

10. **The subset is enumerated, not implied.** Every SPARQL pattern type appears in the completeness table below with a status. A pattern that is out of scope must produce a clear error naming it, never a silent misparse.

## Implementation reference

### Pipeline

```
query string → parse → QueryAst → plan → LogicalPlan → optimize → PhysicalPlan → execute → QueryResult
                (pure)             (pure)              (pure)                    (TripleStore)
```

Everything up to `PhysicalPlan` is pure and testable without a database. Only execution touches I/O.

### AST and plan → `graph-owl-query`

```rust
pub enum QueryForm {
    Select { vars: Vec<Var>, distinct: bool },
    Ask,
    Construct { template: Vec<TriplePatternTemplate> },
}

pub struct QueryAst {
    pub form: QueryForm,
    pub where_clause: GraphPattern,
    pub order_by: Vec<OrderCond>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub enum GraphPattern {
    Bgp(Vec<TriplePatternAst>),
    Path { s: Term, path: PropertyPath, o: Term },
    Filter(Box<GraphPattern>, Expr),
    Optional(Box<GraphPattern>, Box<GraphPattern>),
    Union(Box<GraphPattern>, Box<GraphPattern>),
    Graph(Term, Box<GraphPattern>),      // named graph scoping
    Join(Box<GraphPattern>, Box<GraphPattern>),
    Minus(Box<GraphPattern>, Box<GraphPattern>),
    Exists { inner: Box<GraphPattern>, negated: bool },   // EXISTS / NOT EXISTS
}

pub enum PropertyPath {
    Predicate(Sid),
    Inverse(Box<PropertyPath>),          // ^p
    Sequence(Vec<PropertyPath>),         // p1/p2
    Alternative(Vec<PropertyPath>),      // p1|p2
    ZeroOrMore(Box<PropertyPath>),       // p*
    OneOrMore(Box<PropertyPath>),        // p+
    ZeroOrOne(Box<PropertyPath>),        // p?
}

pub enum PhysicalOp {
    IndexScan { pattern: TriplePatternAst, index: IndexKind, pushed: Vec<Expr> },
    NestedLoopJoin { left: Box<PhysicalOp>, right: Box<PhysicalOp>, on: Vec<Var> },
    HashJoin { left: Box<PhysicalOp>, right: Box<PhysicalOp>, on: Vec<Var> },
    LeftJoin { left: Box<PhysicalOp>, right: Box<PhysicalOp> },   // OPTIONAL
    Union { inputs: Vec<PhysicalOp> },
    PathEval { s: Term, path: PropertyPath, o: Term, max_depth: usize },
    Filter { input: Box<PhysicalOp>, expr: Expr },
    Distinct(Box<PhysicalOp>),
    Sort { input: Box<PhysicalOp>, by: Vec<OrderCond> },
    Limit { input: Box<PhysicalOp>, n: usize, offset: usize },
    AntiJoin { left: Box<PhysicalOp>, right: Box<PhysicalOp>, on: Vec<Var> },  // MINUS
    SemiJoin { left: Box<PhysicalOp>, right: Box<PhysicalOp>, negated: bool }, // (NOT) EXISTS
}
```

### Pattern completeness

`00a-product-position.md` says the engine implements useful subsets, not specifications. A subset is only honest if it is enumerated — an unlisted pattern is indistinguishable from an oversight, and a user discovers it as a parse error at 2am rather than as a documented boundary.

| SPARQL pattern | Status | Reasoning |
|---|---|---|
| `Bgp` (triple patterns) | **v1** | The core |
| `Filter` | **v1** | Pushed into index scans |
| `Optional` (`LeftJoin`) | **v1** | Metadata is sparse; most useful queries need it |
| `Union` | **v1** | Type-alternation queries |
| `Graph` | **v1** | Named-graph scoping is how the reasoning overlay is isolated |
| `Path` (property paths) | **v1** | The thing REST cannot express |
| `Minus` | **v1 — added** | "Assets with no owner", "columns not classified". Governance queries are disproportionately *negative*, and `Minus` is the natural form. Omitting it was an oversight, not a decision |
| `NotExists` / `Exists` | **v1 — added** | Same reason. `FILTER NOT EXISTS` is the shape most people reach for first; without it the negative queries above are inexpressible |
| `Bind` | v2 | Useful, not blocking; computed bindings can wait for a demand case |
| `Values` | v2 | An inline table for parameterized queries. Wanted by the Epic 41 workbench; not needed to ship the engine |
| `Subquery` | v2 | Needed for per-group aggregation. Deferred with the aggregate functions it exists to serve |
| `Unwind` | **out** | Not SPARQL — it is Cypher's, and it lands with Epic 7b in the Cypher front end |
| `Service` (federation) | **out** | `ROADMAP.md` not-doing. Federation is a distributed-query project |

**`Minus` and `NotExists` move into v1** because negation is not an advanced feature in a governance context — it is the primary question shape. "What is *missing*" is most of what a steward asks.

### Execution model

Ten `PhysicalOp` variants without an execution protocol is a plan, not an engine. The operators share one lifecycle:

```rust
pub trait Operator {
    fn open(&mut self, ctx: &mut ExecContext) -> Result<(), QueryError>;
    fn next_batch(&mut self, ctx: &mut ExecContext) -> Result<Option<Batch>, QueryError>;
    fn close(&mut self) -> Result<(), QueryError>;
    fn schema(&self) -> &[Var];      // which variables this operator binds
}

pub struct Batch { pub vars: Vec<Var>, pub rows: Vec<Vec<Option<Term>>> }   // default 1024 rows
```

**Batched pull, not row-at-a-time and not materialize-everything.** Row-at-a-time pays a virtual dispatch per row; materializing pays unbounded memory on the exact queries most worth bounding. A batch amortizes dispatch while keeping peak memory a function of batch size and plan depth rather than result size.

`schema()` is what makes join correctness checkable at plan time rather than discoverable at runtime: a join whose inputs do not share the variables it joins on is a planner bug, and it should fail to plan rather than silently produce a cross product.

### Resource tracking

A query engine reachable by agents (Epic 14) and by arbitrary Bolt clients (Epic 7d) needs to know what a query cost *before* it finishes.

```rust
pub struct Tracker {                    // atomics: cheap to update, safe to read mid-flight
    pub flakes_read: AtomicU64,
    pub rows_scanned: AtomicU64,
    pub rows_returned: AtomicU64,
    pub elapsed: Instant,
}

pub struct QueryLimits {
    pub max_flakes_read: Option<u64>,
    pub max_rows_returned: Option<u64>,
    pub max_duration: Duration,
}
```

Three properties that make this worth building rather than bolting on:

1. **Zero cost when disabled** — an `Option<Arc<Tracker>>` that is `None` costs a null check per batch, not per row.
2. **Limits are enforced at batch boundaries**, so a runaway query is stopped in bounded time without a per-row check.
3. **The counters are the same numbers Epic 10 exports as metrics and Epic 41 shows in the workbench.** One source, three consumers — not three approximations.

A finer-grained cost model (per-operation weights, a fuel abstraction) is deferred to `37a-scale.md`, where there are benchmarks to calibrate it against. Inventing weights without measurements produces a number that looks authoritative and is not.

### Fast-path candidates

The general plan-and-execute path is correct for every query and optimal for none of the common ones. These five shapes dominate a metadata workload and each collapses to a single index operation:

| Shape | Fast path | Why it matters |
|---|---|---|
| `COUNT` over one pattern | Index-only count, no materialization | Every facet in the Epic 39 search UI |
| Star join on a bound subject | One SPOT range scan, group in place | Rendering an entity page — the single commonest query in the system |
| Bound `(p, o)` existence check | POST point lookup, early exit on first hit | Every authorization predicate (Epic 13) |
| `ORDER BY` + small `LIMIT` | Top-k heap, never a full sort | Search results, recent changes |
| Single-predicate reachability | Direct traversal handoff to Epic 7a | Lineage, containment, `sameAs` closure |

**Fast paths are recognized on the logical plan and must produce results identical to the general path** — asserted by a differential test running both and comparing, which is the only way a fast path stays honest as the planner changes.

### Index selection

The planner maps each bound-term pattern to the index from Epic 4:

| Bound | Index |
|---|---|
| `s` | SPOT |
| `s`, `p` | SPOT |
| `p` | PSOT |
| `p`, `o` | POST |
| `o` (reference) | OPST |
| nothing | PSOT (full scan, cardinality-warned) |

### Algebraic optimization

Six rewrites in the planner, all pure and separately testable:

| Optimization | What it does | Without it |
|---|---|---|
| **Selectivity estimation** | Per-predicate cardinality from `predicate_registry` statistics | Join order is guesswork |
| **Join ordering** | Reorder BGP patterns most-selective-first | Nested loops over the largest relation |
| **Filter pushdown** | `FILTER` into the index scan | Materialize millions of bindings, discard most |
| **Projection pushdown** | Fetch only variables reaching `SELECT` | Wide rows carried through every join |
| **Subquery decorrelation** | Flatten correlated subqueries where possible | Re-execute the inner query per outer row |
| **Lazy `UNION` evaluation** | Evaluate a branch only when its bindings are demanded | Both branches always evaluated, even when one suffices |

Statistics are maintained incrementally on flake write, not by periodic `ANALYZE` — a stale cardinality produces a bad plan silently, and the write path already knows what changed.

### Join ordering

Cardinality-driven, using `TripleStore::count` on each pattern: order patterns ascending by estimated cardinality so the smallest relation drives. Counts are cached per query. This is the single highest-leverage optimization and the reason `count` is on the port.

### Property-path evaluation

`p+` and `p*` are bounded BFS with a visited set — the same machinery as Epic 29's lineage traversal, and it must be shared rather than reimplemented. Default `max_depth` 10, configurable, with `truncated` reported. Cycles terminate.

### Authorization compilation

Epic 13 produces a `Predicate` intermediate representation. The planner lowers it into `IndexScan.pushed` so every scan is already filtered. A query the principal may not answer at all returns empty, not `403` — existence must not leak through error codes.

## Acceptance criteria (feature level)

- [ ] Every subset feature above parses, plans, and executes correctly.
- [ ] Property paths traverse, terminate on cycles, and report truncation.
- [ ] BGP semantics are homomorphism-based — a variable may bind twice.
- [ ] Filters reach the index scan, verified by plan inspection.
- [ ] Join order is cardinality-driven.
- [ ] Authorization is compiled in; results are pre-filtered.
- [ ] `POST /sparql` returns `application/sparql-results+json`; `CONSTRUCT` returns Turtle.
- [ ] Results are cursor-paginated per `00d-api-conventions.md`.
- [ ] A malformed query returns `400` with the parse error position.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Parse the subset (pure)

**Value**: The front end, fully testable with no database.
**Path**: `spargebra` parses and `spareval` evaluates (decisions 8, 9). This slice is the **`QueryableDataset` implementation over flakes** — pattern-to-index selection, `as_of` resolution, and the access predicate applied inside the scan. No parser and no evaluator is written.
**Acceptance criteria**:
- `SELECT`, `ASK`, `CONSTRUCT` parse.
- BGP with 1, 2, and 3 patterns; `;` and `,` abbreviations.
- `PREFIX` declarations resolve; an undeclared prefix → error naming it.
- `FILTER` with comparison, `&&`, `||`, `!`, `BOUND`, `regex`.
- `OPTIONAL`, `UNION`, `GRAPH`.
- Property paths: `p`, `^p`, `p1/p2`, `p1|p2`, `p*`, `p+`, `p?`, and nested combinations.
- `ORDER BY`, `LIMIT`, `OFFSET`, `DISTINCT`.
- Literals: string with language tag, typed literal, integer, decimal, boolean.
- A syntax error reports **line and column**.
- Unsupported constructs (`SERVICE`, `GROUP BY`) → a *specific* "unsupported" error, not a generic parse failure.
**RED**: A corpus of ~50 queries asserting the mapping to `LogicalPlan`, plus malformed queries with expected error positions. **The unsupported-construct tests are the point of this slice**: a valid query using a node we do not evaluate must return "`MINUS` is not supported yet", naming the construct — never a syntax error, because the syntax was fine. Mutator watch: a parser accepting `SERVICE` must fail; off-by-one error positions must fail the position assertions.
**GREEN**: parser.
**REFACTOR**: assess hand-written vs. a parser-generator dependency. Hand-written for the subset — the grammar is small, and a generator's error messages are worse, which matters because error position is an acceptance criterion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Plan and select indexes (pure)

**Value**: The optimization that makes queries fast, tested as a pure function.
**Path**: `QueryAst` → `LogicalPlan` → `PhysicalPlan` with index selection and join ordering.
**Acceptance criteria**:
- Each bound-term combination selects the index from the table above — asserted **by name** in the plan.
- Join order is ascending by estimated cardinality, using injected counts (no I/O in the test).
- A `FILTER` on a scanned variable appears in that scan's `pushed`, not as a separate `Filter` op.
- A `FILTER` that cannot be pushed (spans two patterns) remains a `Filter` op.
- `OPTIONAL` becomes `LeftJoin`, never an inner join.
- `UNION` becomes `Union` with both branches planned.
- Planning is deterministic — same AST and counts, identical plan.
**RED**: Plan-shape tests asserting the operator tree and chosen index by name. A cardinality test with injected counts asserting reorder. A pushdown test asserting the filter is *inside* the scan. Mutator watch: an always-SPOT planner must fail the index assertions; ignoring cardinality must fail the reorder test; `OPTIONAL` planned as inner join must fail — that one silently drops rows.
**GREEN**: planner, index selection, cardinality ordering, pushdown.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Execute BGPs with correct semantics

**Value**: Correct answers.
**Path**: physical operators over `TripleStore`.
**Acceptance criteria**:
- Single-pattern query returns matching bindings.
- Two-pattern join on a shared variable returns the join.
- **Homomorphism**: `{ ?a dsc:feeds ?b . ?b dsc:feeds ?c }` on a self-loop `x feeds x` binds `?a = ?b = ?c = x`. An isomorphism implementation returns nothing here.
- Unbound variables in results are `Unbound`, not omitted.
- `DISTINCT` deduplicates whole rows, not per-variable.
- `ASK` returns a boolean without materializing bindings.
- `CONSTRUCT` produces triples from the template, with blank-node handling.
- Empty result is `200` with zero bindings, not `404`.
**RED**: The self-loop homomorphism test is the specification for decision 3 — it is the case that separates correct SPARQL from an intuitive-but-wrong implementation. `DISTINCT` on rows differing in one column must not collapse. Mutator watch: isomorphism semantics must fail the self-loop; per-variable `DISTINCT` must fail the row test.
**GREEN**: operators, binding semantics.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Property paths traverse safely

**Value**: The lineage query, and the reason SPARQL earns its place.
**Path**: `PathEval` as bounded BFS with a visited set, shared with Epic 29.
**Acceptance criteria**:
- `p+` on a 5-deep chain returns all reachable nodes.
- `p*` includes the start node; `p+` does not.
- `^p` traverses in reverse via OPST.
- `p1/p2` sequences; `p1|p2` alternates.
- A cycle terminates and returns each node once.
- Exceeding `max_depth` returns a partial result flagged `truncated`, not an error.
- A diamond returns the far node once, not twice.
- Traversal shares one implementation with Epic 29's lineage — asserted structurally, not by convention.
**RED**: Depth-5 test asserting exactly the reachable set. `p*` vs `p+` on the start node — the classic off-by-one. Cycle test with a timeout. Diamond dedup test. Mutator watch: `p*` excluding the start must fail; a missing visited set must hang; depth off-by-one must fail the depth-5 boundary.
**GREEN**: bounded BFS, shared traversal module.
**REFACTOR**: this is the second consumer of bounded traversal (Epic 29 is the other). Extract to one place now — two implementations of cycle detection is a latent divergence.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Authorization is compiled in

**Value**: A SPARQL endpoint that is safe to expose. Without this, Epic 14 cannot ship.
**Path**: Epic 13's predicate IR lowered into `IndexScan.pushed`.
**Acceptance criteria**:
- A principal restricted to one domain sees only that domain's triples.
- Restricted triples are absent from results — not present-and-blanked.
- `LIMIT 25` returns 25 *permitted* rows, not 25 pre-filter rows minus the denied ones.
- Counts and `ASK` respect the filter.
- Filtering is in the scan, verified by plan inspection — one round trip, not fetch-then-discard.
- A policy shape the planner cannot lower **fails loudly** rather than silently returning unfiltered results.
- An admin principal sees everything.
**RED**: The `LIMIT` test is the one that catches post-filtering — a page of 25 with 10 denied returns 15 under post-filtering and 25 under compiled filtering. The unsupported-policy test asserts a raise rather than a leak. Mutator watch: post-filtering must fail the `LIMIT` test; silently dropping an unlowerable clause must fail the loud-failure test — that is the data-leak failure mode.
**GREEN**: predicate lowering, plan integration.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: The SPARQL endpoint

**Value**: Callers can use it.
**Path**: `POST /sparql` and `GET /sparql?query=`.
**Acceptance criteria**:
- `SELECT`/`ASK` → `application/sparql-results+json` in spec shape.
- `CONSTRUCT` → `text/turtle`.
- `GET` with a long query → `414`, with a message pointing at `POST`.
- Malformed query → `400` problem+json carrying line and column.
- Query timeout → `408` with a partial-results indication, not a hung connection.
- Results are cursor-paginated; the cursor encodes plan and offset so a mid-pagination plan change is `400`.
- `Content-Type` negotiation honours `Accept`.
- Queries are logged with duration and result count; the query text is logged at debug only (it may contain sensitive literals).
**RED**: Spec-shape golden tests for results JSON. A timeout test asserting `408` rather than a hang. A logging test asserting query text is absent at info level — a query embedding a customer identifier must not land in production logs. Mutator watch: logging query text at info must fail that assertion.
**GREEN**: endpoint, serialization, negotiation, timeout, logging.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Aggregates and `GROUP BY`** → **promoted to v2, not deferred indefinitely.** The earlier reason — "metadata questions are mostly existential" — does not survive contact with this project's own console: the Overview page (Epic 93) computes counts by kind, documentation coverage and a recent-changes list, and every one of those is `COUNT` + `GROUP BY` written by hand in SQL because SPARQL cannot express it. "How many columns per schema", "how many undocumented tables per owner" are the questions a data steward opens a catalog to ask.

  Cheaper than it looks now that parsing is free (decision 8): `spargebra` already parses `GROUP BY`, `HAVING` and the aggregate functions into standard algebra nodes. What remains is evaluating a grouping operator over a result stream, which is a fold — not a language feature.

  Sequenced after the v1 patterns because an aggregate over a wrong join is confidently wrong, and joins have to be right first.
- **Subqueries, `VALUES`, `BIND`** → v2 per the pattern-completeness table; `VALUES` is wanted by the Epic 41 workbench and is the likeliest of the three to be pulled forward.
- **`SERVICE` / federation** → not planned. Cross-instance query is Epic 37b's export territory.
- **Entailment regimes** → Epic 6's overlay is opt-in per query via `GRAPH graph:reasoning`; formal regimes are not planned.
- **SPARQL Update (`INSERT`/`DELETE`)** → deliberately not planned. Writes go through the catalog API so validation, versioning, and authorization apply. A SPARQL write path would bypass all three.
- **Cypher** → a module lowering onto the same plan. **Now scheduled** as Epic 7b (`07b-engine-cypher.md`), not on demand — Epic 7d's Bolt server depends on it. `UNWIND` lands there rather than here.
- **A calibrated cost model (per-operation weights)** → `37a-scale.md`, where there are measurements to calibrate against.
- **Query result caching** → after Epic 37a measures which queries repeat.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Parse and plan stages have **zero I/O dependencies** — asserted.
5. Plan-shape assertions name the chosen index for every pattern shape.
6. All path-evaluation tests carry timeouts.
7. **Every fast path is differentially tested against the general path** and produces identical results.
8. **Every pattern in the completeness table is either implemented or rejected with a documented error** — no pattern parses into a silent no-op.
9. BGP 3-pattern query < 20ms p50 per `00a-product-position.md`.
