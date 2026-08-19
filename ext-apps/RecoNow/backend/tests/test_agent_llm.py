"""Do the agents use a model at all, and is it safe when they do?

**They did not.** `agent_runtime` was a trigger bus, a grant check and a cost
counter with no model call anywhere. The LLM layer (`ai.py`) existed with three
call sites, none of them an agent.

**What changed, and what deliberately did not.** An agent now *may* call a
model, and every word it produces passes `grounding.ground_draft` first — so a
model that invents a figure is refused and the computed text is used instead.
The numbers are never the model's to produce: research on AI in Indian tax
practice is consistent that models help on language and document work and
create risk on judgement and figures, and this product's own incident history
agrees (a fabricated "₹8.2 L sits inside the s.16(4) window" shipped once).

**The fallback is the point, not a degradation.** With no model reachable —
which is the current state of this deployment — every agent still produces its
deterministic narrative. A product whose explanations vanish when an inference
server is down is a product that cannot be relied on to explain anything.
"""

from __future__ import annotations

from app.agent_narrator import explain_case


CASE = {
    "invoice_no": "INV-MAR-011",
    "reason_code": "gst:AmountMismatch",
    "supplier_name": "Sharma Infrastructure Pvt Ltd",
    "books_amount": 180000.0,
    "portal_amount": 180500.0,
}


class TestWithNoModelAvailable:
    def test_it_still_explains_the_case(self):
        result = explain_case(CASE, model=None)

        assert result["text"]
        assert "1,80,000" in result["text"]

    def test_it_says_the_text_was_computed_not_generated(self):
        """A reader deciding how much to trust a sentence needs to know which
        produced it."""
        result = explain_case(CASE, model=None)

        assert result["source"] == "computed"


class TestWithAModelAvailable:
    def test_a_faithful_rephrasing_is_used(self):
        def model(_prompt: str) -> str:
            return (
                "Your books record ₹1,80,000 for INV-MAR-011 while the portal shows "
                "₹1,80,500."
            )

        result = explain_case(CASE, model=model)

        assert result["source"] == "model"
        assert "1,80,500" in result["text"]

    def test_a_rephrasing_that_invents_a_figure_is_refused(self):
        """The load-bearing case. A model will do this by default, confidently,
        and about a tax position."""
        def model(_prompt: str) -> str:
            return "INV-MAR-011 is short by ₹8,20,000 and must be reversed."

        result = explain_case(CASE, model=model)

        assert result["source"] == "computed"
        assert "820000" not in result["text"].replace(",", "")
        assert result["refusal"]

    def test_a_model_that_fails_falls_back_rather_than_breaking_the_screen(self):
        def model(_prompt: str) -> str:
            raise RuntimeError("inference server down")

        result = explain_case(CASE, model=model)

        assert result["source"] == "computed"
        assert result["text"]

    def test_a_model_returning_nothing_falls_back(self):
        result = explain_case(CASE, model=lambda _p: "")

        assert result["source"] == "computed"

    def test_the_prompt_carries_the_figures_so_a_faithful_answer_is_possible(self):
        """A model asked to explain a case without being given its numbers can
        only invent them. Refusing what we made unavoidable would be unfair to
        the model and useless to the user."""
        seen: list[str] = []

        explain_case(CASE, model=lambda p: seen.append(p) or "")

        assert "180000" in seen[0].replace(",", "") or "1,80,000" in seen[0]
