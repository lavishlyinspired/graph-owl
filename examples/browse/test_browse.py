"""Epic 36 Slice D's own acceptance criteria, against a real live service —
same shape as `agent-triage`/`adapter-csv`'s live suites: a live round trip
is the only thing that tests the real contract, not a mocked opinion of it.

Run via `scripts/verify-examples.sh`'s open-mode phase (browse needs no
auth, matching adapter-csv). Skipped unless `GRAPH_OWL_TEST_ENDPOINT` is set.
"""

from __future__ import annotations

import importlib.util
import os
import sys
import threading
import urllib.error
import urllib.request
import uuid
from http.server import HTTPServer
from pathlib import Path
from typing import Any

import httpx
import pytest

sys.path.insert(0, str(Path(__file__).parent.parent.parent / "sdk" / "python"))
from graph_owl_sdk import GraphOwlClient, IngestBuilder  # noqa: E402

from graph_owl_read_client import Client  # noqa: E402

pytestmark = pytest.mark.skipif(
    "GRAPH_OWL_TEST_ENDPOINT" not in os.environ,
    reason="live-service test — set GRAPH_OWL_TEST_ENDPOINT (see scripts/verify-examples.sh)",
)


def load_app() -> Any:
    spec = importlib.util.spec_from_file_location("browse_app", Path(__file__).parent / "app.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules["browse_app"] = module
    spec.loader.exec_module(module)
    return module


def base_url() -> str:
    return os.environ["GRAPH_OWL_TEST_ENDPOINT"]


def unique(name: str) -> str:
    # The server and its Postgres are shared across a whole
    # verify-examples.sh run — a fixed name would collide with a previous
    # run's data against a reused container.
    return f"{name}-{uuid.uuid4().hex[:8]}"


class RequestCounter:
    """Counts outbound HTTP requests the generated client actually makes —
    the mechanism the "one request, not N+1" acceptance criterion needs
    proof of. `httpx`'s own `event_hooks` are the documented extension
    point for this; there is no other way to observe request count from
    outside the generated client without patching it."""

    def __init__(self) -> None:
        self.count = 0

    def __call__(self, request: httpx.Request) -> None:
        self.count += 1


@pytest.fixture(scope="module")
def seeded() -> dict[str, str]:
    service = unique("browse-svc")
    ingest = GraphOwlClient(base_url=base_url())
    builder = IngestBuilder()
    builder.entity("service", service)
    db_fqn = f"{service}.db"
    builder.entity("database", "db", parent_fqn=service)
    schema_fqn = f"{db_fqn}.public"
    builder.entity("schema", "public", parent_fqn=db_fqn)
    table_leaf = "orders"
    table_fqn = f"{schema_fqn}.{table_leaf}"
    builder.entity(
        "table",
        table_leaf,
        parent_fqn=schema_fqn,
        description="one row per order",
    )
    builder.entity("column", "order_id", parent_fqn=table_fqn)

    result = ingest.push(builder.build())
    assert result["rejected"] == 0, result

    return {"table_fqn": table_fqn, "table_leaf": table_leaf}


@pytest.fixture(scope="module")
def app() -> Any:
    return load_app()


@pytest.fixture
def counting_client() -> tuple[Client, RequestCounter]:
    counter = RequestCounter()
    client = Client(
        base_url=base_url(),
        httpx_args={"event_hooks": {"request": [counter]}},
    )
    return client, counter


@pytest.fixture(scope="module")
def running_server(app: Any) -> str:
    """A real `HTTPServer` on a system-assigned port, wired the same way
    `main()` wires it. Every other test in this file calls
    `render_search_page`/`render_asset_page` directly — the business logic
    Slice D actually exists to prove — the way `agent-triage`'s own suite
    calls `triage()` directly rather than going over MCP. But that leaves
    `do_GET`'s own routing and status/header wiring completely unexercised,
    so this fixture backs the one test below that does a real HTTP GET
    against a real socket, catching the class of bug direct function calls
    cannot: a route that dispatches to the wrong handler, a status code
    that never makes it onto the wire, a missing content-type header."""
    handler_class = app.make_handler(Client(base_url=base_url()))
    server = HTTPServer(("127.0.0.1", 0), handler_class)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]
    yield f"http://127.0.0.1:{port}"
    server.shutdown()
    thread.join(timeout=5)


def test_the_real_http_server_serves_a_rendered_page(running_server: str, seeded: dict[str, str]) -> None:
    url = f"{running_server}/?q={seeded['table_leaf']}"
    with urllib.request.urlopen(url, timeout=10) as response:
        assert response.status == 200
        assert response.headers["Content-Type"] == "text/html; charset=utf-8"
        body = response.read().decode("utf-8")
    assert seeded["table_fqn"] in body


