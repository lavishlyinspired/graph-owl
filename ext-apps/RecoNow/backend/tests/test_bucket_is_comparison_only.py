"""The bucket says whether the records agree. The finding says what it means.

**The architectural point, and a real conflation it was hiding.**
`reconcile_buckets` decided the bucket as:

    BUCKET_REVIEW if (labels or _differs(...)) else BUCKET_MATCHED

so **any** finding forced the bucket to Review. A perfectly matched invoice
carrying a s.17(5) blocked-credit finding was reported as a *value
disagreement*, which it is not: both sides agree on every figure, and the
credit is unavailable for a reason that has nothing to do with the comparison.

That made the two screens genuinely redundant — bucket and findings were
perfectly correlated on real data, so Reconciliation could only ever restate
what Findings already said.

The split:

- **Reconciliation** answers *do these records agree?* — decided by the
  comparison alone.
- **Findings** answer *what does this mean, and must someone act?* — decided
  by the rules, and attached to whatever bucket the invoice landed in.

A matched invoice with a finding is the case that proves they are different
questions, and it is common: blocked credit, goods not yet received, a
supplier unpaid past 180 days. Every one of those agrees perfectly with the
portal and none of them is claimable.
"""

from __future__ import annotations

from app.reconcile_result import (
    BUCKET_MATCHED,
    BUCKET_REVIEW,
    reconcile_buckets,
)


def _book(invoice="INV-1", taxable=100000, igst=18000):
    return {
        "invoice_no": invoice, "supplier_gstin": "27AABCS1429B1Z8",
        "supplier_name": "Sharma", "taxable": taxable,
        "igst": igst, "cgst": 0, "sgst": 0, "cess": 0,
    }


def _finding(invoice, label):
    return {"invoice_no": invoice, "reason_code": label, "supplier_gstin": "27AABCS1429B1Z8"}


def _bucket_of(result, invoice="INV-1"):
    return next(r["bucket"] for r in result.rows if r["invoice_no"] == invoice)


class TestTheBucketIsDecidedByTheComparisonAlone:
    def test_a_matched_invoice_with_blocked_credit_is_still_matched(self):
        """**Case C.** Both sides report the same figures — that is what
        "matched" means. The credit being unavailable under s.17(5) is a
        different statement, and calling the invoice a value disagreement
        because of it tells the reader something untrue about the data."""
        result = reconcile_buckets(
            [_book()], [_book()], [_finding("INV-1", "gst:ITCNotAvailable")]
        )

        assert _bucket_of(result) == BUCKET_MATCHED

    def test_such_an_invoice_still_carries_its_finding(self):
        """Reclassifying it must not lose the finding — that would trade one
        wrong answer for a worse one."""
        result = reconcile_buckets(
            [_book()], [_book()], [_finding("INV-1", "gst:ITCNotAvailable")]
        )

        row = next(r for r in result.rows if r["invoice_no"] == "INV-1")
        assert row["labels"] == ["gst:ITCNotAvailable"]
        assert row["blocked"] is True

    def test_a_matched_invoice_with_an_overdue_payment_is_still_matched(self):
        """Rule 37 is about whether you paid the supplier. It says nothing
        about whether the two records agree."""
        result = reconcile_buckets(
            [_book()], [_book()], [_finding("INV-1", "gst:PaymentOverdue")]
        )

        assert _bucket_of(result) == BUCKET_MATCHED

    def test_an_invoice_whose_values_actually_differ_is_review(self):
        """The bucket that Review is *for*, and it must keep working."""
        result = reconcile_buckets([_book(igst=18000)], [_book(igst=17640)], [])

        assert _bucket_of(result) == BUCKET_REVIEW

    def test_values_differing_makes_it_review_whether_or_not_a_rule_fired(self):
        result = reconcile_buckets(
            [_book(igst=18000)],
            [_book(igst=17640)],
            [_finding("INV-1", "gst:AmountMismatch")],
        )

        assert _bucket_of(result) == BUCKET_REVIEW

    def test_agreeing_records_are_matched_however_many_rules_fired(self):
        """Three compliance findings on an invoice both sides agree about is
        still an invoice both sides agree about."""
        result = reconcile_buckets(
            [_book()],
            [_book()],
            [
                _finding("INV-1", "gst:ITCNotAvailable"),
                _finding("INV-1", "gst:PaymentOverdue"),
                _finding("INV-1", "gst:GoodsReceiptTiming"),
            ],
        )

        assert _bucket_of(result) == BUCKET_MATCHED


class TestWhatThisChangesDownstream:
    def test_the_match_rate_measures_agreement_not_compliance(self):
        """A period where every invoice matches and half carry blocked credit
        has a 100% match rate and a serious ITC problem. Reporting 50% would
        conflate two things a CA reports separately."""
        books = [_book("INV-1"), _book("INV-2")]
        result = reconcile_buckets(
            books, books, [_finding("INV-1", "gst:ITCNotAvailable")]
        )

        assert result.match_rate == 1.0

    def test_blocked_credit_is_still_routed_to_blocked_not_to_confirmed(self):
        """The bucket changed; where the money is counted must not. A matched
        invoice with blocked credit is matched *and* its credit is lost."""
        from app.reconcile_result import itc_position

        result = reconcile_buckets(
            [_book()], [_book()], [_finding("INV-1", "gst:ITCNotAvailable")]
        )
        position = itc_position(result)

        assert position["blocked"] == 18000
        assert position["confirmed"] == 0
