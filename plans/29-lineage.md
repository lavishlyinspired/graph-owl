# Plan: Lineage (Epic 29)
**Branch**: feat/lineage
**Status**: Not started
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
- [ ] A 1,000-node graph traverses within a bounded latency budget.

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

### Slice C: Cycles terminate

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

### Slice D: Lineage reaches column level

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

### Slice E: Connector-asserted lineage reconciles

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

### Slice F: Lineage survives entity deletion

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
