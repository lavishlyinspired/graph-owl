# Plan: Bolt Protocol Server (Epic 7d) ★

**Branch**: feat/engine-bolt
**Status**: Not started
**Ships after Phase 2**: this epic requires Epic 12's authentication, so it is the one Phase-1 epic that lands later — a second listening port with no identity to bind a session to is not a thing to ship. Feature flag stays off until then.
**Depends on**: Epic 7b (Cypher), Epic 7c (LPG projection), Epic 12 (auth), Epic 13 (authorization)
**Crates**: **`graph-owl-bolt`** (new — wire protocol, feature-gated, off by default)

## Goal

Speak the property-graph wire protocol, so every existing driver, browser, notebook integration, BI connector, and visualization tool in that ecosystem connects to graph-owl without anyone writing an adapter.

## Why this is the highest-leverage integration in the roadmap

Every other integration in this project costs one unit of work per integration: a connector per source (Epic 15), a format per serialization (Epic 9). Bolt costs **one unit of work for the entire ecosystem** — official drivers for Java, Python, JavaScript, Go, .NET, and Rust; graph browsers and visual explorers; the visualization products surveyed in the reference material; notebook and BI connectors. None of them need to know graph-owl exists beyond a connection URI.

It is also the cheapest way to make the LPG claim real. A property-graph model nobody can connect to with their existing tools is a data-model choice; one that answers on the standard port is a product capability.

The competitive point is sharper still: a client connects with the driver it already uses, runs the Cypher it already knows — and gets **time-travel, OWL 2 RL inference, and SHACL-style validation** (Epics 4, 6, 5) that the database it thinks it is talking to does not have.

## Resolved decisions

1. **Server only, never a client.** graph-owl answers Bolt; it does not connect out over Bolt. Pushing data *to* an external property-graph store is Epic 9a and uses a driver there, deliberately kept out of this crate so the protocol implementation has one direction to be correct in.
2. **Read-only, consistent with every other query surface.** `CREATE`/`MERGE`/`SET`/`DELETE` are rejected (Epic 7b decision 3). Writes go through the catalog API so validation, versioning, and authorization apply.
3. **Feature-gated, off by default.** It opens a second listening port. A deployment that does not want one must be able to compile it out, and the `00a` operational budget means the default configuration is the small one.
4. **Authentication reuses Epic 12, not a parallel user store.** Bolt's `HELLO` carries credentials; they resolve to the same `Principal` as an HTTP request. A second identity path would be a second thing to get wrong, and the wrong one is the one nobody audits.
5. **Authorization is the same compiled predicate as SPARQL and Cypher.** Epic 7b Slice E's cross-language equivalence test extends to a third surface. Three query surfaces disagreeing about what a principal may see is three chances at a data leak.
6. **Protocol version negotiation supports a declared range, and refuses outside it.** Silently accepting an unsupported version and then failing on an unknown message is a worse experience than refusing the handshake with a version list.
7. **One connection is one session is one authorization context.** No credential switching mid-connection.

## Implementation reference

```rust
// graph-owl-bolt
pub struct BoltServer {
    query: Arc<dyn QueryEngine>,       // Epic 7 — same engine as HTTP
    lpg:   Arc<dyn LpgProjection>,     // Epic 7c
    auth:  Arc<dyn Authenticator>,     // Epic 12
    authz: Arc<dyn Authz>,             // Epic 13
    limits: BoltLimits,
}

pub struct BoltLimits {
    pub max_connections: usize,
    pub max_message_bytes: usize,      // refuse oversized frames before allocating
    pub query_timeout: Duration,
    pub fetch_batch_size: usize,       // records per PULL
}

// PackStream: the binary serialization Bolt carries
pub trait PackStreamCodec {
    fn encode(&self, v: &BoltValue, out: &mut BytesMut) -> Result<(), CodecError>;
    fn decode(&self, buf: &mut BytesMut) -> Result<Option<BoltValue>, CodecError>;
}

pub enum BoltValue {
    Null, Boolean(bool), Integer(i64), Float(f64), Bytes(Vec<u8>), String(String),
    List(Vec<BoltValue>), Dictionary(BTreeMap<String, BoltValue>),
    Node(BoltNode), Relationship(BoltRelationship), Path(BoltPath),  // from Epic 7c
    Date(..), Time(..), DateTime(..), Duration(..), Point(..),
}
```

