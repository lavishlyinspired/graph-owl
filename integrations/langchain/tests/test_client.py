"""Slice A RED: the core MCP client.

Two properties matter more than the transport mechanics: a client cannot be
constructed without a principal (decision 2 — no ambient, no service-account
fallback), and nothing it does can put the credential where a human reading
logs or a bug report would see it.
"""

import json
import logging

import pytest

from graph_owl_langchain._core.client import (
    GraphOwlClient,
    GraphOwlConnectionError,
    GraphOwlToolError,
)
from graph_owl_langchain._core.principal import Principal

SECRET = "sk-super-secret-token-value"


class _FakeResponse:
    def __init__(self, body: bytes, status: int = 200):
        self.status = status
        self._body = body

    def read(self):
        return self._body

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


def _tool_result(payload, is_error=False, request_id=1):
    """A JSON-RPC response wrapping one MCP tool result — the real shape:
    the tool's own payload is JSON-encoded a *second* time, inside
    ``result.content[0].text``, with ``result.isError`` carrying whether
    the tool refused rather than a top-level JSON-RPC ``error`` member
    (``graph_owl_mcp::jsonrpc``'s own doc: a tool that ran and answered
    "no such asset" succeeded at the protocol level)."""
    envelope = {
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "content": [{"type": "text", "text": json.dumps(payload)}],
            "isError": is_error,
        },
    }
    return json.dumps(envelope).encode("utf-8")


def _capturing_opener(captured, body=None):
    if body is None:
        body = _tool_result({"hits": []})

    def opener(request):
        captured.setdefault("requests", []).append(request)
        return _FakeResponse(body)

    return opener


def test_constructing_a_client_without_a_principal_raises():
    with pytest.raises(TypeError):
        GraphOwlClient(endpoint="https://graph-owl.internal")  # type: ignore[call-arg]


def test_a_client_never_reveals_its_token_in_repr():
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal", principal=Principal(token=SECRET)
    )
    assert SECRET not in repr(client)


def test_a_connection_failure_is_a_typed_error_naming_the_endpoint():
    def broken_opener(request):
        raise OSError("connection refused")

    client = GraphOwlClient(
        endpoint="https://unreachable.example",
        principal=Principal(token=SECRET),
        opener=broken_opener,
    )

    with pytest.raises(GraphOwlConnectionError) as excinfo:
        client.call_tool("search_assets", {"query": "orders"})

    assert "https://unreachable.example" in str(excinfo.value)
    assert "connection refused" in str(excinfo.value)


def test_a_connection_failure_never_leaks_the_token_in_the_exception_text():
    def broken_opener(request):
        raise OSError("connection refused")

    client = GraphOwlClient(
        endpoint="https://unreachable.example",
        principal=Principal(token=SECRET),
        opener=broken_opener,
    )

    with pytest.raises(GraphOwlConnectionError) as excinfo:
        client.call_tool("search_assets", {"query": "orders"})

    assert SECRET not in str(excinfo.value)


def test_a_connection_failure_never_leaks_the_token_into_a_log_record(caplog):
    def broken_opener(request):
        raise OSError("connection refused")

    client = GraphOwlClient(
        endpoint="https://unreachable.example",
        principal=Principal(token=SECRET),
        opener=broken_opener,
    )

    with caplog.at_level(logging.DEBUG):
        with pytest.raises(GraphOwlConnectionError):
            client.call_tool("search_assets", {"query": "orders"})

    for record in caplog.records:
        assert SECRET not in record.getMessage()


def test_a_successful_call_sends_the_token_as_a_bearer_header():
    captured = {}
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_capturing_opener(captured),
    )
    client.call_tool("search_assets", {"query": "orders"})

    sent = captured["requests"][0]
    assert dict(sent.headers)["Authorization"] == f"Bearer {SECRET}"
    assert sent.full_url == "https://graph-owl.internal/mcp"


def test_a_successful_call_returns_the_tools_unwrapped_payload():
    body = _tool_result({"hits": [{"fullyQualifiedName": "warehouse.t"}], "total": 1})
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=lambda request: _FakeResponse(body),
    )

    result = client.call_tool("search_assets", {"query": "orders"})
    assert result == {"hits": [{"fullyQualifiedName": "warehouse.t"}], "total": 1}


def test_a_trailing_slash_on_the_endpoint_does_not_double_up_the_path():
    captured = {}
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal/",
        principal=Principal(token=SECRET),
        opener=_capturing_opener(captured),
    )
    client.call_tool("search_assets", {"query": "orders"})

    assert captured["requests"][0].full_url == "https://graph-owl.internal/mcp"


def test_the_request_is_a_well_formed_jsonrpc_tools_call():
    captured = {}
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_capturing_opener(captured),
    )
    client.call_tool("search_assets", {"query": "orders"})

    sent = captured["requests"][0]
    body = json.loads(sent.data)
    assert body["jsonrpc"] == "2.0"
    assert body["method"] == "tools/call"
    assert body["params"] == {"name": "search_assets", "arguments": {"query": "orders"}}
    assert isinstance(body["id"], int)
    assert sent.get_method() == "POST"
    assert dict(sent.headers)["Content-type"] == "application/json"


def test_the_first_call_on_a_fresh_client_uses_request_id_one():
    captured = {}
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_capturing_opener(captured),
    )
    client.call_tool("search_assets", {"query": "orders"})

    assert json.loads(captured["requests"][0].data)["id"] == 1


