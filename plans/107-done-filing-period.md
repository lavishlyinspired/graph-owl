# Plan 107 — filing period as a first-class graph entity

**Status**: All 5 slices shipped 16 August 2026, verified live throughout. Slice 1 `440c78d`, Slice 2 `df5e3bd`, Slice 3 `c84ae61` (plus a Slice 4 prerequisite, `period-list`, and a regression test for a phantom-placeholder trap found three times, `7fc0ec7`), Slice 4 `da459fc`, Slice 5 `eb2b37a`. **Branch**: main.
**Trigger**: an external competitive review of the GST pack against a dedicated
GSTR-2B reconciliation tool (12 August 2026) observed that graph-owl cannot
answer "what changed between April and May" as a graph traversal. Verified
against the repo, not taken on faith: `packs/gst/ontology.ttl:34` declares
`gst:period` as a `gst:Property` — a scalar literal on a fact (`pack.toml`'s
`GstinTransposition` rule binds it as an evidence variable, `?period`) — and
`plans/00c-domain-model.md` has zero occurrences of `FilingPeriod` or any
period-shaped entity. The gap is real: there is nothing in the graph a query
can traverse *from* to ask a period-scoped or cross-period question; every
such question today would have to filter on the `gst:period` string per fact,
by hand, per query.

Positioning note added the same day, `plans/00a-product-position.md`'s "A
pack-level example" section, names this gap without claiming it solved —
this plan is that follow-up, run through story-splitting before planning per
this project's own convention (`grill-me`/`story-splitting` before
`planning`, per `CLAUDE.md`).

