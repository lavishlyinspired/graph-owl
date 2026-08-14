# Plan 116 — Preview an installed pack's own ontology as a graph

**Status**: not started. **Branch**: main.

**Trigger**: "let me select and preview [a pack's] ontology in the UI as a
graph... run SPARQL... run Cypher [on it]" — investigated before writing a
line of code, because the console already has an ontology editor (Epic 42
Slice G) and a pack-data browser (Plan 115), and the overlap turned out to be
almost total.

## What's actually true, verified live against a running server

- **A pack's `ontology.ttl` is not a special file — it is loaded exactly like
  any other pack document.** `packs/gst/pack.toml` and
  `packs/hospitality/pack.toml` both declare it as `[[documents]] path =
  "ontology.ttl" source = "{packId}-ontology"`, and
  `connectors/python/graph_owl_packs/loader.py`'s `load_pack` POSTs it to
  `/graph/import/rdf` in the same loop as every fixture. It lands as flakes in
  `graph:import:{packId}-ontology`, indistinguishable from any other loaded
  source. Confirmed against a live server: `gst-ontology` holds 56 triples and
  is fully queryable through `/sparql` today.
- **The Explore "Pack data" block (Plan 115 B1) already lists it**, right
  alongside `gst-gstr2b-2026-08` and every other source — `{packId}-ontology`
  is just another entry in `loadedSourcesFromSparql`'s named-graph listing.
- **Clicking it already works as a schema browser.** `PackSourceView` +
  `subjectsQuery`/`typesQuery` (`packData.ts`) already list every class and
  property with its `rdf:type` (`Class`/`Property`) and triple count — the
  ontology's own `rdf:type gst:Class` assertions are ordinary `?s a ?t`
  triples, and `typesQuery`'s `a` shorthand catches them with no code aware
  that "Class" is a pack's own meta-vocabulary term. `SubjectExplorer` opens
  one class or property's neighbourhood from there.
- **SPARQL and Cypher already run against this data.** `/sparql` and
  `/cypher` are general, already-shipped endpoints (Plan 111 Slice B); a user
  who knows the graph IRI can already write
  `SELECT ?s ?p ?o WHERE { GRAPH <…#graph:import:gst-ontology> { ?s ?p ?o } }`
  today and get results, Table or Graph view, no new backend work needed.

## The actual gap

**`PackSourceView`/`SubjectExplorer` render one subject's one-hop
neighbourhood at a time — there is no "this pack's whole ontology, as one
graph" view.** The console already has exactly that capability, built for a
different input: `OntologyEditor.tsx` (Epic 42 Slice G) parses pasted
Turtle/N-Triples/JSON-LD into a full Cytoscape graph with namespace/predicate
filtering. But its only input is a text box a human types or pastes into —
there is no path from an already-installed pack's already-imported ontology
into it. A user who wants to *see* `gst-ontology` as one connected picture has
to manually copy `packs/gst/ontology.ttl`'s source text in, which defeats the
point of it already being loaded, live, queryable data.

**CONSTRUCT queries do not work against this engine** — verified live:
`CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <iri> { ?s ?p ?o } }` returns empty rows
where the equivalent `SELECT ?s ?p ?o WHERE { … }` returns all 56 triples
correctly. `graph-owl-query`'s `pushdown.rs` pattern-matches
`Query::Construct` but nothing downstream executes the construct template —
out of scope to fix here; the workaround (a `SELECT ?s ?p ?o` plus
client-side N-Triples formatting) needs no engine change and produces
identical input to what a human would have pasted.

## Design decision — what stays out of this plan

**Cypher-on-pack-data is a separate, much larger, already-deferred gap — not
touched here.** The Workbench's Cypher tab is real and works, but only
against the LPG projection, and `lpg`/`bolt`/`lpg-io` are still placeholder
crates (`plans/00e-crate-architecture.md`'s crate table) — there is no synced
property-graph projection of pack data yet for Cypher to see. That is Epic
territory, not a slice of this plan; this plan's SPARQL-only scope is not an
oversight.

**AI-assisted authoring is out of scope**, per the user's own choice when this
work was scoped (`plans/00i-licensing.md`-governed session: "Build from specs
+ existing code", no reference-repo research of any kind). If wanted later,
it is its own plan, grilled and designed from W3C specs and this codebase's
own patterns — not bundled into a UI wiring slice.

**No new persistence.** The whole point of the gap being this small is that
the ontology is already graph data. This plan adds zero tables, zero new
`/graph/import` calls, zero caching of `ontology.ttl` text anywhere — it reads
what `/sparql` can already answer, the same "persist only what's necessary"
principle the rest of this session's work (`node_semantic_type`, Plan 114
Slice F) already followed.

## Slice A — Load an installed pack's ontology into the editor

- **Where**: `ui/src/features/ontology/OntologyEditor.tsx`,
  `ui/src/features/ontology/ontologyDocument.ts` (or a small new sibling
  module if a pure formatting function doesn't belong in the parse-state
  file).
- **Change**: a "Load installed pack" `Select` above the text area, populated
  from `installedPacks(await api.namespaces())` (same call `PackDataExplorer`
  already makes). Choosing a pack finds its ontology source —
  `${packId}-ontology`, the convention both shipped packs already follow —
  among `loadedSourcesFromSparql(await api.sparql(NAMED_GRAPHS_QUERY).rows)`
  (same query `PackDataExplorer` already runs), runs
  `SELECT ?s ?p ?o WHERE { GRAPH <source.iri> { ?s ?p ?o } }`, formats the
  rows as N-Triples text (new pure function — each row's `s`/`p`/`o` already
  arrive as literal N-Triples terms per `packData.ts`'s existing handling, so
  formatting is `` `${s} ${p} ${o} .` `` joined by newlines, nothing to
  unwrap), and calls the same `setState` the text area's own `onChange`
  already uses with `{ format: "ntriples", document: text }`. Everything
  downstream — parsing, the graph pane, namespace/predicate filters, Check,
  Save — is unchanged; this only changes how the text box gets its first
  value.
- **A pack with no ontology source present in the graph** (declared but not
  yet loaded, or a pack manifest with no `ontology.ttl` at all) is a real
  state, not an error: the picker shows the pack but "Load" is disabled with
  a reason, matching `PackDataExplorer`'s "absent is the default, not an
  error" convention.
- **Acceptance criteria**: selecting an installed pack with a loaded ontology
  source populates the editor's text and renders the graph within one
  debounce cycle, identical to what pasting the same text by hand would
  produce; a pack with no ontology source disables Load rather than sending
  an empty or malformed query; the manual paste path is untouched — this is
  strictly additive.
- **Tests**: a pure test for the row→N-Triples formatter (empty rows → empty
  string; a row shaped like a real `?s ?p ?o` result → one well-formed
  N-Triples line; a row with a language-tagged or typed literal `o` passes
  through unmodified, since the wire already delivers correct N-Triples
  lexical form); a structural test that selecting a pack with a stubbed
  `/namespaces` + `/sparql` response ends with `state.document` populated and
  `format` set to `"ntriples"`; a structural test that a pack with no
  matching source disables the load control.

## Verification

UI-only, same shape as Plan 115: `npm test` + lint + typecheck on the touched
modules. No Rust files touched → no `cargo mutants` run needed for this plan.
