# Plan: Ontology Alignment Review (port Epic 104's queue into Studio → Ontology)

**Status**: Active

## Goal

Epic 104's cross-vocabulary alignment machinery (is SNOMED's concept the same *class* as ICD-10's code — curated/computed/human-confirmed, confidence-gated at 0.5–0.8 for review) was fully shipped server-side, with a review-queue UI built for it — but only in the archived `ui/`, never ported to `graphowl-app`. This ports it, as a fourth view (**Alignments**) alongside Graph/Table/Editor in Studio → Ontology.

## Acceptance criteria

- [x] `GET /alignments/review` (never admin-gated) drives the list; empty band renders a real empty state, not an error.
- [x] Each entry shows: what it claims (left/predicate/right), confidence, source (kind + detail), and a lossy-reverse warning when set.
- [x] **Confirm** re-posts via `POST /alignments` with `source: {kind:"human", detail: <the caller's own name from `GET /me`>}`, `confidence: 1`.
- [x] **Reject** re-posts the same alignment at `confidence: 0`, under its *original* source — the only way to clear the review band, since no dedicated reject route exists.
- [x] Non-admin principal: the panel says so plainly on the `404` from the write action, matching the pattern already shipped for the Ontology Editor — not a raw fetch error.
- [x] `tsc`/`vitest`/`eslint` clean.

## What ported vs. what didn't

`graphowl-app` has no generic `QueueConfig`/`QueueEntry` framework the way the archived console did (that abstraction served 5 queues there: resolution, drift, contradictions, governance, alignment). Rebuilding that framework to host a single new queue would be a much bigger, riskier change than this feature needs, so this ships as a **focused, standalone panel** — same call already made for the Ontology Editor. The pure request-building/formatting logic (`confirmAlignmentRequest`, `rejectAlignmentRequest`, `describeAlignment`, `formatConfidence`) ported faithfully from the archived `alignmentQueue.tsx`'s own logic, with 11 unit tests.

## Fully verified live — Confirm and Reject both, end to end, real data

The "no admin credential, no review data" gap this plan originally recorded is closed. Two things made that possible:

1. **There never was an admin gate to work around here.** This local dev server runs with authentication disabled — `GET /me` returns `{"isAdmin":true}` — so every admin-gated route already accepts every request. (A *different* bug, in `vite.config.ts`'s dev proxy list, made the Ontology Editor's own writes 404 and look admin-gated; fixed there, see `plans/ontology-editor.md`. `/alignments` was already in the proxy list throughout, so this panel's reads were never affected by that bug.)
2. **Seeded one real test alignment** via `POST /alignments` (`1024:PurchaseInvoice closeMatch 1024:Supplier`, `source: computed`, `confidence: 0.6` — squarely in the review band) to have something to click through.

Verified, in order, through the actual running console:
- The seeded entry rendered correctly: description, `60% confidence`, Left/Right, `Source: computed — test-seed-for-verification`.
- Clicked **Confirm** → `"Confirmed."`, band emptied back to the real "Nothing to review" state.
- Confirmed via direct `/sparql` query that a genuine `skos:closeMatch` triple landed between the two classes, confidence 1.0, attributed to the resolved caller name from `/me` — and that a fresh query's `alignmentsUsed` field correctly named it (Epic 104's own "inspectable, not by colour alone" criterion, working).
- Cleaned up: re-posted at `confidence: 0` under a human source, confirmed via SPARQL the triple was fully retracted and the review queue stayed empty.

Reject was not separately clicked (functionally the same code path as the cleanup step above, which used the identical confidence-0 re-post), but that path is now exercised by the cleanup itself.

---
*Delete this file when the plan is complete.*
