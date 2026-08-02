# Plan: Inbound Events & Webhooks (Epic 18)

**Branch**: feat/inbound-events
**Status**: In progress (Slices A-D shipped)
**Depends on**: Epic 16 (ingestion contract), Epic 17 (resolution, so pushes do not duplicate)
**Crates**: `graph-owl-connectors` (webhook receiver, declarative mapping) · `graph-owl-server` (endpoints, signature verification, rate limits) · `graph-owl-core` (InboundEvent) — no new crates

## Goal

Sources notify graph-owl the moment something changes, instead of waiting for the next scheduled pull. The difference between minutes-fresh and nightly-stale context — and stale context is what makes an agent confidently wrong.

## Why this is not part of connectors

None of the machinery below exists in a pull connector, and the failure modes are entirely different. A connector controls its own pace and sees a complete picture; a webhook receiver is handed unordered, duplicated, occasionally-malicious traffic it did not ask for.

## Resolved decisions

1. **At-least-once and unordered is the contract, not an edge case.** Every real webhook sender retries and none guarantees order. Dedup and out-of-order handling are core, not hardening.
2. **Signature verification before parsing.** An unverified payload is never deserialized — parsing untrusted bytes is the attack surface.
3. **Mapping is per-source and declarative.** A dbt run-completed payload and an Airflow DAG-completed payload share nothing. Mappings are configuration, not code, so adding a source needs no release.
4. **Replay over a window, not from the beginning.** A missed-delivery window is recoverable; full replay is not, because senders do not retain history.
5. **A public endpoint is an attack surface.** Rate limits, payload caps, and a dead-letter path are day-one requirements.

## Implementation reference

```rust
pub struct WebhookEndpoint {
    pub id: Uuid,
    pub source: SourceRef,               // bot principal this maps to
    pub secret: Secret,                  // write-only, shown once
    pub signature_scheme: SignatureScheme,   // HmacSha256 { header, prefix } | Ed25519
    pub mapping: MappingRef,
    pub event_filter: Vec<String>,       // sender's event type names
    pub enabled: bool,
}

pub struct InboundEvent {
    pub id: Uuid,
    pub endpoint: Uuid,
    pub sender_event_id: Option<String>, // for dedup
    pub sender_timestamp: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub raw: Vec<u8>,
    pub state: EventState,               // Received|Mapped|Applied|Failed|Duplicate
}
```

### Dedup and ordering

Dedup key is `(endpoint, sender_event_id)` when the sender provides one, else a content hash. Out-of-order arrival is handled by **last-writer-wins on `sender_timestamp`**, not on arrival time: an event describing an older state must not overwrite a newer one. Events without a sender timestamp fall back to arrival order with a recorded warning, because that is the best available and the ambiguity should be visible.

### Mapping

Declarative field-path mapping from payload JSON to `EntityDraft` / `LineageDraft`, with a small expression set (path, literal, concat, lowercase, template). Deliberately not a general scripting language — a mapping that can loop is a mapping that can hang the receiver.

## Acceptance criteria

- [x] Endpoint registration with secret, scheme, mapping, and event filter.
- [x] Signature verified before the payload is parsed; a bad signature is `401` and logged.
- [x] Redelivery of the same event is a no-op recorded as `Duplicate`.
- [x] An out-of-order event describing older state does not overwrite newer state. Closed in Slice D as an extension, once an apply path existed to compare against: `EventState::Superseded` + `entity_last_applied` (`V31`). Mechanism fully tested; not yet exercisable via a live payload, since nothing extracts `sender_timestamp` from one yet (same gap Slices B/C documented).
- [x] Mapping failure sends the event to a dead-letter queue, replayable after fixing the mapping.
- [x] Replay over a time window re-processes without duplicating applied effects.
- [ ] Rate limits and payload caps enforced per endpoint.
- [ ] Ingestion runs Epic 17 resolution so a webhook does not create a duplicate entity.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Endpoint registration and signature verification

