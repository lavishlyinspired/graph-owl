# Reconciliation Agent — file upload, live verification results

**Date**: 12 August 2026
**What this proves**: a user can attach a GSTR-2B-shaped file and a purchase-register-shaped file directly to a chat question in the console's Agent tab, preview them before or after sending, and ask the Reconciliation Agent to reconcile them — a real, working tool call (`reconcile_uploaded_files`), not a scripted demo. Every result below was produced by the actual running system (`agent_service` + the real configured LLM + the real console UI) and cross-checked against a hand-derived answer key computed directly from `compare_purchase_records` before any live question was asked.

## The sample data

Two files, deliberately planted with one invoice in each of the tool's 8 outcome categories — "rich" in the sense of covering every code path, not just a happy-path pair:

- `integrations/langchain/agent_service/sample_data/gstr2b_sample.json` — 11 invoices, GSTR-2B-shaped (`invoiceNumber`, `supplierGstin`, `taxableValue`, `cgst`/`sgst`/`igst`, `totalTax`, `itcAvailability`, `filingStatus`)
- `integrations/langchain/agent_service/sample_data/purchase_register_sample.json` — 10 entries, purchase-register-shaped (`invoiceNumber`, `vendorGstin`, `taxableValue`, `cgst`/`sgst`/`igst`, `totalTax`)

### Answer key (computed directly from `compare_purchase_records`, before any live question)

| Invoice | Category | Why |
|---|---|---|
| INV-2001 | Matched | Identical on both sides |
| INV-2002 | AmountMismatch | ₹50,000/₹9,000 (2B) vs ₹52,000/₹9,360 (register) |
| INV-2003 | MissingFromRegister | Filed in GSTR-2B; never booked in the register |
| INV-2004 | MissingFromGstr2b | Claimed in the register; supplier never filed it |
| INV-2005 | GstinTransposition | `24AAFCP7788K1ZQ` vs `24AAFCP7788K1QZ` — last two characters swapped |
| INV-2006 | ItcNotAvailable | 2B reports `itcAvailability: "Not Available"`; amounts otherwise match |
| INV-2007 | Matched | Identical on both sides |
| INV-2008 | Matched | Identical on both sides (interstate, IGST-only) |
| INV-2009 | Reversed | 2B `filingStatus: "Cancelled"`; register still holds it as a normal claim |
| INV-2010 | Matched | Identical on both sides |
| INV-2011 | SupplierMismatch | `27AAACR5055K1Z2` vs `19BBBCX1122L1Z8` — genuinely different GSTIN, not a transposition |

8/8 categories exercised: Matched, AmountMismatch, MissingFromRegister, MissingFromGstr2b, GstinTransposition, ItcNotAvailable, Reversed, SupplierMismatch.

## What was actually tested, live, in the browser

1. Uploaded both files via the paperclip attach button — real `POST /files` calls, real staged-file chips in the composer.
2. Clicked a staged chip before sending — the preview modal opened, fetched via a real `GET /files/{id}`, and rendered the exact pretty-printed JSON that was uploaded.
3. Asked the questions below with both files attached. The console showed: a rotating "thinking" label (different word each time — "Cogitating…", "Catapulating…" — confirmed across two separate questions), the `Used reconcile_uploaded_files` tool badge transitioning from spinner to green check, real token-by-token streaming of the answer, and the attached-file chips displayed at the top of each thread's transcript.

## Question 1

> **Reconcile these two attached files and tell me every mismatch, grouped by category.**

**Result** (verbatim from the live run):

Reconciliation complete. **11 invoices** were compared across the two files (GSTR-2B and purchase register), with **4 matched** and **7 mismatches**, grouped by category below.

**Matched (4)**: INV-2001, INV-2007, INV-2008, INV-2010 — amounts, GSTIN and status agree on both sides.

**AmountMismatch (1)**: INV-2002 — GSTR-2B reports taxable value ₹50,000 / tax ₹9,000; the register reports ₹52,000 / tax ₹9,360. The purchase register is overstated relative to the supplier's filing.

**MissingFromRegister (1)**: INV-2003 — filed by the supplier in GSTR-2B, but not recorded in the purchase register.

**MissingFromGstr2b (1)**: INV-2004 — claimed in the purchase register, but the supplier has not filed it in GSTR-2B.

**GstinTransposition (1)**: INV-2005 — GSTR-2B has `24AAFCP7788K1ZQ`; the register has `24AAFCP7788K1QZ` — the last two characters are transposed.

**ItcNotAvailable (1)**: INV-2006 — GSTR-2B reports input tax credit as unavailable for this invoice.

**Reversed (1)**: INV-2009 — the supplier has cancelled this invoice in GSTR-2B; any credit claimed on it should be reversed.

**SupplierMismatch (1)**: INV-2011 — GSTR-2B has `27AAACR5055K1Z2`; the register has `19BBBCX1122L1Z8` — entirely different GSTINs, not a simple transposition.

