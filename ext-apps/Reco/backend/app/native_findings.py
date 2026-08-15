"""The UI's primary reconciliation source — built from graph-owl's native
rule engine's findings, not reconciliation.py's own tolerance/matching math.

plans/119-architecture-audit.md §9. reconciliation.py's dual-path role
(main.py's `_run_graphowl_reconcile`) was always described as provisional —
"the native engine runs ALONGSIDE reconciliation.py here, not instead of
it — cutover... only happens once parity is actually demonstrated" — and
parity was demonstrated for GSTR-2A/2B/books three-way reconciliation
(scripts/verify-reconcile-parity.py). This module is that cutover: the same
result shape `reconciliation.reconcile()` produced (so `overview()` and
every frontend page need no changes at all), but each row's `status`/
`reason` comes from what packs/gst's SPARQL rules actually found, not from
a client-side amount comparison the native engine has since made obsolete
— law-driven tolerance, head-wise comparison, and GSTR-1/2A awareness none
of which reconciliation.py ever had.

**Findings attach to a row by (gstin, invoice_no), not by any one query's
own subject convention.** Every finding query in packs/gst projects
`?gstin`/`?number` as evidence (confirmed by reading all 13 queries) even
though the *first* projected variable — the finding's own `subject` field
— differs by rule (a books IRI for most rules, a GSTR-1 IRI for
`MissingInBooks`, since that rule has no books row to anchor on). Keying
on the evidence pair instead of `subject` is what lets one function handle
every label uniformly instead of special-casing which side each rule
anchors on.

**One invoice can carry more than one finding** — confirmed live: an
invoice both filed-but-absent-from-2B (`Gstr1NotIn2b`) and genuinely
mismatched against what was filed (`BooksGstr1Mismatch`) gets both. Picked
by priority (`_STATUS_PRIORITY`): a value disagreement always outranks a
"not yet available" finding, because it is the more actionable fact — a
reviewer needs to know the numbers disagree before they need to know 2B
hasn't caught up yet. All matching findings' reasons are still shown,
joined, so nothing is silently dropped for losing the priority ordering.
"""

from __future__ import annotations

from .reconciliation import (
    REASON_AMOUNT_DIFF,
    REASON_NOT_IN_2B,
    REASON_NOT_IN_BOOKS,
    STATUS_MATCHED,
    STATUS_ONLY_BOOKS,
    STATUS_ONLY_GSTR2B,
    STATUS_REVIEW,
    _row_view,
    normalize_gstin,
    normalize_invoice_no,
    record_tax,
    to_float,
)

#: label -> (status bucket, reason shown in the table). The 4 buckets are
#: reconciliation.py's own taxonomy, kept because the frontend's filter
#: tabs and status-pill styling are hardcoded to exactly these 4 — a
#: bigger taxonomy is a frontend change this cutover does not make.
_STATUS_BY_LABEL: dict[str, tuple[str, str]] = {
    "gst:PotentialMismatch": (STATUS_ONLY_BOOKS, REASON_NOT_IN_2B),
    "gst:SupplierNotFiled": (STATUS_ONLY_BOOKS, "Supplier Not Filed (GSTR-1)"),
    "gst:Gstr1NotIn2b": (STATUS_ONLY_BOOKS, "Filed (GSTR-1), Not Yet in GSTR-2B"),
    "gst:MissingInBooks": (STATUS_ONLY_GSTR2B, "Declared (GSTR-1), Not in Books"),
    "gst:AmountMismatch": (STATUS_REVIEW, REASON_AMOUNT_DIFF),
    "gst:TaxHeadMismatch": (STATUS_REVIEW, "Tax Head Mismatch"),
    "gst:BooksGstr1Mismatch": (STATUS_REVIEW, "Books vs GSTR-1 Amount Mismatch"),
    "gst:ITCNotAvailable": (STATUS_REVIEW, "ITC Not Available"),
    "gst:Reversed": (STATUS_REVIEW, "Reverse Charge"),
    "gst:GstinTransposition": (STATUS_REVIEW, "Possible GSTIN Keying Error"),
    "gst:SupplierPanMismatch": (STATUS_REVIEW, "Different State Registration, Same PAN"),
    "gst:GoodsReceiptTiming": (STATUS_REVIEW, "Goods Receipt Timing"),
    "gst:PaymentOverdue": (STATUS_REVIEW, "Payment Overdue (180 Days)"),
}

#: Status priority when one invoice carries multiple findings — lower
#: index wins. A value disagreement is always the more actionable fact.
_STATUS_PRIORITY = [STATUS_REVIEW, STATUS_ONLY_BOOKS, STATUS_ONLY_GSTR2B]


def _finding_key(finding: dict) -> tuple[str, str] | None:
    gstin = number = None
    for entry in finding.get("evidence", []):
        var = entry.get("var")
        if var == "gstin":
            gstin = entry.get("value")
        elif var == "number":
            number = entry.get("value")
    if not gstin or not number:
        return None
    return (normalize_gstin(gstin), normalize_invoice_no(number))


