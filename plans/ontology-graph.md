# Plan: Ontology Graph (Studio)

**Branch**: feat/ontology-graph
**Status**: Active

## Goal

A reader on Studio's Ontology tab sees the installed pack's real OWL-flavoured
model — classes and the relationships declared between them — as a graph,
sourced live from the same triple store Explore and Vocabulary already read,
not from a second, disconnected data path.

## Background (verified against the running system, not assumed)

- `packs/gst/ontology.ttl` declares 18 `gst:Class`, 33 plain `gst:Property`
  (attributes — no domain, so which class each belongs to is not declared
  anywhere, only usable by sampling real instance data) and 11
  `owl:ObjectProperty` + `rdfs:domain`/`rdfs:range` (relationships). No
  `owl:Class`, no `owl:DatatypeProperty`, no `subClassOf`/`equivalentClass`/
  `disjointWith`/cardinality/`owl:imports` anywhere in any pack today.
- All of it is already live in the running triple store and answers to the
  existing generic `/sparql` endpoint — confirmed by querying the running
  server directly for both the classes and the domain/range relationships.
  **No backend work is needed for slice 1.**
- `_archived/ui/src/features/ontology-builder/` (22 files, React Flow) is a
  real, previously-shipped, mostly-tested visual ontology editor —
  `plans/122-frontend-rebuild.md` already flagged it "genuinely reusable."
  It was never ported into `graphowl-app`. User has chosen to port it,
  keeping React Flow rather than rebuilding on G6 (`00f-ui-architecture.md`
  chose React Flow for this feature specifically, on 14 Aug 2026).
- Its `importTtl`/`importNTriples` parser (`formats.ts`) recognises a class
  by `rdf:type` ending in `#Class` (an accident that happens to also match
  `gst:Class`, not a designed compatibility — worth a comment when ported)
  and a relationship by `owl:ObjectProperty` + `rdfs:domain`/`rdfs:range`. It
  does **not** parse plain `gst:Property` at all — real classes will render
  with zero attributes in slice 1. That is a named, deferred gap, not a bug.
- The pack's ontology data lives in a named graph at
  `https://graph-owl.dev/ns/catalog#graph:import:{packId}-ontology` —
  confirmed empirically (`SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s a
  gst:Class } }` returned exactly this IRI against the running server).
- `components/ui/*` (shadcn primitives the archived UI's dialogs/panels
  import) do not exist in `graphowl-app`, and nothing in this app uses that
  layer — every other screen (`explore.tsx`, `entity.tsx`) is plain Tailwind
  in JSX. The presentational shell gets rebuilt to match; only the pure,
  already-tested logic (`flowModel.ts`, `layout.ts`, the TTL/N-Triples
  parsing half of `formats.ts`) and the canvas's rendering approach
  (React Flow, adapted off the old `palette` object onto this app's
  `StyleColors`/CSS-var convention) actually port unchanged.

## Acceptance Criteria

- [ ] A new "Ontology" tab exists under Studio, reachable like every other
      Studio tab.
- [ ] Opening it against the installed `gst` pack shows all 18 real classes
      as nodes and all 11 real relationships as labelled, directed edges —
      not placeholder or hand-authored data.
- [ ] The data comes from the same `/sparql` surface every other real screen
      in this console uses, over the pack's actual named graph.
- [ ] A class with no declared relationships still renders as an
      unconnected node, not dropped from the picture.
- [ ] The screen states its own known gap (attributes not shown) rather than
      silently looking complete.

## Slices

Every slice follows RED-GREEN-KILL SURVIVORS-REFACTOR (this app's fast inner
loop — `tsc`/`vitest`/`eslint` on touched files; full mutation testing only
on request, per this session's established rhythm for `graphowl-app`, not
run for any of the ~15 other features shipped in it this session either).
Commits wait for explicit approval, same as everything else this session —
the Rust workspace's "commit without asking" standing instruction is scoped
to the Rust workspace, not this frontend.

### Slice 1: The real GST ontology renders as a graph

