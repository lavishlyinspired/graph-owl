# GST reconciliation — evaluation set

**Written before any agent exists, which is the only time it can be written
honestly.** An answer key produced after seeing a system's output measures how
well the key was fitted to the system, not how well the system answers. Every
expected answer below is derived from `../fixtures/` and `../law/` by hand and
is checkable against them without running anything.

The set is deliberately structured so that a **vector-only baseline over the
same source text can be run against it**. If retrieval over the raw fixtures
answers the multi-hop questions as well as the graph does, the graph is not
earning its maintenance cost and the honest conclusion is to say so. The
single-hop questions are expected to be close; questions 12–15 are where the
difference should show, because each needs two or three joins that no amount
of similarity search over invoice text can perform.

**Scoring.** Precision and recall at the level of the *finding*, not the
sentence: a correct answer names the right invoices and no others. A response
that names INV-1003 and INV-1004 where the key says INV-1003 alone scores
recall 1.0 and precision 0.5. Report a Wilson interval on a sample, not a
single number — at fifteen questions the interval is wide, and pretending
otherwise is how an evaluation stops being informative.

**Citations are part of correctness.** An answer that names the right invoice
and cites the wrong provision is wrong. Section 16(2)(aa), Section 16(2)(d),
Section 17(5), Rule 36(4) and the pack's own matching policy each govern
different findings, and confusing them is exactly the failure a reviewer needs
the system not to make.

---

## Single-hop (1–5): does it read the graph at all

| # | Question | Expected answer | Governed by |
|---|---|---|---|
| 1 | Which July 2026 invoices did the supplier never file in GSTR-2B? | INV-1003 only. INV-1004 is *also* unmatched, but under a near-identical GSTIN — an answer naming it here without that qualification is wrong (see 13). | Sec 16(2)(aa) |
| 2 | Which invoices have a value disagreement with GSTR-2B? | INV-1002 (₹100,000 vs ₹95,000) and INV-2002 (₹100,000 vs ₹80,000). **Not** INV-2001. | Rule 36(4) |
| 3 | Which matched invoices carry no usable input tax credit? | INV-1005 — every number agrees and the authority reports `itcAvailable = N`. | Sec 17(5) |
| 4 | Which invoices are flagged reverse-charge? | INV-1006. | Sec 16(2)(aa) |
| 5 | Which invoices are unpaid past 180 days? | INV-1003 (paid at 240 days) and INV-2002 (never paid, six years old). **Not** INV-1001 (paid at 20 days) and **not** INV-1006 (unpaid but only six days old — not yet due). | Sec 16(2)(d) |

## Discrimination (6–11): does it know when *not* to answer

These are the questions a system that pattern-matches on "invoice + problem"
gets wrong. Each has a tempting near-answer.

| # | Question | Expected answer | Why it is a trap |
|---|---|---|---|
| 6 | Is INV-1001 compliant? | Yes — matched, credit available, not reverse-charge, paid in 20 days. No finding of any kind. | It is the only wholly clean invoice; a system biased toward finding problems will invent one. |
| 7 | Is INV-2001's 5% value difference a problem? | **No.** The invoice is dated July 2020, when Rule 36(4) allowed a 10% provisional cap under Notification 75/2019-CT. | The identical delta on a 2026 invoice *is* a finding. Answering "yes, 5% is a mismatch" means the cap was assumed rather than read. |
| 8 | Why is INV-2002 a finding when INV-2001 is not, given both are July 2020? | The delta is 20% against the same 10% cap. Same rule, same period, different magnitude. | Tests whether the cap is understood as a threshold rather than as a label. |
| 9 | INV-1005 matches GSTR-2B exactly. Can the credit be claimed? | No. Matching is necessary, not sufficient — the authority reports the credit as unavailable. | The most valuable question in the set. A reconciliation framed as "do the numbers agree" answers yes. |
| 10 | Has INV-1006 been paid, and is that a problem? | No payment event exists for it — and it is **not** a problem: the invoice is six days old and the 180 days have not elapsed. INV-2002 is the same absence six years on, and that one is a breach. | Absence versus lateness, and then absence versus *not yet due*. A model that only reasons over payment dates has nothing to look at; one that treats every missing payment as overdue accuses a taxpayer who has done nothing wrong. |
| 11 | Which supplier does INV-1004 belong to? | Unresolved, deliberately. The register says `27AABCU9603R1MZ`, GSTR-2B says `27AABCU9603R1ZM`, and the pack surfaces the pair rather than choosing. | An answer that picks one silently has performed the merge the matching policy forbids. |

## Multi-hop (12–15): where the graph should beat retrieval

Each needs at least two joins. A vector baseline over the fixture text can
retrieve the relevant invoices; it cannot traverse to the provision in force,
or from an invoice to an event that does not exist.

| # | Question | Expected answer | Hops |
|---|---|---|---|
| 12 | What was the provisional credit cap in force when INV-2001 was issued, and under which notification? | 10%, Notification 75/2019-CT. The 5% and nil caps came later and do not apply. | invoice → date → provision in force → citation |
| 13 | Is INV-1004 genuinely missing from GSTR-2B, or is something else going on? | Something else: the supplier *did* file it, under a GSTIN differing by a two-character transposition. Both findings are true and the second explains the first. | invoice → exact-match failure → similarity candidate → 2B line |
| 14 | Which invoices would become compliant if the taxpayer paid them today? | **INV-1006 only** — it is unpaid but still inside its 180 days, so paying now keeps the condition satisfied. INV-1003 (already paid, late) and INV-2002 (six years unpaid) are both past the point where payment restores anything. | invoice → purchase event → payment event or its absence → elapsed span against the filing date → statute |
| 15 | For each unmatched or disputed July 2026 invoice, what is the total tax at risk, and under which provision? | ₹45,000 (INV-1003, Sec 16(2)(aa)); ₹9,000 (INV-1004, matching policy — at risk only until the transposition is adjudicated); ₹7,200 (INV-1005, Sec 17(5)); ₹10,800 (INV-1006, Sec 16(2)(aa) reverse charge); INV-1002's disputed taxable-value delta is ₹5,000 with the tax difference ₹900 (Rule 36(4)). | every finding → its subject → its tax amount → its provision |

---

## Running it

There is no agent yet, so today this file is scored by hand against
`scripts/verify-gst-reconciliation.sh`'s output — which already asserts the
finding-level answers for questions 1–5 and the discrimination in 6–11. That
is a deliberate ordering: the deterministic layer must be right before a
language model is asked to narrate it, because a model given wrong findings
will narrate them fluently and a reviewer will believe it.

**What this set cannot measure**, stated so nobody reads more into a score
than it carries: fifteen questions over eight invoices is an
existence-and-discrimination check, not a statistical claim. It tells you the
system distinguishes the cases it was built to distinguish. It tells you
nothing about a real estate of fifty thousand invoices, where the interesting
failures are the ones nobody planted.
