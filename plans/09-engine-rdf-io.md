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

### Slice B: JSON-LD with a versioned context — **shipped, 5 August 2026**

**Value**: The format web clients actually use.
**Shipped**:
- `oxjsonld` 0.2.5 adopted (same Oxigraph family as `oxttl`, same `oxrdf =0.3.3` pin; MIT OR Apache-2.0, 649K downloads — checked `00l-build-vs-adopt.md`). `parse_json_ld`/`serialize_json_ld_with_context` wired into `RdfFormat::JsonLd`.
- **Remote `@context` fetching refused by default, verified by reading `oxjsonld`'s own source** (`context.rs`): no `load_document_callback` set means the parser errors before any network call — SSRF-safe by construction, not by an added check. `parse_json_ld_with_allowed_hosts` is the explicit opt-in, host-checked via `url::Url` (not a hand-rolled split, so a userinfo trick like `http://allowed.com@evil.com/` cannot fool it — tested directly).
- `parse_json_ld_with_loader` factors the fetch mechanism out from the allowlist policy, so the SSRF-refusal and round-trip tests need no real network access.
- **Context is a served, versioned artifact**: `JsonLdContext::core_v1().url()` → `https://graph-owl.dev/context/v1`; `graph-owl-server` serves it at `GET /context/{version}` (`application/ld+json`, 404 for an unknown version) via `JsonLdContext::to_document()` — the *same* function the crate's own compaction consults, so route and compaction cannot drift into two different mappings. Compacted output carries this URL as `@context`, never the inline object `oxjsonld` writes by default.
- `@graph` maps to `cx` — confirmed free from `oxjsonld`'s own quad-based parsing (a node object with sibling `@id`/`@graph` yields a `Quad` whose `graph_name` is that `@id`, the same shape N-Quads already produces), tested directly.
- **Frame implemented as a documented subset of <https://www.w3.org/TR/json-ld-framing/>**, sourced from the spec itself — neither `oxjsonld` 0.2.5 nor `json-ld` 0.21.4 (the only two permissively licensed JSON-LD crates on crates.io, both checked) implements the framing algorithm at all. `frame_json_ld` matches by `@type` and by every predicate the frame names (the spec's own default `@requireAll` behaviour), nests referenced nodes once, and turns a repeat into a bare `{"@id": ...}` rather than recursing forever — proven directly with a two-node reference cycle. `@embed`/`@explicit`/`@omitDefault`/list framing are not implemented; recorded here rather than silently absent.
**Scope note, not a silent gap**: "compact" in this crate means base-relative `@id` shortening, not CURIE term compaction — verified by reading `oxjsonld`'s `from_rdf.rs`: `with_prefix` records prefixes in the emitted `@context` as metadata for a *consumer's* own compaction, but the writer itself always emits predicate/`@type` keys as full IRIs. Two different `JsonLdContext`s (differing `base`) do produce genuinely different, both-valid bytes — tested — which is what the criterion actually asks for, expressed through the mechanism this adopted crate really has.
**Tests**: `graph-owl-rdf-io::tests` — 18 total (9 from Slice A plus 9 new: JSON-LD round-trip, context-version-pinning, two-contexts-differ, `@graph`→`cx`, remote-context-refused-by-default, unlisted-host-refused, `is_host_allowed` userinfo/suffix-bypass resistance, frame-by-type-with-nesting, frame-cycle-safety). `graph-owl-server::tests` — 2 new (`/context/v1` serves the exact `JsonLdContext::core_v1()` document; `/context/v99` is `404`).

### Slice C: DCAT and PROV-O export — **shipped, 6 August 2026**

