"""Feeding the rules that were starved.

`packs/gst` registers 13 finding rules. Four fired on real data. The other
nine read predicates the ingestion never wrote — it landed invoice documents
and nothing else — so three statutory checks were dark:

- `gst:PaymentOverdue`  — Rule 37 / s.16(2)(d), ITC reversal when a supplier
  is unpaid 180 days. Needs `gst:PurchaseEvent` and `gst:PaymentEvent`.
- `gst:GoodsReceiptTiming` — s.16(2)(b), no credit before the goods arrive.
  Needs `gst:GoodsReceipt` and a 2B statement carrying `gst:period`.
- `gst:ITCNotAvailable` — s.17(5) blocked credit. Needs `gst:itcAvailable`
  off the 2B.

Every predicate these need is **already declared in `packs/gst/pack.toml`**.
Nothing in the pack changes; this is purely what the ingestion emits.

Both new file kinds are optional. A firm that cannot export a payment ledger
still gets everything else, and the product says which checks are off rather
than reporting a clean result it did not earn.
"""

from __future__ import annotations

import pytest

from app.graphowl_client import rows_to_turtle


def _books(invoice="INV-1", gstin="27AABCS1429B1Z8", **kw):
    row = {
        "invoice_no": invoice, "supplier_gstin": gstin, "supplier_name": "Sharma Infra",
        "invoice_date": "01-03-2026", "taxable": 100000, "igst": 18000,
        "cgst": 0, "sgst": 0, "cess": 0,
    }
    row.update(kw)
    return row


class TestPaymentEvents:
    def test_a_books_row_also_records_when_the_purchase_happened(self):
        """`payment-overdue.sparql` anchors on a `gst:PurchaseEvent`; without
        one it matches nothing however many payments are loaded."""
        turtle = rows_to_turtle([_books()], "books")

        assert "a gst:PurchaseEvent" in turtle
        assert "gst:onInvoice" in turtle
        assert 'gst:atTime "2026-03-01"' in turtle

    def test_a_payment_row_becomes_a_payment_event_on_its_invoice(self):
        rows = [{
            "invoice_no": "INV-1", "supplier_gstin": "27AABCS1429B1Z8",
            "payment_date": "15-09-2026", "taxable": 118000,
        }]

        turtle = rows_to_turtle(rows, "payments")

        assert "a gst:PaymentEvent" in turtle
        assert 'gst:atTime "2026-09-15"' in turtle
        # The event must point at the *books* invoice subject, which is what
        # the rule joins on — not at the canonical subject.
        assert "gst:onInvoice <https://graph-owl.dev/packs/gst#books-27AABCS1429B1Z8-INV-1>" in turtle

    def test_a_payment_with_no_date_is_not_emitted_as_a_dateless_event(self):
        """An event with no time cannot answer "how many days apart", and a
        rule reading it would silently treat it as never paid."""
        rows = [{"invoice_no": "INV-1", "supplier_gstin": "27AABCS1429B1Z8", "payment_date": ""}]

        turtle = rows_to_turtle(rows, "payments")

        assert "a gst:PaymentEvent" not in turtle


class TestGoodsReceipts:
    def test_a_grn_row_becomes_a_goods_receipt_on_its_invoice(self):
        rows = [{
            "invoice_no": "INV-1", "supplier_gstin": "27AABCS1429B1Z8",
            "receipt_date": "05-04-2026",
        }]

        turtle = rows_to_turtle(rows, "grn")

        assert "a gst:GoodsReceipt" in turtle
        assert 'gst:atTime "2026-04-05"' in turtle
        assert "gst:onInvoice <https://graph-owl.dev/packs/gst#books-27AABCS1429B1Z8-INV-1>" in turtle


class TestItcAvailability:
    def test_the_2b_itc_flag_reaches_the_graph(self):
        """`itc-not-available.sparql` filters `?itcAvailable != "Y"`. Without
        the predicate the rule matches nothing and every blocked credit is
        silently treated as claimable."""
        rows = [_books(itc_available="N")]

        turtle = rows_to_turtle(rows, "gstr2b")

        assert 'gst:itcAvailable "N"' in turtle

    def test_a_2b_upload_records_its_own_period(self):
        """`goods-receipt-timing.sparql` compares the receipt date against the
        2B statement's period, reached via `?filed gst:reflectedIn ?statement`."""
        rows = [_books(period="2026-03")]

        turtle = rows_to_turtle(rows, "gstr2b")

        assert "a gst:Gstr2bStatement" in turtle
        assert 'gst:period "2026-03"' in turtle

    def test_one_statement_subject_per_period_not_one_per_invoice(self):
        rows = [_books(invoice="A", period="2026-03"), _books(invoice="B", period="2026-03")]

        turtle = rows_to_turtle(rows, "gstr2b")

        assert turtle.count("a gst:Gstr2bStatement") == 1


