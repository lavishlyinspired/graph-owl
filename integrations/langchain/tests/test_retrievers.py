"""Slice B RED: GraphOwlRetriever — composes real MCP tool calls into
`Document`s, never flattened text (decision 3).

Finding 2 (`43-framework-integrations.md`): `AssetContext.related` is FQN
strings only, no relationship type — that lives on `explain_lineage`'s
`LineageStep` instead, so the retriever composes `search_assets` +
`get_asset_context` + `explain_lineage` per hit, not one call.
"""

import json

import pytest
from langchain_core.documents import Document

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.retrievers import GraphOwlRetriever

SECRET = "sk-super-secret-token-value"


def _tool_result(payload, is_error=False, request_id=1):
    envelope = {
        "jsonrpc": "2.0",
        "id": request_id,
        "result": {
            "content": [{"type": "text", "text": json.dumps(payload)}],
            "isError": is_error,
        },
    }
    return json.dumps(envelope).encode("utf-8")


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


def _method_of(request) -> str:
    return json.loads(request.data)["params"]["name"]


def _sequenced_opener(responses_by_tool):
    """Routes each request to a canned response keyed by tool name — the
    retriever calls several different tools per query, so a single fixed
    response (as Slice A's own tests use) cannot stand in for the server."""

    def opener(request):
        name = _method_of(request)
        body = responses_by_tool[name]
        return _FakeResponse(body if isinstance(body, bytes) else body.pop(0))

    return opener


def test_constructing_a_retriever_without_a_principal_raises():
    with pytest.raises(Exception):  # noqa: B017 — pydantic's own ValidationError
        GraphOwlRetriever(endpoint="https://graph-owl.internal")  # type: ignore[call-arg]


def test_an_empty_search_returns_an_empty_list_not_an_exception():
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener({"search_assets": _tool_result({"hits": [], "total": 0})}),
    )
    docs = retriever.invoke("orders")
    assert docs == []


def test_a_hit_becomes_a_document_with_rendered_page_content_and_metadata():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": "Daily order totals.",
                "related": [],
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result({"memories": []}),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )

    docs = retriever.invoke("orders")

    assert len(docs) == 1
    doc = docs[0]
    assert isinstance(doc, Document)
    assert "warehouse.retail.orders (table)" in doc.page_content
    assert "Daily order totals." in doc.page_content
    assert doc.metadata["fullyQualifiedName"] == "warehouse.retail.orders"
    assert doc.metadata["kind"] == "table"


def test_a_lineage_edge_is_rendered_with_its_relationship_type():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "related": [],
            }
        ),
        "explain_lineage": _tool_result(
            {
                "steps": [
                    {
                        "fromFqn": "warehouse.retail.orders",
                        "toFqn": "warehouse.retail.revenue",
                        "relationship": "feeds",
                        "source": "connector:snowflake",
                    }
                ]
            }
        ),
        "recall_memory": _tool_result({"memories": []}),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )

    docs = retriever.invoke("orders")
    fact = docs[0].metadata["facts"][0]
    assert fact["relationship"] == "feeds"
    assert fact["source"] == "connector:snowflake"
    assert fact["derived"] is False


def test_a_recalled_memorys_confidence_and_derived_flag_round_trip():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "related": [],
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result(
            {
                "memories": [
                    {
                        "kind": "note",
                        "content": "This table drops refunds silently.",
                        "confidence": 0.9,
                        "humanAuthored": False,
                    }
                ]
            }
        ),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )

    docs = retriever.invoke("orders")
    fact = docs[0].metadata["facts"][0]
    assert fact["text"] == "This table drops refunds silently."
    assert fact["confidence"] == 0.9
    # `human_authored: False` is what "machine-authored, treat as an
    # inference" actually means on the wire — see decision 4.
    assert fact["derived"] is True


def test_a_human_authored_memory_is_not_marked_derived():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "related": [],
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result(
            {
                "memories": [
                    {
                        "kind": "note",
                        "content": "Ask finance before changing this.",
                        "confidence": 0.95,
                        "humanAuthored": True,
                    }
                ]
            }
        ),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )

    docs = retriever.invoke("orders")
    fact = docs[0].metadata["facts"][0]
    assert fact["derived"] is False


def test_low_confidence_memories_are_excluded_by_default():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "related": [],
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result(
            {
                "memories": [
                    {
                        "kind": "note",
                        "content": "weak guess",
                        "confidence": 0.1,
                        "humanAuthored": False,
                    }
                ]
            }
        ),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )

    docs = retriever.invoke("orders")
    assert docs[0].metadata["facts"] == []


def test_derived_memories_can_be_included_explicitly():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "related": [],
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result(
            {
                "memories": [
                    {
                        "kind": "note",
                        "content": "machine guess",
                        "confidence": 0.9,
                        "humanAuthored": False,
                    }
                ]
            }
        ),
    }
    excluding = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        include_derived=False,
        opener=_sequenced_opener(responses),
    )
    docs = excluding.invoke("orders")
    assert docs[0].metadata["facts"] == []

    including = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        include_derived=True,
        opener=_sequenced_opener(responses),
    )
    docs = including.invoke("orders")
    assert len(docs[0].metadata["facts"]) == 1


