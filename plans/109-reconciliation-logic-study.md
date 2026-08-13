# Study — how GST purchase reconciliation is actually done, across six implementations

**Status**: study only. No code was taken from any of them; what is written down
here is domain logic, which is determined by the GST return formats and the
Act, not by anyone's expression of it.

**Why this document exists.** Plan 108 built the reconciliation from the return
formats and the statute, and it was right about the statute and wrong about the
*mechanics* in four ways that only showed up against real data. Every one of
those four is something all six of these tools already do. This is the record of
what they do and why, so the next slice starts from the state of the art rather
than rediscovering it.

## Licence position, stated plainly

Three of the six are not permissively licensed:

| Repository | Licence |
|---|---|
| `Rekvia` | MIT |
| `gst-recon-engine` | MIT |
| `Free-GST-Billing-Software` | MIT |
| `india-compliance` | **GPL-3.0** |
| `gstr-reconciliation` | **none declared** |
| `GST-Bank-Reconciliation-Tool` | **All Rights Reserved** |

All six were read, on the repository owner's explicit instruction, after the
licence position was raised. **The line held while reading is the one that
matters: copyright protects expression, not facts or processes.** "Match on
PAN when the GSTIN differs" is a fact about Indian tax registration; the code
that implements it is expression. This document records the former. No source,
structure, constant table, identifier scheme, comment or test fixture from any
of the three non-permissive repositories has been copied into graph-owl, and
nothing in this project derives from their expression.

`plans/00l-build-vs-adopt.md` carries the adoption decision (nothing adopted).

## What every one of them does that Plan 108 did not

Four things, and each corresponds to a defect this project shipped and fixed:

1. **Normalize the invoice number before matching.** All six strip case and
   punctuation. `Rekvia` reduces to `[A-Z0-9]`; `india-compliance` has a
   `get_cleaner_bill_no` that also strips the fiscal year. graph-owl joined on
   the raw literal, so `INV/1014` and `INV-1014` were two invoices.

2. **Tolerate a rounding difference.** `india-compliance` treats a difference
   `<= 1` as a match — the same ₹1 this project arrived at independently, and
   for the same reason: GSTR-3B is filed in whole rupees, so a sub-rupee
   difference cannot change what is claimed. `Rekvia` uses ±₹2.

3. **Compare head-wise.** CGST, SGST, IGST and Cess are separate fields in
   every one of them. A total-only comparison cannot see an intra-state supply
   booked as inter-state, which nets to zero.

4. **Report a match *status*, not just an exception.** Which brings us to the
   real structural difference.

## The structural insight: matching is a status per pair, assigned in priority order

`india-compliance` models this most completely. It does not run independent
"finding" rules; it runs an ordered **rule matrix** over field × match-mode and
assigns the first status that fits:

| Status | Meaning |
|---|---|
| `Exact Match` | every field exact, zero difference |
| `Suggested Match` | fuzzy invoice number, **or** ≤₹1 difference — a human confirms |
| `Mismatch` | identity matches, values or heads do not |
| `Only in 2A/2B` | no counterpart in the books |
| `Only in Books` | no counterpart in the return |
| `Manual Match` | a human linked the pair |

Two properties of that design graph-owl does **not** have and should consider:

- **A pair is matched at most once.** Each rule pass removes the pairs it
  matched from the pool, so one invoice cannot be claimed by two rules. Every
  graph-owl rule is an independent SPARQL query over the whole graph, so one
  invoice can and does appear under several findings — which is why this
  project has needed three separate hand-written guards (`PotentialMismatch`
  standing down, the transposed-GSTIN guard, the 2B-presence guard) to stop
  rules contradicting each other. Those guards are doing by hand what
  sequential assignment does structurally.
- **`Suggested Match` is a first-class outcome.** graph-owl has only "finding"
  or "silence", so a probable match with a fuzzy number has nowhere to go: it
  either fires a false accusation or vanishes. This is the same gap
  `[findings.similarity]` exists for on `GstinTransposition` and it is not
  generalised.

