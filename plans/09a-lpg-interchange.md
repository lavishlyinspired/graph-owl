# Plan: Property-Graph Interchange & External Store Sync (Epic 9a)

**Branch**: feat/lpg-interchange
**Status**: Not started
**Depends on**: Epic 7c (LPG projection), Epic 9 (RDF I/O — shares the streaming-serializer shape)
**Crates**: **`graph-owl-lpg-io`** (new — per-format and per-driver features)

## Goal

Move the property graph in and out: standard interchange files for archival and tooling, and one-way projection into an external property-graph store for teams that already run one.

Symmetric with Epic 9, which does exactly this for the RDF serializations. Same crate shape, same streaming discipline, different formats.

## Resolved decisions

1. **Export is the primary direction; import is narrow.** Exporting the catalog graph serves visualization, archival, offline analysis, and migration *out* — the credibility case for "your data is not trapped here." Import exists for one job: seeding graph-owl from an existing property graph. It is not a general ingestion path; that is Epic 15/16.
2. **Sync to an external store is one-directional and lossy by design.** graph-owl → external. Never back. A bidirectional sync between two graph databases is a distributed-consistency project, and Epic 4 decision 1 already spent this system's consistency budget on the relational/flake split.
3. **The external store is a projection target, not a backend.** It is never read from to answer a query. If it drifts, it is re-projected. Same one-directional invariant that makes the flake reconciliation safe.
4. **One `Connector` trait for reading external property graphs, in `graph-owl-connectors`.** Reading Neo4j, Memgraph, FalkorDB, TigerGraph, or Kùzu *as a source* is a connector, not a new crate each — the 100-connectors rule from `00e-crate-architecture.md`. This epic owns the *write* direction; Epic 15 owns the read.
5. **Streaming serialization, never build-then-write.** A 10M-element export must not materialize. Same requirement as Epic 9, and the reason both crates exist rather than a `serde` call site.
6. **Bulk CSV export targets the documented bulk-import shape** of the major property-graph stores — separate node and relationship files with a header line declaring ids, labels, types, and typed properties. This is the only path that loads tens of millions of elements in reasonable time; a generated Cypher script is not.
7. **Every driver is a separate cargo feature.** A deployment exporting GraphML must not compile a database driver.

## Implementation reference

```rust
pub trait LpgWriter {                 // streaming, per decision 5
    fn begin(&mut self, meta: &ExportMeta) -> Result<(), IoError>;
    fn node(&mut self, n: &LpgNode) -> Result<(), IoError>;
    fn edge(&mut self, e: &LpgEdge) -> Result<(), IoError>;
    fn finish(self) -> Result<ExportSummary, IoError>;
}

pub trait LpgReader {
    fn read(&mut self) -> Result<Option<LpgElement>, IoError>;   // pull-based
}

#[async_trait]
pub trait GraphProjectionTarget: Send + Sync {   // decision 2: one direction
    async fn project(&self, batch: &ElementBatch) -> Result<ProjectionAck, TargetError>;
    async fn checkpoint(&self) -> Result<Checkpoint, TargetError>;
    async fn reset(&self, scope: &ProjectionScope) -> Result<(), TargetError>;
}
```

### Formats

| Format | Direction | Feature | Chosen because |
|---|---|---|---|
| **GraphML** | out + in | `graphml` | The one interchange format essentially every graph tool and visualizer reads |
| **Bulk CSV** | out | `csv` | The only shape that loads at scale (decision 6) |
| **Cypher script** | out | `cypher-script` | Human-readable, diffable, works against any openCypher store; slow, and labelled so |
| **JSON Graph** | out | `json` | What the Epic 40 explorer and web tooling consume |
| **JSON Lines** | out + in | `jsonl` | Streaming-native, one element per line, resumable |

Deliberately **not** here: RDF serializations (Epic 9), and any vendor-proprietary dump format — those are read via a connector, never written.

### Targets

| Target | Feature | Mechanism |
|---|---|---|
| Bolt-speaking stores (Neo4j, Memgraph) | `bolt-target` | Bolt client, batched parameterized `UNWIND` writes |
| FalkorDB | `falkor-target` | Redis protocol, `GRAPH.QUERY` |
| Files | always | The writers above |

FalkorDB is listed separately because it speaks the Redis wire protocol rather than Bolt — the same Cypher lands over a different transport, which is exactly the kind of detail that turns into a leaky abstraction if the trait pretends otherwise. `GraphProjectionTarget` is the seam; the transport is the adapter's business.

### Idempotency

Projection is re-runnable. Elements are written with `MERGE` on the Epic 7c element id, so a re-projection after a partial failure converges rather than duplicating. `checkpoint` records the last successfully projected transaction time so an incremental projection resumes rather than restarting.

## Acceptance criteria

