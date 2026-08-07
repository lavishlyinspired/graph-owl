"""Slice C RED: token refresh — "an expired token triggers one refresh and
does not loop." A refresh callback runs at most once per call; a second
401 after a fresh token is a real failure, not something to retry forever.
"""

import io
import json
import urllib.error

import pytest

from graph_owl_langchain._core.client import GraphOwlClient, GraphOwlToolError
from graph_owl_langchain._core.principal import Principal


def _unauthorized_error(request, detail="the bearer token has expired"):
    # graph-owl-server's real 401 body: RFC 9457 problem+json
    # (`AppError::into_response`), reached before the JSON-RPC handler ever
    # runs — never a JSON-RPC envelope.
    body = json.dumps(
        {
            "type": "https://graph-owl.dev/problems/token-expired",
            "title": "Token Expired",
            "status": 401,
            "detail": detail,
        }
    ).encode("utf-8")
    return urllib.error.HTTPError(request.full_url, 401, "Unauthorized", {}, io.BytesIO(body))


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


def _ok_result(payload):
    envelope = {
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"content": [{"type": "text", "text": json.dumps(payload)}], "isError": False},
    }
    return json.dumps(envelope).encode("utf-8")


def test_a_401_with_no_refresh_callback_raises_without_retrying():
    calls = {"count": 0}

    def opener(request):
        calls["count"] += 1
        raise _unauthorized_error(request)

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="stale"),
        opener=opener,
    )
    with pytest.raises(GraphOwlToolError):
        client.call_tool("search_assets", {"query": "orders"})
    assert calls["count"] == 1, "no refresh callback means no retry"


def test_a_401_with_a_refresh_callback_retries_once_and_succeeds():
    calls = {"opener": 0, "refresh": 0}
    sent_tokens = []

    def refresh():
        calls["refresh"] += 1
        return "fresh-token"

    def opener(request):
        calls["opener"] += 1
        sent_tokens.append(dict(request.headers)["Authorization"])
        if calls["opener"] == 1:
            raise _unauthorized_error(request)
        return _FakeResponse(_ok_result({"hits": []}))

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="stale", refresh=refresh),
        opener=opener,
    )
    result = client.call_tool("search_assets", {"query": "orders"})

    assert result == {"hits": []}
    assert calls["refresh"] == 1
    assert calls["opener"] == 2
    assert sent_tokens == ["Bearer stale", "Bearer fresh-token"]


def test_a_second_401_after_refreshing_raises_rather_than_looping():
    calls = {"opener": 0, "refresh": 0}

    def refresh():
        calls["refresh"] += 1
        return "still-bad-token"

    def opener(request):
        calls["opener"] += 1
        raise _unauthorized_error(request)

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="stale", refresh=refresh),
        opener=opener,
    )
    with pytest.raises(GraphOwlToolError):
        client.call_tool("search_assets", {"query": "orders"})

    assert calls["refresh"] == 1, "refresh must not be called more than once per request"
    assert calls["opener"] == 2, "one original attempt plus exactly one retry"


def test_the_refreshed_token_is_used_by_the_next_call_too():
    """The client remembers the refreshed token rather than re-expiring on
    every subsequent call — refreshing is a client-state update, not a
    one-shot patch applied only to the request that triggered it."""
    calls = {"opener": 0, "refresh": 0}
    sent_tokens = []

    def refresh():
        calls["refresh"] += 1
        return "fresh-token"

    def opener(request):
        calls["opener"] += 1
        sent_tokens.append(dict(request.headers)["Authorization"])
        if calls["opener"] == 1:
            raise _unauthorized_error(request)
        return _FakeResponse(_ok_result({"hits": []}))

    client = GraphOwlClient(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="stale", refresh=refresh),
        opener=opener,
    )
    client.call_tool("search_assets", {"query": "first"})
    client.call_tool("search_assets", {"query": "second"})

    assert calls["refresh"] == 1, "the second call must reuse the already-refreshed token"
    assert sent_tokens == ["Bearer stale", "Bearer fresh-token", "Bearer fresh-token"]
