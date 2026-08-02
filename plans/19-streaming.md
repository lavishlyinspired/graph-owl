# Plan: Streaming Ingestion (Epic 19)

**Branch**: feat/streaming
**Status**: Shipped (Slices A–F; Slice F partial — see its own section for the two Pulsar criteria deliberately left unmet)
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
6. **Two client crates, not one — Kafka and Pulsar do not share a wire protocol.** The original sketch below listed `Kafka | Pulsar | Redpanda` as if `(Kafka protocol)` covered all three; it does not. Redpanda genuinely speaks the Kafka protocol, so it needs no separate client. Pulsar has its own binary protocol and needs its own. Checked against `00i`'s permissive-only `cargo deny` allowlist (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib) before adopting either, since neither is covered by `00l-build-vs-adopt.md` (that document is scoped to the query/reasoning/RDF engine, not brokers):
   - **`rdkafka`** (MIT) for Kafka and Redpanda. Wraps `rdkafka-sys` (MIT), which vendors and statically compiles **librdkafka** (BSD-2-Clause) from C source at build time — a real build-time cost (a C toolchain is required) but every licence in the chain is permissive. The two pure-Rust alternatives were checked and disqualified on capability, not licence: `rskafka` (InfluxData, MIT/Apache-2.0) states outright "no support for offset tracking, consumer groups, transactions" — a hard blocker, since Slice E's rebalancing criteria are meaningless without consumer groups; `kafka` (kafka-rust) is pure Rust but effectively unmaintained. `rdkafka` is the only option that can satisfy this plan's own acceptance criteria.
   - **`pulsar`** (MIT OR Apache-2.0, `streamnative/pulsar-rs`) for Apache Pulsar. Pure Rust, no C dependency — the crate's own docs state it explicitly does not depend on the C++ Pulsar client. `default-features = false` with `tokio-rustls-runtime-ring` + `compression`: the crate's `default` feature set pulls in both `tokio-runtime` *and* `async-std-runtime` (this project uses only tokio) and `native-tls` (this project uses `rustls` everywhere else, see `reqwest`'s workspace dependency) — trimmed to match rather than carrying a second async runtime and a second TLS stack for no reason.
7. **The streaming consumer calls `Catalog::resolve_asset` automatically after every upsert.** Asked directly rather than assumed, because Epic 18's own plan explicitly left this open ("belongs to whichever epic decides ingestion should trigger resolution automatically") and this epic's own Slice A criterion — "Epic 17 resolution runs so streamed entities do not duplicate" — answers it concretely for streaming. Every other ingestion path (batch push, webhook) computes blocking keys automatically inside `upsert_asset` but leaves the match/merge decision to an explicit caller; streaming has no caller waiting on a response the way a webhook's sender or a batch push's client does, so nothing else is in a position to ask for it. `Principal::system()` attributes the automated call, matching the precedent already set for machine-driven writes (Epic 18's `process_inbound_event`, `V15`'s seeded `system` user). The consumer does not branch on `Resolution`'s variant — `New`, `Existing` (auto-merged) and `Ambiguous` (queued for review) are all "handled" by `resolve_asset` itself; the consumer's job is only to have called it.
   - **Both are crate-local dependencies in `graph-owl-connectors/Cargo.toml`, not workspace-level** (matching the existing `csv = "1"` precedent there) — neither is needed by any other crate today, and promoting to `[workspace.dependencies]` before a second consumer exists would be speculative.
   - **Test infrastructure is deliberately *not* shared with the Postgres testcontainers setup.** `testcontainers-modules 0.11` (the version already pinned workspace-wide for Postgres) has no `pulsar` feature; `0.12` adds it but requires bumping the `testcontainers` core crate from `^0.23` to `^0.24`. CLAUDE.md documents in detail how fragile the tuned Postgres container-reuse behaviour (`ReuseDirective::Always`, the `reusable-containers` feature) has been to get right and how a container-config change silently invalidates its reuse hash. Bumping the shared workspace pin to satisfy this epic risks that unrelated, already-working setup for no benefit to it. Instead, `graph-owl-connectors` pins its own `testcontainers`/`testcontainers-modules` versions in its own `[dev-dependencies]`, independent of the workspace pin — Cargo is fine compiling two versions of the same crate for different workspace members, and the cost (a larger `target/`, not a slower `cargo check` on unrelated crates) is worth the isolation.

## Implementation reference

```rust
pub struct StreamSubscription {
    pub id: Uuid,
    pub broker: BrokerConfig,            // KafkaProtocol { .. } (Kafka, Redpanda — one client, rdkafka) | Pulsar { .. } (a distinct protocol — pulsar crate)
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

- [x] Subscribe to a topic and apply mapped metadata continuously.
- [x] Offsets commit only after successful apply.
- [ ] A crash mid-batch reprocesses without duplicating effects (Epic 18 dedup). **Unproven, deliberately unchecked.** The commit-after-apply *code* is right and its commit path is covered by a passing test; what no test demonstrates is the end-to-end restart. Four attempts at simulating a crash in-process failed for a harness reason (a replacement consumer never receives, even under a fresh group that should replay from offset 0) — recorded at the test itself, which is `#[ignore]`d rather than deleted or weakened.
- [x] Lag is exposed per partition as a Prometheus gauge. **Kafka only** — Pulsar backlog needs the admin REST API (Slice F).
- [x] A poison message is quarantined after N attempts and the consumer advances.
- [x] Backpressure bounds memory under a faster producer — `queued.max.messages.kbytes`, librdkafka pausing its own fetch when the prefetch queue fills.
- [x] Rebalancing does not lose or duplicate in-flight work.
- [x] Replay from a timestamp or offset re-processes idempotently.

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

**Shipped (Slices A–E).** `StreamSubscription`/`BrokerConfig`/`StartPosition` in `graph-owl-storage` (`V33`), admin-gated `POST`/`GET /streaming/subscriptions`, `KafkaConsumer` in `graph-owl-connectors`, and `graph-owl-server::streaming` as the composition root — the only place that depends on both the broker client and `Catalog`, matching the role it already plays for connector runs. `Catalog::apply_streamed_message` reuses Epic 18's `resolve_and_validate_draft` wholesale rather than a second mapping path.

**The worst bug in the epic: a self-deadlock on a non-reentrant mutex.** Marking a consumer `Failed` while preserving its previous `last_commit` was written the obvious way — a `CONSUMER_HEALTH.lock()` read passed as an *argument* to `set_health`, which locks the same mutex itself. A temporary's guard lives until the end of the enclosing statement, so `set_health` blocked on a lock the same thread already held. `std::sync::Mutex` is not reentrant, and on tokio's current-thread runtime a blocked thread is the *whole* runtime. It cost four test timeouts at 240s each, and in production the first broker outage would have hung `/ready` for the entire server — a streaming fault presenting as a total readiness failure. Fixed with a `mark_failed` that mutates in place under one acquisition; the rule that falls out is worth keeping: **never call a locking function from inside an expression that is already holding that lock**, and prefer one function that does the whole read-modify-write to a read composed with a write.

**The kill-and-restart test does not work, and is `#[ignore]`d rather than quietly softened.** Four attempts — polling to a deadline, one long uninterrupted wait, a run-unique consumer group — all left the replacement consumer receiving nothing across a 90-second window. The decisive observation is that a *fresh* group with `auto.offset.reset = earliest` should replay from offset 0 no matter what the first consumer committed, and it still received nothing: that rules out the offset semantics the test exists to check and points at librdkafka's group membership outliving the dropped handle inside one process. Slice B's other behaviour is covered and passes; this specific end-to-end path is not, and the epic criterion stays unchecked to say so. A future attempt should assert on the **committed offset** directly rather than stage a fake crash — the property is "the offset advanced only for applied messages", which needs no second consumer at all.

**Dropping a consumer does not release its partitions.** Kafka keeps a departed member's assignment until `session.timeout.ms` (45s by default) expires, so a replacement joining the same group can sit unassigned for the better part of a minute — and `recv` simply blocks while it does. The kill-and-restart test originally called `process_one_message` a fixed six times, which meant it either blocked forever waiting for a rebalance that had not happened yet, or stopped early. It now polls to a deadline against the property it actually cares about ("all ten exist exactly once"), not against a receive count. Worth stating because the same shape will catch the next test written here: **a fixed number of blocking receives is a guess about broker timing dressed up as a test.**

**A replay's first message is a different question from its next one**, and giving both the same 3-second answer made every replay report an empty window. A replay runs in its own consumer group — that is precisely what keeps it from disturbing live offsets — so before any message can arrive it must join the group, be assigned partitions, and resolve `offsets_for_times`. That handshake takes several seconds against a real broker. The first wait is now 30s and every later one stays 3s.

**A fourth bug, and the most serious of the rest: the consume loop had no backoff.** `recv` against an unreachable broker fails *immediately*, so `loop { process_one_message(…) }` was a tight loop — a core burned per dead subscription, forever. It surfaced as a test against a deliberately-unreachable broker hanging for **78 minutes at full CPU** rather than failing, which also stalled every other verification queued behind it. `process_one_message` now reports whether it actually received anything, and `run_consumer` backs off 100ms → 30s on consecutive failures, resetting on the first success. Worth stating plainly because the failure mode is asymmetric: with a healthy broker this code path never executes, so no amount of happy-path testing would have found it, and in production it would have looked like unexplained CPU load rather than a streaming fault.

**Three further bugs a compile could not have caught**, all found by running against a real broker and all worth recording because each looked like something else first:

1. **`rename_all` on a tagged enum does not rename variant *fields*.** `BrokerConfig` shipped `bootstrap_servers` in snake_case while every neighbouring struct was camelCase — the identical failure CLAUDE.md already records for Epic 18's `Authorship`, resurfacing at the first multi-word variant field written since. `rename_all_fields = "camelCase"` is now on both `BrokerConfig` and `StartPosition`.
2. **`BrokerTransportFailure` against a Docker-mapped port is an IPv6 fallback, not a broken broker.** `localhost` resolves to both records, librdkafka tries `::1` first, and a Docker port mapping is IPv4-only. `broker.address.family = "v4"` is set in *production* code, not just tests — every environment this project targets is IPv4-only, and the alternative is a multi-second connect delay on every start.
3. **`seek` on a just-assigned partition fails with "erroneous state".** A partition is not fetching yet when `post_rebalance` runs, so the honouring of `StartPosition::Offset`/`Timestamp` had to move from "accept the default assignment, then seek" to "build the assignment you want and `assign` it" — the pattern librdkafka's own C examples use. Found only because a test asserted the *skipped* message was absent rather than that *a* message arrived.

**Slice C reports lag from a poll that runs independently of message processing**, which is the point: a stalled consumer is by definition not executing the apply path, so lag measured there would freeze at its last value exactly when it matters. Lag is `high_watermark - committed`, taken from the broker (`fetch_watermarks`), never estimated from what the client has locally buffered — and against the *committed* offset rather than `position()`, because a fetched-but-unapplied message is still outstanding work.

**Slice D's retry is in-place, not via redelivery.** Within a running consumer, librdkafka's position has already moved past a received message, so "retry" cannot mean "wait for the broker to send it again" — that only happens on restart, which would turn one bad message into "blocked until someone bounces the server." `poison_threshold` attempts happen in a loop around parse-and-apply; after them the message goes to `stream_dead_letters` (`V34`) **and then the offset commits**, which is correct precisely because the payload is preserved in full and replayable. If the DLQ write itself fails, the commit is skipped — the one case where losing the message is possible, and the one place decision 2's uncommitted-means-redelivered backstop is load-bearing.

Backpressure is `queued.max.messages.kbytes = 16384`, made explicit rather than inherited: librdkafka pauses fetching when its prefetch queue fills, and that pause *is* the "polling stops, memory does not grow" criterion. 16 MB because `00a`'s idle-RSS budget is 100 MB for the whole process and several concurrent subscriptions at the 64 MB default would exceed it on prefetch buffers alone.

**Slice E's replay isolation is structural, not a rule to remember.** `replay_window` connects with `graph-owl-replay-{uuid}` as its consumer group, and group membership is what owns committed offsets in Kafka — so a replay *cannot* move the live consumer's position, whatever it does. Revoke-safety is similarly structural: `pre_rebalance` runs on the same thread that drives `recv`, so it can never interleave with a message mid-apply, and it commits before releasing.

### Slice F: Pulsar parity

**Why last, not interleaved with A–E.** Kafka and Pulsar are different enough in their delivery model that building both consumers at once, slice by slice, would mean debugging two unfamiliar systems through the same five RED tests simultaneously — attribution becomes a guess. Slices A–E prove the whole pipeline (`StreamSubscription`, `ConsumerHealth`, offset-commit-after-apply, lag, poison quarantine, rebalance-and-replay) end to end against one broker first. This slice ports the same contract to Pulsar, translating each Kafka-shaped criterion to Pulsar's actual primitives rather than forcing Pulsar to imitate Kafka:

| Kafka concept (Slices A–E) | Pulsar equivalent used here |
|---|---|
| Consumer group, partition assignment | **`Key_Shared` subscription** — not `Shared` (no ordering guarantee) and not `Failover` (idles standbys instead of splitting work); `Key_Shared` routes by the same message key consistently, which is Kafka's partition-key behaviour under a different name |
| Offset, committed after apply | **Cursor ack**, sent after apply — `consumer.ack(&msg)` only once `process_inbound_event`-equivalent apply succeeds, same ordering as decision 2 |
| `stream_consumer_lag{topic,partition}` | **Subscription backlog** (unacked message count) via the Pulsar admin API — Pulsar has no partition offset delta to compute a lag *number* from; backlog is the broker-reported equivalent decision 3 asks for |
| Hand-rolled poison-message DLQ (Slice D) | **Native `dead_letter_policy`** (`max_redeliver_count`) — Pulsar's client has built-in dead-letter-topic support, so this is *thinner* than the Kafka path, not a reimplementation of it |
| Partition revoke/assign on rebalance | **Key redistribution** when a consumer joins or leaves a `Key_Shared` subscription — no explicit revoke/assign callback exists in the Pulsar client the way `rdkafka`'s `rebalance_cb` does; correctness is proven the same way Slice E proves it for Kafka (two consumers, exactly-once-applied assertion), not by a callback-shaped test that Pulsar has no callback to satisfy |

**Acceptance criteria**: the same eight epic-level criteria (see above), proven against a Pulsar testcontainer instead of Kafka; `BrokerConfig::Pulsar { .. }` selects this path; `Catalog`-facing behaviour (mapping, dedup, resolution, dead-letter visibility) is identical regardless of which broker delivered the message — verified by running the *same* facade-level test bodies parameterized over both broker kinds, not a second hand-written test suite that could silently drift from the first.
**RED**: The two-consumer exactly-once test from Slice E, re-run against a `Key_Shared` Pulsar subscription. A backlog test analogous to Slice C's lag test. Mutator watch: acking before apply must fail the same kill-and-restart shape of test Slice B already established, now against Pulsar's cursor instead of Kafka's offset.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped, with one criterion honestly unmet.** `PulsarConsumer` mirrors `KafkaConsumer`'s connect/recv/commit shape so `graph-owl-server`'s orchestration is one code path over two brokers (a `StreamConsumer` enum, not a trait object: the set is closed at two, both are constructed in one place, and a trait would need async-fn-in-trait-objects for `recv` — real complexity for a third implementation nobody has proposed). `Key_Shared` is the subscription type, not `Shared` (no ordering guarantee, and Epic 18's out-of-order design depends on per-key order) and not `Failover` (idles standbys instead of splitting work).

