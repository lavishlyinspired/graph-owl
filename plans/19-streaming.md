# Plan: Streaming Ingestion (Epic 19)

**Branch**: feat/streaming
**Status**: Not started
**Depends on**: Epic 16 (ingestion contract), Epic 18 (dedup and ordering machinery)
**Crates**: `graph-owl-connectors` (broker consumers, offset management; reuses Epic 18's mapping) · `graph-owl-server` (consumer lifecycle, lag metrics) — no new crates

## Goal

Consume metadata continuously from a message broker, holding a durable subscription rather than receiving isolated calls — which makes ordering and replay tractable at the cost of running a stateful consumer.

## Resolved decisions

1. **Durable subscription, not a push endpoint.** graph-owl owns the offset, so replay is exact and gaps are detectable. Epic 18's webhooks cannot offer either.
2. **Offsets commit after apply, never before.** At-least-once with dedup (Epic 18) beats at-most-once with data loss. A pre-commit crash reprocesses; a post-commit crash loses.
3. **Lag is a first-class metric, not a log line.** Consumer lag is the single number that says whether the context is current. It belongs on the dashboard next to request latency.
4. **Poison messages are quarantined, never retried forever.** A message that fails N times goes to DLQ and the consumer advances. Blocking a partition on one bad message stops all metadata behind it.
5. **One consumer group per deployment.** Multiple graph-owl instances consuming the same topic in the same group share partitions; different groups would each apply everything.

## Implementation reference

```rust
pub struct StreamSubscription {
    pub id: Uuid,
    pub broker: BrokerConfig,            // Kafka | Pulsar | Redpanda (Kafka protocol)
    pub topic: String,
    pub consumer_group: String,
    pub mapping: MappingRef,             // reuses Epic 18's declarative mapping
    pub start_position: StartPosition,   // Earliest | Latest | Timestamp(t) | Offset(n)
    pub max_in_flight: usize,
    pub poison_threshold: u32,           // default 3
}

pub struct ConsumerHealth {
    pub assigned_partitions: Vec<i32>,
    pub lag_per_partition: HashMap<i32, i64>,
    pub last_commit: Option<DateTime<Utc>>,
    pub state: ConsumerState,            // Starting|Consuming|Rebalancing|Paused|Failed
}
```

Reuses Epic 18's mapping and dedup wholesale — the payload shapes and duplication problem are identical; only the transport differs.

### Backpressure

The consumer bounds in-flight messages (`max_in_flight`) and stops polling when the apply pipeline is saturated. It does **not** buffer unboundedly — an unbounded buffer converts a slow database into an OOM.

### Rebalancing

On partition reassignment the consumer finishes in-flight work for revoked partitions, commits, then releases. Abandoning in-flight work on revoke causes reprocessing; committing work for a partition you no longer own causes loss.

## Acceptance criteria

- [ ] Subscribe to a topic and apply mapped metadata continuously.
- [ ] Offsets commit only after successful apply.
- [ ] A crash mid-batch reprocesses without duplicating effects (Epic 18 dedup).
- [ ] Lag is exposed per partition as a Prometheus gauge.
- [ ] A poison message is quarantined after N attempts and the consumer advances.
- [ ] Backpressure bounds memory under a faster producer.
- [ ] Rebalancing does not lose or duplicate in-flight work.
- [ ] Replay from a timestamp or offset re-processes idempotently.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Consume and apply

**Acceptance criteria**: subscribe to a topic against a real broker (testcontainers); messages map via Epic 18's mapping and apply through Epic 16's ingestion path; Epic 17 resolution runs so streamed entities do not duplicate existing ones; `start_position` honoured for all four variants; consuming an empty topic idles without spinning.
**RED**: End-to-end test against a Kafka testcontainer: produce, consume, assert the entity exists via the HTTP API. An idle test asserting CPU is not consumed by a tight poll loop. Mutator watch: skipping resolution must fail a duplicate-entity assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Offset commit after apply

**Acceptance criteria**: offset commits only after the apply succeeds; a failure before apply leaves the offset uncommitted; killing the consumer mid-batch and restarting reprocesses the uncommitted messages; reprocessing produces no duplicate effects; commit interval is configurable; a commit failure is retried and does not lose the applied state.
**RED**: The kill-and-restart test is the specification — produce 10, kill after 5 applied but before commit, restart, assert 10 entities exist and none duplicated. Mutator watch: committing before apply must fail the restart test by losing the uncommitted messages, which is silent data loss.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Lag and health

**Acceptance criteria**: `stream_consumer_lag{topic,partition}` gauge; `ConsumerState` exposed via `/ready` — a failed consumer makes readiness fail; lag is computed against the broker's high-water mark, not estimated; assigned partitions reported; `last_commit` timestamp exposed so a stalled-but-alive consumer is detectable.
**RED**: A test producing 100 messages with the consumer paused, asserting lag reports 100. A stall test asserting `last_commit` staleness is visible — a consumer that is alive but not progressing is the failure mode metrics must catch. Mutator watch: estimated rather than broker-reported lag must fail the exact-100 assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Poison messages and backpressure

**Acceptance criteria**: a message failing `poison_threshold` times moves to DLQ with its raw payload and error, and the consumer advances past it; the DLQ is replayable after a mapping fix; in-flight messages are bounded by `max_in_flight`; a producer faster than the apply pipeline causes polling to pause, not memory to grow; memory stays bounded in a sustained-overload test.
**RED**: A poison test with one permanently-bad message among good ones, asserting the good ones after it are applied — a blocking retry would starve them. A sustained-overload memory test. Mutator watch: infinite retry must fail the starvation test; unbounded buffering must fail the memory bound.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Rebalancing and replay

**Acceptance criteria**: on revoke, in-flight work for revoked partitions completes and commits before release; on assign, consumption starts from the committed offset; two consumers in one group split partitions and neither double-applies; replay from a timestamp re-processes idempotently; replay does not disturb the live subscription's offsets (it runs as a separate group).
**RED**: A two-consumer test asserting each message is applied exactly once across both. A replay test asserting the live consumer's offsets are unaffected. Mutator watch: abandoning in-flight work on revoke must fail the exactly-once assertion; replay sharing the live consumer group must fail the offset-isolation test.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **RDF stream processing (C-SPARQL, RSP-QL)** → continuous queries over streams are out of scope; this epic ingests, it does not query streams.
- **Exactly-once via transactional producer** → at-least-once plus dedup is simpler and sufficient; revisit only if a source cannot supply a dedup key.
- **Schema-registry-driven mapping** (Avro schema → mapping automatically) → attractive; needs Epic 27's contracts first.
- **Brokers beyond the Kafka protocol** (SQS, Pub/Sub, NATS) → each is a `BrokerConfig` variant; add on demand.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. Integration tests run against a real broker via testcontainers, not a stub.
5. Kill-and-restart test verified (Slice B) — this is where silent data loss lives.
6. Sustained-overload memory bound asserted (Slice D).
