"""The reconciliation answers, scored against the pack's own answer key.

**These assert the expected answers from `packs/gst/eval/questions.md`**, which
was written before this file existed. That ordering is the whole value: the key
was derived by hand from the fixtures, so a passing test here means the system
agrees with an answer nobody fitted to it.

No model, no API key. Every one of these answers is derived by rules and cited
to a provision; narration is a separate, optional, failable step.
"""

from __future__ import annotations

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from reconcile_agent import AgentError, Answer, answer, findings, local_name, narrate  # noqa: E402

#: The nine findings the real reconciliation produces, in the wire shape
#: `GET /findings` returns. Taken from a real run, not invented.
FINDINGS = [
    {"label": "gst:PotentialMismatch", "subject": "https://graph-owl.dev/packs/gst#pr-INV-1003",
     "governedBy": "gst:Section16-2-aa", "evidence": [{"subject": "s", "predicate": "gst:taxAmount", "value": "45000.00"}]},
    {"label": "gst:PotentialMismatch", "subject": "https://graph-owl.dev/packs/gst#pr-INV-1004",
     "governedBy": "gst:Section16-2-aa", "evidence": [{"subject": "s", "predicate": "gst:taxAmount", "value": "9000.00"}]},
    {"label": "gst:AmountMismatch", "subject": "https://graph-owl.dev/packs/gst#pr-INV-1002",
     "governedBy": "gst:Rule36-4", "evidence": [{"subject": "s", "predicate": "gst:citation", "value": "Notification 40/2021-CT"}]},
    {"label": "gst:AmountMismatch", "subject": "https://graph-owl.dev/packs/gst#pr-INV-2002",
     "governedBy": "gst:Rule36-4", "evidence": [{"subject": "s", "predicate": "gst:citation", "value": "Notification 75/2019-CT"}]},
    {"label": "gst:ITCNotAvailable", "subject": "https://graph-owl.dev/packs/gst#pr-INV-1005",
     "governedBy": "gst:Section17-5", "evidence": [{"subject": "s", "predicate": "gst:itcAvailable", "value": "N"}]},
    {"label": "gst:Reversed", "subject": "https://graph-owl.dev/packs/gst#pr-INV-1006",
     "governedBy": "gst:Section16-2-aa", "evidence": [{"subject": "s", "predicate": "gst:reverseCharge", "value": "R"}]},
    {"label": "gst:GstinTransposition", "subject": "https://graph-owl.dev/packs/gst#pr-INV-1004",
     "governedBy": "gst:MatchingPolicy", "evidence": [{"subject": "s", "predicate": "gst:supplierGstin", "value": "27AABCU9603R1MZ"}]},
    {"label": "gst:PaymentOverdue", "subject": "https://graph-owl.dev/packs/gst#purchase-INV-1003",
     "governedBy": "gst:Section16-2-d", "evidence": [{"subject": "s", "predicate": "gst:atTime", "value": "2027-03-12"}]},
    {"label": "gst:PaymentOverdue", "subject": "https://graph-owl.dev/packs/gst#purchase-INV-2002",
     "governedBy": "gst:Section16-2-d", "evidence": [{"subject": "s", "predicate": "gst:atTime", "value": "2020-07-12"}]},
]


def subjects(number: int) -> list[str]:
    return answer(number, FINDINGS).subjects


def test_question_1_names_the_unfiled_invoices() -> None:
    """Key: INV-1003, and INV-1004 which is also unmatched — under a
    near-identical GSTIN, which question 13 is about."""
    assert subjects(1) == ["pr-INV-1003", "pr-INV-1004"]


def test_question_2_names_both_value_disagreements_and_not_the_compliant_one() -> None:
    """**The load-bearing negative.** INV-2001's 5% delta was inside the 10%
    cap in force in July 2020, so it is not a finding and must not appear."""
    found = subjects(2)

    assert found == ["pr-INV-1002", "pr-INV-2002"]
    assert "pr-INV-2001" not in found


def test_question_2_carries_the_notification_each_was_judged_under() -> None:
    """Citations are part of correctness: the same rule cites a *different*
    notification for a 2020 invoice than for a 2026 one, and an answer that
    blurred them would be wrong even with the right invoices."""
    cited = [
        fact["value"]
        for entry in answer(2, FINDINGS).evidence
        for fact in entry["facts"]
        if fact["predicate"].endswith("citation")
    ]

    assert cited == ["Notification 40/2021-CT", "Notification 75/2019-CT"]


def test_question_3_names_the_matched_invoice_with_no_usable_credit() -> None:
    """The most valuable question in the key: every number agrees and the
    credit still cannot be claimed."""
    assert subjects(3) == ["pr-INV-1005"]


def test_question_4_names_the_reverse_charge_invoice() -> None:
    assert subjects(4) == ["pr-INV-1006"]


