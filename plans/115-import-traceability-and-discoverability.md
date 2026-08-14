# Plan 115 — Import traceability and discoverability: name the import, filter by period, surface pack data in Explore

**Status**: C1, C2, B1 shipped. B2 deferred, A rejected. **Branch**: main.

**Trigger**: "I uploaded GST data — what was imported, where did it go, and how
do I inspect it?" Investigated as a navigation gap rather than a missing
feature: the GST pack data *is* stored and *is* reachable — the reconciliation
workspace reads it, `SubjectExplorer` (`ui/src/graph/SubjectExplorer.tsx`) was
built exactly for "not a catalog asset" subjects — but the path from upload to
graph is invisible, and Explore (the first place a user looks) shows only the
asset containment tree.

## What's actually true, verified in code

- The Explore hierarchy (`ui/src/App.tsx:4018`, `ui/src/api.ts:1255`) is the
  **catalog asset** containment tree — database → schema → table → column,
  served from `/assets/roots` and `/assets/{id}/children`, backed by the
  relational `assets` table (`graph-owl-storage-postgres/migrations/V3`).
- GST imports land as **flakes in named graphs** in the graph engine
  (`graph-owl-engine-postgres`'s `flakes` table) via
  `Catalog::import_rdf` (`graph-owl-api/src/lib.rs:16066`); `import_rdf`
  never writes an `assets` row. Decision 6: relational is the source of
  truth for assets; the graph is a projection, and pack data lives in the
  graph only.
- `importThroughSurface` (`ui/src/features/packs/importFile.ts:39`) already
  computes and returns the import source name — `gst-<key>-<period>`, e.g.
  `gst-gstr2b-2025-07` — but the upload toast
  (`ui/src/features/reconciliation/ReconciliationWorkspace.tsx:339`) prints
  only counts.
- The reconciliation workspace already computes `summary.periods` per source
  (`ReconciliationWorkspace.tsx:388`); there is no way to narrow the
  statement to one filing period.
- Installed packs are discoverable from `/namespaces` (`declaredBy:
  "pack:<id>"`); `packIdOf`/`installedPacks` in
  `ui/src/features/packs/packSurfaces.ts` already parse that.

## Design decision — what is rejected and why

**A — putting pack subjects into the Explore asset tree — rejected.**
It would blur two different concepts: ontology/application assets (relational,
containment-structured) versus imported business data (graph flakes, flat).
The tree's `onSelect`/`index`/`?asset=` deep-link/`asOf` banner/content pane
all assume `Asset`; generalizing selection to `Sid` is real surface area for a
path the target user (a CA) does not take. `packSurfaces.ts`'s own rule —
"the console has no GST tab, and must not grow one" — is the same boundary.

**B2 — graph-subject browsing from Explore — deferred.** A subject
picker/explorer workflow ("show me the entities this source produced") has no
demonstrated user need yet. It would cost a seed picker and a browse flow;
revisit only when a real user asks for it.

## Conceptual model the plan makes visible

`Pack → Source → Graph facts → Reconciliation → Findings`

instead of `Upload → mysterious processing → findings`. The source name
`gst-gstr2b-2025-07` becomes a first-class identifier a CA can match from the
worksheet through to the graph.

## Slices

### Slice C1 — Name the import in the success message ✅ part of this commit

- **Where**: `ui/src/features/reconciliation/ReconciliationWorkspace.tsx`
  (`SourceCard.handle`).
- **Change**: the toast becomes
  `"gst-gstr2b-2025-07: 42 invoice(s) read, 168 facts added."` — the source
  name is already returned by `importThroughSurface` in `ImportOutcome.source`;
  no backend change.
- **Acceptance criteria**: uploading from either entry point (pack admin,
  reconciliation) names the source in the message; nothing else changes.
- **Tests**: assert the message includes the source name.

### Slice C2 — GST period filter

- **Where**: `ui/src/features/reconciliation/ReconciliationWorkspace.tsx`.
- **Change**: a `period` state defaulting to "all"; after `refresh`, union the
  periods across loaded sources; a `Select` offers `All` plus each period.
  Filter `rows` by period before building `statement`/`suppliers`, so the
  statement totals, head-wise table, findings join and by-supplier view all
  narrow to the selected period.
- **Acceptance criteria**: selecting a period narrows the whole workspace to
  it; `All` behaves exactly as today (additive, not a redesign).
- **Tests**: `statement.ts` is pure — extend with per-period selection;
  structural test for the Select.

### Slice B1 — Pack data section in the Explore sider

- **Where**: `ui/src/App.tsx` explore sider (below the asset tree) +
  `ui/src/features/packs/packSurfaces.ts`.
- **Change**: a "Pack data" block under the asset tree lists installed packs
  from `/namespaces` (`declaredBy: "pack:<id>"`) and, under each pack, its
  loaded import sources. Clicking a source navigates to the Reconciliation
  section.
- **Acceptance criteria**: with a pack installed and data loaded, Explore shows
  the pack and its sources; absent when no pack is installed; the asset tree
  is untouched.
- **Tests**: structural test with a stubbed `/namespaces` response.

### Slice B2 — Browse a source's graph subjects (deferred)

Open a source's subjects from Explore via `SubjectExplorer` seeded at a `gst:`
subject. Not built: no demonstrated need; costs a subject picker.

## Verification

UI-only. `npm test` + lint + typecheck on the touched modules. No Rust files
touched → no `cargo mutants` run needed for this plan.
