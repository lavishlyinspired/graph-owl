# Plan: Fold the standalone Workbench into Studio as a Queries tab; move Knowledge under PLATFORM; increase base text size

**Status**: Complete

## Goal

The SPARQL/Cypher runner lived at its own `/workbench` route, reached from a standalone sidebar item, disconnected from the rest of the authoring surfaces in Vocabulary Studio. Folded in as Studio's 9th tab ("Queries"), same pattern Request B already used to retire the TRACE group — one more surface absorbed into a tab rather than its own nav slot.

## What changed

- **New `src/routes/studio/QueriesTab.tsx`**: ports the former `workbench.tsx`/`SparqlTab.tsx` runner verbatim (SPARQL/Cypher toggle, textarea keyed to the active language's placeholder, Run button, KPI row — rows/facts scanned/truncated — and a variable-column result table). `runSparql`/`runCypher` (`lib/api.ts`) were already generic over one shared `SparqlResult` shape (`SparqlOutcome` server-side), so this is one runner parameterized by which endpoint it calls, not two.
- **`studio.tsx`**: `TABS` gains `"queries"` (`strings.studioTabQueries`, replacing the old bare `"sparql"` tab/`studioTabSparql`); renders `<QueriesTab />`.
- **`src/routes/studio/SparqlTab.tsx` and `src/routes/workbench.tsx` deleted** — fully superseded, no remaining callers.
- **`lib/routes.ts`**: `"workbench"` removed from `ROUTES`.
- **`lib/nav.ts`**: `{ label: "Knowledge", route: "knowledge" }` moved out of UNDERSTAND (now just `Explore`) and into PLATFORM, relabeled `"Knowledge packs"`, replacing the just-removed `{ label: "Workbench", route: "workbench" }` at the same array position.
- **`lib/strings.ts`**: `studioTabSparql: "SPARQL"` → `studioTabQueries: "Queries"`.

## Verified live

- **SPARQL**: `PREFIX gst: ... SELECT ?s ?name WHERE { GRAPH ?g { ?s a gst:Supplier ; gst:supplierName ?name } } LIMIT 5`-shaped query run through the UI → real rows returned (`ROWS 5 FACTS SCANNED 50 TRUNCATED no`), matching a direct `curl POST /sparql` of the same query.
- **Cypher**: `MATCH (s)-[p]->(o) RETURN s, p, o LIMIT 5` run through the UI → `ROWS 0 FACTS SCANNED 0`, "Query returned no rows." Reproduced identically via direct `curl POST /cypher`, and confirmed **not** a wiring bug: tried three narrowings (unconstrained pattern, `WHERE t.type = "supplier"`, `:Supplier` label match, `-[:issuedBy]->` relationship-type match) — all return `factsScanned: 0`. Root cause is architectural, not this change: Cypher's relationship lowering queries a reified `relType`/`fromEntity`/`toEntity` triple triad that only entities created through `Catalog::upsert_asset`/`project_incremental` populate (`plans/07b-engine-cypher.md`, `plans/09a-lpg-interchange.md` in the `graph-owl` backend repo). The GST demo pack is loaded as plain RDF (`?s a gst:Supplier`, no reified LPG edges), so Cypher structurally cannot see it — SPARQL can, because it reads triples directly. This is the exact same "0 rows, honest message" result the old Workbench page produced when it was originally verified against this same demo pack (backend `DEMOS.md`, epic "Console H": *"Cypher executed 0 rows/0 facts read with the honest 'matched nothing' message"*) — not a regression introduced by this move.
- Both modes execute without error and render the honest KPI row either way; the UI correctly reports zero matches rather than crashing or silently hiding the result.
- Sidebar confirmed: Workbench gone, Studio shows Build/Glossary/Business view/Proposals/Graph/Ontology/Validate/Queries/Export; PLATFORM now leads with "Knowledge packs".

## Text size: every arbitrary `text-[Npx]` value increased

The whole console sets font size exclusively via Tailwind arbitrary values (`text-[12px]`, `text-[9.5px]`, ...) — no named `text-sm`/`text-base`/etc. utilities are used anywhere, and there is no root `font-size` override, so a `<html>`-level change would have done nothing (arbitrary `px` values are absolute, not `rem`-relative). The only real fix is rescaling every literal value.

Ran a single-pass script (`/tmp/bump_text_size.py`) over every `.ts`/`.tsx` file, mapping each of the 18 distinct sizes found (`8px` through `21px`) to itself **+1.5px**, preserving strict ordering (no two distinct sizes collapse together) so the existing visual hierarchy — micro labels vs. body vs. headings — is unchanged, just larger throughout:

| old | new | · | old | new | · | old | new |
|---|---|---|---|---|---|---|---|
| 8 | 9.5 | | 10.5 | 12 | | 15 | 16.5 |
| 8.5 | 10 | | 11 | 12.5 | | 16 | 17.5 |
| 9 | 10.5 | | 11.5 | 13 | | 17 | 18.5 |
| 9.5 | 11 | | 12 | 13.5 | | 18 | 19.5 |
| 10 | 11.5 | | 12.5 | 14 | | 20 | 21.5 |
| | | | 13 | 14.5 | | 21 | 22.5 |
| | | | 14 | 15.5 | | | |

733 substitutions across 49 files. Used a single regex pass with a lookup callback (not sequential `sed`), because the new-value set overlaps the old-value set (e.g. `12px`→`13.5px` while a separate, unrelated `13px`→`14.5px`) — a naive two-step find/replace would have double-bumped anything already rewritten to a value another rule was still targeting.

- [x] Verified live: Home KPI grid, Governance's dense empty-state table, Explore's graph canvas node labels, and the Entity panel (all 6 tabs, full fact rows, side KPI cards) all render with visibly larger, more legible text and no overflow, clipping, or wrapping regressions.
- [x] 401/401 tests pass, `tsc` clean.
- [x] `eslint` unaffected by this change specifically (verified by diffing error counts against a stash of the whole session's other uncommitted work) — the 65 current `no-raw-jsx-text` errors are pre-existing from earlier, unrelated session work (e.g. `home.tsx`'s hardcoded mock KPI/consumer literals), not something this script could introduce since it only ever rewrites `text-[Npx]` substrings inside `className`.

## Acceptance criteria

- [x] Studio gains a Queries tab with SPARQL and Cypher sub-modes, both wired to the real backend.
- [x] Workbench removed from the sidebar and route table.
- [x] Knowledge moved into PLATFORM, relabeled "Knowledge packs".
- [x] Both query languages tested live; SPARQL returns real rows, Cypher's zero-row result is confirmed as an existing backend/data-shape limitation rather than a bug in this change.
- [x] Overall text size increased app-wide, verified live across 4 representative screens.
- [x] `tsc`/`vitest` clean on every touched file; `eslint` unaffected (pre-existing errors elsewhere, not from this work).

---
*Delete this file when the plan is complete.*
