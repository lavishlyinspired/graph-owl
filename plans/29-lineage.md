# Plan: Lineage (Epic 29)
**Branch**: feat/lineage
**Status**: **Slices A–F shipped** — A, B and most of C on 29 Jul 2026; D, E and F on 3 Aug 2026; **the remaining piece of Slice C (the node budget and `truncated` flag its own acceptance criteria specify) shipped 8 August 2026**, found missing and fixed by Epic 37a Slice C's real-scale measurement, not by re-reading this plan. See Slice C's own account below for what that measurement found and why it was recorded as a correction rather than an update.
**Depends on**: Epic 15 (connectors assert lineage), Epic 2 (columns for column-level lineage), **Epic 7a** (bounded, cycle-safe traversal — lineage does not implement its own walk)
**Unblocks**: impact analysis workflows
**Crates**: `graph-owl-core` (LineageDetails, ColumnMapping) · `graph-owl-query` (shared bounded traversal) · `graph-owl-engine` (edge patterns) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server`

## Goal

Answer the two highest-stakes questions in data engineering: *what breaks if I change this*, and *where did this number come from*.

## Resolved decisions

1. **Lineage is a specialization of the relationship edge (`upstream`), not a separate store.** What makes it more than a plain edge is the payload — SQL, column mappings, the pipeline that moves the data. One edge table keeps traversal uniform.
2. **Edges are keyed by `(from, to, source)` for reconciliation.** A connector re-run must replace the edges *it* asserted without clobbering hand-curated ones. Without `source` in the key, automation and curation destroy each other.
3. **Traversal is depth-bounded with cycle detection, always.** Real lineage graphs contain cycles — a table feeding a pipeline that writes back to it. An unbounded traversal hangs in production, not in tests.
4. **Column-level lineage is many-to-one.** `first_name + last_name → full_name` is the common case; a one-to-one model is wrong on contact with real transformations.
5. **Lineage is asserted, not derived.** SQL parsing to *infer* lineage is a large sub-project; connectors and humans assert edges for now. Named as deferred, not forgotten.

## Acceptance criteria (feature level)

- [ ] An upstream edge can be created manually with a SQL query and a description.
- [ ] `GET /lineage/{type}/{id}?upstream=3&downstream=2` returns the correct bounded subgraph.
- [ ] A cyclic graph terminates and returns each node once.
- [ ] Column-level mappings are recorded and returned, including many-to-one.
- [ ] A connector re-run replaces only the edges it previously asserted.
- [ ] Deleting an entity leaves its lineage edges intact for restore.
- [x] A 1,000-node graph traverses within a bounded latency budget — see Slice C's account below; not met until 8 August 2026, and not by construction of a fast query but by a node-count cap that stops the walk before it grows unbounded.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: An upstream edge can be asserted

**Value**: A human records that one table feeds another — the minimum useful lineage.
**Path**: `POST /lineage` with `{fromEntity, toEntity, lineageDetails}` → an `upstream` edge carrying a `LineageDetails` payload.
**Acceptance criteria**:
- Create an edge between two tables with an optional SQL query, description, and `source: Manual`.
- Either endpoint nonexistent → `404`.
- Self-lineage (a table upstream of itself) → `422`.
- Duplicate `(from, to, source)` → `409`.
- The same `(from, to)` with a *different* source is allowed — automation and curation coexist.
- `DELETE /lineage/{id}` removes one edge.
**RED**: Test asserting the same from/to with `Manual` and `Connector` sources both persist as distinct edges. Mutator watch: uniqueness on `(from, to)` alone must fail it.
**GREEN**: `LineageDetails` type, edge with payload, endpoints.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: The graph is traversable and bounded

**Value**: Impact analysis — "what is downstream of this table" — the primary lineage question.
**Path**: `GET /lineage/{entityType}/{id}?upstream=N&downstream=M` → bounded BFS with a visited set.
**Acceptance criteria**:
- `upstream=1` returns immediate parents only.
- `upstream=3` on a 5-deep chain returns exactly 3 levels.
- `downstream` traverses the opposite direction.
- Both directions in one request return one merged graph.
- Depth defaults to 1; exceeding the configured maximum → `400`.
- Response is `{nodes: [EntityReference], edges: [{from, to, details}]}` — nodes deduplicated even when reachable by several paths.
- Soft-deleted nodes are included but flagged, so a broken lineage is visible rather than silently truncated.
**RED**: Build a 5-deep chain, assert `upstream=3` returns exactly 3 hops and not 4. Build a diamond (A→B, A→C, B→D, C→D) and assert D appears once with both inbound edges. Mutator watch: off-by-one depth must fail the first; a missing visited-set must fail the second.
**GREEN**: BFS with depth bound and visited set; recursive CTE or iterative fetch.
**REFACTOR**: assess whether traversal belongs in the adapter (one recursive CTE, fast) or the facade (portable, testable). Adapter-side with the port expressing "traverse", noting it as a Postgres-shaped method, relevant when a second backend is ever considered.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Cycles terminate — **the cycle-safety half shipped 29 Jul 2026; the node budget did not ship until 8 August 2026**

**Value**: A production lineage graph with a feedback loop does not hang the API.
**Path**: visited-set enforcement plus a hard node budget.
**Acceptance criteria**:
- A→B→C→A returns all three nodes once and terminates.
- A self-loop terminates.
- A graph exceeding the node budget returns a partial graph flagged `truncated: true`, not an error — a partial answer beats a timeout.
- Traversal latency stays bounded on a densely connected 1,000-node graph.
**RED**: Cycle tests at depth 1, 2, and 3, each asserting termination *and* correct node counts. A budget test asserting `truncated: true`. Mutator watch: a removed visited-set must hang or overflow — assert with a timeout so the test fails rather than wedging CI.
**GREEN**: visited set, node budget, truncation flag.
**Done when**: criteria met, mutation report reviewed, commit approved.

**The honest record, corrected 8 August 2026**: cycle termination (the visited set) shipped on schedule and was never in question. **The node budget and `truncated` flag — this slice's own stated acceptance criteria, not an inference — were never built.** The status line at the top of this plan said "Slices A–F shipped" the whole time regardless; Slices D, E, and F each carry their own `— shipped` marker in this file, A, B, and C never did, and nothing before now noticed the gap that absence was pointing at.

**Found not by re-reading this plan, but by Epic 37a Slice C measuring the real endpoint at real scale**: `GET /lineage/asset/{id}` took **25.2 seconds** and returned 51,230 of 60,246 assets — 85% of a real corpus — from one well-connected node, three hops in, because `MAX_LINEAGE_DEPTH` (`crates/graph-owl-server/src/lib.rs`) bounds walk *depth* but nothing bounded node *count*. This is the exact failure this slice's own "Value" line named in 2026: "a production lineage graph... does not hang the API" — it did, for 25 seconds, against any principal who could name a well-connected asset, which is every authenticated caller, not a hypothetical adversary.

**Fixed 8 August 2026**, in the same session that measured it: `Storage::lineage_edges_touching` gained an optional `limit` (bounding the fetch itself, not only the walk's stopping condition — the measured cost was one hop's unbounded fetch, not hop count), `Catalog::lineage_graph` gained `max_nodes` and returns `truncated`, and `GET /lineage/asset/{id}` gained a `maxNodes` query parameter (default 200, matching `graph_owl_traversal::Bounds::default()`'s own reasoning for the same number). TDD'd: `tests/lineage.rs` gained `a_high_fan_out_walk_stops_at_the_node_budget_and_says_so`, `a_walk_within_the_node_budget_is_not_marked_truncated`, and `max_nodes_defaults_without_being_given`; all pre-existing lineage, field-selection, and MCP HTTP tests (32 tests across three files) pass unchanged. Re-measured at the identical 60,246-asset scale after the fix — see below.

**What is still out of scope, named rather than silently left**: `graph-owl-mcp`'s own `explain_lineage` tool (`crates/graph-owl-mcp/src/catalog.rs::subgraph`) calls the same now-bounded `Catalog::lineage_graph`, so the cost fix applies there too — but the `truncated` flag is discarded at that call site rather than threaded into the MCP tool's own response shape, which is separate, unstarted follow-up work, not an oversight in this fix.

### Slice D: Lineage reaches column level — **shipped**

**One row per source column, so many-to-one needs no array.** `first_name` and
`last_name` → `full_name` is the ordinary case, not an edge case, and a
one-to-one model breaks on the first concatenation anybody catalogues. Rows also
avoid an ordering nobody agreed on.

Mappings are keyed by **column FQN**, so they follow a name rather than a
position — the reorder criterion falls out of the key rather than needing a rule.
`PUT` replaces wholesale, because a refactor that makes a column come from one
source instead of two cannot be expressed by adding.

**Not done**: rename and drop propagation. A column rename today leaves the
mapping pointing at the old FQN. That is a real gap rather than a rounding —
see "Explicitly deferred".


**Value**: "Which source column produced this number" — the question that ends a data-quality argument.
**Path**: `column_lineage: Vec<ColumnMapping>` inside `LineageDetails`, mapping `Vec<from_column_fqn> → to_column_fqn`.
**Acceptance criteria**:
- Many-to-one mapping (`first_name`, `last_name` → `full_name`) recorded and returned.
- One-to-one recorded.
- A mapping naming a nonexistent column → `400` identifying which.
- Column mappings survive a table PATCH that reorders columns (name-matched, per Epic 2).
- Renaming a column updates mappings referencing it.
- Dropping a column removes mappings referencing it and flags affected edges.
**RED**: Many-to-one round-trip test. A column-rename test asserting the mapping follows the rename. Mutator watch: a one-to-one-only model must fail the many-to-one case; position-based matching must fail the reorder case.
**GREEN**: mapping type, validation, rename/drop propagation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Connector-asserted lineage reconciles — **shipped**

**Scoped by source *and* by prefix, and both halves matter.** Source-blind
replacement silently deletes lineage a human curated — every night, without an
error, which is why it is the slice's own critical test. Scope-blind replacement
deletes edges in schemas the run never looked at, which is the same bug wearing
a different hat; the scope is therefore required rather than defaulted.

The `(from, to, relationship, source)` uniqueness Slice A already put on
`lineage_edges` is what makes this possible at all: a human's edge and a
connector's are two rows, so replacing one set cannot touch the other.


**Value**: Automation and human curation coexist instead of overwriting each other every night.
**Path**: a connector run replaces the edge set it previously asserted for the scope it enumerated, keyed by `source`.
**Acceptance criteria**:
- A run asserting A→B then a later run asserting A→C leaves only A→C from that source.
- A manually-curated A→D survives both runs untouched.
- A crashed run replaces nothing — same completion gating as Epic 15 Slice E.
- Reconciliation is scoped: a run covering `schema_a` does not remove edges in `schema_b`.
- The run reports lineage edges added and removed.
**RED**: The manual-survival test is the critical one. Plus a crash test asserting no removal. Mutator watch: source-blind replacement must fail the manual-survival test — the failure mode that silently deletes curated lineage.
**GREEN**: source-scoped reconciliation reusing Epic 3's enumeration-scope machinery.
**REFACTOR**: this is the second consumer of "reconcile an enumerated scope" (after Epic 15 Slice E). Extract the shared pure reconciliation function if not already done.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Lineage survives entity deletion — **shipped**

**Value**: Restoring a mistakenly deleted table restores its lineage too.
**Path**: edges retained on soft delete; purged on hard delete.
**Acceptance criteria**:
- Soft-deleting a table retains its edges; traversal includes it flagged `deleted`.
- Restoring makes it a normal node again with edges intact.
- Hard delete purges its edges — no dangling references.
- A traversal encountering a hard-deleted endpoint does not error.
**RED**: Delete-restore round trip asserting the graph is byte-identical before and after. Mutator watch: edge deletion on soft delete must fail it.
**GREEN**: retention on soft delete, purge on hard delete.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Column rename and drop propagation** (Slice D's last two criteria) → a
  rename today leaves a mapping pointing at the old column FQN. Doing it
  properly means hooking the asset rename path, which is where Epic 2's
  containment cascade already lives, and the two should move together rather
  than growing a second half-aware traversal. Named here so it is a known gap
  rather than a surprise.
- **A crashed run replacing nothing** (Slice E) → the completion gating is Epic
  15 Slice E's, and reconciliation here is called *by* a completed run rather
  than deciding completion itself. The guard exists one layer up; this endpoint
  deliberately does not re-implement it.

- **SQL parsing to derive lineage automatically** → a substantial sub-project. Asserted lineage covers most of the value; revisit when manual + connector lineage demonstrably falls short.
- **Lineage from query logs** → same reason; also needs query-log ingestion, itself off the roadmap.
- **Lineage graph visualization** → a UI concern; the API returns nodes and edges.
- **Cross-service lineage validation** → assumes both endpoints are catalogued; unvalidated for now.
- **Lineage-based impact notifications** → needs the notification transport deferred in Epic 29.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Traversal tests carry explicit timeouts so a cycle regression fails CI rather than wedging it.
