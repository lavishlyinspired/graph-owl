# Plan: Scale Validation (Epic 37a)
**Branch**: feat/scale
**Status**: Not started
**Depends on**: Epic 34 (a realistic entity mix to generate)
**Crates**: No new crates. Corpus generator in `graph-owl-cli`; benchmarks as workspace `benches/`; budgets asserted in CI

## Goal

Prove the system behaves at 100,000 entities, and make a regression fail the build rather than surface in production.

## Why this is an epic, not a task

Every prior epic asserts correctness. None asserts that correctness survives volume. Catalogs degrade in specific, predictable ways — a list endpoint that joins per row, a lineage traversal without a depth guard, a search facet computed over an unfiltered corpus. Each is invisible on ten entities and fatal on a hundred thousand.

The deliverable is not a one-off benchmark report. It is a **corpus generator plus asserted budgets running in CI**, so performance becomes a property the build defends rather than a thing someone measures occasionally.

## Resolved decisions

1. **Budgets are asserted, not reported.** A benchmark nobody fails is a benchmark nobody reads.
2. **The corpus is generated, not fixtured.** 100k entities cannot live in git. A seeded deterministic generator produces the same corpus every run.
3. **The corpus is realistically shaped** — power-law distributions for tables per schema and lineage fan-out, not uniform ones. Uniform data hides exactly the hot-spot problems this epic exists to find.
4. **Single-instance targets.** Horizontal scaling and sharding are out of scope; the goal is proving one instance handles a realistic organization.
5. **Budgets are revised deliberately with the reason recorded** — never silently raised to make a build pass.
6. **The generator is shared with Epic absorbed into 4's time-travel benchmarks**, built reusably there and extended here.

7. **The scaling path is not swapping the storage engine, and this epic is what
   proves it.** The reflex when a graph query is slow is to reach for a graph
   database. That is the wrong first move here for a reason this epic can
   *measure* rather than assert: at catalog scale the cost is almost never the
   storage engine — it is an unbounded traversal, a missing index, a per-row
   join, or a query plan nobody looked at. Each of those is fixable in place,
   and each is cheaper than an operational dependency.

   Concretely, the ladder is: **fix the query** (bounds, indexes, plans — the
   acceptance criteria below) → **partition the flake table** (the trigger this
   epic measures) → **add SOP/OSP orderings** if a real access pattern needs
   them → **extract the subgraph in-process** (Epic 103) when the walk is deep
   rather than the graph large. A second datastore sits below all four.

8. **The one exception is depth, and it is answered in-process.** A recursive
   CTE degrades on deep walks for a specific, verified reason: the per-path
   cycle guard is `NOT dst = ANY(path)`, a linear array scan per candidate row.
   That is a real ceiling and not a tuning problem. It is also not a reason to
   run a second server — Epic 103 loads a bounded, already-authorized subgraph
   into memory where the same test is a hash lookup.

   **This epic sets the threshold.** Epic 103's routing rule needs a measured
   crossover between CTE cost and extraction cost; without it, "deep queries go
   in-process" is a guess with a plan attached. Measuring that crossover is an
   acceptance criterion here.


## Targets

The corpus, approximating a large organization:

| Dimension | Target |
|---|---|
| Entities (all types) | 100,000 |
| Tables | 60,000 |
| Columns | 1,200,000 (~20/table) |
| Relationships | 250,000 |
| Lineage edges | 80,000 |
| Version history | ~5 versions/entity → 500,000 |
| Users / teams | 2,000 / 200 |
| Tags applied | 300,000 |

## Acceptance criteria (feature level)

- [ ] A deterministic generator produces the corpus reproducibly.
- [ ] Every budget below is asserted in CI and fails the build on regression.
- [ ] Query plans are reviewed for every endpoint; no sequential scan on a hot path.
- [ ] A soak test shows no memory growth or connection leak over sustained load.
- [ ] Connector throughput is measured against a 10,000-table source.
- [ ] Results are recorded so trends are visible across releases.
- [ ] **Traversal depth is swept, not sampled** — walk latency reported at
      depths 1 through 8 on the power-law corpus, which is where the cycle
      guard's cost shows up. A single mid-depth number would hide the curve
      that decides Epic 103's routing threshold.
