"""A disputed invoice is mostly agreed, and the agreed part is claimable.

**Found 19 August 2026 by checking the working paper against the ITC position,
as asked.** A Review-bucket invoice — both sides carry it, the values differ —
contributed *only its difference* to the ITC position. The agreed portion was
counted in no class at all.

On real March data that was ₹2,10,760 of claimable credit missing from the
position, and it is why the two screens' totals differed by ₹95,220 rather
than by the population difference the comparison note describes. The note was
true and was covering for a bug.

**The split is the whole point of the Review bucket.** Books say ₹1,80,000,
portal says ₹1,80,500: ₹1,80,000 is agreed by both sides and claimable now,
and only ₹500 is in doubt. Treating the whole invoice as disputed overstates
the problem; treating none of it as disputed hides it. The distinction is
exactly what a reviewer needs, and it is the same reasoning `under_review`
already applied to the difference — just never applied to the remainder.
"""

from __future__ import annotations

from decimal import Decimal

from app.reconcile_result import itc_position


class _Result:
    def __init__(self, rows):
        self.rows = rows


def _row(bucket, books, portal, blocked=False):
    return {
        "bucket": bucket,
        "books_tax": Decimal(str(books)),
        "portal_tax": Decimal(str(portal)),
        "blocked": blocked,
    }


class TestTheAgreedPortionOfADisputedInvoice:
    def test_the_agreed_part_is_confirmed_and_only_the_difference_is_in_doubt(self):
        position = itc_position(_Result([_row("review", 180000, 180500)]))

        assert position["under_review"] == Decimal("500")
        assert position["confirmed"] == Decimal("180000")

    def test_the_agreed_part_is_the_lower_of_the_two_sides(self):
        """Never the higher. Claiming the portal's larger figure when your own
        books support less is an excess claim, and claiming your own larger
        figure when the portal supports less is one the portal will reject."""
        higher_books = itc_position(_Result([_row("review", 180500, 180000)]))

        assert higher_books["confirmed"] == Decimal("180000")
        assert higher_books["under_review"] == Decimal("500")

    def test_every_rupee_of_a_review_invoice_lands_somewhere(self):
        """The property that was broken: ₹2,10,760 of a real period's credit
        was in no class at all."""
        position = itc_position(_Result([_row("review", 180000, 180500)]))

        accounted = position["confirmed"] + position["under_review"]
        assert accounted == Decimal("180500")

    def test_a_blocked_review_invoice_is_still_wholly_blocked(self):
        """s.17(5) does not care that the two sides agree about the amount.
        Blocked wins over every other classification."""
        position = itc_position(_Result([_row("review", 180000, 180500, blocked=True)]))

        assert position["blocked"] == Decimal("180000")
        assert position["confirmed"] == Decimal("0")
        assert position["under_review"] == Decimal("0")

    def test_a_matched_invoice_is_unaffected(self):
        """The change must not disturb the bucket that was already right."""
        position = itc_position(_Result([_row("matched", 45000, 45000)]))

        assert position["confirmed"] == Decimal("45000")
        assert position["under_review"] == Decimal("0")

    def test_two_review_invoices_accumulate_both_parts(self):
        position = itc_position(
            _Result([_row("review", 100, 150), _row("review", 200, 180)])
        )

        assert position["confirmed"] == Decimal("280")
        assert position["under_review"] == Decimal("70")
