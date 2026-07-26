# Plan: Ingestion APIs, SDKs, Batch & Custom Adapters (Epic 16)

**Branch**: feat/ingestion-apis
**Status**: Not started
**Depends on**: Epic 1 (contract), Epic 15 (upsert semantics)
**Crate**: `graph-owl-connectors` (shared types), SDK generated from Epic 1's OpenAPI

## Goal

A supported path to push metadata from anything you can write code against — for every source that will never have a shipped connector.

## Resolved decisions

1. **Custom adapters run out-of-process, not as plugins.** No ABI coupling, no shared crash blast radius, any language, and they ship on the adapter author's schedule. The `Connector` trait (Epic 15) stays the *in-tree* extension point for sources worth maintaining upstream; the SDK is the *out-of-tree* one for everything else.
2. **Batch is a job, not a request.** A 500k-row file cannot be request/response. Upload returns a job handle; progress and per-row errors are polled.
3. **Validation at the ingestion boundary.** Epic 5's shapes run before anything lands, so a malformed push is rejected per-entity rather than corrupting the graph.
4. **Idempotency is mandatory for push, not optional.** A retried push must converge. Without it, at-least-once transport (Epic 18) duplicates.
5. **SDKs are generated from the OpenAPI contract, hand-wrapped for ergonomics.** Generated-only clients are unpleasant; hand-written ones drift.

## Implementation reference

```rust
// Shared ingestion envelope — one shape for every push path
pub struct IngestRequest {
    pub source: SourceRef,               // who is pushing; a bot principal
    pub scope: Option<ScopeRef>,         // what this push claims to cover
    pub entities: Vec<EntityDraft>,
    pub relationships: Vec<RelationshipDraft>,
    pub lineage: Vec<LineageDraft>,
    pub idempotency_key: Option<String>,
}

pub struct IngestResult {
    pub created: usize, pub updated: usize, pub unchanged: usize,
    pub failed: Vec<ItemError>,          // index + problem, never aborts the batch
}
```

`EntityDraft` is FQN-keyed, not id-keyed — the pusher does not know graph-owl's UUIDs.

### Endpoints

| Endpoint | Semantics |
|---|---|
| `POST /ingest` | Synchronous, ≤ 1000 items, `207 Multi-Status` |
| `POST /ingest/batch` | Multipart file upload → `202` + job handle |
| `GET /ingest/jobs/{id}` | Status, counts, per-row errors, progress |
| `DELETE /ingest/jobs/{id}` | Cancel an in-flight job |

### Batch formats

CSV (documented column contract per entity type), JSONL (one `EntityDraft` per line), Parquet (columnar, for large exports from a warehouse). JSONL is canonical; CSV and Parquet map onto it.

Processing is streaming — a 500k-row file is never fully materialized. Errors accumulate to a bounded cap (default 1000) after which the job fails with "too many errors" rather than producing an unreadable report.

## Acceptance criteria

- [ ] Synchronous push returns `207` with per-item status; one bad item does not discard the rest.
- [ ] A replayed `Idempotency-Key` returns the original response, creating nothing.
- [ ] Batch upload returns a job handle; progress and per-row errors are pollable.
- [ ] Batch processing is streaming — memory bounded regardless of file size.
- [ ] Epic 5 validation runs before anything lands; violations reject per-entity.
- [ ] SDKs exist for at least two languages, generated in CI and round-trip tested.
- [ ] A documented custom-adapter guide with a runnable example.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Synchronous push with partial success

**Acceptance criteria**: `POST /ingest` accepts entities, relationships, and lineage in one call; ≤1000 items, larger → `400`; `207` with per-item index, status, and problem; item 42 invalid → 999 land, one reported; parents applied before children within the batch; a relationship whose endpoints are in the same batch resolves.
**RED**: 100 items with item 42 invalid, asserting 99 created and one reported with index 42. An intra-batch relationship test — a push containing a table and an edge to it must work, since a pusher cannot pre-create in dependency order. Mutator watch: all-or-nothing must fail the partial test; requiring pre-existing endpoints must fail the intra-batch test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Idempotency

**Acceptance criteria**: `Idempotency-Key` header; a replay within 24h returns the original response body and status, creating nothing; the same key with a *different* body → `409` (a key identifies a request, not a slot); keys expire after 24h; concurrent identical requests produce one effect, not two.
**RED**: The different-body test — reusing a key for different content is a client bug and must be reported, not silently served the old response. A concurrency test firing N identical requests asserting one effect. Mutator watch: serving the cached response for a different body must fail; a non-atomic key check must fail the concurrency test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Batch file ingestion

**Acceptance criteria**: multipart upload of JSONL, CSV, or Parquet → `202` + job id; `GET /ingest/jobs/{id}` reports queued/running/succeeded/partial/failed with counts and progress; per-row errors carry the row number; memory stays bounded on a 500k-row file; error cap (1000) fails the job with a clear reason rather than an unreadable report; cancel stops processing and reports what landed; a crashed job transitions out of `running` via a reaper.
**RED**: A memory-bounded test against a large generated file — assert peak RSS, not just completion. A crash test asserting the reaper marks the job failed rather than leaving it `running` forever. Mutator watch: buffering the whole file must fail the memory bound; a missing reaper must fail the crash test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Boundary validation

**Acceptance criteria**: Epic 5 shapes run on drafts before persistence; a `Violation` rejects that entity with the shape and constraint named; `Warning` lands with the warning recorded; validation failures are per-entity, never per-batch; a draft referencing an unregistered predicate → rejected naming it; validation runs before the FQN uniqueness check so a client sees the substantive error, not a conflict.
**RED**: An ordering test: a draft that is both shape-invalid and FQN-conflicting must report the shape violation, which is the actionable one. Mutator watch: reporting the conflict first must fail it; batch-level rejection must fail the per-entity assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: SDKs

**Acceptance criteria**: generated from Epic 1's OpenAPI in CI for TypeScript and Python; hand-written ergonomic wrappers for the ingestion path (builder for `IngestRequest`, automatic batching, retry with backoff, idempotency-key generation); an end-to-end test pushes through each SDK against a running service; SDK version is pinned to a contract version; generation runs on every PR so a contract change that breaks an SDK fails the build.
**RED**: Round-trip test per SDK against a real service. A contract-drift test: change a field type and assert SDK generation or the round-trip fails. Mutator watch: an SDK that silently ignores an unknown field must fail the drift test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Custom adapter guide and example

**Acceptance criteria**: a documented guide covering the push contract, idempotency, scoping, error handling, and bot-principal setup; a runnable reference adapter (a small script pushing from a fixture source) that lives in the repo and runs in CI; the guide states the out-of-process decision and why; the example uses only the published SDK — no internal reach-through.
**RED**: The example runs as an integration test. A test asserting it imports only published SDK surface. Mutator watch: an example reaching into internals must fail the surface check.
**REFACTOR**: whatever the example finds awkward is a real SDK defect. Fix the SDK.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **In-process plugins** → never (decision 1).
- **gRPC ingestion** → HTTP suffices; revisit if throughput demands it.
- **Schema registry integration** (Avro/Protobuf contract enforcement on push) → Epic 27's contracts are the general answer.
- **Additional SDK languages** → each is a CI job once two are proven.
- **Streaming push (long-lived connection)** → Epic 19 owns broker consumption; a push stream would duplicate it.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. Batch memory bound asserted against a large generated file, not a fixture.
5. SDK round-trips run against a real service in CI.
