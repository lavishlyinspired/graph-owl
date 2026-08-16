# Plan 120 — graph-owl's console becomes domain-agnostic; reco-now becomes a domain investigation workspace

**Status**: Slices D (`408b631`), A (`63f9dd4`), C (`plans/121-label-resolution.md`, 4 sub-slices — `af5b23e`, `954917a`, `e907755`, `bd26e60`), E (`7f2f5d2`, narrower than originally scoped — see its own row below), B (`65c11ec`, restored onto the current data model rather than ported literally — see its own row below), H (`7c0d82c`, pure relocation, no behavior change — see its own row below) and F (`b69c6be`, dark theme kept per direct user choice, new indigo accent — see its own row below) shipped 16 August 2026, all verified live. G scoped, not started. Written in response to a direct architectural request: reframe reco-now from "reconciliation results viewer" to "domain investigation workspace powered by GraphOWL," determine what graph-owl's console's purpose actually is, move whatever is GST-specific out of the console and into reco-now, fix the data-persistence and label-resolution bugs found along the way, and restore a regression in the Ontology Builder.

**Companions**: `plans/00a-product-position.md` (graph-owl's stated purpose — read before anything else here), `plans/00f-ui-architecture.md` / `00h-ui-design-system.md` (console patterns and budgets), `plans/105f-gst-console-surfaces.md` (an already-existing gap analysis this plan builds directly on), `plans/107-filing-period.md` (the per-month/filing-period epic — already scoped, unbuilt, not re-scoped here), `plans/119-architecture-audit.md` (the reco-now/graph-owl integration work this plan follows on from).

**Method**: story-split per the `story-splitting` skill, grounded in three parallel Explore-agent investigations (console reconciliation panel, label resolution, Ontology Builder regression) plus direct verification of graph-owl's import/dedup semantics. Every claim below cites a file:line or a command actually run — nothing here is inferred from memory. This is deliberately not a single "do everything" epic; it is a set of independently valuable, independently shippable child stories, several of which do not depend on each other at all.

---

## 0. Determining graph-owl's purpose (the user asked this directly)

Not a new decision — already made, in `plans/00a-product-position.md`'s "A pack-level example: the GST pack vs. a reconciliation tool" section (added 12 August 2026, before this session):

> **graph-owl is not a GST product.** ... the honest differentiation is **findings vs. matches, not "better matching."** A dedicated reconciliation tool's core loop — row-level matching, explainable match reasons, supplier-level risk ranking — is a genuinely good product shape, **already built, and not a gap graph-owl should try to out-build with more matching rules.** What the flake model, retract-not-delete history, and reasoning-as-overlay make possible instead is carrying a match forward into *why*.

This is the answer, and it is more concrete now than when it was written: **reco-now is that "already built" dedicated reconciliation tool** — Plan 119's cutover (§9) made it consume graph-owl's native findings directly, with law-driven tolerance and cross-document reasoning no spreadsheet-shaped tool has. The comparison `00a` drew in the abstract is now literally true of this codebase.

So: **graph-owl's console's job is the platform's own differentiators — findings with evidence and governance, the graph they were derived from, the vocabulary that describes them, ad hoc querying — for *any* installed pack, not a best-effort GST invoice-matching UI that happens to be worse at matching than the tool built specifically for it.** Everything below follows from taking that literally.

---

## 1. What's actually there today (grounded findings)

### 1.1 The console's reconciliation surface is ~4,300 lines, partially pack-driven, substantially GST-hardcoded

| File | Lines | State |
|---|---|---|
| `ui/src/features/reconciliation/ReconciliationWorkspace.tsx` | 1,484 | the screen |
| `ui/src/features/reconciliation/statement.ts` | 714 | GST-shaped statement arithmetic |
| `ui/src/features/reconciliation/statement.test.ts` | 803 | |
| `ui/src/features/reconciliation/ReconciliationWorkspace.structural.test.ts` | 145 | |
| `ui/src/features/packs/packSurfaces.ts` | 250 | pack discovery — **hardcoded to GST only** |
| `ui/src/features/packs/importFile.ts`, `books.ts`, `gstr1.ts`, `gstr2b.ts` | 66 + 348 + 258 + 242 | GST file-format converters |

Genuinely pack-driven: currency/locale, match key, identity, field roles, measures, sources — all read from `packs/gst/pack.toml`'s `[console.reconciliation]` table (`pack.toml:788-931`).

Not pack-driven, confirmed by direct citation:
- `reconciliation.label`/`.subtitle` are **declared in `pack.toml` and never read** — the component hardcodes an identical string as `COPY.subtitle` instead (`ReconciliationWorkspace.tsx:95-96`). `record_noun = "invoice"` is declared and typed but has **zero consumers** — every "invoice(s)" string is a TS literal (`:143,349,391,447`).
- `COPY` (`:93-186`, ~90 lines) is GST prose hardcoded regardless of pack — "GST period," "As per GSTR-2B," "ITC is claimed head by head in GSTR-3B," etc.
- `invoiceColumns` (`:466-516`) and the CSV export header (`statement.ts:603`) hardcode `IGST`/`CGST`/`SGST`/`Cess`/`GSTIN` as column titles.
- `statement.ts`'s `Heads`/`SourceInvoice`/`HEAD_FIELDS` (`:32-88`) hardcode exactly six TypeScript fields (`taxableValue, taxAmount, igst, cgst, sgst, cess`) — a pack's `measures` can only label these six, never introduce a differently-shaped statement.
- `sourceQuery` (`:218-254`) hardcodes the bound SPARQL variable names `?gstin`/`?supplierName`.
- **Pack *discovery* itself is hardcoded**: `packSurfaces.ts:54-105` filters against `REGISTRY: readonly PackSurfaces[] = [GST]`. A deployment with only `hospitality` installed correctly shows "No domain pack is installed" — but *by accident*: `surfacesFor` cannot recognize any pack but `"gst"`, so a hypothetical third pack that declares `[console.reconciliation]` correctly and completely would **also** be invisible. An unused, already-correct `installedPacks()` helper exists in the same file (`:238-250`) without this limitation.

**Verdict**: this is not "a domain-neutral platform with one GST-flavored screen" — it is a GST-specific screen wearing a partial config layer. `plans/105f-gst-console-surfaces.md` (11 August 2026, before this session) already stated the discipline this violates: *"Everything below must land as pack-configured surfaces over the existing patterns, not console code that knows what GST is... a 'GST dashboard' that is not also a 'hospitality dashboard' would be the failure the hospitality pack exists to detect."* The code drifted from the document; per this project's own standing rule, the document is right.

### 1.2 Label resolution: the mechanism exists, but only on the one screen this plan is retiring

- **Findings queue and graph/Explore view use a purely syntactic label**: `displayTerm()` (`ui/src/features/review/findingsQueue.tsx:95-100`) strips an IRI to its local name. It never reads a literal off the subject. That is exactly why a Supplier subject renders as `supplier-27AAAFN2938K1Z2` — that string *is* "the local name"; there is nothing more for this function to do.
- **`SubjectExplorer.tsx:92`** (the graph view) is worse: only the clicked seed node gets a real label (`new Map([[seed, label]])`); every neighbour falls through to the same bare-id fallback.
- **Server-side, there is no resolution attempt at all**: `finding_evidence_graph` (`graph-owl-server/src/lib.rs:9264-9276`) builds each node as `{id, iri, sources, semanticType}` — no literal lookup. `EvidenceGraphNode`'s own doc comment (`ui/src/api.ts:643-665`) states the assumption directly: *"there is no catalog asset to resolve a display name from — the resolved IRI ... is the only label the server has."* That assumption is not true of a pack subject with a `gst:supplierName` (or hospitality's own name predicate) — it was true of the code that existed when the comment was written.
- **A working pattern already exists, once**: `packs/gst/pack.toml:823-833` declares `[console.reconciliation.fields] party_name = "supplierName"`, and `ReconciliationWorkspace.tsx` builds a bespoke SPARQL join for it (`:218-260`) and renders it in one table column (`:469,800`). It has never been generalized past that one screen.
- The manifest schema itself has no shared concept of this: `connectors/python/graph_owl_packs/manifest.py`'s `Manifest`/`Predicate`/`Document` dataclasses carry nothing label-shaped; `[console]` is parsed as untyped JSON, per its own doc comment (`pack_install.rs:193-217`): *"the console is the only consumer and it is the right place to know the shape."* True, but it means every console surface that wants a label has to invent its own reading — and only one ever did.

