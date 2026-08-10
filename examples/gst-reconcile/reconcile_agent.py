"""Answering GST reconciliation questions from the graph — Epic 105 P9.

**The deterministic layer answers; a model only narrates.** Every answer here
is assembled from findings the rules derived and the evidence they cite. The
language model is optional, is handed an answer that is already complete, and
is forbidden from changing which invoices are named — because a model given
wrong findings narrates them fluently and a reviewer believes it.

That ordering is why this file can be scored against
`packs/gst/eval/questions.md` **today, with no model and no API key at all**.
If the structured answers are wrong, no amount of narration fixes them; if
they are right, narration is a presentation concern.

Stdlib only (`urllib`), so `scripts/check-examples-purity.py` passes and the
example runs against a bare Python.

    python examples/gst-reconcile/reconcile_agent.py --question 1
    python examples/gst-reconcile/reconcile_agent.py --all
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass, field

#: Which finding label answers which evaluation question. This is the whole
#: "routing" layer, and it is a table rather than a model call on purpose: a
#: question about missing invoices must not be able to return an answer about
#: reverse charge because a model was feeling creative.
QUESTION_LABELS = {
    1: ("gst:PotentialMismatch", "Claimed in the register, never filed by the supplier"),
    2: ("gst:AmountMismatch", "Values disagree by more than the cap then in force"),
    3: ("gst:ITCNotAvailable", "Matched, and the authority reports no credit available"),
    4: ("gst:Reversed", "Matched, and flagged reverse-charge"),
    5: ("gst:PaymentOverdue", "Unpaid past 180 days of the invoice date"),
}


class AgentError(RuntimeError):
    """The graph could not be reached, or answered something unusable."""


@dataclass
class Answer:
    """A structured answer, before anybody narrates it."""

    question: str
    subjects: list[str]
    citations: list[str] = field(default_factory=list)
    evidence: list[dict] = field(default_factory=list)

    def as_text(self) -> str:
        """The answer without a model, which is the one that gets scored."""
        if not self.subjects:
            return f"{self.question}\n  None."
        lines = [self.question]
        for subject, citation in zip(self.subjects, self.citations, strict=False):
            lines.append(f"  {subject}  (governed by {citation})")
        return "\n".join(lines)


def _get(server: str, path: str, token: str | None) -> object:
    request = urllib.request.Request(f"{server.rstrip('/')}{path}", method="GET")
    if token:
        request.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as refused:
        raise AgentError(f"GET {path} failed: HTTP {refused.code}") from refused
    except urllib.error.URLError as unreachable:
        raise AgentError(f"GET {path} was unreachable: {unreachable.reason}") from unreachable


def findings(server: str, token: str | None, pack: str = "gst") -> list[dict]:
    """Every finding the rules derived for this pack.

    **The only source of an answer.** There is deliberately no path in this
    file from a question to a raw invoice: an assertion that is not a finding
    has not been through the rules, carries no citation, and is exactly the
    kind of claim a compliance tool must not make.
    """
    rows = _get(server, f"/findings?pack={pack}", token)
    if not isinstance(rows, list):
        raise AgentError("/findings did not return a list")
    return rows


def local_name(term: str) -> str:
    cut = max(term.rfind("#"), term.rfind("/"))
    tail = term[cut + 1 :] if cut >= 0 else term
    return tail or term


def answer(question_number: int, rows: list[dict]) -> Answer:
    """One evaluation question, answered from the findings.

    # Raises

    `AgentError` for a question this file does not cover — silently returning
    an empty answer would score as "found nothing", which is a *wrong* answer
    rather than a missing one.
    """
    if question_number not in QUESTION_LABELS:
        raise AgentError(
            f"question {question_number} is not one this example answers "
            f"(it covers {sorted(QUESTION_LABELS)}) — returning nothing would "
            f"score as a wrong answer rather than an absent one"
        )

    label, question = QUESTION_LABELS[question_number]
    # Sorted by subject, not left in the order the server returned. `/findings`
    # is newest-first, so two runs over the same data can print the same answer
    # in different orders — which reads as instability to anyone diffing a
    # report, and makes a test that pins an order pass against a recorded
    # fixture while differing against a live server.
    matching = sorted((r for r in rows if r["label"] == label), key=lambda r: r["subject"])
    return Answer(
        question=question,
        subjects=[local_name(r["subject"]) for r in matching],
        citations=[r["governedBy"] for r in matching],
        evidence=[{"subject": local_name(r["subject"]), "facts": r["evidence"]} for r in matching],
    )


def narrate(
    found: Answer,
    base_url: str,
    model: str,
    api_key: str | None,
    fallback_base_url: str | None = None,
    fallback_model: str | None = None,
) -> str:
    """Optionally ask a model to phrase an answer that is already complete.

    **The prompt states the constraint the architecture depends on**: the model
    may not add, remove or rename an invoice. It is given the conclusion and
    the evidence and asked for prose. If it disobeys, the structured answer
    above is still what gets scored — which is the point of keeping them
    separate.

    Any OpenAI-compatible endpoint. No vendor is named here; the deployment
    picks one through `LLM_API_BASE_URL`.

    **A fallback model, because a reasoning model can stall.** Measured across
    ten real narrations: one free reasoning model returned only its
    chain-of-thought on 2 runs at a 2000-token budget, 0 at 4000, and 1 at
    6000 — where it consumed the entire budget. It expands its thinking to fill
    whatever it is given, so no ceiling makes it certain. A cheaper
    non-reasoning model as second choice is more robust than a bigger number,
    and it is only called when the first produced nothing.
    """
    try:
        return _complete(found, base_url, model, api_key)
    except AgentError as first_failure:
        if not (fallback_base_url and fallback_model):
            raise
        try:
            return _complete(found, fallback_base_url, fallback_model, api_key)
        except AgentError:
            # **The *first* model's failure, explicitly.** A bare `raise` here
            # re-raises the fallback's error instead, which reports a
            # connection problem with the safety net rather than the reasoning
            # stall that actually caused the narration to fail. Caught by the
            # test that asserts which message surfaces.
            raise first_failure from None


def _complete(found: Answer, base_url: str, model: str, api_key: str | None) -> str:
    """One completion against one endpoint."""
    payload = {
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": (
                    "You explain a compliance finding that has already been "
                    "derived. Use only the invoices and citations given. Do "
                    "not add, remove or rename an invoice, and do not offer a "
                    "conclusion the evidence does not state."
                ),
            },
            {
                "role": "user",
                "content": json.dumps(
                    {
                        "question": found.question,
                        "invoices": found.subjects,
                        "citations": found.citations,
                        "evidence": found.evidence,
                    }
                ),
            },
        ],
        # **Generous on purpose, and the number is measured.** A reasoning
        # model spends this budget thinking before it writes anything, so a
        # tight limit returns an empty `content` that looks exactly like a
        # broken endpoint. On a real narration payload one free model used
        # 1315 completion tokens and returned nothing under 800, while another
        # answered in 152. 2000 covers both; a non-reasoning model simply
        # stops early and is billed for what it used.
        # Measured across ten real narrations per budget: at 2000 a reasoning
        # model returned only its chain-of-thought twice, at 4000 never, and
        # at 6000 once — where it used the whole 6000. It expands to fill what
        # it is given, so 4000 is the measured sweet spot rather than a
        # guarantee, and `fallback_model` is what actually makes it reliable.
        "max_tokens": 4000,
    }
    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/chat/completions",
        data=json.dumps(payload).encode("utf-8"),
        method="POST",
    )
    request.add_header("content-type", "application/json")
    # **Named, because the default is blocked.** `urllib` sends
    # `Python-urllib/3.x`, and at least one hosted endpoint sits behind an edge
    # filter that rejects it with `403 error code: 1010` — a browser-signature
    # ban that reads exactly like a bad credential and cost an investigation.
    # The same request from `curl` succeeded, which is what isolated it.
    request.add_header("user-agent", "graph-owl-gst-reconcile/0.1")
    if api_key:
        request.add_header("authorization", f"Bearer {api_key}")
    try:
        with urllib.request.urlopen(request, timeout=90) as response:
            body = json.loads(response.read())
    except urllib.error.HTTPError as refused:
        detail = refused.read().decode("utf-8", errors="replace")[:200]
        raise AgentError(f"the model endpoint refused: HTTP {refused.code} {detail}") from refused
    except urllib.error.URLError as unreachable:
        raise AgentError(f"the model endpoint was unreachable: {unreachable.reason}") from unreachable
    try:
        message = body["choices"][0]["message"]
    except (KeyError, IndexError) as unexpected:
        raise AgentError("the model endpoint returned no message") from unexpected

    content = (message.get("content") or "").strip()
    if content:
        return content
    # A reasoning model that ran out of budget mid-thought leaves `content`
    # empty and its working in a sibling field. Reporting that as prose would
    # put a model's private reasoning in front of a reviewer as though it were
    # the explanation, so this is an error rather than a fallback.
    if message.get("reasoning_content") or message.get("reasoning"):
        raise AgentError(
            "the model returned only reasoning and no answer — usually too "
            "small a token budget for a reasoning model"
        )
    raise AgentError("the model endpoint returned an empty message")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="reconcile_agent",
        description="Answer GST reconciliation questions from derived findings.",
    )
    parser.add_argument("--question", type=int, help=f"one of {sorted(QUESTION_LABELS)}")
    parser.add_argument("--all", action="store_true", help="answer every covered question")
    parser.add_argument(
        "--server", default=os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080")
    )
    parser.add_argument("--token", default=os.environ.get("GRAPH_OWL_TOKEN"))
    parser.add_argument(
        "--narrate",
        action="store_true",
        help="also ask a model to phrase the answer (needs LLM_API_BASE_URL "
        "and LLM_MODEL; the structured answer is printed either way)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if not args.all and args.question is None:
        print("give --question N or --all", file=sys.stderr)
        return 2

    try:
        rows = findings(args.server, args.token)
        wanted = sorted(QUESTION_LABELS) if args.all else [args.question]
        answers = [answer(number, rows) for number in wanted]
    except AgentError as failed:
        print(str(failed), file=sys.stderr)
        return 2

    for found in answers:
        print(found.as_text())
        if args.narrate:
            base_url = os.environ.get("LLM_API_BASE_URL", "")
            model = os.environ.get("LLM_MODEL", "")
            if not (base_url and model):
                # Not a failure: the structured answer above is complete, and
                # saying so is more useful than a stack trace.
                print("  (no LLM_API_BASE_URL/LLM_MODEL set — structured answer only)")
                continue
            try:
                print(
                    "  "
                    + narrate(
                        found,
                        base_url,
                        model,
                        os.environ.get("LLM_API_KEY"),
                        fallback_base_url=os.environ.get("LLM_FALLBACK_BASE_URL", base_url),
                        fallback_model=os.environ.get("LLM_FALLBACK_MODEL"),
                    )
                )
            except AgentError as failed:
                # **The narration failing must not fail the answer.** The
                # finding is derived and cited; prose is a presentation layer.
                print(f"  (narration unavailable: {failed})")
        print()
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
