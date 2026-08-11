"""P11: a real LangGraph agent completes a multi-hop investigation over the
P10 tools — Epic 105 P11 (`plans/105s-langgraph-investigation-agent.md`).

Targets evaluation question 14 (`packs/gst/eval/questions.md`): "Which
invoices would become compliant if the taxpayer paid them today?" No
existing reference script answers this — `examples/gst-reconcile/
reconcile_agent.py` only routes questions 1-5 through a fixed finding-label
table, and questions 12-15 are explicitly the ones with no agent yet.

Like `test_langgraph_integration.py`, this proves the *wiring* (toolkit ->
LangGraph's tool-calling loop -> real MCP call shapes -> `investigate`'s
return value) with a scripted, deterministic model rather than a real
inference API — matching `test_reconcile_agent.py`'s own precedent of
testing entirely against in-process fakes, no live server and no API key.
"""

import json
import sys
from pathlib import Path
from typing import Any

import pytest
from langchain_core.language_models.chat_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage
from langchain_core.outputs import ChatGeneration, ChatResult

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.tools import GraphOwlToolkit

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "examples"))

from gst_investigation_agent import (  # noqa: E402
    SCORED_QUESTIONS,
    SYSTEM_PROMPT,
    AgentError,
    investigate,
    score_investigation,
)

SECRET = "sk-super-secret-token-value"

#: The real P10 declarations for the two tools this investigation needs —
#: taken from `graph_owl_mcp::lib`'s own `ToolDeclaration`s, not invented.
MANIFEST = [
    {
        "name": "resolve_entity",
        "description": "Given a name or id from unstructured text, find which "
        "real catalog assets it most likely refers to.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer"},
            },
            "required": ["query"],
        },
    },
    {
        "name": "calculate_risk",
        "description": "Every open obligation for one subject — a due date "
        "and how many days remain (negative once overdue).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pack": {"type": "string"},
                "subject": {"type": "string"},
            },
            "required": ["pack", "subject"],
        },
    },
]

#: Wire shapes taken from `graph_owl_mcp::jsonrpc`'s real render arms:
#: `resolve_entity` -> `ResolvedEntityContext` (camelCase), `calculate_risk`
#: -> `{"obligations": [...], "count": N}`. Values are `packs/gst/fixtures`'
#: own INV-1006 (unpaid, still inside its 180-day window as of the pack's
#: `as_of` — question 5's own key already establishes "six days old, not
#: yet due").
RESOLVE_ENTITY_RESULT = {
    "candidates": [
        {
            "fullyQualifiedName": "https://graph-owl.dev/packs/gst#purchase-INV-1006",
            "kind": "purchase",
            "score": 0.81,
        }
    ],
    "truncated": False,
}
CALCULATE_RISK_RESULT = {
    "obligations": [
        {
            "pack": "gst",
            "label": "gst:PaymentOverdue",
            "subject": "https://graph-owl.dev/packs/gst#purchase-INV-1006",
            "governedBy": "gst:Section16-2-d",
            "anchor": "2026-08-05",
            "due": "2027-02-01",
            "daysRemaining": 3,
        }
    ],
    "count": 1,
}


class _FakeResponse:
    def __init__(self, body: bytes):
        self.status = 200
        self._body = body

    def read(self):
        return self._body

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def _server_opener():
    def opener(request):
        payload = json.loads(request.data)
        method = payload["method"]
        if method == "tools/list":
            body = json.dumps(
                {"jsonrpc": "2.0", "id": payload["id"], "result": {"tools": MANIFEST}}
            )
            return _FakeResponse(body.encode("utf-8"))
        name = payload["params"]["name"]
        if name == "resolve_entity":
            result = RESOLVE_ENTITY_RESULT
        elif name == "calculate_risk":
            result = CALCULATE_RISK_RESULT
        else:
            result = {}
        body = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": payload["id"],
                "result": {
                    "content": [{"type": "text", "text": json.dumps(result)}],
                    "isError": False,
                },
            }
        )
        return _FakeResponse(body.encode("utf-8"))

    return opener


class _ScriptedToolCallingModel(BaseChatModel):
    """A fixed sequence of tool calls, one per invocation, then a plain
    final answer. Also records every message list it was handed, so a test
    can inspect what `investigate` actually sent the model — not only what
    the model sent back."""

    steps: list[AIMessage] = []
    calls: list[list[BaseMessage]] = []

    def _generate(
        self, messages: list[BaseMessage], stop: list[str] | None = None, **kwargs: Any
    ) -> ChatResult:
        step = len(self.calls)
        self.calls.append(list(messages))
        message = self.steps[step]
        return ChatResult(generations=[ChatGeneration(message=message)])

    @property
    def _llm_type(self) -> str:
        return "scripted-tool-calling-model"

    def bind_tools(self, tools: Any, **kwargs: Any) -> "_ScriptedToolCallingModel":
        return self


