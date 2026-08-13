# Plan 108 — Books ↔ GSTR-1 ↔ GSTR-2B: a three-way reconciliation graph, with period-aware carry-forward and receipt timing

**Status**: **Slices 1–5, 7 and 8 shipped 13 August 2026.** Slice 6
(period-aware carry-forward) remains blocked on `107-filing-period.md`
Slice 1 (`gst:FilingPeriod`), still unbuilt. **Branch**: main.

**What shipped, and where it went past this plan.** Slices 1–5 and 8 landed
as written — `gst:Gstr1Invoice`, `gst:filedDate`, `gst:GoodsReceipt`,
`gst:Section16-2-b`, five new `[[findings]]` and their queries, all pure
pack content. Slice 7 was written here as "one line per label in
`[console.queues]`" and was deliberately widened, because that was not the
real gap: the Review queue is a reviewer's tool and closing a period needs a
different page. What shipped is a **Reconciliation route** — three source
uploads, a run button, a statement (books total, GSTR-2B total, the
difference, what the findings explain and what they do not), and the
exceptions grouped by next action. The queue labels were added too.

**Three things this plan did not anticipate, all found by running it:**

- **A purchase-register importer was the actual blocker.** Every rule here
  compares the taxpayer's books against something, and the books could only
  arrive as a Turtle fixture inside the pack — so a CA uploading a real
  GSTR-2B was reconciling it against demo data. `books.ts` (CSV/TSV, alias
  column matching) and `gstr1.ts` (GSTR-2A JSON) shipped alongside.
- **`SupplierNotFiled` and `MissingInBooks` shipped with a false-accusation
  bug**, caught only against real data: a transposed GSTIN breaks the exact
  join in both directions, so one typo produced three findings and two of
  them were wrong — including "the supplier has not reported this invoice"
  about a supplier who had. Fixed with a guard both queries now document.
- **Slash-bearing invoice numbers (`RST/2026/0455`) produced invalid
  Turtle** in all three importers, including the GSTR-2B one that predates
  this plan. Now percent-encoded. Nothing in this document predicted it and
  it would have failed on the first real upload.

**Depends on**: nothing, for the slices that shipped.

**Trigger**: three rounds of the same question, in this session. First: "how
would the graph be used for a single ₹350 invoice whose supplier files
late." Second, a broader worked model: four distinct reconciliation
questions (supplier hasn't filed at all; supplier filed GSTR-1 but it
didn't reach GSTR-2B; the taxpayer's own books disagree with what the
supplier filed; the supplier's records show an invoice the taxpayer never
booked), each wanting its own graph-native finding rather than one generic
"unmatched" flag — and a correction to the first round: don't build "in 2A
but not in 2B → don't claim" as the rule; **GSTR-2A/GSTR-1 are evidence
sources, GSTR-2B is the only thing that actually gates ITC eligibility for
a period, and the graph's job is to explain *why* an invoice is or isn't in
a given period's 2B**, not to re-derive the claim/don't-claim decision from
2A directly. That correction is why this plan does not add a
`Gstr2aInvoice` class — GSTR-1 (what the supplier declares) already carries
the same evidentiary role 2A would. Third, a fifth case genuinely different
from the first four: **an invoice can be present in the right period's
GSTR-2B and still not be eligible yet, because Section 16(2) requires the
recipient to have *received the goods or services*, and that can happen in
a later period than the invoice/2B date.** Unlike the first four cases,
which are all "does a document contain this invoice," this one is "did an
*event* happen yet" — a structurally different kind of reasoning this plan
tracks separately (Slice 8) rather than folding into the carry-forward
finding. See "Vocabulary decisions" below.

**Cross-checked against practitioner reconciliation material already on
disk in this project's reference planning docs, not only against the
worked examples typed into this session.** All of it converges on the
same scenarios above — a genuine confirmation the split is right, worth
saying rather than hiding (this project's own convergent-design rule,
`00i-licensing.md` rule 6). It also surfaced three things this plan hadn't
covered, folded in below rather than left as a silent gap: reverse-charge/
import-of-service/ISD credits are eligible without ever needing to appear
in GSTR-1/2A/2B at all (Warnings); debit and credit notes are a real
document type this plan does not model (Parking Lot); and a rarer, reverse-
direction anomaly — 2B containing an invoice that hasn't yet reached 2A —
that the normal "2B lags 2A" mental model doesn't predict (Parking Lot).

