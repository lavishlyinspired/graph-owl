# Plan: Source Connectors (Epic 15)
**Branch**: feat/connectors
**Status**: Not started
**Depends on**: Epic 2 (hierarchy to populate), Epic 3 (versioning to make re-runs observable)
**Unblocks**: Epic 29 (connector-asserted lineage)
**Crates**: **`graph-owl-connectors`** (new — `Connector` trait, run machinery, and the Rust Postgres reference connector; **not** a module per source — see decision 1) · `graph-owl-core` (SourceRecord) · `graph-owl-api` (bulk upsert) · `graph-owl-server` (bulk endpoints, run history)

## Goal

Make the catalog populate itself. Hand-registering tables via curl does not survive contact with a real warehouse.

## Resolved decisions

1. **The `Connector` trait and the run machinery are Rust, in the binary; connectors beyond the Postgres reference are Python, out of process.** This reverses an earlier decision that put every connector in one Rust crate as a feature-gated module. Reasoning in `00j-language-boundaries.md`:

   - Warehouse, BI, orchestration, and SaaS metadata APIs have mature, maintained Python clients and often no Rust equivalent. Writing a connector should be an afternoon, not a week reimplementing someone else's HTTP client.
   - A connector is the most likely thing an outside contributor writes, and the data-engineering population writes Python.
   - **A connector is I/O against someone else's flaky API. It should fail as a *job*, never as a fault inside the process holding the graph.**
   - A source changes its API; the connector must ship without rebuilding and redeploying the engine.

   What stays Rust is the part that is a **governance** concern rather than an I/O one: run scheduling, scope filters, run history, identity, and above all deletion detection — decision 4 calls that the sharpest edge in this epic, and getting it wrong tombstones a live catalog.

   **The operational-simplicity budget survives**: a deployment cataloguing only Postgres still runs one binary. A deployment wanting Snowflake also runs a Python worker — a cost it opted into, not one imposed on everyone.
