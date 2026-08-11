"""A LangGraph agent that investigates GST reconciliation questions by
calling graph-owl's own MCP tools — Epic 105 P11
(`plans/105s-langgraph-investigation-agent.md`).

**Lives outside the `graph_owl_langchain` package on purpose.** That
package's own description says "never a chain, an agent, or a runtime"
(`pyproject.toml`, `plans/00j-language-boundaries.md`,
`plans/ROADMAP.md`'s refusal table). `pyproject.toml`'s
`[tool.setuptools.packages.find]` includes only `graph_owl_langchain*`, so
nothing under this directory ships — this is a worked reference for what a
*user's* agent looks like, sibling to `tests/` in the same way `tests/`
already sits unshipped alongside the package.

**Where `examples/gst-reconcile/reconcile_agent.py` (Epic 105 P9) stops,
this starts.** That script is deterministic — a fixed table routes eleven
evaluation questions (1-11) to a `/findings` query, and a model only
narrates an answer that is already complete. Questions 12, 13 and 15 are
the ones no fixed table can answer safely (`reconcile_agent.py`'s own
`QUESTIONS` docstring states exactly why for each), because each needs the
investigator to decide *which* tool to call next based on what the last
one returned, or a citation choice a label filter cannot make without
guessing. That is exactly what a LangGraph ReAct agent does and a routing
table cannot: `create_agent` loops model -> tool -> model until the model
stops calling tools, over every one of Epic 105 P10's 8 intelligence tools
(`GraphOwlToolkit` builds them all from a live `tools/list` manifest, so
nothing here hand-lists a tool name).

Unlike `reconcile_agent.py`, there is no deterministic fallback: the model
genuinely decides which subjects to check. `SYSTEM_PROMPT` states the one
fact a model cannot get from a tool's JSON schema alone — the sign of
`calculate_risk`'s `daysRemaining` — because getting that backwards would
tell a taxpayer paying an already-lapsed invoice restores compliance.

    python examples/gst_investigation_agent.py \\
        --question "Which invoices would become compliant if paid today?"

    python examples/gst_investigation_agent.py --score-all
        # runs and scores questions 13 and 15 against packs/gst/eval/
        # questions.md's own key, using eval_scoring.score_narration
        # (question 12's key names no invoice, so this scoring
        # convention cannot honestly cover it — SCORED_QUESTIONS'S own
        # doc comment)

Needs `LLM_API_BASE_URL`, `LLM_MODEL`, and optionally `LLM_API_KEY` for a
real model (any OpenAI-compatible endpoint, matching
`reconcile_agent.py`'s own env-var names) plus `pip install
langchain-openai` — not a `graph_owl_langchain` dependency, since nothing
in the package itself needs a concrete model provider.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Any

from langchain.agents import create_agent

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.tools import GraphOwlToolkit

# `eval_scoring.py` is a sibling reference example's pure module (stdlib
# only, no LangChain dependency), not a package this repository publishes
# — reached the same way `examples/gst-reconcile`'s own test files reach
# `reconcile_agent.py`, rather than duplicating the scoring math here.
sys.path.insert(
    0, str(Path(__file__).resolve().parents[3] / "examples" / "gst-reconcile")
)
from eval_scoring import FindingScore, score_narration, wilson_interval  # noqa: E402

if TYPE_CHECKING:
    from langchain_core.language_models.chat_models import BaseChatModel

#: Questions 13 and 15 — `packs/gst/eval/questions.md`'s own text and
#: expected invoice set, scoreable by `score_narration` because their
#: correct answer *is* a set of invoices.
#:
#: **Question 12 is deliberately absent.** Its answer key is a rate and a
#: notification number ("10%, Notification 75/2019-CT"), naming no
#: invoice at all — `score_narration`'s invoice-mention convention has
#: nothing to compare against an empty expected set except "did the text
#: also name no invoice," which would score a wrong answer as correct by
#: coincidence. Scoring it needs a citation-matching convention this
#: slice does not build, rather than forcing it through one that would
#: give a confident, wrong number.
SCORED_QUESTIONS: dict[int, tuple[str, list[str]]] = {
    13: (
        "Is INV-1004 genuinely missing from GSTR-2B, or is something else going on?",
        ["INV-1004"],
    ),
    15: (
        "For each unmatched or disputed July 2026 invoice, what is the "
        "total tax at risk, and under which provision?",
        ["INV-1002", "INV-1003", "INV-1004", "INV-1005", "INV-1006"],
    ),
}

#: The one piece of domain knowledge no tool schema states on its own:
#: `Catalog::obligations_from_rows`'s own doc comment establishes that a
#: row whose discharging event is already bound never appears in
#: `calculate_risk`'s output — so every obligation it reports is, by
#: construction, unpaid, and the only judgment left is the sign of how
#: many days remain.
SYSTEM_PROMPT = (
    "You investigate GST reconciliation questions using graph-owl's tools. "
    "Call tools to gather evidence before answering; never state a finding "
    "you did not retrieve. "
    "`calculate_risk` reports only *open* obligations — every one it "
    "returns is already unpaid. Its `daysRemaining` is the deciding "
    "number: zero or positive means the 180-day window is still open, so "
    "paying today would make that invoice compliant. Negative means the "
    "window already closed — the breach already happened, and paying now "
    "does not undo it. Never call an invoice 'compliant if paid today' "
    "when its daysRemaining is negative."
)


class AgentError(RuntimeError):
    """The model endpoint could not be reached or was not configured."""


def investigate(model: BaseChatModel, tools: list[Any], question: str) -> str:
    """Run one question through a real tool-calling loop and return the
    final message's text.

    This is the entire LangGraph-specific surface: `create_agent` already
    is the investigation (model -> tool -> model until it stops calling
    tools), so there is nothing to add beyond wiring `SYSTEM_PROMPT` in
    and reading the last message back out — matching Decision 8
    ("framework-agnostic core, thin framework shims") already on record
    for Epic 43.
    """
    agent = create_agent(model, tools, system_prompt=SYSTEM_PROMPT)
    result = agent.invoke({"messages": [("user", question)]})
    return result["messages"][-1].content


def score_investigation(
    model: BaseChatModel, tools: list[Any], question_number: int
) -> tuple[str, FindingScore]:
    """Run one of `SCORED_QUESTIONS` through a real investigation and score
    the answer against `questions.md`'s own key.

    **This is the path P12 was missing, built rather than approximated**:
    questions 12-15 have no fixed-table answer (`reconcile_agent.py`'s own
    `QUESTIONS` docstring), so nothing could score them until both a real
    tool-calling run *and* a scoring convention for its prose existed.
    Returns the narration too, not only the score — a 0.0 recall with no
    text to look at is a number nobody can debug.

    # Raises

    `AgentError` for a question number `SCORED_QUESTIONS` does not cover.
    """
    if question_number not in SCORED_QUESTIONS:
        raise AgentError(
            f"question {question_number} is not one this script scores "
            f"(it covers {sorted(SCORED_QUESTIONS)})"
        )
    text, expected = SCORED_QUESTIONS[question_number]
    narration = investigate(model, tools, text)
    return narration, score_narration(narration, expected)


def build_chat_model() -> BaseChatModel:
    """A real OpenAI-compatible chat model from the environment, matching
    `reconcile_agent.py`'s own `LLM_API_BASE_URL`/`LLM_MODEL`/
    `LLM_API_KEY` convention so the two reference scripts configure the
    same way.

    Imports `langchain_openai` lazily so neither this module nor its test
    ever requires it — only a real run does.
    """
    base_url = os.environ.get("LLM_API_BASE_URL")
    model_name = os.environ.get("LLM_MODEL")
    if not (base_url and model_name):
        raise AgentError(
            "set LLM_API_BASE_URL and LLM_MODEL to run against a real model "
            "(needs `pip install langchain-openai`)"
        )
    try:
        from langchain_openai import ChatOpenAI
    except ImportError as missing:
        raise AgentError(
            "langchain_openai is not installed — run `pip install langchain-openai`"
        ) from missing
    from pydantic import SecretStr

    return ChatOpenAI(
        base_url=base_url,
        model=model_name,
        api_key=SecretStr(os.environ.get("LLM_API_KEY", "unused")),
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="gst_investigation_agent",
        description="Investigate a GST reconciliation question with a real tool-calling agent.",
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--question")
    group.add_argument(
        "--score-all",
        action="store_true",
        help=f"run and score every question in {sorted(SCORED_QUESTIONS)} against "
        f"packs/gst/eval/questions.md's own key",
    )
    parser.add_argument(
        "--server", default=os.environ.get("GRAPH_OWL_SERVER", "http://localhost:8080")
    )
    parser.add_argument("--token", default=os.environ.get("GRAPH_OWL_TOKEN"))
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if not args.token:
        print("give --token or set GRAPH_OWL_TOKEN", file=sys.stderr)
        return 2

    try:
        model = build_chat_model()
    except AgentError as failed:
        print(str(failed), file=sys.stderr)
        return 2

    toolkit = GraphOwlToolkit(endpoint=args.server, principal=Principal(token=args.token))

    if args.score_all:
        successes = 0
        for number in sorted(SCORED_QUESTIONS):
            narration, score = score_investigation(model, toolkit.tools(), number)
            print(f"Q{number}: precision={score.precision:.2f} recall={score.recall:.2f}")
            print(f"  {narration}\n")
            successes += 1 if score.exact else 0
        lower, upper = wilson_interval(successes, len(SCORED_QUESTIONS))
        print(
            f"{successes}/{len(SCORED_QUESTIONS)} exact — 95% interval "
            f"[{lower:.2f}, {upper:.2f}]"
        )
        return 0

    print(investigate(model, toolkit.tools(), args.question))
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
