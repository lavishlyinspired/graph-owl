# Plan 123 — Reco Now as an agentic reconciliation workspace on graph-owl

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Not started. Plan only.
**Revision 2** (18 Aug 2026) — adds a correctness audit of the existing
reconciliation, GSTR-2A, event-driven agents, all 31 crates, ontology
alignment, graph analytics, the Matcha reference, and the GraphOWL console
defects.
**Depends on**: Plan 122b §7 (the real-data cutover — the starting state),
`packs/gst`, `plans/00l-build-vs-adopt.md`.
**Reference implementation**: <https://matcha-now.netlify.app/> — walked
live; §4 records what it does that this product does not.

> **Domain-neutrality is a hard constraint and this plan does not bend it.**
> Everything GST-shaped lands in `packs/gst` or `ext-apps/RecoNow`. No GST
> noun enters graph-owl's Rust crates;
> `scripts/check-namespace-neutrality.py` stays in the gate. graph-owl gains
> *generic* capability; the GST meaning lives in the pack.

---

## 1. Correctness audit of the reconciliation as it stands

Read every rule in `packs/gst/queries/`, the ingestion in
`graphowl_client.py`, and the identity code in `gst_identity.py`, then ran
them against real government-format data. **Some of this is genuinely good
work and should not be touched.** The defects below are specific.

### What is correct and stays

| Component | Verdict |
|---|---|
| `amount-mismatch.sparql` | **Correct and well-reasoned.** The Rule 36(4) cap is read from the graph by effective date, not hardcoded. The `OPTIONAL`+`!BOUND` "latest in force" idiom is deliberate — `FILTER NOT EXISTS` would not be pushed down and the guard would be silently true. The ₹1 de-minimis floor is justified (GSTR-3B is filed in whole rupees). |
| `invoice_key()` normalisation | **Correct.** Case/punctuation-insensitive, accents transliterated not deleted, and leading zeros deliberately *preserved* — `INV-001` ≠ `INV-1`, because a wrong match claims credit against the wrong invoice while a missed match only leaves a finding unfired. Failing in the safe direction. Verified live: books `TCS-2024-100` matches portal `TCS/2024/100` in the reference implementation too. |
| Canonical subject per (GSTIN, invoice key) | **Correct.** Both sides meet on one subject; kind-independent. |

### Defect 1 — multi-rate invoice lines collapse (material)

`_subject_iri(kind, row)` mints `{kind}-{gstin}-{invoice_no}` — **one subject
per invoice**. A real GSTR-2B has **one row per rate slab per invoice**: an
invoice with 5%, 12% and 18% lines is three rows.

All three rows mint the *same* subject and each writes `gst:taxableValue`.
And **no rule in the pack uses `SUM` or `GROUP BY`** (checked: zero matches
across all 20 queries). So `?filedLine gst:taxableValue ?filed` binds three
times, and each binding is compared against the book total.

Result on real data: **either three findings for one invoice, or one finding
comparing an invoice total against a single rate line.** Both wrong, both
confidently reported.

This never surfaced because the sample files carry one line per invoice.

**Fix**: aggregate to invoice level at ingestion — emit one
`gst:Gstr2bInvoice` per invoice carrying summed `taxableValue` and per-head
tax, and retain the rate lines as separate `gst:InvoiceLine` subjects so
rate-level questions stay answerable. **RED**: a three-line 2B invoice
produces exactly one finding, and its `filed` value equals the sum.

### Defect 2 — no credit/debit note netting

`note_type` and `original_invoice_no` are mapped at upload and then unused.
No rule nets a credit note against its original invoice (§34). A supplier who
issues a ₹10,000 credit note against a ₹1,00,000 invoice legitimately shows
₹90,000 in the 2B — and Reco Now raises an amount mismatch for ₹10,000 that
is not a mismatch at all.

**Fix**: `gst:CreditNote` / `gst:DebitNote` subjects linked to the original
via `gst:amends`, and the comparison runs against the netted value. **RED**: a
CN exactly explaining a difference produces **no** finding; a CN that
over-explains it produces one.

### Defect 3 — exceptions only, no reconciliation result

Reco Now surfaces findings. It never states the four buckets a CA actually
works in — **Matched / Review / Only Books / Only Portal** — nor a match rate,
nor a per-match confidence and reason. A screen of exceptions cannot answer
"how much of this period is done".

### Defect 4 — "at risk" conflates two different things

