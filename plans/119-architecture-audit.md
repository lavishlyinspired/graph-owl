# Plan 119 — Architecture audit: packs, connectors, MCP, and where GST logic actually lives

**Branch**: main. **Status**: Audit only — no code moved or deleted. Per
explicit instruction: produce the audit, identify ownership conflicts,
propose resolutions; implement nothing until reviewed.

**Method**: every claim below is checked against imports, doc comments in
the code itself, actual test fixtures, `.github/workflows/`, and
`scripts/gate.sh` — not against directory names or the historical narrative
that motivated this audit. Two places the narrative and the code disagree
are called out explicitly, because that's exactly the kind of thing this
audit exists to catch.

## 0. One correction to the premise, stated first because it changes how to read everything below

`ext-apps/Reco/graphowl-pack/` is not the product of gradual organic
drift. **It's 45 minutes old** — I (the agent) created it in the immediately
preceding session, as Slice 1 of `plans/118-reco-now-integration.md`, before
this audit was requested. The "historical evolution" in the prompt (`packs/gst
→ GST experimentation → Reco created → Reco gets its own graphowl-pack`) is
correct in shape but the last step happened just now, by me, without first
checking whether `packs/gst` could simply be extended. §5 below is the
resulting finding, and it's a real one: that pack has a duplication problem
worth fixing.

## 1. The corrected mental model, checked against the code

The prompt's framework holds up well against what's actually implemented,
with the evidence living in first-party doc comments rather than needing to
be inferred:

| Layer | Confirms as | Evidence |
|---|---|---|
| **Core** | Domain-agnostic graph/ontology/reasoning/runtime | `plans/105-domain-neutrality.md`: 18 allowlisted namespace constants, CI-enforced (`scripts/check-namespace-neutrality.py`) against any new domain constant landing in `graph-owl-core` |
| **Pack** | Domain/use-case extension: vocabulary + ontology + glossary + rules + queries + findings + fixtures + eval — not just "what the engine knows" | `packs/gst/pack.toml`: `[pack]`, `[glossary]`, `[[documents]]`, `[[predicates]]`, `[[matching.blocking]]`, `[[findings]]` (with `[findings.guidance]`, `[findings.similarity]`, `[findings.span]`), `[[queries]]`, `[console]`. `packs/gst/eval/questions.md` is a ninth category the prompt's list didn't name: hand-derived Q&A for scoring the pack's own reconciliation quality, independent of any code |
| **Connector** | External source → graph-owl canonical representation, narrow responsibility | `crates/graph-owl-connectors/src/lib.rs`'s own doc comment (quoted in full in §4) |
| **MCP** | Agent-facing interface, domain-agnostic | 10 registered tools, verified by grep against `crates/graph-owl-mcp` and `crates/graph-owl-server`: `search_assets`, `get_asset_context`, `explain_lineage`, `analyze_impact`, `get_governance_context`, `query_graph`, `recall_memory`, `calculate_risk`, `resolve_entity`, `run_rule` — none domain-named. (Up from the 7 recorded when this session first surveyed it; 3 were added since.) |
| **Application** | User-facing product built on the engine | `ext-apps/Reco/` |

One thing the prompt's framework doesn't yet have a slot for, found during
this audit: **a pack-specific connector**, in Python, that is neither
generic core infrastructure nor an application. See §4.

## 2. The audit table

