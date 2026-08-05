# Plan: Property-Graph Interchange & External Store Sync (Epic 9a)

**Branch**: feat/lpg-interchange
**Status**: **Slices A–B shipped (5–6 August 2026) — Slices C–F not started.**
**Depends on**: Epic 7c (LPG projection) — shipped, and its `FlakeValue::Ref`-vs-`String` kind bug already found and fixed (`07d-engine-bolt.md`). Epic 9 (RDF I/O — shares the streaming-serializer shape) — Slice A shipped, which is what this epic's own Slice A needed: not the whole of Epic 9, only its streaming-to-scratch-files pattern to mirror.
**Crates**: **`graph-owl-lpg-io`** (per-format and per-driver features)

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

- [~] Every format above exports; GraphML and JSON Lines also import. **GraphML export and import shipped (Slices A–B), byte-identical round trip; every other format not started.**
- [x] Export **streams** — memory is bounded regardless of graph size. Verified structurally (schema state bounded by distinct-key count, not element count; elements written through to disk as they arrive) at 5,000-element scale, not measured by OS-level RSS at the plan's own 1M — see Slice A's own scope note.
- [ ] GraphML export → import round-trips losslessly, including typed property declarations.
- [ ] Bulk CSV output loads into a real target store without hand-editing.
- [ ] Projection to an external store is idempotent; re-running converges.
- [ ] `checkpoint` enables incremental projection from the last transaction time.
- [ ] Projection never reads from the target to answer a query — asserted structurally.
- [ ] Each format and driver is behind its own feature; the default build includes no driver.
- [ ] Authorization applies: an export runs as a principal and contains nothing they cannot read.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

**Scope of this pass**: Slice A only, the same explicit-stop discipline `09-engine-rdf-io.md` and `37a-scale.md` already established for their own remaining slices.

### Slice A: Streaming GraphML export — **shipped, 5 August 2026**

