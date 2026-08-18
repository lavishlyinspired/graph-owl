"""Data problems are reported when the file lands, not silently at rule time.

A payment row with no date is skipped — correctly, because an event with no
time cannot answer "how many days apart", and treating it as never-paid would
manufacture a reversal the client does not owe. But it was skipped *quietly*.
The row simply never became a fact, and nothing told the person who uploaded
the file that seven of their eight payments would not be counted.

That is the same failure the three-state rule outcome exists to prevent, one
layer earlier: silence about data that was discarded reads exactly like data
that was fine.

Every issue here is a *warning*, never a rejection. A file with problems is
still the best information available, and refusing it would leave a reviewer
with nothing.
"""

from __future__ import annotations

import pytest

from app.data_quality import inspect_rows


def _books(**kw):
    row = {
        "invoice_no": "INV-1", "supplier_gstin": "27AABCS1429B1Z8",
        "supplier_name": "Sharma Infra", "invoice_date": "01-03-2026",
        "taxable": 100000, "igst": 18000,
    }
    row.update(kw)
    return row


class TestMissingRequiredValues:
    def test_a_payment_row_with_no_date_is_reported(self):
        issues = inspect_rows([{"invoice_no": "INV-1", "supplier_gstin": "27A", "payment_date": ""}], "payments")

        codes = {i["code"] for i in issues}
        assert "missing_payment_date" in codes

    def test_the_report_says_how_many_rows_and_what_it_costs(self):
        rows = [{"invoice_no": f"INV-{n}", "supplier_gstin": "27A", "payment_date": ""} for n in range(3)]

        issue = next(i for i in inspect_rows(rows, "payments") if i["code"] == "missing_payment_date")

        assert issue["rows"] == 3
        assert "180" in issue["detail"] or "Rule 37" in issue["detail"]

    def test_rows_with_dates_are_not_reported(self):
        issues = inspect_rows(
            [{"invoice_no": "INV-1", "supplier_gstin": "27A", "payment_date": "01-04-2026"}], "payments"
        )

        assert not any(i["code"] == "missing_payment_date" for i in issues)

    def test_a_grn_row_with_no_receipt_date_is_reported(self):
        issues = inspect_rows([{"invoice_no": "INV-1", "supplier_gstin": "27A", "receipt_date": None}], "grn")

        assert any(i["code"] == "missing_receipt_date" for i in issues)

    def test_an_invoice_row_with_no_invoice_number_is_reported(self):
        issues = inspect_rows([_books(invoice_no="")], "books")

        assert any(i["code"] == "missing_invoice_no" for i in issues)

    def test_a_row_with_no_supplier_gstin_is_reported(self):
        issues = inspect_rows([_books(supplier_gstin="")], "books")

        assert any(i["code"] == "missing_supplier_gstin" for i in issues)


class TestSuspectValues:
    def test_an_unparseable_date_is_reported_separately_from_a_missing_one(self):
        """"Not supplied" and "supplied but unreadable" need different fixes:
        one is a gap in the source system, the other a format the mapping got
        wrong."""
        issues = inspect_rows([_books(invoice_date="2026/13/45")], "books")

        codes = {i["code"] for i in issues}
        assert "unparseable_date" in codes
        assert "missing_invoice_date" not in codes

    def test_a_malformed_gstin_is_reported(self):
        """A GSTIN is 15 characters. A 10-character one will never match a
        portal row, and the reconciliation would report it as unfiled."""
        issues = inspect_rows([_books(supplier_gstin="27AABCS142")], "books")

        assert any(i["code"] == "malformed_gstin" for i in issues)

    def test_a_valid_gstin_is_not_reported(self):
        assert not any(i["code"] == "malformed_gstin" for i in inspect_rows([_books()], "books"))

    def test_a_duplicate_invoice_within_one_file_is_reported(self):
        """Two rows for one invoice are aggregated as rate lines. That is right
        for a multi-rate invoice and wrong for an accidental double-entry, and
        only the person who exported the file can tell which."""
        issues = inspect_rows([_books(), _books()], "books")

        assert any(i["code"] == "duplicate_invoice" for i in issues)

    def test_a_non_numeric_amount_is_reported(self):
        issues = inspect_rows([_books(taxable="N/A")], "books")

        assert any(i["code"] == "non_numeric_amount" for i in issues)


class TestShape:
    def test_a_clean_file_reports_nothing(self):
        assert inspect_rows([_books()], "books") == []

    def test_every_issue_names_an_example_row_so_it_can_be_found(self):
        rows = [_books(invoice_no="GOOD"), _books(invoice_no="")]

        for issue in inspect_rows(rows, "books"):
            assert "example_row" in issue
            assert isinstance(issue["example_row"], int)

    def test_every_issue_has_a_severity(self):
        """A missing payment date silently disables a statutory check; a
        duplicate is worth a look. Ranking them lets the screen lead with the
        one that costs money."""
        rows = [_books(invoice_no=""), _books(), _books()]

        for issue in inspect_rows(rows, "books"):
            assert issue["severity"] in {"blocking", "warning"}

    def test_an_empty_file_is_reported_rather_than_silently_accepted(self):
        issues = inspect_rows([], "books")

        assert any(i["code"] == "empty_file" for i in issues)


def test_a_missing_date_is_not_reported_as_an_unreadable_one():
    """The real negative for the missing-vs-unreadable split.

    An earlier version of this file asserted the distinction against a code
    that is never emitted for that kind, so it passed however the code
    behaved. Conflating the two sends a reviewer to fix a date format when
    the column is simply empty.
    """
    issues = inspect_rows(
        [{"invoice_no": "INV-1", "supplier_gstin": "27AABCS1429B1Z8", "payment_date": ""}],
        "payments",
    )

    codes = {i["code"] for i in issues}
    assert "missing_payment_date" in codes
    assert "unparseable_date" not in codes
