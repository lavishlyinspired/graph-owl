"""What is actually wrong with *this* invoice, in a sentence.

**The user's ask**: "`gst:ITCNotAvailable` is not displaying the actual summary
of the mismatch". The pack's guidance says what the *rule* means — generic and
authoritative. This says what happened to *this* invoice: which two numbers
disagree, by how much, and how much of the invoice that is.

**Computed, not generated.** Research on AI in Indian tax practice is
consistent that models help on language and document work and create risk on
judgement and figures. A narrative that states an amount must never be able to
state a wrong one, and the deterministic version cannot: every figure in it is
read from the case, not written by a model.

An LLM may *rephrase* this for fluency, and when it does it goes through
`grounding.ground_draft` — so a rephrasing that invents a number is refused and
the computed sentence is shown instead. That is the whole design: the model can
improve the prose and can never change the facts.
"""

from __future__ import annotations

from app.case_narrative import narrate


def _case(**kw) -> dict:
    case = {
        "invoice_no": "INV-MAR-011",
        "reason_code": "gst:AmountMismatch",
        "supplier_name": "Sharma Infrastructure Pvt Ltd",
        "books_amount": 180000.0,
        "portal_amount": 180500.0,
    }
    case.update(kw)
    return case


class TestAMismatchNarrative:
    def test_it_names_both_figures_and_the_difference(self):
        text = narrate(_case())

        assert "1,80,000" in text
        assert "1,80,500" in text
        assert "500" in text

    def test_it_states_the_difference_as_a_share_of_the_invoice(self):
        """₹500 on ₹1,80,000 and ₹500 on ₹600 are the same absolute number and
        completely different problems. The share is what tells a reviewer
        whether to care."""
        text = narrate(_case())

        assert "0.3%" in text or "0.28%" in text

    def test_it_names_the_supplier_because_that_is_who_gets_called(self):
        assert "Sharma Infrastructure" in narrate(_case())

    def test_it_says_which_side_is_higher(self):
        """"They differ" leaves the reader to work out the direction, and the
        direction decides who is wrong."""
        assert "portal" in narrate(_case()).lower()

        reversed_case = narrate(_case(books_amount=180500.0, portal_amount=180000.0))
        assert "books" in reversed_case.lower()


class TestNarrativesForTheOtherShapes:
    def test_an_invoice_the_supplier_never_filed_says_so(self):
        text = narrate(_case(reason_code="gst:SupplierNotFiled", portal_amount=None))

        assert "not" in text.lower()
        assert "1,80,000" in text

    def test_a_blocked_credit_says_the_credit_is_lost_not_merely_flagged(self):
        text = narrate(_case(reason_code="gst:ITCNotAvailable", portal_amount=None))

        assert "1,80,000" in text
        assert "blocked" in text.lower() or "not available" in text.lower()

    def test_an_overdue_payment_names_the_180_day_test(self):
        text = narrate(_case(reason_code="gst:PaymentOverdue", portal_amount=None))

        assert "180" in text


class TestWhatItRefusesToSay:

    def test_a_case_with_no_amounts_does_not_invent_one(self):
        """The same discipline as the working paper. A case with no amount
        evidence must not produce a sentence containing a figure."""
        text = narrate(_case(books_amount=None, portal_amount=None))

        assert text
        assert not any(ch.isdigit() for ch in text.replace("180", "").replace("INV-MAR-011", ""))

    def test_an_unknown_rule_still_produces_a_usable_sentence(self):
        """A rule added tomorrow must not blank the narrative column."""
        text = narrate(_case(reason_code="gst:SomethingNew"))

        assert "INV-MAR-011" in text

    def test_a_zero_difference_is_not_described_as_a_mismatch(self):
        """Two equal figures reaching this rule is a data problem, and calling
        it a mismatch would send someone to argue about nothing."""
        text = narrate(_case(portal_amount=180000.0))

        # Checks the claim, not a substring: "no difference to resolve"
        # contains "differ" and is exactly the right sentence.
        assert "no difference" in text.lower()
        assert "both sides report" in text.lower()
