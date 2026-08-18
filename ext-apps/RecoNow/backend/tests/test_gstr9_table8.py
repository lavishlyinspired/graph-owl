"""GSTR-9 Table 8 — Plan 123 Slice C.

Table 8 is the annual return's ITC reconciliation: what the portal says was
available (8A, from GSTR-2A) against what was actually availed (8B, from
GSTR-3B), with the difference (8D) split into "available but not availed"
(8E) and "available but ineligible" (8F).

**This is where holding 2A pays for itself.** 8A is defined as ITC per
GSTR-2A, not per 2B — the annual return asks about everything the portal ever
showed for the year, including what arrived after each month's 2B froze.
A deployment holding only 2B cannot compute 8A at all.

**Not every row is computable here, and the ones that are not must say so.**
8B is ITC availed per GSTR-3B, and no 3B is ingested; imports (8G-8J) need
customs data. Reporting them as zero would be a filed number nobody derived —
the same false-clean this product refuses everywhere else.
"""

from __future__ import annotations

from decimal import Decimal

from app.gstr9 import table8


def _line(tax: str, *, eligible: bool = True) -> dict:
    return {"tax_amount": Decimal(tax), "itc_available": "Y" if eligible else "N"}


class TestTable8A:
    def test_8a_totals_the_itc_the_portal_showed_across_the_year(self):
        """8A is ITC as per GSTR-2A — everything the portal ever reported for
        the year, which is exactly what 2A holds and 2B does not."""
        result = table8(gstr2a=[_line("18000"), _line("4500")], availed=[])

        assert result["8A"]["value"] == Decimal("22500")

    def test_8a_counts_a_line_the_portal_marked_ineligible(self):
        """8A is what was *available*, not what was claimable. An ineligible
        line still appears in 2A and still belongs in 8A — 8F is where its
        ineligibility is reported."""
        result = table8(gstr2a=[_line("18000"), _line("4500", eligible=False)], availed=[])

        assert result["8A"]["value"] == Decimal("22500")

    def test_8a_of_an_empty_2a_is_zero_and_says_it_was_computed(self):
        """Zero from a real empty 2A is a genuine answer, unlike zero from an
        absent 2A — the distinction the rest of this module turns on."""
        result = table8(gstr2a=[], availed=[])

        assert result["8A"]["value"] == Decimal("0")
        assert result["8A"]["computed"] is True


class TestEligibilityDefaults:
    """The portal marks what it *blocks*; it does not stamp every ordinary
    line as allowed. So an absent flag must mean eligible — treating absence
    as ineligible would report a whole year's credit as lost, and 8F is the
    row a preparer cannot recover from."""

    def test_a_line_with_no_eligibility_flag_at_all_is_treated_as_eligible(self):
        bare = {"tax_amount": Decimal("18000")}

        result = table8(gstr2a=[bare], availed=[{"tax_amount": Decimal("18000")}])

        assert result["8F"]["value"] == Decimal("0")
        assert result["8A"]["value"] == Decimal("18000")

    def test_an_explicit_n_is_the_only_thing_that_makes_a_line_ineligible(self):
        result = table8(
            gstr2a=[_line("18000", eligible=False)],
            availed=[{"tax_amount": Decimal("0")}],
        )

        assert result["8F"]["value"] == Decimal("18000")

    def test_the_flag_is_read_case_and_whitespace_insensitively(self):
        """A real portal export is not consistent about either."""
        result = table8(
            gstr2a=[{"tax_amount": Decimal("18000"), "itc_available": " n "}],
            availed=[{"tax_amount": Decimal("0")}],
        )

        assert result["8F"]["value"] == Decimal("18000")


