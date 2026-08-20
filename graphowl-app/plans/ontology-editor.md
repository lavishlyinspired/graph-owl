# Plan: Ontology Editor (author/validate/save real Turtle, in Studio)

**Status**: Active

## Goal

Let a user actually build/edit ontology declarations from the console, using the backend capability that already exists and is already tested (`/ontology-editor/{preview,dry-run,save}`, Epic 42 Slice G) — ported as a third view inside the existing Studio → Ontology tab, alongside the read-only Graph/Table views shipped in `plans/ontology-graph.md`.

## What this is not

`/ontology-editor/save` writes to one fixed source (`Catalog::import_graph("ontology-editor")` → the named graph `graph:import:ontology-editor`), retracting and re-importing wholesale on every save. It is **a single, separate, author-owned ontology document — independent of any installed pack's own declarations** (`gst`, `hospitality`). Saving here never touches `graph:import:gst-ontology`. Editing a specific pack's own shipped ontology in place is a bigger, separate change (the `source` would need to become caller-supplied, and reads would need to union a pack's graph with its overlay) — out of scope here, and named so it isn't mistaken for what shipped.

## Acceptance criteria (Slice 1)

- [x] Studio → Ontology gets a third view toggle, **Editor**, beside Graph/Table.
- [x] The panel states the scope distinction above, plainly, in the UI (not just this doc).
- [x] Shows what's currently declared in `graph:import:ontology-editor` today (class/relationship counts), reusing the existing `ontologyModelFromSparqlRows` parser — no new parsing logic.
- [x] A Turtle textarea plus three actions: **Preview** (parse only, no storage — `/ontology-editor/preview`), **Check** (shapes + reasoning — `/ontology-editor/dry-run`), **Save** (retract + reimport — `/ontology-editor/save`).
- [x] A syntax error from any of the three shows message + line/column.
- [x] Check shows accepted subjects, rejected subjects with reasons, and the new-inference count.
- [x] Save shows landed/skipped/rejected, or the syntax error that blocked it.
- [x] Non-admin principal: the panel says so plainly, rather than a raw fetch error, *if* that ever actually happens — see the correction below.
- [x] `tsc`/`vitest`/`eslint` clean.
- [x] **The real Save-succeeds path — fully verified live, through the actual UI, not just curl.** Typed real Turtle (`gst:DebitNoteTest a owl:Class .`) into the running console, clicked Save, got `"Saved: 1 subject landed."` Confirmed via a direct `/sparql` query that the triple genuinely landed in `graph:import:ontology-editor`. Cleaned up afterward (`DELETE /graph/import/rdf?source=ontology-editor`).

**Correction to this plan's own earlier entry — the "admin-only" message shown before was never a real auth gate.** This local dev server runs with authentication disabled (`GET /me` → `{"id":"system",...,"isAdmin":true}` — confirmed directly, and the earlier claim that `system` is seeded `is_admin=false` conflated a *database attribution row* with the unrelated, hardcoded `Principal::system()` the `Auth` extractor actually returns in open mode, which has `is_admin: true`). The real cause of every earlier "admin-only" message: **`graphowl-app/vite.config.ts`'s dev proxy list was missing `ontology-editor` entirely.** Its regex (`^/${p}(/|$|\?)`) matches `/ontology-packs` and `/ontology` but not `/ontology-editor` — the request fell through to Vite's own dev server, which 404s an unmatched POST, and this panel's `isAdminOnlyError` heuristic (any 404 = admin gate) mistook that for the real thing. Fixed by adding `"ontology-editor"` to the proxy list. The `isAdminOnlyError` check itself stays — it is still correct behavior *if* a genuine admin gate is ever hit on a real deployment — but nothing in this local environment currently exercises that path, so the "known verification gap" this plan previously recorded is closed, not open.

## Slice 1: author, validate, save a real ontology document

**Value**: An admin can write real OWL/RDFS-shaped Turtle, see exactly what it would do before committing, and land it as queryable graph data — using the same shapes/reasoning gate every other import goes through.
**Path**: Textarea → `POST /ontology-editor/{preview,dry-run,save}` (already shipped, `graph-owl-api`/`graph-owl-server`, Epic 42 Slice G) → real triples in `graph:import:ontology-editor`, immediately visible via the same `/sparql` read path the Graph/Table views already use.
**RED/GREEN**: pure formatting/query-building functions first (`ontologyEditor.ts` — a query builder for the editor's own fixed graph, and result-summary formatters), tested; then the panel component, wired and verified live (matching this session's established practice: canvas/panel components are verified live, not unit-tested — see `OntologyCanvas.tsx`, `OntologyStructureBrowser.tsx`).
**Done when**: acceptance criteria above all hold, checked live against the real dev server, including the admin-gate behavior.

---
*Delete this file when the plan is complete.*
