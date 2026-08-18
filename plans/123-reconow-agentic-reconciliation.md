# Plan 123 — Reco Now as an agentic reconciliation workspace on graph-owl

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Not started. Plan only.
**Depends on**: Plan 122b (the real-data cutover — §7 of that file is the
starting state), `packs/gst` (13 registered finding rules, 20 SPARQL
queries), `plans/00l-build-vs-adopt.md`.
**Supersedes**: nothing. Extends 122b rather than replacing it.

> **Domain-neutrality is a hard constraint and this plan does not bend it.**
> Everything GST-shaped below lands in `packs/gst` or in `ext-apps/RecoNow`.
> No GST noun enters graph-owl's Rust crates;
> `scripts/check-namespace-neutrality.py` is in the gate and stays there.
> graph-owl gains *generic* capability (a rule that returns a projected
> value, an agent grant scope) and the GST meaning lives in the pack.

---

## 1. The finding that reframes everything

Reco Now does not under-use graph-owl at the margins. It uses **3 of its 159
endpoints** — `graph/import/rdf`, `findings`, `ontology-packs` — and
reimplements, less well, several things graph-owl already does properly.

And the reasoning engine is **starved, not weak**:

| | |
|---|---|
| Finding rules registered in `packs/gst` | **13** |
| Rules that fired on the real March 2026 data | **4** |
| Predicates the rules read | `PaymentEvent`, `GoodsReceipt`, `atTime`, `onInvoice`, `itcAvailable`, `period`, `issuedBy`, `taxAmount`, … |
| Predicates Reco Now's ingestion writes | `PurchaseInvoice`, `Gstr2bInvoice`, `Gstr1Invoice`, `recordedIn`, `reflectedIn`, `appearsIn` |

Reco Now ingests **invoice documents only**. It writes no payment events, no
goods-receipt events, no ITC-eligibility flag off the 2B, no period linkage.
So nine rules that are correctly written, correctly registered, and correctly
tied to a statutory provision **can never fire** — including the three that
carry the most money:

- `gst:PaymentOverdue` → **Rule 37**, ITC reversal when a supplier is unpaid
  180 days. Needs `gst:PaymentEvent`.
- `gst:ITCNotAvailable` → **Section 17(5)** blocked credits. Needs
  `gst:itcAvailable` off the 2B.
- `gst:GoodsReceiptTiming` → **Section 16(2)(b)**, no ITC before goods are
  received. Needs `gst:GoodsReceipt`.

**The cheapest large win in this plan is not new intelligence. It is feeding
the intelligence that already exists.**

---

## 2. What a Chartered Accountant actually does

Researched rather than assumed (sources in §11). Reco Now today addresses
roughly the first row and a half of this table.

| Activity | Statutory hook | Reco Now today |
|---|---|---|
| Match purchase register ↔ GSTR-2B | §16(2)(aa), Rule 36(4) | **Yes** — the one thing it does |
| Act on every IMS record before the 14th | IMS; deemed acceptance | Screen exists, no deadline logic |
| Classify ineligible ITC | **§17(5)** | Rule registered, never fires |
| Track unpaid suppliers past 180 days | **Rule 37** | Rule registered, never fires |
| Confirm goods actually received | **§16(2)(b)** | Rule registered, never fires |
| Watch the ITC expiry clock | **§16(4)** | Nothing |
| Match credit/debit notes and amendments | §34 | Nothing |
| Reconcile outward: books ↔ GSTR-1 ↔ GSTR-3B | §37, §39 | Rule exists, no data, no screen |
| Chase suppliers who have not filed | commercial | List only; no ledger of who was chased |
| Compute what goes in **GSTR-3B Table 4** | §39 | Nothing — *the actual deliverable* |
| Annual GSTR-9 / 9C reconciliation | §44 | Nothing |
| Defend a claim when a notice arrives | §73/74 | Evidence exists; no export |

