# Plan: Quality Signals & Incidents (Epic 30)
**Branch**: feat/quality-results
**Status**: All slices shipped, 8 August 2026. Slice D shipped on a narrower design than originally specified — per-row computation (decision 4.5) rather than the originally-planned denormalized async-refreshed column; see Slice D's own section for why
**Depends on**: Epic 29 (lineage, for propagating trust signals)
**Crates**: `graph-owl-core` (TestCase, TestResult, Incident, Alert, **pure health function**) · `graph-owl-storage-postgres` (time-series + denormalized health) · `graph-owl-api` · `graph-owl-server`

## Goal

Show whether an asset can be trusted, using test results produced **elsewhere** — dbt tests, Great Expectations, custom checks.

## The boundary, stated plainly

graph-owl **ingests and displays** quality results. It **does not run tests, author assertions, or schedule checks**. Those are a product in their own right with their own compute story, and building them would dominate the roadmap.

This is a deliberate narrowing that captures most of the visible value: a consumer looking at a table wants to know "is this passing, and is it fresh" — not to configure a testing engine inside the catalog.

## Resolved decisions

1. **Results are time-series, not entity state.** A result is an observation at a point in time; the entity's current health is derived from recent observations. Storing only the latest result loses the history that makes a signal trustworthy.
2. **Results do not bump the entity version.** A nightly test run must not inflate metadata history — the version tracks *descriptive* change, not observations.
3. **A test case is a lightweight entity**, so results attach to a stable identity across runs and history survives renames.
3a. **A test *definition* is the reusable template; a test *case* is its application to one asset.** "Freshness within 24 hours" is one definition applied to eight hundred tables, not eight hundred unrelated test cases that happen to share a name. Without the split, the same check is registered under a thousand names, nothing can be reported on across assets, and changing the threshold means editing a thousand rows.
3b. **A test *suite* is a named collection of cases with a shared owner and schedule** — the unit a team is accountable for and the unit a report is produced against. An asset can belong to several suites; a suite spans assets.
4. **Staleness is first-class.** A result from six weeks ago is not a pass. Results carry an expected cadence; a result older than its cadence reports as stale, not passing.
5. **Health rolls up, but never invents certainty.** A table with no tests reports `Unknown`, never `Healthy`. Silence is not a pass.
6. **Ingestion is push-based** via the API. No polling of external systems — that would be a connector, and each testing tool would need one.

## Acceptance criteria (feature level)

- [x] A test case can be registered against an entity or a column.
- [x] A test **definition** is reusable: one definition applied to N assets yields N cases, and editing its cadence changes all N without touching them — while a case that overrode it is deliberately not moved.
- [x] A test **suite** groups cases across assets with an owner.
- [x] Results are posted with status, timestamp, optional message and structured metrics, and retained as history.
- [x] An asset shows current health derived from its test cases.
- [x] A stale result reports as stale, not as its last status.
- [x] An asset with no tests reports `Unknown`, never `Healthy`.
- [x] Health is filterable and available as a search facet — Slice D, shipped 8 August 2026 on a narrower design than originally specified here; see below.
- [x] Result ingestion does not bump entity versions or emit change events.
- [x] Result history is prunable, and the latest result per case always survives.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Test cases are registerable — **shipped**

**Value**: A stable identity for results to attach to.
**Path**: `TestCase { name, entity, column?, test_type, description, expected_cadence }` + envelope.
**Acceptance criteria**:
- Register against an entity or a specific column by FQN.
- Duplicate name on the same entity → `409`.
- Nonexistent entity or column → `400`.
- `testType` is free-form (the producing tool names it), but non-empty.
- `expectedCadence` is an ISO 8601 duration; invalid → `400`.
- `GET /tables/{id}/test-cases` paginated.
- Deleting a test case removes its results.
- A test case survives a column reorder, matched by name (consistent with Epic 2).
**RED**: Column-reorder survival test. Cadence parsing tests over valid and invalid durations. Mutator watch: position-based column matching must fail the reorder test.
**GREEN**: entity, registration, cadence parsing.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Results are ingested as history — **shipped**