def _toolkit_tools():
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_server_opener(),
    )
    return toolkit.tools()


def test_the_agent_names_only_the_invoice_still_inside_its_window():
    model = _ScriptedToolCallingModel(
        steps=[
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "resolve_entity",
                        "args": {"query": "unpaid GST purchase invoices"},
                        "id": "call-1",
                    }
                ],
            ),
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "calculate_risk",
                        "args": {
                            "pack": "gst",
                            "subject": "https://graph-owl.dev/packs/gst#purchase-INV-1006",
                        },
                        "id": "call-2",
                    }
                ],
            ),
            AIMessage(
                content="INV-1006 would become compliant if paid today "
                "(3 days remain of its 180)."
            ),
        ]
    )

    answer = investigate(
        model,
        _toolkit_tools(),
        "Which invoices would become compliant if the taxpayer paid them today?",
    )

    assert len(model.calls) == 3, "resolve, then calculate_risk, then the final answer"
    tool_messages = [m for m in model.calls[-1] if m.type == "tool"]
    assert len(tool_messages) == 2, "both tool results must reach the model before it answers"
    assert json.loads(tool_messages[0].content) == RESOLVE_ENTITY_RESULT
    assert json.loads(tool_messages[1].content) == CALCULATE_RISK_RESULT
    assert "INV-1006" in answer


def test_score_investigation_scores_a_correct_narration_as_exact():
    """P12's own missing piece: questions 13 and 15 have no fixed-table
    answer (`reconcile_agent.py`'s `QUESTIONS` docstring names exactly
    why), so nothing could score them until a real investigation *and* a
    scoring convention for its prose both existed. This proves the two
    compose correctly, with a scripted model standing in for a real one —
    the same "proves the wiring, not a model's reasoning" posture every
    other test in this file already takes."""
    model = _ScriptedToolCallingModel(
        steps=[
            AIMessage(
                content="INV-1004 is genuinely something else: the supplier "
                "did file it, under a transposed GSTIN."
            )
        ]
    )

    narration, score = score_investigation(model, _toolkit_tools(), 13)

    assert "INV-1004" in narration
    assert score.exact


def test_score_investigation_scores_an_incomplete_narration_honestly():
    """Question 15 expects five invoices; a narration that names only one
    must score partial recall, not be rounded up to correct — the same
    property `score_finding`'s own false-negative test already
    establishes, exercised here through a real investigation call."""
    model = _ScriptedToolCallingModel(
        steps=[AIMessage(content="INV-1003 is at risk for ₹45,000.")]
    )

    _, score = score_investigation(model, _toolkit_tools(), 15)

    assert score.precision == 1.0
    assert score.recall == 1.0 / 5


def test_score_investigation_refuses_a_question_number_it_does_not_cover():
    """Question 12's own absence from `SCORED_QUESTIONS` is deliberate —
    its answer key names no invoice, so the invoice-mention scoring
    convention cannot honestly score it (`SCORED_QUESTIONS`'s own doc
    comment). Silently returning a score of 0 would misreport a genuine
    scope boundary as a failed answer."""
    model = _ScriptedToolCallingModel(steps=[AIMessage(content="10%, Notification 75/2019-CT.")])

    with pytest.raises(AgentError, match="12"):
        score_investigation(model, _toolkit_tools(), 12)


def test_scored_questions_matches_the_answer_key_verbatim():
    """`SCORED_QUESTIONS`'s own text must be the question `questions.md`
    actually asks — a paraphrase would investigate something subtly
    different from what the key was derived against."""
    text, _ = SCORED_QUESTIONS[13]
    assert text == "Is INV-1004 genuinely missing from GSTR-2B, or is something else going on?"


def test_investigate_threads_the_system_prompt_into_every_model_call():
    """`investigate`'s only real logic is wiring `SYSTEM_PROMPT` into
    `create_agent` — the exact thing a mutant deleting that keyword
    argument would remove. `create_agent` does not carry the system
    message in the returned state (checked directly: `result["messages"]`
    starts at the human turn), so the only place to observe it is what the
    model itself was handed."""
    model = _ScriptedToolCallingModel(steps=[AIMessage(content="no obligations are open")])

    investigate(model, _toolkit_tools(), "Is anything overdue?")

    first_call = model.calls[0]
    assert first_call[0].type == "system"
    assert first_call[0].content == SYSTEM_PROMPT
