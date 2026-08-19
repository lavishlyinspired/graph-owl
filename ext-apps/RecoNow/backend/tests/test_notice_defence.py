"""The derivation behind a case — Plan 123 Slice E, `/reasoning/explain`.

**The plan calls this "notice defence", and that names the requirement
exactly.** When an officer asks why a credit was treated the way it was, "the
system flagged it" is not an answer. What is an answer: this rule, resting on
this provision, fired because these specific facts held, and those facts came
from this row of this file uploaded on this date.

Reco Now had the finding and the citation and stopped there. graph-owl carries
the derivation and nothing asked for it.

**The chain is assembled here rather than in the UI** because it spans three
sources — the case, the graph's own explanation, and the upload that produced
the facts — and a chain assembled per-screen would differ per screen. A
defence pack that disagrees with the screen it was exported from is worse than
no defence pack.
"""

from __future__ import annotations

import pytest

from app.notice_defence import defence_chain, explain_query


def _case(**overrides) -> dict:
    case = {
        "id": "case-1",
        "invoice_no": "INV-MAR-011",
        "reason_code": "gst:AmountMismatch",
        "governed_by": "gst:Rule36-4",
        "subject": "https://graph-owl.dev/packs/gst#books-27AABCS1429B1Z8-INV-MAR-011",
        "supplier_gstin": "27AABCS1429B1Z8",
        "supplier_name": "Sharma Infrastructure Pvt Ltd",
        "books_amount": 180000.0,
        "portal_amount": 180500.0,
        "summary": "Both sides report the invoice, and the values differ",
    }
    case.update(overrides)
    return case


class TestTheExplainQuery:
    def test_it_asks_about_the_cases_own_subject(self):
        query = explain_query(_case(), predicate="gst:taxAmount", value="180000")

        assert "books-27AABCS1429B1Z8-INV-MAR-011" in query["s"]
        assert query["p"] == "gst:taxAmount"

    def test_a_case_with_no_subject_cannot_be_explained(self):
        """A case whose subject was never recorded has nothing to trace to.
        Guessing an IRI from the invoice number would produce a defence pack
        citing facts about a subject that may not exist."""
        with pytest.raises(ValueError, match="subject"):
            explain_query(_case(subject=None), predicate="gst:taxAmount", value="1")


class TestTheChain:
    def test_the_chain_runs_from_the_figure_to_the_source_row(self):
        """figure → case → fact → source row, in that order. A chain that
        starts at the rule leaves the reader asking where the number came
        from, which is the first thing an officer asks."""
        chain = defence_chain(
            case=_case(),
            explanation={"derivation": [{"rule": "gst:Rule36-4", "because": ["f1"]}]},
            upload={"filename": "purchase_register_mar2026.csv", "uploaded_at": "2026-04-02", "row": 11},
        )

        assert [step["kind"] for step in chain["steps"]] == [
            "figure",
            "finding",
            "provision",
            "derivation",
            "source",
        ]

    def test_the_provision_step_carries_the_citation_not_just_its_name(self):
        chain = defence_chain(case=_case(), explanation={}, upload=None)

        provision = next(s for s in chain["steps"] if s["kind"] == "provision")
        assert provision["citation"] == "gst:Rule36-4"

    def test_a_case_with_no_upload_still_produces_a_chain_that_says_so(self):
        """The source step is the one most likely to be missing — a case can
        outlive the upload record. Omitting the step silently would make the
        chain look complete when its last link is absent."""
        chain = defence_chain(case=_case(), explanation={}, upload=None)

        source = next(s for s in chain["steps"] if s["kind"] == "source")
        assert source["known"] is False
        assert chain["complete"] is False

    def test_a_full_chain_reports_itself_complete(self):
        chain = defence_chain(
            case=_case(),
            explanation={"derivation": [{"rule": "r", "because": ["f1"]}]},
            upload={"filename": "f.csv", "uploaded_at": "2026-04-02", "row": 11},
        )

        assert chain["complete"] is True

    def test_the_figure_step_states_both_sides_where_both_are_known(self):
        """A mismatch is a disagreement, and a chain naming one number invites
        the question "against what?"."""
        chain = defence_chain(case=_case(), explanation={}, upload=None)

        figure = next(s for s in chain["steps"] if s["kind"] == "figure")
        assert figure["books"] == 180000.0
        assert figure["portal"] == 180500.0

    def test_an_absent_amount_is_reported_as_unknown_not_zero(self):
        """The same distinction the working paper draws. A case with no amount
        evidence must not appear in a defence pack asserting zero."""
        chain = defence_chain(
            case=_case(books_amount=None, portal_amount=None), explanation={}, upload=None
        )

        figure = next(s for s in chain["steps"] if s["kind"] == "figure")
        assert figure["books"] is None
        assert figure["known"] is False

    def test_the_derivation_step_says_when_graph_owl_could_not_explain(self):
        """An empty explanation and an unattempted one are different, and a
        defence pack that silently omits the derivation implies there wasn't
        one to give."""
        chain = defence_chain(case=_case(), explanation={}, upload=None)

        derivation = next(s for s in chain["steps"] if s["kind"] == "derivation")
        assert derivation["known"] is False