### Connection state machine

```
                 ┌──────────┐
   TCP + magic   │NEGOTIATION│  version handshake: 4 offers, server picks or refuses
                 └─────┬─────┘
                       │ HELLO + credentials  ──► Epic 12
                 ┌─────▼─────┐
                 │  AUTHED   │
                 └─────┬─────┘
                       │ RUN
                 ┌─────▼─────┐  PULL / DISCARD ──► streams records
                 │ STREAMING │
                 └─────┬─────┘
             FAILURE   │   SUCCESS
                 ┌─────▼─────┐
                 │  FAILED   │  every message but RESET is ignored until RESET
                 └───────────┘
```

**`FAILED` ignoring everything until `RESET` is the state clients depend on.** A server that keeps accepting messages after a failure lets a driver's pipelined batch run half-executed, and the driver has no way to know which half. This is the single most commonly mis-implemented part of the protocol and gets its own slice.

### Streaming

`PULL n` must stream `n` records, not materialize the result and slice it. A query returning a million rows through a driver that pulls 1,000 at a time must not allocate a million rows server-side. The `QueryEngine` result is consumed as an async stream; back-pressure is the socket's.

## Acceptance criteria

- [ ] Handshake negotiates within the declared version range and refuses outside it with a version list.
- [ ] `HELLO` authenticates via Epic 12, producing the same `Principal` an HTTP request would.
- [ ] `RUN`/`PULL`/`DISCARD`/`BEGIN`/`COMMIT`/`ROLLBACK`/`RESET`/`GOODBYE` behave per the state machine.
- [ ] `FAILED` ignores all messages except `RESET`.
- [ ] PackStream encodes and decodes every `BoltValue` variant, including structures and temporals.
- [ ] Nodes, relationships, and paths carry Epic 7c element ids, labels, types, and properties.
- [ ] Results **stream**; a large result set does not scale server memory with row count.
- [ ] Write clauses are refused with a message naming the catalog API.
- [ ] Authorization matches SPARQL and Cypher exactly for the same logical question.
- [ ] An off-by-default feature flag compiles the listener and its dependencies out entirely.
- [ ] Oversized frames are refused before allocation.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: PackStream codec (pure)

**Acceptance criteria**: every `BoltValue` variant round-trips; all integer width markers encode at their **smallest** representation and decode from any width; strings, lists, and dictionaries encode at each size-class boundary (tiny / 8 / 16 / 32); structures encode with signature and field count; a truncated buffer returns `Ok(None)` — needs more data — not an error; a declared length exceeding `max_message_bytes` is refused **before** allocating; decoding never panics on arbitrary bytes.
**RED**: Size-class boundary tests at each transition — the off-by-one at 15/16 and 255/256 is the classic PackStream bug and produces corrupt frames only under specific payload sizes. A fuzz-style corpus of arbitrary bytes asserting no panic. An allocation-guard test with a declared length of 2^31 and a two-byte buffer. Mutator watch: a wrong boundary constant must fail its case; allocating from the declared length before checking must fail the guard test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Handshake and authentication

**Acceptance criteria**: correct magic preamble accepted, wrong preamble closes the connection without a reply; four version offers, server selects the highest supported; an all-unsupported offer is refused with the supported list; `HELLO` with valid credentials → `SUCCESS` with server metadata; invalid credentials → `FAILURE` and connection close; the resulting `Principal` is byte-identical to the one an HTTP request with the same credentials yields; a missing `user_agent` is tolerated.
**RED**: The identity-equivalence test against the HTTP path is the important one — decision 4 exists because a divergent identity path is the one nobody audits. A refusal test asserting the supported-version list is returned, not a bare close. Mutator watch: a parallel credential check must fail the equivalence test; accepting an unsupported version must fail the refusal test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: The state machine, including `FAILED`