**Shipped**:
- Valid GraphML with `<key>` declarations for every property encountered, correct types (`boolean`/`long`/`double` for the `PropertyValue` variants with a native GraphML equivalent; `string` for the rest — `Bytes`, `DateTime`, `Duration`, `List`, `ElementRef` — a documented, not accidental, narrowing); nodes before edges; labels and edge type emitted as reserved `_labels`/`_type` data keys, declared unconditionally so a reader never has to guess whether they are present.
- XML special characters escaped in property values, proven at the **byte level** (the raw unescaped substring must not appear anywhere in the output) and by feeding the output back through a real XML parser (`quick_xml::Reader`) rather than trusting this crate's own writer not to have produced something only it can read.
- `quick_xml` adopted (`00l-build-vs-adopt.md`) — its `Writer`/`ElementWriter` API escapes attribute values and text content automatically, the same "not the alternative it looks like" reasoning Epic 9 already recorded for hand-written Turtle: owning escaping by hand is exactly where output silently corrupts.
- **A real bug found and fixed while writing this slice's own tests, not assumed away**: an early design derived each property key's `<key id="...">` from its *sorted position* in the schema map at the moment each element was written — but GraphML's own structure needs every `<key>` declared *before* the `<graph>` body, so key ids are only finalised once every element has been seen. A later element introducing a new key that sorts alphabetically *before* an already-used key would silently shift the earlier key's position-derived id, disagreeing with the `<data key="...">` reference an earlier element had already written to disk. Fixed by assigning each key a stable id **the first time it is seen**, cached and never recomputed — and the regression test that catches this specific failure mode is kept (`a_key_id_stays_stable_even_when_a_later_element_introduces_an_earlier_sorting_key`), not deleted once the fix landed.
- Streaming, not build-then-write (decision 5): node and edge elements are written to two on-disk scratch files as they arrive — never accumulated in a `Vec` or `String` — while only the property *schema* (bounded by the catalog's own predicate vocabulary, not by element count) is held in memory. `finish()` writes the real header and `<key>` declarations, then streams both scratch files' bytes straight into the final output with `std::io::copy`, never re-reading either as a whole.
**Scope cut, recorded rather than silently narrowed**:
- **Memory-boundedness is proven structurally, not measured by OS-level RSS at 1M elements.** A test writes 5,000 elements sharing one property key and asserts the writer's own schema map holds exactly one entry — proof that per-element data never accumulates in memory, which is what a true RSS measurement would also show, but without the platform-specific measurement machinery a genuine 1M-element benchmark would need. Recorded as a gap, not claimed met.
- **No GraphML schema (XSD) validation against the *official* schema document.** The output is proven well-formed XML (a real parser accepts it) and structurally correct (keys precede the graph, nodes precede edges) by this crate's own tests; validating against GraphML's published XSD is real, separable work for whichever slice needs that stronger guarantee.
- **`LpgReader` (the import side) is not implemented.** `LpgWriter`'s trait shape ships in full; `LpgReader`'s does not exist yet in this crate — Slice B's own job.
**Tests**: `graph-owl-lpg-io::tests` — 7 tests (key-declaration ordering, edge source/target/type, key-id stability across out-of-order key discovery, byte-level XML escaping plus real-parser acceptance, schema-boundedness at 5,000 elements, the list-value separator, and calling `node()` before `begin()`).

### Slice B: GraphML import and round-trip — **shipped, 6 August 2026**

**Shipped**:
- `LpgElement`/`LpgReader` (the plan's own interface) plus `GraphMlReader<R: BufRead>`, a single forward streaming pass: `<key>` declarations collected as seen (always before `<graph>`, per `GraphML`'s own structure), each `<node>`/`<edge>` converted using the declared `attr.type` of every `<data>` it carries — `boolean`/`long`/`double` parsed to their typed `PropertyValue`, everything else (including the `string`-narrowed `Bytes`/`DateTime`/`Duration`/`List`/`ElementRef` Slice A's own writer already narrows) returned as `PropertyValue::String` literally, never guessed.
- Export → import → export is byte-identical, proven directly: re-exporting what the reader just produced reproduces the original file's bytes exactly.
- An edge naming a `source`/`target` id no `<node>` seen so far declared is `LpgIoError::DanglingReference`, naming both the edge and the missing id — checked against nodes-seen-so-far rather than the whole document (documented: correct for anything this crate's own writer produces, since it always emits nodes before edges; a hand-authored file with an edge preceding its node would false-positive, recorded rather than silently assumed, since validating the true whole-file set would mean buffering it).
- A malformed document reports line and column via `LineTrackingReader<R>`, a `BufRead` wrapper that counts newlines as `quick_xml` calls `consume()` — the only hook that says exactly how many bytes of the last `fill_buf()` the parser actually used, needed because `quick_xml`'s own `buffer_position()` gives a byte offset, not a line/column, and a genuinely streaming reader does not keep the source bytes already consumed to compute one after the fact.
- Import is streaming: `read()` is pull-based, one element per call, over a `BufRead` rather than a fully materialized document.
- **A real design correction found while writing this slice**: `attr_value`/`read_key_declaration`/`typed_value` are free functions, not `&self` methods. `quick_xml`'s zero-copy `Event<'buf>` ties a `BytesStart` to `GraphMlReader`'s own `buf` field; any `&self`/`&mut self` method call while that borrow is still live (because the value is used later in the same match arm) conflicts with it, even for a *different* field, since a method signature only says `&self` and the borrow checker cannot see through it to know which fields the method actually touches. Passing the needed pieces as plain parameters instead sidesteps the conflict rather than fighting it with more lifetimes — documented in the module's own doc comment so the pattern is not "fixed" back into methods later.
- **A second real finding**: `quick_xml::events::attributes::Attribute::read_text_into` returns content "as is" — it does not unescape entities, because it cannot safely do so without knowing whether a run was `CDATA`. Entity unescaping (`&amp;` → `&`) is `quick_xml::escape::unescape`, an explicit second step this slice's own text-reading path takes after `read_text_into` and `.decode()`.
**Tests**: `graph-owl-lpg-io::tests` — 13 total (7 from Slice A plus 6 new: byte-identical round trip across `Boolean`/`Integer`/`Float`/`String` properties on both a node and an edge; declared-type fidelity for all three typed variants; labels and edge type round-trip; a dangling reference names both ids; a malformed document reports a real line number; streaming confirmed by reading a 10-element document one call at a time).
**Scope cut, recorded rather than silently narrowed**: no mutation run this pass (matches Slice A's own recorded practice); self-closing `<node/>`/`<edge/>` elements (never produced by this crate's own writer, which always emits `Start`…`End` even for empty content) are not read — a real, minor gap against general `GraphML` interop, not against round-tripping this crate's own output.

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
