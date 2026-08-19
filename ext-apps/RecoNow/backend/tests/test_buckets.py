"""The reconciliation *result*, not just its exceptions.

Reco Now surfaced findings and nothing else, so it could never answer the
first question a CA asks — "how much of this period is done?". A finding is
raised only when something is wrong; a matched invoice produces none, so a
screen built from findings cannot count what matched.

The four buckets are the ones every reconciliation tool states, and the one
distinction that matters more than the buckets:

**ITC pending is not ITC at risk.** Credit on an invoice the supplier has not
yet filed is *deferred* — claimable in a later period once they file. Credit
blocked under s.17(5) is *lost*. Reporting them as one number misstates the
client's position in both directions, and Reco Now reported one number.
"""

from __future__ import annotations

import pytest

from app.reconcile_result import BUCKET_MATCHED, BUCKET_ONLY_BOOKS, BUCKET_ONLY_PORTAL, \
    BUCKET_REVIEW, itc_position, reconcile_buckets


def _row(invoice="INV-1", gstin="27AABCS1429B1Z8", taxable=100000, igst=18000, **kw):
    row = {
        "invoice_no": invoice, "supplier_gstin": gstin, "supplier_name": "Sharma Infra",
        "taxable": taxable, "igst": igst, "cgst": 0, "sgst": 0, "cess": 0,
    }
    row.update(kw)
    return row


def _finding(invoice, label, gstin="27AABCS1429B1Z8"):
    return {"invoice_no": invoice, "reason_code": label, "supplier_gstin": gstin}


class TestBuckets:
    def test_an_invoice_on_both_sides_with_no_finding_is_matched(self):
        result = reconcile_buckets([_row()], [_row()], [])

        assert result.counts[BUCKET_MATCHED] == 1
        assert result.counts[BUCKET_REVIEW] == 0

    def test_an_invoice_on_both_sides_with_a_finding_is_review(self):
        result = reconcile_buckets(
            [_row(taxable=100000)], [_row(taxable=90000)],
            [_finding("INV-1", "gst:AmountMismatch")],
        )

        assert result.counts[BUCKET_REVIEW] == 1
        assert result.counts[BUCKET_MATCHED] == 0

    def test_an_invoice_only_in_books(self):
        result = reconcile_buckets([_row()], [], [])

        assert result.counts[BUCKET_ONLY_BOOKS] == 1

    def test_an_invoice_only_on_the_portal(self):
        result = reconcile_buckets([], [_row()], [])

        assert result.counts[BUCKET_ONLY_PORTAL] == 1

    def test_sides_are_matched_on_the_normalised_key(self):
        result = reconcile_buckets([_row(invoice="INV/2026/1")], [_row(invoice="INV-2026-1")], [])

        assert result.counts[BUCKET_MATCHED] == 1
        assert result.counts[BUCKET_ONLY_BOOKS] == 0

    def test_same_invoice_number_from_two_suppliers_does_not_match(self):
        result = reconcile_buckets(
            [_row(invoice="001", gstin="27AABCS1429B1Z8")],
            [_row(invoice="001", gstin="29AACCS9460D1Z4")],
            [],
        )

        assert result.counts[BUCKET_MATCHED] == 0
        assert result.counts[BUCKET_ONLY_BOOKS] == 1
        assert result.counts[BUCKET_ONLY_PORTAL] == 1

    def test_match_rate_is_matched_over_every_distinct_invoice(self):
        result = reconcile_buckets(
            [_row(invoice="A"), _row(invoice="B"), _row(invoice="C")],
            [_row(invoice="A")],
            [],
        )

        assert result.total == 3
        assert result.match_rate == pytest.approx(1 / 3)

    def test_match_rate_of_an_empty_period_is_zero_not_a_crash(self):
        result = reconcile_buckets([], [], [])

        assert result.total == 0
        assert result.match_rate == 0

    def test_every_invoice_lands_in_exactly_one_bucket(self):
        """The buckets partition the period. If they did not, the counts would
        not add up to the total and every headline number would be suspect."""
        books = [_row(invoice=n) for n in ("A", "B", "C", "D")]
        portal = [_row(invoice=n) for n in ("A", "B", "E")]
        findings = [_finding("B", "gst:AmountMismatch")]

        result = reconcile_buckets(books, portal, findings)

        assert sum(result.counts.values()) == result.total
        assert result.total == 5  # A B C D E


