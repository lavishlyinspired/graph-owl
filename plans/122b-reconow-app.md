# Plan 122b — Reco Now rebuild (`ext-apps/RecoNow/`)

**Parent**: `plans/122-frontend-rebuild.md` — read it first. Decisions D1–D4,
the nine standing constraints and the archival rules live there.

**Status**: planned, 17 August 2026. Not started.
**Path**: `ext-apps/RecoNow/` — `frontend/` (new) + `backend/` (moved from
`ext-apps/Reco/backend` and extended).

**Starts after** `122a` A0–A3 land, so the shared primitives are stable.
**Except B0**, which has no frontend dependency and may start immediately.

---

## Goal

Implement the 28-destination GST reconciliation workbench described by
`samples/GraphOWL and Reco Now UI Mockups3/Reco Now.dc.html`, on a
persistent multi-client backend, consuming GraphOWL through neutral graph
primitives only.

## Why here

Plan 120 reframed reco-now from "reconciliation results viewer" to "domain
investigation workspace powered by GraphOWL" and moved GST specifics out of
the console. The frontend never caught up: it is still the linear
`Upload → Map → Reconcile → Intelligence → Act` sample — 5 pages of
untyped, untested JavaScript — while the mockup describes a case-centred
workbench with durable workflow state.

The concept documents state the target directly:

> Reco Now should not be a generic "GSTR-2B matching screen". It should be
> a GST reconciliation workbench whose job is to turn reconciliation
> discrepancies into **actionable cases**.

> A CA/accounts user does not primarily want to see a graph. They want to
> answer: **what can I safely claim, what is wrong, why is it wrong, what
> do I need to do, and who do I need to chase?**

---

## 1. The constraint that shapes every slice

**No GST noun may enter the Rust API.** There is not one today among 157
paths — no `/periods`, `/itc`, `/gstin`, `/invoice` — and that is the
domain-neutrality rule holding, not an omission.

So every Reco screen is built from exactly two sources:

| Source | For |
|---|---|
| **GraphOWL, via neutral primitives** — `/sparql`, `/cypher`, `/findings`, `/findings/{id}/evidence-graph`, `/graph/paths`, `/graph/context`, `/packs/gst/reconcile`, `/packs/gst/finding-rules`, `/graph/import/rdf`, `asof` | Facts, evidence, provenance, reasoning, entity resolution, cross-period relationships, the GST pack's rules and law |
| **Reco's own FastAPI backend** | GST workflow state: clients, periods, cases, IMS decisions, follow-ups, approvals, notes, deliverables, mapping templates, users |

If a screen appears to need a GST endpoint in Rust, the answer is a pack
query or a Reco-side endpoint. **Route any proposal to add one back to this
paragraph.**

---

## 2. What exists

### 2.1 Backend — real logic, unusable persistence

`ext-apps/Reco/backend`, FastAPI, 18 routes, ~85KB Python.

| Keep | Why |
|---|---|
| `reconciliation.py` | Matching logic, tolerance, credit notes, RCM. Real. |
| `graphowl_client.py` (19KB) | Rows → Turtle projection, canonical IRIs (`_supplier_iri`, `_filing_iri`, `_canonical_iri`), import/delete, findings. The GraphOWL integration. |
| `native_findings.py` | Rule-driven findings. |
| `exporters.py` | CSV / XLSX working paper / ITC register / report. Deliverables depend on it. |
| `ai.py` | Assistant scaffolding. |

**Replace** — `SESSION: dict = {}` and `AI_JOBS: dict = {}`. Module-level
in-memory state: one session, one client, one period, lost on restart.

Current GraphOWL integration is three endpoints (`/graph/import/rdf`,
`/findings`, `/packs/{pack}/reconcile`). The mockup's case detail, evidence
chain and cross-period screens need `/graph/paths`,
`/findings/{id}/evidence-graph`, `/graph/context`, `/sparql` and `asof`
as well.

### 2.2 Frontend — effectively greenfield

`matcha-frontend`: React 18, plain JS, 5 pages, 3 components, 3
dependencies, no TypeScript, no tests. Harvest `format.js` (currency and
GSTIN formatting) and the reconciliation table's column shape. Nothing else.

### 2.3 The GST pack — already rich

`packs/gst/`: `ontology.ttl`, `law/rule-36-4.ttl` + `sections.ttl`, ten
SPARQL queries (`amount-mismatch`, `gstr1-not-in-2b`, `missing-in-books`,
`itc-not-available`, `goods-receipt-timing`, `gstin-transposition`,
`payment-overdue`, …), and fixtures for books / GSTR-1 / GSTR-2B / IMS /
filing periods / goods receipt.

**The ten queries map almost one-to-one onto the mockup's reason codes.**
Start from them; do not write new matching logic that duplicates a query
that already exists.

