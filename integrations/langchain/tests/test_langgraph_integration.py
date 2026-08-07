"""Slice D: a LangGraph agent completes a multi-step investigation against
`GraphOwlToolkit`'s tools — search, then expand on what it found. Proves
the wiring (toolkit -> LangGraph's tool-calling loop -> real MCP calls),
not an LLM's reasoning, so the "model" is a scripted, deterministic
sequence of tool calls rather than a real inference API.
"""

import json
from typing import Any

from langchain_core.language_models.chat_models import BaseChatModel
from langchain_core.messages import AIMessage, BaseMessage
from langchain_core.outputs import ChatGeneration, ChatResult
from langgraph.prebuilt import create_react_agent

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.tools import GraphOwlToolkit

SECRET = "sk-super-secret-token-value"

MANIFEST = [
    {
        "name": "search_assets",
        "description": "Find assets by name or description.",
        "inputSchema": {
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"],
        },
    },
    {
        "name": "get_asset_context",
        "description": "Everything known about one asset.",
        "inputSchema": {
            "type": "object",
            "properties": {"fullyQualifiedName": {"type": "string"}},
            "required": ["fullyQualifiedName"],
        },
    },
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
        method = payload["method"]
        if method == "tools/list":
            body = json.dumps(
                {"jsonrpc": "2.0", "id": payload["id"], "result": {"tools": MANIFEST}}
            )
            return _FakeResponse(body.encode("utf-8"))
        name = payload["params"]["name"]
        if name == "search_assets":
            result = {"hits": [{"fullyQualifiedName": "warehouse.retail.orders"}]}
        elif name == "get_asset_context":
            fqn = payload["params"]["arguments"]["fullyQualifiedName"]
            result = {"fullyQualifiedName": fqn, "description": "Daily order totals."}
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
    """Emits a fixed sequence of tool calls, one per invocation, then a
    plain final answer — a deterministic stand-in for an LLM's reasoning,
    since this test proves the *wiring* works, not that a real model
    reasons well."""

    steps: list[AIMessage] = []
    calls: list[int] = []

    def _generate(
        self, messages: list[BaseMessage], stop: list[str] | None = None, **kwargs: Any
    ) -> ChatResult:
        step = len(self.calls)
        self.calls.append(step)
        message = self.steps[step]
        return ChatResult(generations=[ChatGeneration(message=message)])

    @property
    def _llm_type(self) -> str:
        return "scripted-tool-calling-model"

    def bind_tools(self, tools: Any, **kwargs: Any) -> "_ScriptedToolCallingModel":
        return self


def test_a_langgraph_agent_completes_a_search_then_expand_investigation():
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_server_opener(),
    )
    tools = toolkit.tools()

    model = _ScriptedToolCallingModel(
        steps=[
            AIMessage(
                content="",
                tool_calls=[
                    {"name": "search_assets", "args": {"query": "orders"}, "id": "call-1"}
                ],
            ),
            AIMessage(
                content="",
                tool_calls=[
                    {
                        "name": "get_asset_context",
                        "args": {"fullyQualifiedName": "warehouse.retail.orders"},
                        "id": "call-2",
                    }
                ],
            ),
            AIMessage(content="warehouse.retail.orders holds daily order totals."),
        ]
    )

    agent = create_react_agent(model, tools)
    result = agent.invoke({"messages": [("user", "find the orders table and describe it")]})

    tool_messages = [m for m in result["messages"] if m.type == "tool"]
    assert len(tool_messages) == 2, "both the search and the expand steps must have run"
    assert json.loads(tool_messages[0].content) == {
        "hits": [{"fullyQualifiedName": "warehouse.retail.orders"}]
    }
    assert json.loads(tool_messages[1].content) == {
        "fullyQualifiedName": "warehouse.retail.orders",
        "description": "Daily order totals.",
    }
    final = result["messages"][-1]
    assert "daily order totals" in final.content.lower()