class TestItcPosition:
    def test_matched_credit_is_confirmed(self):
        position = itc_position(reconcile_buckets([_row()], [_row()], []))

        assert position["confirmed"] == pytest.approx(18000)
        assert position["pending"] == 0
        assert position["blocked"] == 0

    def test_credit_the_supplier_has_not_filed_is_pending_not_at_risk(self):
        """The distinction this module exists for. Deferred, not lost."""
        position = itc_position(reconcile_buckets([_row()], [], []))

        assert position["pending"] == pytest.approx(18000)
        assert position["blocked"] == 0
        assert position["confirmed"] == 0

    def test_credit_blocked_under_17_5_is_blocked_not_pending(self):
        position = itc_position(
            reconcile_buckets([_row()], [_row()], [_finding("INV-1", "gst:ITCNotAvailable")])
        )

        assert position["blocked"] == pytest.approx(18000)
        assert position["pending"] == 0
        assert position["confirmed"] == 0

    def test_a_value_mismatch_puts_only_the_difference_under_review(self):
        """The whole invoice is not in doubt — the disagreement is."""
        position = itc_position(
            reconcile_buckets(
                [_row(taxable=100000, igst=18000)],
                [_row(taxable=100000, igst=17000)],
                [_finding("INV-1", "gst:AmountMismatch")],
            )
        )

        assert position["under_review"] == pytest.approx(1000)

    def test_portal_only_credit_is_unclaimed_not_confirmed(self):
        """In the 2B but not in the books: available credit nobody recorded.
        Counting it as confirmed would claim credit for a purchase the client
        has no invoice for."""
        position = itc_position(reconcile_buckets([], [_row()], []))

        assert position["confirmed"] == 0
        assert position["unclaimed"] == pytest.approx(18000)

    def test_every_rupee_is_in_exactly_one_class(self):
        books = [_row(invoice="A"), _row(invoice="B"), _row(invoice="C")]
        portal = [_row(invoice="A"), _row(invoice="D")]
        findings = [_finding("A", "gst:ITCNotAvailable")]

        result = reconcile_buckets(books, portal, findings)
        position = itc_position(result)

        assert position["total_considered"] == pytest.approx(
            position["confirmed"] + position["pending"] + position["blocked"]
            + position["under_review"] + position["unclaimed"]
        )


class TestObservedDifference:
    """Bucketing must not depend on a rule having fired.

    Found live: INV-MAR-003 showed books 54,000 against portal 42,000 — a
    12,000 difference — and was bucketed *matched*, because no finding existed
    for it. Deriving "matched" from the absence of a finding trusts every rule
    to have run and to have had the data it needs. Nine of thirteen rules in
    this pack are currently starved of input, so that trust is misplaced, and
    the failure is silent and reads as success.

    A difference is observable directly. The finding explains *why* there is
    one; it is not what makes it true.
    """

    def test_a_value_difference_is_review_even_with_no_finding(self):
        result = reconcile_buckets(
            [_row(invoice="INV-MAR-003", taxable=54000)],
            [_row(invoice="INV-MAR-003", taxable=42000)],
            [],
        )

        assert result.counts[BUCKET_REVIEW] == 1
        assert result.counts[BUCKET_MATCHED] == 0

    def test_a_tax_difference_alone_is_review(self):
        """Taxable can agree while the tax does not — a wrong rate applied."""
        result = reconcile_buckets(
            [_row(taxable=100000, igst=18000)],
            [_row(taxable=100000, igst=12000)],
            [],
        )

        assert result.counts[BUCKET_REVIEW] == 1

    def test_a_sub_rupee_difference_is_still_matched(self):
        """The same de-minimis floor `amount-mismatch.sparql` documents: GSTR-3B
        is filed in whole rupees, so a sub-rupee difference cannot change what
        is claimed, and a queue full of one-paisa findings is one a reviewer
        stops reading."""
        result = reconcile_buckets(
            [_row(taxable="100000.40")], [_row(taxable="100000.00")], []
        )

        assert result.counts[BUCKET_MATCHED] == 1

    def test_an_exact_match_is_still_matched(self):
        result = reconcile_buckets([_row()], [_row()], [])

        assert result.counts[BUCKET_MATCHED] == 1