**Acceptance criteria**: register with URL path, secret, scheme, mapping ref; secret stored hashed, shown once, never retrievable; HMAC-SHA256 with configurable header and prefix; Ed25519 supported; a bad signature → `401` **before** deserialization; a missing signature header → `401`; timing-safe comparison; signature covers the raw body bytes, not a re-serialization.
**RED**: A test asserting the parser is never invoked on a bad signature (parse counter or a payload that would panic the parser). A raw-bytes test: a body that re-serializes differently must still verify. Mutator watch: parsing before verifying must fail the counter test; `==` instead of a constant-time compare must fail a timing-safety lint; verifying re-serialized bytes must fail the raw-bytes test.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped**: `graph-owl-connectors::webhook_signature` — pure `verify_hmac_sha256` (`hmac`/`sha2`, constant-time via `Mac::verify_slice`) and `verify_ed25519` (`ed25519-dalek` v2), 0 missed mutants (13/13 caught). `graph_owl_storage::{SignatureScheme, WebhookEndpoint}` — the endpoint type carries no secret field at all (`has_secret: bool` only), matching `ConnectorConfig`'s existing pattern; the raw key material is readable through exactly one method, `Storage::webhook_secret`. Postgres-backed (`V25`/`V26`), `ON CONFLICT (id) DO UPDATE` with a separate `UNIQUE (path)` violation mapped to its own `ConflictKind::WebhookPathExists` rather than silently reattaching a colliding path to the wrong row. 0 missed mutants at the repository layer (28 mutants: 23 caught, 5 unviable).