`gst:FilingPeriod` shipped as a first-class graph entity (Plan 107, all 5
sub-slices), with `period-diff`, `period-history` and `periods-before`.
**The Periods and Cross-period screens are largely a UI over shipped
capability** — check before building.

---

## 3. Acceptance criteria — epic level

1. All 28 destinations reachable, rendering real data, with loading /
   empty / error states.
2. Client and period isolation is **tested, not asserted**: cases, ITC,
   follow-ups and approvals never cross a client or a period boundary.
3. State survives a backend restart.
4. Every generated sentence on a case cites GraphOWL fact ids; an uncited
   sentence never reaches a case.
5. No assistant action leaves Reco Now or changes a number without passing
   the approval queue.
6. No GST noun added to the Rust API.
7. `ext-apps/Reco/frontend` archived; backend moved, not archived.

---

## 4. Slices

Each is one commit. Value · Path · AC · RED.

---

### B0 · Persistence, clients, periods `.api`

**Value** — nothing above the dashboard can be built honestly until
workflow state is durable and scoped. This is the plan's real first slice.

**Path** — `ext-apps/RecoNow/backend/` (moved from `ext-apps/Reco/backend`).
Postgres + migrations. Models: `client` (name, GSTIN, state), `period`,
`case`, `ims_decision`, `follow_up`, `approval`, `note`, `deliverable`,
`mapping_template`, `user`. Every workflow row carries `client_id` and,
where meaningful, `period_id`.

Replace `SESSION` and `AI_JOBS` with repository access. Keep
`reconciliation.py`, `graphowl_client.py`, `native_findings.py`,
`exporters.py` behaviourally unchanged — **write characterisation tests
first** (`characterisation-tests` skill) so the persistence change is
provably behaviour-preserving.

**AC** — two clients, three periods, no cross-contamination. State survives
restart. Existing reconciliation output byte-identical to pre-change on the
sample fixtures. Migrations run forward and roll back.