Two things a CA said that the product currently gets wrong:

1. **"The real pain is the exceptions"** — invoices that *almost* match, and
   credit that should not be claimed at all. Reco Now ranks by exposure, which
   surfaces big numbers, not hard decisions.
2. **"Suppliers might update their filings at any time"** — a reconciliation
   is a photograph of a moving thing. Reco Now recomputes and overwrites; it
   cannot say *what changed since you last looked*.

### The §16(4) deadline is the argument for the graph, in one fact

Two sources in this research disagreed: one said the ITC deadline is
30 September, one 30 November. **30 November is right** (Finance Act 2022,
w.e.f. 1 Oct 2022) — *but only as a ceiling*. The real rule is:

> earlier of **30 November** following the financial year, **or the date the
> annual return is actually filed**.

So the deadline for an invoice depends on another entity's state, changes when
that entity changes, and had a different value before October 2022. That is a
dated, conditional, entity-dependent fact. **It must never be a constant in
Reco Now.** `packs/gst/law/` already models Rule 36(4)'s cap exactly this way
and `provision-in-force.sparql` already reads it. §16(4) gets the same
treatment.

---

## 3. What graph-owl already does that Reco Now reimplements or ignores

| graph-owl | Reco Now today | What the CA gets |
|---|---|---|
| `/reasoning/explain` — *why a fact holds, recursively, down to the assertions under it* | nothing | **A notice-defence trail.** The highest-value endpoint in the product. |
| `/reasoning/derived`, `/reasoning/runs` | nothing | What the law implies that nobody typed |
| `/resolution/queue`, `/packs/{p}/candidates` | nothing | One supplier under two GSTINs / trade names |
| `/validation/report`, `/validation/waivers` (**with expiry**) | nothing | An accepted exception that automatically comes back |
| `/proposals`, `/agents/{id}/grant`, `/agents/{id}/activity` (*including refused attempts*) | own `approval` table | **The agent-approval boundary, already built and audited** |
| `/drift`, `/drift/{id}/apply` | recompute + overwrite | *What changed since you last looked* |
| `/memories` + supersede/retract | nothing | "This vendor always files late" — correctable, never destroyed |
| `/lineage` | nothing | figure → case → fact → source row |
| `/threads/{id}/posts`, `/resolve` | nothing | Preparer ↔ reviewer on the case itself |
| `/graph/paths`, `/graph/context` | nothing | invoice → filing → period chains |
| `/sparql`, `/cypher` | SQL over `case_record` | Ad-hoc CA questions |
| `/policies/dry-run` | nothing | "What would widening tolerance do?" *before* doing it |
| `/packs/{p}/finding-rules` **POST** | nothing | **A new GST rule is data, not a release** |
| `/mcp` | nothing | The agent tool surface |
| `/webhooks/*` | nothing | GSP/GSTN push |

Two of these matter enough to restate:

- **Reco Now has its own `approval` table while graph-owl ships a proposal
  system that records refused attempts.** For an agentic product the audit
  trail of what an agent *tried* and was denied is worth more than the trail
  of what it did. Migrate; do not extend the local table.
