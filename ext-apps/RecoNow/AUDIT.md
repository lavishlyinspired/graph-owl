# RecoNow — Functional Audit Report

**Date:** 19 August 2026
**Scope:** Full functional audit of RecoNow (`ext-apps/RecoNow`) from a chartered-accountant
perspective — reconciliation accuracy, agent behaviour, and the graph-owl engine underneath it.
**Method:** Static review of all 35 backend modules (~9,200 LOC), the `packs/gst` SPARQL rule set
(29 queries) and pack manifest, the Python pack loader/reconcile client, the graph-owl server-side
reconcile endpoint, persistence layer and migrations, and both test suites (executed, not just read).

---

## 1. Verdict

**Substantially sound, with a small number of real defects — none of them in the money maths.**

The core reconciliation computations are unusually careful and, within the data the product ingests,
**accurate from a CA's point of view**. The legal model (s.16(2)(aa) invoice matching, Rule 36(4)
tolerance read from law data, Rule 37 180-day reversal, s.16(4) time bar, GSTR-9 Table 8 against
GSTR-2A) is correctly understood and correctly implemented, with several subtleties right that most
commercial tools get wrong. The three agents that exist work well and are grounded by a mechanical
numeric-citation check that is the strongest safety property in the product.

The material risks are elsewhere: a **legacy fallback engine that disagrees with the primary one**,
a **dashboard label that fabricates "Chronic Non-Filer" judgements**, **7 of 10 subscribed agents
that do not exist**, and **no authentication** on an API that holds multiple clients' tax data.

| Area | Verdict |
|---|---|
| Reconciliation accuracy (books ↔ 2B) | ✅ Correct within declared tolerance model |
| ITC position (5-class) | ✅ Correct; the agreed-portion `min()` treatment is right |
| 2B ↔ 3B comparison (Table 4) | ✅ Correct — compares 4A, not 4C, as it should |
| GSTR-9 Table 8 | ✅ Correct approach; uncomputable rows honestly reported |
| Working paper | ✅ Correct chain; unquantified deductions never silently zeroed |
| Agents (implemented) | ✅ Good — deterministic decisions, grounded prose |
| Agents (fleet as advertised) | ⚠️ 7 of 10 subscribed agents have no implementation |
| graph-owl as engine | ✅ Genuinely load-bearing and well integrated (new path) |
| Legacy SESSION path | ⚠️ Unscoped whole-store reconcile, divergent tolerance, single-user |
| Data layer / multi-client isolation | ✅ Tested isolation — but no auth at the HTTP boundary |
| Test posture | ✅ 561 backend + 84 frontend tests green; environment-dependent DB fixture |

---

## 2. CA-perspective review of the accounting logic

### 2.1 Correct, and worth saying is correct

- **s.16(2)(aa) matching model.** Matching is keyed on `(supplier GSTIN, normalised invoice number)`
  — the two identifiers the statute keys on — with accent-stripping and non-alphanumeric
  normalisation. A books row with no 2B counterpart is "pending" (deferred, not lost), which is the
  right characterisation: it misstates the position to call unfiled supplier invoices "lost credit".
- **Rule 36(4) tolerance is read from law data in the graph, not hardcoded**
  (`amount-mismatch.sparql`). The provision in force is selected by invoice date
  (`effectiveFrom` latest-not-after), so a 2020 invoice gets the 10% cap and a 2026 invoice gets the
  nil cap. A separate ₹1 de-minimis floor is applied *beside* the statutory cap with the correct
  stated reason: GSTR-3B is filed in whole rupees, so a sub-rupee difference cannot change a claim.
- **2B ↔ 3B comparison targets Table 4A, not 4C** (`itc_3b.py`). This is right: 4A is what 2B
  auto-populates; comparing 4C would report every deliberate s.17(5) reversal as an under-claim.
  The direction is named in words (`excess`/`unclaimed`) rather than collapsed into a signed figure —
  the two directions have opposite remedies (s.73/74 demand + s.50 interest vs. claimable within
  s.16(4)) and the code says exactly this.
- **Rule 37 closes the loop.** `payment-overdue.sparql` treats an *absent* payment as a finding
  (correct — never-paid is the worst case), the 180-day threshold is applied with real calendar
  arithmetic in the rule's span band, and `rule_37_reversal_check` then checks whether Table 4B(2)
  of the filed 3B actually carried the reversal. The shortfall is floored at zero because 4B(2)
  also carries s.16(2)(b)/(c) reversals — a larger 4B(2) is ordinary, not negative exposure. Correct.
