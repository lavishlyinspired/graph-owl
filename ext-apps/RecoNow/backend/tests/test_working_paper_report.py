"""The working paper as a document, not a table.

A CA hands this to a partner or an officer. A table of five figures answers
"what is the number"; a working paper has to answer "how did you get there,
and what did you leave out" — which is the question that actually gets asked.

**The skeleton is computed, the prose may be generated.** Same split as the
client report: section order and every figure come from the chain, and a model
may only rewrite the sentences. A working paper is filed evidence; a figure
invented in one is worse than no working paper at all.

**What was deliberately NOT deducted is part of the document.** Findings the
chain has no line for are listed, because a reader who cannot see what was
excluded cannot tell a complete paper from a partial one — and the net figure
looks equally confident either way.
"""

from __future__ import annotations

from app.working_paper_report import build_report, downloadable

PAPER = {
    "lines": [
        {"key": "gross", "kind": "opening", "label": "ITC available", "amount": 279340.0,
         "source": "GSTR-2B — 15 line(s) for this period", "citation": None, "unquantified": 0},
        {"key": "blocked_17_5", "kind": "deduction", "label": "Blocked credit", "amount": 89800.0,
         "source": "graph-owl finding rules", "citation": "s.17(5) — blocked credits", "unquantified": 0},
        {"key": "reversal_rule_37", "kind": "deduction", "label": "Reversal — supplier unpaid 180 days",
         "amount": 45000.0, "source": "graph-owl finding rules",
         "citation": "Rule 37 read with s.16(2)(d)", "unquantified": 0},
        {"key": "net", "kind": "closing", "label": "Net ITC claimable", "amount": 144540.0,
         "source": "computed", "citation": None, "unquantified": 0},
    ],
    "unmodelled": [{"label": "gst:AmountMismatch", "amount": 30000.0}],
    "complete": True,
    "filed": {"direction": "not_evaluated", "difference": None, "needs": "a GSTR-3B",
              "available_2b": 279340.0, "gross_claimed": None, "reversed": None,
              "net_claimed": None, "arithmetic_ok": None},
}


class TestTheDocument:
    def test_it_states_the_chain_in_order(self):
        report = build_report(PAPER, period="March 2026")

        gross_at = report.index("ITC available")
        net_at = report.index("Net ITC claimable")
        assert gross_at < net_at

    def test_every_deduction_carries_its_provision(self):
        """A deduction without its citation is an assertion. This document is
        read by people whose job is to disagree with it."""
        report = build_report(PAPER, period="March 2026")

        assert "s.17(5)" in report
        assert "Rule 37" in report

    def test_it_names_the_period(self):
        assert "March 2026" in build_report(PAPER, period="March 2026")

    def test_it_shows_the_arithmetic_rather_than_only_the_result(self):
        """2,79,340 − 1,34,800 = 1,44,540, written out. A reader checking a
        working paper is checking the subtraction — and the grouping is Indian,
        because this document is read in India."""
        report = build_report(PAPER, period="March 2026")

        assert "2,79,340" in report
        assert "1,44,540" in report


class TestWhatWasExcluded:
    def test_findings_with_no_line_are_listed(self):
        """A reader who cannot see what was excluded cannot tell a complete
        paper from a partial one, and the net figure looks equally confident
        either way."""
        report = build_report(PAPER, period="March 2026")

        assert "gst:AmountMismatch" in report

    def test_a_paper_with_nothing_excluded_says_so_rather_than_omitting_the_section(self):
        """An absent section reads as "nothing to report" only if the reader
        already knows the section exists."""
        clean = {**PAPER, "unmodelled": []}

        assert "none" in build_report(clean, period="March 2026").lower()

    def test_an_incomplete_paper_says_the_net_figure_is_an_upper_bound(self):
        """The one thing a reader must not do is treat an unsized deduction as
        zero. Said in the document, not only on the screen."""
        partial = {**PAPER, "complete": False}

        report = build_report(partial, period="March 2026")
        assert "upper bound" in report.lower()


class TestAgainstTheFiledReturn:
    def test_an_unfiled_period_says_so_rather_than_claiming_agreement(self):
        report = build_report(PAPER, period="March 2026")

        assert "not" in report.lower()
        assert "GSTR-3B" in report

    def test_an_excess_claim_is_stated_plainly(self):
        filed = {**PAPER, "filed": {**PAPER["filed"], "direction": "excess",
                                    "difference": 5000.0, "gross_claimed": 284340.0,
                                    "net_claimed": 284340.0, "reversed": 0.0,
                                    "arithmetic_ok": True, "needs": None}}

        report = build_report(filed, period="March 2026")
        assert "5,000" in report


class TestDownload:
    def test_it_produces_a_filename_naming_the_period(self):
        """A file called `report.txt` in a folder of thirty is a file nobody
        can find next March."""
        name, _ = downloadable(PAPER, period="March 2026")

        assert "march" in name.lower()
        assert "2026" in name

    def test_the_filename_is_safe_for_a_filesystem(self):
        name, _ = downloadable(PAPER, period="March / 2026")

        assert "/" not in name

    def test_the_body_is_the_report(self):
        _, body = downloadable(PAPER, period="March 2026")

        assert "ITC available" in body
