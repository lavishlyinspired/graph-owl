"""The GSTR-3B working paper — Plan 123 Slice D, and §4's "the deliverable".

**A working paper is not a dashboard.** A dashboard shows a number; a working
paper shows how the number was reached, so someone else — a partner reviewing,
an officer asking — can follow each step and disagree with a *specific* one.
Every line therefore carries its own source, and every statutory deduction
carries the provision that required it. A figure a reviewer cannot trace is a
figure they have to take on trust, which is the thing this document exists to
avoid.

The chain:

    gross ITC available            from GSTR-2B
      − s.17(5) blocked            the portal's own ineligibility flag
      − Rule 42/43 reversal        proportionate, exempt/non-business use
      − Rule 37 reversal           unpaid past 180 days
      = net ITC claimable

and then, separately, what the **filed** GSTR-3B actually said. The gap
between the two is the exposure, and it is deliberately not folded into the
chain — a computed position and a filed position are different claims, and
netting them would hide which one is being asserted.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Any

from .itc_3b import compare_2b_to_3b

#: finding label -> (line key, label, citation). **The single place a finding
#: becomes a deduction**, so a new rule cannot silently change a filed figure:
#: anything absent from here surfaces in `unmodelled` rather than being
#: dropped, because a deduction with no line would make net overstate what is
#: claimable.
DEDUCTIONS: dict[str, tuple[str, str, str]] = {
    "gst:ITCNotAvailable": (
        "blocked_17_5",
        "Blocked credit",
        "s.17(5) — blocked credits, not claimable at all",
    ),
    "gst:PaymentOverdue": (
        "reversal_rule_37",
        "Reversal — supplier unpaid 180 days",
        "Rule 37 read with s.16(2)(d) — reclaimable once paid",
    ),
    "gst:ProportionateReversal": (
        "reversal_rule_42_43",
        "Proportionate reversal",
        "Rules 42/43 — exempt and non-business use",
    ),
    "gst:GoodsReceiptTiming": (
        "timing_16_2_b",
        "Goods not received in this period",
        "s.16(2)(b) — no credit before the goods or services are received",
    ),
}

#: Order the lines appear in. Fixed rather than derived from `DEDUCTIONS`,
#: because a working paper read in a different order each time is not
#: reviewable — and because the order is itself the argument: permanent losses
#: before reclaimable ones, so a reader sees what is gone before what is
#: merely deferred.
LINE_ORDER = [
    "blocked_17_5",
    "reversal_rule_42_43",
    "timing_16_2_b",
    "reversal_rule_37",
]


def _total(rows: list[dict]) -> Decimal:
    return sum((Decimal(str(r.get("tax_amount") or 0)) for r in rows), Decimal("0"))


def build_working_paper(
    *,
    gstr2b: list[dict],
    findings: list[dict],
    gstr3b: dict[str, Any] | None,
) -> dict[str, Any]:
    """The traced chain, plus how it compares to what was filed.

    `findings` are the engine's own, each carrying the tax at stake. A finding
    whose label this module does not model is reported in `unmodelled` and
    **not** deducted — silently dropping it would overstate net, and silently
    deducting it would attribute a reduction to a provision nobody chose.
    """
    gross = _total(gstr2b)

    by_key: dict[str, Decimal] = {}
    #: How many findings landed on each line **without an amount anybody
    #: established**. Counted rather than coerced to zero: a deduction of zero
    #: and a deduction of unknown size are different claims, and zeroing makes
    #: net *overstate* what is claimable — the direction that costs money.
    #:
    #: Found live: `gst:PaymentOverdue`'s evidence carries `atTime` bindings
    #: and no tax amount, so every Rule 37 reversal deducted zero while the
    #: rule itself reported findings.
    unquantified: dict[str, int] = {}
    unmodelled: list[dict[str, Any]] = []
    for finding in findings:
        label = finding.get("label")
        mapped = DEDUCTIONS.get(str(label))
        raw = finding.get("tax_amount")
        amount = None if raw is None or raw == "" else Decimal(str(raw))
        if mapped is None:
            unmodelled.append({"label": label, "amount": amount or Decimal("0")})
            continue
        key = mapped[0]
        if amount is None:
            unquantified[key] = unquantified.get(key, 0) + 1
            continue
        by_key[key] = by_key.get(key, Decimal("0")) + amount

    lines: list[dict[str, Any]] = [
        {
            "key": "gross",
            "kind": "opening",
            "label": "ITC available",
            "amount": gross,
            "unquantified": 0,
            "source": f"GSTR-2B — {len(gstr2b)} line(s) for this period",
            "citation": None,
        }
    ]
    for key in LINE_ORDER:
        label, citation = next(
            (l, c) for k, l, c in DEDUCTIONS.values() if k == key
        )
        lines.append(
            {
                "key": key,
                "kind": "deduction",
                "label": label,
                "amount": by_key.get(key, Decimal("0")),
                "unquantified": unquantified.get(key, 0),
                "source": "graph-owl finding rules for this period",
                "citation": citation,
            }
        )

    deductions = sum((line["amount"] for line in lines if line["kind"] == "deduction"), Decimal("0"))
    lines.append(
        {
            "key": "net",
            "kind": "closing",
            "label": "Net ITC claimable",
            "amount": gross - deductions,
            "unquantified": 0,
            "source": "computed — ITC available less every deduction above",
            "citation": None,
        }
    )

    return {
        "lines": lines,
        "unmodelled": unmodelled,
        # Whether every deduction the chain names could actually be sized. A
        # paper with an unquantified line is still the best available position
        # — it just must not claim to be the final one.
        "complete": not unquantified,
        # Kept separate from the chain on purpose: a computed position and a
        # filed one are different claims, and netting them would hide which
        # is being asserted.
        "filed": compare_2b_to_3b(gstr2b_total=gross, gstr3b=gstr3b),
    }


__all__ = ["DEDUCTIONS", "LINE_ORDER", "build_working_paper"]
