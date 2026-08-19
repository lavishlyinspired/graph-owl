"""An agent may only state a number that appears in a fact it cites.

**Plan 123 §5's own words: "the rule that makes this safe (unchanged, and
load-bearing)".** This console has already shipped a fabricated
"₹8.2 L sits inside the s.16(4) window" once. An LLM will do that by default,
confidently, and about a tax position — which is the worst possible subject
for a confident invention, because the reader has no way to tell it from the
figures that came out of the reconciliation.

The rule is not "the model should try to be accurate". It is mechanical: every
claim carries the ids of the facts supporting it, every **number** in the
claim's text must appear in one of those facts, and a claim that fails is
**rejected before render** and the rejection recorded. A model that cannot
support a figure must say so.

**Why rejection is logged rather than silently dropped**: for an agentic
product the record of what an agent tried and was refused is worth more than
the record of what it produced. A refusal nobody counts is a safety property
nobody can audit.
"""

from __future__ import annotations

import pytest

from app.grounding import (
    GroundingError,
    check_claim,
    numbers_in,
    render_claim,
)

FACTS = {
    "f1": {"predicate": "gst:taxAmount", "value": "45000"},
    "f2": {"predicate": "gst:invoiceNumber", "value": "INV-MAR-011"},
    "f3": {"predicate": "gst:supplierGstin", "value": "27AABCS1429B1Z8"},
}


class TestFindingNumbersInText:
    def test_plain_integers_are_found(self):
        assert numbers_in("₹45000 is at risk") == {"45000"}

    def test_grouped_indian_figures_are_found_without_their_separators(self):
        """A model writes ₹1,80,000 and the fact holds 180000. Comparing them
        as written would reject a claim that is perfectly well supported —
        and a rule that rejects true statements gets switched off."""
        assert numbers_in("₹1,80,000 unclaimed") == {"180000"}

    def test_decimals_keep_their_fractional_part(self):
        """Canonical form, so `45000.50` and `45000.5` compare equal — a
        fractional part that *matters* survives, a trailing zero that does not
        is dropped. `45000.5` is not `45000`, which is the property the rule
        depends on."""
        assert numbers_in("45000.50 exactly") == {"45000.5"}
        assert numbers_in("45000.5") != numbers_in("45000")

    def test_a_trailing_zero_decimal_matches_its_integer_form(self):
        """`45000.00` and `45000` are the same amount. The graph writes one and
        a model writes the other, and neither is wrong."""
        assert numbers_in("45000.00") == numbers_in("45000")

    def test_prose_with_no_figures_yields_nothing(self):
        assert numbers_in("the supplier has not filed") == set()

    def test_a_year_inside_an_invoice_number_is_not_treated_as_a_free_number(self):
        """`INV-2026-001` contains digits that are part of an identifier, not a
        claim about an amount. Treating them as free numbers would reject every
        claim that names an invoice."""
        assert numbers_in("INV-2026-001 is unmatched") == set()


class TestTheRule:
    def test_a_claim_whose_figures_all_appear_in_its_facts_passes(self):
        check_claim(text="₹45000 is at risk on INV-MAR-011", fact_ids=["f1", "f2"], facts=FACTS)

    def test_a_claim_stating_a_figure_no_cited_fact_carries_is_refused(self):
        """The exact failure this exists for: a confident number nobody
        computed."""
        with pytest.raises(GroundingError, match="8200000"):
            check_claim(text="₹82,00,000 sits inside the window", fact_ids=["f1"], facts=FACTS)

    def test_citing_a_fact_that_does_not_exist_is_refused(self):
        """A citation to nothing is worse than no citation: it looks like
        support."""
        with pytest.raises(GroundingError, match="unknown"):
            check_claim(text="₹45000", fact_ids=["f-nope"], facts=FACTS)

    def test_a_claim_with_figures_and_no_citations_at_all_is_refused(self):
        with pytest.raises(GroundingError, match="no facts"):
            check_claim(text="₹45000 is at risk", fact_ids=[], facts=FACTS)

    def test_prose_with_no_figures_needs_no_citations(self):
        """The rule governs *numbers*. "The supplier has not filed" is a
        qualitative statement the surrounding finding already supports, and
        demanding a fact id for it would make the rule unusable and therefore
        switched off."""
        check_claim(text="the supplier has not filed", fact_ids=[], facts=FACTS)

    def test_a_figure_supported_by_an_uncited_fact_is_still_refused(self):
        """Support must be *cited*, not merely available. Otherwise the check
        degrades into "is this number anywhere in the database", which is not
        a claim about this case at all."""
        with pytest.raises(GroundingError):
            check_claim(text="₹45000 at risk", fact_ids=["f2"], facts=FACTS)