## What already exists (so this is additive, not foundational)

Verified against the repo, not against what the platform doc originally
proposed:

- **The whole "rule as an inspectable object" idea is already shipped**,
  just not as reified RDF. A finding rule in `packs/gst/pack.toml` is a
  `[[findings]]` entry: a `label`, a `summary`, a `governed_by` citation
  (`gst:Section16-2-aa`, a real subject in `law/sections.ttl` a reviewer can
  traverse to and read), a `query` naming a first-class `.sparql` file (not
  hidden in Rust), and an `evidence` array mapping every bound variable to
  the exact predicate it came from. Six of these exist today —
  `PotentialMismatch`, `AmountMismatch`, `ITCNotAvailable`, `Reversed`,
  `GstinTransposition`, `PaymentOverdue` — and the Python loader
  (`connectors/python/graph_owl_packs/loader.py`) is generic over all of
  it: it reads `[[findings]]`/`[[queries]]`/`[[documents]]`/`[[predicates]]`
  and POSTs them to the server with zero knowledge of GST. **Adding new
  findings, new fixtures, or a new class needs no Rust or Python loader
  change** — the entire mechanism is pack content.
- **`gst:Supplier` is a real, traversable subject**, not a bare literal —
  `gst:issuedBy` (`Invoice → Supplier`) is a genuine edge, so "Invoice →
  Supplier" (the first hop in every diagram both rounds of this question
  drew) already works (`plans/105c-gst-causal-graph.md`).
- **Evidence-chain walking is shipped and generic.** `GET
  /findings/{id}/evidence-graph` (105e) resolves a finding's subject and
  walks `TraversalEngine::subgraph` from it; `Catalog::node_sources` (105g
  Slice 1) labels each node with which imported document asserted it
  (`gst-purchase-register` vs `gst-gstr2b`); `Catalog::near_miss_node` (105g
  Slice 2) surfaces a value-matched candidate when no edge exists yet. This
  is the actual mechanism behind "the graph investigates the chain" from
  both rounds of the request — it needs a GSTR-1 source and (for Slice 6) a
  `FilingPeriod` node to walk *through*, not new machinery.
- **Every import lands in its own named graph**, `graph:import:{source}`
  (`00c-domain-model.md`), never the default graph — this is what makes
  Books, GSTR-1 and GSTR-2B three separable evidence sources in the first
  place rather than one undifferentiated pool of facts. Every existing
  query is written `GRAPH ?register { … }` / `GRAPH ?authority { … }` for
  exactly this reason.
- **Entity resolution across typos is a config concern, not a query
  concern** — `[[matching.blocking]]` in `pack.toml` already declares
  `normalized` (GSTIN + invoice number), `composite` (GSTIN + a 7-day date
  window) and `ngram` (GSTIN transposition) strategies. A new GSTR-1 source
  reuses these without writing new matching code.
- **The console queue is pure config.** `[console.queues]`'s `labels` array
  is the only thing that decides which findings the "GST reconciliation"
  queue shows; the renderer (`findingsQueue.tsx`) is generic per pack. A new
  finding label needs one line added there, not a new component.
- **The agent already exposes all of this generically.** `run_rule`,
  `reconcile`, `explain`, `find_evidence` (P10, shipped) work against
  *any* registered finding rule and *any* subject's evidence graph — a new
  finding registered via `pack.toml` is immediately answerable by the agent
  with no agent-side code change. This is the literal mechanism behind "the
  agent asks the graph, the LLM explains it" from the second round of the
  request.
- **Event-as-subject is already a proven pattern, not a new idea for Slice
  8.** `gst:PurchaseEvent` and `gst:PaymentEvent` already exist in
  `ontology.ttl`, each anchored to an invoice via `gst:onInvoice` and to a
  timestamp via `gst:atTime` — exactly the shape `PaymentOverdue`'s
  `[findings.span]` band already reasons over (a span between two events,
  not two dates flattened onto one row). Goods receipt is a third event of
  the same shape, not a new modelling problem.

## What's missing — mapped to the five scenarios