- **Finding rules are POST-able.** A CA-authored rule ("flag any invoice from
  a supplier in this watchlist") becomes a row, not a deployment.

---

## 4. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Reco Now console (React)                                    │
│  workspace = client + period. Never crosses either.         │
└──────────────┬──────────────────────────────────────────────┘
               │ REST
┌──────────────▼──────────────────────────────────────────────┐
│ Reco Now backend (FastAPI)                                  │
│  • ingestion: files → events + documents (§5)               │
│  • agents: plan → ground → propose  (§6)                    │
│  • projection: findings → cases, exposure, GSTR-3B          │
│  • NO domain rules in code. Rules live in packs/gst.         │
└──────────────┬──────────────────────────────────────────────┘
               │ HTTP  (target: ~15 endpoints, from 3)
┌──────────────▼──────────────────────────────────────────────┐
│ graph-owl — the engine                                      │
│  facts · SPARQL rules · reasoning+explain · resolution ·     │
│  validation+waivers · drift · lineage · memories ·           │
│  proposals+grants · threads                                  │
└─────────────────────────────────────────────────────────────┘
                         ▲
                  packs/gst  (all GST meaning: ontology, law, rules)
```

**The division of responsibility, stated once:**

- **graph-owl** holds facts and derives consequences. Domain-neutral.
- **`packs/gst`** holds what GST *means* — ontology, statutory provisions with
  their dates, and the rules. Data, not code.
- **Reco Now** is workflow: get files in, get decisions out, keep client and
  period separate, and never state a number the graph cannot explain.
- **Agents** propose. Humans dispose. Nothing an agent produces reaches a
  return or leaves the building without a recorded human decision.

---

## 5. Slice A — feed the engine (highest value, lowest novelty)

**Nine registered rules cannot fire because the data they read is never
written.** Fix the ingestion, and three statutory checks light up with no new
reasoning at all.

**AC**

- Upload accepts five kinds, not three: `books`, `gstr2b`, `gstr1`,
  **`payments`** (supplier ledger: invoice, payment date, amount),
  **`grn`** (goods receipt: invoice, receipt date).
- `rows_to_turtle` emits `gst:PaymentEvent` and `gst:GoodsReceipt` with
  `gst:atTime` / `gst:onInvoice`, and carries `gst:itcAvailable` and
  `gst:taxAmount` off the 2B.
- Invoices link to their filing period (`gst:period`), so cross-period rules
  resolve.
- Every kind is optional. A missing file disables the rules that need it and
  **says which** — never silently produces "no findings".

**RED**

- Ingest a payment 200 days after invoice date → `gst:PaymentOverdue` fires,
  `governedBy` = `gst:Section16-2-d`.
- Ingest a 2B row with `itcAvailable = N` → `gst:ITCNotAvailable` fires under
  `gst:Section17-5`.
- Ingest a GRN dated after the period end → `gst:GoodsReceiptTiming` fires.
- Upload no `payments` file → the UI states that Rule 37 checking is off, and
  no `PaymentOverdue` finding is claimed either way.
- *Mutator*: pay on day 180 exactly. Assert the boundary — the rule must not
  fire on the day it is still allowed.

**Why first**: it is the only slice where the intelligence already exists and
only the plumbing is missing.

---

## 6. Slice B — the agent layer

Five agents. Each **plans → grounds → proposes**; none writes.

| Agent | Question it answers | Grounded in | Proposes |
|---|---|---|---|
| **Triage** | "Which 12 of 300 cases actually need me?" | findings + exposure + expiry clock | a ranked queue with a reason per case |
| **Explainer** | "Why does this case exist, in a sentence I can send a client?" | `/reasoning/explain` output | case narrative, every clause fact-linked |
| **Vendor** | "Who do I chase, for what, and what do I say?" | unfiled/mismatched cases + contact history | a draft email per supplier |
| **Eligibility** | "Is this credit actually claimable?" | §17(5), §16(2)(b), Rule 37, §16(4) | a classification + the provision |
| **Close** | "What blocks GSTR-3B, and what goes in Table 4?" | the whole period | a 3B working paper + a blocker list |

### The rule that makes this safe

**An agent may only state a number that appears in a fact it cites.**

Not a style guide — an enforced contract. Each agent returns
`{claims: [{text, fact_ids: [...]}], proposal}`. A claim carrying a figure with
no supporting `fact_id` is **rejected before it renders**, and the rejection is
recorded. This is the direct descendant of Plan 122b's finding that the console
shipped "₹8.2 L sits inside the s.16(4) window" with no query behind it. An LLM
will do that by default and confidently.

**AC**

- Agents run against a configured model; with none configured the screen says
  so (already true — 122b) and no agent output is fabricated.
- Every agent write goes through `/proposals`. Accept/reject is recorded with
  the human's identity.
- `/agents/{id}/grant` scopes what each may touch; `/agents/{id}/activity`
  shows attempts **including refused ones**.
- Token spend and latency are recorded per run and shown. If not measured, not
  shown.
- Model choice is configuration, not code. Fall back to no-agent mode cleanly.

**RED**

- An agent asked to summarise a case with no evidence returns "not enough
  evidence", not prose. *This is the mutation-critical test.*
- A fabricated figure injected into an agent claim is rejected by the citation
  check and logged.
- Accepting a proposal writes exactly one change, attributed to the human.
- Revoking a grant mid-run stops the write and records the refusal.
- *Mutator*: drop the `fact_ids` check → the fabricated-figure test must fail.

---

## 7. Slice C — the screens

28 routes, several of which are the same list under different headings, and
the ones a CA needs most are missing. **Consolidate to 16.**

**Merge**

| Now | Becomes |
|---|---|
| `datasources`, `imports`, `mappings`, `pipeline` | **Data** (one screen, tabs) |
| `suppliers`, `risk`, `atrisk` | **Suppliers** (one row per supplier, tabs) |
| `authority`, `obligations` | **By provision** (they group the same cases twice) |
| `register`, `exceptions`, `queue` | **Register** (saved views, not three routes) |
| `periods`, `gstins`, `users`, `reset` | **Settings** |

**Add — what a CA needs and cannot get today**

| Screen | Why |
|---|---|
| **GSTR-3B working paper** | The actual deliverable. Table 4 with every figure traced to cases. |
| **ITC expiry clock** | Invoices ranked by days to §16(4) cut-off, computed from the graph. |
| **Ineligible ITC (§17(5))** | Blocked credits, with the sub-clause. |
| **Payments & Rule 37** | Unpaid past 180 days, with reversal amount and re-availment note. |
| **Credit notes & amendments** | §34 matching; what moved since last reconciliation. |
| **Notice defence pack** | Pick a period/supplier/invoice → export every fact, rule and citation. |
| **What changed** | `/drift` — supplier amended a filing after you reconciled. |

**Every retained screen must answer**: *what decision does this let a CA make,
and what does it show that supports it?* A screen that answers neither is
deleted, not decorated.

**AC**

- No screen renders a figure it cannot trace to a case or a fact.
- Every case shows its provision, its facts, and a link into GraphOWL.
- `liveCols` everywhere (122b's guard stays green).
- Zero axe violations on touched routes.

---

## 8. Slice D — use the engine

| Build | On |
|---|---|
| "Why is this case here?" opens the derivation tree | `/reasoning/explain` |
| Supplier identity review queue | `/resolution/queue`, `/packs/gst/candidates` |
| Accepted exceptions **that expire** | `/validation/waivers` |
| "What changed since last reconciliation" | `/drift` |
| Client/supplier notes that survive periods | `/memories` (+ supersede, never delete) |
| Case discussion, preparer ↔ reviewer | `/threads` |
| Tolerance change preview | `/policies/dry-run` |
| CA-authored rules from the UI | `POST /packs/gst/finding-rules` |
| Ask → real query | `/sparql` (replacing the keyword match) |

**AC**: Reco Now's own `approval` table is retired in favour of `/proposals`;
exposure and grouping move from SQL to SPARQL where the graph is the better
source; `case_record` remains only as a projection cache with the graph
authoritative.

**RED**: deleting a case row and reprojecting reproduces it exactly. If it
cannot, the graph was not actually the source of truth.

---

## 9. Slice E — new rules in `packs/gst`

Pack data, not code. Each needs its provision modelled with dates.

| Rule | Provision | Note |
|---|---|---|
| `gst:ItcExpiryApproaching` | §16(4) | `min(30 Nov, annual-return filing date)` — **modelled, never a constant** |
| `gst:CreditNoteUnmatched` | §34 | |
| `gst:ImsDeemedAcceptanceRisk` | IMS | no action before the 14th |
| `gst:RcmSelfInvoiceMissing` | §9(3)/9(4) | `reverse-charge.sparql` exists; needs the liability side |
| `gst:Gstr1Vs3bMismatch` | §37/§39 | outward side |
| `gst:DuplicateItcClaim` | §16 | same invoice claimed twice |

**RED for every one**: a positive case **and** a negative — per `CLAUDE.md`,
every surviving mutant in this project so far has been a missing negative test.
For §16(4), assert an invoice one day inside the window does *not* fire.

---

## 10. Sequencing, and what could go wrong

**Order**: A (feed the engine) → C-merge (stop maintaining 28 screens) →
D (use the engine) → B (agents) → E (new rules) → C-add (new screens).

A first because it is pure win. B after D because agents grounded in
`/reasoning/explain` are worth far more than agents grounded in a case row.

| Risk | Handling |
|---|---|
| **LLM fabricates a tax figure** | The citation contract in §6, enforced and tested, plus a human gate on every write. The single biggest risk in the plan. |
| **Domain leak into graph-owl** | `check-namespace-neutrality.py` is in the gate. This plan adds no GST noun to Rust. |
| **Law changes** | Provisions are dated pack data; `provision-in-force.sparql` reads what applied on the invoice date. Nothing to redeploy. |
| **Agent cost** | Measured per run and shown; agents are on-demand, not on-ingest. |
| **Scope** | Slice A alone is a materially better product. Every slice ships standing alone. |
| **Sources conflict** (they did — §2) | Law lives in the pack with a citation, and every finding names its provision. A wrong provision is then visible and fixable as data. |

---

## 11. Sources

- ClearTax — [IMS under GST](https://cleartax.in/s/invoice-management-system-ims-under-gst) · [Section 16(4)](https://cleartax.in/s/section-16-4-of-cgst-act) · [Rule 37 — 180-day reversal](https://cleartax.in/s/rule-37-of-cgst-sgst-rules-itc-reversal-180-days)
- [SmartGST — IMS mandatory guide 2026](https://smartgst.in/blog/gst-invoice-management-system-ims-mandatory-guide-2026) *(states a 30 Sep §16(4) date; contradicted by the sources above and not relied on — see §2)*
- [Tax Garden — §16(4) 30 Nov deadline](https://taxgarden.in/blog/gst-section-16-4-itc-time-limit-annual-return-india-2026)
- [TaxGuru — AI in GSTR-2A/2B reconciliation](https://taxguru.in/goods-and-service-tax/ai-in-gstr-2a-2b-reconciliation.html)
- [ICAI — offline GST reconciliation for CA firms](https://ai.icai.org/usecases_details.php?id=78)
- [CORAA — GSTR-9/9C audit checklist for CA firms 2026](https://coraa.ai/blog/gstr-9-9c-annual-return-audit-checklist-guide)
- [KDK — GSTR-2A & 2B reconciliation guide 2026](https://www.kdksoftware.com/blog/gstr-2a-2b-reconciliation/)
- [A2Z Taxcorp — ICMAI IMS handbook](https://a2ztaxcorp.net/icmai-releases-handbook-on-invoice-management-system-under-gst-to-strengthen-digital-compliance-and-input-tax-credit-governance/)

---

## 12. Quality gate

Per slice: backend pytest green including two-client isolation;
frontend suite; `liveCols` guard clean; axe clean on touched routes; no GST
noun in Rust (`scripts/check-namespace-neutrality.py`); a
`plans/00l-build-vs-adopt.md` row if a dependency was weighed; and for
Slice B, the fabricated-figure rejection test mutation-verified.

Every magic number needs a stated reason in this plan (`CLAUDE.md` / `00i`
rule 4) — which is why §16(4)'s date is modelled rather than written down.
