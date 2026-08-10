"""The ``--ocr`` CLI surface, tested against real local HTTP doubles.

``main()`` builds a real ``GraphOwlClient`` with no way to inject a fake, so
proving the flags actually connect anything means standing up two real
``http.server`` instances — one playing graph-owl, one playing the OCR
endpoint — the same "a real server, not a mock" discipline ``test_ocr.py``
already uses for ``EndpointOcrModel``. Two servers run concurrently here, so
the handler is built per-server (closed over its own routes/received list)
rather than carrying shared class state, which a single scripted double never
needed to worry about.
"""

from __future__ import annotations

import base64
import json
import struct
import sys
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

from graph_owl_worker.cli import EXIT_OK, EXIT_UNUSABLE, build_parser, main
from graph_owl_worker.ocr import DEFAULT_DPI, DEFAULT_ENDPOINT, DEFAULT_MODEL

FIXTURE = Path(__file__).parent / "fixtures" / "page.png"
PDF_FIXTURE = Path(__file__).parent / "fixtures" / "scan.pdf"


def _png_dimensions(data: bytes) -> tuple[int, int]:
    # 8-byte PNG signature, 4-byte chunk length, 4-byte "IHDR" type, then
    # width and height as two big-endian uint32s — a fixed, format-guaranteed
    # offset, so this needs no image library to check what DPI produced.
    return struct.unpack(">II", data[16:24])


def _make_handler(routes: dict[tuple[str, str], tuple[int, dict]], received: list[dict]):
    class ScriptedHandler(BaseHTTPRequestHandler):
        def _handle(self, method: str) -> None:
            length = int(self.headers.get("content-length", 0))
            body = self.rfile.read(length) if length else b""
            path = self.path.split("?", 1)[0]
            received.append(
                {"method": method, "path": self.path, "body": json.loads(body) if body else None}
            )
            status, response_body = routes.get(
                (method, path), (404, {"error": f"no route for {method} {path}"})
            )
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(response_body).encode("utf-8"))

        def do_GET(self) -> None:  # noqa: N802
            self._handle("GET")

        def do_POST(self) -> None:  # noqa: N802
            self._handle("POST")

        def log_message(self, *args):
            pass

    return ScriptedHandler


@contextmanager
def scripted_server(routes: dict[tuple[str, str], tuple[int, dict]]):
    received: list[dict] = []
    server = HTTPServer(("127.0.0.1", 0), _make_handler(routes, received))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", received
    finally:
        server.shutdown()
        thread.join()


# ── flags ────────────────────────────────────────────────────────────────


def test_the_ocr_flags_have_sensible_defaults():
    args = build_parser().parse_args(["somepath"])

    assert args.ocr is False
    assert args.ocr_endpoint == DEFAULT_ENDPOINT
    assert args.model == DEFAULT_MODEL
    assert args.prompt_file is None
    assert args.ocr_dpi == DEFAULT_DPI


# ── failing fast, before any network call ──────────────────────────────────


def test_ocr_without_the_extra_installed_is_unusable_not_a_crash(monkeypatch):
    """Mirrors ``--pdf``'s own missing-extra discipline: the fix is named on
    stderr and the process exits ``EXIT_UNUSABLE`` — it never gets far enough
    to open a socket, so no server needs to exist for this to be true."""
    monkeypatch.setitem(sys.modules, "PIL", None)

    exit_code = main(["--ocr", "somepath"])

    assert exit_code == EXIT_UNUSABLE


def test_a_missing_prompt_file_is_unusable_not_a_crash(tmp_path):
    """A typo'd ``--prompt-file`` path is an operator mistake, not a reason
    for a raw traceback — same "diagnostics on stderr, exit code says what
    happened" contract the rest of this CLI already keeps."""
    missing = tmp_path / "does-not-exist.txt"

    exit_code = main(["--ocr", "--prompt-file", str(missing), "somepath"])

    assert exit_code == EXIT_UNUSABLE


# ── end to end, against two real local doubles ──────────────────────────────


def test_ocr_end_to_end_reaches_the_configured_endpoint_and_submits_the_run(tmp_path):
    """The only way to prove ``--ocr``/``--ocr-endpoint`` actually connect
    anything: route a real image through a real worker run and watch both
    doubles receive the traffic the wiring is supposed to produce."""
    image = tmp_path / "scan.png"
    image.write_bytes(FIXTURE.read_bytes())

    graph_owl_routes = {
        ("GET", "/assets"): (200, {"data": [{"fullyQualifiedName": "prod.orders"}]}),
        ("POST", "/extraction/runs"): (
            200,
            {"outcome": "recorded", "runId": "r1", "asserted": 0, "surfaced": 0},
        ),
    }
    ocr_routes = {
        ("POST", "/v1/chat/completions"): (
            200,
            {"choices": [{"message": {"content": "prod.orders is append-only."}}]},
        ),
    }

    with scripted_server(graph_owl_routes) as (graph_owl_url, graph_owl_received):
        with scripted_server(ocr_routes) as (ocr_url, ocr_received):
            exit_code = main(
                [
                    "--server",
                    graph_owl_url,
                    "--ocr",
                    "--ocr-endpoint",
                    ocr_url,
                    str(image),
                ]
            )

    assert exit_code == EXIT_OK
    assert len(ocr_received) == 1

    extraction_calls = [r for r in graph_owl_received if r["path"] == "/extraction/runs"]
    assert len(extraction_calls) == 1
    assert extraction_calls[0]["body"]["document"]["mediaType"] == "image/png"