def test_a_non_2xx_http_response_is_read_as_an_rfc9457_problem():
    """A non-2xx response from `/mcp` never reaches `jsonrpc::handle` at
    all — it comes from axum's own auth middleware or a request rejection,
    both of which answer with `AppError`'s RFC 9457 problem+json
    (`{"type", "title", "status", "detail"}`), never a JSON-RPC envelope.
    Assuming the JSON-RPC shape here was this client's first, wrong guess
    (see `_as_tool_error`'s own doc) — this test pins the corrected one."""
    import urllib.error

    error_body = (
        b'{"type": "https://graph-owl.dev/problems/malformed-body", '
        b'"title": "Malformed Body", "status": 400, '
        b'"detail": "the request body was not valid JSON"}'
    )

    def raising_opener(request):
        raise urllib.error.HTTPError(
            request.full_url, 400, "Bad Request", {}, __import__("io").BytesIO(error_body)
        )

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=raising_opener,
    )

    with pytest.raises(GraphOwlToolError) as excinfo:
        client.call_tool("search_assets", {"query": "orders"})
    assert "the request body was not valid JSON" in str(excinfo.value)


def test_a_tool_that_refused_raises_naming_the_tool_and_the_reason():
    """The primary error path: the call succeeded at the protocol level and
    the tool itself said no — `isError: true`, not a top-level JSON-RPC
    `error`. This is how `NotFound`/`Unauthenticated`/`Refused`/etc. all
    actually surface (`graph_owl_mcp::jsonrpc`)."""
    body = _tool_result({"error": "no such entity, or it is not visible to you"}, is_error=True)
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=lambda request: _FakeResponse(body),
    )

    with pytest.raises(GraphOwlToolError) as excinfo:
        client.call_tool("get_asset_context", {"fullyQualifiedName": "warehouse.t"})
    assert excinfo.value.tool == "get_asset_context"
    assert "no such entity" in str(excinfo.value)


def test_a_refused_tool_call_with_no_error_field_still_names_the_tool():
    body = _tool_result({}, is_error=True)
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=lambda request: _FakeResponse(body),
    )

    with pytest.raises(GraphOwlToolError) as excinfo:
        client.call_tool("search_assets", {"query": "orders"})
    # Exact match, not `in`: a substring check is satisfied by mutmut's own
    # "wrap the literal" string mutation ("XXunknown errorXX" still contains
    # "unknown error"), so it never proves the *right* text was used.
    assert str(excinfo.value) == "tool 'search_assets' failed: unknown error"


def test_isError_false_does_not_raise_even_with_an_error_shaped_key_present():
    """`isError` is the signal, not the mere presence of an `"error"` key in
    the tool's own payload — a search result could legitimately have a
    field named that for other reasons, and this client must not guess."""
    body = _tool_result({"error": "this is data, not a failure"}, is_error=False)
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=lambda request: _FakeResponse(body),
    )

    result = client.call_tool("search_assets", {"query": "orders"})
    assert result == {"error": "this is data, not a failure"}


def test_the_request_id_increments_across_calls_on_one_client():
    captured = {}
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_capturing_opener(captured),
    )
    client.call_tool("search_assets", {"query": "orders"})
    client.call_tool("search_assets", {"query": "customers"})

    ids = [json.loads(r.data)["id"] for r in captured["requests"]]
    assert ids[1] == ids[0] + 1, f"ids did not increment: {ids}"


def test_a_protocol_level_jsonrpc_error_raises_a_typed_tool_error():
    error_body = (
        b'{"jsonrpc": "2.0", "id": 1, '
        b'"error": {"code": -32602, "message": "invalid params"}}'
    )
    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=lambda request: _FakeResponse(error_body),
    )

    with pytest.raises(GraphOwlToolError) as excinfo:
        client.call_tool("get_asset_context", {"fullyQualifiedName": "warehouse.t"})

    assert "get_asset_context" in str(excinfo.value)
    assert "invalid params" in str(excinfo.value)


def test_a_connection_error_carries_the_endpoint_as_an_attribute():
    def broken_opener(request):
        raise OSError("connection refused")

    client = GraphOwlClient(
        endpoint="https://unreachable.example",
        principal=Principal(token=SECRET),
        opener=broken_opener,
    )

    with pytest.raises(GraphOwlConnectionError) as excinfo:
        client.call_tool("search_assets", {"query": "orders"})

    assert excinfo.value.endpoint == "https://unreachable.example"


def test_the_core_module_never_imports_a_framework():
    """decision 8: the core has no reason to import LangChain or LangGraph.

    Static inspection, not a `sys.modules` snapshot: once *any* other test
    file in the same process imports `langchain_core` (the retriever tests
    do), a runtime check would see it in `sys.modules` regardless of test
    order and report a false failure — the exact trap this test almost
    shipped with. Reading `_core`'s own source for the literal import
    statement is order-independent and is what actually answers "does this
    package need the framework to be installed."
    """
    import ast
    import pathlib

    core_dir = pathlib.Path(__file__).parent.parent / "graph_owl_langchain" / "_core"
    for path in core_dir.glob("*.py"):
        tree = ast.parse(path.read_text(), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                names = [alias.name for alias in node.names]
            elif isinstance(node, ast.ImportFrom):
                names = [node.module or ""]
            else:
                continue
            for name in names:
                assert not name.startswith(("langchain", "langgraph")), (
                    f"{path.name} imports {name} — the core must not depend on a framework"
                )