**What already exists, so this is additive, not foundational.** P8
(`105u-date-window-traversal.md`, `105i-temporal-validity-resolution.md`)
shipped date-window traversal and `as_of`/time-travel at the storage layer —
the *mechanism* for "facts as of a date" already works. What's missing is an
*entity* a period's facts hang off of, and the domain-neutral place to put
one is exactly the mechanism Plan 106 Slices 4a+4b just shipped: a pack's own
ontology (`gst:FilingPeriod` as a new class in `packs/gst/ontology.ttl`, pure
config, zero Rust/TypeScript) plus a registered `run_pack_query` (the same
named-parameterized-query pattern `provision-in-force.sparql` already
proves) rather than new engine code. This is a pack-content epic, not an
engine epic — the routing table in `CLAUDE.md`'s "Which `00*` docs bind
which work" applies `00c`/`00d` only loosely here, since a pack subject
(per this session's own `Sid::is_runtime_pack_namespace` work) is not a
catalog `Entity` and does not go through the asset envelope `00c` defines
for `DatabaseService`/`DashboardService`/etc. — it is a graph subject
declared and instantiated entirely inside the GST pack, the same way
`gst:PotentialMismatch` findings already are.

## Parent

**Actor**: a CA (or the investigation agent acting for one) reviewing GST
compliance for a business.
**Need**: ask a question scoped to one filing period ("what's outstanding
for July 2020"), or a question that compares two periods ("what changed
between April and May", "has this invoice's status moved since last
month"), and get a real graph answer — not a query the agent re-derives
from raw `gst:period` string filters every time, the same rediscovery
problem Plan 106 Slice 4 fixed for `gst:governedBy`.
**Outcome**: period and cross-period questions become a named capability
(a registered query, eventually a console surface) instead of something
every caller reinvents.
**Current constraint**: `gst:period` is a scalar property, not a subject.
Nothing links a period to its facts as a *set*; nothing represents "April
2020" as a thing with its own identity a query can start from.

## Recommended First Slice

**A `gst:FilingPeriod` subject exists per period already present in the
data, and one registered pack query answers "what's in period X" by
traversing to it — not by filtering on the literal.**

Why this first: it is the walking skeleton — smallest real slice that
proves the entity is worth having at all, uses only mechanisms that already
exist (pack ontology declaration, the pack loader, `run_pack_query`), needs
no new source data (every period value already exists in the fixtures), and
is independently demonstrable without committing to the harder cross-period
comparison design yet. If it turns out nobody uses period-scoped queries
over the literal-filter equivalent, the parent stops here having cost one
slice, not four.

## Split Candidates

| Slice | Value | Includes | Defers | Acceptance Examples | Release Constraint |
|---|---|---|---|---|---|
| **1. FilingPeriod exists, one query traverses to it** ✅ **Shipped** | Proves the entity is worth having; unblocks everything else | `gst:FilingPeriod` class + `gst:belongsToPeriod` (an `owl:ObjectProperty`, domain `PurchaseInvoice` per the same "known, accepted limitation" `issuedBy` already documents) in `packs/gst/ontology.ttl`; `fixtures/filing-periods.ttl` declares one `FilingPeriod` instance per distinct `gst:period` value already in the fixtures (`period-2020-07`/`period-2026-07`/`period-2026-08`); each existing fixture subject gets a `belongsToPeriod` triple next to its own `gst:period` literal; `queries/period-summary.sparql` (registered via `[[queries]]`, needed its own `[[predicates]]` entry too — a real gap the RED test found, since a property declared only in `ontology.ttl`'s own triples is descriptive, not a runtime registration) answers "everything belonging to period X" by traversing the edge | Cross-period comparison; console UI; any new source data beyond what fixtures already carry | `run_pack_query(gst, period-summary, {period: <the FilingPeriod's own IRI>})` returns every invoice/fact for that period, traversing the edge; a period IRI naming no `FilingPeriod` returns an empty answer, not `NotFound` — both verified live against the real fixtures, not only the synthetic Rust test | Shipped, commit `440c78d` |
| **2. Cross-period diff, one direction** ✅ **Shipped, resolved differently than sketched here** | Answers the reviewer's actual example ("what changed between April and May") for the narrowest real case | `period-diff.sparql`: two `VALUES` rows, not the "invoice status" reading this row originally sketched — resolved directly (grill-me is user-invocable only) in favor of generalizing `period-summary`'s own `belongsToPeriod` traversal, since a `PurchaseInvoice` belongs to exactly one period by construction and an invoice-lifecycle diff would need the heavier canonical-`Invoice` multi-hop path. `SELECT DISTINCT` needed after the RED test found a self-diff without it double-counts every row | Full field-level diff; more than two periods at once; any "since last N periods" rolling window (Slice 5) | Every subject belonging to exactly one of two named periods, tagged which — verified live against the real fixtures | Shipped, commit `df5e3bd` |
| **3. A subject's full period history** ✅ **Shipped** | Answers "show this invoice across every period it's appeared in" — the third example from the trigger | `period-history.sparql`, parameterized by subject, traversing every `belongsToPeriod` edge a subject has, ordered by the period's own literal. `belongsToPeriod`'s declared `many = false` turned out to be enforced only within one import batch (found live, not assumed) — a genuinely multi-period subject needs two separate imports, not one | Aggregation/summarization of the history (that's a console concern, not a query concern) | A subject linked to more than one period reports all of them in period order — proved against edges deliberately imported out of order | Shipped, commit `c84ae61` |
| **4. Console surface** ✅ **Shipped** | Makes 1–3 usable by a human, not only the agent | `ui/src/features/filingPeriods` — a period picker plus an optional "compare against" picker, calling `period-list`/`period-summary`/`period-diff` via a new `api.runPackQuery`. A real bug surfaced only by testing live in a browser: deriving table columns from the picker's own mode raced the async fetch and crashed on the first render after picking a second period; fixed by deriving columns from the loaded result's own `variables` field instead | Editing anything — this is read-only, matching `run_pack_query`'s own non-admin-gated posture | Verified live against a running demo, matching every other console slice in this project's own convention | Shipped, commit `da459fc` |
| **5. Rolling/relative period queries** ✅ **Shipped, without a relative offset parameter** | Removes the need to know exact period literals to ask a comparison question | `periods-before.sparql` takes one period and returns every `FilingPeriod` before it, most recent first — deliberately not calendar arithmetic (subtracting a literal month could resolve to a period that was never filed) and deliberately no `LIMIT` (no query in this pack had used one; a caller wanting exactly one takes the first row, and the same query composably answers "last N periods" too) | Calendar-locale reasoning, fiscal-year handling beyond what GST already assumes | Given `2020-07` and `2026-07` both present, `periods-before` from `2026-07` resolves to `2020-07` without the caller naming it — verified live, plus that the *closest* period wins when more than one exists before it | Shipped, commit `eb2b37a` |

## Parking Lot

- **Resolved 16 August 2026** (asked directly, `grill-me` itself being
  user-invocable only): predicate name `belongsToPeriod` (uncontroversial,
  directly precedented by `governedBy`/`onInvoice`'s camelCase convention).
  `gst:period`'s existing literal stays on facts unchanged, alongside the
  new edge — additive only, no rule rewrites. Chosen over superseding it
  because Slice 1 is the walking skeleton that can stop here if the entity
  doesn't prove its worth, and superseding is a bigger, riskier bet than
  this slice has earned; revisit once Slices 2/3 (cross-period comparison)
  show whether the literal and the edge ever need to be reconciled.
- **Whether the hospitality pack needs an equivalent** to keep proving
  domain neutrality, or whether "filing period" is legitimately GST-specific
  vocabulary that a differently-shaped temporal grouping (a booking season?
  a reporting quarter?) would represent in hospitality's own pack — worth a
  DN-3-style check before this is called "generic," not after.
- **Whether `FilingPeriod` should carry its own due-date/status fields**
  (overlapping with the already-shipped obligation calendar, P8/F4) or stay
  a pure grouping subject with due-date reasoning left entirely to the
  existing `Obligation` machinery — a real overlap risk between this plan
  and F4 that `planning` should resolve explicitly rather than let two
  features grow toward the same concept independently.

## Warnings

- Slices 2 and 3 both depend on Slice 1's edge shape existing and being
  named consistently — do not let Slice 2 invent its own traversal
  direction independently of Slice 1's.
- The `00c-domain-model.md` entity envelope (FQN construction, versioning,
  soft delete) **does not apply** to `FilingPeriod` — it is a pack subject,
  not a catalog asset, the same distinction Plan 106 Slice 3a's `Sid::
  is_runtime_pack_namespace` work drew explicitly. Do not accidentally route
  this through `Catalog`'s asset machinery; it belongs entirely inside the
  pack (ontology + loader + registered queries), matching how
  `gst:PotentialMismatch` findings already work.
- Every magic value here (which literal format a period uses, e.g.
  `"2020-07"` vs. an ISO interval) must trace to what the fixtures already
  contain, not be invented fresh — `licensing rule 4` (`00i`) applies to
  pack content the same as engine code.

## Next Step

Load `grill-me` on the parking-lot predicate/supersession question before
writing Slice 1's RED test, then load `planning` to turn Slice 1 into a
PR-sized implementation plan with TDD execution steps. Not started without
that confirmation — this document is the split, not the go-ahead to code.
