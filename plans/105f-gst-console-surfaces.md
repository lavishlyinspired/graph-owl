# Plan: GST console surfaces — what a business user needs beyond the review queue (scoping only)

**Status**: Scoping document, 11 August 2026. No code. Written in response to "from the business users perspective, what else should be there in the UI for the GST use case."

**Companion to**: `00f-ui-architecture.md` (stack, budgets, non-negotiables) · `00h-ui-design-system.md` (patterns, screen inventory) · `105a` (provider reality) · `105b` (native reconcile engine, shipped) · `105c` (causal graph, Slice 1 shipped) · `105e` (evidence-chain walk, in flight) · `packs/gst/pack.toml` · `packs/gst/eval/questions.md`.

## What a business user has today

The GST surface currently supports: upload the two inputs (purchase register, GSTR-2B), click **Run reconciliation**, and work a findings queue — accept / dismiss-with-reason, side-by-side evidence, citation rendered first. That is a working **review tool**. A Chartered Accountant or tax team needs a working **monthly workflow**, and the gap between the two is the whole content of this document.

The persona throughout is a CA / finance professional who runs this every filing period: downloads GSTR-2B, exports the purchase register, reconciles, follows up with suppliers, takes ITC, and keeps an audit trail for years.

## Domain-neutrality discipline (stated up front)

Everything below must land as **pack-configured surfaces over the existing patterns**, not console code that knows what GST is. `plans/105-domain-neutrality.md`'s boundary already states this: domain entities are graph subjects described by the pack's own ontology. The five patterns in `00h` (entity page, graph surface, vocabulary browser, review queue, schema-driven form) and the 30-route budget are the envelope; a "GST dashboard" that is not also a "hospitality dashboard" would be the failure the hospitality pack exists to detect. Each item below names the pattern it is a configuration of.

## The gap list, ordered by leverage vs cost

### 1. The filing period as a first-class object — the biggest gap

**What**: business users think in filing periods, not finding streams. There is no "July 2026 reconciliation": no period list, no per-period totals, no re-open of a prior period.

**Why**: a re-run currently returns `{evaluated, found, opened, alreadyOpen}` and appends to one queue. Nobody can answer "what was my position last month and what changed this month?" This is exactly the cross-period gap `105c` names (January miss → February appearance): each upload lands in its own source graph with no link between periods.

**Pattern / cost**: this is a *platform* question before it is a UI one — the `Filing`-per-period model from `105c` does not exist (it was named a deliberate scope cut). The UI surface (period list + per-period review) is the vocabulary-browser/review patterns; the blocker is the data model. Sequencing: the platform decision must precede the surface.

### 2. A per-period ITC scorecard (amounts, not just findings)

**What**: per period — ITC in the purchase register vs in GSTR-2B, reconciled, at-risk ₹ by rule, and a "reconciled cleanly" confirmation.

**Why**: `eval/questions.md` Q15 computes ₹45,000 at risk, ₹900 delta. That is a CA's first question and nothing surfaces amounts at all today. Equally important is the *absence* signal: the queue only shows problems, so a clean month renders as an empty page that reads like "nothing ran". Q6's compliant invoices need a visible "reconciled cleanly" state, or the review is untrustworthy in the one direction that matters.

**Pattern / cost**: Epic 93's overview pattern, period-scoped. Every number already exists in the graph (`taxAmount`); nothing is invented, which is 93's own ship rule. Needs item 1's period model to attach to.

### 3. Supplier-centric view — the strongest "graph, not spreadsheet" surface

**What**: a supplier ledger — per supplier, the invoices claimed / filed / mismatched, and at-risk ₹. Drilled from any finding; a CA follows up per-supplier ("email Alpha Traders — 3 unmatched invoices").

**Why**: findings are per-invoice; follow-up is per-supplier. `105c` Slice 1 (shipped) made `gst:Supplier` real with `issuedBy` edges — the exact substrate this needs. It is the surface that makes the graph earn its maintenance cost versus a spreadsheet join.

**Pattern / cost**: the vocabulary browser (tree + detail) over supplier subjects, or the composable entity page per supplier. Pure pack data; no platform work. Cheap.

### 4. The evidence chain drill-down (already scoped, unbuilt)

**What**: `105e` Slice 2 — the traversal-derived evidence graph in the finding detail pane: finding → subject → supplier → both filings, and what is missing.

**Why**: a reviewer needs to see the graph around a finding, not just the two numbers. Already designed; ship it.

### 5. Follow-up lifecycle, not just accept/dismiss

**What**: finding → follow-up (owner, due date, note, status like awaiting-supplier / cleared / reversed-in-books), plus a run-to-run delta view: "3 accepted cleared, 1 reopened, 2 new."

**Why**: the queue's decision answers "is this a real finding". The workflow *after* acceptance is case management — the supplier files it, a re-run clears it, or the entry is reversed. The reopen machinery already exists (`alreadyOpen`); surfacing the delta between runs is what's missing.

**Pattern / cost**: extension of the review queue pattern (add follow-up state alongside the decision). The run-to-run delta is a report over existing finding history. Cheap-to-medium.

### 6. Auditor-ready export

**What**: per-period, per-supplier findings with evidence and citations, as Excel/CSV — the working file CAs retain for the statutory period.

**Why**: the record is the deliverable, not a side effect. The existing export dialog (Epic 9) is RDF-shaped; GST needs a report-shaped export.

**Pattern / cost**: the export dialog pattern, report-shaped serializer (pack-configurable columns). Cheap; needs item 1's period grouping.

### 7. "Why is my ITC lower this month?" — the differentiator surface

**What**: a natural-language "ask" over the reconciliation that answers with an evidence chain and citations.

**Why**: `105c`'s own roadmap names this the real GraphOwl value and it remains unstarted (platform P10/P11: 8 MCP tools, LangGraph agent). Named last here because it is the only item that is genuinely new platform work and it depends on item 4 (evidence-chain walk) existing first.

### 8. Smaller, real ones

- **Law / notice panel**: a finding cites its section; a defender needs the provision text and what to do. The law TTL already exists (`law/sections.ttl`, `law/rule-36-4.ttl`). Config of the entity/knowledge surface.
- **GSTIN identity resolution**: the transposition finding surfaces a GSTIN pair; a "which is right" adjudication (the existing review pattern) lets the finding close rather than loop every run. Fits Epic 17's merge-adjudication shape.
- **Real-input experience**: today is JSON/TTL fixtures; business reality is Excel from Tally/Zoho, multi-period files, column mapping, and a dry-run preview before commit. Schema-driven form pattern.
- **As-of review**: the pack already runs rules `as_of` a filing date; a reviewer should be able to pick the review moment rather than only "now".

## What this deliberately does not do

- It does not propose new Rust crates or new console routes. Every surface above is a configuration of an existing `00h` pattern or a pack-data change.
- It does not hardcode GST into the console. Each item names the neutral pattern it configures; a surface that cannot also serve the hospitality pack is out of scope by construction.
- It does not commit to slices or a timeline. Items 2–6 are config-scale; item 1 is a platform decision that several depend on; item 7 is multi-week platform work (P10/P11). The next step for items 1 and 7 is `story-splitting`/`grill-me`, exactly as `105b` and `105c` already require for theirs.
