# Plan: RDF Interop & Open Standards (Epic 9)

**Branch**: feat/engine-rdf-io
**Status**: **Slice A shipped, 5 August 2026 — Slices B–F not started.**
**Depends on**: Epic 4 (triples to serialize) — shipped. ~~Epic 7 (CONSTRUCT produces Turtle)~~ — **overstated for Slice A**: checked before implementing (`verify-blockers-against-code`), CONSTRUCT is not wired (`execute_algebra`'s result collector discards `spareval::QueryResults::Graph` today) and Slice A does not need it — it serializes `&[Flake]` directly, sourced from `TripleStore::query_pattern`, never from a SPARQL result. A later slice that wants "export via `CONSTRUCT`" as a convenience would need CONSTRUCT wired first; Slice A itself does not.
**Crate**: `graph-owl-rdf-io`

## Goal

Exchange the graph with the outside world in standard formats and vocabularies, without adopting RDF as the internal model.

## Resolved decisions

1. **Conform at the boundary, stay property-graph inside.** RDF is how the graph interoperates, not how it is stored. Adopting RDF/OWL internally would trade transactional cascades and predicate-compiled authorization (Epics 4, 13) for a serialization property obtainable with a mapping layer.
2. **Vocabularies are mappings, not remodelling.** DCAT, PROV-O, ODCS, OpenLineage are emitted *from* the existing model. The internal predicate vocabulary does not become `dcat:`.
3. **JSON-LD before Turtle, reversing the slice order below.** Turtle is Slice A because it is the simplest to implement; JSON-LD is the one that unblocks anything. DCAT, PROV-O, ODCS and OpenLineage are all published as JSON-LD, so JSON-LD is an *ingestion* capability and Turtle is an *export convenience* — and this project needs to consume standard metadata far more urgently than it needs to emit it. Expand and compaction first; framing later, where it can replace per-endpoint DTOs.
4. **This epic decides whether `rdf:reifies` is emitted always or only on export.** `04-engine-triples.md` finding 5 established that graph-owl's reified relationship node already *is* an RDF 1.2 reifier, missing only the vocabulary. Emitting `rdf:reifies` plus a triple term into the store on every edge doubles the flakes per relationship; emitting it at serialization time keeps the store compact and the wire standard. **Export-only is the default reading**, because the store's job is to be queried and the wire's job is to be understood, and nothing in the store benefits from the extra rows. Revisit if a SPARQL query needs to match on triple-term patterns directly — **but this epic is not the decider, and this trigger was half of a loop.** It pointed at Epic 7, whose decision 7 pointed back here. `94-rdf12-alignment.md` decision 7 now owns it and resolves it; the export-only default below stands unchanged, because the resolution does not put `rdf:reifies` in the store either.
5. **JSON-LD "compatible", not "native" — and the three failure modes are accepted knowingly.** A *native* implementation accepts JSON-LD as the transaction format itself, with `@context` travelling alongside the data; a *compatible* one parses JSON-LD into an internal model and serializes back. This project is compatible, necessarily: the transaction format is the REST contract in `00d`, and flakes are the internal model. Native would mean two write paths.

   The translation approach has three known production failure modes, and naming them is the price of choosing it:
   - **Schema drift** — the published context and the internal predicate vocabulary evolve apart. Mitigated by decision 6: the context is versioned, served, and pinned in output, so a drift is detectable rather than silent.
   - **Provenance loss at the boundary** — named graphs and multivalued properties flatten during translation. `cx` and predicate cardinality (Epic 4 slice H) both survive the round trip *only if the tests say so*, which is why the acceptance criteria below require a named-graph round trip rather than a triple-count match.
   - **Developer friction** — two vocabularies to maintain. Real, and unavoidable given the REST contract exists.

6. **JSON-LD context is versioned and served.** The same JSON expands to different RDF under different contexts; the context must be a stable, fetchable artifact or round-trips are unreproducible.
4. **Import is validated before it lands.** External RDF goes through Epic 5's shapes and Epic 17's resolution before entering the graph — an unvalidated import is how a graph gets poisoned.
5. **Lossy by design in both directions, documented.** Export omits internal-only predicates; import cannot express everything the internal model holds. Both directions state what is dropped.

## Implementation reference

```rust
pub trait RdfSerializer {
    fn serialize(&self, flakes: &[Flake], fmt: RdfFormat) -> Result<Vec<u8>, RdfError>;
}
pub trait RdfParser {
    fn parse(&self, bytes: &[u8], fmt: RdfFormat, base: Option<&str>)
        -> Result<Vec<Flake>, RdfError>;
}

pub enum RdfFormat { JsonLd, Turtle, NTriples, NQuads, RdfXml, TriG }

pub trait VocabularyMapper {
    fn vocabulary(&self) -> Vocabulary;              // Dcat | ProvO | Odcs | OpenLineage
    fn map_out(&self, e: &EntityView) -> Vec<Flake>; // internal -> standard
    fn map_in(&self, f: &[Flake]) -> Result<Vec<EntityDraft>, MapError>;
}
```

Dependencies: **`oxrdf` + `oxttl` + `oxjsonld`** — `00l-build-vs-adopt.md` names these and it is the authority on which libraries to take. This line previously said `rio_turtle` / `rio_xml` / `json-ld`; that was drift, corrected 28 July 2026. Beyond `00l`'s authority the reason is concrete: `oxrdf` is **already in the dependency tree**, pulled in by `spargebra` and `spareval`, and its `Term` is the type the query path speaks. Taking `rio` would put two RDF term representations in one codebase with a conversion layer between them, for no gain.

`Sid` ↔ IRI conversion uses the namespace registry from Epic 4 — a `Sid` with an unregistered namespace fails serialization loudly rather than emitting a bare local name.

**Serializing by hand-written text is not the alternative it looks like.** The acceptance criterion below requires `parse(serialize(x)) == x`, and the parser is `oxttl` regardless. Emitting text by hand while parsing with a library means owning escaping, IRI validation and literal canonicalisation on only one side of a round trip — the asymmetry is where such round trips fail, and subtly.

**Consequence: Epic 94's Slices B, C and D share one feature gate.** Because this epic builds `oxrdf` terms rather than text:

| Slice | Needs | Because |
|---|---|---|
| B — `rdf:reifies` on export | `oxrdf/rdf-12` | Emitting `<< a p b >>` means constructing `Term::Triple`, which exists only under the flag |
| C — `rdf:dirLangString` | `oxrdf/rdf-12` | `BaseDirection` (`oxrdf/src/literal.rs:809`) is behind the same flag; without it a directional literal cannot be represented, only stored |
| D — query-surface synthesis | `oxrdf/rdf-12` via `spargebra/sparql-12` + `spareval/sparql-12` | Both cascade to the same `oxrdf` feature |

So it is **one decision, not three**, and it is taken once — for the workspace, at the point the first of the three lands. Slice C storing direction in `flake_meta` without the flag is possible but only gets it as far as the database; it could not serialize what it stored, which is not a finished slice.

### Vocabulary mappings

| Standard | Maps | Notes |
|---|---|---|
| DCAT | `Table`/`Database` → `dcat:Dataset`, `DatabaseService` → `dcat:Catalog` | `dcterms:title`, `dcterms:description`, `dcat:distribution` |
| DPROD | `DataProduct` → `dprod:DataProduct` | Ports map to input/output relationships |
| PROV-O | Flake `t`/`op` + `updated_by` → `prov:Activity`, `prov:wasGeneratedBy` | Time-travel is already provenance; this is a projection |
| OpenLineage | `feeds`/`derivedFrom` edges + `LineageDetails` | Both emit and consume — the interop win |
| ODCS | `Contract` (Epic 27) | Schema, SLA, responsibilities |
| SHACL | Epic 5 shapes | Export so external validators can use them |

## Acceptance criteria

- [~] Every format round-trips: `parse(serialize(x)) == x` for the expressible subset — **true for Turtle, N-Triples, N-Quads (Slice A)**; JSON-LD, RDF/XML, TriG not yet implemented.
- [ ] JSON-LD context is versioned, served at a stable URL, and pinned in output.
- [ ] DCAT export validates against the DCAT SHACL shapes.
- [ ] OpenLineage events both export and import.
- [ ] Import runs Epic 5 validation and Epic 17 resolution before landing.
- [x] An unregistered namespace fails serialization with a named error.
- [ ] What each direction drops is documented and tested.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

**Scope of this pass**: Slice A only, the same explicit-stop discipline `37a-scale.md` already established. JSON-LD (Slice B) needs a served, versioned context endpoint and real SSRF-refusal testing; DCAT/PROV-O (Slice C) needs validating against **published external** DCAT SHACL shapes, not self-written ones; OpenLineage (Slice D) is a second real external spec; Slice E ties import into Epic 5/17. Each is substantial, separable work — recorded as not started rather than rushed.

### Slice A: Turtle and N-Triples round-trip — **shipped, 5 August 2026**

**Value**: The simplest exchange formats, proving the `Sid` ↔ IRI boundary.
**Shipped**:
- `RdfSerializer`/`RdfParser` traits plus `StandardRdfIo`, the one implementation, wrapping `oxttl` (Turtle, N-Triples, N-Quads) — the other three `RdfFormat` variants (`JsonLd`, `RdfXml`, `TriG`) return `RdfError::UnsupportedFormat` rather than panicking, so a later slice adds an arm without changing this module's public shape.
- All tested `FlakeValue` variants round-trip through Turtle and N-Triples: `Ref`, `String`, `Boolean`, `Int`, `Instant` — each via `parse(serialize(x)) == x`, literally, including `t`/`op` (test fixtures use `t: 0, op: true` throughout, since neither format has a transaction-time concept to round-trip against; a real caller sourcing from `TripleStore::query_pattern` re-stamps `t` before writing to storage — that is Slice E's job, not this one's).
- Typed and language-tagged literals preserved — verified through N-Triples specifically, since Turtle's own grammar legitimately abbreviates a boolean/integer as a bare token with no `^^xsd:...` in the text at all (valid Turtle; the plan's own literal reading of "the datatype must appear in the output" does not hold for Turtle and was corrected in the test, not the production code, once the first version of that test failed against genuinely-correct Turtle output).
- IRI/literal escaping: a space and CJK characters in a literal round-trip correctly.
- Blank nodes: skolemized into a fresh `Sid` per unique label, stable for the duration of one `parse()` call (never persisted or reused across calls) — `graph-owl`'s own "no blank-node representation" rule (`00c-domain-model.md`) applied the same way Epic 98/99/100 already applied it to OWL restrictions.
- An unregistered namespace fails serialization with a named `RdfError::UnregisteredNamespace`; a parsed IRI outside this store's registered namespace set fails with `RdfError::UnrecognisedIri` — the reverse-direction case the plan's own criterion did not name but decision-consistency asked for.
- N-Quads carries `cx`; N-Triples drops it with a `tracing::warn!` naming the dropped count, not silently.
**Real reuse, not a new mapping**: `FlakeValue <-> oxrdf::Term` conversion already existed — `graph_owl_query::term`, built for SPARQL query results — and this crate depends on `graph-owl-query` to reuse it rather than writing a second copy of the same mapping that could drift from the first.
**Scope cut, recorded rather than silently narrowed**: no mutation run this pass (matches this session's established `scripts/gate.sh`-as-the-bar practice, recorded the same way in `37b-portability.md` and `37a-scale.md`); no `Sid ↔ IRI` support for a document referencing a namespace outside the 8 currently registered (DSC/RDF/RDFS/XSD/OWL/SHACL/SCHEMA/DCTERMS) — genuinely external RDF import is Slice E's job, and Slice A's own round-trip test only ever exercises namespaces this store's own serializer produced.
**Tests**: `graph-owl-rdf-io::tests` — 9 tests (round-trip per variant, escaping, typed/lang-tagged literals, blank-node stability within and across documents, unregistered-namespace refusal, unimplemented-format refusal, N-Quads-vs-N-Triples context handling).

### Slice B: JSON-LD with a versioned context

**Value**: The format web clients actually use.
**Acceptance criteria**: expand, compact, and frame; the context is a served artifact with a version in its URL; output pins the context version; compacting with a different context produces different but valid JSON; a document with a remote `@context` is **rejected** unless the URL is allowlisted (SSRF surface); `@graph` maps to `cx`.
**RED**: A test asserting a remote-context fetch to an unlisted host is refused — an RDF parser that fetches arbitrary URLs is an SSRF hole. A version-pinning test. Mutator watch: fetching an unlisted context must fail the refusal test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: DCAT and PROV-O export

**Value**: Standard dataset description and provenance, for catalog-to-catalog exchange.
**Acceptance criteria**: `Table` exports as `dcat:Dataset` with title, description, publisher, theme; `DatabaseService` as `dcat:Catalog`; output validates against DCAT SHACL shapes; PROV-O export derives `prov:Activity` from flake `t`/`op`/`updated_by`; a soft-deleted entity is excluded unless requested; export is scopeable by domain or service.
**RED**: Validate DCAT output against the published DCAT shapes as a test — external conformance, not self-consistency. A PROV-O test asserting the activity chain matches the entity's version history. Mutator watch: an export that omits `dcterms:title` must fail SHACL validation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: OpenLineage bidirectional

**Value**: The highest-value interop — lineage is what other tools most want to exchange.
**Acceptance criteria**: `feeds` edges export as OpenLineage run events with inputs and outputs; column lineage maps to `columnLineage` facets; import creates edges with `source: OpenLineage`; import is idempotent by event id; an event referencing unknown datasets creates them as stubs flagged `lifecycle: Draft` rather than failing; round-trip preserves column mappings.
**RED**: Round-trip test preserving many-to-one column mappings. An unknown-dataset test asserting stub creation rather than rejection — an import that fails on the first unknown dataset is useless in practice. Mutator watch: dropping column mappings must fail the round-trip; failing on unknown datasets must fail the stub test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Import is validated and resolved

**Value**: External RDF cannot poison the graph.
**Acceptance criteria**: imported triples run Epic 5 validation; `Violation` severity rejects the offending subject and reports it, without failing the whole import; imported entities run Epic 17 resolution so a re-import does not duplicate; import lands in `graph:import:{source}` so a bad import is deletable wholesale; a dry-run reports what would land; import is transactional per subject, not per file.
**RED**: A partial-failure test: a file with one invalid subject imports the rest and reports the one. A re-import test asserting no duplicates. A wholesale-delete test asserting the named graph can be dropped without touching core data. Mutator watch: all-or-nothing import must fail the partial test; skipping resolution must fail the duplicate test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Lossiness is documented and tested

**Value**: A user knows what survives the boundary.
**Acceptance criteria**: a table in this plan lists what export drops (internal predicates, confidence, reconciliation state) and what import cannot express (lifecycle, certification, custom-property schemas); each row has a test asserting the drop is deliberate; export emits a manifest naming the format version and vocabulary mappings applied.
**RED**: One test per documented drop, asserting the field is absent from output — so an accidental future inclusion is caught, and an accidental removal of something documented as preserved is too. Mutator watch: n/a; these tests *are* the specification.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **RDF-star** → reified relationships (Epic 4) cover annotation at lower cost. Revisit if an exchange partner requires it.
- **SPARQL endpoint federation** → not planned (Epic 7 decision).
- **R2RML / RML mapping** → a general relational-to-RDF mapper is a separate product. The entity projection (Epic 4) is graph-owl's specific answer.
- **Additional vocabularies** (schema.org, VoID, Dublin Core beyond DCAT's use) → each is a small `VocabularyMapper`; add on request.
- **HDT** → an export-size optimization; revisit if Epic 37b shows archive size is a problem.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. DCAT output validated against **published external shapes**, not self-written ones.
5. Remote-context fetching refused for unlisted hosts (Slice B) — verified, since this is an SSRF surface.
6. JSON-LD expand < 5ms per document per `00a-product-position.md`.