| Current location | What it does | Used by (verified) | Canonical? | Proposed owner |
|---|---|---|---|---|
| `packs/gst/` | The reference GST pack: ontology (10 classes, ~30 properties), glossary, statute text (`law/`), 6 finding rules + their SPARQL queries, matching/blocking config, an eval set | `scripts/demo.sh --gst`, `scripts/verify-pack-load.sh`, `scripts/verify-gst-reconciliation.sh` (all load it for real, end to end); `examples/gst-reconcile/reconcile_agent.py` (scored against its eval set); dozens of Rust test fixtures mirror its `gst:` namespace/shape as realistic test data (`graph-owl-mcp`, `graph-owl-resolution::rule_match`, `graph-owl-resolution::temporal`, `graph-owl-server/tests/{reconcile,pack_install}.rs`) | **Yes** — the platform's own P1/P9/DN-3 reference pack | Packs (unchanged) |
| `packs/hospitality/` | The second proof pack — DN-3's "shares nothing with tax" acceptance test | `scripts/verify-pack-load.sh` | Yes, for its stated purpose (neutrality proof) | Packs (unchanged) |
| `ext-apps/Reco/graphowl-pack/` | Reco's own pack: namespace `reco:`, 17 predicates matching Reco's literal CSV field set. Ingestion-only — no findings, queries, or matching config | `ext-apps/Reco/backend/app/main.py` (`_install_graphowl_pack`, `_ingest_to_graphowl`) | **No — and it shouldn't be, as currently shaped.** See §5: it duplicates 5+ predicates `packs/gst` already has | Packs, but needs to become an *extension* of `packs/gst`, not a parallel pack |
| `connectors/python/graph_owl_packs/{loader,manifest,cli}.py` | Generic pack-loading infrastructure: read `pack.toml`, declare namespace, register predicates, import documents, register finding rules/queries. **Knows no domain** — its own docstring: *"The loader has no knowledge of hospitality, tax or automotive"* | `packs/gst`, `packs/hospitality` (via `scripts/demo.sh`, `verify-pack-load.sh`), `ext-apps/Reco` (via my Slice 1), `connectors/python/tests/` (30 tests) | **Yes** — the only pack-loading implementation that exists, Python side, by design (`00j-language-boundaries.md`) | Pack infrastructure (Python) |
| `connectors/python/graph_owl_packs/gstr2b.py` | A **connector**, mislabeled by its parent package's name: normalizes a live GSP's GSTR-2B JSON into `packs/gst`'s vocabulary. Own docstring literally says "GSTR-2B as an ingestion connector" | `connectors/python/tests/test_gstr2b.py`; CLI entry point `graph-owl-gstr2b` | Yes, for what it is — but it's a connector living inside a package named `graph_owl_packs` | **Connectors** (Python), not Packs |
| `connectors/python/graph_owl_packs/erpnext.py` | A **connector** — discovers a Frappe/ERPNext DocType's schema at runtime and lands it, with zero domain knowledge. The strongest existing evidence for Epic 105's neutrality claim (a schema this project didn't design for itself) | `connectors/python/tests/test_erpnext.py`; CLI entry point `graph-owl-erpnext` | Yes, same mislabeling issue | **Connectors** (Python) |
| `connectors/python/graph_owl_packs/reconcile.py` | **Not an implementation.** Its own docstring: *"This module used to evaluate finding rules... The rule evaluator is now native... this module only ever asks the server to run them."* A 30-line trigger for `POST /packs/{id}/reconcile` | `connectors/python/tests/test_reconcile.py`; CLI entry point `graph-owl-reconcile` | Yes, as a thin trigger | Pack infrastructure (Python) — correctly scoped already |
| `crates/graph-owl-connectors/` | Generic ingestion governance: the `Connector` trait, run scoping/ordering, plus one real implementation (Postgres) and shared batch/streaming/webhook machinery | Its own doc comment: *"Connectors beyond this one are Python, out of process, pushing through the ingestion API — see `00j-language-boundaries.md`. What stays here is the governance part."* | Yes | Connectors (Rust) — **explicitly, by its own doc comment, the sibling of the Python connectors above, not their replacement** |
| `crates/graph-owl-mcp/` + MCP routes in `graph-owl-server` | Agent-facing interface: 10 domain-agnostic tools, read-only (write deferred to Epic 32) | `integrations/langchain/graph_owl_langchain` (the LangChain tool wrappers), the console's Agent tab, `crates/graph-owl-server/tests/mcp.rs` | Yes | MCP (unchanged) |
| `examples/gst-reconcile/reconcile_agent.py` | **Not a reconciliation implementation.** Reads findings *already produced* by the native engine and optionally narrates them with an LLM; scored against `packs/gst/eval/questions.md` with zero API cost. Governed by `scripts/check-examples-purity.py` (stdlib + `graph_owl_sdk` only) | Its own test (`test_reconcile_agent.py`), `eval_scoring.py` | Yes, as a reference example | Examples (Epic 36 reference apps) — no change |
| `integrations/langchain/agent_service/reconcile_uploaded.py` | **Deliberately a second, different mechanism** — its own docstring says so explicitly: ad hoc, chat-attached, session-only reconciliation with a fixed rounding tolerance, never persisted, "as distinct from the pack-backed reconciliation... Deliberately not the same mechanism, and not trying to be" | `integrations/langchain/agent_service/server.py` (the LangChain tool surface), its own tests | Yes, for its own stated scope | Integrations (LangChain) — no change |
| `ext-apps/Reco/backend/app/reconciliation.py` | A **third, independent** matching implementation: exact/fuzzy `(gstin, invoice_no)` key + tolerance compare, in-memory, over pandas-parsed rows. Unlike the two above, **its own docstring does not acknowledge the native engine exists** | `ext-apps/Reco/backend/app/main.py`'s `/api/reconcile` | **This is the real duplication.** Not a documented, deliberate alternative like the two above — an independent reimplementation that never calls `POST /packs/{id}/reconcile` at all | Application (Reco) for now, but this is Slice 2's target — see `plans/118-reco-now-integration.md` |
| `ext-apps/Reco/backend/app/{main.py FIELD_LABELS, ai.py templates, exporters.py}` | Column auto-mapping keywords, Section 16(2)(aa) email/report templates, ITC-register/working-paper export formats | Reco's own upload/reconcile/act flow | N/A — this is genuinely product/UX logic (column guessing heuristics, report formatting), not domain semantics an engine pack should own | **Application (Reco)** — correctly placed already |

## 3. Duplication and ownership conflicts, ranked by how real they are

### 3.1 Real: `ext-apps/Reco/graphowl-pack` duplicates `packs/gst` vocabulary

Confirmed by direct comparison: `packs/gst/ontology.ttl` already declares
`gst:invoiceNumber`, `gst:supplierGstin`, `gst:invoiceDate`, `gst:taxableValue`,
`gst:reverseCharge`, and `pack.toml` already registers `igst`/`cgst`/`sgst`/
`cess`/`supplierName` as predicates. My Slice 1 pack independently declared
`reco:invoiceNumber`, `reco:supplierGstin`, `reco:invoiceDate`,
`reco:taxableValue`, `reco:reverseCharge`, `reco:igst/cgst/sgst/cess`,
`reco:supplierName` — **seven-plus predicates that already exist, under a
namespace nothing relates back to the pack that has them.** This is exactly
the framework's Option B territory from the prompt, done wrong: I built a
parallel pack instead of an extension.

**What Reco's pack genuinely needs that `packs/gst` doesn't have**: `hsnCode`,
`imsStatus`, `noteType`, `voucherType`, `voucherNumber`,
`originalInvoiceNumber` — six real, non-overlapping fields.

**Proposed fix (not yet done)**: `ext-apps/Reco/graphowl-pack` keeps its own
namespace declaration (a pack must own one to register anything at all — DN-1
doesn't support "borrow another pack's namespace"), but registers only those
six genuinely new predicates. Reco's ingestion writes subjects using **`gst:`
predicates for every field `packs/gst` already has**, and `reco:` predicates
only for the six it doesn't — the composition the prompt's Option B describes,
rather than the Option-C-shaped duplicate I actually built. This requires
`packs/gst` to be loaded *before* Reco's pack in `_install_graphowl_pack`
(predicates must be registered before any document uses them, per Epic 105's
own P1 finding), which is a real dependency Reco's pack.toml doesn't declare
today.

### 3.2 Not duplication, already self-documented: the two "other" reconcilers

`reconcile_agent.py` (reads engine output, narrates) and
`reconcile_uploaded.py` (deliberately ungrounded, chat-scoped, no legal-cap
tolerance) both explain in their own docstrings why they aren't the same
mechanism as the native engine. Nothing to fix here — flagging only because
the prompt's framing treated "multiple GST-shaped Python files" as
inherently suspicious, and these two survive inspection.

### 3.3 Real but different in kind: `ext-apps/Reco/backend/app/reconciliation.py`

This one has no such self-awareness. It reimplements matching from scratch,
in a way that has no path back to the native engine's tolerance-from-statute
behaviour (Rule 36(4), traversed by date) — it uses a flat `tolerance: float`
the user sets by hand. This is `plans/118-reco-now-integration.md`'s
already-named Slice 2 (deferred, not started): route `/api/reconcile` through
`POST /packs/{id}/reconcile` once Reco's pack registers findings/queries, and
retire the hand-rolled matcher. Not done in this audit — named here because
it's the clearest real duplication in the whole tree.

### 3.4 Not duplication, mislabeled: `connectors/python/graph_owl_packs`

The package name is the whole source of the suspicion in the prompt. Once
separated by what each file actually does:

- `loader.py` / `manifest.py` / `cli.py` → **pack infrastructure**, correctly
  named
- `gstr2b.py` / `erpnext.py` → **connectors**, incorrectly filed under a
  package called `graph_owl_packs`
- `reconcile.py` → a pack-reconciliation **trigger**, arguably infrastructure,
  arguably a connector-adjacent utility — either reading is defensible

`crates/graph-owl-connectors`'s own doc comment settles whether this is a
competing system: it isn't — it names Python-out-of-process as the
*intended* location for every connector beyond Postgres. The Rust crate
(`batch`, `document`, `extraction`, `ingest`, `job`, `postgres`, `rows`,
`streaming`, `streaming_pulsar`, `umls`, `webhook_mapping`,
`webhook_signature`) is generic ingestion-pipeline governance; the Python
files are small, per-external-system adapters that only need to speak HTTP
to graph-owl's already-public API. Different problems, deliberately split
by language per `00j-language-boundaries.md` — not duplication.

**What's real**: the package name. A reader who does exactly what this
prompt did — read `connectors/python/graph_owl_packs/` top to bottom — will
reasonably suspect duplication with `crates/graph-owl-connectors` on name
alone. Worth a rename (`graph_owl_packs` → something like
`graph_owl_pack_tools`, with `gstr2b.py`/`erpnext.py` in a sibling
`connectors/python/graph_owl_connectors/` package) purely for
discoverability. Low priority, no behaviour change, not done here.

### 3.5 Process gap, not architecture: nothing Python is in CI

`grep` against `.github/workflows/*.yml` for `connectors/python`,
`integrations/langchain`, or `graph_owl_packs` returns nothing.
`scripts/gate.sh` never invokes `pytest`. Every one of `connectors/python`'s
30 tests, `integrations/langchain`'s 16 test files, and `examples/`'s tests
run only when someone remembers to run them by hand (or via
`scripts/verify-pack-load.sh`/`demo.sh`, which exercise behaviour, not the
unit suites). This doesn't make any of it non-canonical — `crates/` is the
thing CI actually gates — but it's worth naming as a real gap before calling
anything here "the canonical implementation," since canonical usually implies
"and CI would catch someone breaking it."

## 4. `crates/graph-owl-connectors/src/lib.rs`, quoted in full (the evidence for §3.4)

> Source connectors: the `Connector` trait, the run machinery, and the
> Postgres reference implementation.
>
> **Status**: Epic 15, first vertical slice. Connectors beyond this one are
> Python, out of process, pushing through the ingestion API — see
> `plans/00j-language-boundaries.md`. What stays here is the governance part:
> the trait, run scoping, and the ordering guarantee.

## 5. What this audit recommends, pending review — nothing below is done

1. **Fix `ext-apps/Reco/graphowl-pack`** (§3.1): drop the 7 duplicated
   predicates, keep only the 6 genuinely new ones, load `packs/gst` first.
   Smallest, highest-value fix — undoes a mistake from 45 minutes ago before
   anything else builds on it.
2. **Leave `packs/gst`, `crates/graph-owl-connectors`, `crates/graph-owl-mcp`,
   `examples/gst-reconcile`, `integrations/langchain` exactly as they are** —
   each is correctly scoped and either canonical or a documented deliberate
   alternative.
3. **Slice 2 of `plans/118-reco-now-integration.md`** — corrected per review.
   The original wording ("retire `reconciliation.py`") was too blunt: it
   does not follow that because `reconciliation.py` never calls the native
   engine, 100% of its *behavior* already exists there. That has to be
   verified per capability, not assumed from "it's a duplicate
   implementation." §5a below is that verification, done now rather than
   deferred, because it changes how big Slice 2 actually is.

### 5a. Behavior mapping — `reconciliation.py` vs the native engine, verified against `packs/gst/pack.toml` and its `.sparql` queries directly, not assumed

| Reco behavior | Native engine equivalent | Verified how | Action |
|---|---|---|---|
| `normalize_gstin`/`normalize_invoice_no` (case/punctuation strip) | `[[matching.blocking]] strategy = "normalized"` over `gst:supplierGstin`/`gst:invoiceNumber` (`graph-owl-core::blocking_strategy::Normalized`, DN-2) | Read `pack.toml` lines ~200-230 | Superseded |
| Exact key match (`invoice_key`, GSTIN+invoice number) | Canonical `gst:Invoice` join — Plan 109 Slice 2's own comment in `amount-mismatch.sparql`: "computed deterministically from the exact GSTIN and normalized invoice number" | Read the query | Superseded |
| Fuzzy fallback (Reco's is just the *same* normalized-key match run twice — no real edit-distance tolerance) | `ngram` blocking (n=3) on `supplierGstin` and `invoiceNumber`, feeding `GstinTransposition`/`SupplierPanMismatch` with tuned similarity bands (0.40-0.999) | Read `pack.toml`'s `[findings.similarity]` blocks | **Native is strictly more capable** — Reco has no real fuzzy match today |
| Flat tolerance (`SESSION["tolerance"]`, user-adjustable, default ₹1) vs. `AmountMismatch` | **Corrected 16 August 2026 — resolved, not open.** The first pass of this table read only the query's header comment ("the cap is read from the graph") and stopped there. The query's *filter clause*, read in full, has two conditions ANDed together: the statutory `capPercent` threshold, **and** a flat `> 1` floor — with its own comment explaining why: *"a queue full of ₹0.01 findings is one a reviewer stops reading... ₹1 is chosen as the smallest unit a CA would ever act on."* `packs/gst/law/rule-36-4.ttl` also confirms the cap is nil from 2022 onward — so for any 2026 invoice (all of Reco's real data), the statutory half of the filter is `> 0`, and the ₹1 floor is what actually governs, same as Reco's own default tolerance | Read `amount-mismatch.sparql`'s full `FILTER` clause, not just its header comment, plus `rule-36-4.ttl`'s dated provisions | **Superseded — native is a strict improvement, not a semantic gap.** It has Reco's own ₹1 floor already, plus correctly relaxes it for any invoice actually dated before 2022 (which Reco's flat tolerance does not do — Reco would apply ₹1 to a 2020 invoice too, where the law allowed a 10% band) |
| `STATUS_ONLY_BOOKS` (single status: "not in GSTR-2B") | Two findings, not one: `PotentialMismatch` (no GSTR-1 loaded — the base case) *or* `SupplierNotFiled` (GSTR-1 loaded, supplier genuinely didn't file) | `pack.toml`: `PotentialMismatch` "stands down entirely once GSTR-1 evidence is loaded" | **Native is strictly more capable** — it tells apart two causes Reco's single status conflates |
| `STATUS_ONLY_GSTR2B` (single status: "not in books") | `MissingInBooks` (`missing-in-books.sparql`) | **Corrected 16 August 2026**: this table originally read the query's summary and stopped. Its `WHERE` clause requires `gst:Gstr1Invoice` in a mandatory `GRAPH` join, not an `OPTIONAL` guard the way `missing-in-gstr2b.sparql` does — checked directly, not assumed. Moves to the "needs GSTR-1" row below | Not reachable with books+2B alone — see the corrected count below |
| `STATUS_MATCHED` (silent — no row emitted) | Absence of any finding for a canonical invoice | Convention stated throughout `pack.toml`'s finding comments | Superseded |
| `classify_mismatches`'s hardcoded strings (`"Section 16(2)(aa), CGST Act"`) | `governed_by` + `law/sections.ttl`, traversed | Verified present, not assumed | Superseded — native is authoritative (real law graph vs. a literal string that can drift from the statute) |
| GSTIN transposition, tax-head mismatch, supplier-PAN-same-GSTIN mismatch | **Not present in `reconciliation.py` at all** | Grepped `reconciliation.py` for equivalents — none | New capability, not a migration |
| Three-way books/GSTR-1/GSTR-2B split (`SupplierNotFiled`, `Gstr1NotIn2b`, `BooksGstr1Mismatch`, and — corrected — `MissingInBooks`), goods-receipt timing, 180-day payment-overdue | **Not present**, and **not reachable today** — these query `gst:Gstr1Invoice`/`gst:GoodsReceipt`/`gst:PaymentEvent` in a *required* join, none of which Reco's two-dataset (books + GSTR-2B) upload flow ingests | Grepped each `.sparql` file's `WHERE` clause for whether the reference is inside `OPTIONAL` (fires fine with no data) or a plain `GRAPH` pattern (requires it) — not just whether the string appears | Blocked on Reco's UI gaining GSTR-1/goods-receipt/payment upload — **not in scope for the reconciliation.py→engine swap alone** |
| `supplier_health` (ITC-at-risk rollup by supplier), `ims_actions` (accept/follow-up/investigate buckets), `match_stats` (totals, match rate) | No engine equivalent — these are presentation aggregations, and IMS is a GST-portal UI concept the engine has no reason to know about | Checked `graph-owl-resolution`/`graph-owl-api` for anything analogous — nothing | **Reco-owned**, correctly — these should be recomputed *over the engine's findings* instead of over `reconciliation.py`'s own result rows, not moved anywhere |

**What this changes about Slice 2's scope — corrected three times now, each
time by checking the query text instead of its name/summary.** `packs/gst`
registers **13** finding rules (recounted directly, `grep -c
'^\[\[findings\]\]' packs/gst/pack.toml` — an earlier pass here said 12).
7 of the 13 (`PotentialMismatch`, `AmountMismatch`, `ITCNotAvailable`,
`Reversed`, `GstinTransposition`, `TaxHeadMismatch`, `SupplierPanMismatch`)
are reachable with the two datasets Reco already collects — `MissingInBooks`
moved out of this group (above). Of those 7, 2 (`GstinTransposition`,
`SupplierPanMismatch`) additionally need `[matching.blocking]` similarity
config not yet built. Of the remaining 5, 2 more turned out to need data
or value-coding reco-now doesn't have: `ITCNotAvailable` reads
`gst:itcAvailable`, a field reco-now's CSV format has no column for at
all; `Reversed` filters `gst:reverseCharge = "R"` (the GST portal's own
code), while reco-now's `reverse_charge` values are plain "Yes"/"No" text
— found only once the actual finding queries' `WHERE` clauses were read
in full (§5c), not from field-level vocabulary compatibility. **3
implemented in this slice**: `PotentialMismatch`, `AmountMismatch`,
`TaxHeadMismatch` — see `ext-apps/Reco/graphowl-pack/queries/README.md`.
The other 10 (6 needing GSTR-1/goods-receipt/payment data —
`SupplierNotFiled`, `Gstr1NotIn2b`, `MissingInBooks`, `BooksGstr1Mismatch`,
`GoodsReceiptTiming`, `PaymentOverdue` — plus the 2 needing blocking config
and the 2 needing field work above) are separate, later slices, not part
of retiring `reconciliation.py`.

### 5b. The corrected retirement sequence for `reconciliation.py`

Replacing the earlier "retire it in Slice 2" with the specific, ordered
sequence below. **Nothing in this sequence has been executed** — it's the
plan for Slice 2, not a report of what happened:

| Step | Action | Result |
|---|---|---|
| 1 | Inspect/extract `reconciliation.py`'s behavior (§5a) | **Done** |
| 2 | Fix `ext-apps/Reco/graphowl-pack` per §3.1 (dedupe against `gst:` predicates) | **Done** |
| 3 | Add `[[queries]]`/`[[findings]]` to Reco's pack — 3 of the 7 reachable `packs/gst` finding rules (§5a's revised count), against a canonical-invoice/Supplier graph shape `graphowl_client.rows_to_turtle` had to be extended to build (§5c) | **Done, narrower than planned** — 3 findings, not 7 or 8 |
| 4 | `/api/reconcile` calls `POST /packs/reco/reconcile` in **addition to** the existing `reconciliation.py` path — not replacing it yet | **Done** |
| 5 | **Parity tests**: fresh fixture data, live server, not inspection | **Done** — `ext-apps/Reco/scripts/verify-reconcile-parity.py`, passing |
| 6 | Only once (5) is green: make `reconciliation.py`'s output the *derived* one, or route `/api/reconcile` to read the engine directly | **Not done — see §5c. The parity result itself is why**: it's not full parity, and (6) as originally scoped would be a real regression, not a safe swap |
| 7 | Only after (6) has run for real without a regression: delete `reconciliation.py` | **Not done, and not recommended as originally scoped — see §5c** |

### 5c. What the live parity test actually found, and the cutover decision

Run against a genuinely fresh database (accumulated test data across
sessions silently corrupts this comparison — hit twice while building it;
see the script's own comment) with `ext-apps/Reco/SAMPLE/*_aug2026.csv`'s
hand-designed answer key: 7 clean matches, 1 amount mismatch, 2 books-only,
1 portal-only.

**`reconciliation.py`: exactly right** — `{total: 11, matched: 7, review: 1,
only_books: 2, only_gstr2b: 1}`. (This only became true after fixing a
second, unrelated pre-existing bug found *by* this parity test: `_auto_map`
never matched `igst`/`cgst`/`sgst` against a real GSTR-2B export's actual
column names — "Integrated Tax"/"Central Tax"/"State/UT Tax" — only the
literal substrings "igst"/"cgst"/"sgst". Every tax component silently
zeroed on the portal side against realistic headers, which both
`SAMPLE/gstr2b_mar2026.csv` and the fresh `gstr2b_aug2026.csv` use. Fixed
in `main.py`'s `_FIELD_KEYWORDS`, `tests/test_main.py` added — this repo
had zero tests for `main.py` before.)

**The native engine: 3 findings** — 2× `PotentialMismatch` (exactly the 2
`only_books` invoices), 1× `AmountMismatch` **and** 1× `TaxHeadMismatch`
for the same invoice (INV-AUG-103: taxable value differs 42000 vs 40000,
and — genuinely, checked against the raw CSV, not a bug — CGST/SGST differ
proportionally by ₹180, correctly tripping both rules independently).
Correct silence on all 7 matched invoices.

**One real, structural gap, not a bug**: nothing in the 3 wired findings —
or in any of `packs/gst`'s 13 — covers `only_gstr2b` ("in GSTR-2B, no
matching books entry") without GSTR-1 evidence. `MissingInBooks` is the
rule that names this case, and it requires `gst:Gstr1Invoice` in a
mandatory join (§5a). `reconciliation.py` catches it (INV-AUG-111,
correctly) by simple set difference; the native engine, as designed,
treats "declared but not booked" as a claim that needs the supplier's own
GSTR-1 filing as evidence, not an absence to infer from GSTR-2B alone.

**Decision: do not proceed to steps 6-7 as scoped.** Making the native
path primary and retiring `reconciliation.py` would delete `only_gstr2b`
coverage entirely — a real regression, not a safe cutover, and exactly
what the corrected process in this section exists to prevent. Two ways
forward, not decided here because both are product decisions:

- **Keep the dual-path permanently**, not as a stepping stone: native
  findings *enrich* 3 of 4 categories (statute citations, the
  tax-head-mismatch nuance, and — once §3.1's remaining gaps close —
  transposition/PAN-mismatch detection `reconciliation.py` has no
  equivalent for at all), while `reconciliation.py` remains the only
  source for `only_gstr2b`. This is a coherent end-state, not a
  compromise.
- **Or**: author a new finding rule (in Reco's own pack, not `packs/gst`
  — this case doesn't exist there because the platform's GST pack
  assumes GSTR-1 is the right evidence for it) that infers `only_gstr2b`
  from books+2B alone, the way `reconciliation.py` already does — then
  re-run this parity script. If it passes, steps 6-7 are worth
  reconsidering.

Either way, `reconciliation.py`'s presentation functions
(`supplier_health`, `ims_actions`, `match_stats` — §5a) were already
Reco-owned regardless of this decision; "retire `reconciliation.py`" was
never fully achievable as originally stated even in the best case — at
most its matching logic could eventually be retired, not the whole file.

This sequence is now the authoritative description of Slice 2 — it
supersedes the one-paragraph version in `plans/118-reco-now-integration.md`,
which should be read as pointing here.

4. **Optional, low-priority rename**: `connectors/python/graph_owl_packs` →
   split pack-infrastructure from connector modules by package, purely for
   the discoverability problem in §3.4. Not urgent — nothing is functionally
   wrong.
5. **Optional**: wire `connectors/python`'s and `integrations/langchain`'s
   test suites into CI or `scripts/gate.sh`, so "canonical" code is also
   "gated" code. Separate decision from the architecture question.

## 6. Repo-wide dead-code audit — `_archived/` candidates

Requested as a follow-on: not "does grep find an import" but a per-item
check against imports, registration/config/CLI/discovery, tests, CI, and
docs — classified ACTIVE / INDIRECT / HISTORICAL / DEAD / UNCERTAIN.
**Nothing has been moved.** This is a proposal.

**Method**: `crates/` and `ui/` audited by dedicated search passes (full
`cargo check --workspace --all-features --all-targets` for the former,
cross-referencing every low-hit-count file against `App.tsx`, the
`queues.ts`/`vocabularies.ts`/`packSurfaces.ts` config-registry patterns,
and two-hop parent components for the latter); `scripts/` audited file-by-file
against CI, `gate.sh`, and `plans/*.md`; `connectors/`, `examples/`,
`integrations/langchain/` audited directly in this session, extending the
per-file checks already done in §2-§4 above. The one substantial finding
(`rebuild_usage_rollups`) was independently re-verified with a fresh grep
before being included here, not taken on the sub-agent's word alone.

### 6.1 Proposed for `_archived/` (DEAD, evidence-backed)

| Item | Classification | Evidence checked | Notes |
|---|---|---|---|
| `Storage::rebuild_usage_rollups` (trait method, `graph-owl-storage/src/lib.rs:3925`) + its two adapter implementations (`graph-owl-storage-memory/src/lib.rs:4103`, `graph-owl-storage-postgres/src/lib.rs:7312`) + `Catalog::rebuild_usage_rollups` facade (`graph-owl-api/src/lib.rs:7159`) | **DEAD** | Independently re-verified: exactly 5 matches for `rebuild_usage_rollups` workspace-wide — the 4 definitions above plus the facade's own one-line internal call into the trait method. Zero external callers: not in `graph-owl-server`'s `main.rs`/`lib.rs`, no `.route(...)`, zero hits in any `tests/*.rs` or `#[cfg(test)]` block anywhere. The method's own doc comment claims justification as being needed for "Slice B's equivalence test" — searched for that test by name; it does not exist, in any `plans/*.md` or any `#[test]` in the tree | The doc comment reads like a real reason to keep it; checking what it actually points at found nothing. This is the audit's own cautionary pattern, inverted — a plausible-sounding justification that doesn't survive verification |
| `scripts/demo copy.sh` | **DEAD / HISTORICAL** | Zero references anywhere (CI, `gate.sh`, any `plans/*.md`, any other script). Diffed against `scripts/demo.sh`: an older, feature-incomplete version — missing `--gst`, OIDC auto-detection, and the agent-service startup step that `demo.sh` has | Almost certainly an accidental `cp`-with-space-in-filename left uncommitted-cleanup; superseded by `demo.sh` |
| `examples/adapter-csv/` (directory) | **DEAD, trivially** | Zero files (confirmed with `find`), not tracked by git (`git ls-files` returns nothing for it). `scripts/verify-examples.sh`'s "adapter-csv" phase does not read from this directory at all — it runs `sdk/python/tests/test_example_adapter_live.py`; the directory name is only an echo label | Empty and untracked — there is no content to lose either way. Worth a plain `rmdir` rather than an "archive" once confirmed, since nothing would be preserved by moving an empty directory |

### 6.2 UNCERTAIN — flagging, not proposing

| Item | Why uncertain |
|---|---|
| `scripts/check-llm.sh` | Zero references anywhere (CI, `gate.sh`, docs, other scripts) — but it's a coherent, self-contained diagnostic (curl-based smoke test against the same `LLM_PROVIDER`/`LLM_API_BASE_URL`/`LLM_MODEL` env vars `demo.sh`'s agent-service step and `integrations/langchain/agent_service` both use) with an obvious, legitimate manual-tool purpose. The other manual tools in `scripts/` all have an explicit "run this when X" line in `plans/*.md` or `CLAUDE.md`; this one doesn't. Plausibly just undocumented rather than dead |

### 6.3 Corrections surfaced along the way (not archival, worth fixing separately)

- `graph-owl-bolt` and `graph-owl-cli` were listed in this session's earlier
  understanding (carried from `CLAUDE.md`'s crate table) as intentional
  placeholder stubs. Both have grown into substantial, wired, tested code
  since — `graph-owl-bolt` (3,725 lines, feature-gated Bolt/PackStream
  listener, wired in `graph-owl-server`'s `main.rs`) and `graph-owl-cli`
  (3,027 src + 1,522 test lines, real `clap` subcommands, invoked in CI).
  Their own `lib.rs` doc comments still say `"Status: placeholder"`, which is
  now false and worth fixing — not an archive question, a stale-comment one.
  Only `graph-owl-search-hnsw` and `graph-owl-search-opensearch` are still
  true stubs.
- `AppError::Forbidden` in `graph-owl-server` carries a stale
  `#[allow(dead_code)]` and a comment saying it "will be constructed by the
  authorization middleware" — it already is, twice (`follow_asset`/
  `unfollow_asset`). The suppression is the only dead thing here; delete the
  attribute, not the variant.

### 6.4 Everything else checked and found clean

`ui/` (140 files swept): zero DEAD. Every low-hit-count file resolved to a
real consumer through a barrel export, a config-registry pattern
(`queues.ts`, `vocabularies.ts`, `packSurfaces.ts`), or a two-hop parent
component. `connectors/python/graph_owl_packs/` (all 6 files): all ACTIVE,
consistent with §2-§4 above. `integrations/langchain/`: `agent_service`'s
`files.py`/`providers.py`/`streaming.py` are imported via
`from agent_service.X import ...` (absolute-package style, not `from .X
import`, which is why an initial narrower check missed them) — all ACTIVE.
19 of 21 `scripts/` files ACTIVE (7 direct-CI, 2 indirect-via-CI, 2 via
`gate.sh`, 9 documented manual tools). `plans/` (130 files) was
**deliberately not classified individually**, per explicit instruction — the
project's own convention (`CLAUDE.md`: "Completed, kept as historical
record — do not delete", covering `90-done-table-entity.md` and
`91-done-relationships.md`) already handles this, and nothing found
suggests a plan outside that convention needs the same treatment.

No files were moved or deleted to produce this audit.
