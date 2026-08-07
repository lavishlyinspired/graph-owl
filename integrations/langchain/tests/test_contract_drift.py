"""Slice F: a contract change that breaks the adapter fails the build.

Needs a live `graph-owl-server` — set `GRAPH_OWL_TEST_ENDPOINT` (CI does;
see `.github/workflows/langchain-integration.yml`). Skipped locally when
unset, the same convention `graph-owl-sdk`'s own integration tests use, so
`pytest` without a live service still runs everything else.
"""

import os

import pytest

from graph_owl_langchain._core.client import GraphOwlClient
from graph_owl_langchain._core.contract import REQUIRED_TOOLS
from graph_owl_langchain._core.principal import Principal

ENDPOINT = os.environ.get("GRAPH_OWL_TEST_ENDPOINT")

pytestmark = pytest.mark.skipif(
    not ENDPOINT, reason="set GRAPH_OWL_TEST_ENDPOINT to run against a live graph-owl-server"
)


def _client() -> GraphOwlClient:
    token = os.environ.get("GRAPH_OWL_TEST_TOKEN", "test-token")
    assert ENDPOINT is not None
    return GraphOwlClient(endpoint=ENDPOINT, principal=Principal(token=token))


def test_every_required_tool_is_still_declared_by_the_live_manifest():
    """The contract-drift check itself: if the server ever stops declaring
    one of `REQUIRED_TOOLS`, this fails here — in CI, on the change that
    caused it — rather than as a runtime `GraphOwlToolError` an agent hits
    in production with no obvious cause."""
    declared = {tool["name"] for tool in _client().list_tools()}
    missing = set(REQUIRED_TOOLS) - declared
    assert not missing, f"the live server no longer declares: {missing}"


def test_a_declared_tools_input_schema_still_has_the_fields_this_package_reads():
    """Narrower than manifest parity: proves the *shape* this package
    depends on, not only the tool's existence. A rename of
    `fullyQualifiedName` on the server side would not be caught by
    presence alone."""
    by_name = {tool["name"]: tool for tool in _client().list_tools()}
    schema = by_name["search_assets"]["inputSchema"]
    assert "query" in schema["properties"]


def test_the_live_server_actually_answers_a_search_call():
    """The end-to-end proof this whole slice exists for: not a mock, a
    real `/mcp` round trip through this package's own client."""
    result = _client().call_tool("search_assets", {"query": "nonexistent-anywhere"})
    assert "hits" in result