**One deliberate departure from the plan's own wording**: "secret stored hashed" is not implementable as written — HMAC verification recomputes the MAC from the raw key, which a hash cannot supply, and Ed25519's "secret" here is the sender's *public* verifying key, which is not sensitive on its own. The secret is stored as raw key material instead, behind the same one-read-path seam a hash would have used. `Catalog::receive_webhook` orchestrates: a disabled endpoint reads `404` (pulled forward from Slice E's own reasoning — an existence signal is unnecessary), a bad or missing signature is `401` (`CatalogError::Unauthenticated`, a new variant distinct from `Forbidden` per RFC 9110's 401-vs-403), and a verification failure is logged at `warn` naming only the endpoint id — never the secret, header value, or body. 0 missed mutants at the facade (12 real + further unviable across two rounds — the second round closed a gap where `webhook_endpoint`/`webhook_endpoint_by_path`/`list_webhook_endpoints` had no test distinguishing their real reads from `Ok(None)`/`Ok(vec![])`).

HTTP layer: `POST`/`GET /webhooks/endpoints` (admin-gated, modeled on `save_connector_config`/`list_connector_configs`), `POST /webhooks/receive/{path}` — `axum::body::Bytes` read before any JSON parsing, the signature header named by the endpoint's own configured scheme. Not `authenticated` in the OpenAPI route table (the sender carries no bearer token; the endpoint's own signature scheme stands in for it), documented inline so the omitted `401` entry reads as a deliberate distinction rather than a gap. 0 missed mutants (12 caught, 7 unviable) — reached only by running the *un-scoped* integration suite for this diff (`--lib` is blind to these handlers, which only `tests/webhooks.rs` and `tests/openapi.rs` exercise; see CLAUDE.md's `--lib`-blindness note), with `--test-threads=1` passed through as `-- -- --test-threads=1` to avoid the documented default-parallelism container contention in cargo-mutants' own baseline check.

### Slice B: Dedup and ordering

**Acceptance criteria**: same `sender_event_id` twice → second recorded `Duplicate`, no effect; no sender id → content-hash dedup; an event with an older `sender_timestamp` than the entity's current state does not overwrite it; an event with no timestamp falls back to arrival order and records a warning; concurrent duplicate deliveries produce one effect.
**RED**: The out-of-order test is the important one: apply event at T2, then deliver event at T1, assert T2's state survives. Concurrency test for simultaneous duplicates. Mutator watch: arrival-order LWW must fail the out-of-order test — this is the bug that silently reverts fresh metadata to stale.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, partially — the out-of-order criterion is deliberately deferred, not met.** Before writing this slice, a real fork surfaced: "an event with an older `sender_timestamp` than the entity's current state does not overwrite it" presupposes an identified entity, and nothing before Slice C's mapping says *which* entity a payload describes. Asked and decided: defer real LWW enforcement to Slice C, where entity identity actually exists, rather than inventing an unspecified subject key now or approximating with a coarser one (e.g. per-endpoint) that would incorrectly make unrelated entities from the same source compete.

What *is* shipped and fully verified: `graph_owl_core::webhook::dedup_key` (sender's own id when given, else a `sha256` content hash of the raw bytes, prefixed `id:`/`hash:` so the two spaces cannot collide) and `compare_timestamps`/`Freshness` (the pure LWW comparison — `Newer`/`Older`/`Ambiguous`, the last for a missing `sender_timestamp`, which is "falls back to arrival order and records a warning"'s decision). Both pure, 0 missed mutants (5 caught, 1 unviable). **Still not wired anywhere as of Slice C either** — Slice C's mapping produces a draft and checks it against shapes, but does not apply one to a real entity; that only happens once Slice D's dead-letter/replay work adds the Mapped→Applied transition, which is the actual point an "older than current state" comparison has anything real to compare against.

`InboundEvent` gained a stored `dedup_key` field (computed once, not recomputed — so a later replay compares against exactly the key a redelivery was judged against). `Storage::create_inbound_event` now inserts the event row, then attempts an `inbound_event_dedup` marker keyed `(endpoint_id, dedup_key)` (`V27`); a conflict downgrades that row's own `state` to `Duplicate` before commit — never a second effect for an already-claimed key, and never blocking two different entities' events even when their sender picked the same event id (scoped per endpoint, verified). Row-order matters here for a reason worth remembering: the marker's `first_event_id` is a real foreign key, so the event row must exist *before* the marker references it — inserting the marker first fails against an empty table every time (found immediately by the integration suite, not by inspection). 0 missed mutants — though only 2 were generated at all (both the whole-function replacement, both unviable): cargo-mutants has no built-in mutator for an `Option::is_none()` method-call condition the way it does for bare `!` negation, so the reordering's correctness rests on the four-test integration matrix (redelivery, distinct keys, distinct endpoints, and a real `tokio::spawn` concurrency test asserting exactly one `Received` and one `Duplicate`) rather than on a generated mutant — worth knowing before trusting a thin mutant count as "little to test" here.

Every delivery reaching `Catalog::receive_webhook` today has `sender_event_id: None` (extraction from a payload is Slice C's own declarative-mapping problem, not this slice's), so content-hash dedup is the only path exercised end-to-end through the live facade; the sender-id path is proven directly against `dedup_key` and against storage with a hand-constructed event. 0 missed mutants at the facade (1 generated, unviable).

### Slice C: Declarative mapping

**Acceptance criteria**: mapping config maps payload paths to draft fields; a missing required path → mapping failure naming the path; the expression set is closed (path, literal, concat, lowercase, template) and cannot loop; a mapping producing an invalid draft is rejected by Epic 5 validation with both the mapping and the shape named; mappings are versioned so a fix is auditable; a sample payload can be dry-run against a mapping without applying.
**RED**: Dry-run test asserting no effect. A mapping-failure test asserting the error names both the mapping and the offending path — a failure that only says "invalid draft" is unfixable. Mutator watch: an unbounded expression evaluator must fail a loop-attempt test.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped.** `graph_owl_storage::Expression` — five variants (`Path` via RFC 6901 JSON Pointer, `Literal`, `Concat`, `Lowercase`, `Template`), matching the plan's own vocabulary exactly. Closed by construction, not by a runtime check: every variant recurses into a strictly smaller, *owned* sub-expression (`Box`/`Vec`/`BTreeMap`), so a cyclic `Expression` cannot be constructed at all — there is no unsafe `Rc<RefCell<_>>` trick available to build one. The one genuine loop risk in a small template language — a naive evaluator that re-scans its own *output* for more `{placeholders}`, which a bound value containing literal braces could hijack — is closed structurally: substitution is a single left-to-right pass over the pattern, checked by a mutator-watch test binding a value that itself contains `{curly braces}` and asserting they come through as literal text. 0 missed mutants (14 caught, 1 unviable).

`apply_mapping` (`graph-owl-connectors::webhook_mapping`) turns a mapping plus a payload into a `RowDraft` — **reusing Epic 16 Slice C's own entity-draft type** rather than inventing the plan's aspirational `EntityDraft` fresh; neither `EntityDraft` nor `LineageDraft` exist anywhere in this codebase outside plan prose, and `RowDraft` already *is* that concept. A missing `kind` or `entity_name` path names the mapping and the field (never just "invalid draft"); an unresolved optional field is absent, not an error, matching batch ingestion's own "empty means absent" rule.

`graph_owl_storage::Mapping` is versioned by an **append-only table** (`mapping_versions`, `V28`, `UNIQUE (name, version)`) — every `upsert_mapping` computes the next version inside the `INSERT` itself (`COALESCE(MAX(version), 0) + 1` as a subquery, one round trip, no read-then-write window) and adds a row rather than overwriting one, so an old rule is still there to diff against after a fix. `Catalog::mapping`/`mapping_versions` read the latest and the full history respectively. 0 missed mutants (2 real + further unviable, storage; 2 real + further unviable, facade).

**The shape-rejection criterion reuses `Catalog::validate_draft` (Epic 16 Slice D) rather than building new machinery** — a real design fork, asked rather than assumed: the criterion's literal wording implies synchronous SHACL checking of a not-yet-persisted entity, which sounds new, but `validate_draft` already does exactly this (projects a draft to flakes, validates against currently-compiled shapes, returns the shape and constraint that failed) and already backs the identical criterion on the batch-ingestion path. `Catalog::dry_run_mapping` applies a mapping, and if the draft resolves, builds a synthetic `Asset` (mirroring `ingest()`'s own construction exactly, attributed to `system`) and runs it through the same check. **A separate question the user raised directly**: whether to adopt the `shacl` crate (crates.io/docs.rs, MIT/Apache-2.0) instead of this project's own hand-built constraint engine. Checked and rejected for now: `shacl` validates against its own in-memory RDF graph or a remote SPARQL endpoint, neither of which is the "caller supplies a trait, data never leaves our storage" shape `00l-build-vs-adopt.md` uses as its adoption test for this exact class of library (the reason `spareval` is adopted and `oxigraph` is not) — adopting it would mean materializing every `Flake` into a foreign graph representation, or standing up a SPARQL endpoint, to replace a working, already-integrated engine. A real migration, not a drop-in swap, and out of scope for this slice.

