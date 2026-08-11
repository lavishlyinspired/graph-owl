"""`chat_playground.streaming.run_investigation_stream` — a local-only chat
UI's streaming core (see `examples/chat_playground/streaming.py`'s own
docstring for why this lives outside `graph_owl_langchain`).

Two properties matter and neither is obvious from reading the generator
alone, so both are proven against real (scripted) execution rather than
asserted by inspection:

1. Streaming actually yields progress as it happens (message text plus
   tool-call updates), not just a final blob — the whole reason this
   exists instead of reusing `gst_investigation_agent.py`'s
   `investigate()`.
2. Two investigations running concurrently (the "ask a new question
   while the first still runs" requirement) do not cross-contaminate —
   each one's streamed text belongs to its own question, not the
   other's, even when both are awaited via `asyncio.gather`.

Same scripted-model, in-process-fake-server pattern as
`test_langgraph_integration.py`/`test_gst_investigation_agent.py`: this
proves the wiring, not that a real model reasons well.
"""

import asyncio
import json
import sys
from pathlib import Path
from typing import Any

from langchain_core.language_models.chat_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage
from langchain_core.outputs import ChatGeneration, ChatResult

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.tools import GraphOwlToolkit

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "examples"))

from chat_playground.streaming import run_investigation_stream  # noqa: E402

SECRET = "sk-super-secret-token-value"
SYSTEM_PROMPT = "Investigate using the tools available."

MANIFEST = [
    {
        "name": "search_assets",
        "description": "Find assets by name or description.",
        "inputSchema": {
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"],
        },
    }
]


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
        if payload["method"] == "tools/list":
            body = json.dumps(
                {"jsonrpc": "2.0", "id": payload["id"], "result": {"tools": MANIFEST}}
            )
            return _FakeResponse(body.encode("utf-8"))
        result = {"hits": [{"fullyQualifiedName": "warehouse.retail.orders"}]}
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


def _toolkit_tools():
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_server_opener(),
    )
    return toolkit.tools()


class _ScriptedToolCallingModel(BaseChatModel):
    """A fixed sequence of tool calls, one per invocation, then a plain
    final answer — deterministic, so streamed chunks are predictable."""

    steps: list[AIMessage] = []
    calls: list[int] = []

    def _generate(
        self,
        messages: list[BaseMessage],
        stop: list[str] | None = None,
        run_manager: Any = None,
        **kwargs: Any,
    ) -> ChatResult:
        step = len(self.calls)
        self.calls.append(step)
        return ChatResult(generations=[ChatGeneration(message=self.steps[step])])

    @property
    def _llm_type(self) -> str:
        return "scripted-tool-calling-model"

    def bind_tools(self, tools: Any, **kwargs: Any) -> "_ScriptedToolCallingModel":
        return self


async def _collect(model: Any, question: str, thread_id: str) -> list:
    return [
        chunk
        async for chunk in run_investigation_stream(
            model, _toolkit_tools(), question, thread_id, SYSTEM_PROMPT
        )
    ]


def test_streaming_yields_progress_and_a_final_message_not_just_a_blob():
    model = _ScriptedToolCallingModel(
        steps=[
            AIMessage(
                content="",
                tool_calls=[
                    {"name": "search_assets", "args": {"query": "orders"}, "id": "call-1"}
                ],
            ),
            AIMessage(content="warehouse.retail.orders is the orders table."),
        ]
    )

    chunks = asyncio.run(_collect(model, "find the orders table", "thread-a"))

    kinds = [c.kind for c in chunks]
    assert "update" in kinds, f"expected at least one tool-call update, got {kinds}"
    message_chunks = [c for c in chunks if c.kind == "message"]
    assert message_chunks, "expected at least one message chunk"
    assert any("orders table" in c.text for c in message_chunks), message_chunks


def test_two_concurrent_investigations_do_not_cross_contaminate():
    """The actual promise this playground exists to keep: asking a second
    question does not have to wait for the first to finish, and each
    stream's text belongs to its own question."""
    model_a = _ScriptedToolCallingModel(steps=[AIMessage(content="answer for question A")])
    model_b = _ScriptedToolCallingModel(steps=[AIMessage(content="answer for question B")])

    async def run_both():
        return await asyncio.gather(
            _collect(model_a, "question A", "thread-a"),
            _collect(model_b, "question B", "thread-b"),
        )

    chunks_a, chunks_b = asyncio.run(run_both())

    text_a = "".join(c.text for c in chunks_a if c.kind == "message")
    text_b = "".join(c.text for c in chunks_b if c.kind == "message")

    assert "answer for question A" in text_a
    assert "answer for question B" not in text_a, "thread A leaked thread B's answer"
    assert "answer for question B" in text_b
    assert "answer for question A" not in text_b, "thread B leaked thread A's answer"
