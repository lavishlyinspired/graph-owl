"""GSTR-2B against GSTR-3B — the core monthly ITC check.

**This is the reconciliation a notice is written about.** Table 4A of GSTR-3B
is auto-populated from GSTR-2B; 4C is what actually reached the credit ledger.
Where what was claimed and what the portal supports disagree, one of the two
is wrong, and the taxpayer finds out from the department rather than from
their own books.

The two directions are **not** symmetrical and must never be collapsed into
one "difference" figure:

- **Claimed more than 2B supports** — an excess claim. Interest under s.50 and
  a demand under s.73/74. This is the one that costs money.
- **Claimed less than 2B supports** — credit left unclaimed. Recoverable,
  until s.16(4) closes the window. This costs money too, but later and
  only if ignored.

Reporting a single signed number would make a preparer read the sign to learn
which situation they are in, and a sign is the easiest thing on a screen to
misread.
"""

from __future__ import annotations

from decimal import Decimal

from app.itc_3b import compare_2b_to_3b


def _return(**overrides) -> dict:
    row = {
        "period": "2026-03",
        "itc_4a": Decimal("292500"),
        "itc_reversed_4b1": Decimal("0"),
        "itc_reversed_4b2": Decimal("0"),
        "itc_net_4c": Decimal("292500"),
    }
    row.update(overrides)
    return row


class TestTheTwoDirections:
    def test_claiming_more_than_the_2b_supports_is_an_excess_claim(self):
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("250000"), gstr3b=_return()
        )

        assert result["direction"] == "excess"
        assert result["difference"] == Decimal("42500")

    def test_claiming_less_than_the_2b_supports_is_unclaimed_credit(self):
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("300000"), gstr3b=_return()
        )

        assert result["direction"] == "unclaimed"
        assert result["difference"] == Decimal("7500")

    def test_the_two_directions_are_never_the_same_label(self):
        """The whole point. An excess claim and unclaimed credit are opposite
        situations with opposite remedies."""
        excess = compare_2b_to_3b(gstr2b_total=Decimal("1"), gstr3b=_return())
        unclaimed = compare_2b_to_3b(gstr2b_total=Decimal("999999"), gstr3b=_return())

        assert excess["direction"] != unclaimed["direction"]

    def test_agreement_is_its_own_answer_not_a_zero_difference(self):
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("292500"), gstr3b=_return()
        )

        assert result["direction"] == "agrees"
        assert result["difference"] == Decimal("0")

    def test_the_difference_is_always_reported_unsigned(self):
        """The direction carries the sign. A magnitude that is sometimes
        negative makes every consumer re-derive which way round it is."""
        for total in (Decimal("1"), Decimal("999999"), Decimal("292500")):
            result = compare_2b_to_3b(gstr2b_total=total, gstr3b=_return())
            assert result["difference"] >= 0, result


class TestWhichFigureIsCompared:
    def test_4a_is_compared_against_the_2b_not_4c(self):
        """4A is the row auto-populated from 2B, so 4A is what 2B is supposed
        to agree with. 4C is 4A minus reversals the taxpayer made deliberately
        — comparing *that* against 2B would report every legitimate s.17(5)
        reversal as an under-claim."""
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("292500"),
            gstr3b=_return(
                itc_reversed_4b1=Decimal("18000"), itc_net_4c=Decimal("274500")
            ),
        )

        assert result["direction"] == "agrees"

    def test_the_net_figure_is_still_reported_for_the_working_paper(self):
        """Gross -> reversals -> net is the chain a working paper has to show,
        so the comparison carries the net figure even though it does not
        compare against it."""
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("292500"),
            gstr3b=_return(
                itc_reversed_4b1=Decimal("18000"), itc_net_4c=Decimal("274500")
            ),
        )

        assert result["net_claimed"] == Decimal("274500")
        assert result["gross_claimed"] == Decimal("292500")
        assert result["reversed"] == Decimal("18000")


class TestArithmeticIntegrityOfTheReturnItself:
    def test_a_4c_that_does_not_equal_4a_minus_4b_is_flagged(self):
        """4C = 4A − 4B is the return's own arithmetic. A filed return that
        fails it was mis-keyed, and every figure downstream of it is wrong."""
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("292500"),
            gstr3b=_return(
                itc_reversed_4b1=Decimal("18000"), itc_net_4c=Decimal("292500")
            ),
        )

        assert result["arithmetic_ok"] is False

    def test_a_consistent_return_passes_its_own_arithmetic(self):
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("292500"),
            gstr3b=_return(
                itc_reversed_4b1=Decimal("18000"),
                itc_reversed_4b2=Decimal("9000"),
                itc_net_4c=Decimal("265500"),
            ),
        )

        assert result["arithmetic_ok"] is True