Everything unmatched is called at risk. It is not. Credit on an invoice the
supplier has not yet filed is **pending** — deferred, claimable in a later
period once they file. Credit blocked under §17(5) is **lost**. Reporting them
as one number misstates the client's position in both directions.

### Defect 5 — the engine is starved (carried from revision 1)

| | |
|---|---|
| Finding rules registered | **13** |
| Rules that fired on real data | **4** |
| Predicates the rules read | `PaymentEvent`, `GoodsReceipt`, `atTime`, `onInvoice`, `itcAvailable`, `period`, `taxAmount` |
| Predicates ingestion writes | `PurchaseInvoice`, `Gstr2bInvoice`, `Gstr1Invoice`, `recordedIn`, `reflectedIn`, `appearsIn` |

Nine correctly-written rules tied to real provisions **cannot fire** —
`gst:PaymentOverdue` (Rule 37), `gst:ITCNotAvailable` (§17(5)),
`gst:GoodsReceiptTiming` (§16(2)(b)) among them.

### Defect 6 — no Rule 42/43 proportionate reversal

§17(5) blocked credit is modelled. **Rule 42/43** — proportionate reversal
where inputs serve both taxable and exempt supplies — is not modelled at all,
and it is a routine monthly computation for any client with exempt turnover.

---

## 2. GSTR-2A — the omission, and why it is architecturally interesting

Revision 1 covered 2B only. That was wrong.

| | GSTR-2A | GSTR-2B |
|---|---|---|
| Nature | **Dynamic**, updates continuously | **Static**, frozen on the 14th |
| Used for | tracking, follow-up, **GSTR-9 Table 8** | **claiming ITC** |
| Cut-off | none — keeps changing after you file 3B | fixed |

A CA needs both. 2B is what you claim on; **2A is how you find out that a
supplier filed late, or amended, after the 2B you already claimed against was
frozen.** That is precisely the "suppliers might update their filings at any
time" pain point, and it is the single best fit for capability graph-owl
already has:

- **`/drift`** — 2A re-pulled monthly, diffed against the frozen 2B and
  against the prior 2A. Each change becomes a drift item a CA reviews,
  applies or ignores. This is what "what changed since I last looked" *is*.
- **Temporal / as-of** — the console already has an `AS OF` control.
  "What did the 2A say on the day I filed 3B" is a defensible answer to a
  notice, and the flake model already carries time.
- **`gst:B2BA` amendments** — the reference implementation keeps a
  "B2B History Knowledge Bank" of historic 2B files for cross-period
  amendment lookup. Same idea, but a graph does it natively: the amendment is
  an edge, not a lookup table.

**Slice**: ingest 2A as its own kind, alongside 2B, never instead of it.
Findings: `gst:FiledLateInGstr2a` (in 2A, absent from the 2B claimed against),
`gst:AmendedAfterClaim` (2A value now differs from the frozen 2B), and
GSTR-9 Table 8 support. **RED**: a supplier filing in month N+1 for a month-N
invoice produces a `FiledLateInGstr2a` finding against month N and **no**
duplicate claim in month N+1.

---

## 3. All 31 crates, and which ones Reco Now should be using

Revision 1 named a handful. Here is the whole workspace and the honest verdict
for each. **Reco Now currently touches the capability of three.**