`MappingOutcome` (`Draft`/`MissingField`/`InvalidKind`/`ShapeViolation`) is the dry-run's whole result type — every variant a legitimate outcome, not a `CatalogError`, because a mapping that does not fit a sample payload is what a dry run exists to discover. `POST/GET /webhooks/mappings[/…]`, admin-gated the same way webhook endpoints are; `POST .../dry-run`'s body is the sample payload itself, unwrapped, so a dry run tests exactly what a sender would transmit. 0 missed mutants (7 caught + 19 unviable, reached only by the un-scoped integration suite — `--lib` is blind to the HTTP handlers here for the same reason it was in Slices A and B).

### Slice D: Dead-letter and replay

**Acceptance criteria**: mapping or validation failure moves the event to DLQ with the reason and raw payload retained; DLQ is listable and filterable by endpoint and reason; replay after a mapping fix re-processes and applies; replay of an already-applied event is a no-op (dedup still holds); replay over a time window processes in `sender_timestamp` order; DLQ retention is bounded and configurable.
**RED**: The replay-idempotency test: replay a window containing already-applied events, assert no duplicate effects. Mutator watch: replay bypassing dedup must fail it — replaying a window would otherwise double-apply everything in it.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped.** This is the slice that actually reaches the catalog: `Catalog::process_inbound_event` looks up an event's endpoint and mapping, parses the raw payload, calls the same `resolve_and_validate_draft` helper `dry_run_mapping` calls (refactored out of Slice C's own code once the duplication between "check it" and "check it and apply it" became real, not just similar-looking), and on success upserts the resulting draft — moving the event `Received` → `Mapped` → `Applied`. Any rejection along the way — JSON that doesn't parse, a missing mapping, a missing field, an invalid kind, a shape violation, **or a structural failure from `upsert_asset` itself** (a `table` with no parent is well-formed by every earlier check and still refused by containment) — dead-letters the event with a reason, never propagates as an unhandled error. That last case was not anticipated going in; four of the happy-path facade tests hit it first (a `table` fixture with no parent), which is what surfaced that `process_inbound_event` needed to catch `CatalogError::Validation`/`Conflict` from the upsert too, not only from mapping/shape checking.

**Idempotency is one gate, not two.** `process_inbound_event` only acts on `Received`/`Failed` events; `Applied`/`Duplicate` are left alone. `replay_window` calls the exact same function for every event in its window rather than re-deciding what counts as "already handled" — restating that rule at the replay layer is exactly how the two would drift apart later. `list_inbound_events_in_window` bounds the window by `received_at` (always populated) and orders by `COALESCE(sender_timestamp, received_at)` — the same arrival-order fallback `Freshness::Ambiguous` names, applied for the first time now that Slice D actually needs an ordering, not just the comparison Slice B proved in isolation.

The dedup marker table (`V27`) gained an `ON DELETE CASCADE` (`V30`) after purging a dead-lettered event that was its own dedup key's first delivery hit the foreign key — found by the integration suite, not by inspection. Once the event is gone the marker is moot anyway: a genuine redelivery after a purge is correctly treated as new, not incorrectly blocked against a row nobody can look up any more.

