"""Explaining a case from its **real data**, with a model.

**The user's ask**: not the rule's generic meaning, but *why this rule fired
for this invoice* — what data is involved, what the values actually are, and
how the conclusion was reached.

The pack's guidance is generic and authored once per rule. The computed
narrative states the two figures. Neither reads the *rest* of the row: the tax
heads, the dates, the HSN, the place of supply, the evidence the rule itself
projected. A model can, and that is exactly the kind of work the research says
models are good at — reading a lot of structured context and saying what is
notable in it.

**The grounding rule still binds, and the facts are what make it usable.** A
model handed a case with no numbers can only invent them; a model handed
*every* number in the row can explain it without inventing anything. So
`gather_facts` supplies the whole row from both sides, plus the derived
figures a reader would expect (the difference, its share), and any figure
outside that set is still refused.
"""

from __future__ import annotations

from app.case_explainer import (
    build_prompt,
    gather_facts,
    numeric_facts,
)

CASE = {
    "invoice_no": "INV-MAR-011",
    "reason_code": "gst:AmountMismatch",
    "supplier_name": "Sharma Infrastructure Pvt Ltd",
    "supplier_gstin": "27AABCS1429B1Z8",
    "books_amount": 180000.0,
    "portal_amount": 180500.0,
    "governed_by": "gst:Rule36-4",
    "summary": "Both sides report the invoice, and the values differ",
}
BOOKS_ROW = {
    "invoice_no": "INV-MAR-011",
    "invoice_date": "2026-03-18",
    "taxable": 1000000,
    "igst": 180000,
    "cgst": 0,
    "sgst": 0,
    "hsn": "9954",
}
PORTAL_ROW = {
    "invoice_no": "INV-MAR-011",
    "invoice_date": "2026-03-18",
    "taxable": 1002778,
    "igst": 180500,
    "cgst": 0,
    "sgst": 0,
}
GUIDANCE = {
    "title": "Your books and GSTR-2B disagree",
    "meaning": "Both sides report this invoice and the amounts differ.",
    "next_action": "Establish which side is right before filing.",
}


class TestGatheringTheRealRow:
    def test_it_carries_both_sides_of_the_row_not_just_the_totals(self):
        """The computed narrative already states the two totals. What a model
        adds is everything else in the row — and it can only add it if it is
        given it."""
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE)

        assert facts["books"]["taxable"] == 1000000
        assert facts["portal"]["igst"] == 180500

    def test_it_computes_the_figures_a_reader_would_expect(self):
        """The difference and its share are what a reviewer asks for next. If
        the model has to derive them it will get them wrong, and grounding
        will then refuse a sentence that was only trying to be helpful."""
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE)

        assert facts["difference"] == 500.0
        assert 0.27 <= facts["difference_pct"] <= 0.29

    def test_it_carries_the_rule_and_the_provision_it_rests_on(self):
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE)

        assert facts["rule"] == "gst:AmountMismatch"
        assert facts["governed_by"] == "gst:Rule36-4"

    def test_a_case_with_no_portal_side_still_gathers(self):
        """Only-books findings have one side. Half a row is still the real
        data, and refusing to explain it would leave the most common finding
        type unexplained."""
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=None, guidance=GUIDANCE)

        assert facts["portal"] is None
        assert facts["books"]["igst"] == 180000


class TestWhatTheModelIsAllowedToSay:
    def test_every_number_in_the_row_is_groundable(self):
        """The grounding rule refuses any figure not in the supplied facts. If
        the tax heads and the taxable value are not supplied, a model that
        mentions them — correctly — gets refused, and the explanation is worse
        for being accurate."""
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE)
        supplied = numeric_facts(facts)

        for value in (1000000, 180000, 180500, 500, 1002778):
            assert any(str(value) in str(v) for v in supplied.values()), value

    def test_the_derived_share_is_groundable_too(self):
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE)
        supplied = numeric_facts(facts)

        assert any("0.2" in str(v) or "0.3" in str(v) for v in supplied.values())

    def test_identifiers_are_supplied_so_naming_them_is_not_a_refusal(self):
        """An invoice number and a GSTIN contain digits. A model naming them —
        which it must, to be useful — has to have been given them."""
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE)
        supplied = numeric_facts(facts)

        joined = " ".join(str(v) for v in supplied.values())
        assert "INV-MAR-011" in joined
        assert "27AABCS1429B1Z8" in joined


class TestStatutoryConstants:
    """**Found by running a real model.** A Rule 37 explanation was refused for
    "states 180" — the 180-day threshold, which is part of the *provision*, not
    a claim about the data.

    A statutory constant that appears in the rule's own definition is supported
    by that definition. Refusing it would make every time-bound rule
    unexplainable by a model — 180 days, 30 November, the Rule 36(4) cap — which
    is most of the rules worth explaining.
    """

    def test_the_rules_own_definition_is_a_supplied_fact(self):
        facts = gather_facts(
            case={**CASE, "reason_code": "gst:PaymentOverdue",
                  "summary": "Credit taken on an invoice not paid within 180 days of its date"},
            books_row=BOOKS_ROW, portal_row=None, guidance=GUIDANCE,
        )
        supplied = numeric_facts(facts)

        joined = " ".join(str(v) for v in supplied.values())
        assert "180" in joined

    def test_the_provision_reference_is_supplied_too(self):
        """`s.16(2)(d)` and `Rule36-4` carry digits a model will quote."""
        facts = gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=None, guidance=GUIDANCE)
        supplied = numeric_facts(facts)

        joined = " ".join(str(v) for v in supplied.values())
        assert "Rule36-4" in joined


