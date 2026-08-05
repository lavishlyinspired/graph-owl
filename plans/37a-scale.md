# Plan: Scale Validation (Epic 37a)
**Branch**: feat/scale
**Status**: **Slice A shipped, 5 August 2026 — Slices B–F not started.** See the scope note at the top of "## Slices" for why this pass stops here.
**Depends on**: Epic 34 (a realistic entity mix to generate) — shipped
**Crates**: No new crates. Corpus generator in `graph-owl-cli` (shipped) — `graph_owl_cli::corpus`; benchmarks as workspace `benches/` (not yet); budgets asserted in CI (not yet)

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

- [x] A deterministic generator produces the corpus reproducibly.
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
- [ ] **The measurement above must report *write* latency, not only read latency** (added 28 Jul 2026). The partitioning trigger is usually described in terms of query speed, but the earlier cliff on this schema is on the **insert** side: every index here leads with `namespace_s, sid_s` or `namespace_p, sid_p`, so inserts **scatter across the keyspace** rather than appending to one hot rightmost page the way a time-ordered index does. Once the four indexes exceed `shared_buffers`, each insert becomes four random read/write pairs. A trigger tuned on read latency alone would fire late. Report insert throughput and p99 write latency at the same 1M / 5M / 10M points.
- [ ] **BRIN on `t` is evaluated against the four B-trees, and the result is reported either way.** `t` is monotonically increasing on an append-only table and every index already carries it as a trailing column — the textbook BRIN case, where a block-range index is on the order of a hundredth the size of the equivalent B-tree. It would not replace the four orderings, which serve selective point lookups where B-tree wins; the question is whether it serves the **time-range** access that time travel makes routine, at a fraction of the memory. Whether it helps depends on how well `t` correlates with physical row order — measurable, not arguable, and a negative result is as useful to record as a positive one.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first. Here the "RED" is a failing budget assertion.

**Scope of this pass, decided explicitly rather than discovered by running out of session:** Slice A (the corpus generator) is genuinely achievable, self-contained, and does not require anything this pass cannot honestly deliver. Slices B–F are a different category of work — five more benchmark dimensions (read, traversal, search, write/ingestion) each needing real CI wiring, plus Slice F's **mandatory 1-hour soak test** and budget numbers meant to be *sized* against real LUBM/BSBM/Wikidata downloads (see "Where the data comes from" below). Faking those numbers, or claiming a soak test "passed" without running one for a real hour, would violate this project's own "measured, not assumed" discipline (see `CLAUDE.md`'s build-loop section, and the identical lesson already recorded for Epic 98's sidecar timing). This pass ships Slice A only; Slices B–F are recorded here as **not started**, not silently dropped.

### Slice A: The corpus exists — **shipped, 5 August 2026**

**Value**: Everything else depends on it.
**Path**: `graph-owl generate-corpus --seed 1 --tables 1000 --out corpus.tar.zst`, packaged in the *identical* archive shape Epic 37b's `Catalog::export_archive` produces — so `graph-owl restore --in corpus.tar.zst` (already built, already tested in that epic) is the loader. No new bulk-insert endpoint was built; none was needed.
**Shipped**:
- Deterministic: `graph_owl_cli::corpus::generate(seed, target_tables)` is a pure function (no I/O) — the same seed always produces the same entity/relationship *shape* (names, FQNs, structure, soft-delete count); ids are freshly minted per call, so determinism is asserted on shape, not on raw bytes.
- Realistically shaped — power-law tables-per-schema **and** power-law lineage fan-out, both via the same rank-based `1/rank`, harmonic-normalized construction (simple, and genuinely a power law rather than approximately one). Asserted directly: the busiest schema holds more than twice a median schema's table count, checked against a fixture, not eyeballed.
- Includes soft-deleted entities (~2%), multiple versions per entity (1–3), and a deliberately injected cyclic lineage chain (A→B→C→A) — the three shapes decision 3 and this slice's own criteria name explicitly.
- Real end-to-end proof, not just a unit test: `graph-owl-cli/tests/corpus.rs` generates a corpus, packages it, and restores it into a real Postgres-backed catalog over real HTTP, asserting the restored entity/relationship counts match exactly.
**Scope cut, recorded rather than silently narrowed**:
- **"Extends Epic absorbed into 4's generator"** — that generator does not exist in the current codebase (checked before writing this one; see `[[epic-100-blocked-on-real-gaps]]`-style phantom-dependency pattern, same finding here). This generator is net new, not an extension.
- **Entities are services/databases/schemas/tables only** — no columns, users, teams, tags, or custom properties, unlike the full target table above. The structural properties Slices B–F would need (hierarchy depth, fan-out shape, version count, soft-delete, cycles) are all present; the additional entity kinds are a bounded, separable addition if a later slice's budget needs them specifically.
- **"Loads in under 10 minutes"/"loadable incrementally" are not independently verified at 100k-table scale.** The end-to-end test above proves the pipeline at 50 tables; nothing in this pass ran it at the plan's own 60,000-table target. `graph-owl restore` (Epic 37b) pages the *read* side at 500 rows/request during export, but a *generate-and-restore* run's wall-clock time at real scale is unmeasured — the honest position per this project's own "measured, not assumed" rule, not a claim either way.
- **No dedicated `benches/` workspace member.** Nothing yet asserts a budget in CI (that is Slices B–F's own job); this slice's tests assert *shape* (determinism, power-law, the named shapes) and *correctness* (the real restore), not *speed*.
**Tests**: `graph-owl-cli::corpus::tests` (6 tests: determinism, distribution, seed-difference, ordering, injected shapes, archive round-trip), `graph-owl-cli/tests/corpus.rs` (1 real end-to-end test against Postgres).

### Slices B–F: not started

Read-path budgets, traversal budgets, search budgets, write/ingestion budgets, and the sustained-load soak test. Each needs real CI wiring and, for B–E, real budget numbers; F additionally needs an actual 1-hour run. None of this is hard to *design* — the plan below already does — but none of it can be honestly marked done without either running it for real (the soak test genuinely takes an hour; the LUBM/BSBM/Wikidata sizing genuinely needs the downloads) or building CI infrastructure changes that deserve their own review rather than arriving as a side effect of a generator slice. Left as originally written below, unstarted.

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

## Where the data comes from (added 30 July 2026)

**Generated benchmarks for the numbers that need to be dialled**, real dumps for
the distributions that cannot be invented:

- **LUBM** and **BSBM** — parameterised RDF generators, so scale is a knob
  rather than a download. These are what "10M flakes" should mean here.
- **Wikidata** or **DBpedia** dumps — real degree distributions and real
  vocabulary sprawl, which no generator reproduces. Use these to *size* budgets,
  not to discover bottlenecks.

**And a distinction worth keeping.** Tabular dataset sources (Kaggle and
similar) are the wrong shape for this: they give volume without graph structure,
and the costs this epic measures are structural — path length, branching factor,
degree skew.

**Synthetic still wins for isolating a variable.** Epic 103's entry condition is
whether traversal cost grows with *depth*; a real graph confounds depth with
branching factor and degree distribution, so it would show that something is
slow without showing which term dominates. Generate for the gate, download for
the budget.

