"""What every figure on every screen means, how it was derived, and what to do.

**The user's own ask, and the right shape for it**: "how is this value derived,
what's the calculation... you should have an explanation as a hover or an (i)
popup. This has to be for all the screens."

Kept as **data in one module** rather than as tooltip strings scattered through
JSX, for three reasons that have each already cost this product something:

- A formula written next to the component that renders it drifts from the code
  that computes it. Here the explanation sits beside neither and is imported by
  both, so a figure and its stated derivation are one edit.
- The same figure appears on several screens. Two screens explaining one number
  differently is worse than neither explaining it.
- **A figure without its remedy is half an answer.** "Blocked" and "pending"
  are the same size of number and opposite situations — one is lost, the other
  is a phone call. Every entry therefore carries `action`, not just `formula`.

**`means` is written for a business reader, `formula` for someone checking the
arithmetic.** Neither substitutes for the other: a CA asked to defend a figure
needs the second, and everyone reading the screen needs the first.
"""

from __future__ import annotations

from typing import Any


def figure(*, means: str, formula: str, action: str, source: str) -> dict[str, str]:
    """One explained figure.

    `source` names where the inputs came from, because "computed from the
    reconciliation" and "read off the filed return" are different kinds of
    number and a reader deciding whether to trust one needs to know which.
    """
    return {"means": means, "formula": formula, "action": action, "source": source}


#: The five ITC classes. Definitions copied in spirit — not in wording — from
#: `reconcile_result.itc_position`'s own docstring, which is the authority on
#: what each class contains.
ITC_POSITION: dict[str, dict[str, str]] = {
    "confirmed": figure(
        means="Credit you can claim now. The invoice is in your books, the supplier "
        "has filed it, and nothing blocks it.",
        formula="Sum of books-side tax across every invoice in the Matched bucket.",
        action="Claim it in this period's GSTR-3B.",
        source="Your purchase register matched against GSTR-2B.",
    ),
    "pending": figure(
        means="Credit that is deferred, not lost. You booked the purchase and the "
        "supplier has not filed it yet — it becomes claimable in a later period "
        "once they do.",
        formula="Sum of books-side tax across every invoice in the Only-Books bucket.",
        action="Chase the supplier. This is a phone call, not a write-off.",
        source="Invoices in your books with no GSTR-2B counterpart.",
    ),
    "blocked": figure(
        means="Credit that is lost. Section 17(5) or reverse charge — no amount of "
        "chasing changes it.",
        formula="Sum of tax across every invoice the portal or the rules marked "
        "ineligible.",
        action="Reverse it. Do not carry it forward as recoverable.",
        source="GSTR-2B's own ITC-availability flag, and the s.17(5) rule.",
    ),
    "under_review": figure(
        means="The part of a matched invoice that the two sides disagree about. "
        "Only the difference is in doubt, never the whole invoice.",
        formula="Sum of |books tax − portal tax| across every invoice in the "
        "Review bucket.",
        action="Reconcile the difference with the supplier before filing.",
        source="Invoices present on both sides with different values.",
    ),
    "unclaimed": figure(
        means="Credit the portal shows and your books do not. Available, and nobody "
        "recorded the purchase.",
        formula="Sum of portal-side tax across every invoice in the Only-Portal bucket.",
        action="Find the invoice and book it. Claiming without one is how a notice "
        "starts.",
        source="GSTR-2B lines with no purchase-register counterpart.",
    ),
    "total_considered": figure(
        means="Everything the reconciliation classified, added up.",
        formula="confirmed + pending + blocked + under review + unclaimed. Note that "
        "under review contributes only the difference between the two sides, not "
        "the whole invoice — so this is not the period's total ITC.",
        action="Use the individual classes to decide anything; this is a coverage "
        "check, not a claim figure.",
        source="The five classes above.",
    ),
}

#: Why the ITC position and the GSTR-3B working paper report different totals.
#: **The single most confusing pair of screens in the product**, and the reason
#: is legitimate: they count different populations. Two correct numbers that
#: differ look like a bug unless each says what it counted.
ITC_VS_WORKING_PAPER = (
    "This screen classifies your **books-side** credit into where each rupee stands. "
    "The GSTR-3B working paper starts from the **portal-side** total GSTR-2B made "
    "available and subtracts statutory reversals. They count different populations, "
    "so the totals differ legitimately — use this screen to decide what to chase, "
    "and the working paper to decide what to file."
)

