"""GSTR-3B — the summary return, and the reconciliation that closes the loop.

**Structurally unlike every other kind this product ingests.** Books, 2A, 2B
and GSTR-1 are all *line-level*: one row per invoice. GSTR-3B is a **summary**
— one figure per Table 4 row, per period, with no invoice lines at all. It
cannot be keyed by invoice number or supplier GSTIN because it has neither.

Table 4's own structure, taken from the current (August 2022 onward) format:

| row | what it holds |
|---|---|
| 4A | gross ITC, **auto-populated from GSTR-2B** — eligible and ineligible both |
| 4B(1) | permanent reversals — Rule 38, Rules 42/43, s.17(5). Cannot be reclaimed. |
| 4B(2) | temporary reversals — Rule 37, s.16(2)(b)/(c). Reclaimable later. |
| 4C | net ITC to the credit ledger. **4A − 4B.** |
| 4D(1) | ITC reclaimed that an earlier period had reversed |
| 4D(2) | ITC unavailable by law — s.16(4) time bar, place-of-supply |

**4A being auto-populated from 2B is what makes the reconciliation possible
at all**: the portal's own figure and the taxpayer's claimed figure are
supposed to agree, and where they do not, one of the two is wrong in a way
that shows up in a notice rather than in the books.
"""

from __future__ import annotations

import pytest

from app.graphowl_client import rows_to_turtle


def a_3b_row(**overrides) -> dict:
    row = {
        "period": "2026-03",
        "itc_4a": "292500",
        "itc_reversed_4b1": "18000",
        "itc_reversed_4b2": "9000",
        "itc_net_4c": "265500",
        "itc_reclaimed_4d1": "0",
        "itc_unavailable_4d2": "4500",
    }
    row.update(overrides)
    return row


class TestGstr3bIsASummaryNotALineLevelReturn:
    def test_a_3b_row_mints_a_return_subject_for_its_period(self):
        turtle = rows_to_turtle([a_3b_row()], "gstr3b")

        assert "a gst:Gstr3bReturn" in turtle
        assert 'gst:period "2026-03"' in turtle

    def test_the_table_4_figures_reach_the_graph_under_their_own_row_names(self):
        """Named for the rows a preparer actually sees on the return. A single
        "itc" figure would make the working paper's gross -> reversals -> net
        chain untraceable, which is the one thing it exists to show."""
        turtle = rows_to_turtle([a_3b_row()], "gstr3b")

        for predicate, value in [
            ("gst:itcAvailable4A", "292500"),
            ("gst:itcReversed4B1", "18000"),
            ("gst:itcReversed4B2", "9000"),
            ("gst:itcNet4C", "265500"),
            ("gst:itcUnavailable4D2", "4500"),
        ]:
            assert f'{predicate} "{value}"' in turtle, predicate

    def test_a_3b_row_carries_no_invoice_or_supplier_subject(self):
        """It has neither. Minting an invoice subject for a summary would put
        a fabricated invoice into a store every finding query reads."""
        turtle = rows_to_turtle([a_3b_row()], "gstr3b")

        assert "gst:PurchaseInvoice" not in turtle
        assert "gst:Supplier" not in turtle
        assert "gst:invoiceNumber" not in turtle

    def test_two_periods_are_two_returns(self):
        turtle = rows_to_turtle(
            [a_3b_row(), a_3b_row(period="2026-04")],
            "gstr3b",
        )

        assert turtle.count("a gst:Gstr3bReturn") == 2

    def test_a_zero_figure_is_recorded_rather_than_omitted(self):
        """Zero reclaimed and *no* reclaim figure are different claims on a
        filed return, and 4D(1) is legitimately zero most months."""
        turtle = rows_to_turtle([a_3b_row()], "gstr3b")

        assert 'gst:itcReclaimed4D1 "0"' in turtle

    def test_a_3b_with_no_period_is_refused_rather_than_landed_unkeyed(self):
        """Every other kind tolerates a missing field. A 3B without a period
        cannot be compared against anything — it is not a partial answer, it
        is an unplaceable one."""
        row = a_3b_row()
        del row["period"]

        with pytest.raises(ValueError, match="period"):
            rows_to_turtle([row], "gstr3b")

    def test_an_absent_optional_figure_is_omitted_not_written_as_zero(self):
        """Same absent-vs-recorded-blank distinction every other kind draws.
        A 4B(2) nobody filled in is not a claim that no reversal was due."""
        row = a_3b_row()
        del row["itc_reversed_4b2"]

        turtle = rows_to_turtle([row], "gstr3b")

        assert "gst:itcReversed4B2" not in turtle
        assert "gst:itcAvailable4A" in turtle
