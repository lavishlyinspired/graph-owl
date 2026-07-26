# Plan: Inbound Events & Webhooks (Epic 18)

**Branch**: feat/inbound-events
**Status**: Not started
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

- [ ] Endpoint registration with secret, scheme, mapping, and event filter.
- [ ] Signature verified before the payload is parsed; a bad signature is `401` and logged.
- [ ] Redelivery of the same event is a no-op recorded as `Duplicate`.
- [ ] An out-of-order event describing older state does not overwrite newer state.
- [ ] Mapping failure sends the event to a dead-letter queue, replayable after fixing the mapping.
- [ ] Replay over a time window re-processes without duplicating applied effects.
- [ ] Rate limits and payload caps enforced per endpoint.
- [ ] Ingestion runs Epic 17 resolution so a webhook does not create a duplicate entity.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Endpoint registration and signature verification

**Acceptance criteria**: register with URL path, secret, scheme, mapping ref; secret stored hashed, shown once, never retrievable; HMAC-SHA256 with configurable header and prefix; Ed25519 supported; a bad signature → `401` **before** deserialization; a missing signature header → `401`; timing-safe comparison; signature covers the raw body bytes, not a re-serialization.
**RED**: A test asserting the parser is never invoked on a bad signature (parse counter or a payload that would panic the parser). A raw-bytes test: a body that re-serializes differently must still verify. Mutator watch: parsing before verifying must fail the counter test; `==` instead of a constant-time compare must fail a timing-safety lint; verifying re-serialized bytes must fail the raw-bytes test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Dedup and ordering

**Acceptance criteria**: same `sender_event_id` twice → second recorded `Duplicate`, no effect; no sender id → content-hash dedup; an event with an older `sender_timestamp` than the entity's current state does not overwrite it; an event with no timestamp falls back to arrival order and records a warning; concurrent duplicate deliveries produce one effect.
**RED**: The out-of-order test is the important one: apply event at T2, then deliver event at T1, assert T2's state survives. Concurrency test for simultaneous duplicates. Mutator watch: arrival-order LWW must fail the out-of-order test — this is the bug that silently reverts fresh metadata to stale.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Declarative mapping

**Acceptance criteria**: mapping config maps payload paths to draft fields; a missing required path → mapping failure naming the path; the expression set is closed (path, literal, concat, lowercase, template) and cannot loop; a mapping producing an invalid draft is rejected by Epic 5 validation with both the mapping and the shape named; mappings are versioned so a fix is auditable; a sample payload can be dry-run against a mapping without applying.
**RED**: Dry-run test asserting no effect. A mapping-failure test asserting the error names both the mapping and the offending path — a failure that only says "invalid draft" is unfixable. Mutator watch: an unbounded expression evaluator must fail a loop-attempt test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Dead-letter and replay

**Acceptance criteria**: mapping or validation failure moves the event to DLQ with the reason and raw payload retained; DLQ is listable and filterable by endpoint and reason; replay after a mapping fix re-processes and applies; replay of an already-applied event is a no-op (dedup still holds); replay over a time window processes in `sender_timestamp` order; DLQ retention is bounded and configurable.
**RED**: The replay-idempotency test: replay a window containing already-applied events, assert no duplicate effects. Mutator watch: replay bypassing dedup must fail it — replaying a window would otherwise double-apply everything in it.
**Done when**: criteria met, mutation report reviewed, commit approved.

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
