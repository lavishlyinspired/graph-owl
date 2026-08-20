# Plan: Merge Entity into Explore as a tab, fix its broken subtabs, retire the TRACE nav group

**Status**: Active

## Goal

`/entity/:id` was a separate full page from `/explore/:id`, forcing a reader to hop between "the graph" and "the entity's own detail" for the same subject. Merged: Explore now has a **Graph | Entity** toggle sharing one entity picker, and the picker itself lists every real entity (144, live-verified), not only ones a rule flagged.

## Slice 2: fold Paths in too, retire the standalone TRACE group

Once Lineage/History/Evidence were real Entity subtabs (not fake data, not stubs — see below), the standalone TRACE nav group (Lineage, Paths, History, Evidence) had nothing left to justify its own sidebar section: three of its four screens were already superseded by real per-entity data, and the fourth (Paths) is a natural fifth Entity subtab — "paths from *this* entity to another" — not a screen that needs its own nav slot with a second from/to text box.

- **Paths**: folded into Entity as a subtab (`PathsPanel`), `from` pre-filled to the entity in view, reusing `findPaths`/`toPathsConfig`/`TraceDetail` unchanged. Verified live: real 1-hop path found and rendered (`books-...-INV-APR-003 → supplier-...`), matching the Graph tab's own `issuedBy` edge exactly.
- **TRACE nav group removed entirely.** `lineage-view.tsx`, `history.tsx`, `evidence.tsx`, `paths.tsx` deleted (their routes, not just their nav entries) — the first three because Entity's own tabs already cover the same ground with better (real per-entity, not generic-config) data; Paths because it's fully replaced by the Entity subtab. `trace.ts`/`TraceDetail.tsx`/`strings.ts` trimmed to just what Paths still needs (`toLineageConfig`/`toHistoryConfig`/`toEvidenceConfig`, the `TraceChain` breadcrumb machinery, and ~45 now-dead strings removed); `fetchLineage`/`LineageGraph` removed from `api.ts` as the same kind of now-uncalled dead code.
- **Route budget**: 4 routes fewer (now well under the 30-route ceiling `routes.test.ts` enforces).
- **VOCABULARY (Studio) moved directly above GOVERN** in the sidebar, per direct request — `INGEST` kept its own original relative position (still right after `GOVERN`), only `VOCABULARY` itself moved.
- **One real loss, named rather than hidden**: `/evidence` was a *global* findings browser (every finding across the whole graph), not per-entity — Entity's own Evidence tab is deliberately narrower (one entity's provenance). No equivalent "browse every finding" screen exists elsewhere in the console after this change; removed anyway per explicit instruction, not silently.
- 405/405 tests pass (11 net removed: ~19 lineage/history/evidence `trace.test.ts` cases dropped, `nav.test.ts`'s two stale `pageTitleForPath` fixtures repointed at still-live routes), `tsc`/`eslint` clean on every touched file.

## What was actually broken in Evidence / Lineage / History / Queries

Found by reading the code, not assumed:

- **Evidence**: hardcoded fake data — three invented sources ("GST Return — July 2026", confidence `0.98`, ...) that existed nowhere in the graph. Replaced with real provenance (`node.sources`, already returned by `/graph/context`/`/assets/{id}/graph` — no new fetch needed).
- **Lineage**: not broken exactly, but not real either — just a "click here" link out to `/lineage-view/:id`, no inline content. Replaced with real upstream/downstream counts, grouped by relationship, computed from data already fetched for the Overview tab's own Facts/Impact panels.
- **History**: a genuine bug. The dedicated History *tab* only ever read `versions` (catalog-asset-only) — for any graph-only subject (a GST invoice, most of this console's real data) it silently showed "No prior versions" even though the Overview tab's own embedded history panel, right next to it, correctly showed real finding-based history for that exact case. Two copies of the same logic had drifted apart. Fixed by extracting one `HistoryList` component both places now call.
- **Queries**: hardcoded stub — "No saved queries reference this entity yet," no real concept of a saved query behind it at all. Replaced with real, correct, copyable SPARQL (`outgoingFactsQuery`/`incomingReferencesQuery`) scoped to the entity actually being viewed — verified live to return real rows. Deliberately **not** offered for catalog-asset entities: an asset's own graph-queryable subject is keyed by an internal namespace code this screen has no reliable way to construct client-side, and a query that silently targeted the wrong subject would be worse than no query.

## Acceptance criteria

- [x] Explore gets a **Graph | Entity** toggle; `EntityPanel` (extracted from the old `entity.tsx`, its own redundant "Graph" subtab dropped) renders under "Entity", driven by the same `id`.
- [x] The entity picker lists every real entity in the graph (`allEntitiesQuery`/`entitiesFromSparqlRows`, scoped to installed packs, ontology-schema subjects like `Class`/`Property` filtered out), not only findings-flagged ones — merged with real finding counts where they exist (`mergeEntityOptions`), findings-first ranking preserved.
- [x] The first entity in that list loads automatically on a bare `/explore` visit (same "open on something, not nothing" behavior the old findings-only version had).
- [x] `openTargetFor` points at `/explore/:id?view=entity`; `/entity/:id` becomes a redirect to the same URL, so nothing that still constructs the old path (a saved link, `contradictions.tsx`, `trace.ts`) breaks.
- [x] "Entity" removed from the nav (`nav.test.ts` updated to expect it reached contextually, like `pipeline`).
- [x] Evidence/Lineage/History/Queries all fixed as described above.
- [x] 24 new/changed unit tests (`entityList`, `entityQueries`, `mergeEntityOptions`, `openTargetFor`), all passing; full suite 426/426, `tsc`/`eslint` clean on every touched file.
- [x] **Verified live, through the actual running console**: 144 real entities in the picker; clicking into the Entity tab for a real invoice showed real Evidence (`reco-302f19ab8be4-books`), real Lineage (matching the Graph tab's own edges exactly), real History (the graph-only-subject case that was previously silently empty), and real, correct SPARQL in Queries (independently confirmed against `/sparql` directly — 17 real rows). Right-click → **Open entity** on a graph node → lands on the Entity tab with the right subject selected, closing the loop end to end.

---
*Delete this file when the plan is complete.*