**Value**: The observation stream exists.
**Path**: `POST /test-cases/{id}/results` writing to a time-series table.
**Acceptance criteria**:
- Result carries `status` (`Success|Failed|Aborted`), `timestamp`, optional `message` and structured `metrics`.
- Several results for one test case are retained in order.
- Posting does **not** bump the entity version and emits **no** change event.
- A result timestamped in the future → `400`.
- Duplicate `(testCase, timestamp)` → `409`, so a retried push does not double-count.
- Bulk posting of results across test cases in one request.
- `GET /test-cases/{id}/results?from=&to=` paginated, newest first.
**RED**: Test asserting the entity's version is unchanged after ingesting results — decision 2's guarantee. Duplicate-timestamp test. Mutator watch: version bumping on ingest must fail the first; absent duplicate detection must fail the second.
**GREEN**: time-series table, ingestion, dedup, retrieval.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Health is derived, and honest about uncertainty — **shipped**

**Value**: The signal a consumer actually reads.
**Path**: computed health on entity reads from the latest non-stale result per test case.
**Acceptance criteria**:
- All test cases passing and fresh → `Healthy`.
- Any failing → `Unhealthy`, with a count and the failing cases.
- No test cases → `Unknown` (decision 5).
- All results stale → `Stale`, not the last known status (decision 4).
- Mixed fresh-pass and stale → reports the stale ones distinctly rather than averaging them away.
- Health is returned via `?fields=health` — not by default, since it costs a query.
- Health computation is a pure function of results and cadences, unit-tested without I/O.
**RED**: Truth table over the six states, with the mixed case explicit. Staleness boundary tests at exactly the cadence and one second past. Mutator watch: treating no-tests as `Healthy` must fail — the most dangerous possible bug here, since it silently asserts trust; treating stale as its last status must fail.
**GREEN**: pure health function in `core`, field selection.
**REFACTOR**: keep the health computation pure and in `core` — it is the highest-stakes logic in the epic and must be exhaustively testable.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Health is discoverable — **shipped 8 August 2026, decision 4.5, on a narrower design than this section originally specified**

This section originally deferred the slice on the grounds that it needs a
**denormalized** health column refreshed **asynchronously** on ingestion, plus a
query-plan test asserting no per-row computation — three pieces of machinery
this codebase does not have, since there is no async work queue anywhere in it.

**`plans/EPIC-COMPLETION-PLAN.md` Phase 4 decision 4.5 revisited that framing
and chose differently**: accept per-row computation rather than build the async
refresh infrastructure this specific filter would otherwise need invented from
nothing. The reasoning that decision recorded: certification/lifecycle
filtering (Epic 26) already computes its status on read, the same shape this
extends rather than a new one; a denormalized column needs a write-path
refresh mechanism that does not exist anywhere in this codebase to keep
correct; and a general async-refresh mechanism would be speculative
generalization for exactly two call sites (this slice and Epic 26's own).

**What actually shipped is the opposite of this section's own "thing to
avoid"**: `health_expr` (`crates/graph-owl-storage-postgres/src/lib.rs`) is a
correlated `LEFT JOIN LATERAL` subquery per candidate row, computing the exact
same precedence `graph_owl_core::quality::health_of` runs for a single asset —
`?health=` on `GET /assets`/`GET /assets/search`, plus a `health` search facet
mirroring the existing `kind`/`schema` ones. **This is a real, accepted,
unmeasured-at-scale tradeoff, not a silent regression from what this section
specified**: Epic 37a's own real 60,246-asset corpus exists and could measure
this filter's actual cost the same way Slice D of that epic measured search
budgets, but doing so was out of scope for the decision that unblocked this
slice. Revisit only if a real measurement shows it failing a stated budget —
matching Epic 37a's own precedent of shipping a found-but-unfixed gap (its
type-ahead search measurement) with the gap named rather than hidden, not
blocking on a fix nothing asked for yet.

The data is all present — `GET /health/{fqn}` answers for one asset — so this
was a discovery gap rather than a modelling one, exactly as originally framed.