**Value**: The correctness core. Drivers rely on these transitions precisely.
**Acceptance criteria**: every legal transition in the diagram; an illegal message for the current state → `FAILURE`, not a panic or a silent ignore; after a `FAILURE`, `RUN`/`PULL`/`DISCARD` are **ignored** and `RESET` returns to `AUTHED`; `GOODBYE` closes cleanly at any state; `RESET` from any state succeeds; an explicit transaction (`BEGIN`/`COMMIT`) is honoured, and `ROLLBACK` discards; a dropped socket mid-stream releases resources.
**RED**: A pipelined-batch test: send `RUN`, an erroring `RUN`, then `PULL`, all without waiting — assert the post-failure messages are ignored and only `RESET` recovers. This is the failure mode decision-note above describes, and it is invisible in request-response testing. A resource-release test asserting no leak after an abrupt disconnect. Mutator watch: processing messages in `FAILED` must fail the pipelining test; a panic on an illegal message must fail the state coverage.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Query execution streams results

**Acceptance criteria**: `RUN` parses Cypher via Epic 7b and executes via Epic 7's engine — no second execution path; `PULL n` returns at most `n` records then `SUCCESS` with `has_more`; `PULL -1` drains; `DISCARD` consumes without transmitting; a 100k-row result under `fetch_batch_size` 1000 holds **bounded** server memory, asserted by measurement not inspection; `query_timeout` produces `FAILURE` with a timeout code and frees resources; nodes and relationships carry Epic 7c element ids, labels, types, and properties; a path result carries alternating nodes and relationships.
**RED**: The bounded-memory test is the one that catches a materialize-then-slice implementation, which passes every functional test. Measure peak RSS or use an instrumented allocator across the 100k-row pull. Mutator watch: collecting the full result before paging must fail it; ignoring `n` must fail the batch-size assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Authorization and write refusal

**Acceptance criteria**: the same logical question asked over Bolt, Cypher-over-HTTP, and SPARQL under a restricted principal returns identical results; a write clause → `FAILURE` naming the catalog API endpoint to use instead; a query the principal may not answer returns empty, not an error code that leaks existence; `max_connections` is enforced and the refusal is a clean protocol-level rejection; per-connection limits cannot be raised by the client.
**RED**: Extend Epic 7b Slice E's cross-language equivalence test to three surfaces. A divergence here is a data leak, and it is exactly the kind that appears when a third surface is added late. Mutator watch: bypassing the compiled predicate on the Bolt path must fail the three-way equivalence.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Real drivers connect

**Value**: The claim is "existing tooling works", and only a real driver can demonstrate that.
**Acceptance criteria**: an integration test using an off-the-shelf property-graph driver connects, authenticates, runs a query, and reads typed results; node labels, relationship types, and properties arrive correctly typed in the driver's own object model; temporal values survive; an explicit transaction works; a driver-side timeout is handled cleanly on both ends; the feature flag off → the crate and its dependencies do not compile in, asserted by a dependency-tree check, and the port is not opened.
**RED**: The driver test is the acceptance of the whole epic — a hand-rolled client can be wrong in the same way the server is, and prove nothing. The compile-out assertion is what keeps decision 3 honest. Mutator watch: a subtly wrong structure signature passes a hand-rolled test and fails a real driver, which is the point.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Bolt writes** → never (decision 2), same reasoning as SPARQL Update and Cypher writes.
- **Routing / cluster discovery** → single-node deployment (`00a-product-position.md`). The protocol's routing messages are refused explicitly rather than half-answered.
- **Bolt client** → Epic 9a, for pushing to external property-graph stores.
- **HTTP query API in the same protocol dialect** → graph-owl's own REST and `/cypher` (Epic 7b) already cover it.
- **Gremlin / TinkerPop server** → see `00e-crate-architecture.md`. Bolt reaches the larger tool population; a second wire protocol needs its own demand signal.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. The codec and the state machine are pure and must be exhaustive.
2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Three-way authorization equivalence verified** (Slice E).
5. **Bounded memory under a large streamed result verified by measurement** (Slice D).
6. **Feature-off build asserted to exclude the crate and its deps** (Slice F).
7. Decoder fuzzed against arbitrary input with no panics (Slice A).