def test_search_hits_are_capped_at_the_configured_limit():
    hits = [{"fullyQualifiedName": f"warehouse.t{i}", "kind": "table"} for i in range(3)]
    captured = {"search_args": None}

    def opener(request):
        payload = json.loads(request.data)
        name = payload["params"]["name"]
        if name == "search_assets":
            captured["search_args"] = payload["params"]["arguments"]
            return _FakeResponse(_tool_result({"hits": hits[:2]}))
        if name == "get_asset_context":
            fqn = payload["params"]["arguments"]["fullyQualifiedName"]
            context = {
                "fullyQualifiedName": fqn,
                "kind": "table",
                "description": None,
                "related": [],
            }
            return _FakeResponse(_tool_result(context))
        if name == "explain_lineage":
            return _FakeResponse(_tool_result({"steps": []}))
        return _FakeResponse(_tool_result({"memories": []}))

    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        limit=2,
        opener=opener,
    )
    docs = retriever.invoke("orders")
    assert captured["search_args"]["limit"] == 2
    assert len(docs) == 2


def test_the_search_query_is_forwarded_verbatim():
    captured = {}

    def opener(request):
        payload = json.loads(request.data)
        name = payload["params"]["name"]
        if name == "search_assets":
            captured["args"] = payload["params"]["arguments"]
            return _FakeResponse(_tool_result({"hits": []}))
        return _FakeResponse(_tool_result({}))

    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal", principal=Principal(token=SECRET), opener=opener
    )
    retriever.invoke("refund anomalies")
    assert captured["args"]["query"] == "refund anomalies"


def test_each_hit_fetches_its_own_context_not_a_shared_one():
    """`fqn` must flow from each hit into its own `get_asset_context` and
    `explain_lineage` calls — a constant or wrong fqn would still produce
    the right *count* of documents, which is why this checks identity, not
    just how many came back."""

    def opener(request):
        payload = json.loads(request.data)
        name = payload["params"]["name"]
        if name == "search_assets":
            hits = [
                {"fullyQualifiedName": "warehouse.t1", "kind": "table"},
                {"fullyQualifiedName": "warehouse.t2", "kind": "table"},
            ]
            return _FakeResponse(_tool_result({"hits": hits}))
        if name == "get_asset_context":
            fqn = payload["params"]["arguments"]["fullyQualifiedName"]
            context = {"fullyQualifiedName": fqn, "kind": "table", "description": None}
            return _FakeResponse(_tool_result(context))
        if name == "explain_lineage":
            fqn = payload["params"]["arguments"]["fullyQualifiedName"]
            return _FakeResponse(
                _tool_result(
                    {"steps": [{"fromFqn": fqn, "toFqn": "x", "relationship": fqn, "source": "s"}]}
                )
            )
        return _FakeResponse(_tool_result({"memories": []}))

    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal", principal=Principal(token=SECRET), opener=opener
    )
    docs = retriever.invoke("q")

    fqns = {doc.metadata["fullyQualifiedName"] for doc in docs}
    assert fqns == {"warehouse.t1", "warehouse.t2"}
    # The lineage relationship was echoed back as the seed fqn above, so a
    # mixed-up fqn would show up here as the *wrong* document's relationship.
    relationships = {doc.metadata["facts"][0]["relationship"] for doc in docs}
    assert relationships == {"warehouse.t1", "warehouse.t2"}


def test_a_memory_with_no_humanAuthored_field_is_not_marked_derived():
    """The safer default when authorship is unknown: do not falsely label
    something inferred, matching the same reasoning as lineage facts having
    no derived signal at all."""
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "related": [],
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result(
            {"memories": [{"kind": "note", "content": "no authorship field", "confidence": 0.9}]}
        ),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )
    docs = retriever.invoke("orders")
    assert docs[0].metadata["facts"][0]["derived"] is False


def test_a_missing_fullyQualifiedName_in_the_asset_context_falls_back_to_the_searched_fqn():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result({"kind": "table", "description": None}),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result({"memories": []}),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )
    docs = retriever.invoke("orders")
    assert docs[0].metadata["fullyQualifiedName"] == "warehouse.retail.orders"


def test_truncation_on_either_asset_context_or_lineage_marks_the_document_truncated():
    responses = {
        "search_assets": _tool_result(
            {"hits": [{"fullyQualifiedName": "warehouse.retail.orders", "kind": "table"}]}
        ),
        "get_asset_context": _tool_result(
            {
                "fullyQualifiedName": "warehouse.retail.orders",
                "kind": "table",
                "description": None,
                "truncated": True,
            }
        ),
        "explain_lineage": _tool_result({"steps": []}),
        "recall_memory": _tool_result({"memories": []}),
    }
    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token=SECRET),
        opener=_sequenced_opener(responses),
    )
    docs = retriever.invoke("orders")
    assert docs[0].metadata["truncated"] is True