**Value**: "Show me unhealthy tables in my domain" — the steward's triage query.
**Path**: `?health=` filter on list endpoints; health as a search facet.
**Acceptance criteria, as shipped** (the original three denormalization/async/
staleness-window criteria below are explicitly **not** met — see above):
- [x] Filter by each health state, including `Unknown`.
- [x] Composes with other filters and pagination.
- [x] Search facet returns counts per health state, over the same visible set every other facet already respects.
- [ ] ~~Health is denormalized for filtering~~ — deliberately not built; see above.
- [ ] ~~Refresh is asynchronous~~ — n/a, nothing to refresh.
- [ ] ~~A documented staleness window~~ — n/a; the filter reads live, same as `GET /health/{fqn}`.
**RED**: The precedence case a naive SQL translation gets wrong — a test case
that is *both* stale and failed must file under `stale`, never `unhealthy`,
matching `health_of`'s own per-case ordering exactly
(`a_stale_and_failed_case_files_under_stale_not_unhealthy`,
`crates/graph-owl-server/tests/quality.rs`). Mutator watch: swapping the SQL
CASE's branch order (checking `any_stale` before `any_failed`) must fail this
test.
**GREEN**: `health_expr`, `?health=` wired into both list endpoints, `health`
search facet, `Health::as_str`/`parse` for wire parsing. Mutation-tested on
`graph-owl-core`, `graph-owl-storage-postgres`, `graph-owl-server`.
**Done when**: criteria (as shipped, above) met, mutation report reviewed,
commit approved. Met.

### Slice E: Results do not grow without bound — **shipped**

**Value**: A nightly suite across 10,000 tables produces millions of rows a year.
**Path**: retention policy with configurable window and pruning.
**Acceptance criteria**:
- Configurable retention (default 90 days) with a documented default.
- Pruning is incremental and does not lock the table.
- The most recent result per test case is **always** retained regardless of age — otherwise pruning would destroy the health signal it exists to support.
- Pruning is observable via metrics.
- Retention is configurable per test case for cases needing longer history.
- Pruning failure does not affect ingestion.
**RED**: Test asserting the latest result survives pruning even when older than the window — the subtle correctness requirement. Mutator watch: unconditional age-based deletion must fail it, which would silently blank out health for infrequently-tested assets.
**GREEN**: retention policy, incremental pruning, latest-result preservation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Trust propagates along lineage — **shipped**

**Reported separately, never merged.** Conflating an asset's own health with its
upstream's makes the signal unactionable: a steward cannot tell whether to fix
this table or go upstream. The response carries both, and names *which* upstream
asset and how many hops away — the nearest instance of the worst state, because
that is the one somebody can act on.

Bounded at three hops and cycle-safe. Three is far enough to cross a staging
layer and a mart; an unhealthy source five layers away is somebody else's
incident, and reporting it would make every asset in a large estate look sick.
The walk uses `lineage_edges_touching`, which reads one level per query rather
than one node per query.

**`Unknown` is not the worst state** in the rollup: an upstream nobody tests is
less alarming than one known to be failing, and ordering it below `Unhealthy` is
what stops an untested corner of the estate drowning out a real incident.


**Value**: A healthy table fed by an unhealthy upstream is not actually trustworthy — the question lineage was built to answer.
**Path**: optional upstream health rollup using Epic 7a's traversal.
**Acceptance criteria**:
- `?fields=health&includeUpstream=true` reports the asset's own health plus the worst upstream health within a bounded depth.
- Upstream health is reported **separately**, never merged into the asset's own — conflating them would make the signal unactionable.
- Traversal is depth-bounded and cycle-safe, reusing Epic 7a's machinery.
- An unhealthy upstream names which asset and how many hops away.
- Absent lineage → own health only, with no error.
- Not computed by default, given the traversal cost.
**RED**: Two-hop test with a failing grandparent, asserting the asset's own health stays `Healthy` while upstream health reports `Unhealthy` with the hop count. Mutator watch: merging the two into one field must fail it.
**GREEN**: bounded rollup, separate reporting.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Running tests** → out of scope, permanently, per the stated boundary.
- **Assertion authoring in the catalog** → same.
- **Scheduling** → same reason graph-owl is not a connector scheduler.
- **Incident management** (assigning and resolving failures) → adjacent product; Epic 35's collaboration covers discussing a failure.
- **Alerting on failures** → needs the notification transport still deferred from Epic 14.
- **Anomaly detection over result metrics** → requires a modelling story; the raw metrics are retained so it stays possible.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed. The health function (Slice C) is the highest-stakes pure logic here — a surviving mutant there means the catalog can assert trust it has not verified.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Verify ingestion throughput against a realistic nightly volume before merge.