def _findings_by_key(findings: list[dict]) -> dict[tuple[str, str], list[dict]]:
    by_key: dict[tuple[str, str], list[dict]] = {}
    for finding in findings:
        key = _finding_key(finding)
        if key is None:
            continue
        by_key.setdefault(key, []).append(finding)
    return by_key


def _status_and_reason(matches: list[dict]) -> tuple[str, str]:
    resolved = [
        _STATUS_BY_LABEL[f["label"]] for f in matches if f["label"] in _STATUS_BY_LABEL
    ]
    if not resolved:
        return STATUS_MATCHED, ""
    resolved.sort(key=lambda pair: _STATUS_PRIORITY.index(pair[0]))
    status = resolved[0][0]
    reasons = list(dict.fromkeys(reason for _, reason in resolved))
    return status, "; ".join(reasons)


def _key(record: dict) -> tuple[str, str]:
    return (
        normalize_gstin(record.get("supplier_gstin", "")),
        normalize_invoice_no(record.get("invoice_no", "")),
    )


def reconcile(
    books: list[dict],
    portal: list[dict],
    gstr1: list[dict],
    findings: list[dict],
) -> list[dict]:
    """Books, GSTR-2B and GSTR-1/2A rows (main.py's `_normalize` output for
    each), overlaid with what graph-owl's native engine found. Same result
    shape as `reconciliation.reconcile()`.
    """
    findings_by_key = _findings_by_key(findings)
    portal_by_key = {_key(row): row for row in portal}
    gstr1_by_key = {_key(row): row for row in gstr1}
    seen_portal: set[tuple[str, str]] = set()
    seen_gstr1: set[tuple[str, str]] = set()

    results: list[dict] = []

    for book in books:
        key = _key(book)
        matched_portal = portal_by_key.get(key)
        if matched_portal is not None:
            seen_portal.add(key)
        if key in gstr1_by_key:
            seen_gstr1.add(key)

        matches = findings_by_key.get(key, [])
        status, reason = _status_and_reason(matches)

        taxable_book = to_float(book.get("taxable", 0))
        tax_book = record_tax(book)
        taxable_portal = to_float(matched_portal.get("taxable", 0)) if matched_portal else 0.0
        tax_portal = record_tax(matched_portal) if matched_portal else 0.0

        if status == STATUS_MATCHED and matched_portal is None:
            # No finding named this invoice, and it has no 2B counterpart
            # either — the native engine's own rules did not flag it (most
            # often reverse-charge, which every "not filed" rule excludes
            # on purpose), so there is nothing actionable to report. Kept
            # as its own row rather than dropped, matching
            # reconciliation.py's own behaviour for the same case.
            status, reason = STATUS_MATCHED, "No Finding"

        diff = None
        tax_diff = 0.0
        if matched_portal is not None:
            tax_diff = round(abs(tax_portal - tax_book), 2)
            if status == STATUS_REVIEW:
                diff = max(abs(taxable_portal - taxable_book), tax_diff)

        results.append(
            {
                "status": status,
                "reason": reason,
                "diff": diff,
                "tax_diff": tax_diff,
                "itc": round(tax_book, 2),
                "book": _row_view(book, taxable_book, tax_book),
                "portal": _row_view(matched_portal, taxable_portal, tax_portal)
                if matched_portal
                else None,
            }
        )

    for row in portal:
        key = _key(row)
        if key in seen_portal:
            continue
        taxable_portal = to_float(row.get("taxable", 0))
        tax_portal = record_tax(row)
        matches = findings_by_key.get(key, [])
        status, reason = _status_and_reason(matches)
        if status == STATUS_MATCHED:
            status, reason = STATUS_ONLY_GSTR2B, REASON_NOT_IN_BOOKS
        results.append(
            {
                "status": status,
                "reason": reason,
                "diff": None,
                "tax_diff": 0.0,
                "itc": round(tax_portal, 2),
                "book": None,
                "portal": _row_view(row, taxable_portal, tax_portal),
            }
        )

    for row in gstr1:
        key = _key(row)
        if key in seen_gstr1:
            continue
        matches = findings_by_key.get(key, [])
        if not any(f["label"] == "gst:MissingInBooks" for f in matches):
            # A GSTR-1 row not already covered by a books row and not the
            # subject of MissingInBooks is a row no rule has anything to
            # say about yet (e.g. it shares a period with a filing that
            # covers other invoices) — not a row this table represents.
            continue
        taxable = to_float(row.get("taxable", 0))
        tax = record_tax(row)
        _, reason = _status_and_reason(matches)
        results.append(
            {
                "status": STATUS_ONLY_GSTR2B,
                "reason": reason,
                "diff": None,
                "tax_diff": 0.0,
                "itc": round(tax, 2),
                "book": None,
                "portal": _row_view(row, taxable, tax),
            }
        )

    return results


__all__ = ["reconcile"]