def test_a_custom_prompt_file_reaches_the_endpoint_request(tmp_path):
    image = tmp_path / "scan.png"
    image.write_bytes(FIXTURE.read_bytes())
    prompt_file = tmp_path / "prompt.txt"
    prompt_file.write_text("describe this page only", encoding="utf-8")

    graph_owl_routes = {
        ("GET", "/assets"): (200, {"data": []}),
        ("POST", "/extraction/runs"): (
            200,
            {"outcome": "recorded", "runId": "r1", "asserted": 0, "surfaced": 0},
        ),
    }
    ocr_routes = {
        ("POST", "/v1/chat/completions"): (200, {"choices": [{"message": {"content": "x"}}]}),
    }

    with scripted_server(graph_owl_routes) as (graph_owl_url, _graph_owl_received):
        with scripted_server(ocr_routes) as (ocr_url, ocr_received):
            exit_code = main(
                [
                    "--server",
                    graph_owl_url,
                    "--ocr",
                    "--ocr-endpoint",
                    ocr_url,
                    "--prompt-file",
                    str(prompt_file),
                    str(image),
                ]
            )

    assert exit_code == EXIT_OK
    content = ocr_received[0]["body"]["messages"][0]["content"]
    text_block = next(b for b in content if b["type"] == "text")
    assert text_block["text"] == "describe this page only"


def test_a_scanned_pdf_is_routed_through_ocr_end_to_end(tmp_path):
    """A scanned PDF (the fixture's two blank pages have no text layer) must
    reach the OCR endpoint through --ocr, not fall through to the built-in
    text parser or a no-parser-configured failure."""
    pdf = tmp_path / "scan.pdf"
    pdf.write_bytes(PDF_FIXTURE.read_bytes())

    graph_owl_routes = {
        ("GET", "/assets"): (200, {"data": []}),
        ("POST", "/extraction/runs"): (
            200,
            {"outcome": "recorded", "runId": "r1", "asserted": 0, "surfaced": 0},
        ),
    }
    ocr_routes = {
        ("POST", "/v1/chat/completions"): (200, {"choices": [{"message": {"content": "x"}}]}),
    }

    with scripted_server(graph_owl_routes) as (graph_owl_url, graph_owl_received):
        with scripted_server(ocr_routes) as (ocr_url, ocr_received):
            exit_code = main(
                [
                    "--server",
                    graph_owl_url,
                    "--ocr",
                    "--ocr-endpoint",
                    ocr_url,
                    str(pdf),
                ]
            )

    assert exit_code == EXIT_OK
    assert len(ocr_received) == 2  # two pages in the fixture PDF
    extraction_calls = [r for r in graph_owl_received if r["path"] == "/extraction/runs"]
    assert extraction_calls[0]["body"]["document"]["mediaType"] == "application/pdf"


def test_ocr_wins_application_pdf_over_pdf_when_both_flags_are_set(tmp_path):
    """The CLI's registration order must make ``--ocr`` win ``application/pdf``
    away from ``--pdf`` — a scanned PDF has no text for ``PdfParser`` to find,
    and silently extracting nothing is worse than OCR-ing it."""
    pdf = tmp_path / "scan.pdf"
    pdf.write_bytes(PDF_FIXTURE.read_bytes())

    graph_owl_routes = {
        ("GET", "/assets"): (200, {"data": []}),
        ("POST", "/extraction/runs"): (
            200,
            {"outcome": "recorded", "runId": "r1", "asserted": 0, "surfaced": 0},
        ),
    }
    ocr_routes = {
        ("POST", "/v1/chat/completions"): (200, {"choices": [{"message": {"content": "x"}}]}),
    }

    with scripted_server(graph_owl_routes) as (graph_owl_url, _graph_owl_received):
        with scripted_server(ocr_routes) as (ocr_url, ocr_received):
            exit_code = main(
                [
                    "--server",
                    graph_owl_url,
                    "--pdf",
                    "--ocr",
                    "--ocr-endpoint",
                    ocr_url,
                    str(pdf),
                ]
            )

    assert exit_code == EXIT_OK
    # Only OcrPdfParser talks to the OCR endpoint; PdfParser never does. Any
    # request at all here proves OCR — not text extraction — won the route.
    assert len(ocr_received) == 2


def test_the_ocr_dpi_flag_reaches_the_rasterizer(tmp_path):
    pdf = tmp_path / "scan.pdf"
    pdf.write_bytes(PDF_FIXTURE.read_bytes())

    graph_owl_routes = {
        ("GET", "/assets"): (200, {"data": []}),
        ("POST", "/extraction/runs"): (
            200,
            {"outcome": "recorded", "runId": "r1", "asserted": 0, "surfaced": 0},
        ),
    }
    ocr_routes = {
        ("POST", "/v1/chat/completions"): (200, {"choices": [{"message": {"content": "x"}}]}),
    }

    with scripted_server(graph_owl_routes) as (graph_owl_url, _graph_owl_received):
        with scripted_server(ocr_routes) as (ocr_url, ocr_received):
            exit_code = main(
                [
                    "--server",
                    graph_owl_url,
                    "--ocr",
                    "--ocr-endpoint",
                    ocr_url,
                    "--ocr-dpi",
                    "72",
                    str(pdf),
                ]
            )

    assert exit_code == EXIT_OK
    content = ocr_received[0]["body"]["messages"][0]["content"]
    image_block = next(b for b in content if b["type"] == "image_url")
    encoded = image_block["image_url"]["url"].removeprefix("data:image/png;base64,")
    # The fixture's pages are 200x300 PDF points; at 72 DPI (1:1 with a PDF
    # point) the rendered raster is exactly that size in pixels.
    assert _png_dimensions(base64.b64decode(encoded)) == (200, 300)
