"""GSTR-2B against GSTR-3B — the core monthly ITC reconciliation.

Table 4A of GSTR-3B is **auto-populated from GSTR-2B**. That single fact is
what makes this comparison meaningful: the portal states what credit it made
available, the taxpayer states what they took, and the two are supposed to be
the same number. Where they are not, the taxpayer usually finds out from a
notice rather than from their own books.

**The two directions are not symmetrical and are never collapsed into one
signed figure.** An excess claim attracts interest under s.50 and a demand
under s.73/74; unclaimed credit is recoverable until s.16(4) closes the
window. Opposite situations, opposite remedies — and a sign is the easiest
thing on a screen to misread, so the direction is named in words and the
magnitude is always reported unsigned.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Any

#: What the difference means, in the preparer's terms rather than the
#: arithmetic's. `agrees` is a real outcome and not merely "difference == 0" —
#: a consumer showing only a number cannot tell it from a run that never
#: happened, which is what `not_evaluated` is for.
EXCESS = "excess"
UNCLAIMED = "unclaimed"
AGREES = "agrees"
NOT_EVALUATED = "not_evaluated"


def _amount(value: object) -> Decimal | None:
    if value is None or value == "":
        return None
    try:
        return Decimal(str(value))
    except (ArithmeticError, ValueError):
        return None


def _or_zero(value: object) -> Decimal:
    parsed = _amount(value)
    return parsed if parsed is not None else Decimal("0")


def compare_2b_to_3b(
    *, gstr2b_total: Decimal, gstr3b: dict[str, Any] | None
) -> dict[str, Any]:
    """What the portal made available against what the return claimed.

    `gstr2b_total` is the summed tax across every 2B line for the period.
    `gstr3b` is one period's Table 4 figures, or `None` when no return was
    supplied.

    **4A is what is compared, not 4C.** 4A is the row 2B auto-populates, so it
    is the one 2B is supposed to agree with. 4C is 4A minus reversals the
    taxpayer made deliberately — comparing that against 2B would report every
    legitimate s.17(5) reversal as an under-claim, turning correct compliance
    into a finding.
    """
    if gstr3b is None:
        return {
            "direction": NOT_EVALUATED,
            "difference": None,
            "needs": "a GSTR-3B for this period — none was supplied",
            "available_2b": gstr2b_total,
            "gross_claimed": None,
            "reversed": None,
            "net_claimed": None,
            "arithmetic_ok": None,
        }

    gross = _amount(gstr3b.get("itc_4a"))
    if gross is None:
        return {
            "direction": NOT_EVALUATED,
            "difference": None,
            "needs": "Table 4A on the supplied GSTR-3B — the row 2B populates",
            "available_2b": gstr2b_total,
            "gross_claimed": None,
            "reversed": None,
            "net_claimed": _amount(gstr3b.get("itc_net_4c")),
            "arithmetic_ok": None,
        }

    reversed_total = _or_zero(gstr3b.get("itc_reversed_4b1")) + _or_zero(
        gstr3b.get("itc_reversed_4b2")
    )
    net = _amount(gstr3b.get("itc_net_4c"))

    difference = gross - gstr2b_total
    if difference > 0:
        direction = EXCESS
    elif difference < 0:
        direction = UNCLAIMED
    else:
        direction = AGREES

    return {
        "direction": direction,
        "difference": abs(difference),
        "needs": None,
        "available_2b": gstr2b_total,
        "gross_claimed": gross,
        "reversed": reversed_total,
        "net_claimed": net,
        # 4C = 4A - 4B is the return's own arithmetic. A filed return failing
        # it was mis-keyed, and every figure downstream of it is wrong — worth
        # saying before anyone reads the comparison above.
        "arithmetic_ok": None if net is None else net == gross - reversed_total,
    }


#: Rule 37 outcomes. `nothing_due` and `reversed` both mean "no exposure" and
#: are still distinct: one says the rule had nothing to act on, the other says
#: it acted. A reviewer asking "did we handle the 180-day cases" needs to know
#: which.
NOTHING_DUE = "nothing_due"
REVERSED = "reversed"
PARTIALLY_REVERSED = "partially_reversed"
NOT_REVERSED = "not_reversed"


def rule_37_reversal_check(
    *, overdue_tax: Decimal, gstr3b: dict[str, Any] | None
) -> dict[str, Any]:
    """Whether the Rule 37 reversal the engine says is due was actually made.

    **This is where a finding becomes an exposure.** `gst:PaymentOverdue`
    already reports invoices unpaid past 180 days, and until a GSTR-3B is read
    that is where it stops — the product says credit must be reversed and has
    no way to know whether it was. Table 4B(2) is where a Rule 37 reversal is
    reported, so this closes the loop: an unreversed overdue credit is sitting
    in a **filed** return, not on a to-do list.

    `overdue_tax` is the tax on every invoice the engine flagged overdue.

    Two absences are treated identically and deliberately: no return, and a
    return with a blank 4B(2). Neither is evidence that a reversal was skipped,
    and reporting either as a failure accuses a taxpayer on the strength of
    data nobody supplied.
    """
    if overdue_tax <= 0:
        return {"status": NOTHING_DUE, "shortfall": Decimal("0"), "reversed": None}

    reversed_4b2 = _amount(gstr3b.get("itc_reversed_4b2")) if gstr3b else None
    if reversed_4b2 is None:
        return {
            "status": NOT_EVALUATED,
            "shortfall": None,
            "reversed": None,
            "needs": "Table 4B(2) on this period's GSTR-3B — where a Rule 37 "
            "reversal is reported",
        }

    # 4B(2) also carries s.16(2)(b)/(c) reversals, so a figure larger than the
    # overdue tax is ordinary rather than suspicious — and must not surface as
    # negative exposure.
    shortfall = max(overdue_tax - reversed_4b2, Decimal("0"))
    if shortfall == 0:
        status = REVERSED
    elif reversed_4b2 > 0:
        status = PARTIALLY_REVERSED
    else:
        status = NOT_REVERSED

    return {"status": status, "shortfall": shortfall, "reversed": reversed_4b2}


__all__ = [
    "AGREES",
    "EXCESS",
    "NOTHING_DUE",
    "NOT_EVALUATED",
    "NOT_REVERSED",
    "PARTIALLY_REVERSED",
    "REVERSED",
    "UNCLAIMED",
    "compare_2b_to_3b",
    "rule_37_reversal_check",
]
