"""Slice D RED: `GraphOwlToolkit` — every exposed tool built from the live
MCP manifest, never a hand-maintained list (decision 5's "no invented
composites" falls out of this for free: nothing here can add a tool that
was not declared).
"""

import json

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.tools import GraphOwlToolkit

SECRET = "sk-super-secret-token-value"

MANIFEST = [
    {
        "name": "search_assets",
        "description": "Find assets by name or description.",
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


def _manifest_opener(manifest, call_results=None):
    call_results = call_results or {}

    def opener(request):
        payload = json.loads(request.data)
        method = payload["method"]
        if method == "tools/list":
            envelope = {"jsonrpc": "2.0", "id": payload["id"], "result": {"tools": manifest}}
            return _FakeResponse(json.dumps(envelope).encode("utf-8"))
        # tools/call
        name = payload["params"]["name"]
        result_payload = call_results.get(name, {})
        body = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": payload["id"],
                "result": {
                    "content": [{"type": "text", "text": json.dumps(result_payload)}],
                    "isError": False,
                },
            }
        )
        return _FakeResponse(body.encode("utf-8"))

    return opener


def test_the_toolkit_exposes_exactly_the_declared_tool_names():
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_manifest_opener(MANIFEST),
    )
    names = {tool.name for tool in toolkit.tools()}
    assert names == {"search_assets", "get_asset_context"}


def test_manifest_parity_a_tool_the_server_never_declared_is_never_exposed():
    """The inverse of hardcoding: a hand-maintained list could accidentally
    expose a tool the *server* dropped. This proves the toolkit only ever
    reflects what `tools/list` actually said, this run."""
    narrow_manifest = [MANIFEST[0]]  # only search_assets
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_manifest_opener(narrow_manifest),
    )
    names = {tool.name for tool in toolkit.tools()}
    assert names == {"search_assets"}
    assert "get_asset_context" not in names


def test_manifest_parity_a_new_server_side_tool_appears_without_a_release():
    """The other half of the same property: a tool the server *adds* shows
    up here immediately — nothing in this package names it."""
    wider_manifest = [
        *MANIFEST,
        {
            "name": "explain_lineage",
            "description": "Where an asset's data comes from.",
            "inputSchema": {
                "type": "object",
                "properties": {"fullyQualifiedName": {"type": "string"}},
                "required": ["fullyQualifiedName"],
            },
        },
    ]
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_manifest_opener(wider_manifest),
    )
    names = {tool.name for tool in toolkit.tools()}
    assert "explain_lineage" in names


def test_a_tools_description_and_schema_come_from_the_manifest_not_hand_written():
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_manifest_opener(MANIFEST),
    )
    by_name = {tool.name: tool for tool in toolkit.tools()}
    search = by_name["search_assets"]
    assert search.description == "Find assets by name or description."
    assert search.args_schema == MANIFEST[0]["inputSchema"]


def test_invoking_a_toolkit_tool_calls_the_matching_mcp_tool():
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_manifest_opener(MANIFEST, call_results={"search_assets": {"hits": ["ok"]}}),
    )
    by_name = {tool.name: tool for tool in toolkit.tools()}
    result = by_name["search_assets"].invoke({"query": "orders"})
    assert json.loads(result) == {"hits": ["ok"]}


def test_no_composite_tool_exists_the_exposed_set_never_exceeds_the_manifest():
    """Decision 5, structurally: nothing here can add a tool the server
    never declared, because every tool is built from `list_tools()` and
    nothing else contributes to the set."""
    toolkit = GraphOwlToolkit(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_manifest_opener(MANIFEST),
    )
    declared_names = {decl["name"] for decl in MANIFEST}
    exposed_names = {tool.name for tool in toolkit.tools()}
    assert exposed_names == declared_names, "the exposed set must equal, never exceed, the manifest"