| Crate | What it is | Reco Now use |
|---|---|---|
| `graph-owl-core` | domain types, flake model | via HTTP |
| `graph-owl-storage` / `-postgres` / `-memory` | Storage port + adapters | indirect |
| `graph-owl-engine` / `-postgres` | triple store port + adapter | indirect |
| **`graph-owl-query`** (5,456 loc) | **SPARQL over flakes** | **Replace SQL aggregates. Ad-hoc CA questions.** |
| **`graph-owl-reasoning`** (3,491 loc) | OWL 2 RL forward chaining | **`/reasoning/explain` = notice defence** |
| `graph-owl-reasoning-el` | OWL 2 EL via whelk sidecar | classify the GST TBox |
| `graph-owl-reasoning-ql` | OWL 2 QL query rewriting | cheaper inference at query time |
| **`graph-owl-constraint`** (3,997 loc) | SHACL shape compilation | **"every invoice must have a GSTIN of valid form" as a shape, not code** |
| **`graph-owl-ontology`** (2,572 loc) | shapes, **alignment**, profiles, packs | **§7 — ERP vocabulary → GST ontology** |
| **`graph-owl-resolution`** (1,866 loc, 8 modules: bands, rule_match, normalize, temporal, score, mention, subject_identity) | entity resolution | **supplier identity; `mention.rs` resolves a name in an email to a supplier** |
| **`graph-owl-analytics`** (1,236 loc) | `pagerank`, `degree_centrality`, `connected_components`, `orphans`, `AnalyticsBudget` | **§8 — circular trading, supplier centrality, orphan invoices** |
| **`graph-owl-traversal`** / `-memory` | neighbours, bounded subgraph | invoice → supplier → filing → period chains |
| **`graph-owl-search`** / `-hnsw` | search, vector index | semantic supplier//invoice search (**see §9 defect**) |
| **`graph-owl-mcp`** | MCP server over the context graph | **the agent tool surface — §5** |
| `graph-owl-events` | `EventSink`, `ChangeEvent` | **§5 — the trigger bus for event-driven agents** |
| `graph-owl-authz` | `(principal, operation, resource) → Decision` | per-client scoping of agents |
| `graph-owl-connectors` | `Connector` trait + run machinery | GSP/GSTN pull for 2A/2B |
| `graph-owl-rdf-io` | RDF interop | already used for import |
| `graph-owl-lpg` / `-lpg-io` | property-graph model + interchange | export a period to Neo4j for a forensic reviewer |
| `graph-owl-bolt` | Bolt wire protocol | let a CA point a graph tool at it |
| `graph-owl-cli` | apply/plan/**drift**, admin | pack CI |
| `graph-owl-api` | `Catalog` facade | via HTTP |
| `graph-owl-server` | HTTP layer | the boundary |
| `graph-owl-ui` | embedded console | §9 |
| `graph-owl-search-opensearch` | deferred adapter | no |

**Rule for this plan**: Reco Now consumes these over HTTP. It does not link
Rust crates, and it does not reimplement any of them in Python. Where an
endpoint is missing for a capability that exists in a crate, the endpoint is
the work — generic, in graph-owl; never a GST-shaped one.

---

## 4. What the reference implementation does that this does not

Walked <https://matcha-now.netlify.app/> live with its sample data.

**Its information architecture is five stages, not twenty-eight routes**:
`Upload → Map → Reconcile → Intelligence → Act`. That is the workflow. Adopt
the shape.

| Observed | Take |
|---|---|
| Buckets **Matched / Review / Only Books / Only Portal**, and a **match rate** (42.9%) | Adopt — §1 defect 3 |
| Per-row **REASON** ("Exact") and **CONF** (100%) | Adopt — a match needs a why and a strength |
| Both **TAXABLE and TAX** shown for **both** sides, plus **DIFF** | Adopt — invoice-level, not one rate line |
| **Net ITC for GSTR-3B Table 4**, with gross → reversals → net | Adopt — *the deliverable* |
| **"ITC pending (supplier not filed): ₹40,500 — claimable in future periods"** | Adopt — §1 defect 4 |
| **ITC Reversals — §17(5), Rule 42/43** | Adopt — §1 defect 6 |
| **B2B History Knowledge Bank** — historic 2B for B2BA cross-period lookup | Adopt as graph edges, not a lookup table — §2 |
| **Prior Period Excess** | Adopt |
| **Mismatch tolerance set at upload** | Adopt, but as a policy with `/policies/dry-run` |
| AI auto-detects file type and jurisdiction; PDF accepted | Adopt — an ingestion agent, §5 |
| Exports: CSV, **Working Paper .xlsx**, **ITC Register .xlsx** | Adopt |
| **Books↔portal ladder** visual, green/red/amber/blue per bucket | Adopt — far better than a table for conveying a reconciliation |

---

## 5. Agents — event-triggered, visible, and doing much more

Revision 1's five on-demand agents were too few and too passive. Agents
**subscribe to events** and their state is visible at all times.

### The trigger bus

`graph-owl-events` already defines `EventSink`/`ChangeEvent`, and
`/webhooks/*` already exists. Agents subscribe; nothing is polled.

| Event | Agent woken | What it does |
|---|---|---|
| file uploaded | **Ingestion** | detect kind + jurisdiction, propose a column mapping, flag unreadable rows |
| mapping confirmed | **Validation** | run SHACL shapes, propose fixes for malformed GSTINs/dates |
| reconciliation finished | **Triage** | rank what needs a human, with a reason each |
| finding created | **Explainer** | draft the case narrative from `/reasoning/explain` |
| finding created | **Eligibility** | §17(5), §16(2)(b), Rule 37, Rule 42/43, §16(4) |
| supplier unfiled > N days | **Vendor** | draft the chase email |
| **2A re-pulled / drift item raised** | **Drift** | classify each change: late filing, amendment, or reversal |
| resolution queue non-empty | **Identity** | propose merges with evidence |
| §16(4) window closing | **Deadline** | rank invoices by days remaining |
| IMS 14th approaching | **IMS** | flag records that will be deemed accepted |
| period closing | **Close** | assemble the 3B working paper and the blocker list |
| analytics run | **Pattern** | surface rings and abnormal centrality (§8) |

### Seeing what is running

A first-class **Agent activity** screen, and a persistent header indicator:

- what is **running now**, on which client/period, since when
- what each agent **last proposed**, and whether a human accepted it
- **refused attempts** — `/agents/{id}/activity` records these already, and
  for an agentic product that record is worth more than the record of success
- each agent's **grant** (`/agents/{id}/grant`), revocable from the UI
- tokens, latency and cost **per run** — measured or not shown

### The rule that makes this safe (unchanged, and load-bearing)

**An agent may only state a number that appears in a fact it cites.** Claims
carry `fact_ids`; a figure without support is rejected before render and the
rejection is logged. This console has already shipped a fabricated
"₹8.2 L sits inside the s.16(4) window" once (Plan 122b). An LLM will do that
by default, confidently, and about a tax position.

**AC**: every agent write goes through `/proposals`; Reco Now's own `approval`
table is retired. Accept/reject records the human. Revoking a grant mid-run
stops the write and records the refusal.

**RED**: an agent asked to summarise a case with no evidence returns "not
enough evidence", not prose — mutation-verified by removing the `fact_ids`
check and confirming the fabrication test fails.

---

## 6. Reasoning, validation, lineage, memory, threads

| Build | On | For the CA |
|---|---|---|
| Derivation tree behind every case | `/reasoning/explain` | **notice defence** |
| What the law implies but nobody typed | `/reasoning/derived`, `/reasoning/runs` | |
| GSTIN/date/HSN well-formedness as **shapes** | `/validation/*` + `graph-owl-constraint` | data quality without code |
| Accepted exceptions **that expire** | `/validation/waivers` | a waiver that comes back |
| figure → case → fact → source row | `/lineage` | working-paper traceability |
| "this vendor always files late" | `/memories` + supersede/retract | survives periods, correctable, never destroyed |
| preparer ↔ reviewer on the case | `/threads` | review without email |
| tolerance change preview | `/policies/dry-run` | before, not after |
| CA-authored rules from the UI | `POST /packs/gst/finding-rules` | **a new rule is data, not a release** |
| Ask → real query | `/sparql`, `/cypher` | replaces the keyword match |

---

## 7. Ontology and alignment

`graph-owl-ontology` ships `alignment.rs` (`MatchPredicate`,
`AlignmentSource`, `Alignment`, `alignment_to_flakes`) and `profile.rs`
(`detect_rl` / `detect_el` / `detect_ql`), plus pack overrides
(`apply_overrides`, `EffectiveTerm`). None is used.

| Use | Why it matters |
|---|---|
| **Client vocabulary → GST ontology alignment** | Every client's ERP calls things something different. Today that is a per-file column mapping thrown away after upload. As an **alignment** it is a durable, reusable, inspectable asset: "this client's `Party Code` *is* `gst:supplierGstin`". |
| **HSN / rate alignment** | Client item codes → HSN → rate, as graph edges. |
| **Pack overrides** | A client-specific tolerance or term without forking the pack. |
| **Profile detection** | Know which reasoning profile the GST TBox is in, and therefore which reasoner is sound to run. |
| **`resolution/mention.rs`** | Resolve "Sharma Infra" in a supplier email to the actual supplier subject. |

The Map stage becomes an alignment editor, and the mapping template
(Plan 122b's fit-check) becomes the degenerate case of one.

---

## 8. Graph analytics

`graph-owl-analytics` implements `pagerank`, `degree_centrality`,
`connected_components`, `orphans` over a CSR projection with an
`AnalyticsBudget`. Reachable at `/graph/context/analytics` — **verified
working against the real GST subgraph** (returns `inDegree`, `outDegree`,
`orphans`, `edgeTypes` for the March 2026 invoices).

| Algorithm | GST reading |
|---|---|
| **`connected_components`** | Clusters of suppliers and invoices that only transact with each other — the structural signature of **circular trading / fake-ITC rings**, which no per-invoice rule can see. |
| **`pagerank`** | Which supplier is structurally most consequential, not merely largest by rupee — one feeding disputed invoices across many periods. |
| **`degree_centrality`** | Abnormal invoice counts for a supplier's size. |
| **`orphans`** | Invoices connected to nothing: exactly Only-Books and Only-Portal, derived structurally instead of by a bucket rule. |

**RED**: a synthetic three-supplier ring is returned as one component and
ranked above an unconnected supplier of larger value. *Mutator*: break the
ring with one edge — it must stop being reported as a ring.

**Budget**: `AnalyticsBudget` exists; analytics is opt-in per period, not on
every ingest.

---

## 9. The GraphOWL console — real defects found

Verified live against the reconciled March 2026 data.

| Defect | Evidence | Fix |
|---|---|---|
| **Search cannot find graph subjects** | `GET /search?q=INV-MAR-011` → `[]`, while `/graph/context` on the same subject returns its full neighbourhood. Search covers assets, glossary terms and business metrics; the GST data is flakes with no asset representation. | Index graph subjects, or give Explore a non-search entry point. **Explore is the console's main screen and cannot reach the data the console holds.** |
| **Overview reports 0 graph nodes** | Overview: `GRAPH FACTS 724`, `GRAPH NODES 0`. Analytics on one seed returns 9 nodes. | Node counting is wrong or counts a different thing than its label claims. |
| **Explore needs a search term to show anything** | Blank screen with "Search or open an entity". With search broken, unreachable. | Seed from recent subjects, findings, or a pack. |
| **Graph display** | `GraphCanvas.tsx` (AntV G6). CLAUDE.md records two hard-won fixes (canvas listener → React state; double-fire guard). Re-verify against real GST data at scale, plus edge labels for `recordedIn`/`reflectedIn`/`issuedBy` and colouring by bucket. | Render the reconciliation ladder here too. |

Plus: the console behaved **correctly** in one important way and that must not
regress — with every request failing it said so rather than rendering a
plausible overview.

---

## 10. Screens

**Reco Now: 28 routes → the five-stage shape**, plus what a CA cannot get today.

`Data (upload · map/align · files) → Reconcile (ladder + 4 buckets) →
Cases (register · exceptions · case) → Intelligence (ITC position · 3B
working paper · analytics · agents) → Act (IMS · approvals · follow-ups ·
deliverables) → Settings`

**New screens**

| Screen | Why |
|---|---|
| **GSTR-3B working paper** | Gross → §17(5) → Rule 42/43 → net Table 4, every figure traced |
| **ITC position** | confirmed safe · **pending** · at risk · reversed — four numbers, not one |
| **ITC expiry clock** | §16(4), `min(30 Nov, GSTR-9 filing date)`, from the graph |
| **Payments & Rule 37** | unpaid > 180 days, reversal and re-availment |
| **Credit notes & amendments** | §34 + B2BA + 2A drift |
| **Agent activity** | §5 — what is running, what was refused |
| **Patterns** | §8 — rings, centrality, orphans |
| **Notice defence pack** | period/supplier/invoice → every fact, rule, citation, as-of |

---

## 11. Sequencing

| # | Slice | Why here |
|---|---|---|
| **A** | **Correctness**: multi-rate aggregation, credit-note netting, 4 buckets, pending-vs-at-risk | Everything downstream inherits these numbers. Nothing else matters if they are wrong. |
| **B** | **Feed the engine**: payments, GRN, `itcAvailable`, period links | Lights up 9 starved rules with no new reasoning |
| **C** | **GSTR-2A + drift** | The moving-target problem |
| **D** | **Screens → five stages** | Stop maintaining 28 |
| **E** | **Use graph-owl**: explain, resolution, validation, waivers, lineage, memories, threads, proposals | Agents are worth far more on top of this |
| **F** | **Agents, event-triggered + visible** | |
| **G** | **Alignment + analytics** | |
| **H** | **New pack rules**: §16(4), §34, IMS, RCM, Rule 42/43, GSTR-1↔3B, duplicates | |
| **I** | **GraphOWL console fixes** | Independent; can run in parallel |

Slice A alone makes the existing product correct. Every slice ships standing
alone.

---

## 11a. Blocking finding — reconciliation is not period-scoped

**Found 19 August 2026 by verifying the three-state rule outcome across two
periods, and it undercuts that feature.**

Ingest *is* scoped: each (client, period, kind) lands in its own named source
graph (`reco-{hash}-{kind}`), and the store shows one graph per kind per
period, exactly as designed.

**Evaluation is not.** `reconcile_pack` runs each rule's SPARQL across the
whole store, and the new `requires` probe does the same. Both are correct for
one period and wrong for two:

- A period with no goods-receipt file reported `gst:GoodsReceiptTiming` as
  **passed**, because a *different* period had supplied GRN data. That is
  precisely the "checked, clean" lie the three-state outcome exists to
  prevent — reintroduced one level up.
- April, holding only a payment ledger, reported 38 findings. They were
  mostly March's.

**This is not a regression.** The unscoped reconcile is pre-existing and
already recorded in `_ingest_scoped_to_graphowl`'s own comment. What is new is
that more now rests on it: rule outcomes are stored per (client, period) while
the engine that produced them cannot tell periods apart, so the *storage*
implies a precision the *computation* does not have.

**Consequences to accept until it is fixed**: with one period per client
everything above is correct. With two, per-period rule outcomes and finding
counts are unreliable, and "passed" is the dangerous direction.

**The fix is not a probe tweak.** Scoping the probe alone would leave findings
global — a period would then show honest rule states beside another period's
findings, which is worse than uniformly wrong. Reconciliation needs a scope
argument end to end: which named graphs a run may read, applied to both the
rule queries and the requirement probes. That is a graph-owl change of real
size and it should be its own slice, **before F (agents)** — an agent
grounded in cross-period findings would be confidently wrong.

Sequencing therefore inserts **C0 · period-scoped evaluation** ahead of C.

### C0 — shipped 19 August 2026

`in_run_scope` sits between authorization and the fact budget in
`scope_facts`. The two questions stay separate on purpose: authorization
answers *may this principal see this fact*, a run scope answers *should this
evaluation be looking at it*. Collapsing them would mean widening a policy to
narrow a run.

Three decisions worth stating, because each has a failure mode:

- **A fact belonging to no named graph is always in scope.** Schema,
  projection and vocabulary facts are not import-sourced and belong to no run.
  Excluding them would leave a scoped run holding data it cannot interpret.
- **The caller names a graph by the source it imported under**
  (`reco-abc-books`); the stored id carries graph-owl's own `graph:import:`
  prefix. Matching the id *or* its trailing segment means the caller does not
  have to know a convention that is graph-owl's rather than theirs. A looser
  prefix match was rejected and has a test against it — `reco-abc` naming
  `reco-abc-books` would silently widen every run.
- **The scope applies to the requirement probes too**, not only the rule
  queries. A requirement satisfied by another period's data is the same bug one
  layer down, and it produces the more dangerous answer: the rule runs and
  reports.

Verified against two real periods of one client — March unchanged (₹72,900
confirmed, ₹89,800 blocked, both `gst:GoodsReceiptTiming` and
`gst:ITCNotAvailable` FAILED), April correctly NOT EVALUATED for both, having
uploaded neither a GRN column nor an ITC flag. Narrowing cost the working case
nothing, which is the check that matters: a scope that breaks the case it was
not aimed at has not been verified, only observed.

**Known gap, deliberately not closed here.** Findings are stored per pack with
no run tag, so two periods holding the *same* invoice numbers would still share
findings. Real filing periods do not repeat invoice numbers, and the durable
fix — a run-tagged finding — belongs with the findings model, not with the
scope predicate. `findings_for_period` filters by invoice identity as an
interim measure, and derives that identity from the finding's own evidence
bindings rather than a top-level field, because a raw finding has no
`invoice_no` key at all. That mistake shipped once in this slice and was caught
only by a period reading zero blocked ITC while its own rule said FAILED.

**A process failure this slice exposed, recorded because it is the more useful
finding.** `cargo test -p graph-owl-api --lib` had not compiled since the C0
commit — `scope_facts` gained a parameter, `FindingRuleDef` had gained
`requires` one commit earlier, and 18 test call sites were never updated. 736
passing tests sat invisible behind a compile error, and the slice was verified
end to end instead. `in_run_scope` itself had **no unit test at all**, for a
predicate whose permissive failure mode is a rule reporting on data the run was
never given. Closed with 9 tests, negative cases included, and mutation-tested:
3 mutants, 3 caught, 0 missed, 0 unviable. The lesson is not "run the tests" —
it is that an epic-end fast loop must include `cargo test -p <crate> --lib` for
every crate whose *signatures* changed, not only the crate whose behaviour did.

### GSTR-3B — the summary return, and a correction to Slice C

**Added 19 August 2026 after a direct challenge: "gstr1 and gstr2a are the
same thing, isn't it?"** Checked against sources rather than recalled, and the
answer is worth writing down because it has now been got wrong once here.

They are **not** the same, and the relationship is asymmetric. GSTR-1 is what
a **supplier** files — their outward supplies. GSTR-2A is auto-populated **for
the recipient** from those GSTR-1s, plus GSTR-5/6/7/8. Two sides of one flow,
and critically: *a recipient cannot download a supplier's GSTR-1.* They only
ever see its effect in their own 2A.

**The correction that follows**: in this product's ITC context the
pre-existing `gstr1` kind was already 2A data. All three of its rules
(`missing-in-books`, `gstr1-not-in-2b`, `books-gstr1-mismatch`) are inward —
"what the supplier declared about me", which is 2A's definition. Slice C's
split was right on the temporal axis (2A has a pull date, a filing does not)
but it added a kind beside one that already meant the same thing, rather than
noticing the existing one was mislabelled. `gstr1` is kept for the narrow real
case — supplier filing data obtained directly, as in an intra-group
reconciliation — and the UI now says which authority each represents instead
of the old conflated "GSTR-2A / GSTR-1" label.

**What was actually missing was GSTR-3B**, which §4 already names *the
deliverable* ("Net ITC for GSTR-3B Table 4, with gross → reversals → net").

Table 4's structure, current format:

| row | holds |
|---|---|
| 4A | gross ITC, **auto-populated from GSTR-2B** — eligible and ineligible both |
| 4B(1) | permanent reversals — Rule 38, Rules 42/43, s.17(5). Not reclaimable. |
| 4B(2) | reclaimable reversals — Rule 37, s.16(2)(b)/(c) |
| 4C | net ITC to the credit ledger. 4A − 4B. |
| 4D(1) | ITC reclaimed that an earlier period reversed |
| 4D(2) | ITC unavailable by law — s.16(4), place of supply |

**4A being auto-populated from 2B is the whole basis of the reconciliation.**
The portal states what it made available and the taxpayer states what they
took; the two are meant to be the same number, and where they are not the
taxpayer usually learns it from a notice.

Three design decisions, each with a failure mode:

- **A summary is not a document.** Every other kind is line-level, keyed by
  invoice and GSTIN. A 3B has neither, so it gets its own ingestion path
  (`SUMMARY_KINDS`) and neither credit-note netting nor rate-line aggregation
  is applied — both would run against a figure the filer already totalled.
  A period is **mandatory** here, unlike every other field in the client: a 3B
  naming no period is not a partial answer, it is an unplaceable one.
- **The two directions are never one signed number.** Claiming more than 2B
  supports is an excess claim — interest under s.50, demand under s.73/74.
  Claiming less is credit left unclaimed, recoverable until s.16(4) closes.
  Opposite situations with opposite remedies, and a sign is the easiest thing
  on a screen to misread, so the direction is named in words and the magnitude
  is always unsigned.
- **4A is compared, not 4C.** 4A is the row 2B populates. 4C is 4A minus
  reversals the taxpayer made deliberately, so comparing *that* against 2B
  would report every legitimate s.17(5) reversal as an under-claim — turning
  correct compliance into a finding. This is the mutation that killed four
  tests when tried.

**Two things this unlocks that nothing else could.** GSTR-9 Table 8B, which
this codebase reported as uncomputable a commit earlier, is now derived from
4C summed across the year (the strict definition is Table 6(B)+6(H); the two
can differ where a 6(H) reclaim straddles the year end, and that is stated
rather than glossed). And **Rule 37 closes**: the engine already finds
invoices unpaid past 180 days and had no way to know whether the reversal was
made. 4B(2) is where a Rule 37 reversal is reported, so an unreversed overdue
credit is now visible as an exposure sitting in a *filed* return rather than
an item on a to-do list. Absence stays absence — no 3B, or a blank 4B(2),
reports not-evaluated, because neither is evidence a reversal was skipped and
reporting it as failure accuses a taxpayer on the strength of missing data.

Computed in `app/itc_3b.py` rather than as pack rules, deliberately: both
compare a period **total** against a summary figure, which is aggregation
rather than a join over invoices. `app/gstr9.py` already sets that precedent.

### C0, continued — the rule panel says what it checked

Two gaps named directly in a live session, both about a screen that reports
correctly and communicates nothing.

`RuleOutcome` gained `summary`, carried end to end: `graph-owl-api` → the
reconcile response → migration `0007` → `repo.py` → the React panel. The packs
already wrote good one-line summaries and nothing consumed them. A label alone
is not an explanation — `gst:PaymentOverdue` means something only to whoever
wrote it. A rule that supplies no summary stores `NULL` and the panel renders
nothing rather than inventing text, which is the same three-state discipline
this plan applies to rule outcomes themselves.

A flagged rule's "N findings" became a control rather than a fact: clicking it
filters the invoice table to the invoices carrying that label. `visibleRows`
was extracted as a pure function so the two independent narrowings — bucket and
rule label — are testable apart from the component. Stryker is not configured
for this frontend; its mutants were reasoned through by hand and that is stated
here rather than reported as a run.

---

## 12. Risks

| Risk | Handling |
|---|---|
| **LLM fabricates a tax figure** | §5's citation contract, enforced, mutation-tested, plus a human gate on every write |
| **Wrong reconciliation ships confidently** | Slice A first; every rule gets a positive **and** a negative test — every surviving mutant in this project so far has been a missing negative |
| **Domain leak into graph-owl** | `check-namespace-neutrality.py` in the gate; no GST noun in Rust |
| **Law changes** | Provisions are dated pack data; nothing to redeploy |
| **Agent cost / runaway** | Event-triggered but budgeted, measured per run, grants revocable mid-run |
| **Analytics cost** | `AnalyticsBudget`, opt-in per period |
| **Sources disagree** (they did — §13) | Law lives in the pack with a citation; a wrong provision is visible and fixable as data |

---

## 13. Sources

- ClearTax — [IMS under GST](https://cleartax.in/s/invoice-management-system-ims-under-gst) · [Section 16(4)](https://cleartax.in/s/section-16-4-of-cgst-act) · [Rule 37 — 180-day reversal](https://cleartax.in/s/rule-37-of-cgst-sgst-rules-itc-reversal-180-days)
- [Tax Garden — §16(4): 30 Nov deadline](https://taxgarden.in/blog/gst-section-16-4-itc-time-limit-annual-return-india-2026) — **30 November is correct**; [SmartGST's IMS guide](https://smartgst.in/blog/gst-invoice-management-system-ims-mandatory-guide-2026) states 30 September and is not relied on. The rule is the *earlier of* 30 Nov or the date GSTR-9 is actually filed — which is why it is modelled, not written down.
- [KDK — GSTR-2A & 2B reconciliation 2026](https://www.kdksoftware.com/blog/gstr-2a-2b-reconciliation/) · [Tata nexarc — 2A vs 2B](https://blog.tatanexarc.com/msme/gstr-2a-vs-gstr-2b/) · [IndiaFilings — 2A vs 2B](https://www.indiafilings.com/learn/difference-between-gstr-2a-and-gstr-2b)
- [CompuTax — GSTR 1, 3B, 2A and 2B reconciliation](https://www.computaxonline.com/blog/post/2026/01/15/gst-reconciliation-gstr-1-3b-2a-2b)
- [CORAA — GSTR-9/9C audit checklist 2026](https://coraa.ai/blog/gstr-9-9c-annual-return-audit-checklist-guide)
- [TaxGuru — AI in GSTR-2A/2B reconciliation](https://taxguru.in/goods-and-service-tax/ai-in-gstr-2a-2b-reconciliation.html) · [ICAI — offline reconciliation for CA firms](https://ai.icai.org/usecases_details.php?id=78)
- [A2Z Taxcorp — ICMAI IMS handbook](https://a2ztaxcorp.net/icmai-releases-handbook-on-invoice-management-system-under-gst-to-strengthen-digital-compliance-and-input-tax-credit-governance/)
- **Reference implementation**: <https://matcha-now.netlify.app/> (walked live, §4)

---

## 14. Quality gate

Per slice: backend pytest including two-client isolation; frontend suite;
`liveCols` guard clean; axe clean on touched routes; no GST noun in Rust
(`scripts/check-namespace-neutrality.py`); a `00l-build-vs-adopt.md` row if a
dependency was weighed. Slice A's aggregation and netting, and Slice F's
fabricated-figure rejection, are **mutation-verified**.

Every magic number needs a stated reason (`CLAUDE.md` / `00i` rule 4) — which
is why §16(4)'s date is modelled and the ₹1 floor carries its justification in
the query.
