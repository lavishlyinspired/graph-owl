"""Supplier follow-ups, grouped the way you actually send them.

**The vendor agent drafts messages and nothing renders them.** Mockups 4 and 5
show the shape: a Generate step, then one card per *supplier* carrying an
at-risk chip and the drafted message.

**Per supplier, not per invoice** — the agent drafts per invoice because that
is what a finding names, but you send one email to a supplier, not one per
invoice they failed to file. A supplier with three unfiled invoices gets one
message listing all three; three separate emails is how a working relationship
gets damaged by software.
"""

from __future__ import annotations

from app.follow_ups import group_drafts

DRAFTS = [
    {"invoice_no": "INV-1", "message": "Dear Phantom...", "source": "model"},
    {"invoice_no": "INV-2", "message": "Dear Phantom...", "source": "model"},
    {"invoice_no": "INV-3", "message": "Dear Ghost...", "source": "computed"},
]
CASES = [
    {"invoice_no": "INV-1", "supplier_name": "Phantom Supplies Co",
     "supplier_gstin": "22AABCX8888B1ZQ", "books_amount": 8640.0},
    {"invoice_no": "INV-2", "supplier_name": "Phantom Supplies Co",
     "supplier_gstin": "22AABCX8888B1ZQ", "books_amount": 4000.0},
    {"invoice_no": "INV-3", "supplier_name": "Ghost Vendor Pvt Ltd",
     "supplier_gstin": "11AABCZ9999A1Z1", "books_amount": 17100.0},
]


class TestGrouping:
    def test_one_card_per_supplier_not_per_invoice(self):
        """You send one email to a supplier. Three separate emails about three
        invoices is how a working relationship gets damaged by software."""
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        assert len(groups) == 2

    def test_a_group_carries_every_invoice_it_covers(self):
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        phantom = next(g for g in groups if "Phantom" in g["supplier_name"])
        assert sorted(phantom["invoices"]) == ["INV-1", "INV-2"]

    def test_the_at_risk_amount_is_the_supplier_total(self):
        """The chip says what the conversation is worth. Per-invoice amounts
        would make a supplier look like several small problems instead of one
        real one."""
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        phantom = next(g for g in groups if "Phantom" in g["supplier_name"])
        assert phantom["at_risk"] == 12640.0

    def test_the_largest_exposure_comes_first(self):
        """A list ordered by anything else makes a reviewer read all of it to
        find the one worth their morning."""
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        assert groups[0]["at_risk"] >= groups[-1]["at_risk"]

    def test_the_gstin_travels_with_the_group(self):
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        assert all(g["supplier_gstin"] for g in groups)


class TestTheMessage:
    def test_a_group_of_one_uses_that_invoice_s_own_draft(self):
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        ghost = next(g for g in groups if "Ghost" in g["supplier_name"])
        assert ghost["message"] == "Dear Ghost..."

    def test_a_group_reports_whether_a_model_wrote_it(self):
        """A message that leaves the building has to say what produced it."""
        groups = group_drafts(drafts=DRAFTS, cases=CASES)

        phantom = next(g for g in groups if "Phantom" in g["supplier_name"])
        assert phantom["source"] == "model"

    def test_a_group_mixing_model_and_computed_drafts_reports_computed(self):
        """The weaker claim wins. Calling a part-computed message "model" would
        overstate what was generated; the reverse understates nothing that
        matters."""
        mixed = [
            {"invoice_no": "INV-1", "message": "a", "source": "model"},
            {"invoice_no": "INV-2", "message": "b", "source": "computed"},
        ]

        groups = group_drafts(drafts=mixed, cases=CASES)

        assert next(g for g in groups if "Phantom" in g["supplier_name"])["source"] == "computed"


class TestAbsences:
    def test_a_draft_with_no_matching_case_is_dropped_not_crashed(self):
        """A stale draft from a previous run must not take the screen with it."""
        groups = group_drafts(
            drafts=[{"invoice_no": "GONE", "message": "x", "source": "computed"}], cases=CASES
        )

        assert groups == []

    def test_no_drafts_yields_no_groups(self):
        assert group_drafts(drafts=[], cases=CASES) == []

    def test_a_case_with_no_amount_contributes_nothing_rather_than_breaking(self):
        cases = [{**CASES[0], "books_amount": None}]
        groups = group_drafts(
            drafts=[{"invoice_no": "INV-1", "message": "x", "source": "computed"}], cases=cases
        )

        assert groups[0]["at_risk"] == 0.0