class TestAbsentIsNotAFact:
    """**Found by reading real model output.** Several explanations said "the
    portal tax amount is None" and "the portal has not captured the tax" — an
    inference drawn from a field that is absent because the *rule* did not
    project it, not because the portal lacks it.

    For `gst:SupplierNotFiled` the portal genuinely has no entry, and saying so
    is correct. For `gst:PaymentOverdue` the portal has the invoice perfectly
    well; the rule simply reports no portal amount. Rendering both as `None`
    let the model turn one into the other.

    This is the same absent-versus-zero discipline the rest of this codebase
    applies to figures, applied to a prompt: **a missing field must not appear
    at all**, and where its absence is itself meaningful the prompt must say
    which kind of absence it is.
    """

    def test_a_none_total_is_omitted_rather_than_rendered_as_none(self):
        prompt = build_prompt(
            gather_facts(
                case={**CASE, "portal_amount": None},
                books_row=BOOKS_ROW, portal_row=None, guidance=GUIDANCE,
            )
        )

        assert "None" not in prompt

    def test_an_absent_portal_side_is_named_as_absent_not_as_a_value(self):
        prompt = build_prompt(
            gather_facts(
                case={**CASE, "portal_amount": None},
                books_row=BOOKS_ROW, portal_row=None, guidance=GUIDANCE,
            )
        )

        assert "no matching entry" in prompt.lower() or "nothing on this side" in prompt.lower()

    def test_a_present_portal_row_with_no_projected_total_says_which_is_which(self):
        """The case that produced the wrong inference: the portal *has* the
        invoice and the rule reports no portal total. The prompt must not let
        those read the same."""
        prompt = build_prompt(
            gather_facts(
                case={**CASE, "portal_amount": None},
                books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE,
            )
        )

        assert "not projected by this rule" in prompt.lower()
        assert "None" not in prompt


class TestElapsedTimeIsSuppliedNotDerived:
    """**Found by watching a real refusal.** A Rule 37 explanation was refused
    for "states 08, 181, 19" — the model had counted the days between the
    invoice date and today and written "181 days".

    That is the model doing arithmetic despite being told not to, and the
    refusal caught it. But the answer is not to scold the model: a time-based
    rule *is about* elapsed time, and an explanation that cannot mention how
    long is not an explanation. **Compute it here, so the model never has a
    reason to.** The same reasoning that already supplies the difference and
    its percentage.
    """

    def test_days_since_the_invoice_date_is_supplied(self):
        facts = gather_facts(
            case={**CASE, "reason_code": "gst:PaymentOverdue"},
            books_row={**BOOKS_ROW, "invoice_date": "2026-03-01"},
            portal_row=None, guidance=GUIDANCE, today="2026-08-29",
        )

        assert facts["days_since_invoice"] == 181

    def test_the_day_count_is_groundable(self):
        facts = gather_facts(
            case={**CASE, "reason_code": "gst:PaymentOverdue"},
            books_row={**BOOKS_ROW, "invoice_date": "2026-03-01"},
            portal_row=None, guidance=GUIDANCE, today="2026-08-29",
        )
        supplied = numeric_facts(facts)

        assert any("181" in str(v) for v in supplied.values())

    def test_an_unparseable_date_yields_no_day_count_rather_than_a_wrong_one(self):
        """A date the parser cannot read must not produce a confident number of
        days. Absent is the honest answer."""
        facts = gather_facts(
            case=CASE, books_row={**BOOKS_ROW, "invoice_date": "not a date"},
            portal_row=None, guidance=GUIDANCE, today="2026-08-29",
        )

        assert facts["days_since_invoice"] is None

    def test_a_case_with_no_date_at_all_yields_none(self):
        row = {k: v for k, v in BOOKS_ROW.items() if k != "invoice_date"}
        facts = gather_facts(case=CASE, books_row=row, portal_row=None, guidance=GUIDANCE)

        assert facts["days_since_invoice"] is None


class TestThePrompt:
    def test_it_asks_for_this_invoice_not_the_rule_in_general(self):
        """The pack already says what the rule means. A model repeating that
        adds nothing and costs a round trip."""
        prompt = build_prompt(gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE))

        assert "INV-MAR-011" in prompt
        assert "this invoice" in prompt.lower()

    def test_it_forbids_computing_and_says_why(self):
        """A model told only "be accurate" will still do arithmetic. Told the
        figures are already computed and it must not derive new ones, it has
        no reason to."""
        prompt = build_prompt(gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE))

        assert "do not" in prompt.lower()
        assert "calculat" in prompt.lower() or "comput" in prompt.lower()

    def test_it_labels_which_figure_is_the_tax_and_which_is_the_taxable_value(self):
        """A model given a bare list will sometimes call the tax total a
        taxable value. Grounding checks numbers, not the words around them, so
        a mislabelled-but-real figure passes — the prompt is the only place
        that can prevent it."""
        prompt = build_prompt(gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE))

        assert "taxable value" in prompt.lower()
        assert "tax amount" in prompt.lower() or "tax total" in prompt.lower()

    def test_it_carries_the_whole_row_so_the_answer_can_be_specific(self):
        prompt = build_prompt(gather_facts(case=CASE, books_row=BOOKS_ROW, portal_row=PORTAL_ROW, guidance=GUIDANCE))

        assert "9954" in prompt
        assert "2026-03-18" in prompt
