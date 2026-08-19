"""What is actually wrong with *this* invoice, in one sentence.

The pack's `[findings.guidance]` says what the **rule** means — generic, and
authoritative. This says what happened to **this invoice**: which two numbers
disagree, by how much, and how much of the invoice that is.

**Computed, not generated, and that is a design decision rather than a
limitation.** Research on AI in Indian tax practice is consistent that models
help on language and document work and create risk on judgement and figures. A
sentence that states an amount must never be able to state a wrong one, and
this one cannot: every figure in it is read from the case.

An LLM may **rephrase** this for fluency, and when it does it goes through
`grounding.ground_draft` — a rephrasing that invents a number is refused and
the computed sentence shown instead. The model can improve the prose and can
never change the facts.
"""

from __future__ import annotations

from typing import Any


def rupees(amount: float | None) -> str:
    """Indian grouping. `toLocaleString('en-IN')`'s output, computed here so
    the narrative reads the same in an email, a PDF and the browser."""
    if amount is None:
        return "—"
    whole = f"{abs(round(amount)):,}"
    # Convert 1,800,000 -> 18,00,000: after the last three digits, group in twos.
    digits = f"{abs(round(amount))}"
    if len(digits) > 3:
        head, tail = digits[:-3], digits[-3:]
        parts = []
        while len(head) > 2:
            parts.insert(0, head[-2:])
            head = head[:-2]
        if head:
            parts.insert(0, head)
        whole = ",".join(parts + [tail])
    return f"{'-' if amount < 0 else ''}₹{whole}"


def _share(difference: float, base: float | None) -> str:
    """The difference as a share of the invoice.

    **₹500 on ₹1,80,000 and ₹500 on ₹600 are the same absolute number and
    completely different problems.** The share is what tells a reviewer whether
    to care, and omitting it makes every small difference look like every large
    one.
    """
    if not base:
        return ""
    pct = abs(difference) / abs(base) * 100
    return f" — {pct:.2g}% of the invoice"


def narrate(case: dict[str, Any]) -> str:
    """One sentence about this case, built only from what the case carries."""
    invoice = case.get("invoice_no") or "this invoice"
    supplier = case.get("supplier_name")
    who = f" from {supplier}" if supplier else ""
    books = case.get("books_amount")
    portal = case.get("portal_amount")
    label = str(case.get("reason_code") or "")

    if books is not None and portal is not None:
        difference = books - portal
        if difference == 0:
            # Two equal figures reaching a mismatch rule is a data problem.
            # Calling it a mismatch would send someone to argue about nothing.
            return (
                f"{invoice}{who}: both sides report {rupees(books)}. "
                "No difference to resolve — check why this was flagged."
            )
        # Which side is higher, named. "They differ" leaves the reader to work
        # out the direction, and the direction decides who is wrong.
        higher = "the portal" if portal > books else "your books"
        return (
            f"{invoice}{who}: your books say {rupees(books)}, the portal says "
            f"{rupees(portal)}. {higher.capitalize()} is higher by "
            f"{rupees(abs(difference))}{_share(difference, books)}."
        )

    amount = books if books is not None else portal
    if amount is None:
        # No amount evidence — the same discipline the working paper applies.
        # A sentence containing a figure it cannot support is the thing this
        # module exists to prevent.
        return f"{invoice}{who}: flagged, with no amount recorded against it."

    if "NotFiled" in label or "PotentialMismatch" in label or "Gstr1NotIn2b" in label:
        return (
            f"{invoice}{who}: {rupees(amount)} of credit is in your books and has "
            "not reached any GSTR-2B."
        )
    if "ITCNotAvailable" in label:
        return (
            f"{invoice}{who}: the portal reports {rupees(amount)} of credit as "
            "blocked — not available to claim."
        )
    if "PaymentOverdue" in label:
        return (
            f"{invoice}{who}: {rupees(amount)} of credit rests on an invoice unpaid "
            "for more than 180 days, so it must be reversed."
        )
    if "GoodsReceipt" in label:
        return (
            f"{invoice}{who}: {rupees(amount)} of credit cannot be taken yet — the "
            "goods arrived after this period."
        )
    if "MissingInBooks" in label:
        return (
            f"{invoice}{who}: the portal shows {rupees(amount)} of credit your books "
            "do not record."
        )
    # A rule added tomorrow must not blank the narrative column.
    return f"{invoice}{who}: flagged, with {rupees(amount)} at stake."


__all__ = ["narrate", "rupees"]