class TestAbsentData:
    def test_no_3b_at_all_is_not_evaluated_rather_than_a_zero_claim(self):
        """The product's standing rule. With no 3B, "claimed nothing" would
        report the whole 2B as unclaimed credit — a large, wrong, and very
        actionable-looking number."""
        result = compare_2b_to_3b(gstr2b_total=Decimal("292500"), gstr3b=None)

        assert result["direction"] == "not_evaluated"
        assert result["difference"] is None
        assert "GSTR-3B" in result["needs"]

    def test_a_3b_with_no_4a_figure_cannot_be_compared(self):
        result = compare_2b_to_3b(
            gstr2b_total=Decimal("292500"), gstr3b=_return(itc_4a=None)
        )

        assert result["direction"] == "not_evaluated"
        assert result["needs"]


class TestRule37ReversalWasActuallyMade:
    """**The check that connects a finding to the filed return.**

    The engine already detects invoices unpaid past 180 days
    (`gst:PaymentOverdue`, s.16(2)(d) / Rule 37). Until now that was where it
    stopped: the product said "this credit must be reversed" and had no way to
    know whether it was. Table 4B(2) is where a Rule 37 reversal is reported,
    so with a 3B the loop closes — and an unreversed overdue credit is a real
    exposure sitting in a filed return, not a to-do.
    """

    def test_overdue_credit_with_no_4b2_reversal_is_flagged(self):
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(
            overdue_tax=Decimal("45000"),
            gstr3b=_return(itc_reversed_4b2=Decimal("0")),
        )

        assert result["status"] == "not_reversed"
        assert result["shortfall"] == Decimal("45000")

    def test_a_sufficient_reversal_clears_the_check(self):
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(
            overdue_tax=Decimal("45000"),
            gstr3b=_return(itc_reversed_4b2=Decimal("45000")),
        )

        assert result["status"] == "reversed"
        assert result["shortfall"] == Decimal("0")

    def test_a_partial_reversal_reports_only_the_shortfall(self):
        """Reversing some is not reversing none, and the exposure is the
        remainder — reporting the whole amount again would overstate it."""
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(
            overdue_tax=Decimal("45000"),
            gstr3b=_return(itc_reversed_4b2=Decimal("30000")),
        )

        assert result["status"] == "partially_reversed"
        assert result["shortfall"] == Decimal("15000")

    def test_reversing_more_than_is_overdue_is_not_a_shortfall(self):
        """4B(2) also carries s.16(2)(b)/(c) reversals, so a larger figure is
        ordinary rather than suspicious. It must not read as negative
        exposure."""
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(
            overdue_tax=Decimal("45000"),
            gstr3b=_return(itc_reversed_4b2=Decimal("60000")),
        )

        assert result["status"] == "reversed"
        assert result["shortfall"] == Decimal("0")

    def test_nothing_overdue_is_not_a_finding_however_the_3b_reads(self):
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(
            overdue_tax=Decimal("0"), gstr3b=_return(itc_reversed_4b2=Decimal("0"))
        )

        assert result["status"] == "nothing_due"

    def test_without_a_3b_the_check_is_not_evaluated_rather_than_failed(self):
        """An unfiled or unsupplied return is not evidence that a reversal was
        skipped. Reporting it as a failure would accuse a taxpayer on the
        strength of missing data."""
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(overdue_tax=Decimal("45000"), gstr3b=None)

        assert result["status"] == "not_evaluated"
        assert result["shortfall"] is None

    def test_a_3b_with_no_4b2_figure_is_not_evaluated_rather_than_zero(self):
        """A blank 4B(2) is an unfilled cell, not a declaration that nothing
        was reversed — and the two lead to opposite conversations."""
        from app.itc_3b import rule_37_reversal_check

        result = rule_37_reversal_check(
            overdue_tax=Decimal("45000"), gstr3b=_return(itc_reversed_4b2=None)
        )

        assert result["status"] == "not_evaluated"
