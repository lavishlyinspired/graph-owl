# Plan 107 — filing period as a first-class graph entity

**Status**: story-split, not yet planned or built. **Branch**: main.
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
| **1. FilingPeriod exists, one query traverses to it** | Proves the entity is worth having; unblocks everything else | `gst:FilingPeriod` class + a `belongsToPeriod`-shaped predicate in `packs/gst/ontology.ttl`; pack loader (or a fixture-load step) creates one `FilingPeriod` subject per distinct `gst:period` value already in the fixtures and links each fact's subject to it; one registered query (`period-summary.sparql` or similar, via the Slice-4a/4b mechanism) answers "everything belonging to period X" via the edge | Cross-period comparison; console UI; any new source data beyond what fixtures already carry | `run_pack_query(gst, period-summary, {period: "2020-07"})` returns every invoice/fact for July 2020, traversing the edge; a period with zero facts returns an empty answer, not `NotFound` (mirrors `run_pack_query`'s own established absent-vs-empty convention) | Ships behind nothing — additive pack content, same posture as Slice 4a |
| **2. Cross-period diff, one direction (invoice status across two named periods)** | Answers the reviewer's actual example ("what changed between April and May") for the narrowest real case | A second registered query taking two period bindings, returning invoices present/absent/changed between them, scoped to *status* only (not amount, not every field) | Full field-level diff; more than two periods at once; any "since last N periods" rolling window | Given INV-2001 absent from April's period-linked facts and present in May's, the query reports it as newly appearing; given no change, the query reports nothing for that invoice (silence-is-the-signal, matching this project's own `qlRewrite` convention) | Same as Slice 1 |
| **3. A subject's full period history** | Answers "show this invoice across every period it's appeared in" — the third example from the trigger | A query parameterized by subject instead of period, traversing every `FilingPeriod` a given fact belongs to, ordered | Aggregation/summarization of the history (that's a console concern, not a query concern) | Given an invoice linked to three periods, the query returns all three in period order | Same as Slice 1 |
| **4. Console surface** | Makes 1–3 usable by a human, not only the agent | A new admin/obligation-calendar-shaped read-only view: pick a period (or two), see the summary/diff | Editing anything — this is read-only, matching `run_pack_query`'s own non-admin-gated posture | Verified live against a running demo (`agent-browser`), matching every other console slice in this project's own convention | Ships behind nothing; it is additive UI |
| **5. Rolling/relative period queries** ("this month vs. last") | Removes the need to know exact period literals to ask a comparison question | A query taking a relative offset instead of two named periods | Anything beyond "N periods back" — no calendar-locale reasoning, no fiscal-year handling beyond what GST already assumes | Given periods 2020-06 and 2020-07 both present, "compare to previous period" from 2020-07 resolves to 2020-06 without the caller naming it | Same as Slice 1 |

## Parking Lot

- **Which predicate name** (`belongsToPeriod` vs. something else) and
  **whether `gst:period`'s existing literal stays on facts alongside the new
  edge, or is superseded by it** — a real design question for `planning`,
  not resolved here. Superseding it outright would touch every existing
  finding rule that currently binds `?period` as a literal (`pack.toml`'s
  `GstinTransposition` rule at minimum); keeping both risks the two
  drifting apart. Load `grill-me` on this specific question before slice 1's
  RED test, since it is exactly the kind of fuzzy design call `grill-me`
  exists for, not a call to make silently while writing the ontology file.
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