| # | Scenario (as described) | Exists today? | Gap |
|---|---|---|---|
| 1 | Books has it, supplier hasn't filed GSTR-1 at all | No — GSTR-1 doesn't exist as a source | New class, fixtures, import surface, finding |
| 2 | GSTR-1 has it, GSTR-2B doesn't (filed late / mapping / wrong GSTIN / B2C misclassification) | Partially — `PotentialMismatch` today conflates this with #1, because there is no GSTR-1 layer to tell them apart | Splits into two real findings once GSTR-1 exists (see Slices 2–3) |
| 2b | …and it specifically appears in a **later period's** 2B (the ₹350 July→August case) | No — `gst:period` is a scalar literal per fact, nothing traverses "the next period this appears in" | Needs `FilingPeriod` (Plan 107) + one new query (Slice 6) |
| 3 | Books and GSTR-1 disagree on amount/number/date | Partially — `AmountMismatch` compares Books vs **2B**, not vs GSTR-1 | New query, same shape, different named graph |
| 4 | GSTR-1 (+2B) has it, Books doesn't | No — no query runs in this direction at all | New query, mirrored `OPTIONAL`+`!BOUND` |
| 5 | 2B has it in period P, but goods/services were received **after** P (Section 16(2)(b)) | No — no `GoodsReceipt` event, no query comparing 2B period to receipt date | New event class (reuses `onInvoice`/`atTime`), new query, new citation (Slice 8) |
| — | Fully matched, no finding | Already the default (silence is the signal — no finding fires) | Nothing to build; flagged in Parking Lot as a UX question, not a data gap |