**Verdict**: not a missing feature, a wiring gap. The fix is to **generalize the one working pattern into a shared, pack-declared mechanism** (e.g., a label predicate per class or per namespace, consumed once by `displayTerm`/`nodeLabel`/`EvidenceGraphNode`), not to keep it bespoke to a screen this plan retires.

### 1.3 Ontology Builder: one narrow bug, one genuine regression, one straightforward merge target

- **"Can't select GST pack and view its ontology" — traced to a specific, narrow cause, not a console bug.** `main.py`'s `_install_graphowl_pack` calls `load_pack(GST_PACK_DIR, ..., include_documents=False)` — deliberate (keeps graph-owl's own demo fixtures out of reco-now's reconciliation runs, per its own comment, `main.py:234-243`) — but `include_documents=False` has no partial mode: it skips **every** `[[documents]]` entry, including `ontology.ttl`, not just the demo fixtures. Reco separately re-imports `law/*.ttl` (`main.py:251-258`) but never `gst-ontology`. Against the *standalone* console (not behind reco-now), pack loading defaults `include_documents=true` and this works correctly (`pack_install.rs:166-191`) — confirmed by my own live testing of the console earlier this session. **This is a one-file fix**: import `ontology.ttl` the same explicit way `main.py` already imports `law/sections.ttl`.
- **"Can't filter per namespace and view its graph — genuine regression, not misremembering.** `features/ontology/OntologyEditor.tsx` had a live namespace-filtered graph view (`namespaceFilter`, `allNamespaces`, `namespacesIn()`, a Cytoscape render). Commit `5cd2fd4` ("finish the shadcn/Tailwind migration... Plan 117") deleted that file and merged its Check/Save logic into `OntologyBuilder.tsx`'s Code tab — but **dropped the filter code rather than carrying it over**. `plans/00f-ui-architecture.md` documents the merge decision but never mentions preserving the filter. Today's Code tab (`OntologyBuilder.tsx:573-640`) has a format `Select`, Check/Save, and a plain `Textarea` — nothing else.
- **Workbench (SPARQL/Cypher)** is not its own feature folder — it's inline in the 4,707-line `App.tsx` (`:2061-2354`, ~290 lines, `Segmented` toggle at `:2153-2161`) plus `ui/src/workbench/*.ts` (~780 lines of pure helpers, already separable). Extracting it into a tab inside Ontology Builder is a straightforward relocation, not new design.