class TestFindingsWithoutGstin:
    """A finding that omits the supplier must still reach its invoice.

    Found live: `gst:ITCNotAvailable` binds `number` and `taxAmount` as
    evidence but not `gstin`, so its cases carried no supplier GSTIN. Matching
    on `(gstin, invoice)` then failed silently — two invoices with blocked
    credit under s.17(5) were bucketed *matched* and their blocked ITC
    reported as zero. The label was produced correctly and thrown away at the
    join.

    Falling back to the invoice number alone is safe only where that number is
    unambiguous in the period. Where two suppliers share one, guessing would
    attach a blocked-credit finding to the wrong supplier's invoice, which is
    worse than not attaching it.
    """

    def test_a_finding_with_no_gstin_still_matches_its_invoice(self):
        result = reconcile_buckets(
            [_row(invoice="INV-MAR-006")],
            [_row(invoice="INV-MAR-006")],
            [{"invoice_no": "INV-MAR-006", "reason_code": "gst:ITCNotAvailable",
              "supplier_gstin": None}],
        )

        # **Updated when the bucket stopped being decided by findings.** This
        # test is about a finding *attaching* to its invoice when the GSTIN is
        # missing — the bucket assertion was incidental, and asserted the old
        # conflation: both sides report identical figures here, so the invoice
        # is Matched. The credit being blocked is a separate statement, and it
        # is the one this test actually cares about.
        assert result.counts[BUCKET_MATCHED] == 1
        assert result.rows[0]["labels"] == ["gst:ITCNotAvailable"]
        assert result.rows[0]["blocked"] is True

    def test_blocked_credit_is_quantified_when_the_finding_omits_the_gstin(self):
        position = itc_position(
            reconcile_buckets(
                [_row(invoice="INV-MAR-006", igst=55800)],
                [_row(invoice="INV-MAR-006", igst=55800)],
                [{"invoice_no": "INV-MAR-006", "reason_code": "gst:ITCNotAvailable",
                  "supplier_gstin": ""}],
            )
        )

        assert position["blocked"] == pytest.approx(55800)

    def test_an_ambiguous_invoice_number_is_not_guessed_at(self):
        """Two suppliers, one invoice number, a finding naming neither.
        Attaching it to either would be a coin flip on whose credit is
        blocked."""
        result = reconcile_buckets(
            [_row(invoice="001", gstin="27AABCS1429B1Z8"),
             _row(invoice="001", gstin="29AACCS9460D1Z4")],
            [_row(invoice="001", gstin="27AABCS1429B1Z8"),
             _row(invoice="001", gstin="29AACCS9460D1Z4")],
            [{"invoice_no": "001", "reason_code": "gst:ITCNotAvailable", "supplier_gstin": None}],
        )

        assert all(not r["blocked"] for r in result.rows)

    def test_a_finding_that_does_carry_a_gstin_still_matches_exactly(self):
        result = reconcile_buckets(
            [_row(invoice="001", gstin="27AABCS1429B1Z8"),
             _row(invoice="001", gstin="29AACCS9460D1Z4")],
            [_row(invoice="001", gstin="27AABCS1429B1Z8"),
             _row(invoice="001", gstin="29AACCS9460D1Z4")],
            [_finding("001", "gst:ITCNotAvailable", gstin="29AACCS9460D1Z4")],
        )

        blocked = [r for r in result.rows if r["blocked"]]
        assert len(blocked) == 1
        assert blocked[0]["supplier_gstin"] == "29AACCS9460D1Z4"
