"""GSTR-9 Table 8 — the annual ITC reconciliation. Plan 123 Slice C.

Table 8 asks one question across a whole financial year: **the portal said
this much credit was available; how much did you actually take, and where did
the rest go?** 8A is the portal's figure, 8B is yours, 8D is the gap, and 8E/8F
split that gap into credit still claimable and credit lost.

**8A is defined against GSTR-2A, not GSTR-2B**, which is why this module
belongs to the 2A slice. 2B freezes on the 14th; 2A keeps moving. Over a year
the difference is every invoice a supplier filed late — a deployment holding
only 2B cannot compute 8A at all, and one that substituted 2B would under-report
the credit available and overstate its own compliance.

**Rows this deployment's data cannot support are reported as uncomputed, never
as zero.** A GSTR-9 is filed. A zero nobody derived is worse than a blank,
because a blank prompts a question and a zero closes one. Each uncomputed row
names the dataset that would close it, so a preparer knows what to go and get
rather than being told only that the cell is empty.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Any

#: Rows that need a dataset this product does not ingest, and what each needs.
#: Kept as data rather than scattered through the computation so that adding a
#: source is one edit — and so the list of what is *missing* is readable on its
#: own, which is the thing a preparer actually asks for.
UNSUPPORTED: dict[str, str] = {
    "8B": "GSTR-3B — ITC availed as filed, which this product does not ingest",
    "8C": "GSTR-3B for the following April–September, for credit availed next year",
    "8G": "customs data — IGST paid on import of goods",
    "8H": "customs data — IGST credit availed on import",
    "8I": "customs data — the 8G/8H difference",
    "8J": "customs data — import credit available but not availed",
    "8K": "8E, 8F and 8J together, so it waits on the import rows",
}


def _computed(value: Decimal) -> dict[str, Any]:
    return {"value": value, "computed": True, "needs": None}


def _uncomputed(needs: str) -> dict[str, Any]:
    return {"value": None, "computed": False, "needs": needs}


def _total(lines: list[dict], predicate=lambda line: True) -> Decimal:
    return sum(
        (Decimal(str(line.get("tax_amount") or 0)) for line in lines if predicate(line)),
        Decimal("0"),
    )


def _is_eligible(line: dict) -> bool:
    """Absent means eligible. The portal marks what it blocks; it does not
    stamp every ordinary line as allowed, so treating an absent flag as
    ineligible would report the whole year's credit as lost."""
    flag = line.get("itc_available")
    return flag is None or str(flag).strip().upper() != "N"


def table8(*, gstr2a: list[dict], availed: list[dict]) -> dict[str, dict[str, Any]]:
    """Table 8, row by row, each carrying whether it could be computed at all.

    `gstr2a` is every 2A line for the year — the portal's own record.
    `availed` is what the books actually claimed. An **empty** `availed` means
    no figure was supplied, not that nothing was claimed: 8D against it would
    report the entire year's credit as unclaimed, which is why the difference
    rows go uncomputed rather than confidently wrong.
    """
    available = _total(gstr2a)
    ineligible = _total(gstr2a, lambda line: not _is_eligible(line))

    rows: dict[str, dict[str, Any]] = {
        "8A": _computed(available),
        **{label: _uncomputed(needs) for label, needs in UNSUPPORTED.items()},
    }

    if not availed:
        no_figure = "a books ITC figure for the year — none was supplied"
        rows["8D"] = _uncomputed(no_figure)
        rows["8E"] = _uncomputed(no_figure)
        rows["8F"] = _uncomputed(no_figure)
        return rows

    claimed = _total(availed)
    difference = available - claimed

    rows["8D"] = _computed(difference)
    # 8F is credit the portal itself blocked — lost, not deferred. 8E is the
    # remainder of the gap, which can still be claimed. Reporting one number
    # for both would tell a preparer that lost credit is recoverable.
    rows["8F"] = _computed(ineligible)
    rows["8E"] = _computed(difference - ineligible)
    return rows


__all__ = ["UNSUPPORTED", "table8"]