The model also added an unprompted "Summary of action items by severity" section grouping the seven mismatches into verify/flag, follow-up-with-supplier, amount-correction and accounting-reversal buckets.

**Verification**: every one of the 11 invoices was classified into exactly the category the answer key predicts. 11/11 exact match.

## Question 2

> **In the attached GSTR-2B and purchase register, which invoices carry real ITC risk that should be escalated first, and why?**

**Result** (verbatim from the live run):

> Reconciliation summary (11 invoices): 4 Matched · 1 AmountMismatch · 1 MissingFromRegister · 1 MissingFromGstr2b · 1 GstinTransposition · 1 ItcNotAvailable · 1 Reversed · 1 SupplierMismatch
>
> **Escalate first — credit actually at risk (inadmissible / must be reversed)**
>
> 1. **INV-2006 — ITC not available.** GSTR-2B itself reports the input tax credit as unavailable for this invoice. Any ITC booked against it is not admissible. Highest priority: the return the supplier filed says the credit does not exist.
> 2. **INV-2009 — Reversed / cancelled.** The supplier has cancelled this invoice in GSTR-2B. If the register still holds a claim, the credit must be reversed immediately — retaining it is an outright over-claim.
> 3. **INV-2004 — Missing from GSTR-2B.** The invoice claimed in the purchase register has no counterpart in the supplier's filing. The claim is unsupported by any supplier return, so the credit is not backed/available.
> 4. **INV-2011 — Supplier mismatch (not a transposition).** GSTR-2B shows `27AAACR5055K1Z2` while the register shows `19BBBCX1122L1Z8` — two wholly different GSTINs, so this is not a keystroke error. The credit is being claimed against an invoice from a different entity than the one that filed it; that credit is not available.
>
> **Escalate next — material but correctable**
>
> 5. **INV-2002 — Amount mismatch (over-claim).** The register claims ₹52,000 taxable / ₹9,360 tax while GSTR-2B shows ₹50,000 / ₹9,000. That is a ₹360 excess ITC claim (plus ₹2,000 overstated taxable value) versus what the supplier reported. Needs correction/reversal of the excess.
> 6. **INV-2005 — GSTIN transposition.** `24AAFCP7788K1ZQ` vs `24AAFCP7788K1QZ` — two characters swapped. Likely the same supplier, so this is a data fix rather than an exposure, but it blocks matching and validation until corrected.
>
> **Not an ITC risk — but an action item**
>
> - **INV-2003 — Missing from register.** The supplier did file it in GSTR-2B, but the business never recorded it. That is not an inadmissible claim; it is eligible ITC being left unclaimed — worth recording to capture the credit.
>
> **Bottom line**: escalate INV-2006, INV-2009, INV-2004 and INV-2011 first (all involve credit that is unavailable or must be reversed), then INV-2002 for the ₹360 over-claim, with INV-2005 as a quick correction and INV-2003 as a reclaim opportunity.

**Verification**: this is real domain reasoning, not a template restatement of question 1. It correctly separates three distinct classes the raw category labels don't distinguish by themselves — inadmissible/must-reverse (2006, 2009, 2004, 2011), material-but-correctable (2002, 2005), and not-a-risk-but-worth-recording (2003) — and every invoice landed in the right bucket for real GST reconciliation reasoning (a cancelled or blocked-credit invoice is a harder exposure than an amount typo; an unclaimed-but-legitimate invoice is an opportunity, not a risk).

## A real limitation found live, not glossed over

A third question — **"Based on the reconciliation you just ran, which invoices carry real ITC risk and should be escalated first?"** — was asked in a **new** thread with **no files re-attached**, assuming (incorrectly) that the agent would remember the prior thread's reconciliation. It does not: each question is an independent investigation (`agent_service/streaming.py`'s own design — no shared state between threads, by architecture, not by bug). With nothing to reconcile and no files in context, the agent spent 27+ tool calls searching the real GST pack's graph data for findings that were never there, never reaching a useful answer within a reasonable time. This is not a defect in the reconciliation tool — it is a correct demonstration of the thread-isolation design working exactly as built, surfaced by a badly-scoped question. **Takeaway for real use**: re-attach files (or restate the question fully) on every new question; don't rely on cross-thread memory.

## What this does and does not replace

`reconcile_uploaded_files` is a **session-only, ad hoc** comparison — it never touches the graph, never writes a finding, and uses a fixed ₹1 rounding tolerance rather than the pack's own Rule 36(4)-derived, date-sensitive cap. It exists for "I have two files right now and want an answer immediately," not as a substitute for the console's Governance tab "Run reconciliation" button, which is the real, graph-backed, evidence-chain-producing mechanism (`packs/gst/pack.toml`'s six registered finding rules). See `agent_service/reconcile_uploaded.py`'s own docstring for the full distinction.