### 1.4 Data persistence: confirmed root cause of the "stale/hardcoded-looking" totals

Verified directly against `Catalog::import_rdf`'s own doc comment and code (`graph-owl-api/src/lib.rs:16303-16358`):

- **`import_rdf` dedupes only within the same `source`'s own import graph** — *"a subject already present in `graph:import:{source}` is reported `skipped` rather than re-landed."* A subject not already present is landed; a subject already present is **not updated**, only skipped.
- `graphowl_client.py`'s `_ingest_to_graphowl` mints a **fresh random `source`** on every upload — `f"reco-{kind}-{uuid.uuid4().hex[:8]}"` — so every upload lands in a genuinely new named graph, never the same one twice.
- Every finding query in `packs/gst/queries/*.sparql` uses an **unbound** `GRAPH ?register { }` pattern, which matches facts from **every** graph that has ever existed, old and new alike.

Net effect: re-uploading a corrected CSV does not replace anything — it adds a full parallel copy under a new source name, and every subsequent query silently aggregates across all of them. This is exactly consistent with the inflated totals reported (₹2,63,700.00 etc.) — accumulated test uploads across a long session, cleared only by the fresh-database resets already done as part of this plan's prerequisite cleanup, not by any code path that exists today.

**The fix already has precedent in this codebase.** `Catalog::delete_import` (`graph-owl-api/src/lib.rs:16469`) *"drops every triple the source ever landed"* and is already composed with `import_rdf` by `save_rdf_edit` (`:40696`, an internal ontology-editing flow) for exactly this "replace, don't accumulate" purpose. **No HTTP route exposes `delete_import` today** — it is reachable only from `save_rdf_edit`'s internal call. The correct fix is: (a) a new `DELETE /graph/import/rdf?source=...` route, thin wrapper over the already-tested `Catalog::delete_import`; (b) reco-now switches from a random per-upload source to a **stable, per-kind source** (`reco-books`, `reco-gstr2b`, `reco-gstr1`), calling the new DELETE before each `import_rdf` POST.