**A concrete bug found while tracing scenario 2b, not a hypothetical.**
`ui/src/features/packs/gstr2b.ts:134-136` derives `gst:period` for every
imported invoice from **the invoice's own date** (`invoiceDate.slice(0,
7)`), with the comment "deriving it from the clock would silently change
what a re-import of the same period means." That reasoning is right about
the clock, but the derivation is still wrong for what `period` needs to
mean here: **GSTR-2B's period is the return period the snapshot document
itself covers, which is exactly the field that differs from the invoice
date in every carry-forward case** — an invoice dated 7 July that surfaces
in August's 2B must carry `period = "2026-08"`, and today's code would tag
it `"2026-07"` regardless of which month's file was actually uploaded. This
makes the exact scenario this plan exists for silently untestable through
the real upload path today. `connectors/python/graph_owl_packs/gstr2b.py`
carries the same bug — the file's own comment says the two are pinned to
identical fixture assertions, so both need the identical fix in the same
slice or the pinned test breaks in one language and not the other.

## How the graph is utilized (the direct answer)

Restating both rounds of the request in graph-owl's actual vocabulary,
not the illustrative diagrams:

**The graph is three named subgraphs joined by shared subjects, not three
tables joined by a batch job.** Books (`graph:import:gst-purchase-register`),
GSTR-1 (`graph:import:gst-gstr1`, new) and GSTR-2B
(`graph:import:gst-gstr2b-{period}`, corrected to be period-scoped — see
above) are three independent named graphs. "The same invoice" across all
three is not assumed — it is a matching decision `[[matching.blocking]]`
already makes robust against a transposed GSTIN or a same-day-vs-week-later
posting, exactly so a real invoice number typo doesn't silently produce a
false "missing" finding.

**A rule is a query plus a citation, and that already gives you what
"the rule becomes a graph object" was asking for** — every finding names
the statute subject it's judged under (`governed_by`), so a reviewer
traverses from the finding to `gst:Section16-2-aa` and reads why the
finding exists, and the SPARQL itself is a first-class, versioned file, not
logic hidden inside Rust. What this plan **deliberately does not do** is go
further and reify the rule itself as RDF triples (`gst:ITCCarryForward
gst:evaluates gst:PurchaseInvoice-1001`, etc.) — that would be a second
representation of the same rule that the SPARQL text already is, with
nothing today that needs to *query the rules themselves* (e.g., "which
rules cite Section 16") to justify keeping the two in sync. If that need
shows up later, it's a small addition on top of the existing `governed_by`
edges, not a redesign.

**The evidence chain the user hand-drew is `GET
/findings/{id}/evidence-graph`'s existing output, with two more hops once
GSTR-1 and `FilingPeriod` exist.** Today it walks `Invoice → Supplier` and
`Finding → Section` for any of the six existing findings, with each node
labelled by which source asserted it. Slices 1–4 add a GSTR-1 node this
walk passes through; Slice 6 adds a `FilingPeriod` node each 2B-derived
fact belongs to. No new traversal code — the traversal is already generic
over whatever edges the fixtures happen to instantiate.

**Findings are computed, not asserted.** Nothing writes "matched" or
"ITCCarryForward" as a permanent flake on the invoice; every finding is the
live result of running its SPARQL against the current graph. Re-running
reconciliation after a correction lands (say, the supplier refiles GSTR-1
with the right GSTIN) simply stops producing the finding — there's nothing
to retract. This is why silence-is-the-signal (no finding) already serves
as "matched," and adding a positive "Matched" record would be new state
that has to be kept consistent with the graph rather than read live from
it (see Parking Lot).

**Slices 1–5 and 7 are document reconciliation; Slice 8 is event
reasoning, and the difference matters for how each is queried.** Every
finding through Slice 7 answers "does a document (Books/GSTR-1/GSTR-2B)
contain this invoice" — a presence/absence join across named graphs, the
same shape all six shipped findings already use. Slice 8 answers "did
something happen yet" — it doesn't matter whether GSTR-2B *contains* the
invoice, only whether the `gst:GoodsReceipt` event's `atTime` is later than
the period the invoice was claimed eligible in. Conflating the two would
be a mistake: an invoice can pass every document check (it's in Books, in
GSTR-1, in the right period's 2B) and still not be eligible, because
Section 16(2) needs *all* of its conditions satisfied, and "the documents
agree" is only one of them. This is also why Slice 8 does **not** depend on
`FilingPeriod` (Plan 107) the way Slice 6 does — it only needs to compare
two dates it already has (the 2B document's own `period`, and the receipt
event's `atTime`), not traverse a chain of periods looking for the first
one where something appears.

## Vocabulary decisions

New ontology content, all pure pack config (`packs/gst/ontology.ttl`,
`packs/gst/pack.toml`), zero engine changes:

- `gst:Gstr1Invoice rdf:type gst:Class` — what the supplier declared, in
  their own filing. Reuses every existing invoice predicate
  (`supplierGstin`, `invoiceNumber`, `invoiceDate`, `taxableValue`,
  `taxAmount`, `period`, `issuedBy`) with **one new predicate**:
- `gst:filedDate rdf:type gst:Property` — when the supplier actually
  submitted the GSTR-1/IFF, distinct from `invoiceDate`. This is the field
  that makes "reported after the cutoff" an explainable fact rather than an
  inferred one: an invoice dated 7 July with `filedDate = "2026-08-20"`
  is why it can't be in July's 2B and can be in August's, without the graph
  needing to know what a "cutoff" is at all — the 2B document for a given
  period either contains the invoice or it doesn't, and that document *is*
  the authority's own answer to whether the filing landed in time.
- **No `gst:Gstr2aInvoice` class.** Per the trigger's own correction, 2A is
  a revolving view over the same supplier-declared data GSTR-1 already is;
  modelling both would give the graph two sources for one fact with no
  query that needs the distinction. If a future requirement specifically
  needs 2A's "as of right now, unfrozen" semantics as opposed to GSTR-1's
  "as filed," it is a `dryRun`-style query timestamp on the same class, not
  a second class — worth writing down here so it isn't silently
  re-proposed.
- Two more predicates needed for the new-direction queries, both already
  informally implied by existing predicates and worth confirming
  exist/don't during Slice 1's RED test rather than assumed here:
  `gst:invoiceDate` and `gst:taxableValue` are shared as-is; no other new
  scalar predicates are anticipated beyond `filedDate`.
- `gst:GoodsReceipt rdf:type gst:Class` (Slice 8) — **needs zero new
  predicates.** It reuses `gst:onInvoice` (edge back to the invoice) and
  `gst:atTime` (the receipt date) exactly the way `gst:PurchaseEvent` and
  `gst:PaymentEvent` already do; the loader, the event pattern and the
  span-comparison idiom are all already proven by `PaymentOverdue`. This is
  the cheapest addition in the whole plan.
- **`gst:Section16-2-b`, a genuinely new law subject** — the one citation
  in this plan that isn't a reuse of something `law/sections.ttl` already
  has. Every other new finding cites an existing section; this is the
  first case that needs a new one added, following the same spec-first
  sourcing (the published Section 16(2)(b) text, not a paraphrase from
  memory) every existing entry in that file already used. Write it before
  Slice 8's `[[findings]]` entry, not as part of it, so the citation itself
  gets the same scrutiny the query does.

## Parent

**Actor**: a CA (or the investigation agent acting for one) reviewing a
business's ITC position.
**Need**: for any invoice, one traceable finding that says *why* something
is wrong (or that nothing is), across Books, GSTR-1 and GSTR-2B — not a
bare "unmatched," and not four separately-run tools whose results have to
be reconciled by hand.
**Outcome**: four new named findings, each citing its statute, each
carrying an evidence chain a reviewer can walk into the graph and verify —
matching the shape the six existing findings already have.
**Current constraint**: GSTR-1 does not exist as a graph source at all, so
today's single `PotentialMismatch` finding cannot distinguish "supplier
never filed anything" from "supplier filed, but it never reached my 2B" —
two very different next actions for the CA.

## Split candidates

| Slice | Value | Includes | Defers | Acceptance examples | Release constraint |
|---|---|---|---|---|---|
| **1. GSTR-1 exists as a source** | Unblocks every scenario below; proves the class is worth having | `gst:Gstr1Invoice` + `gst:filedDate` in `ontology.ttl`; `packs/gst/fixtures/gstr1.ttl` (mirrors `gstr2b.ttl`'s shape); client-side import surface mirroring `gstr2b.ts` (field names verified against the same published GSP/IFF reference `gstr2b.ts` already cites, not guessed); the `period`-derivation fix in both `gstr2b.ts` and `gstr2b.py` (return period from the document's own envelope, not `invoiceDate.slice(0,7)`) | Any new finding rule; console UI beyond the existing generic upload panel | Uploading a GSTR-1 JSON with an invoice dated 7 July lands a `gst:Gstr1Invoice` in `graph:import:gst-gstr1` with `period` reflecting the *return* period, not the invoice date; re-running the pinned Python/TS fixture assertions still agree | Ships behind nothing — additive pack content |
| **2. `SupplierNotFiled` finding** | Answers "has the supplier filed anything at all" | `missing-in-gstr1.sparql` (same `OPTIONAL`+`!BOUND` idiom as `missing-in-gstr2b.sparql`, `?register` vs new `?gstr1` graph, **excluding reverse-charge invoices** — see Warnings); `[[findings]]` entry, citing `gst:Section16-2-aa` (same section as `PotentialMismatch` — verify against `law/sections.ttl` before reuse rather than adding a new statute subject on assumption) | Distinguishing *why* the supplier hasn't filed (late vs never); import-of-service and ISD credit, which this pack does not model as invoices at all yet — out of scope, not silently mishandled | Books has INV-101, no `Gstr1Invoice` exists for GSTIN+number in any graph → finding fires; a fixture with a matching `Gstr1Invoice` present does not; a fixture with `gst:reverseCharge = "Y"` and no `Gstr1Invoice` does not fire | Same |
| **3. `Gstr1NotIn2b` finding, and narrow `PotentialMismatch`'s population** | Splits today's one conflated finding into two, each with a distinct next action | `gstr1-not-in-2b.sparql`; new `[[findings]]` entry citing `gst:Section16-2-aa`; **`missing-in-gstr2b.sparql` gains an `OPTIONAL`+`!BOUND` guard excluding invoices that *do* have a `Gstr1Invoice`**, so an invoice doesn't fire both findings at once | Slice 6's period-aware variant (this finding stays "never reached 2B in *any* period we hold," which needs no `FilingPeriod`) | Given GSTR-1 has INV-102, no 2B (in any uploaded period) has it → `Gstr1NotIn2b` fires and `PotentialMismatch` does not; given no GSTR-1 record exists for an invoice at all → `PotentialMismatch` fires (unchanged) | Same — but this slice edits an existing shipped query; re-run its existing fixture assertions, not only the new one |
| **4. `MissingInBooks` finding** | The reverse direction — GST records show an invoice the taxpayer never booked | `missing-in-books.sparql`, same idiom mirrored (`?gstr1`/`?authority` present, `?register` absent); `[[findings]]` entry — citation needs checking against `law/sections.ttl`, likely a new, narrower `gst:Section16` framing since this isn't an ITC-eligibility question, it's a bookkeeping-completeness one | Any auto-remediation ("add to books") — this is a surfaced question, not a write | Given `Gstr1Invoice` INV-103 exists and `PurchaseInvoice` INV-103 does not → finding fires | Same |
| **5. `BooksGstr1Mismatch` finding** | Answers "did I book what the supplier declared, correctly" | `books-gstr1-mismatch.sparql`, adapted from `amount-mismatch.sparql`'s shape but against `?gstr1` instead of `?authority`; scoped to `taxableValue` only for the first cut, matching `AmountMismatch`'s own existing scope discipline (invoice-number/date/tax-head diffing is a later, separate slice, not bundled here) | Multi-field diff (number, date, tax head) — explicitly deferred, not silently dropped | Given Books claims ₹530 and GSTR-1 declares ₹350 for the same invoice → finding fires with both values as evidence | Same |
| **6. Period-aware `ITCCarryForward` finding** *(depends on Plan 107 Slice 1)* | The original ₹350 July→August example, answerable for real | Requires `gst:FilingPeriod` + `belongsToPeriod` (107 Slice 1) to exist first; each `graph:import:gst-gstr2b-{period}` document links its invoices to that period's `FilingPeriod` subject at import time; one new registered query finding, for an invoice present in GSTR-1 and absent from period P's 2B, the **earliest later period** whose 2B does contain it | Any period before the first one with data; multi-invoice batch summaries (console concern, not a query concern) | Given INV-1001 (₹350, dated 7 July) has no `2026-07` 2B record and a `2026-08` 2B record → finding reports `currentPeriod: "2026-07"`, `eligiblePeriod: "2026-08"`; given no later period contains it yet → the finding does not fire (nothing to carry forward *to* yet is a different, undecided case — see Parking Lot) | Ships only once Plan 107 Slice 1 ships; do not start before that predicate-naming decision is resolved |
| **7. Console wiring** | Makes 1–6 visible without a scroll through raw JSON | One line per new finding label added to `[console.queues].labels` in `pack.toml` — the queue renderer is already generic | A period picker (that's Plan 107 Slice 4's job, not this plan's) | New finding labels appear in the existing "GST reconciliation" queue, verified live (`agent-browser`), matching every other console slice's own convention | Ships behind nothing |
| **8. `GoodsReceiptTiming` finding** *(independent of Plan 107 — can ship in parallel with 2–5)* | Catches the case every document check passes and the invoice is still not eligible, because the goods hadn't arrived yet | `gst:GoodsReceipt` class (Slice content only, see Vocabulary decisions); `gst:Section16-2-b` added to `law/sections.ttl`; `packs/gst/fixtures/goods-receipt.ttl`; `goods-receipt-timing.sparql` comparing a matched invoice's 2B `period` against its `GoodsReceipt`'s `atTime`; `[[findings]]` entry citing `gst:Section16-2-b` | A `GoodsReceipt` event that's simply absent from the graph entirely (not "late," just never recorded) — flagged in Parking Lot, not folded in here | Given INV-004 (₹20,000, invoice 30 Aug, in August's 2B) has a `GoodsReceipt` with `atTime = "2026-09-04"` → finding fires, citing `gst:Section16-2-b`, evidence includes both dates; given a `GoodsReceipt` in the same month as the 2B period → no finding | Ships independently of Slice 6 |

## Recommended finding copy (final, in this pack's existing terse third-person tone)

The product-facing wording surfaced during this planning round was good and
concrete; adapted here to match the six shipped findings' existing register
(`packs/gst/pack.toml`'s own summaries are dry and third-person, not
conversational) so a reviewer can't tell a new finding from an old one by
tone alone:

| Label | `summary` | `governed_by` |
|---|---|---|
| `gst:SupplierNotFiled` | "An invoice claimed in the purchase register that the supplier has not reported in any GSTR-1/IFF filing" | `gst:Section16-2-aa` (confirm, don't assume — see Parking Lot) |
| `gst:Gstr1NotIn2b` | "The supplier declared the invoice in GSTR-1/IFF, and it has not reached any GSTR-2B this pack holds" | `gst:Section16-2-aa` (confirm) |
| `gst:MissingInBooks` | "The supplier's declared invoice appears in GSTR-1/IFF or GSTR-2B with no matching entry in the purchase register" | `gst:Section16` (confirm — this is a completeness question, not an eligibility one; may warrant its own citation rather than the general section) |
| `gst:BooksGstr1Mismatch` | "The purchase register and the supplier's GSTR-1/IFF declare the same invoice with different values" | `gst:Rule36-4` (reuse — the same cap-tolerance logic `AmountMismatch` already applies, just against a different named graph) |
| `gst:ITCCarryForward` | "The invoice is absent from this period's GSTR-2B and present in a later period's — eligibility is carried forward" | `gst:Section16-2-aa` (confirm) |
| `gst:GoodsReceiptTiming` | "The invoice is matched and its GSTR-2B period contains it, but the goods or services were received in a later period" | `gst:Section16-2-b` (new — see Vocabulary decisions) |

Every "(confirm)" above is provisional pending the Parking Lot citation
check — not a licence to write the `[[findings]]` entry with these
citations unverified.

## Parking Lot

- **Does a fully-matched invoice deserve a positive record, or does silence
  stay the signal?** The trigger's own diagram lists "Books + GSTR-1 + 2B
  all match → Matched" as a fifth row alongside the four findings. Every
  existing finding in this pack is negative-only (something to look at);
  adding a positive "Matched" would be new persisted state to keep
  consistent with a graph that otherwise computes everything live, and
  changes what "no finding" has always meant in this pack. Load `grill-me`
  on this specifically before Slice 2 — it changes the shape of every
  finding table added here if the answer is "yes, add it."
- **Slice 6's "no later period contains it yet" case.** An invoice absent
  from every period uploaded so far, including the most recent — is that
  silence (nothing to report until a later period exists), or a distinct
  "still pending, watch this" finding with no eligible period bound yet? A
  real design question, not resolved here — resolve alongside the point
  above, since both are "when does absence deserve its own finding."
- **Whether `filedDate` on GSTR-1 needs an equivalent "as reported vs as
  amended" distinction.** GSTR-1 can be revised after initial filing; this
  plan models one `filedDate` per invoice and does not attempt filing
  history. Worth a note, not a blocker — flag if a fixture ever needs it.
- **Citation for `Gstr1NotIn2b` and `MissingInBooks`.** Both are provisionally
  assigned `gst:Section16-2-aa` and `gst:Section16` respectively above as a
  starting point, not a verified legal conclusion — confirm against
  `packs/gst/law/sections.ttl` and the published statute (licensing rule 4,
  `00i`) before writing either `[[findings]]` entry; do not invent a new
  `Section`/`Rule` subject without the same spec-first sourcing every
  existing citation in this pack already used.
- **Whether GSTR-1 needs the same near-miss/similarity band
  `GstinTransposition` has**, for a GSTR-1 filed under a near-identical
  GSTIN. Plausible, not scoped here — a later slice if it turns out to
  matter once real GSTR-1 fixtures exist.
- **Debit notes and credit notes are not modeled anywhere in this plan.**
  Real reconciliation nets them against the base invoice at each stage
  (books, 2A, 2B all separately), and the existing pack's own
  `AmountMismatch` cap logic already reads as if invoices are the only
  document type. A genuine gap, not a subtle one — worth its own slice
  once the six-plus findings here are stable, not folded in now, since it
  touches every existing query's join shape, not just the new ones.
- **A reverse-direction data anomaly**: an invoice present in a period's
  GSTR-2B that has *not yet* reached GSTR-1/2A. The normal mental model
  (2B is generated *from* GSTR-1/2A data, so it should always lag or equal
  it, never lead it) doesn't predict this, and it points at a genuine data
  inconsistency in the imported source rather than an ITC-eligibility
  question. Not worth a dedicated finding on the evidence gathered so far
  — flagged so it isn't mistaken for a bug in this pack's own queries if a
  real GSTR-1/2B pair ever produces it.
- **Annual-level reconciliation (GSTR-9's own ITC comparison table) is a
  natural generalization of Slice 6's carry-forward logic across a full
  financial year rather than month-to-month**, and explicitly out of scope
  here — this plan stays month-scoped throughout. Worth a pointer for
  whoever picks up annual-return support later, not a reason to widen this
  plan now.
- **Slice 6 and Slice 8 can disagree about which period an invoice is
  eligible in, and nothing today reconciles that.** An invoice could be
  absent from July's 2B and present in August's (Slice 6 says "claim in
  August") while its `GoodsReceipt` lands in September (Slice 8 says
  "claim in September") — both findings would fire on the same invoice,
  each naming a different period, and a CA needs one answer, not two. The
  legally correct rule is that eligibility follows the *latest* of every
  satisfied Section 16(2) condition — which argues for eventually unifying
  Slices 6 and 8 into one "true eligible period = max(2B-availability
  period, receipt period, …)" computation rather than two independently
  firing findings. Not attempted in this plan: per `105g`'s own precedent
  (it rejected a generic gap-interpreter after checking it against the six
  real findings and finding five of six didn't need it), build the two
  narrow findings first and only unify them if real fixtures show the
  conflict actually happens, not on the suspicion that it might.
- **A `GoodsReceipt` that's absent from the graph entirely** (not "later,"
  just never recorded) is arguably the more important case Section
  16(2)(b) implies — ITC isn't eligible *at all* until goods are received,
  regardless of what 2B says. Structurally this is a fourth "presence in
  one source, absence in another" finding (2B has it, no `GoodsReceipt`
  exists), unlike Slice 8's "both exist, dates disagree" — closer in shape
  to Slices 2–4 than to Slice 8. Worth its own slice once Slice 8 ships and
  real data shows how often it's the absence, not the lateness, that
  matters.

## Warnings

- **`OPTIONAL` + `!BOUND`, never `FILTER NOT EXISTS`**, for every new
  absence-detecting query — the existing two queries document exactly why
  (`NOT EXISTS` is invisible to this engine's pushdown planner and silently
  reports everything as missing). Copy the idiom, don't reinvent it.
- **Every new query pattern must sit inside its own `GRAPH ?var { }`
  block.** A pattern outside one silently matches nothing against real
  imported data — this bit `missing-in-gstr2b.sparql` once already and is
  written up in that file's own comments for exactly this reason.
- **Amounts compare as strings/decimals, never as parsed floats** —
  `money()` in `gstr2b.ts` fixes every figure to two decimals for this
  reason; the GSTR-1 importer needs the identical normalizer, not a
  rewritten one that could drift from it.
- **Dates compare lexicographically as ISO-8601** — `isoDate()`'s existing
  day-first-to-ISO conversion and its refusal to guess an unplaceable date
  apply identically to `filedDate`; a wrong ordering here doesn't just
  break a display, it makes Slice 6's "earliest later period" computation
  silently pick the wrong period.
- **Do not let Slice 3's narrowing of `PotentialMismatch` regress its
  existing fixture assertions.** It is a shipped, tested finding; editing
  its query changes production behaviour for every invoice already flagged
  by it, not just new GSTR-1 cases — re-run its existing tests, don't only
  add new ones.
- **The `period`-derivation fix in Slice 1 touches a shipped, pinned
  cross-language fixture pair.** Fix `gstr2b.ts` and `gstr2b.py` in the
  same slice, and expect the shared fixture assertions both are pinned
  against to need updating in lockstep — an update to one without the
  other is exactly the drift the file's own comment warns about.
- **Slice 8's query needs the same period-scoping discipline as everything
  else, even though it doesn't need `FilingPeriod`.** It still must
  compare "2B's own period" against "the receipt event's date sliced to a
  period" using the *corrected* period derivation from Slice 1 — building
  Slice 8 against the pre-fix `gstr2b.ts` behaviour would silently launder
  the same bug this plan opens by finding.
- **Slices 2 and 3's absence queries must exclude reverse-charge
  invoices**, or they will fire `SupplierNotFiled`/`Gstr1NotIn2b` on every
  RCM (and, once modeled, import-of-service/ISD) invoice — a whole
  category of ITC that is legitimately claimable without a matching
  supplier-side GSTR-1 line, because the recipient self-assesses rather
  than waiting on the supplier to file. The existing `gst:reverseCharge`
  predicate already carries this signal (`gst:Reversed` already reads it);
  add the same `FILTER` to `missing-in-gstr1.sparql` and
  `gstr1-not-in-2b.sparql` rather than discovering the false-positive rate
  against real data.

## Next Step

Load `grill-me` on the Parking Lot design questions before Slice 1's RED
test — the same gate `107-filing-period.md` already puts on its own
parking lot, for the same reason: these are fuzzy calls this document
deliberately leaves open rather than deciding silently while writing an
ontology file. Three are load-bearing enough to resolve before writing any
code: positive "Matched" record vs. silence; absence-with-no-later-period-
yet (Slice 6); and whether Slice 6/Slice 8's eligible-period conflict needs
resolving now or can wait for real fixtures to show it actually happens.
Then load `planning` to turn Slice 1 (GSTR-1 as a source, including the
`period`-derivation fix) into a PR-sized implementation plan with TDD
execution steps. Slices 2–5 and 8 do not depend on Plan 107 and can each
become their own PR-sized plan once Slice 1 ships; Slice 6 stays blocked on
`107-filing-period.md` Slice 1 regardless of how far the others progress.