**Neither is fixed in this slice**, and neither should be papered over: they are
a change to what a finding *is*, not another query.

## PAN-level matching — the one domain fact worth implementing immediately

A GSTIN is `SS PPPPPPPPPP E Z C`: two state digits, then the **ten-character
PAN**, then an entity code, `Z`, and a checksum. Two GSTINs sharing characters
3–12 are **the same legal entity registered in different states**.

`india-compliance` runs a whole second matching tier on this: after GSTIN-level
matching, what is left is re-keyed on PAN and matched again. The case is common
and entirely legitimate — a supplier bills you from their Maharashtra
registration and you booked them under the Karnataka one, or a group company
files centrally.

**And it carries a subtlety that matters for this project's newest rule**: at
PAN level, `india-compliance` compares **total GST only**, not the head-wise
split — because a genuinely cross-state pair *should* differ on the heads
(IGST one side, CGST+SGST the other). graph-owl's `TaxHeadMismatch` joins on
identical GSTIN, so it is safe today; a PAN-level variant must not reuse it.

This is implemented in the slice that follows this document, as
`gst:SupplierPanMismatch` — see `queries/supplier-pan-mismatch.sparql`.

## Fuzzy invoice matching, and the guard that makes it safe

`india-compliance` accepts a fuzzy invoice-number match only when the two
invoice dates are **within 10 days** of each other, then requires either a 100%
partial ratio or a ≥90% similarity. `Rekvia` blocks on `(GSTIN, rounded tax)`
first and only then tries a fuzzy number.

The shared principle is the one graph-owl's own `blocking_strategy.rs` already
states: **a fuzzy key narrows who is compared, it never decides a match by
itself.** graph-owl declares exactly these strategies (`normalized`,
`composite` with a `date_window`, `ngram`) in `packs/gst/pack.toml` and the
reconciliation does not invoke any of them — the finding rules are plain SPARQL
joins. That is the largest single gap between this project and the reference
set, and it is an engine change, not a query change.

## Bucket vocabulary the practitioner formats demand

From `CA_GURUJI`'s own reconciliation workbooks, which are the CA-facing
requirement rather than a tool's implementation:

```
Balance as per Tally (books)      BASIC | CGST | SGST | IGST | CESS
  Less : Debit note as per Tally
= Net Balance as per Tally
Balance as per 2B  /  2A          (each with its own debit-note deduction)
  Add  : credit not taken but showing in 2B
  Less : credit taken but not shown in 2B
  Add  : showing in 2A but not in 2B
  Less : showing in 2B but not in 2A
```

Two of those lines graph-owl still cannot produce:

- **Debit and credit notes are not modelled at all.** Every format nets them
  per source. This is Plan 108's own parking-lot item and it is the largest
  remaining correctness gap — a register with credit notes reconciles wrongly
  today, not partially.
- **"In 2B but not in 2A"** has no rule. The reverse-direction anomaly Plan 108
  flagged; the practitioner format treats it as a routine line, not an oddity.

## Risk tagging

`Rekvia` assigns HIGH / MEDIUM / LOW per row, and escalates to
`HIGH (Invalid GSTIN)` when the GSTIN fails its checksum-shaped regex. graph-owl
validates no GSTIN anywhere. A structurally invalid GSTIN in the register is
worth surfacing before any matching runs, because it explains every downstream
"missing" finding for that supplier at once.

## What this study changed, and what it deliberately did not

**Implemented off the back of it**: PAN-level supplier matching
(`gst:SupplierPanMismatch`).

**Recorded, not built** — each needs its own slice and its own argument:

- Sequential one-pair-one-status matching, replacing independent rules and the
  hand-written guards that keep them from contradicting each other.
- `Suggested Match` as an outcome between "finding" and "silence".
- Debit and credit notes as a document type.
- The `2B \ 2A` line.
- GSTIN structural validation as a pre-matching check.
- Invoking `[[matching.blocking]]` from reconciliation at all.