- [ ] The **flake-table partitioning trigger is measured, not assumed**: index-only-scan ratio, index size against `shared_buffers`, and p99 pattern-query latency are reported at 1M / 5M / 10M flakes. Epic 4 says "start unpartitioned, partition by `namespace_s` at ~10M"; this epic is what turns that number from a guess into a measurement, and is licensed to move it.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first. Here the "RED" is a failing budget assertion.

### Slice A: The corpus exists

**Value**: Everything else depends on it.
**Path**: a seeded generator producing the entity mix above, loadable in reasonable time.
**Acceptance criteria**:
- Deterministic: the same seed yields an identical corpus.
- Realistically shaped — power-law tables-per-schema and lineage fan-out (decision 3).
- Loads in under 10 minutes so CI can use it.
- Loadable incrementally so a test can request a subset.
- Includes soft-deleted entities, several versions per entity, and cyclic lineage — the shapes that break naive implementations.
- Extends Epic absorbed into 4's generator rather than duplicating it.
**RED**: Determinism test asserting two runs with one seed produce identical checksums. Distribution test asserting the shape is power-law, not uniform. Mutator watch: a uniform generator must fail the distribution assertion — otherwise the whole epic tests the wrong thing.
**GREEN**: generator, distributions, bulk loading.
**Done when**: criteria met, corpus reproducible, commit approved.

### Slice B: Read paths meet budget

**Value**: The endpoints every user hits stay fast.
**Path**: benchmark harness over list, get, and filter endpoints against the corpus.
**Acceptance criteria** (p95, warm cache, single instance):

| Operation | Budget |
|---|---|
| `GET /tables/{id}` | < 20 ms |
| `GET /tables/{id}?fields=columns,owners,tags` | < 60 ms |
| `GET /tables` page of 25 | < 100 ms |
| `GET /tables` page 1000 (deep cursor) | < 120 ms |
| Filter by owner, tag, or domain | < 150 ms |
| `GET /tables/name/{fqn}` | < 20 ms |

- Deep pagination does **not** degrade with offset — the property cursor pagination was chosen for.
- Query plans reviewed; no sequential scan.
- Budgets asserted in CI.
**RED**: Benchmarks asserting each budget, plus a plan assertion per query. The deep-cursor test is the important one — it catches an accidental regression to offset semantics. Mutator watch: an offset-based implementation must fail the page-1000 budget.
**GREEN**: index tuning, query fixes, CI assertions.
**Done when**: every budget green, plans reviewed, commit approved.

### Slice C: Traversal meets budget

**Value**: Lineage and hierarchy operations are where catalogs most often fall over.
**Path**: benchmarks over lineage traversal, FQN cascade, and ownership inheritance.
**Acceptance criteria** (p95):

| Operation | Budget |
|---|---|
| Lineage depth 3, moderate fan-out | < 200 ms |
| Lineage depth 3, high fan-out (p99 node) | < 800 ms |
| Lineage on a cyclic subgraph | terminates < 1 s |
| FQN cascade renaming a database with 5,000 descendants | < 5 s, atomic |
| Ownership inheritance resolution in a list page | < 150 ms |

- The high-fan-out case uses the corpus's genuinely worst node, not an average one.
- Cascade remains transactional at this size — it does not exceed statement or lock timeouts.
- The node budget and `truncated` flag engage correctly rather than the query simply running long.
**RED**: Benchmarks against the worst-case nodes the generator produced. A cascade test at 5,000 descendants asserting atomicity *and* the time budget. Mutator watch: a removed node budget must fail the high-fan-out case.
**GREEN**: index tuning, traversal optimization, budget enforcement.
**Done when**: every budget green, commit approved.