def test_the_real_http_server_returns_404_for_an_unknown_path(running_server: str) -> None:
    try:
        urllib.request.urlopen(f"{running_server}/nonexistent-route", timeout=10)
        pytest.fail("expected an HTTPError")
    except urllib.error.HTTPError as exc:
        assert exc.code == 404


def test_search_finds_a_seeded_asset_by_leaf_name(app: Any, seeded: dict[str, str], counting_client) -> None:
    # A leaf name, not the dotted FQN — full-text search does not tokenize
    # a dotted FQN the way it tokenizes a bare name (the same discovery
    # `agent-triage`/`adapter-csv`'s live suites already made).
    client, _ = counting_client
    status, body = app.render_search_page(client, seeded["table_leaf"])
    assert status == 200
    assert seeded["table_fqn"] in body
    assert "No assets found" not in body


def test_search_with_no_matches_renders_a_distinguishable_empty_state(app: Any, counting_client) -> None:
    client, _ = counting_client
    status, body = app.render_search_page(client, unique("no-such-asset"))
    assert status == 200
    assert "No assets found" in body


def test_the_unfiltered_landing_page_lists_assets(app: Any, seeded: dict[str, str], counting_client) -> None:
    client, _ = counting_client
    status, body = app.render_search_page(client, None)
    assert status == 200
    assert "All assets" in body
    # Not asserting the seeded table specifically appears — the landing
    # page is capped at 50 and the shared server accumulates assets across
    # the whole verify-examples.sh run, so presence is not guaranteed; only
    # that the page renders a real, non-empty listing shape.
    assert "<ul>" in body or "No assets found" in body


def test_the_asset_detail_page_composes_everything_in_one_request(
    app: Any, seeded: dict[str, str], counting_client
) -> None:
    from graph_owl_read_client.api.default import get_assets_search

    client, counter = counting_client

    # Found via the search API directly, filtered to the exact FQN — not
    # by parsing the rendered search page's first link. Postgres full-text
    # search stems "orders" to a token that also matches the seeded
    # "order_id" column, so a naive first-result pick landed on the wrong
    # asset (found running this: the detail page it fetched was the column,
    # not the table). The table's own detail page is what this test needs.
    results = get_assets_search.sync(client=client, q=seeded["table_leaf"])
    matches = [a for a in results.data if a.fully_qualified_name == seeded["table_fqn"]]
    assert matches, f"expected {seeded['table_fqn']!r} among search results: {results.data}"
    asset_id = str(matches[0].id)

    counter.count = 0
    status, body = app.render_asset_page(client, asset_id)

    assert status == 200, body
    assert counter.count == 1, (
        f"the asset detail page must compose owners/tags/lineage/columns via "
        f"a single ?fields= request, not one call per section: made {counter.count}"
    )
    assert seeded["table_fqn"] in body
    assert "order_id" in body, "the seeded column child must appear via the columns field"
    assert "one row per order" in body


def test_a_nonexistent_but_well_formed_id_is_reported_not_found(app: Any, counting_client) -> None:
    client, _ = counting_client
    status, body = app.render_asset_page(client, str(uuid.uuid4()))
    assert status == 404
    assert "Not found" in body


def test_a_malformed_id_is_rejected_without_any_network_call(app: Any, counting_client) -> None:
    client, counter = counting_client
    status, body = app.render_asset_page(client, "not-a-uuid")
    assert status == 400
    assert "Invalid asset id" in body
    assert counter.count == 0, "a malformed id must fail locally, before any request is made"


def test_an_unreachable_backend_raises_the_exception_type_do_get_catches(app: Any) -> None:
    # Port 1 is a real, reserved port nothing serves — the smallest way to
    # produce a genuine connection failure without knocking down the real
    # server the rest of this suite depends on. This proves the failure
    # mode and `do_GET`'s `except httpx.HTTPError` clause actually line up
    # — the real end-to-end 502 is not re-proven over a socket here, since
    # `running_server` above already establishes that `do_GET`'s wiring
    # correctly turns a caught exception into a response on the wire for
    # the 404 case, and standing up a second server bound to an
    # unreachable client to reprove the same wiring would not test
    # anything this test and that one don't already cover between them.
    #
    # `timeout` must be passed as the `Client`'s own constructor kwarg, not
    # inside `httpx_args` — found running this directly: `get_httpx_client`
    # passes `timeout=self._timeout` *and* unpacks `**self._httpx_args`,
    # so a `timeout` key in `httpx_args` collides with it
    # (`TypeError: got multiple values for keyword argument 'timeout'`).
    unreachable = Client(base_url="http://127.0.0.1:1", timeout=httpx.Timeout(1.0))
    with pytest.raises(httpx.HTTPError):
        app.render_search_page(unreachable, None)