**Not met: at-least-once on the Pulsar path.** Pulsar's ack takes the *message value*, which cannot survive being handed back through a cross-broker signature carrying only coordinates — so the ack happens inside `recv` rather than after a successful apply. The consequence, stated plainly rather than papered over: **Pulsar is at-most-once-per-delivery where Kafka is at-least-once** — a crash between ack and apply loses that message instead of redelivering it. Closing it means holding the un-acked message across the apply, which is a different orchestration shape than Slices A–E built; it is a known gap, not an oversight.

**Also not met: Pulsar lag.** Backlog lives on Pulsar's admin REST API — a separate HTTP surface on a different port from the binary protocol this consumer speaks. `StreamConsumer::lag` returns `None` for Pulsar and the periodic poller stops rather than reporting a fabricated zero, which would be worse than reporting nothing.

## Explicitly deferred (with destination)

- **RDF stream processing (C-SPARQL, RSP-QL)** → continuous queries over streams are out of scope; this epic ingests, it does not query streams.
- **Exactly-once via transactional producer** → at-least-once plus dedup is simpler and sufficient; revisit only if a source cannot supply a dedup key.
- **Schema-registry-driven mapping** (Avro schema → mapping automatically) → attractive; needs Epic 27's contracts first.
- **Brokers beyond the Kafka protocol** (SQS, Pub/Sub, NATS) → each is a `BrokerConfig` variant; add on demand.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. Integration tests run against a real broker via testcontainers, not a stub — Kafka for Slices A–E, Pulsar for Slice F.
5. Kill-and-restart test verified (Slice B) — this is where silent data loss lives.
6. Sustained-overload memory bound asserted (Slice D).
7. Slice F's facade-level tests are the *same test bodies* run against both broker kinds, not a parallel hand-written suite.