`DELETE /webhooks/dead-letters?olderThanDays=N` is retention's whole mechanism — the bound is named by whoever calls it (a runbook, a schedule), not a persisted setting this crate invents on its own; nothing else in this codebase has a general config store to put one in; and the API budget (`00a`) has no room for the schedule scaffolding a self-configuring cutoff would need. `POST /webhooks/replay` takes `{endpoint, since, until}` in the body (an action, not a filtered read) and returns `ReplaySummary { attempted, applied, still_failed, skipped }` — the `still_failed` count is what a replay that does not fix anything reports, found missing from the facade tests by mutation testing (`+=` → `-=`/`*=` on that one field, uncaught until a dedicated test replayed a window with nothing fixed).

0 missed mutants throughout, one documented equivalent: `graph-owl-core`/`graph-owl-storage`'s own diffs were data/trait-only and generated no mutants at all; `graph-owl-storage-postgres` and `graph-owl-api` both 0 missed once verified against the un-scoped integration suite (`--lib` is blind to Postgres-backed and HTTP-handler code, as in every prior slice); `graph-owl-server`'s one remaining "MISSED" — `ReplayRequest::validate_body` mutated to `vec![]` — is equivalent, not a gap: the real implementation already returns `Vec::new()` unconditionally (matching `ConnectionTestRequest`'s own precedent for a body with nothing to structurally validate), so the mutant is byte-identical to the code it replaced.

**Extended before commit to close a gap the slice split had left genuinely open.** The epic-level criterion "an out-of-order event describing older state does not overwrite newer state" was not satisfied by any slice as scoped — Slice D's own acceptance criteria (quoted above) never restate it, only the top-level list does, and closing it needed a place to compare against that did not exist until this slice's apply path did. Raised directly rather than left for a future session: `EventState` gains a sixth variant, `Superseded` — deliberately not reusing `Failed` (nothing about a stale delivery needs fixing, and "replay after a mapping fix" does not apply to it) or `Duplicate` (this is not a redelivery of the same event, just one overtaken by a different, newer one). `entity_last_applied` (`V31`, keyed by FQN rather than id — the comparison has to run *before* the entity necessarily exists) is the high-water mark `process_inbound_event` checks via `compare_timestamps` before every upsert, updating it only after a successful apply whose event actually carried a `sender_timestamp`. `replay_window` skips `Superseded` alongside `Applied`/`Duplicate`: once correctly recognized as stale, re-litigating it on replay would not change the answer, since the high-water mark only moves forward.

**Still not exercisable end-to-end**, for the same reason Slices B and C already documented: nothing extracts `sender_timestamp` from a live payload, so every delivery through the real HTTP pipeline today takes the `Freshness::Ambiguous` branch and applies in arrival order. The mechanism is fully built and tested by constructing events with an explicit `sender_timestamp` directly (matching Slice B's own testing pattern for the same gap) — proven correct and ready for the day extraction lands, not yet provably correct against a real sender. 0 missed mutants at the facade and storage layer; the `match` dispatching on `compare_timestamps`'s result generated none at all — cargo-mutants has no built-in mutator for a plain enum match the way it does for `!` negation or a comparison operator, so this decision's coverage rests entirely on the four dedicated tests (newer applies and moves the mark, older is superseded and does not overwrite, no-timestamp still applies, superseded is skipped on replay), not on a generated mutant.

### Slice E: Abuse resistance

**Acceptance criteria**: per-endpoint rate limit with `429` and `Retry-After`; payload size cap with `413`; a disabled endpoint returns `404`, not `403` (an existence signal is unnecessary); malformed JSON after a valid signature → `400` and DLQ, not a panic; a burst does not exhaust the connection pool — ingestion is queued, not synchronous; metrics per endpoint for received, applied, duplicate, dead-lettered.
**RED**: A burst test asserting the pool is not exhausted and the service stays responsive to other traffic. A malformed-payload test asserting no panic. Mutator watch: synchronous apply under burst must fail the responsiveness assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Outbound webhooks** → Epic 14; different direction, different concerns.
- **Sender-specific first-class integrations** (a built-in dbt receiver) → the declarative mapping covers it; a named integration is just a shipped mapping.
- **Webhook-triggered connector runs** ("something changed, go re-scan") → a reasonable pattern; add when a source's payload is too thin to map directly.
- **Push-based lineage from query engines** → Epic 28's usage ingestion is the closer fit.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Run the `security-review` skill** — this is an unauthenticated-by-default public endpoint.
5. Verify no payload is parsed before signature verification (Slice A).
6. Verify replay cannot double-apply (Slice D).