1a. **The Postgres connector stays Rust and in the binary**, as the reference implementation that proves the trait and needs no second runtime.
2. **Source → Processor → Sink pipeline.** The source yields raw records, processors enrich or filter, the sink writes to the catalog. Each stage is independently testable; the sink is the only stage that talks to graph-owl.
3. **FQN-keyed idempotent upsert via `PUT`.** A re-run against an unchanged source must produce zero new versions. Epic 3's no-op-produces-no-version rule is what makes this observable and therefore testable.
4. **Deletion detection is opt-in per run**, and is the sharpest edge in this epic. A connector must distinguish "the table is gone" from "the table was filtered out of this run" from "this run crashed halfway". Getting it wrong tombstones a live catalog.
5. **graph-owl does not become a scheduler.** Runs are triggered by an API call or external cron. Scheduling is a solved problem owned by other software.
6. **Connectors authenticate as bot users** (Epic 11's `is_bot`), so `updated_by` attributes changes to a named connector rather than to `system`.
7. **Every source record carries a `source_hash`, and an unchanged hash short-circuits the write.** Decision 3 makes a re-run *converge*; it does not make it *cheap* — the catalog still receives, validates, and diffs every record before deciding nothing changed. A hash computed at the source turns the second run into "read, compare, skip" for the unchanged majority. Three outcomes, decided before the network call: **create** if the FQN is unknown, **patch** if the hash differs, **skip** if it matches.
8. **Connector configuration is a JSON Schema, not a struct with a doc comment.** Each module publishes a schema for its connection parameters. This is what lets Epic 41's admin UI generate a working configuration form per connector without a UI change per connector, and it gives validation, defaults, and documentation from one declaration. Hand-written config forms across 100+ connectors is the largest avoidable cost in the whole connector programme.
9. **Traversal order is declared, not coded.** A connector states its entity topology — service → database → schema → table → column — as data, and a shared runner walks it depth-first, guaranteeing parents exist before children. Every connector otherwise re-implements the same ordering logic, and the one that gets it subtly wrong writes orphans that only surface as broken FQNs weeks later. Declaring it also makes the order *inspectable*, which is what lets a run report progress per level instead of as one opaque counter.
10. **Connectors are discovered from a registry, not from a match statement.** A module registers itself with its type name, its schema, and its constructor; adding a connector touches its own directory and nothing else. A central `match` over connector names is a merge-conflict magnet and the reason "add a connector" stops being a self-service task.

## Implementation reference

```rust
// graph-owl-connectors — one trait, one module per source, all feature-gated
#[async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> &ConnectorDescriptor;
    async fn test_connection(&self) -> Result<(), ConnectorError>;
    async fn fetch(&self, scope: &RunScope) -> Result<RecordStream, ConnectorError>;
}

pub struct ConnectorDescriptor {
    pub type_name: &'static str,        // "postgres", "kafka", …
    pub config_schema: &'static str,    // JSON Schema — decision 8
    pub capabilities: Capabilities,     // lineage? usage? profiling? deletion detection?
}

pub struct SourceRecord {
    pub fqn: String,
    pub source_hash: [u8; 32],          // decision 7 — over the source-owned fields only
    pub payload: EntityPayload,
}

pub enum SyncAction { Create, Patch, Skip }

// decision 9 — the traversal order is data the runner walks, not control flow per connector
pub struct TopologyNode {
    pub entity_type: &'static str,
    pub produces: fn(&Context) -> BoxStream<SourceRecord>,
    pub children: &'static [TopologyNode],
}
```

**The hash covers source-owned fields only.** A hash over the whole record changes whenever a human edits a description in the catalog, which makes every hand-curated entity permanently "changed" and defeats the purpose. `Capabilities` exists so the catalog never asks a connector for something it cannot do — a source with no deletion-detection capability must not silently appear to report zero deletions.

## Acceptance criteria (feature level)

- [ ] Pointing the connector at a Postgres instance populates services, databases, schemas, tables, and columns.
- [ ] A second run against an unchanged source produces zero new versions.
- [ ] A changed column type produces exactly one Major version bump on that table.
- [ ] A table dropped at the source is tombstoned, not orphaned.
- [ ] A crashed run does not tombstone anything.
- [ ] Include/exclude filters scope a run by schema and table name.
- [ ] Run history reports counts and per-entity failures.
- [ ] A 10,000-table source completes without exhausting memory.
- [ ] An unchanged record is **skipped before the write path**, not written and diffed — asserted by counting writes, not by observing the outcome.
- [ ] Every connector publishes a JSON Schema for its configuration, and an invalid configuration is rejected at registration with the schema path that failed.
- [ ] Adding a connector module requires **no edit to any shared file** — asserted structurally.
- [ ] Parent-before-child ordering is guaranteed by the shared runner, not by each connector — asserted by a topology whose children are declared before their parents, which must still write in the correct order.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: A Postgres source yields its structure

**Value**: The introspection half, provable in isolation against a real database and with no catalog involved.
**Path**: `graph-owl-connectors` crate; `Connector` trait; `postgres` module querying `information_schema`, yielding a stream of `SourceRecord`s.
**Acceptance criteria**:
- Against a seeded Postgres: yields databases, schemas, tables, and columns with name, data type, nullability, and ordinal position.
- System schemas (`pg_catalog`, `information_schema`) excluded by default.
- Views are yielded and marked distinctly from tables.
- Records stream rather than accumulating — a 10,000-table source does not build a 10,000-element vector.
- Connection failure is a typed error naming the failure, not a panic.
**RED**: Repository-style test against a testcontainer seeded with two schemas, four tables, mixed types, one view. Assert types map correctly and system schemas are absent. Mutator watch: an inclusive system-schema filter must fail; a filter excluding everything must fail the positive assertions.
**GREEN**: crate, trait, introspection queries, streaming iterator.
**REFACTOR**: assess the `Connector` trait's shape now, with one implementation — it is easier to change before the second connector exists. The trait must not leak `sqlx` types.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Source records become catalog entities

**Value**: The end-to-end path lands — a real database appears in the catalog without anyone typing curl.
**Path**: sink stage translating `SourceRecord`s into `PUT` upserts keyed by FQN, parents before children.
**Acceptance criteria**:
- Running against a seeded Postgres creates the full four-level hierarchy plus columns.
- Parents are created before children — no orphan is ever written.
- FQNs match Epic 2's derivation exactly.
- A run against an empty database succeeds, creating only the service.
- Per-entity failure does not abort the run; it is recorded and the run continues.
**RED**: End-to-end test: seed Postgres, run connector, assert via the HTTP API that the hierarchy exists with correct FQNs. Mutator watch: child-before-parent ordering must fail; abort-on-first-error must fail the continue-on-failure assertion.
**GREEN**: sink, ordering, per-record error isolation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Re-runs converge

**Value**: The property that makes scheduled runs safe. Without it, a nightly connector inflates version history to uselessness.
**Path**: FQN-keyed `PUT` upsert relying on Epic 3's no-op detection.
**Acceptance criteria**:
- Second run against an unchanged source: zero new versions on any entity, zero change events.
- Changing one column's type: exactly one entity's version bumps, Major.
- Adding a table: one new entity, others untouched.
- Editing a description in the catalog by hand, then re-running: the hand-edited description is **not** clobbered by the source's null description.
- Run reports created/updated/unchanged counts.
**RED**: The hand-edit preservation test is the important one — a connector that overwrites human curation with source nulls destroys the catalog's value. Assert the description survives. Mutator watch: blanket overwrite must fail it; treating every field as changed must fail the zero-new-versions assertion.
**GREEN**: upsert semantics that merge rather than replace, skipping null source fields.
**REFACTOR**: the merge rule (source wins for structural fields, catalog wins for curated fields) is domain knowledge. Assess placing it in the facade rather than the connector, so every writer obeys it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C2: Fingerprinting makes convergence cheap

**Value**: Slice C made a re-run *correct*; this makes it *affordable*. On a 10,000-table source where twelve tables changed overnight, the difference is 12 writes versus 10,000.
**Path**: `source_hash` computed by the connector over source-owned fields; stored per entity; compared before the write.
**Acceptance criteria**:
- An unchanged record produces `SyncAction::Skip` and **zero** write-path calls — asserted by a counting fake, not by checking the version number afterwards.
- A record whose source-owned fields changed produces `Patch`.
- An unknown FQN produces `Create`.
- **A hand-edited description in the catalog does not change the hash**, so hand-curated entities are not permanently marked changed.
- A connector that does not compute a hash degrades to Slice C behaviour rather than skipping everything.
- The hash algorithm and the field set that feeds it are recorded with the entity, so changing either invalidates old hashes rather than silently comparing incomparable values.
**RED**: The counting-fake test. Asserting "no new version" passes whether the write was skipped or performed-and-diffed, so it cannot distinguish the fix from the bug it is meant to prove — only counting the calls can. Second RED: the hand-edit test, because a hash over catalog state rather than source state makes every curated entity a permanent false positive. Mutator watch: hashing the whole record must fail the hand-edit test; a missing-hash path that skips rather than falls back must fail the degradation test.
**GREEN**: hash computation, storage, comparison, three-way action.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Runs are scoped by filters

**Value**: A connector can target one schema instead of an entire warehouse.
**Path**: include/exclude regex patterns on schema and table names, applied in the source stage.
**Acceptance criteria**:
- `includeSchemas: ["public"]` yields only that schema.
- `excludeTables: ["^tmp_"]` skips matching tables.
- Exclude wins over include when both match.
- An invalid regex → `400` at configuration time, not mid-run.
- Filtered-out entities are **not** tombstoned — this is the distinction Slice E depends on.
- The run reports how many entities were filtered.
**RED**: Test asserting a previously-catalogued table that is now filtered out is left untouched, not tombstoned. Mutator watch: treating filtered-out as absent must fail it — this is the bug that silently deletes half a catalog.
**GREEN**: filters, explicit filtered-vs-absent distinction carried through the pipeline.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Deletions at the source are detected

**Value**: The catalog stops accumulating tables that no longer exist. The sharpest edge in the epic.
**Path**: a run records the set of FQNs it *fully enumerated*; entities under an enumerated scope that were absent from the run are tombstoned — but only if the run completed successfully.
**Acceptance criteria**:
- A table dropped at the source is tombstoned after a successful full run.
- A table filtered out is **not** tombstoned.
- A **crashed** run tombstones nothing.
- A run that enumerated only `schema_a` does not tombstone anything in `schema_b`.
- Deletion detection is opt-in (`detectDeletions: true`), defaulting off.
- A tombstone-count threshold aborts the run — if a run would tombstone more than N% of a scope, it stops and reports rather than proceeding, on the assumption that a source-side outage is more likely than a mass drop.
**RED**: Four separate tests for dropped / filtered / crashed / out-of-scope. The crash test is the critical one: kill the run mid-stream and assert nothing is tombstoned. Plus a threshold test asserting a run that would delete 90% aborts. Mutator watch: tombstoning on partial enumeration must fail the crash and scope tests; an absent threshold must fail the abort test.
**GREEN**: enumeration-scope tracking, completion gating, threshold guard.
**REFACTOR**: this slice carries the most conditional logic in the epic. Assess extracting a pure `reconcile(enumerated_scope, seen_fqns, existing_fqns) -> Vec<ToTombstone>` into core — pure, exhaustively testable, no I/O.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Ingestion is bulk and idempotent

**Value**: A 10,000-table run completes in minutes, not hours, without duplicating on retry.
**Path**: `POST /{collection}/bulk` with `207 Multi-Status` (deferred here from Epic 16 (`16-ingestion-apis.md`)); `Idempotency-Key` support.
**Acceptance criteria**:
- Batches of up to 1000; larger → `400`.
- `207` with per-item status and per-item error.
- One bad entity does not discard the other 999.
- A replayed batch with the same `Idempotency-Key` returns the original response, creating nothing new.
- Idempotency records expire after 24h.
- Bulk writes emit one change event per entity, not one per batch.
**RED**: Test posting 100 entities with item 42 invalid, asserting 99 created and one reported. Replay test asserting no duplicates. Mutator watch: all-or-nothing batch semantics must fail the partial test.
**GREEN**: bulk endpoints, per-item isolation, idempotency store.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Runs are observable and reportable

**Value**: An operator can tell whether last night's run worked and what it did.
**Path**: `ConnectorRun` record; `POST /connectors/{id}/run`; `GET /connectors/{id}/runs`.
**Acceptance criteria**:
- Run records id, connector, start/end, status (`Running|Success|PartialFailure|Failed`), and counts (created/updated/unchanged/tombstoned/filtered/failed).
- Per-entity failures recorded with the reason, capped to avoid unbounded growth.
- A run in progress is visible as `Running`.
- A crashed run is eventually marked `Failed`, not left `Running` forever.
- Concurrent runs of the same connector → `409`.
- Run history is paginated and prunable.
**RED**: Test asserting a crashed run transitions out of `Running` via a stale-run reaper, not by hanging. Mutator watch: absent reaper must fail it.
**GREEN**: run entity, lifecycle, reaper, endpoints.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice H: Connectors act as identifiable principals

**Value**: `updated_by` names which connector changed a description, not `system`.
**Path**: each connector configured with a bot `User` (Epic 11 — users, teams, ownership); its principal is used for every write in the run.
**Acceptance criteria**: entities written by a connector carry that bot as `updated_by`; change events carry it; a connector without a configured bot fails at configuration time; bot users are excluded from human-oriented user lists by default.
**RED**: Test asserting `updatedBy` equals the bot's name after a run. Mutator watch: falling back to `system` must fail it.
**GREEN**: bot principal wiring.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Open table formats (Iceberg and similar)** → a connector module like any other source, once a named deployment needs it. graph-owl catalogs the table's *metadata* — schema, partitioning, snapshots, location. It does not read the data files; that is the data plane (`00a-product-position.md`).

- **A scheduler** → external cron or an orchestrator. Decision 5; not a gap.
- **Connectors beyond Postgres** (Snowflake, BigQuery, Kafka, dbt, Airflow) → each is independently valuable and largely mechanical once the trait is proven. Each is its own small plan, not a deferral of this epic.
- **Lineage extraction during ingestion** → Epic 15.
- **Profiling / sampling** (row counts, null ratios, distributions) → a distinct product surface, explicitly off the roadmap.
- **CSV import/export** → alongside bulk, when a non-programmatic bulk-edit workflow is requested.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. End-to-end connector tests run against a seeded Postgres testcontainer, exercising the real pipeline.
5. Deletion-detection tests (Slice E) reviewed with particular care — this is where a bug silently destroys a catalog.