### 1.5 Per-month / filing-period view: already fully scoped, not re-scoped here

`plans/105f-gst-console-surfaces.md` §1 (11 August 2026) already names this "the biggest gap," and `plans/107-filing-period.md` already designs `gst:FilingPeriod` as a first-class graph entity, plus the query and slice sequencing to answer "what's outstanding for period X" and "show this invoice across every period it's appeared in." `plans/108-books-gstr1-gstr2b-reconciliation.md`'s own status line says its Slice 6 (period-aware carry-forward) *"remains blocked on `107-filing-period.md` Slice 1, still unbuilt."*

This plan does not redesign that work — it references it as Child Story G below, ready to pick up, and notes that shipping it also unblocks Plan 108 Slice 6.

---

## 2. Split Candidates

| Slice | Value | Includes | Defers | Acceptance examples | Release constraint |
|---|---|---|---|---|---|
| **A. Fix Ontology Builder pack-ontology loading in the reco-now deployment** ✅ **Shipped** | Can view/edit the real GST ontology instead of a permanently-disabled pack card | `main.py`'s `_install_graphowl_pack` separately imports `ontology.ttl` under source `gst-ontology`, same pattern as `law/*.ttl` | Any Ontology Builder UI change | Selecting "gst" in the Ontology Builder against a reco-now-run stack loads the real ontology, not "No ontology installed yet" | Shippable immediately, one file |
| **B. Restore namespace-filtered graph view in Ontology Builder** ✅ **Shipped, restored not ported** | Regression fix — filter an installed pack's ontology graph by namespace again | The deleted `OntologyEditor.tsx` filtered raw triples through Cytoscape; the current model has no raw triples once imported, so this landed as `EntityType.namespace` + `namespaceOf`/`namespacesIn`/`filterModelByNamespace` in `flowModel.ts`, filtering entities+relationships (a relationship survives only when both endpoints do) rather than a literal port of the old triple-level filter | A redesign of the filter UI beyond what existed before | Selecting a namespace shows only that namespace's classes/predicates in the graph; "All namespaces" shows everything | Shipped, commit `65c11ec` |
| **C. Generalize label resolution — any pack subject shows its declared name, not its IRI, everywhere** ✅ **Shipped** (`plans/121-label-resolution.md`) | Fixes the supplier-name bug in Findings *and* Explore; proves domain-neutrality (must also work for hospitality) | A pack-declared label predicate (per class or per namespace, `pack.toml` schema addition), consumed once by `displayTerm`/`nodeLabel`/`EvidenceGraphNode` server- and client-side, replacing the Reconciliation-only bespoke query | Rich label formatting (e.g. "Supplier · Nimbus Freight Logistics"); resolving labels for subjects with no declared predicate (falls back to today's `displayTerm` behavior) | A finding whose subject is a GST Supplier shows its `supplierName` in the queue row, the evidence panel, and the graph view; a hospitality subject shows its own declared label the same way, proving the mechanism isn't GST-specific | Independently shippable, no dependency on D/E |
| **D. Fix data persistence — a re-upload replaces prior data for the same source, never accumulates** ✅ **Shipped** | Directly fixes the reported stale/inflated totals; makes every other number in the system trustworthy | New `DELETE /graph/import/rdf?source=...` route (thin wrapper over `Catalog::delete_import`); reco-now's `graphowl_client.py`/`main.py` move to stable per-kind source names and call delete-before-import on each upload | A UI affordance for reviewing/rolling back a delete (not asked for; note as a parking-lot item if wanted later) | Upload books.csv, note totals; re-upload a corrected books.csv; totals reflect only the correction, not the sum of both uploads | Foundational — recommended first or second slice; everything else is easier to verify correctly once this is true |
| **E. Shrink the console's reconciliation surface to domain-neutral findings + evidence; retire the GST-specific invoice-statement table** ✅ **Shipped, narrower than scoped here** | Resolved by direct user direction instead of the deferred `story-splitting`/`grill-me` pass: keep the read-only statement/graph-tiles/supplier/findings view (all of it already pack-driven via `statement.ts`, not GST-hardcoded), remove only the upload cards (`SourceCard`) and the manual "Run reconciliation" button — reco-now's own upload flow already triggers graph-owl's native rule engine automatically, so both were duplicating a step reco-now already owns. The full ~3,000+-line removal this row originally described (retiring the statement table itself) did **not** happen — the user explicitly chose to keep it read-only rather than remove it entirely. | The `story-splitting`/`grill-me` pass this row called for; a from-scratch generic statement pattern; removing `statement.ts`'s six-field model | Fresh upload through reco-now → graph-owl's console shows the statement/tiles/supplier view/findings with no upload control and no run button, populated with no action taken on the page | Shipped, commit `7f2f5d2` |
| **F. Reco-now: reframe from "reconciliation results viewer" to "domain investigation workspace powered by GraphOWL"** ✅ **Shipped** | The product's own navigation/copy matches what it actually does post-cutover (Plan 119 §9) — surfaces graph-owl's real findings with evidence and governance, not a spreadsheet-style matcher | TopNav's "Reconcile" step relabelled "Investigate" (routing key unchanged); `ReconcilePage`'s "Reconciliation Results" heading relabelled "Findings"; a "powered by GraphOWL" tag beside the wordmark and in the browser tab title; dark theme kept (direct user choice over a light GraphOWL-palette switch) with the green "matcha" accent replaced by indigo (`#818cf8`, `--color-matcha-accent`/`-surface`, renamed from `-green` since a color still named green would mislead) | Any new capability — this slice is framing and presentation, not new data or logic | The nav and page titles read as an investigation tool; a fresh visual theme is applied consistently across Upload/Map/Reconcile/Intelligence/Act | Shipped, commit `b69c6be` |
| **G. Filing period / per-month view** | CAs can answer "what was my position last month, what changed this month" | Already fully designed in `plans/107-filing-period.md` — pick up as written | n/a, already scoped | See `107-filing-period.md`'s own acceptance criteria | Already release-planned; also unblocks Plan 108 Slice 6 |
| **H. Merge Workbench (SPARQL + Cypher) into Ontology Builder as a tab** ✅ **Shipped** | One place for vocabulary structure and ad hoc querying | `WorkbenchPanel.tsx` extracts `App.tsx`'s old `WorkbenchPage` (2061-2346) unchanged in behavior, mounted as a third `TabsContent` beside Visual/Code; `routes.ts` drops `"workbench"` from the CI-asserted route budget | Any change to Workbench's own query/result behavior | Workbench's SPARQL/Cypher toggle and results appear as a tab inside Ontology Builder; the old standalone "Workbench" nav entry is removed | Shipped, commit `7c0d82c` |

## Recommended First Slice

**D (data persistence), then A (ontology builder pack-loading fix).**

Why D first: it is the user's most concrete, currently-reproducible complaint, it is architecturally foundational (every other number and every other slice's manual testing is easier to trust once uploads behave correctly), it is well-bounded (one new Rust route reusing an already-tested method, one Python client change), and it is independently demonstrable end to end (upload, re-upload, confirm totals hold steady) without waiting on anything else in this plan.

