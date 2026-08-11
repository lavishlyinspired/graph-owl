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
this starts.** That script is deterministic — a fixed table routes exactly
5 evaluation questions to a `/findings` query, and a model only narrates an
answer that is already complete. `packs/gst/eval/questions.md`'s own
"Running it" section says "there is no agent yet," and questions 12-15 are
explicitly the ones no fixed table can answer, because each needs the
investigator to decide *which* tool to call next based on what the last
one returned. That is exactly what a LangGraph ReAct agent does and a
routing table cannot: `create_agent` loops model -> tool -> model until the
model stops calling tools, over every one of Epic 105 P10's 8 intelligence
tools (`GraphOwlToolkit` builds them all from a live `tools/list`
manifest, so nothing here hand-lists a tool name).

Unlike `reconcile_agent.py`, there is no deterministic fallback: the model
genuinely decides which subjects to check. `SYSTEM_PROMPT` states the one
fact a model cannot get from a tool's JSON schema alone — the sign of
`calculate_risk`'s `daysRemaining` — because getting that backwards would
tell a taxpayer paying an already-lapsed invoice restores compliance.

    python examples/gst_investigation_agent.py \\
        --question "Which invoices would become compliant if paid today?"

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
from typing import TYPE_CHECKING, Any

from langchain.agents import create_agent

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.tools import GraphOwlToolkit

if TYPE_CHECKING:
    from langchain_core.language_models.chat_models import BaseChatModel

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
    parser.add_argument("--question", required=True)
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
    print(investigate(model, toolkit.tools(), args.question))
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