class TestRowsTheDataCannotSupport:
    def test_8b_is_not_computed_without_a_gstr3b(self):
        """8B is ITC availed per GSTR-3B. No 3B is ingested, and reporting a
        proxy under 8B's name would put a number in a filed return that
        nobody derived."""
        result = table8(gstr2a=[_line("18000")], availed=[])

        assert result["8B"]["computed"] is False
        assert result["8B"]["value"] is None
        assert "GSTR-3B" in result["8B"]["needs"]

    def test_an_uncomputable_row_never_reports_zero(self):
        """Zero and unknown are opposite claims on a return."""
        result = table8(gstr2a=[], availed=[])

        for row in result.values():
            if row["computed"] is False:
                assert row["value"] is None

    def test_import_rows_are_not_computed_without_customs_data(self):
        result = table8(gstr2a=[_line("18000")], availed=[])

        for label in ("8G", "8H", "8J"):
            assert result[label]["computed"] is False
            assert result[label]["needs"]

    def test_every_uncomputable_row_names_what_it_needs(self):
        """A blank cell tells a preparer nothing. Naming the missing dataset
        tells them what to go and get."""
        result = table8(gstr2a=[], availed=[])

        for label, row in result.items():
            if not row["computed"]:
                assert row["needs"], f"{label} is uncomputable with no reason given"


class TestTable8BFromGstr3b:
    """**Closes a row this module used to report as uncomputable.** 8B is ITC
    availed per GSTR-3B; with 3B ingested it is derivable, and the honest
    thing is to compute it rather than keep saying it cannot be.

    The strict definition is Table 6(B) + 6(H) of GSTR-9, which breaks down
    what was availed. This uses **4C summed across the year's returns** — the
    net ITC that actually reached the credit ledger — which is the same
    quantity reached from the monthly returns rather than the annual
    breakdown. Stated here rather than left for a reader to discover, because
    the two can differ where a 6(H) reclaim straddles the year end.
    """

    def test_8b_totals_the_net_itc_claimed_across_the_years_returns(self):
        result = table8(
            gstr2a=[_line("300000")],
            availed=[],
            returns=[{"itc_net_4c": Decimal("120000")}, {"itc_net_4c": Decimal("145000")}],
        )

        assert result["8B"]["computed"] is True
        assert result["8B"]["value"] == Decimal("265000")

    def test_8d_is_computed_from_8b_once_3b_is_present(self):
        """The point of closing 8B: the difference row it feeds becomes real."""
        result = table8(
            gstr2a=[_line("300000")],
            availed=[],
            returns=[{"itc_net_4c": Decimal("265000")}],
        )

        assert result["8D"]["computed"] is True
        assert result["8D"]["value"] == Decimal("35000")

    def test_without_returns_8b_is_still_uncomputed(self):
        """The old behaviour has to survive. A deployment with no 3B must not
        start reporting a fabricated 8B."""
        result = table8(gstr2a=[_line("300000")], availed=[])

        assert result["8B"]["computed"] is False
        assert result["8B"]["value"] is None

    def test_a_return_missing_its_4c_does_not_silently_count_as_zero(self):
        """One unparseable return among twelve would otherwise understate the
        year's claim by a month and overstate 8D by the same amount."""
        result = table8(
            gstr2a=[_line("300000")],
            availed=[],
            returns=[{"itc_net_4c": Decimal("120000")}, {"itc_net_4c": None}],
        )

        assert result["8B"]["computed"] is False
        assert "4C" in result["8B"]["needs"]


class TestDifferences:
    def test_8d_is_the_difference_between_available_and_availed(self):
        result = table8(gstr2a=[_line("20000")], availed=[{"tax_amount": Decimal("15000")}])

        assert result["8D"]["value"] == Decimal("5000")
        assert result["8D"]["computed"] is True

    def test_8d_is_not_computed_when_nothing_was_availed_because_none_was_supplied(self):
        """An empty `availed` is an absent books figure, not a claim of zero.
        8D computed against it would report the whole of 8A as unclaimed."""
        result = table8(gstr2a=[_line("20000")], availed=[])

        assert result["8D"]["computed"] is False

    def test_8f_separates_ineligible_credit_from_merely_unclaimed(self):
        """8E and 8F are different outcomes: 8E can still be claimed, 8F is
        lost. Collapsing them overstates what is recoverable."""
        result = table8(
            gstr2a=[_line("20000"), _line("5000", eligible=False)],
            availed=[{"tax_amount": Decimal("20000")}],
        )

        assert result["8F"]["value"] == Decimal("5000")
        assert result["8E"]["value"] == Decimal("0")

    def test_a_negative_difference_is_reported_not_clamped(self):
        """Availing more than the portal showed is a real and serious
        condition. Clamping it to zero hides an excess claim."""
        result = table8(gstr2a=[_line("10000")], availed=[{"tax_amount": Decimal("14000")}])

        assert result["8D"]["value"] == Decimal("-4000")