**Value**: Anyone inspecting Studio can see the pack's actual class/
relationship model instead of having no way to see it at all.
**Path**: Studio nav → new "Ontology" tab → `fetchInstalledPacks()` (already
exists) to name the pack → a new `ontologyGraphQuery(packId)` builds
`SELECT ?s ?p ?o WHERE { GRAPH <https://graph-owl.dev/ns/catalog#graph:import:
{packId}-ontology> { ?s ?p ?o } }` → existing `runSparql` → rows joined into
N-Triples text → ported `importNTriples`/`triplesToModel` → `OntologyModel`
→ ported `OntologyCanvas` (React Flow), colours adapted to this app's
`StyleColors`.
**Required implementation skills**: `tdd`, `testing`, `refactoring` (React
component + graph-rendering conventions already established by
`GraphCanvas.tsx` apply here too).
**Acceptance criteria**:
  - Visiting the Ontology tab with the `gst` pack selected renders exactly
    18 nodes and 11 edges, matching a direct `/sparql` count.
  - Each edge is labelled with the relationship's own name (`issuedBy`,
    `onInvoice`, etc.), not a generic "relates to".
  - A class with zero relationships (there are some — not every one of the
    18 appears in the 11 relationship triples) still renders as a node.
  - No console errors; `tsc`/`vitest`/`eslint` clean on all touched files.
**RED**: Pure-logic tests first, no component yet —
  - `ontologyGraphQuery("gst")` returns the exact query string above.
  - `ntriplesFromSparqlRows` (ported/re-tested) turns `{s,p,o}` rows into
    `s p o .\n` lines, skipping any row missing a term.
  - `triplesToModel` (ported from `formats.ts`, re-tested against this
    project's factory-function style) turns the real 18/11-shape fixture
    into an `OntologyModel` with 18 `entityTypes` and 11 `relationships`,
    and a class absent from every `owl:ObjectProperty` triple still appears
    in `entityTypes`.
  - Mutator-relevant negative case: a triple whose object ends in `#Class`
    for an unrelated vocabulary must still be treated as a class (the
    suffix match is intentional, not a bug to "fix" into an exact match) —
    write the test that pins this down explicitly, since it is exactly the
    kind of thing a `===` vs `.endsWith` mutant would flip silently.
**GREEN**: Port `formats.ts`'s import half and `flowModel.ts` with minimal
  changes; write `ontologyGraphQuery`/`ntriplesFromSparqlRows` fresh (each
  is under 10 lines, cheaper to re-write to this project's style than to
  adapt); wire a thin `OntologyGraphRoute`/tab component that fetches,
  converts, and renders via the ported canvas.
**REFACTOR**: Assess once green — likely nothing, given the logic ports
  nearly verbatim and the shell is new and thin.
**Done when**: Acceptance criteria met, human reviews, commit approved.

### Slice 2 (sketch — detail deferred until slice 1 lands)

Clicking a node shows a detail panel: label, IRI, namespace, and its real
in/out relationships (still no attributes — that gap stays named until a
slice explicitly takes on usage-based attribute inference, which is a
materially bigger feature: attributes have no `rdfs:domain` in this pack, so
"which class owns `gst:taxAmount`" is only answerable by sampling which
class's *instances* actually carry that property in the data, not by
reading the ontology declarations alone).

### Slice 3 (sketch — explicitly out of scope unless requested)

Hand-authoring (Add Entity Type / Add Relationship dialogs, from the
archived `AddEntityTypeDialog.tsx`/`AddRelationshipDialog.tsx`) has no
backing save API today, same as when the original was built
(`types.ts`'s own doc comment says so). Porting the dialogs without a save
path would ship a control that visibly does nothing when used — not taken
on in this plan without an explicit ask and a decision on where authored
changes go.

## Pre-PR Quality Gate

Before the PR:
1. `tsc -b`, `vitest run`, `eslint` clean on all touched files (this
   session's established bar for `graphowl-app`).
2. Refactoring assessment — only if it adds value.
3. Mutation testing via Stryker — on request, matching how every other
   `graphowl-app` feature this session was verified.

---
*Delete this file when the plan is complete. If `plans/` is empty, delete
the directory.*