### Slice D: Search meets budget

**Value**: The headline capability must stay fast on a full index.
**Path**: benchmarks over the fully indexed corpus.
**Acceptance criteria** (p95):

| Operation | Budget |
|---|---|
| Simple term query | < 100 ms |
| Query with 3 facets | < 200 ms |
| Type-ahead suggestion | < 50 ms |
| Full reindex of 100k entities | < 15 min |
| Index lag after a write | < 1 s |

- Facet counts stay correct at volume.
- Reindex holds the zero-downtime property at this size.
- Index size on disk is recorded so growth is visible.
- Authorization filtering (Epic 8 — `08-engine-search.md`) is included in the measured queries — an unfiltered benchmark measures the wrong thing.
**RED**: Benchmarks with authz filtering active. A reindex test asserting continuous availability throughout. Mutator watch: benchmarking without the authz predicate must be caught by asserting the predicate is present in the executed query.
**GREEN**: index tuning, query optimization.
**Done when**: every budget green, commit approved.

### Slice E: Write and ingestion paths meet budget

**Value**: Connectors must keep up with a large source.
**Path**: benchmarks over single writes, bulk writes, and a full connector run.
**Acceptance criteria**:

| Operation | Budget |
|---|---|
| Single entity create | < 50 ms p95 |
| Bulk create, 1000 entities | < 5 s |
| Connector run, 10,000 tables (first) | < 10 min |
| Connector re-run, unchanged | < 3 min, zero versions |
| Version history write overhead | < 20% of base write |

- The unchanged re-run genuinely produces zero versions at scale, not merely few — Epic 15's convergence property under load.
- Bulk writes do not hold long transactions that block reads.
- Connector memory stays bounded — streaming, not accumulating (Epic 15 Slice A's property, verified at volume).
**RED**: A connector-run benchmark asserting bounded memory, not just elapsed time. The zero-version re-run assertion at 10k tables. Mutator watch: an accumulating (non-streaming) source must fail the memory bound.
**GREEN**: batching, transaction scoping, streaming verification.
**Done when**: every budget green, commit approved.

### Slice F: Sustained load is stable

**Value**: Catches leaks and unbounded growth that point benchmarks miss entirely.
**Path**: a soak test running mixed load for an extended period.
**Acceptance criteria**:
- 1 hour of mixed read/write/search load at moderate concurrency.
- RSS growth under 10% from steady state to end.
- No connection-pool leak — pool returns to baseline.
- No unbounded table growth beyond what the workload justifies.
- Latency at the end within 10% of the start — no gradual degradation.
- Runs nightly rather than per-PR, given its duration.
- Failure produces an actionable artifact: heap snapshot or pool statistics, not just a red build.
**RED**: The soak harness with assertions on each property. A deliberately-leaking branch must fail it — a soak test that cannot detect a leak is theatre. Mutator watch: verified by the deliberate-leak branch.
**GREEN**: soak harness, resource assertions, nightly CI job.
**Done when**: criteria met, deliberate-leak branch fails the soak, commit approved.

## Explicitly deferred (with destination)

- **Horizontal scaling / sharding** → single-instance targets first, per decision 4. Revisit if one instance provably cannot serve a real organization.
- **Read replicas** → an operational deployment choice; the application is already read-heavy and replica-friendly.
- **Caching layer** → adding one before measuring would be premature; this epic produces the measurements that would justify it.
- **Multi-region latency** → depends on multi-tenancy and deployment topology, both off the roadmap.
- **Load testing the CLI (Epic 20)** → apply against a 100k-entity declared state is worth measuring; add if metadata-as-code adoption reaches that size.

## Pre-PR quality gate

1. `cargo mutants` on changed application code — 0 missed.
2. Refactoring assessment on any optimization made — a fast implementation that nobody can read is a poor trade at this stage.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Every budget green in CI.
5. Any budget revision recorded with its reason in this plan — never silently raised.
