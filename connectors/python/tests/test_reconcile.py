"""Triggering reconciliation — Epic 105 P5b.

**This file used to test a rule-evaluation engine — similarity, span
arithmetic, row-to-finding construction.** That engine is deleted from
Python; it is native now, in `graph-owl-resolution::rule_match` and
`Catalog::reconcile_pack`, tested there with the same fixture-derived
numbers this file used to assert (the 180-day boundary, the transposition
threshold). What remains here is `run_findings`'s only remaining job: turn
a pack id into one `POST /packs/{pack}/reconcile` call and read back the
response. Nothing left to test but the HTTP shape.
"""

from __future__ import annotations

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from graph_owl_packs.loader import LoadError  # noqa: E402
from graph_owl_packs.reconcile import run_findings  # noqa: E402


class _Server:
    """A graph-owl double, answering whatever `/packs/{pack}/reconcile` is
    asked to."""

    def __init__(self, status: int = 200, body: dict | None = None) -> None:
        self.status = status
        self.body = body if body is not None else {"pack": "gst", "evaluated": 6, "found": 2, "opened": 2, "alreadyOpen": 0}
        self.requests: list[dict] = []
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("content-length", 0))
                raw = self.rfile.read(length) if length else b""
                outer.requests.append(
                    {"path": self.path, "auth": self.headers.get("authorization"), "raw": raw}
                )
                encoded = json.dumps(outer.body).encode()
                self.send_response(outer.status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

            def log_message(self, *args: object) -> None:
                pass

        self._http = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self._http.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self._http.server_port}"

    def close(self) -> None:
        self._http.shutdown()


def test_run_findings_posts_to_the_packs_reconcile_route() -> None:
    server = _Server()
    try:
        run_findings("gst", server.url)
    finally:
        server.close()

    assert len(server.requests) == 1
    assert server.requests[0]["path"] == "/packs/gst/reconcile"


def test_run_findings_sends_no_body() -> None:
    # The route takes no payload — the rules were registered separately, by
    # the loader, at install time. A body here would be dead weight.
    server = _Server()
    try:
        run_findings("gst", server.url)
    finally:
        server.close()

    assert server.requests[0]["raw"] == b""


def test_a_token_reaches_the_request() -> None:
    server = _Server()
    try:
        run_findings("gst", server.url, token="secret")
    finally:
        server.close()

    assert server.requests[0]["auth"] == "Bearer secret"


def test_the_response_is_read_back_into_a_reconcile_result() -> None:
    server = _Server(body={"pack": "gst", "evaluated": 6, "found": 2, "opened": 1, "alreadyOpen": 1})
    try:
        result = run_findings("gst", server.url)
    finally:
        server.close()

    assert result.pack_id == "gst"
    assert result.evaluated == 6
    assert result.found == 2
    assert result.opened == 1
    assert result.already_open == 1


def test_a_server_refusal_is_a_load_error_naming_the_status() -> None:
    server = _Server(status=404, body={"detail": "not found"})
    try:
        with pytest.raises(LoadError, match="404"):
            run_findings("nonexistent-pack", server.url)
    finally:
        server.close()


def test_an_unreachable_server_names_the_server() -> None:
    with pytest.raises(LoadError, match="unreachable"):
        run_findings("gst", "http://127.0.0.1:1")
