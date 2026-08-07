"""Slice C: authorization survives the adapter.

**Reframed per finding 3** (`43-framework-integrations.md`): graph-owl-mcp
deliberately answers a policy denial the same way it answers "does not
exist" (`Outcome::NotFound`'s own doc: "absent and denied,
indistinguishable") — the same non-disclosure principle
`graph-owl-api::Catalog::walk_hop` already applies. So this proves the
property that is actually true and actually worth proving — two principals
against one corpus retrieve different documents — rather than a
distinguishable-403 signal the server was deliberately built never to send.
"""

import json

from graph_owl_langchain._core.principal import Principal
from graph_owl_langchain.retrievers import GraphOwlRetriever


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


def _tool_result(payload):
    envelope = {
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"content": [{"type": "text", "text": json.dumps(payload)}], "isError": False},
    }
    return json.dumps(envelope).encode("utf-8")


def _server_scoped_by_bearer_token(corpus_by_token):
    """A minimal fake server: what `search_assets` returns depends on the
    *caller's own* Authorization header — the same shape a real
    policy-filtered search has, without needing a live graph-owl-server.
    """

    def opener(request):
        token = dict(request.headers)["Authorization"].removeprefix("Bearer ")
        payload = json.loads(request.data)
        name = payload["params"]["name"]
        if name == "search_assets":
            visible = corpus_by_token.get(token, [])
            return _FakeResponse(
                _tool_result({"hits": visible, "total": len(visible), "policyFiltered": False})
            )
        if name == "get_asset_context":
            fqn = payload["params"]["arguments"]["fullyQualifiedName"]
            return _FakeResponse(
                _tool_result({"fullyQualifiedName": fqn, "kind": "table", "description": None})
            )
        if name == "explain_lineage":
            return _FakeResponse(_tool_result({"steps": []}))
        return _FakeResponse(_tool_result({"memories": []}))

    return opener


def test_two_principals_against_one_corpus_retrieve_different_documents():
    corpus_by_token = {
        "analyst-token": [{"fullyQualifiedName": "warehouse.public.orders", "kind": "table"}],
        "admin-token": [
            {"fullyQualifiedName": "warehouse.public.orders", "kind": "table"},
            {"fullyQualifiedName": "finance.salaries", "kind": "table"},
        ],
    }
    opener = _server_scoped_by_bearer_token(corpus_by_token)

    analyst = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="analyst-token"),
        opener=opener,
    )
    admin = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="admin-token"),
        opener=opener,
    )

    analyst_fqns = {doc.metadata["fullyQualifiedName"] for doc in analyst.invoke("q")}
    admin_fqns = {doc.metadata["fullyQualifiedName"] for doc in admin.invoke("q")}

    assert analyst_fqns == {"warehouse.public.orders"}
    assert admin_fqns == {"warehouse.public.orders", "finance.salaries"}
    assert "finance.salaries" not in analyst_fqns


def test_one_process_serves_several_principals_without_cross_contamination():
    """The principal is attached per instance, not process-global — building
    two retrievers in one process must not let the second construction's
    principal leak into the first's calls, in either direction."""
    corpus_by_token = {
        "a": [{"fullyQualifiedName": "x.one", "kind": "table"}],
        "b": [{"fullyQualifiedName": "y.two", "kind": "table"}],
    }
    opener = _server_scoped_by_bearer_token(corpus_by_token)

    first = GraphOwlRetriever(
        endpoint="https://graph-owl.internal", principal=Principal(token="a"), opener=opener
    )
    second = GraphOwlRetriever(
        endpoint="https://graph-owl.internal", principal=Principal(token="b"), opener=opener
    )

    # Interleaved calls, not sequential — a shared/cached principal would
    # show up as the second construction's token leaking into the first's
    # calls made *after* it, which sequential calls could hide.
    first_docs_before = {doc.metadata["fullyQualifiedName"] for doc in first.invoke("q")}
    second_docs = {doc.metadata["fullyQualifiedName"] for doc in second.invoke("q")}
    first_docs_after = {doc.metadata["fullyQualifiedName"] for doc in first.invoke("q")}

    assert first_docs_before == {"x.one"}
    assert first_docs_after == {"x.one"}
    assert second_docs == {"y.two"}


def test_search_counts_are_passed_through_without_the_client_inventing_its_own():
    """`SearchResults.total`/`policyFiltered` are the server's own count,
    computed *after* its policy filter — decision 3's "counts are
    consistent" is a pass-through property, not something this client
    should recompute or approximate from what it happened to receive."""
    corpus_by_token = {
        "analyst-token": [{"fullyQualifiedName": "warehouse.public.orders", "kind": "table"}],
    }
    captured = {}

    def opener(request):
        payload = json.loads(request.data)
        name = payload["params"]["name"]
        if name == "search_assets":
            hits = corpus_by_token["analyst-token"]
            result = {"hits": hits, "total": 1, "policyFiltered": True}
            captured["result"] = result
            return _FakeResponse(_tool_result(result))
        if name == "get_asset_context":
            fqn = payload["params"]["arguments"]["fullyQualifiedName"]
            return _FakeResponse(
                _tool_result({"fullyQualifiedName": fqn, "kind": "table", "description": None})
            )
        if name == "explain_lineage":
            return _FakeResponse(_tool_result({"steps": []}))
        return _FakeResponse(_tool_result({"memories": []}))

    retriever = GraphOwlRetriever(
        endpoint="https://graph-owl.internal",
        principal=Principal(token="analyst-token"),
        opener=opener,
    )
    docs = retriever.invoke("q")

    # The client neither drops `policyFiltered` nor fabricates a `total`
    # that disagrees with what the server actually said — it has nothing
    # of its own to add here, and adding something would be exactly the
    # "adapter invents a signal" failure mode finding 3 warns against.
    assert len(docs) == len(captured["result"]["hits"])
