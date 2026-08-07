"""Slice D RED: `GraphOwlClient.list_tools()` — the `tools/list` JSON-RPC
method, manifest discovery for the toolkit.

**Different unwrapping from `call_tool`**: `tools/list`'s handler returns
`{"tools": [...]}` directly as the JSON-RPC `result` — no `content[0].text`
double-encoding, which is specific to `tools/call`'s response construction
(`graph_owl_mcp::jsonrpc::tool_response`). Reusing `call_tool`'s `_unwrap`
here would be wrong, not merely redundant.
"""

import json

from graph_owl_langchain._core.client import GraphOwlClient
from graph_owl_langchain._core.principal import Principal

SECRET = "sk-super-secret-token-value"


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


def _list_tools_response(tools):
    envelope = {"jsonrpc": "2.0", "id": 1, "result": {"tools": tools}}
    return json.dumps(envelope).encode("utf-8")


def test_list_tools_calls_the_tools_list_method_with_no_params():
    captured = {}

    def opener(request):
        captured["body"] = json.loads(request.data)
        return _FakeResponse(_list_tools_response([]))

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal", principal=Principal(token=SECRET), opener=opener
    )
    client.list_tools()

    assert captured["body"]["method"] == "tools/list"
    assert "params" not in captured["body"]


def test_list_tools_returns_the_declared_tools_unmodified():
    declared = [
        {
            "name": "search_assets",
            "description": "Find assets by name.",
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}},
        }
    ]

    def opener(request):
        return _FakeResponse(_list_tools_response(declared))

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal", principal=Principal(token=SECRET), opener=opener
    )
    tools = client.list_tools()

    assert tools == declared


def test_list_tools_on_an_empty_manifest_returns_an_empty_list():
    def opener(request):
        return _FakeResponse(_list_tools_response([]))

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal", principal=Principal(token=SECRET), opener=opener
    )
    assert client.list_tools() == []