Why A second: it is the cheapest possible slice in this entire plan (a few lines in `main.py`, mirroring a pattern already used two lines above it for `law/*.ttl`), and it unblocks *visually verifying* B, C, and the ontology side of this plan in the browser rather than only through curl.

C (label resolution) is the next highest-leverage slice — it is cross-cutting (fixes Findings *and* Explore in one mechanism), and because it must work for hospitality too, building it correctly is itself a domain-neutrality proof, the same discipline `105f` already demands. B and H are the cheapest remaining UI slices and can land in either order once A is in.

E and F are the actual "reframe" the user asked for by name, and are deliberately left as a parking-lot item rather than a committed slice: E in particular needs its own decision-tree pass (what, if anything, replaces the invoice-table workflow in the console once it's gone — nothing, a redirect to reco-now, or a genuinely generic "statement" pattern usable by any pack) before it can be split into safely shippable pieces.

## Parking Lot

- **E's own decomposition** — run `story-splitting` (or `grill-me`, since "what replaces the invoice table in the console, if anything" is a real open decision, not a splitting-mechanics question) once C and D have shipped.
- **A UI affordance for reviewing what a delete-before-reimport (Slice D) is about to remove**, if accidental data loss on a bad re-upload becomes a real concern — not asked for, noted only as a possible follow-up.
- **`record_noun`, `reconciliation.label`, `reconciliation.subtitle`** in `packs/gst/pack.toml` are already declared and already unused (§1.1) — trivial to wire once any part of E starts, not worth its own slice.
- **Rich label formatting** for Slice C (e.g., composing multiple predicates, or a class-level "primary label" vs. a subject-level override) — start with the simplest version (one declared predicate per class, falling back to today's `displayTerm` when absent) and let a second slice add richness only if a real need appears.
- Whether `Heads`/`SourceInvoice`'s six fixed tax-head fields (`statement.ts`) should become genuinely generic or stay GST-specific is a real open question belonging to E's own split, not decided here.

## Warnings

- **E is not a component split in disguise** — "shrink the console panel" and "reframe reco-now" are two different user-facing capabilities (an operator using graph-owl directly vs. a CA using reco-now) that happen to be causally linked; keep them as separate slices (E and F) rather than one "move the code" task, and sequence F to land only once E's scope is actually decided, so reco-now's new copy doesn't promise a surface that hasn't arrived yet.
- **Slice D changes an HTTP contract** (a new DELETE route) — regenerate and diff `openapi.json` as part of that slice, and note `cargo public-api -p graph-owl-api`'s snapshot was already found stale before this plan (Plan 119 §10) — don't let D's real, deliberate surface addition get lost in that pre-existing drift; regenerate deliberately as part of D, not by accident.
- **C's label predicate must be genuinely pack-declared, not GST-shaped in a new place** — the failure mode `105f` already names ("a GST dashboard that is not also a hospitality dashboard") applies exactly as much to a label-resolution feature as to a reconciliation screen; the acceptance example above deliberately requires proving it against hospitality, not just GST.
- **Do not start G (filing period) as a side effect of D** — they touch adjacent concepts (both about "what happened when") but Plan 107 is a separate, already-designed body of work with its own entry conditions; conflating them risks reopening a design that's already settled.

## Next Step

Select D as the first slice and load `planning` to turn it into PR-sized implementation stages — every stage must run the full `tdd`/`testing`/`mutation-testing`/`refactoring` cycle (RED-GREEN-MUTATE-KILL MUTANTS-REFACTOR) before the next stage starts, per this project's standing process. A is small enough it may not need its own `planning` pass — implement it directly with RED (a test asserting `gst-ontology` is imported) → GREEN, same discipline, just without a multi-stage plan.

### D — shipped

Commit `408b631`. `DELETE /graph/import/rdf?source=...` route added (admin-gated,
same source-validation as the existing POST), 14/14 tests in `graph_import.rs`
pass. `graphowl_client.delete_document` added with 5 new tests; `main.py`'s
`_ingest_to_graphowl` now uses a stable `f"reco-{kind}"` source and calls
delete-before-import, with 2 new tests asserting the sequence. `openapi.json`
diff came back empty — the route is not schema-captured, matching earlier
precedent for this codebase, so no snapshot update was needed.

Verified live end to end, not just against the test database: fresh Postgres,
uploaded the real 3-file Aug 2026 sample fixture via `POST /api/upload`,
landed 33/26/10 triples (books/gstr2b/gstr1), and `/api/reconcile`'s stats
matched the hand-derived answer key exactly (14 total, 7 matched, 2 review,
3 only_books, 2 only_gstr2b). Re-uploaded the identical files: landed counts
and reconcile stats were byte-for-byte identical, not doubled.
`verify-reconcile-parity.py` still passes. Next up: A.