def test_question_5_names_the_overdue_and_not_the_merely_unpaid() -> None:
    """INV-1006 is unpaid and six days old — not due, not a finding. Its
    absence here is the assertion."""
    found = subjects(5)

    assert found == ["purchase-INV-1003", "purchase-INV-2002"]
    assert not any("INV-1006" in s for s in found)


def test_the_clean_invoice_appears_in_no_answer_at_all() -> None:
    """Key question 6: INV-1001 is compliant on every count, and a system
    biased toward finding problems invents one."""
    every = [s for number in sorted([1, 2, 3, 4, 5]) for s in subjects(number)]

    assert not any("INV-1001" in s for s in every)


def test_question_6_answers_the_clean_invoice_with_no_finding_of_any_kind() -> None:
    """Key: 'Yes — matched, credit available, not reverse-charge, paid in 20
    days. No finding of any kind.' A system biased toward finding problems
    would invent one; the correct structured answer names nothing."""
    assert subjects(6) == []


def test_question_7_answers_the_within_cap_delta_as_no_finding() -> None:
    """Key: 'No. The invoice is dated July 2020, when Rule 36(4) allowed a
    10% provisional cap.' INV-2001's 5% delta must not appear as an
    AmountMismatch — the identical delta on a 2026 invoice *is* a finding,
    so this is the load-bearing negative the cap resolution exists for."""
    assert subjects(7) == []


def test_question_8_names_only_the_invoice_over_its_own_periods_cap() -> None:
    """Key: same rule, same period, different magnitude — INV-2002's 20%
    delta is a finding against the 10% cap; INV-2001's 5% is not, even
    though the question asks about both."""
    assert subjects(8) == ["pr-INV-2002"]


def test_question_9_answers_that_a_perfect_match_still_has_no_credit() -> None:
    """Key: 'No. Matching is necessary, not sufficient.' Same finding as
    question 3, reached by asking about the invoice directly rather than
    the class of invoices — the two must agree, or the routing table is
    quietly encoding two different answers for one finding."""
    assert subjects(9) == subjects(3) == ["pr-INV-1005"]


def test_question_10_answers_that_the_unpaid_invoice_is_not_yet_a_problem() -> None:
    """Key: 'No payment event exists for it — and it is not a problem: the
    invoice is six days old and the 180 days have not elapsed.' Absence of
    a PaymentOverdue finding is the correct answer, not absence of data."""
    assert subjects(10) == []


def test_question_11_surfaces_the_unresolved_transposition_rather_than_choosing() -> None:
    """Key: 'Unresolved, deliberately... the pack surfaces the pair rather
    than choosing.' An answer that silently picked one GSTIN would have
    performed the merge the matching policy forbids."""
    assert subjects(11) == ["pr-INV-1004"]


def test_a_subject_with_no_recognisable_invoice_number_is_refused() -> None:
    """A candidate-scoped question (7-10) filters findings by invoice
    number extracted from the subject's local name. A subject this cannot
    place must not silently fall out of every candidate filter, which
    would read as 'not a finding' for the wrong reason."""
    malformed = [
        {"label": "gst:AmountMismatch", "subject": "https://graph-owl.dev/packs/gst#pr-not-an-invoice",
         "governedBy": "gst:Rule36-4", "evidence": []},
    ]
    with pytest.raises(AgentError, match="invoice number"):
        answer(7, malformed)


def test_a_question_outside_the_covered_set_fails_rather_than_answering_nothing() -> None:
    """An empty answer scores as "found nothing", which is a *wrong* answer
    rather than an absent one — and would quietly inflate a recall score."""
    with pytest.raises(AgentError, match="not one this example answers"):
        answer(99, FINDINGS)


def test_an_answer_reads_as_prose_without_any_model() -> None:
    text = answer(3, FINDINGS).as_text()

    assert "pr-INV-1005" in text
    assert "gst:Section17-5" in text


def test_an_empty_answer_says_none_rather_than_printing_a_bare_heading() -> None:
    assert "None." in Answer(question="q", subjects=[]).as_text()


def test_local_name_survives_a_term_with_no_separator() -> None:
    assert local_name("bare") == "bare"
    assert local_name("https://x/ns#Term") == "Term"


class _Endpoint:
    """A model double, so narration is testable with no key and no network."""

    def __init__(self, status: int = 200, body: dict | None = None) -> None:
        payload = body if body is not None else {
            "choices": [{"message": {"content": "INV-1005 carries no usable credit."}}]
        }
        outer_status, outer_body = status, json.dumps(payload).encode()

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                self.send_response(outer_status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(outer_body)))
                self.end_headers()
                self.wfile.write(outer_body)

            def log_message(self, *args: object) -> None:
                pass

        self._http = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self._http.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self._http.server_port}"

    def close(self) -> None:
        self._http.shutdown()


