# Plan: SHACL authoring workflow — seed, import, preview, commit, all from the Validation screen

**Branch**: main (per this repo's commit-freely convention)
**Status**: Active

## Goal

The Validation screen (`graphowl-app`) showed all zeros with no way to tell "never validated" from "genuinely clean," and no way to trigger validation at all except a raw `curl`. Root cause, confirmed by reading `graph-owl-constraint`/`graph-owl-api` directly: `POST /validation/shapes/seed` only ever writes one fixed, hardcoded built-in shape set (`TableShape`/`ColumnShape`/`ConfidenceShape`/`RelationshipShape`/`EnvelopeShape` — Epic 5's core-entity-model shapes), which don't even target the GST demo pack's classes (`gst:Supplier`, `gst:PurchaseInvoice`, ...) — so a real validation run against them finds nothing to flag, not because the data is clean by inspection, but because nothing in the graph is a candidate.

This plan closes three real gaps: no UI to trigger seed/run, no way to know when validation last ran, and no way to write/import a *custom* shape (the only way to get a validation result that actually says something about domain data like the GST pack).

## What already exists (read from source, not assumed)

- `graph_owl_constraint::shapes::core_shapes(t) -> Vec<Flake>` — the fixed built-in set, pure, no I/O.
- `graph_owl_constraint::shapes::read_all(facts: &[Flake]) -> (Vec<CompiledShape>, Vec<ShapeError>)` — compiles raw shape flakes, tolerant of individual bad shapes.
- `graph_owl_constraint::validate(&compiled, &base) -> Report` — pure, no I/O. This is the key enabler for preview: call it against shapes that have **not** been written to storage yet.
- `graph_owl_rdf_io` — Turtle parsing already exists (`oxttl`, MIT/Apache-2.0, already an approved dependency), including `sh:` prefix registration and line/column error locations (`parse_turtle_with_location`).
- `Catalog::seed_core_shapes` — writes `core_shapes(t)` via `graph.assert_flakes`, returns only a count today.
- `Catalog::run_validation` — reads `shapes_graph()` context, compiles, validates against `asserted_base()`, persists a `ValidationRun`. Unchanged by this plan.
- `shapes_graph()` — the dedicated named-graph context shapes live in, separate from ordinary business data (why a generic `/sparql` query never finds them — confirmed live).
- `flake_body(&Flake) -> serde_json::Value` (`graph-owl-server/src/lib.rs`) — the existing `{s, p, o, t}` JSON rendering used elsewhere (`/reasoning/runs`'s retraction DTO neighbourhood); reused rather than inventing a second flake JSON shape.

**Nothing here needs a new crate or a new parsing dependency** — `00e`/`00l`'s checks are already satisfied by what Epic 5 and the RDF-IO epic already brought in.

## Acceptance criteria

- [x] An admin can seed the built-in shapes from the Validation screen (not curl) and see the actual flakes written, not just a count.
- [x] The Validation screen shows when validation was last computed, distinguishing "never run" from "run, genuinely clean." (Shown as the graph's own logical instant, not a fabricated wall-clock "X minutes ago" — `computedAtT` has no real-time mapping exposed by this API.)
- [x] An admin can paste or upload custom SHACL (Turtle) and preview what it would find against the real graph, with zero persistent effect if they don't commit.
- [x] An admin can commit previewed shapes; the flakes written are shown, and the Validation report reflects real findings from them on the next run.
- [x] Malformed Turtle or a structurally invalid shape (`ShapeError`) is reported with a real location/reason, never a silent no-op.

## Slices

### Slice 1: Seed + last-validated timestamp, self-service from the UI

**Value**: An admin sees why the screen was empty (never seeded/run) and can fix it themselves in two clicks, with the actual flakes written shown as proof something happened.
**Path**: `Validation` screen → "Seed built-in shapes" button → `POST /validation/shapes/seed` (extended to return flakes, not just a count) → "Run validation" button → `POST /validation/runs` (unchanged) → `GET /validation/report` (already returns `computedAtT`) → screen shows "Last checked <relative time>" or "Never validated."
**Backend RED**: `Catalog::seed_core_shapes` returns `Result<Vec<Flake>, CatalogError>` instead of `Result<usize, CatalogError>` — a unit test asserting the returned flakes match `core_shapes(t)` exactly (not just a count), and that seeding twice returns the same flakes both times (idempotency, already covered structurally by `core_shapes`'s own existing test but not through this method's return value). HTTP test: `POST /validation/shapes/seed` response body contains a real `flakes: [...]` array shaped like `flake_body`'s output, not only `flakes: <count>`.
**GREEN**: Change the return type; the one caller (`seed_core_shapes` HTTP handler) renders `flakes.iter().map(flake_body).collect()` alongside the count.
**Frontend RED**: a `vitest` test for a new `seedCoreShapes()`/`fetchValidationReport()` pairing rendering "Last checked" from `computedAtT`, and a "Never validated" state when `computedAtT` is `0`/absent.
**Acceptance criteria**: Seed button calls the real endpoint and shows a real flake count + a togglable list of what was written; Run button calls `/validation/runs`; the header shows a real relative timestamp or "Never validated," verified live against the real server (not mocked).

### Slice 2: Preview custom SHACL — paste or upload, zero persistent effect

**Value**: An admin can try a shape against real data before deciding to keep it — the actual reason "preview" was asked for.
**Path**: New `POST /validation/shapes/preview` (any authenticated principal — it writes nothing) — body: raw Turtle text. Parses via `graph_owl_rdf_io::parse_turtle` with every flake's `cx` forced to `shapes_graph()` (parsed Turtle has no graph context of its own), compiles via `read_all`, dry-runs `graph_owl_constraint::validate(&compiled, &Catalog::asserted_base(...))` — **never calls `assert_flakes`**. Response: parsed flake count, compiled shape count, any `ShapeError`s (with the parser's line/column when the failure is a parse error, not a structural one), and the would-be violations/warnings the same shape as `/validation/report`'s rows.
**Backend RED**: a shape requiring a property no GST invoice carries previews with N real violations against the live `asserted_base()`, with **zero flakes present afterward** in `shapes_graph()` (proves nothing was written) — the sharpest mutant here is a preview that accidentally calls `assert_flakes`, so the RED test must assert storage state before and after, not just the response body. A second case: malformed Turtle previews a parse error with a real line/column, not a generic 400. A third: valid Turtle that is not a legal shape (`ShapeError::NoTarget` etc.) previews that specific reader error.
**GREEN**: new `Catalog::preview_shapes(turtle: &[u8]) -> Result<ShapesPreview, CatalogError>`, thin HTTP wrapper, `openapi.rs` entry.
**Frontend**: a SHACL editor (textarea, monospace, matching the Queries tab's existing styling) plus a file-upload control (`<input type="file" accept=".ttl">`, reads as text client-side — no separate upload endpoint needed, the body is the same raw Turtle either way); "Preview" button renders parsed-shape summary + would-be violations, or the parse/structural error with its location.
**Acceptance criteria**: pasting a real shape against the live GST pack shows real, non-zero preview violations; re-checking `shapes_graph()` afterward (via the existing `/validation/runs` shape count) proves nothing was committed; a broken Turtle snippet shows a real parser location, not a generic failure.

### Slice 3: Commit previewed shapes, see what was written, report reflects them

**Value**: Closes the loop — what was previewed can be kept, and the Validation report becomes genuinely informative for domain data (not just the core entity model).
**Path**: New `POST /validation/shapes/import` (admin-only, matching `seed_core_shapes`'s gating) — same parse+compile as preview, but on success calls `graph.assert_flakes` and returns the written flakes via `flake_body`. UI: "Commit" button appears only after a successful preview of the *same* pasted text (a commit of unreviewed text is not offered — the preview step is load-bearing, not decorative); on success, shows the flakes written and prompts "Run validation now?" to close the loop into Slice 1's Run button.
**Backend RED**: importing the same shape previewed in Slice 2 leaves `shapes_graph()` containing exactly those flakes; a subsequent `run_validation` reports the same violation count the preview predicted (proving preview and commit share the same evaluation path, not two implementations that can drift); importing malformed Turtle writes nothing (same guard as preview, now checked on the writing path too).
**GREEN**: `Catalog::import_shapes(turtle: &[u8]) -> Result<Vec<Flake>, CatalogError>`, thin HTTP wrapper, `openapi.rs` entry.
**Acceptance criteria**: commit → real flakes shown → Run validation → real report row appears for the GST pack, verified live end-to-end through the actual running console (not curl only, this time — the UI is the point).

## Why SHACL never worked against the GST pack — root cause and fix

Asked directly: "we already have an ontology defined for GST, why not SHACL?" The honest answer needed tracing `asserted_base()`, not guessing.

`Catalog::import_rdf` — the path every connector, including the GST pack loader, writes through — always lands its data in a named `graph:import:{source}` context (`graph:import:gst-purchase-register`, `graph:import:gst-gstr2b`, etc. — confirmed live via SPARQL: `SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s a gst:Supplier } }`). That's deliberate: it's what makes `delete_import` able to drop a bad import wholesale without touching anything else. But `run_validation`/`preview_shapes`/`import_shapes` all read their estate through `asserted_base()`, which queried `cx: Some(None)` — the default graph *only*. A shape correctly targeting `gst:Supplier` therefore reported zero violations against data that was genuinely, confirmably present — not because the data conformed, but because the query never read the graph it lived in. **Not GST-specific**: this blocked every connector's data from ever being validated by anything, built-in shape or custom one alike.

`asserted_base()`'s own doc comment already stated the real intent — exclude the reasoning overlay so inference can't read its own conclusions — but the implementation over-delivered on that by excluding every named graph, not just the overlay. `reasoning_base()` (used by `run_reasoning`) already has the correct fix pattern for a related problem: an explicit `Budget::include_graphs` opt-in layered on top of `asserted_base()`.

**Fix, deliberately validation-only**: a new `Catalog::validation_base()` — `asserted_base()`'s twin, querying `cx: None` (every graph) and filtering out only the three synthetic overlays (`graph:shapes`, `graph:reasoning`, `graph:extraction`). `run_validation`, `run_validation_as` and `preview_shapes` (and therefore `import_shapes`, which delegates to it) now use it. `asserted_base()` itself is untouched, and `reasoning_base`/`run_reasoning` still call it unchanged — reasoning already has its own careful, explicit opt-in design, and silently widening its default base to include every connector's data was a real, unrequested behavior change to a different feature, not a byproduct worth accepting quietly.

**Verified live**: a shape requiring `gst:reviewedBy` (minCount 1) on `gst:Supplier` now correctly reports `conforms: false, violations: 19` against the real GST pack's 19 real suppliers, with real focus nodes (`1024:supplier-06AAKCA0977G1Z3`, ...) and a real `assertMissing` suggestion per row. The built-in core-entity shapes are unaffected (`conforms: true` — they target `dsc:` classes GST data never uses, so widening the scan added no new candidates for them).

Two new unit tests (`validation_reaches_connector_imported_data`) prove: (1) an offender in a connector import graph is caught, (2) the reasoning/extraction overlays stay excluded even though the scan is wider — the second is the sharpest mutant here, since a blanket "read everything" fix would pass the first test just as well as the correct one.

## A real bug, found live and fixed with a regression test

`Catalog::import_shapes` originally reused `preview_shapes`'s parsed flakes verbatim — including their `t`, which a Turtle parse never sets (`oxttl` defaults to `0`, not a real graph instant). Every unit test (in-memory `RecordingGraph`) and the first HTTP integration test passed anyway, because both started from a genuinely empty shapes graph, where `t: 0` is indistinguishable from "first."

Found live, against the real dev database, once the built-in shapes were already seeded at a real `t` (~389) *before* importing a custom one: the import reported success, the flakes really did land in Postgres (confirmed directly via `docker exec ... psql`, `cx = graph:shapes`, `t = 0`), but a subsequent real `run_validation` never counted it — `shapes` stayed at the built-in count, never `+1`. The written data was correct; it simply never surfaced again.

Root-caused, then reproduced deliberately as a new regression test (`importing_after_the_built_in_shapes_are_already_seeded_still_takes_effect`) that seeds the built-in set *before* importing — the ordering that makes the bug reproducible, which is exactly what the first, order-independent test could never exercise. Confirmed RED, then fixed by stamping a fresh `t` from `graph.next_time()` immediately before the write — the same pattern `seed_core_shapes` already used and `import_shapes` should have from the start. The returned `ShapesPreview::Checked.flakes` now carries the real, written `t` too, so the UI's flake viewer shows what's actually in the graph, not what the parser happened to default to.

Verified fixed against the real dev server: seed → import → run now correctly reports `shapes: 6` (5 built-in + 1 custom), with a real `t` on the imported flakes.

## Restarting the dev server: what actually happened

Loading the new endpoints required rebuilding and restarting the long-running `graph-owl-server` process. Two things broke on restart, neither a regression in this work:

1. **Auth mode flipped from open to OIDC.** `.env` currently sets `OIDC_ISSUER`, but the process that had been running all session was started before that was added (or with an override) — it never re-read `.env`, so it kept running open. A fresh process reads the current `.env` and genuinely requires bearer tokens. Fixed by launching with `OIDC_ISSUER=` (matching `scripts/demo.sh`'s own documented escape hatch) to restore the open-mode behavior every previous curl call in this session relied on.
2. **The dev Postgres (`graphowl-demo`, port 55432, via `scripts/demo.sh`) had stopped.** Restarting `graph-owl-server` alone surfaced a connection-pool timeout; `docker start graphowl-demo` (data preserved, not recreated) brought it back — confirmed the real GST pack data (35 suppliers) and prior session state were intact afterward.

## Slice 4: readable shapes, real results everywhere, a GST shape gallery

Three follow-on gaps, reported directly against the shipped Slice 1–3 UI: "Seed built-in shapes" showed "57 flakes written" with no way to know which shapes those were; neither seeding nor (less obviously) the author flow surfaced validation results consistently; and there was nothing to select against the GST pack short of hand-writing Turtle from a blank textarea.

**Backend — `describe_shape`, and every write path returns it:**

- `CompiledShape::shape()` (`graph-owl-constraint`) — a new accessor exposing the full `Shape` (target, constraints, message), previously only reachable via `id()`/`severity()`.
- `ShapesPreview::Checked` gained `shape_details: Vec<Shape>` alongside the existing `flakes`. `preview_shapes`, `import_shapes` (which delegates to preview), and now `seed_core_shapes` all populate it — `seed_core_shapes`'s return type changed from `Vec<Flake>` to `ShapesPreview`, running the *same* `read_all`/`validate` pair the other two paths use against `validation_base()` right after writing, so seeding now reports real `conforms`/`violations`/`warnings`/`info` instead of silence.
- `describe_shape`/`describe_target`/`describe_constraint` (`graph-owl-api`) render a `Shape` as `{ id, target: { kind, value }, severity, message, constraints: [{ path, kind, detail }] }`. **The one real finding while writing this**: every `sh:property` branch compiles to `Constraint::And([...])`, even for a single constraint (`read_branch`'s own doc comment: "an `And` of one is fine" — never unwrapped, because it doesn't change how a *violation* reads). Left as-is, a rendered shape summary would show every ordinary property as an opaque "all of 1 constraints" instead of the real constraint underneath. Fixed with `flatten_constraint`, which recurses through `And` (only) to reach the real, path-bearing leaves — `Or`/`Not` are deliberately left unflattened, since collapsing "either A or B" into a flat list would misstate it as "A and B."
- HTTP: `shapes_preview_body` renders `shapeDetails` via `describe_shape`; `POST /validation/shapes/seed`'s handler now returns the same envelope `/preview` and `/import` already did, instead of a bespoke `{ flakes }` shape.

**Frontend — one shared result view, a curated GST gallery:**

- `ShapesResultView` (new, in `ShapesPanel.tsx`) renders a `ShapesPreviewResult["kind"="checked"]` — KPI row, refused-shapes warning, readable `ShapeDetailsList`, the flake toggle, and the sample-violations table — used for **all three** actions (seed/preview/import) instead of seed getting a bare flake count while preview/import got the rich view.
- `ShapeDetailsList` renders each shape's real target and constraint list in plain language (`gst:reviewedBy: at least 1 required`), sourced entirely from the backend's `describe_shape` output.
- `src/lib/gstShapeTemplates.ts`: four curated shapes targeting the real GST pack, selectable from a dropdown that populates the textarea. Every property name and every stated outcome (conforms, or N of M missing) was verified live against the real pack data before being written down — not guessed:
  - *Suppliers have a name* — conforms (18/19; one supplier is missing it).
  - *Suppliers are reviewed* — catches all 19 (no supplier has ever been reviewed).
  - *Purchase invoices have a taxable value and date* — conforms (32/32).
  - *Purchase invoices state their HSN code* — catches 16 of 32.

**Sidebar text color**: `chrome/Rail.tsx`'s `NavLink` base color moved from `text-gowl-t3` to `text-gowl-t1` (hover to `text-gowl-t0`) — only the clickable nav items, not the group headers (`HOME`, `GOVERNANCE`, ...), which stay at their existing dim `text-gowl-t7`.

**Verified live**: seeding shows "SHAPES 5 · CONFORMS yes" plus all five built-in shapes' real targets/constraints spelled out. Selecting "Purchase invoices state their HSN code" from the gallery, then Preview, shows "SHAPES 1 · CONFORMS no · VIOLATIONS 16" with real focus nodes (`1024:pr-INV-1001`, ...) — matching the template's own stated, pre-measured outcome exactly. 758 Rust unit tests (`graph-owl-api` + `graph-owl-constraint`), 17 HTTP integration tests, 397 frontend tests all pass; `tsc`/`eslint`/`clippy`/`cargo fmt` clean throughout.

## Explicitly not attempted this pass

- Editing or retracting a previously-imported shape (only additive import; matches `seed_core_shapes`'s own existing idempotent-additive design).
- SHACL-SPARQL constraints (`sh:sparql`) through the import path — `run_validation_as`'s existing SPARQL-constraint authorization work is orthogonal and untouched; imported shapes in this plan are the structural/property-shape subset `read_shape` already parses.
- A shape library/catalog UI (saved named shape sets) — one paste-box at a time, matching the smallest version that proves the path.

## Pre-PR Quality Gate

Each slice: `cargo check -p graph-owl-api -p graph-owl-server`, targeted `--lib` tests, `cargo mutants` scoped via `--in-diff` on the touched files before commit; `npx tsc --noEmit`, `npx vitest run`, live verification via `agent-browser` against the real running server. Full `scripts/gate.sh` deferred per this repo's standing rule (only on request or after several epics accumulate).

---
*Delete this file when the plan is complete.*
