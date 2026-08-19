"""The client report — the LLM use the reference mockup leads with.

Screenshot 8 of the delivered mockups is a generated report with a fixed
skeleton: executive summary, key findings, issue breakdown, risk assessment,
recommended actions, next steps — with Copy, Download and Regenerate.

**The skeleton is ours; only the prose is the model's.** A model asked for "a
report" produces a different shape every time, and a document a client
receives monthly must not be reorganised at random. Section order and the
figures in them are computed here; the model writes the sentences.

**Every figure is supplied and grounded**, so a report that states a number the
reconciliation does not carry is refused and the computed report shown instead
— the same rule the case explainer uses. A client report is the single
worst place in this product for an invented figure: it leaves the building.
"""

from __future__ import annotations

from app.client_report import SECTIONS, build_facts, computed_report


FACTS_INPUT = {
    "period": "March 2026",
    "counts": {"matched": 6, "review": 7, "only_books": 2, "only_portal": 2},
    "itc": {
        "confirmed": 177300.0, "pending": 25740.0, "blocked": 89800.0,
        "under_review": 2250.0, "unclaimed": 9990.0, "total_considered": 305080.0,
    },
    "match_rate": 0.353,
    "outcomes": [
        {"label": "gst:ITCNotAvailable", "title": "Credit reported as unavailable",
         "status": "flagged", "found": 2, "governed_by": "gst:Section17-5"},
        {"label": "gst:PaymentOverdue", "title": "Unpaid past 180 days",
         "status": "flagged", "found": 1, "governed_by": "gst:Section16-2-d"},
        {"label": "gst:Reversed", "title": "Reverse charge", "status": "passed", "found": 0},
    ],
}


class TestTheSkeletonIsOurs:
    def test_the_sections_are_fixed_and_ordered(self):
        """A document a client receives monthly must not be reorganised at
        random, and a model asked for "a report" produces a different shape
        every time."""
        assert SECTIONS[0] == "EXECUTIVE SUMMARY"
        assert "RECOMMENDED ACTIONS" in SECTIONS
        assert SECTIONS[-1] == "NEXT STEPS"

    def test_the_computed_report_carries_every_section(self):
        report = computed_report(build_facts(**FACTS_INPUT))

        for section in SECTIONS:
            assert section in report


class TestTheFigures:
    def test_the_facts_carry_the_period_and_the_headline_numbers(self):
        facts = build_facts(**FACTS_INPUT)

        assert facts["period"] == "March 2026"
        assert facts["total_invoices"] == 17
        assert facts["itc_confirmed"] == 177300.0

    def test_at_risk_is_blocked_plus_disputed_not_everything_unmatched(self):
        """"At risk" must mean what the reconcile screen's headline card means.
        A report using a different definition of the same phrase is how two
        numbers for one thing reach a client."""
        facts = build_facts(**FACTS_INPUT)

        assert facts["itc_at_risk"] == 92050.0

    def test_only_flagged_rules_reach_the_issue_breakdown(self):
        """A passed check is not an issue, and listing it pads a report a
        client is meant to act on."""
        facts = build_facts(**FACTS_INPUT)

        labels = [i["title"] for i in facts["issues"]]
        assert "Reverse charge" not in labels
        assert len(facts["issues"]) == 2

    def test_issues_are_ordered_by_how_many_invoices_they_name(self):
        facts = build_facts(**FACTS_INPUT)

        assert facts["issues"][0]["found"] >= facts["issues"][-1]["found"]

    def test_every_figure_in_the_computed_report_is_in_the_facts(self):
        """The grounding contract: the model is handed exactly what the
        computed report states, so a faithful rewrite can never be refused."""
        facts = build_facts(**FACTS_INPUT)
        report = computed_report(facts)

        from app.grounding import numbers_in

        supplied = " ".join(str(v) for v in facts.values())
        for number in numbers_in(report):
            assert number in supplied.replace(",", ""), number


class TestWhatTheModelMayCite:
    """**Found by generating a real report.** It was refused for "states 180" —
    from the issue title "Unpaid past 180 days", which the computed report
    itself prints. The supplied facts excluded lists, so the issue titles and
    their citations were not cited text and every statutory constant in them
    was unsupported.

    Same class as the case explainer's: a constant stated by a provision is
    supported by the text that states it."""

    def test_issue_titles_and_citations_are_supplied(self):
        from app.client_report import groundable

        supplied = groundable(build_facts(**FACTS_INPUT))
        joined = " ".join(str(v) for v in supplied.values())

        assert "180" in joined
        assert "Section17-5" in joined

    def test_every_number_the_computed_report_prints_is_groundable(self):
        """The contract that makes a faithful rewrite impossible to refuse."""
        from app.grounding import numbers_in
        from app.client_report import groundable

        facts = build_facts(**FACTS_INPUT)
        supplied = " ".join(str(v) for v in groundable(facts).values()).replace(",", "")

        for number in numbers_in(computed_report(facts)):
            assert number in supplied, number


class TestTheComputedReportStandsAlone:
    def test_it_names_the_period_and_the_match_rate(self):
        report = computed_report(build_facts(**FACTS_INPUT))

        assert "March 2026" in report
        assert "35.3" in report

    def test_it_states_what_is_recoverable_separately_from_what_is_lost(self):
        """The distinction the whole product turns on. A report that gives one
        "at risk" number tells a client to worry about money they can still
        collect."""
        report = computed_report(build_facts(**FACTS_INPUT))

        assert "recoverable" in report.lower() or "deferred" in report.lower()
        assert "blocked" in report.lower()

    def test_a_period_with_nothing_wrong_still_produces_a_report(self):
        clean = {**FACTS_INPUT, "outcomes": [], "itc": {**FACTS_INPUT["itc"], "blocked": 0.0}}
        report = computed_report(build_facts(**clean))

        assert "EXECUTIVE SUMMARY" in report