**RED** — a follow-up created for client A is **not** visible to client B
(the negative assertion the mockup's own copy promises: *"Cases, ITC and
follow-ups never cross clients"*). A case in period 2026-08 is not returned
for 2026-07. Restart, re-read, same state. *Mutators*: a repository query
missing its `client_id` predicate still returns plausible rows for a
single-client fixture — **the isolation tests must use two clients**, or
the mutation survives.

---

### B1 · Shell, client switcher, period picker, inbox, Ask

**Value** — the chrome present on all 28 screens.

**Path** — `ext-apps/RecoNow/frontend/` (Vite, React 19, TS strict,
Tailwind 4, Radix). Reco's own token set: warm paper, indigo accent, Public
Sans, light only (§1.2 of the parent).

**AC** — client switcher (GSTIN + state, isolation copy), period picker,
ITC-at-risk chip, inbox with review/approve/reject, Ask panel that returns
cited answers or *"not enough evidence"*, ⌘K. Router with route-level lazy.
axe clean.

**RED** — Ask returns "not enough evidence" when it cannot ground, rather
than an uncited sentence — assert the refusal path explicitly. Switching
client clears the case list rather than showing stale rows. *Mutators*: the
grounding check inverted; the switcher updating the label but not the query.

---

### B2 · Dashboard

**Value** — the close-readiness view: what a CA opens on a Monday.

**AC** — three ITC cards (reconciled/eligible, needs review, at risk),
close-readiness briefing with citation count and "How this was written",
"What needs a decision" sorted by ITC exposure, "What the graph engine did
for this period" (the five engine steps, from real run data), period state
with open exceptions and exposure, match-rate trend, assistants summary.

**RED** — the briefing's citation count equals the facts actually cited
(assert the set). Every card total reconciles to the register's filtered
sum — assert equality, because two independently computed totals that
disagree is the defect a dashboard ships with.

---

### B3 · Upload & map

**Value** — three files in, one reconciliation out, mapping saved as a
template so next month starts mapped.

**AC** — five-step progress, per-file tabs, sample row, raw toggle, mapping
table with confidence, unmapped-column retention, per-file confirm, one
unconfirmed file blocks reconciliation, template reused next period.

**RED** — an unmapped column is still retrievable after import. One
unconfirmed file blocks; all confirmed unblocks (both directions). The
saved template applies to a second period's identically shaped file.
*Mutators*: the block condition inverted; the template matching on the
wrong key.

---

### B4 · Register · Exceptions · Case detail

**Value** — the core loop. Case detail is the screen the product is judged
on.

**AC** — Register: bucket chips, search, filters, sort by ITC at risk,
bulk selection, exposure total for the current filter, export. Exceptions:
grouped by reason code, mapping to `packs/gst/queries/`. Case detail:
books/GSTR-1/GSTR-2B field comparison, "Why this case exists" with rule id
+ confidence + cited fact count, evidence chain over named graphs,
recommended action (drafted, not sent), IMS decision (accept / reject /
pending) with its recompute consequence stated, supplier pattern,
prev/next within the group, "Open investigation in GraphOWL" deep link.

**RED** — the exposure total equals the sum of the filtered rows, not of
all rows (the classic filter bug, and a negative test). IMS accept records
the supplier's value and is durable. "Open in GraphOWL" resolves to a real
subject. An amount-mismatch case cites the same facts
`packs/gst/queries/amount-mismatch.sparql` returns. *Mutators*: the
comparison columns transposed (books vs 2B) — assert per-column, since a
swap still renders three plausible numbers.

---

### B5 · ITC — position · at risk · eligibility

**AC** — ITC position for the period, at-risk list by supplier with
exposure and filing lateness, eligibility with reason codes. Reconciles to
the dashboard cards.

**RED** — ITC totals equal the dashboard's to the rupee. Rule 36(4)
treatment traces to `packs/gst/law/rule-36-4.ttl`, not to a constant in
Reco. *Mutators*: an eligibility boundary off by one — assert a case
exactly on the threshold.

---

### B6 · Compliance and Suppliers

**AC** — Authority (sections and rules from `packs/gst/law/`), Obligations
(due dates, status), Suppliers list, Supplier risk (filing behaviour,
exposure), Follow-ups (drafted, approved, sent, with history).

**RED** — a follow-up cannot reach "sent" without an approval row
(the product invariant, tested as a refusal). Supplier risk derives from
filing facts, not from a stored score that can drift.

---

### B7 · Operate — Review queue · IMS · Approvals

**Value** — where the "nothing sends itself" promise is either true or a
label.

**AC** — review queue with per-case decisions, IMS batch with accept /
reject / pending and the recompute consequence, approvals with what will
happen on approve. Bulk actions preview their effect first. Nothing reaches
the portal until 3B is filed — and the UI says so because it is enforced.

**RED** — an assistant-drafted action cannot execute without an approval
row; assert the refusal. A bulk approve affects the selected set only.
*Mutators*: the approval guard removed; bulk applied to the unfiltered set.

---

### B8 · Assistants

**AC** — usage stats, the five-stage pipeline (first three GraphOWL agents,
last two Reco's), assistant table with trigger/runs/accepted, latest output
with citations and destination, "Where AI appears in Reco Now" map, drafts
awaiting approval, token split and cost per resolved case, "What assistants
may not do", grounding percentage.

**Depends on** `122a` A8's agent-run persistence for the GraphOWL-side
stages. Reco's own two stages use Reco's backend. **If A8's prerequisite
plan has not landed, B8 ships the Reco stages only and states the gap** —
it does not fabricate the GraphOWL numbers.

**RED** — an uncited sentence never reaches a case (assert at write). The
grounding percentage is computed from real citation data.

---

### B9 · Deliver — Deliverables · Analytics

**AC** — Deliverables from `exporters.py` (working paper, ITC register,
client report), each recording what period and client it was generated
for. Analytics: exceptions by reason × period heatmap (cells click through
to cases), "What changed and why" with cited fact count, supplier portfolio
quadrant, cost-to-resolve trend, assistant contribution.

**RED** — a heatmap cell opens exactly the cases that produced it. A
generated deliverable's totals match the register at generation time.

---

### B10 · Data, Settings, cutover, archive

**AC** — Imports, Sources, Mappings, Rules, GSTINs, Users, New session.
`ext-apps/Reco/frontend` → `_archived/reco-frontend/` with a line in
`_archived/README.md`. `launch.sh` points at `ext-apps/RecoNow`. Full gate
green. `DEMOS.md` updated, `EPIC-STATUS.md` regenerated.

**RED** — "New session" clears the working session **without** deleting
persisted client data — the destructive-by-accident case, and the reason
this is a test rather than a button.

---

## 5. Explicitly deferred

| Deferred | Destination |
|---|---|
| GSTN portal filing (anything that actually submits) | Not planned. The mockup is explicit: *"nothing reaches the portal until you file 3B"*. |
| Live GSTN / GSP API integration | Its own plan. `plans/105a-gstr2b-provider-behaviour.md` is the prior art. |
| GraphOWL-side assistant stages, if A8's prerequisite has not landed | B8 ships partial and states the gap. |
| Responsive / mobile | Post-cutover. Mockup is 1440px only. |
| Multi-user concurrent editing of the same case | Post-cutover; needs a locking decision. |

---

## 6. Pre-PR quality gate

Per slice: backend tests green (pytest, including the two-client isolation
tests); characterisation tests green for anything moved in B0; frontend
suite; axe clean on touched routes; no GST noun added to the Rust API
(grep-asserted); `plans/00l-build-vs-adopt.md` row if a dependency was
considered.

Frontend mutation runs are batched with the Rust ones per `CLAUDE.md` —
`stryker` spawns 17 runners and must not interleave with a workspace suite.