- [ ] Every format above exports; GraphML and JSON Lines also import.
- [ ] Export **streams** — memory is bounded regardless of graph size, verified by measurement.
- [ ] GraphML export → import round-trips losslessly, including typed property declarations.
- [ ] Bulk CSV output loads into a real target store without hand-editing.
- [ ] Projection to an external store is idempotent; re-running converges.
- [ ] `checkpoint` enables incremental projection from the last transaction time.
- [ ] Projection never reads from the target to answer a query — asserted structurally.
- [ ] Each format and driver is behind its own feature; the default build includes no driver.
- [ ] Authorization applies: an export runs as a principal and contains nothing they cannot read.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Streaming GraphML export

**Acceptance criteria**: valid GraphML with `<key>` declarations for every property encountered, correct types; nodes before edges; labels and edge types emitted per the format's conventions; XML special characters escaped, including in property values and ids; a 1M-element export holds bounded memory, measured; the output validates against the GraphML schema.
**RED**: An escaping test with `<`, `&`, quotes, and a null byte in a property value — a description field containing markup is completely ordinary in a metadata catalog, and unescaped output produces a file that fails to parse at the far end. The bounded-memory test catches build-then-write. Mutator watch: skipping escaping must fail; collecting into a `String` before writing must fail the memory measurement.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: GraphML import and round-trip

**Acceptance criteria**: import produces `LpgNode`/`LpgEdge` values; declared key types are honoured, not string-guessed; export → import → export is byte-identical over a fixture covering every property type; an edge referencing an undeclared node → a reported error naming the id; a malformed document reports line and column; import is streaming.
**RED**: The byte-identical round trip is the specification. A type-fidelity test: an integer property must not return as a string — a silent widening breaks every downstream comparison. Mutator watch: ignoring `<key>` types must fail type fidelity; tolerating a dangling edge reference must fail the error test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Bulk CSV and Cypher script

**Acceptance criteria**: separate node and relationship files with typed headers per the documented bulk shape; array properties use the documented separator, with escaping when a value contains it; a real target store's bulk importer accepts the output unmodified; the Cypher script is parameterized, batched, and idempotent via `MERGE`; the script is labelled slow, with the CSV path named as the alternative at scale.
**RED**: The separator-escaping test — an array property whose value contains the array separator is the bug that silently splits one value into two, corrupting data with no error anywhere. An end-to-end test running the real importer. Mutator watch: unescaped separators must fail; `CREATE` instead of `MERGE` must fail idempotency.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: One-directional projection to an external store

**Acceptance criteria**: `GraphProjectionTarget` implemented for a Bolt-speaking store; batched `UNWIND` writes, not per-element round trips; idempotent — projecting twice yields one copy; a mid-batch failure leaves a consistent checkpoint and re-running converges; `reset` clears a scope; a structural test asserts **nothing in the query path reads from a projection target** (decision 3); the target's own schema/indexes are created once, idempotently.
**RED**: The structural no-read test is the guard on decision 3 — the moment a query reads the projection, graph-owl has a second source of truth and a drift class it cannot detect. A partial-failure-then-resume test. Mutator watch: per-element writes must fail the batching assertion; a read from the target must fail the structural test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Incremental projection and authorization

**Acceptance criteria**: `checkpoint` records the last projected transaction time; a subsequent projection sends only elements after it; a retraction since the checkpoint removes the element from the target rather than leaving it stale; an export or projection runs as a principal and omits everything Epic 13 denies; an unauthorized export is empty, not an error; the checkpoint is per target and per scope.
**RED**: The retraction test: an incremental projection that only ever adds leaves deleted assets visible in the target forever — a governance failure with a plausible-looking cause. The authorization test asserting a restricted principal's export omits denied entities. Mutator watch: ignoring retractions must fail; exporting unfiltered must fail the authorization test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: JSON Graph and JSON Lines

**Acceptance criteria**: JSON Graph output is exactly what the Epic 40 explorer consumes — asserted against that consumer's fixture, not an invented shape; JSON Lines streams one element per line and imports resumably from an arbitrary line; a truncated final line is reported, not silently dropped; both formats carry `_graph` and `_t` from Epic 7c so derived and historical elements stay distinguishable.
**RED**: The shared-fixture test with Epic 40 is what stops the export format and the UI's expectation from drifting. A truncated-file test — a partial write is the normal failure mode for a streaming export. Mutator watch: silently dropping a truncated line must fail; omitting `_graph` must fail the derived-element assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Bidirectional sync with an external store** → never (decision 2).
- **Reading external property graphs as sources** → Epic 15, as connector modules.
- **Vendor-proprietary dump formats** → read via connector; never written.
- **HDT / binary compressed interchange** → `00e-crate-architecture.md` rejects a compression crate; revisit only if export size becomes a real complaint.
- **Live change-data-capture into the target** → Epic 18's event stream is the natural carrier; needs the incremental path (Slice E) proven first.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Bounded memory verified by measurement on a 1M-element export** (Slice A).
5. **A real target store's bulk importer consumes the CSV output unmodified** (Slice C).
6. **Structural assertion that no query path reads a projection target** (Slice D).
7. Default build contains no database driver — asserted by dependency tree.