#: Buckets on the reconcile screen.
BUCKETS: dict[str, dict[str, str]] = {
    "matched": figure(
        means="Both sides report the invoice and the values agree.",
        formula="Invoices present in both the purchase register and GSTR-2B whose "
        "tax totals agree within the cap in force under Rule 36(4).",
        action="Nothing. These are the ones you do not have to think about.",
        source="Purchase register ∩ GSTR-2B.",
    ),
    "review": figure(
        means="Both sides report the invoice and the values differ.",
        formula="Invoices present on both sides whose tax totals differ by more than "
        "Rule 36(4)'s cap allows.",
        action="Establish which side is right before filing.",
        source="Purchase register ∩ GSTR-2B, values compared.",
    ),
    "only_books": figure(
        means="You have the invoice; the supplier has not filed it.",
        formula="Invoices in the purchase register with no GSTR-2B counterpart in "
        "any period held.",
        action="Chase the supplier — the credit is deferred, not lost.",
        source="Purchase register − GSTR-2B.",
    ),
    "only_portal": figure(
        means="The supplier filed it; you have not recorded the purchase.",
        formula="GSTR-2B lines with no purchase-register counterpart.",
        action="Find the invoice and book it, or establish it is not yours.",
        source="GSTR-2B − purchase register.",
    ),
    "match_rate": figure(
        means="How much of the period reconciled cleanly on the first pass.",
        formula="matched ÷ (every invoice on either side). A low rate is not an "
        "error — it is how much work the period holds.",
        action="Use it to size the work, not to judge the data.",
        source="The four buckets.",
    ),
}

#: The GSTR-3B working paper's own chain.
WORKING_PAPER: dict[str, dict[str, str]] = {
    "gross": figure(
        means="Everything GSTR-2B made available to you this period.",
        formula="Sum of tax across every GSTR-2B line for the period.",
        action="This is the ceiling. Nothing below it can exceed it.",
        source="GSTR-2B as filed by your suppliers.",
    ),
    "blocked_17_5": figure(
        means="Credit the law does not allow at all.",
        formula="Sum of tax on invoices the portal or the s.17(5) rule marked "
        "ineligible.",
        action="Report in Table 4B(1). Permanent — never reclaimable.",
        source="graph-owl's s.17(5) rule.",
    ),
    "reversal_rule_42_43": figure(
        means="The share of credit attributable to exempt or non-business use.",
        formula="Computed on the period's turnover split, not per invoice. Reco Now "
        "reports that a reversal is due and does not compute the amount — the "
        "inputs are yours.",
        action="Compute and report in Table 4B(1).",
        source="The period's declared exempt and total turnover.",
    ),
    "timing_16_2_b": figure(
        means="Credit you cannot take yet because the goods have not arrived.",
        formula="Sum of tax on invoices whose goods-receipt date falls after the "
        "GSTR-2B period.",
        action="Defer to the period of receipt. Not lost.",
        source="Your goods-receipt records against the 2B period.",
    ),
    "reversal_rule_37": figure(
        means="Credit that must be reversed because the supplier is unpaid past 180 "
        "days.",
        formula="Sum of tax on invoices with no payment within 180 days of the "
        "invoice date.",
        action="Report in Table 4B(2). Reclaimable once you pay.",
        source="Your payment ledger against invoice dates.",
    ),
    "net": figure(
        means="What you can actually claim this period, after every deduction above.",
        formula="ITC available − blocked − proportionate − timing − Rule 37.",
        action="This is the figure Table 4C should carry.",
        source="The chain above.",
    ),
}


def explained(catalog: dict[str, dict[str, str]], keys: list[str]) -> dict[str, Any]:
    """The subset of `catalog` a screen actually shows.

    Sending only what is rendered keeps the payload honest: an explanation for
    a figure the screen does not display is an explanation nobody can check
    against anything.
    """
    return {key: catalog[key] for key in keys if key in catalog}


__all__ = [
    "BUCKETS",
    "ITC_POSITION",
    "ITC_VS_WORKING_PAPER",
    "WORKING_PAPER",
    "explained",
    "figure",
]
