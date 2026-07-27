# Plan: RDF Interop & Open Standards (Epic 9)

**Branch**: feat/engine-rdf-io
**Status**: Not started
**Depends on**: Epic 4 (triples to serialize), Epic 7 (CONSTRUCT produces Turtle)
**Crate**: `graph-owl-rdf-io`

## Goal

Exchange the graph with the outside world in standard formats and vocabularies, without adopting RDF as the internal model.

## Resolved decisions

1. **Conform at the boundary, stay property-graph inside.** RDF is how the graph interoperates, not how it is stored. Adopting RDF/OWL internally would trade transactional cascades and predicate-compiled authorization (Epics 4, 13) for a serialization property obtainable with a mapping layer.
2. **Vocabularies are mappings, not remodelling.** DCAT, PROV-O, ODCS, OpenLineage are emitted *from* the existing model. The internal predicate vocabulary does not become `dcat:`.
3. **JSON-LD before Turtle, reversing the slice order below.** Turtle is Slice A because it is the simplest to implement; JSON-LD is the one that unblocks anything. DCAT, PROV-O, ODCS and OpenLineage are all published as JSON-LD, so JSON-LD is an *ingestion* capability and Turtle is an *export convenience* — and this project needs to consume standard metadata far more urgently than it needs to emit it. Expand and compaction first; framing later, where it can replace per-endpoint DTOs.
4. **This epic decides whether `rdf:reifies` is emitted always or only on export.** `04-engine-triples.md` finding 5 established that graph-owl's reified relationship node already *is* an RDF 1.2 reifier, missing only the vocabulary. Emitting `rdf:reifies` plus a triple term into the store on every edge doubles the flakes per relationship; emitting it at serialization time keeps the store compact and the wire standard. **Export-only is the default reading**, because the store's job is to be queried and the wire's job is to be understood, and nothing in the store benefits from the extra rows. Revisit if a SPARQL query needs to match on triple-term patterns directly.
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

Dependencies: `rio_turtle` / `rio_xml` for Turtle/N-Triples/RDF-XML, `json-ld` for expand/compact/frame. `Sid` ↔ IRI conversion uses the namespace registry from Epic 4 — a `Sid` with an unregistered namespace fails serialization loudly rather than emitting a bare local name.

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

- [ ] Every format round-trips: `parse(serialize(x)) == x` for the expressible subset.
- [ ] JSON-LD context is versioned, served at a stable URL, and pinned in output.
- [ ] DCAT export validates against the DCAT SHACL shapes.
- [ ] OpenLineage events both export and import.
- [ ] Import runs Epic 5 validation and Epic 17 resolution before landing.
- [ ] An unregistered namespace fails serialization with a named error.
- [ ] What each direction drops is documented and tested.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Turtle and N-Triples round-trip

**Value**: The simplest exchange formats, proving the `Sid` ↔ IRI boundary.
**Acceptance criteria**: all `FlakeValue` variants round-trip; typed and language-tagged literals preserved; IRI escaping handled (spaces, unicode, `<>`); blank nodes are stable within one document; an unregistered namespace → named error; N-Quads carries `cx`, N-Triples drops it with a documented warning.
**RED**: Round-trip per variant. An IRI-escaping test with a space and a unicode character — the classic silent-corruption case. A test asserting N-Triples *warns* when dropping named-graph information rather than dropping it silently. Mutator watch: unescaped IRI output must fail; silent `cx` loss must fail the warning assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

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
