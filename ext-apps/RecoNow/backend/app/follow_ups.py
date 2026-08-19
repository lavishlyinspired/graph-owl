"""Supplier follow-ups, grouped the way you actually send them.

The vendor agent drafts per **invoice**, because that is what a finding names.
You send one email to a **supplier**. A supplier with three unfiled invoices
gets one message listing all three — three separate emails is how a working
relationship gets damaged by software.
"""

from __future__ import annotations

from typing import Any


def group_drafts(*, drafts: list[dict[str, Any]], cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """One card per supplier, largest exposure first.

    A draft whose invoice has no matching case is **dropped** — a stale draft
    from an earlier run must not take the screen with it.
    """
    case_by_invoice = {str(c.get("invoice_no")): c for c in cases}

    groups: dict[str, dict[str, Any]] = {}
    for draft in drafts:
        case = case_by_invoice.get(str(draft.get("invoice_no")))
        if case is None:
            continue
        gstin = str(case.get("supplier_gstin") or "")
        group = groups.setdefault(
            gstin,
            {
                "supplier_gstin": gstin,
                "supplier_name": case.get("supplier_name") or gstin,
                "invoices": [],
                "at_risk": 0.0,
                "message": draft.get("message"),
                "source": draft.get("source"),
            },
        )
        group["invoices"].append(str(draft.get("invoice_no")))
        group["at_risk"] += float(case.get("books_amount") or 0)
        # The weaker claim wins: calling a part-computed message "model" would
        # overstate what was generated, and the reverse understates nothing
        # that matters.
        if draft.get("source") == "computed":
            group["source"] = "computed"

    # Largest exposure first — a list ordered by anything else makes a reviewer
    # read all of it to find the one worth their morning.
    return sorted(groups.values(), key=lambda g: -g["at_risk"])


__all__ = ["group_drafts"]