- **s.16(4) time bar is not a hardcoded date** (`itc-time-bar-approaching.sparql`). The deadline is
  read from the graph as a `gst:claimDeadline` per filing period, because it moves earlier the
  moment GSTR-9 is filed. A hardcoded 30-November rule would report a window open when it has
  closed — the dangerous direction. This is the single most commonly bungled rule in GST tooling,
  and it is right here.
- **ITC position in five classes** (`reconcile_result.py`): confirmed / pending / blocked /
  under_review / unclaimed. For a disputed invoice, only the *difference* is under review and the
  agreed remainder is confirmed — taken as `min(books, portal)`, never the higher side, so the
  position can never recommend an excess claim. This is exactly how a CA would work it by hand.
- **GSTR-9 Table 8** (`gstr9.py`): 8A is computed against **GSTR-2A, not 2B** — correct, because 2B
  freezes on the 14th and 8A is defined on the portal's full record. Rows the deployment cannot
  support (8C, 8G–8K — customs/import data) are reported *uncomputed, with the missing dataset
  named*, never as zero. A zero nobody derived reads as a filed position; this product refuses to
  emit one.
- **Credit/debit note netting (s.34)** is done by note *kind*, not by recorded sign — because real
  GST files write credit notes negative and some ERPs write the magnitude. Over-large notes and
  notes naming an absent invoice are surfaced for a human rather than silently absorbed or clamped.
- **Rate-line aggregation before comparison.** GSTR-2B is line-structured (one row per rate slab);
  lines are summed to invoice level before matching, so an invoice total is never compared against
  one of its own rate lines.
- **Money is `Decimal` end to end**; floats appear only at the JSON boundary. The working paper
  cannot disagree with itself by a rounding rupee.

### 2.2 Where a CA would push back

1. **Table 4A is modelled as one figure.** The current return splits 4A into sub-rows (imports,
   RCM inward supplies, ISD, all-other). RecoNow's 2B↔3B comparison treats `itc_4a` as a single
   total. For taxpayers with imports or RCM credits this will mis-attribute differences between
   2B-populated and manually-entered 4A components. Acceptable for a services-only SME; a
   limitation worth one sentence on the screen.
2. **No interest quantification.** A Rule 37 reversal that was *not* made is an exposure attracting
   s.50 interest from the date of availing; the product reports the shortfall but never the
   interest. For a notice-defence product this is the number the officer leads with.
3. **Rule 42/43 inputs are out of scope.** `gst:ProportionateReversal` is modelled in the working
   paper's deduction table, but no dataset supplies exempt/non-business turnover, so the rule is
   starved in practice (the codebase itself notes nine of thirteen rules were input-starved before
   the recent work). Honestly reported as not-evaluated — but a CA should know the proportionate
   reversal check is effectively dormant until turnover data is ingested.
4. **IMS lifecycle is advisory only.** `ims_actions` recommends accept/reject/investigate per
   invoice but no IMS status is tracked through time (pending → accepted → deemed), so a period
   close cannot prove what was actually actioned on the portal.

---

## 3. Defects found

### 3.1 High

**F1 — `supplier_health` fabricates a judgement.** [reconciliation.py](backend/app/reconciliation.py)
labels **every** supplier with any at-risk row `"Chronic Non-Filer"` and leaves `filing_6mo` blank.
One ₹500 rounding dispute makes a supplier a chronic non-filer on the dashboard and in exports.
That is an invented characterisation of a third party — the exact class of output the grounding
rule exists to prevent, emitted by deterministic code. *Fix: derive the label from distinct-period
recurrence (`capabilities.supplier_pattern` already implements this correctly) or drop the column.*

**F2 — The legacy SESSION path reconciles against the whole store.** `_run_graphowl_reconcile`
([main.py](backend/app/main.py)) calls `run_findings` **without** the `graphs` scope that
`reconcile_route` passes. On the legacy path, rules read every graph in the store — another
period's or client's data can satisfy or trigger a rule, which is the contamination the scoped
endpoint was built to close. The legacy path is also a single global `SESSION` dict: not
multi-user safe, and its flat ₹1 tolerance **disagrees** with the native engine's statutory cap
(the characterisation test documents a real ₹180/1.33% divergence it absorbed as "legitimate").
Two engines that can disagree on the same file is a liability however well-documented.
*Fix: retire the legacy path per plans/119 §5b, or scope it identically.*

