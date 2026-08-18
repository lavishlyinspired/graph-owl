"""An invoice with no portal counterpart is not "matched".

**Found 19 August 2026 by hand-deriving the sample fixture's answer key**
while investigating why `test_characterisation.py` had been failing. The
characterisation pin was right and the code had regressed: three invoices the
supplier never filed (AMZ/2024/567, HCL/2024/077, WIP/2024/118 — present in
`sample_data.books_rows()`, absent from `gstr2b_rows()`) were being reported
as **matched**.

`native_findings.reconcile` defaults an invoice's status to matched when no
finding names it, then keeps that default even where there is demonstrably no
2B row to have matched *against*. The comment defended it on reverse-charge
grounds — under RCM the recipient self-assesses, so no supplier line is
expected — but that reasoning only covers invoices actually flagged as
reverse-charge. For every other case it converts "no rule fired" into "both
sides agree", which is this product's central failure mode: a check that never
ran reading exactly like a check that passed.

The direction matters. Reporting an unfiled invoice as matched tells a CA
their credit is safe when the supplier has filed nothing, which is the error
that costs money rather than time.
"""

from __future__ import annotations

from app.native_findings import reconcile
from app.reconciliation import STATUS_MATCHED, STATUS_ONLY_BOOKS, STATUS_ONLY_GSTR2B


def _book(number: str, **kw) -> dict:
    row = {
        "invoice_no": number,
        "supplier_gstin": "27AABCU9603R1ZM",
        "supplier_name": "Tata Steel Ltd",
        "taxable": 100000,
        "igst": 18000,
        "cgst": 0,
        "sgst": 0,
    }
    row.update(kw)
    return row


def _statuses(rows: list[dict]) -> list[str]:
    return [r["status"] for r in rows]


def test_an_invoice_with_no_portal_row_and_no_finding_is_only_books():
    """The regression itself. No 2B row exists, so nothing matched."""
    rows = reconcile([_book("AMZ/2024/567")], portal=[], gstr1=[], findings=[])

    assert _statuses(rows) == [STATUS_ONLY_BOOKS]


def test_an_invoice_with_a_portal_row_and_no_finding_is_matched():
    """The other direction, so the fix cannot be "call everything only_books".
    Both sides present and no rule complained is a genuine match."""
    book = _book("INV-2024-001")
    portal = [dict(book)]

    rows = reconcile([book], portal=portal, gstr1=[], findings=[])

    assert _statuses(rows) == [STATUS_MATCHED]


def test_a_reverse_charge_invoice_with_no_portal_row_stays_matched():
    """The case the old comment was actually right about. Under RCM the
    recipient self-assesses and no supplier line is ever expected, so an
    absent 2B row is not a failure — flagging it would manufacture work on
    every RCM invoice a firm holds."""
    rows = reconcile(
        [_book("RCM/2024/001", reverse_charge="Y")], portal=[], gstr1=[], findings=[]
    )

    assert _statuses(rows) == [STATUS_MATCHED]


def test_the_sample_fixture_reaches_its_hand_derived_answer():
    """Derived from `sample_data` by hand, not read off the code: 10 distinct
    books invoices, 8 distinct 2B, 7 in both, of which RI-7890 differs on tax
    (13,500 books against 13,680 portal). So 6 matched, 1 review, 3 only-books,
    1 only-2B — which is exactly what the characterisation pin has always
    said."""
    from app import sample_data
    from app.main import _auto_map, _build_dataset, _normalize

    books_ds = _build_dataset(sample_data.books_rows(), "Books", "books")
    portal_ds = _build_dataset(sample_data.gstr2b_rows(), "Portal", "gstr2b")
    books = _normalize(books_ds, _auto_map(books_ds["headers"]))
    portal = _normalize(portal_ds, _auto_map(portal_ds["headers"]))

    rows = reconcile(books, portal=portal, gstr1=[], findings=[])

    counts: dict[str, int] = {}
    for row in rows:
        counts[row["status"]] = counts.get(row["status"], 0) + 1

    # 10 books invoices plus the one the portal carries and the books do not.
    assert len(rows) == 11, counts
    assert counts.get(STATUS_ONLY_BOOKS) == 3, counts
    assert counts.get(STATUS_ONLY_GSTR2B) == 1, counts
    # 7, not the pin's 6, and that is correct here: the native path delegates
    # amount-mismatch detection to the engine's own rule, and this call passes
    # no findings. RI-7890's 180-rupee difference becomes a review only when
    # `gst:AmountMismatch` names it — the case below.
    assert counts.get(STATUS_MATCHED) == 7, counts


def test_an_amount_mismatch_finding_moves_its_invoice_out_of_matched():
    """The other half of the fixture's answer key. Without this the test above
    would pass against a build that had lost review detection entirely."""
    from app import sample_data
    from app.main import _auto_map, _build_dataset, _normalize
    from app.reconciliation import STATUS_REVIEW

    books_ds = _build_dataset(sample_data.books_rows(), "Books", "books")
    portal_ds = _build_dataset(sample_data.gstr2b_rows(), "Portal", "gstr2b")
    books = _normalize(books_ds, _auto_map(books_ds["headers"]))
    portal = _normalize(portal_ds, _auto_map(portal_ds["headers"]))

    finding = {
        "label": "gst:AmountMismatch",
        "evidence": [
            {"var": "gstin", "value": "27AABCR7890P1Z1"},
            {"var": "number", "value": "RI-7890"},
        ],
    }
    gstin = next(b["supplier_gstin"] for b in books if b["invoice_no"] == "RI-7890")
    finding["evidence"][0]["value"] = gstin

    rows = reconcile(books, portal=portal, gstr1=[], findings=[finding])

    by_number = {r["book"]["invoice_no"]: r["status"] for r in rows if r.get("book")}
    assert by_number["RI-7890"] == STATUS_REVIEW, by_number
    assert sum(1 for s in by_number.values() if s == STATUS_MATCHED) == 6, by_number