**Value**: Standard dataset description and provenance, for catalog-to-catalog exchange.
**Shipped**:
- Three new registered namespaces (`graph-owl-core`): `DCAT` (`http://www.w3.org/ns/dcat#`), `PROV` (`http://www.w3.org/ns/prov#`), `FOAF` (`http://xmlns.com/foaf/0.1/`) — needed to express and parse these vocabularies as real IRIs, not runtime-only predicates.
- `Catalog::export_dcat(&DcatExportScope) -> Vec<Flake>` (`graph-owl-api`): `Table` → `dcat:Dataset` (`dct:title`, `dct:description` with an FQN fallback, `dct:publisher` from the first owner or the seeded `system` principal, `dcat:theme` from the asset's own kind), `Service` → `dcat:Catalog` (same fields). PROV-O: one `prov:Activity` per entry in the asset's real version history (not synthesised), chained `prov:wasInformedBy` oldest→newest, rooted at the subject via `prov:wasGeneratedBy` on the latest.
- Scoping: `service_fqn` reuses `graph_owl_core::archive::ScopeSelector::FqnPrefix` (the same mechanism `export_archive` already scopes by, not a second one); `domain_id` via `resolve_asset_domain` per asset (no bulk list-by-domain storage method exists yet — documented as O(n) across the page, fine at current scale, revisit if it needs indexing); `include_deleted` excludes tombstones by default.
- `graph_owl_constraint::shapes::dcat_ap_conformance_shapes(t)`: mandatory-property requirements for `dcat:Dataset`/`dcat:Catalog`, **not the official SHACL file verbatim** — see its own doc comment for the full reasoning. Summary: the real shapes were fetched (`SEMICeu/DCAT-AP` release 3.0.1, CC-BY-4.0, retrieved 6 August 2026) and checked directly against `graph-owl-constraint`'s reader; three real incompatibilities were found by reading `shapes.rs` itself (not guessed): `sh:severity` is read once per NodeShape here, not per property, so a property-level `sh:severity` triple — which every property in the official file carries — is an *unrecognised term* and fails the whole shape; `sh:nodeKind` here takes the literal strings `"ref"`/`"literal"`, not the real SHACL node-kind IRIs (`sh:BlankNodeOrIRI` etc.), so a real value is never text and always fails to read; `sh:node`/`sh:shape` shape-reference indirection has no arm in `constraint_from` at all. All three would mean substantially extending this engine's SHACL subset — out of this slice's scope. What is preserved exactly, quoted in the doc comment for traceability without re-fetching: every `minCount`/`maxCount` on `dct:title`, `dct:description`, `dct:publisher` for both classes is the official number; only the encoding is this engine's own dialect (the same one `core_shapes` already uses).
**Tests**: `graph-owl-constraint::shapes::tests::the_dcat_ap_conformance_shapes` — 5 tests (both shapes compile; a Dataset missing title+description is rejected — the plan's own mutator watch; a conforming Dataset passes; a Catalog without a publisher is rejected even with title+description — the one cardinality that genuinely differs between the two classes; a conforming Catalog passes). `graph-owl-api::dcat_export_tests` — 6 tests (Table→Dataset / Service→Catalog with all four fields; real export output validated against the real shapes fixture — the RED test the plan names; an export with title stripped fails validation — the mutator watch, run against real output rather than a hand-built fixture; PROV-O activity count matches real version-history length and a multi-version chain produces `wasInformedBy`; a soft-deleted asset is excluded by default and included on request; export scoped to one service excludes a sibling service's table).
**Scope cut, recorded rather than silently narrowed**: theme is a coarse per-kind classification (`theme:table`/`theme:catalog`), not derived from any tagging/glossary system — DCAT-AP's own shape does not require `dcat:theme` at all, so this is present because the plan's own criterion names it, not because conformance demands it. No mutation run this pass (matches Slice A/B's own recorded practice).

### Slice D: OpenLineage bidirectional — **shipped, 6 August 2026**

**Value**: The highest-value interop — lineage is what other tools most want to exchange.
**Shipped**:
- `LineageSource::OpenLineage` (new third variant, `graph-owl-core`) and `LineageDetails::openlineage_event_id: Option<String>` — a Postgres migration (`V48__lineage_openlineage_event.sql`) and both storage backends wired to persist it, so provenance survives a round trip through the real backend, not only the in-memory one.
- `Catalog::export_openlineage(&DcatExportScope) -> Vec<serde_json::Value>`: one `RunEvent` per table-level `feeds` edge (`run.runId` is the edge's own id — already a UUID, so export is stable across re-runs rather than inventing a second identity for the same fact), `job` named from the edge's pipeline asset if it has one. Column-level `feeds`/`derivedFrom` edges between the two tables' columns become the output dataset's `columnLineage.fields.<col>.inputFields` facet, matching <https://openlineage.io/spec/facets/1-2-0/ColumnLineageDatasetFacet.json> exactly (Apache-2.0, OpenLineage project — the JSON Schema fetched and read directly, not a reference implementation).
- `Catalog::import_openlineage(principal, &serde_json::Value) -> OpenLineageImportOutcome`: asserts a `feeds` edge per input×output pair (and per column mapping, if `columnLineage` facets are present) with `source: OpenLineage` and `openlineage_event_id` set to `run.runId`. **Idempotent through the same uniqueness `assert_lineage` already enforces** — `(from, to, relationship, source)` — rather than a second dedup mechanism: two imports of the same event resolve to the same pair, so the second hits the existing conflict and is reported `skipped`, not landed or errored.
- Unknown datasets become stubs, `lifecycle: Draft`, via a new `Catalog::create_stub_asset` — **not** `upsert_asset` + `set_lifecycle`, because the lifecycle state machine only allows *entering* `Draft` at creation (`can_transition` permits `Draft|Deprecated -> Active`, never the reverse) and `upsert_asset` hardcodes `Active` on every new asset. Found by a real test failure (`` `active` cannot move to `draft` ``), not anticipated — `create_stub_asset` builds the `Asset` directly with `lifecycle: Draft` and calls `storage.upsert_asset`, the same construction `Catalog::upsert_asset` uses minus that one hardcoded default. A dataset name must be `database.schema.table` for stub creation to walk its container chain (`Service.Database.Schema.Table`); a differently-shaped name is a validation error naming it, not a guess.
**Tests**: `graph-owl-api::openlineage_tests` — 4 tests (a table-level edge with column lineage exports both the RunEvent shape and the `columnLineage` facet; an event naming two never-seen datasets creates the full container chain as `Draft` stubs and lands one edge with `source: OpenLineage`; re-importing the identical event lands nothing new and reports it skipped; round-tripping a many-to-one column mapping — two input fields into one output field — through import then export preserves both).
**Scope cut, recorded rather than silently narrowed**: only `feeds`/`derivedFrom` at table and column level project to/from OpenLineage — dashboards, topics, and ML models (legal lineage endpoints elsewhere in this catalog) are out of scope, since OpenLineage's own dataset model has no equivalent for them. No mutation run this pass (matches Slices A–C's own recorded practice).

### Slice E: Import is validated and resolved

**Value**: External RDF cannot poison the graph.
**Acceptance criteria**: imported triples run Epic 5 validation; `Violation` severity rejects the offending subject and reports it, without failing the whole import; imported entities run Epic 17 resolution so a re-import does not duplicate; import lands in `graph:import:{source}` so a bad import is deletable wholesale; a dry-run reports what would land; import is transactional per subject, not per file.
**RED**: A partial-failure test: a file with one invalid subject imports the rest and reports the one. A re-import test asserting no duplicates. A wholesale-delete test asserting the named graph can be dropped without touching core data. Mutator watch: all-or-nothing import must fail the partial test; skipping resolution must fail the duplicate test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Lossiness is documented and tested — **shipped, 6 August 2026**

**Value**: A user knows what survives the boundary.
**Shipped**: `Catalog::export_manifest(ExportFormat) -> ExportManifest` names the format version and vocabulary mappings each export uses — static metadata (no storage access), paired by a caller with `export_dcat`'s or `export_openlineage`'s own data. The lossiness table below is Slice F's real deliverable; each row is a test in `graph-owl-api::lossiness_tests` asserting the field is genuinely absent from real export output (not a hand-built fixture) or genuinely never set by real import.

| Direction | What is dropped / cannot be expressed | Why | Test |
|---|---|---|---|
| DCAT/PROV-O export | `Asset::extension` (org custom fields) and `Asset::properties` (source-reported free-form bag) | Neither DCAT nor PROV-O has an equivalent open-ended bag; only the four curated fields (title/description/publisher/theme) are mapped | `dcat_export_omits_extension_and_properties` |
| DCAT/PROV-O export | `Asset::lifecycle`/`Asset::deprecation` | DCAT has no lifecycle-state vocabulary; a Draft/Deprecated/Retired asset exports identically to an Active one | `dcat_export_omits_lifecycle_and_deprecation` |
| DCAT/PROV-O export | `Asset::version`/`Asset::change_description` beyond what the PROV-O activity chain already carries (`generatedAtTime`/`wasAssociatedWith`) | DCAT/PROV-O has no `major.minor` version-number concept; only *when* and *who*, not *what changed* | `dcat_export_omits_raw_version_numbers` |
| DCAT/PROV-O export | Flake-level confidence scores (Epic 6) and Epic 17 reconciliation/merge state | `export_dcat` reads only `Asset`/`AssetVersion`, never flakes or merge records — structurally unreachable, not filtered | `dcat_export_never_touches_confidence_or_merge_state` |
| OpenLineage export | Everything DCAT/PROV-O drops, above, plus lineage `LineageDetails::query`/`description` (the SQL and human note behind an edge) | OpenLineage's `RunEvent` has no field for the transformation's own source text; `job.name` carries only the pipeline identity, not its logic | `openlineage_export_omits_query_and_description` |
| OpenLineage import | `lifecycle` beyond `Draft` on a stub, certification (Epic 26), and custom-property schemas (Epic 22) | A `RunEvent` names datasets by string; it has no certification or extension-schema concept to import *from*, so a stub is created exactly `Draft` and nothing else, regardless of what the event contains | `openlineage_import_never_sets_certification_or_extension` |

**RED**: One test per row, asserting the field is absent from real export output or never set by real import — so an accidental future inclusion is caught, and an accidental removal of something documented as preserved is too. Mutator watch: n/a; these tests *are* the specification.
**Tests**: `graph-owl-api::lossiness_tests` — 6 tests (one per row above) plus 2 for the manifest itself (`export_manifest` names a real format version and at least one vocabulary mapping, for both formats).

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
