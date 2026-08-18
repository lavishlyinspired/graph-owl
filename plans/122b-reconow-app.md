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

**Shipped** — `ext-apps/RecoNow/backend/` (moved via `git mv`, `.venv`
recreated at the new path since a venv's shebang bakes in an absolute
path). All 10 models landed in `migrations/0001_initial.{up,down}.sql`, a
hand-rolled migration runner (`app/db.py` — build-vs-adopt entry in `00l`:
`psycopg` rejected on licence (LGPL-3.0), `yoyo-migrations` blocked on
auditability, a full ORM judged unnecessary for 10 small tables), and a
repository module (`app/repo.py`) covering all 10 tables with `client_id`
(and `period_id`, where the table has one) as a required predicate on every
scoped read — never an optional filter. `app_user`, not `user`: reserved
word in every SQL dialect.

Characterisation tests (`tests/test_characterisation.py`) pin the current
SESSION-based `/api/sample → /api/reconcile → /api/overview` flow's exact
stats and the working-paper CSV's shape — written and passing against the
*pre-change* code first, as the plan's own RED instructs.

**Verified, not merely asserted**: the isolation RED tests
(`tests/test_repo_isolation.py`) run against a real Postgres database (a
fresh one per test, `CREATE DATABASE` on the same shared, reusable
`graph-owl-tests` container the Rust suite already uses — no second
container). Two clients × three periods, exactly as the AC states, plus the
plan's own named mutant: manually dropping `list_follow_ups`'s `client_id`
predicate was confirmed to fail the two-client test (client B saw client
A's follow-up) before being reverted — the test is load-bearing, not
merely passing by construction. Restart was verified two ways: a second
pool connection within one test, and — because that alone doesn't rule out
pool-level caching — two genuinely separate Python processes against a
dedicated database, the second reading back exactly what the first wrote
after fully closing its pool. Rollback verified round-trip: migrate → roll
back → tables gone → re-migrate → tables back.

**Scope boundary, stated rather than silently left**: `main.py` itself
still reads and writes `SESSION`/`AI_JOBS` — this slice proves the
persistence and isolation layer is correct in isolation (the AC's own
bar), not that every HTTP route has been rewired onto it. Wiring
`client_id`/`period_id` into the request path is deferred to B1, which
already owns introducing those as real, user-facing concepts (the client
switcher and period picker) — rewiring the API to require them ahead of
having a UI to supply them would be scoping the HTTP contract before the
concept it belongs to exists. `AI_JOBS` specifically is left as an
in-memory dict rather than made durable: it tracks in-flight async job
*progress* (an AI draft being generated), which a restart legitimately
discards and the client re-triggers — persisting it would not buy the
resumability its own ephemeral nature can't support, unlike the 10 models
above, which are workflow *decisions* that must survive.

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

**Shipped** — `ext-apps/RecoNow/frontend/` (Vite, React 19, TS strict,
Tailwind 4), same proven tooling `graphowl-app` already validated, Reco's
own token set (`theme.css`, warm paper `#f4f2ee`, indigo `#5b6bb5`, Public
Sans + IBM Plex Mono) read directly off the mockup's own hex values, not
approximated. `lib/routes.ts`/`lib/nav.ts` mirror the mockup's real `nav`
array verbatim — 28 destinations, 9 groups, checked by
`nav.test.ts`/`router.structural.test.ts` (`ROUTES.length === 28`,
asserted, not just believed). 6 bespoke screens
(home/pipeline/register/case/agents/analytics) vs. 22 sharing the mockup's
own "generic" config-driven template — noted here for B2–B10: build the one
shared component once, not 22 bespoke pages.

`app/main.py` gained its first routes built directly on B0's `repo.py`:
client/period CRUD, case create/list, and `POST .../ask` — a deterministic
keyword match over the current client+period's own `case_record` rows, not
an LLM call, so "grounded" means exactly what it says: every citation
traces to a row the caller can also see. `app.state.db_pool`, connected at
startup from `DATABASE_URL`, best-effort the same way
`_install_graphowl_pack` already is — routes that need it return 503 rather
than the app failing to start on a laptop with no Postgres running.

**RED proven live, not only in the suite**: stood up a real backend against
a real Postgres database and the Vite dev server, then drove the actual
browser — created a real client and period through the UI, asked about a
real case (`INV-1025`) and got back a grounded answer citing it, then asked
about one that doesn't exist and got the refusal copy verbatim, never a
fabricated sentence. `POST .../cases` and `POST .../ask` both re-checked
for cross-client isolation through the HTTP layer (`test_ask.py`'s third
test), not just at the repo layer B0 already proved.

**Two real bugs found live, not by a test, and fixed same-session**: (1)
neither `ClientSwitcher` nor `PeriodPicker` closed on an outside click —
only their own trigger toggled them — so a stray click could land inside a
still-open dropdown instead of the element underneath; a click meant for
the Ask input once landed in a leftover "Month" field, typing the question
into the wrong place entirely. Fixed with a small `useClickOutside` hook,
wired to both. (2) Approving or rejecting an inbox item refreshed the
drawer's own list but not the TopBar's pending-count badge, which only
refetched on open/close — so the badge stayed stale immediately after a
real decision. Fixed by having `InboxDrawer` notify `AppShell` on decide,
added to the badge effect's own dependency list.

`workspace.ts`'s `selectClient` (client switch clears the selected period,
the actual mechanism behind "no stale rows" — every period-scoped fetch
depends on `periodId`) was manually mutated to drop the reset and confirmed
to fail its own test before being reverted, matching B0's own mutation
discipline.