class TestOptionality:
    """Both new kinds are optional, and the product must say what that costs."""

    def test_the_new_kinds_are_accepted(self):
        for kind in ("payments", "grn"):
            rows_to_turtle([{"invoice_no": "X", "supplier_gstin": "27A",
                             "payment_date": "01-01-2026", "receipt_date": "01-01-2026"}], kind)

    def test_an_unknown_kind_is_still_refused(self):
        with pytest.raises(ValueError):
            rows_to_turtle([_books()], "not-a-kind")

    def test_checks_disabled_names_the_rules_a_missing_file_switches_off(self):
        from app.main import checks_disabled

        assert "gst:PaymentOverdue" in checks_disabled({"books", "gstr2b"})
        assert "gst:GoodsReceiptTiming" in checks_disabled({"books", "gstr2b"})
        # GSTR-1 is optional too, and its absence disables three more —
        # every check that needs the supplier's own declaration.
        assert "gst:MissingInBooks" in checks_disabled({"books", "gstr2b"})
        # GSTR-2A likewise, and its two are the ones nothing else can answer:
        # a 2B alone cannot say what the portal has reported *since* it froze.
        assert "gst:FiledLateInGstr2a" in checks_disabled({"books", "gstr2b", "gstr1"})
        assert "gst:AmendedAfterClaim" in checks_disabled({"books", "gstr2b", "gstr1"})
        # Uploading 2A must not be mistaken for uploading GSTR-1: they are
        # different authorities and switch on different checks.
        assert "gst:MissingInBooks" in checks_disabled({"books", "gstr2b", "gstr2a"})

        # GSTR-3B switches on the two checks nothing else can answer: whether
        # what was *claimed* matches what the portal gave, and whether the
        # 180-day reversals actually reached the return.
        assert "gst:ItcClaimedVsAvailable" in checks_disabled(
            {"books", "gstr2b", "gstr1", "gstr2a"}
        )
        assert "gst:Rule37ReversalMade" in checks_disabled(
            {"books", "gstr2b", "gstr1", "gstr2a"}
        )

        every_kind = {"books", "gstr2b", "payments", "grn", "gstr1", "gstr2a", "gstr3b"}
        assert checks_disabled(every_kind) == {}

    def test_every_disabled_check_says_why_it_matters(self):
        """A rule label alone tells a reviewer nothing. "Rule 37 — credit must
        be reversed on invoices unpaid for 180 days" tells them what they are
        not being warned about."""
        from app.main import checks_disabled

        for label, reason in checks_disabled({"books"}).items():
            assert reason, f"{label} is disabled with no reason given"

    def test_uploading_payments_switches_its_check_back_on(self):
        from app.main import checks_disabled

        assert "gst:PaymentOverdue" not in checks_disabled({"books", "gstr2b", "payments"})
        assert "gst:GoodsReceiptTiming" in checks_disabled({"books", "gstr2b", "payments"})


class TestAutoMapping:
    """The new columns must bind themselves, or every upload starts with a
    manual mapping the user has no reason to expect."""

    def test_a_payment_ledger_maps_its_own_columns(self):
        from app.main import _auto_map

        headers = ["Invoice No", "Supplier GSTIN", "Supplier Name", "Payment Date", "Amount Paid"]
        mapping = _auto_map(headers)

        assert headers[mapping["payment_date"]] == "Payment Date"
        assert headers[mapping["invoice_no"]] == "Invoice No"

    def test_a_grn_file_maps_its_own_columns(self):
        from app.main import _auto_map

        headers = ["Invoice No", "Supplier GSTIN", "Supplier Name", "Goods Receipt Date", "GRN No"]
        mapping = _auto_map(headers)

        assert headers[mapping["receipt_date"]] == "Goods Receipt Date"

    def test_a_payment_date_does_not_steal_the_invoice_date_column(self):
        """Both contain "date". The specific keyword must win, or a payment
        ledger's date lands as the invoice date and the 180-day test measures
        an interval from the wrong end."""
        from app.main import _auto_map

        headers = ["Invoice No", "Invoice Date", "Payment Date"]
        mapping = _auto_map(headers)

        assert headers[mapping["invoice_date"]] == "Invoice Date"
        assert headers[mapping["payment_date"]] == "Payment Date"


class TestPeriodFallback:
    """The 2B's period comes from the workspace when the file omits it.

    `goods-receipt-timing.sparql` compares the goods-receipt date against the
    2B statement's period. Real GSTR-2B exports frequently carry no "Return
    Period" column — the sample here does not — so requiring one would leave
    s.16(2)(b) permanently dark for most files. The app already knows which
    period the upload belongs to: it is the workspace the user is in.
    """

    def test_the_workspace_period_becomes_the_statement_period(self):
        from app.main import period_label_to_yyyy_mm

        assert period_label_to_yyyy_mm("March", 2026) == "2026-03"
        assert period_label_to_yyyy_mm("December", 2025) == "2025-12"

    def test_an_unknown_month_yields_nothing_rather_than_a_wrong_period(self):
        """A wrong period would silently mis-date every goods-receipt
        comparison. No period at all leaves the rule unfired, which is
        visible."""
        from app.main import period_label_to_yyyy_mm

        assert period_label_to_yyyy_mm("Smarch", 2026) is None
        assert period_label_to_yyyy_mm(None, 2026) is None

    def test_a_period_already_in_the_file_is_not_overwritten(self):
        """A column in the file is a statement of fact about that file, and
        beats an inference from the workspace."""
        from app.main import apply_period_fallback

        rows = [{"invoice_no": "A", "period": "2026-02"}, {"invoice_no": "B"}]

        out = apply_period_fallback(rows, "2026-03")

        assert out[0]["period"] == "2026-02"
        assert out[1]["period"] == "2026-03"