### 3.2 Medium

**F3 — 7 of 10 subscribed agents do not exist.** `DEFAULT_SUBSCRIPTIONS` registers ingestion,
validation, triage, vendor, risk, explainer, eligibility, drift, close, pattern. Only **triage,
vendor and risk** have implementations; the rest record a "skipped — no implementation" run. The
subscription screen therefore advertises a fleet that is 70% aspirational. What exists is good;
what is displayed overstates it.

**F4 — Agent state is process-memory only.** `AGENT_RUNS`, grants and the registry live in module
globals, capped and dropped on restart. An audit trail of agent decisions that evaporates on
redeploy is not an audit trail. The code says so itself ("a durable record belongs in graph-owl
agent activity") — it remains true.

**F5 — SPARQL injection surface in the risk agent.** `SUPPLIER_HISTORY % gstin` interpolates a
GSTIN straight into a query string. GSTIN shape is only *warned* on at ingestion
(`data_quality.py` severity "warning", never blocking), so a malformed value containing a quote
reaches the query. Low exploitability (localhost tool, constrained input) but the wrong pattern.

**F6 — No authentication anywhere on the backend.** Every route trusts the `client_id` in the URL.
Row-level isolation is genuinely tested (`test_repo_isolation.py`), but isolation without identity
is a courtesy, not a control. Fine for a single-firm desktop deployment; not deployable multi-user.

### 3.3 Low

- **F7** — Dead code in the legacy engine: `itc = tax_book if match is not None else tax_book`
  (both branches identical) and a "fuzzy" match loop in `_find_portal_match` that re-tests the
  exact key it already failed.
- **F8** — Terminology: `match_stats`' `gross_itc` (matched + review + portal-only tax) is fed to
  the AI prompts and template summary as "**Net** ITC for GSTR-3B Table 4". It is neither net nor
  a 4C figure. The number is defensible; the label is not.
- **F9** — `classify_mismatches` reports `itc: 0.0` for the "Only in Portal" bucket while the
  row-level data carries the real tax — understates unclaimed credit in the classification summary.
- **F10** — The backend suite depends on a shared Docker Postgres at `localhost:55000`. A first
  full run produced **13 failures + 70 errors** (environment/permission), a second run was
  **561/561 green**. Not self-contained; per plans/119 §3.5, no Python tests run in CI at all, so
  this flake has no tripwire.

---

## 4. The agents, assessed on what they actually do

**Triage** — ranks cases by *recoverability, then amount*, from a declared urgency table with a
stated reason per rank, and explicitly **never asks the model to rank**. This is the correct
architecture: ranking is judgement with money attached and must be reproducible. Duplicate
SupplierNotFiled/PotentialMismatch double-drafting was found and fixed.

**Vendor** — drafts supplier chase emails. The model only ever rewrites a computed template;
`grounding.ground_draft` then verifies every number in the draft appears in the figures actually
supplied to the prompt, and a failing draft is replaced by the deterministic template. The
grounding rule is mechanical, refuses unknown citations, does not echo refused text, and logs
refusals. This is genuinely well done — the failure modes (a model quoting "Section 16(2)(aa)"
being refused; identifier digits being read as amounts) were found against a real model and fixed.

**Risk** — the one agent that genuinely needs the graph: cross-period recurrence via MCP
`query_graph`, prior judgements via `recall_memory`, and — critically — a supplier it could not
check is reported `checked: false` with the reason, never as clean. That is the honest direction.

**Runtime** — grants are re-checked *per write* (so revocation mid-run works), every step is a
typed span (tool input/output, decisions with reasons, model calls with grounding verdicts), and a
failing span is recorded and re-raised. Cost is `None` when unmeasured rather than a fabricated
zero. The observability model is better than most production agent systems.

## 5. graph-owl as the engine

The integration is real, not decorative, on the current path:

- Uploads land as RDF named graphs via `POST /graph/import/rdf`, **deleted-then-replaced under a
  stable per-(client, period, kind) source name** — the fix for totals accumulating across uploads.
- `POST /packs/{pack}/reconcile` runs graph-owl's native rule engine with a **graph scope** naming
  exactly this period's uploads plus the pack's law/ontology graphs (verified server-side in
  `crates/graph-owl-server/src/lib.rs` — the `graphs` parameter exists and is threaded into
  `Catalog::reconcile_pack`). Cross-period contamination is structurally closed on this path.
- Rule outcomes are the engine's own three-state record (passed / flagged / **not evaluated**),
  stored per period and surfaced — so a rule that could not run never reads as a rule that passed.
- Findings attach to invoices by `(gstin, invoice)` evidence bindings with an unambiguous-only
  invoice-number fallback, and per-period filtering uses the same identity the case carries.
- MCP is used where it is warranted (cross-period supplier history) and not where it would be
  ceremony.

The engine's own constraints are respected and worked around honestly — e.g. the SPARQL evaluator
has no date arithmetic, so `payment-overdue` returns anchors and the threshold is applied by the
rule's span band rather than pretending the query can subtract dates.

**Residual engine-side risks for RecoNow:** findings are stored pack-globally and scoped to a
period client-side by invoice identity (correct as implemented, but it leans on
`findings_for_period`'s fallback matching); and the legacy path (F2) bypasses the scoping entirely.

## 6. Data layer

- Migrations 0001–0007 are small, reversible (paired up/down), and match the repo code.
- Multi-client isolation is enforced by **required** `client_id` predicates (never optional
  filters) and covered by dedicated isolation tests, including cross-client invisibility and
  restart durability.
- Mapping templates record the source headers they were learned from and refuse to apply to a
  differently-shaped file — a wrong template fails safe into re-mapping instead of silently
  mapping invoice numbers onto GSTINs.
- `NaN` cells are normalised to `None` before JSON persistence (Postgres rejects bare `NaN`).
- Upload data-quality inspection **warns, never rejects**, and distinguishes "absent" from
  "unparseable" from "malformed GSTIN" with row counts and a 1-based example row. Silence about
  discarded rows — the failure this layer exists to prevent — is designed out.

## 7. Test & verification posture

| Suite | Result (this audit) | Notes |
|---|---|---|
| Backend pytest | **561 passed** (2nd run); 13 failed + 70 errors (1st run, env) | Needs Docker Postgres on :55000; not in CI |
| Frontend vitest | **84 passed, 11 files** | Pure-function libs well covered |
| Frontend Stryker | 68.65% on `src/lib/*` (self-reported, 19 Aug 2026) | Components deliberately unmutated; exclusions reasoned |
| Characterisation tests | Present and load-bearing | Pin the sample-flow numbers against hand-derived answers; already caught one real regression |

Test quality is high: negative assertions exist beside positive ones ("a supplier who filed nothing
is not reported as costing nothing"), screens are cross-checked against each other, and the pinned
numbers are hand-derived rather than copied from output.

## 8. Recommendations, in order

1. **Fix F1** — remove or derive the "Chronic Non-Filer" label. It is the only output in the
   product that states a judgement nobody computed.
2. **Retire or scope the legacy SESSION path** (F2). Two engines, two tolerance models, one screen
   of divergence waiting to happen.
3. **Either build or unlist the seven missing agents** (F3). A subscription that can never fire is
   an honest gap; the screen should show it as one.
4. **Add authentication before any non-localhost deployment** (F6); parameterise the risk agent's
   SPARQL (F5).
5. **Persist agent runs** (F4) — the trace model is the product's best feature; it deserves
   durability.
6. **Quantify s.50 interest on unreversed Rule 37 shortfalls** — the number a notice actually
   demands.
7. **Put the Python suites in CI** (F10) — the flake found in this audit currently has no tripwire.
8. **Split Table 4A sub-rows** when the first client with imports/RCM arrives (§2.2.1).

## 9. Summary

From a chartered accountant's chair: **the numbers RecoNow produces on the current (repo-backed,
graph-scoped) path can be relied on within the data you give it**, and — rarer — the product tells
you, on screen, which checks it could not run and which figures it could not derive. The legal
interpretations embedded in the rules (16(2)(aa), 36(4), 37, 16(4), Table 4 vs 2B, GSTR-9 Table 8)
are correct, including the subtle parts. The agents that exist are grounded, deterministic where
money is concerned, and honest about what they could not check.

The exposure is concentrated in the seams: a legacy engine still reachable, a dashboard label that
invents supplier characterisations, an advertised agent fleet larger than the real one, and an API
with no identity layer. All four are fixable without touching the core computation.