**Scope note**: the inbox is wired to real `approval` rows (B0's own
table), not yet populated by anything — nothing in B1 generates an
approval. The queue itself, and the decide mechanism, are real and tested
live above; what creates approvals in the first place is B4's (cases) and
B7's (operate) job. axe was not run against this shell yet — deferred
alongside the rest of the automated a11y pass to whichever slice finishes
enough real screens to make a meaningful smoke suite, matching
`graphowl-app`'s own `first-run.spec.ts` precedent.

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

**Shipped**, reordered ahead of B2: B2's own AC needs real reconciliation
totals ("every card total reconciles to the register's filtered sum",
"what the graph engine did ... from real run data"), which only exist once
a reconcile has actually run — building the dashboard first would mean
building it against numbers that cannot exist yet. `app/main.py` gained
`.../datasets/{kind}/upload`, `.../datasets/{kind}/mapping`,
`.../datasets`, and `.../reconcile`, all reusing the pre-existing parsing
and auto-map helpers (`_parse_upload`, `_build_dataset`, `_auto_map`,
`_normalize`) unchanged, per this plan's own D3/B0 precedent of not
rewriting what already works. `WORKSPACES`, a new in-memory dict keyed by
`f"{client_id}:{period_id}"`, is the legitimate replacement for
`SESSION["datasets"]`/`SESSION["mapping"]` — in-progress mapping state, not
a workflow decision B0's durability guarantee is about. The mapping
*template* itself, once confirmed, persists through B0's own
`mapping_template` table (`repo.upsert_mapping_template` /
`get_mapping_template`), which is what makes template reuse across periods
real rather than re-derived from scratch.

**A real isolation gap found while wiring this, not by a test**:
`_ingest_to_graphowl`'s old `source = f"reco-{kind}"` was a single global
name — harmless for the pre-B0 single-session app, silently unsafe now
that two clients' uploads can be in flight against the same graph-owl
store at once, since a re-upload deletes-then-replaces its source and a
shared name means client B's upload would delete and replace client A's
own books. Fixed in `_ingest_scoped_to_graphowl`: the source name now
carries `client_id` and `period_id`. **Documented, not fixed, because it
cannot be from this side**: the native reconcile engine itself
(`run_findings`) still runs unscoped over the *whole* graph-owl store —
true before this slice (`_install_graphowl_pack`'s own comment already
says so) and still true after it. Scoped ingestion stops one client's
upload from overwriting another's; it does not give two clients a safely
concurrent *reconcile* — that needs a graph-owl-side scoping mechanism,
out of reach for a Python backend to add unilaterally. Recorded here so it
is a known, named gap rather than something a future session has to
rediscover.

`reconcile` bridges GraphOWL's own findings into B0's `case_record` table
— the first place native-engine output becomes a durable workflow row —
de-duplicating by `invoice_no` within the client+period so a re-run (a
corrected mapping, a re-upload) does not double a case that already exists.

**Verified live**, backend and browser both: uploaded a real CSV
(`books_test.csv`, 2 real invoices) through the actual browser file input
against a real Postgres — the mapping table rendered the true auto-detected
columns and sample values (`Invoice Date → 15-12-2025`, `Taxable Amount →
500000`, ...), confirmed the mapping, and the sidebar's checkmark and the
now-enabled "Reconcile" button both updated correctly. Clicking Reconcile
against no running graph-owl-server produced the exact expected degraded
message ("... was unreachable: [Errno 61] Connection refused") rather than
a crash or a silent no-op — the same best-effort contract
`_install_graphowl_pack` already established, now proven live for this
path too. **Scope note, stated rather than hidden**: this session did not
stand up a real graph-owl-server with the GST pack loaded, so the
reconcile *success* path (a real `run_findings` call actually finding
something, and the finding → `case_record` bridge actually firing) is
covered by the pieces it composes — `graphowl_client.py`'s own existing
645-line test suite, and this slice's isolation/dedup logic tested
directly — but not by one live end-to-end run all the way through a real
finding. Worth a live pass in a future slice once `scripts/demo.sh`-style
seeding is wired to the client/period model.

**A second real bug found live**: a client/period id persisted in
`localStorage` from an earlier session survived a switch to a *different*
Postgres database — the ids were non-null, so every `!clientId` guard
downstream passed, while the TopBar (which fetches its own list fresh)
correctly showed "Select a client". A stale-but-truthy id would have let
`pipeline.tsx` accept uploads and mapping confirmations against a client
that does not exist in the current database — the confirm step would only
fail later, at the Postgres foreign-key constraint, with no clear error
shown. Fixed in `AppShell`: a validation effect fetches the real
clients/periods lists whenever the workspace ids change and clears the
workspace the moment either no longer resolves, rather than trusting a
persisted id indefinitely.

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