def test_narration_returns_the_models_prose_when_one_is_configured() -> None:
    endpoint = _Endpoint()
    try:
        text = narrate(answer(3, FINDINGS), endpoint.url, "any-model", "key")
    finally:
        endpoint.close()

    assert "INV-1005" in text


def test_a_refused_model_is_an_error_the_caller_can_report_not_a_crash() -> None:
    """**Exactly the case seen with a live key**: the endpoint answers `401
    Invalid token`. The finding is still derived and cited, so the answer
    stands and only the prose is missing."""
    endpoint = _Endpoint(status=401, body={"error": "Invalid token"})
    try:
        with pytest.raises(AgentError, match="401"):
            narrate(answer(3, FINDINGS), endpoint.url, "any-model", "bad-key")
    finally:
        endpoint.close()


def test_a_model_returning_no_message_is_an_error_rather_than_an_empty_answer() -> None:
    endpoint = _Endpoint(body={"choices": []})
    try:
        with pytest.raises(AgentError, match="no message"):
            narrate(answer(3, FINDINGS), endpoint.url, "any-model", "key")
    finally:
        endpoint.close()


def test_findings_refuses_a_server_that_does_not_return_a_list() -> None:
    """An error page deserializing into "no findings" would report a clean
    reconciliation — the same failure the GSTR-2B connector guards against."""

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802
            body = json.dumps({"error": "nope"}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args: object) -> None:
            pass

    server = HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with pytest.raises(AgentError, match="did not return a list"):
            findings(f"http://127.0.0.1:{server.server_port}", None)
    finally:
        server.shutdown()


def test_a_reasoning_model_that_never_reached_an_answer_is_an_error() -> None:
    """**Measured, not imagined.** Three of the four free models returned an
    empty `content` with their working in `reasoning_content` when given too
    small a token budget. Printing that as the explanation would put a model's
    private chain-of-thought in front of a reviewer as though it were the
    reasoning behind a compliance finding — which it is not, since the finding
    was derived by rules before the model saw anything."""
    endpoint = _Endpoint(
        body={"choices": [{"message": {"content": "", "reasoning_content": "Let me think..."}}]}
    )
    try:
        with pytest.raises(AgentError, match="only reasoning"):
            narrate(answer(3, FINDINGS), endpoint.url, "any-model", "key")
    finally:
        endpoint.close()


def test_an_empty_message_with_no_reasoning_is_a_plain_error() -> None:
    endpoint = _Endpoint(body={"choices": [{"message": {"content": ""}}]})
    try:
        with pytest.raises(AgentError, match="empty message"):
            narrate(answer(3, FINDINGS), endpoint.url, "any-model", "key")
    finally:
        endpoint.close()


def test_a_stalled_reasoning_model_falls_back_to_a_second_one() -> None:
    """**Measured, and the reason a fallback exists at all.**

    A reasoning model expands its thinking to fill whatever budget it is
    given: `deepseek-v4-flash-free` returned only reasoning on 2 of 10 real
    narrations at 2000 tokens, 0 of 10 at 4000, and 1 of 10 at 6000 — where it
    consumed the entire 6000. No ceiling makes it certain, so the answer is a
    second model rather than a bigger number.
    """
    stalled = _Endpoint(
        body={"choices": [{"message": {"content": "", "reasoning_content": "thinking..."}}]}
    )
    working = _Endpoint(body={"choices": [{"message": {"content": "INV-1005 has no credit."}}]})
    try:
        text = narrate(
            answer(3, FINDINGS), stalled.url, "reasoning-model", "key",
            fallback_base_url=working.url, fallback_model="simpler-model",
        )
    finally:
        stalled.close()
        working.close()

    assert "INV-1005" in text


def test_a_fallback_is_not_used_when_the_first_model_answers() -> None:
    """The fallback is insurance, not a second opinion — spending two calls on
    every narration would double the cost for nothing."""
    working = _Endpoint(body={"choices": [{"message": {"content": "first model"}}]})
    unreachable = "http://127.0.0.1:1"
    try:
        text = narrate(
            answer(3, FINDINGS), working.url, "m", "key",
            fallback_base_url=unreachable, fallback_model="never-called",
        )
    finally:
        working.close()

    assert text == "first model"


def test_both_models_failing_reports_the_original_failure() -> None:
    """The first model's error is the one worth showing — the fallback's is a
    detail about the safety net, not about what went wrong."""
    stalled = _Endpoint(body={"choices": [{"message": {"content": "", "reasoning": "..."}}]})
    try:
        with pytest.raises(AgentError, match="only reasoning"):
            narrate(
                answer(3, FINDINGS), stalled.url, "m", "key",
                fallback_base_url="http://127.0.0.1:1", fallback_model="also-broken",
            )
    finally:
        stalled.close()