class TestDigitsInsideASuppliedIdentifier:
    """**Found 19 August 2026 by running a real model against real data.**

    The model wrote a genuinely accurate explanation and was refused for
    "states 003, 06" — fragments of `INV-MAR-003` and the GSTIN
    `06AAKCA0977G1Z3`, both of which were supplied. `numbers_in` correctly
    declines to read digits *inside* an identifier as an amount, but phrasing
    varies: an identifier split across a line, quoted, or written with a
    different dash reaches the checker as a bare number.

    **A number appearing within a supplied identifier is supported by it.**
    Refusing otherwise makes the rule non-deterministic from the reader's point
    of view — the same true explanation passes or fails depending on how the
    model happened to punctuate — and a safety rule that refuses correct
    statements at random is one that gets switched off.

    The property that matters is unchanged: a figure appearing in **no** fact,
    identifier or otherwise, is still refused.
    """

    FACTS = {
        "invoice": {"value": "INV-MAR-003"},
        "gstin": {"value": "06AAKCA0977G1Z3"},
        "tax": {"value": "30000"},
    }

    def test_a_fragment_of_a_supplied_invoice_number_is_supported(self):
        check_claim(text="Invoice 003 is the one", fact_ids=["invoice"], facts=self.FACTS)

    def test_a_fragment_of_a_supplied_gstin_is_supported(self):
        check_claim(text="the 06 state code", fact_ids=["gstin"], facts=self.FACTS)

    def test_a_figure_in_no_supplied_fact_is_still_refused(self):
        """The property the whole rule exists for, unchanged."""
        with pytest.raises(GroundingError, match="820000"):
            check_claim(text="short by 8,20,000", fact_ids=["tax"], facts=self.FACTS)

    def test_a_fragment_of_an_uncited_identifier_is_not_supported(self):
        """Support must still be *cited*. Otherwise the relaxation becomes "is
        this digit anywhere in the database", which is not a claim about this
        case."""
        with pytest.raises(GroundingError):
            check_claim(text="the 06 state code", fact_ids=["tax"], facts=self.FACTS)


class TestRenderingRefusesRatherThanFabricates:
    def test_a_grounded_claim_renders(self):
        rendered = render_claim(
            text="₹45000 is at risk on INV-MAR-011", fact_ids=["f1", "f2"], facts=FACTS
        )

        assert rendered["text"].startswith("₹45000")
        assert rendered["grounded"] is True

    def test_an_ungrounded_claim_renders_a_refusal_not_the_claim(self):
        """The plan's own RED: an agent asked to summarise a case with no
        evidence returns "not enough evidence", **not prose**."""
        rendered = render_claim(text="₹82,00,000 is at risk", fact_ids=[], facts=FACTS)

        assert rendered["grounded"] is False
        assert "not enough evidence" in rendered["text"].lower()
        assert "82" not in rendered["text"]

    def test_the_refusal_says_which_figure_could_not_be_supported(self):
        """A refusal a reviewer cannot act on is only slightly better than a
        fabrication."""
        rendered = render_claim(text="₹82,00,000 is at risk", fact_ids=["f1"], facts=FACTS)

        assert "8200000" in rendered["reason"]

    def test_every_refusal_is_recorded(self):
        """For an agentic product the record of what an agent tried and was
        refused is worth more than the record of what it produced. A refusal
        nobody counts is a safety property nobody can audit."""
        log: list[dict] = []

        render_claim(text="₹82,00,000", fact_ids=[], facts=FACTS, log=log)
        render_claim(text="₹45000", fact_ids=["f1"], facts=FACTS, log=log)

        assert len(log) == 1
        assert log[0]["refused"] is True
        assert "8200000" in log[0]["reason"]


class TestGroundingAModelDraft:
    """The rule applied where a model's output actually reaches a reader.

    `ai.draft_follow_up` sends an ITC figure to a language model and renders
    whatever comes back. "No invented figures" appears in a system prompt in
    this codebase already — which is a request, not a control. This is the
    control.
    """

    def test_a_draft_repeating_the_supplied_figure_is_grounded(self):
        from app.grounding import ground_draft

        result = ground_draft(
            draft="Please file your GSTR-1 so we may claim INR 45,000.",
            supplied={"itc": 45000, "invoice_no": "INV-MAR-011"},
        )

        assert result["grounded"] is True

    def test_a_draft_inventing_a_figure_is_refused(self):
        """The exact shape of the incident this exists for: a plausible,
        confident, wrong amount in a document that goes to a third party."""
        from app.grounding import ground_draft

        result = ground_draft(
            draft="Please file your GSTR-1 so we may claim INR 8,20,000.",
            supplied={"itc": 45000, "invoice_no": "INV-MAR-011"},
        )

        assert result["grounded"] is False
        assert "820000" not in result["text"]

    def test_a_draft_naming_the_invoice_is_not_refused_over_its_digits(self):
        """An invoice number is an identifier, not an amount. A rule that
        refused every draft naming one would be switched off within a day."""
        from app.grounding import ground_draft

        result = ground_draft(
            draft="Invoice INV-2026-001 for INR 45,000 remains unfiled.",
            supplied={"itc": 45000, "invoice_no": "INV-2026-001"},
        )

        assert result["grounded"] is True

    def test_a_purely_qualitative_draft_needs_no_figures(self):
        from app.grounding import ground_draft

        result = ground_draft(
            draft="Please confirm when you expect to file.", supplied={"itc": 45000}
        )

        assert result["grounded"] is True

    def test_rounded_restatements_of_a_supplied_figure_are_still_refused(self):
        """"INR 45 thousand" is fine — no digits. "INR 45,001" is not, however
        close. A tolerance here would be a licence to be approximately wrong
        about money."""
        from app.grounding import ground_draft

        result = ground_draft(draft="about INR 45,001", supplied={"itc": 45000})

        assert result["grounded"] is False
