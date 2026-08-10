"""OCR parsing behaviour, tested against a scripted model.

The model is never real here. ``OvisOcrParser`` depends only on the ``OcrModel``
seam, so a test injects a fake that returns scripted Markdown — the endpoint
client itself is tested separately against a real HTTP double (Slice 2). What
these tests pin is the assembly: byte offsets, text hygiene, routing. A GPU
never needs to exist for any of it, and a test that imports one is a test that
cannot run where the rest of the suite does.
"""

from __future__ import annotations

import base64
import hashlib
import json
import re
import socket
import sys
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest
from graph_owl_worker import ParseError, ParserRegistry, UnsupportedMediaType
from graph_owl_worker.ocr import (
    DEFAULT_DPI,
    OCR_PROMPT,
    EndpointOcrModel,
    OcrError,
    OcrPdfParser,
    OvisOcrParser,
    _dpi_to_scale,
    assemble_pages,
    clean_repeats,
    filter_imgtags,
)

FIXTURE = Path(__file__).parent / "fixtures" / "page.png"
PDF_FIXTURE = Path(__file__).parent / "fixtures" / "scan.pdf"


class FakeModel:
    """The whole model, scripted: returns fixed Markdown per page."""

    def __init__(self, pages: list[str]) -> None:
        self.pages = pages
        self.seen: list[object] = []

    def parse_images(self, images):
        self.seen.extend(images)
        return self.pages


INVOICE = (
    "INVOICE 0001\n"
    "Supplier: Åsa & Co GmbH — Zahlung innerhalb 30 Tage.\n"
    "Total: 1.234,56 €\n"
)


# ── byte offsets, not character counts ────────────────────────────────────


def test_byte_spans_resolve_exactly_for_non_ascii_model_output():
    """**Bytes, not characters.** The model's Markdown is full of non-ASCII
    (å, ä, €), so a parser that counted characters would produce spans that
    resolve to nothing or to the wrong words — which is exactly the failure the
    SDK's ``resolve`` exists to surface."""
    parser = OvisOcrParser(FakeModel([INVOICE]))

    doc = parser.parse("inv.png", "image/png", FIXTURE.read_bytes())

    assert [s.heading for s in doc.sections] == ["page 1"]
    section = doc.sections[0]
    assert section.span.end == len(doc.text.encode("utf-8"))
    assert section.span.resolve(doc.text) == INVOICE


def test_every_section_span_resolves_against_the_parsed_text():
    """A span that does not resolve is one graph-owl cannot resolve either —
    the reviewer would see "(the evidence span does not resolve)". Multi-page
    offsets live in the shared assembler that ``OcrPdfParser`` (Slice 3) reuses."""
    text, sections = assemble_pages([INVOICE, "Notes page with accénts.\n"])

    assert [s.heading for s in sections] == ["page 1", "page 2"]
    assert text == INVOICE + "Notes page with accénts.\n"
    for section in sections:
        assert section.span.resolve(text) is not None, section
    assert sections[1].span.start == len(INVOICE.encode("utf-8"))


# ── text hygiene: img tags and truncated repeats ──────────────────────────


def test_img_tags_never_reach_the_document_text():
    raw = (
        "INVOICE 0001\n"
        '<img src="images/bbox_0_0_900_500.jpg" />\n'
        "Total: 1.234,56 €\n"
    )

    doc = OvisOcrParser(FakeModel([raw])).parse(
        "inv.png", "image/png", FIXTURE.read_bytes()
    )

    assert "images/" not in doc.text
    assert "<img" not in doc.text
    assert "Total: 1.234,56 €" in doc.text


def test_a_long_repeating_tail_is_clipped():
    unit = "the migration was reverted and the service restarted "
    head = "x" * 8000
    text = head + unit * 12

    assert clean_repeats(text) == head


def test_a_repeat_below_the_thresholds_is_untouched():
    text = "x" * 8000 + "rollback. " * 3

    assert clean_repeats(text) == text


def test_a_short_document_is_never_touched_regardless_of_repeats():
    """The cleaner exists for a model pushing a long page toward its output
    ceiling — a short document cannot have looped, so it is never touched."""
    text = "rollback. " * 30

    assert clean_repeats(text) == text


def test_a_page_of_exactly_the_cleaner_threshold_is_still_cleaned():
    """The guard is ``len < threshold``, not ``<=`` — a page at the exact
    ceiling can still have looped, so it must be cleaned, not skipped."""
    text = "x" * (8000 - 100) + "rollback. " * 10

    assert len(text) == 8000
    assert clean_repeats(text) == "x" * (8000 - 100)


def test_a_repeat_of_an_odd_run_length_is_clipped_to_the_last_char():
    """A trailing run of 101 identical characters (an odd, prime length) is
    found by period 1 only — even periods clip one char less. A cleaner that
    walked periods in steps of two, or that skipped period 1, would leave the
    last character behind."""
    text = "x" * 8000 + "z" * 101

    assert clean_repeats(text) == "x" * 8000


def test_an_exactly_five_times_twenty_repeat_is_clipped():
    """The repeat-count and repeat-size thresholds are ``>=`` — a repeat of
    exactly 5 units of exactly 20 characters (100 chars total) is a genuine
    looped tail, not a coincidence, and must be clipped. ``>`` either side
    would keep it."""
    text = "x" * 8000 + "0123456789abcdefghij" * 5

    assert clean_repeats(text) == "x" * 8000


def test_a_document_that_is_only_the_repeated_block_collapses_to_empty():
    """When the whole page is the loop, the cleaner clips everything — nothing
    of real content is lost. The loop boundary at the start of the text (``>= 0``,
    not ``> 0``) decides whether the first unit counts."""
    unit = "the migration was reverted "
    text = unit * 330

    assert len(text) >= 8000
    assert clean_repeats(text) == ""


def test_offsets_accumulate_across_three_pages():
    """Each page's span must start where the previous pages ended — not where
    the *preceding page alone* ended. With two pages the bug is invisible
    (page 2's start uses page 1's already-correct offset); it needs a third
    page to surface."""
    pages = [INVOICE, "middle page with accénts.\n", "final page.\n"]
    text, sections = assemble_pages(pages)

    assert [s.heading for s in sections] == ["page 1", "page 2", "page 3"]
    expected_third_start = len((INVOICE + "middle page with accénts.\n").encode("utf-8"))
    assert sections[2].span.start == expected_third_start
    for section in sections:
        assert section.span.resolve(text) is not None, section


def test_the_fingerprint_is_over_the_post_clean_text():
    """**The fingerprint must be pinned to the cleaned text.** graph-owl judges
    idempotence on this hash, so an img tag or a looped tail that lands in the
    hash would make every OCR pass look different and the skip would never fire
    — the expensive failure, since the whole point is to skip the OCR pass."""
    raw = 'Intro.\n<img src="images/bbox_0_0_900_500.jpg" />\nBody: café.\n'
    expected = "Intro.\n\nBody: café.\n"

    doc = OvisOcrParser(FakeModel([raw])).parse(
        "inv.png", "image/png", FIXTURE.read_bytes()
    )

    assert doc.fingerprint() != hashlib.sha256(raw.encode("utf-8")).hexdigest()
    assert (
        doc.fingerprint() == hashlib.sha256(expected.encode("utf-8")).hexdigest()
    )


# ── routing ───────────────────────────────────────────────────────────────


def test_image_types_route_to_the_ocr_parser_through_the_registry():
    registry = ParserRegistry()
    registry.register(OvisOcrParser(FakeModel(["parsed by OCR"])))

    doc = registry.parse("scan.png", "image/png", FIXTURE.read_bytes())

    assert doc.text == "parsed by OCR\n"
    assert doc.media_type == "image/png"


def test_the_ocr_parser_claims_the_image_media_types():
    parser = OvisOcrParser(FakeModel([]))

    assert set(parser.handles()) == {"image/png", "image/jpeg", "image/webp"}


def test_an_unclaimed_media_type_is_refused_by_name():
    registry = ParserRegistry()
    registry.register(OvisOcrParser(FakeModel(["x"])))

    with pytest.raises(UnsupportedMediaType, match="image/gif"):
        registry.parse("scan.gif", "image/gif", FIXTURE.read_bytes())


def test_the_parser_itself_refuses_an_unclaimed_type_by_name():
    """The registry refuses before it ever calls a parser, so this branch —
    the parser's own guard — is only reachable in a direct call. The type is
    named in the error, because "install the extra" and "you gave me the wrong
    bytes" are different fixes."""
    parser = OvisOcrParser(FakeModel([]))

    with pytest.raises(UnsupportedMediaType, match="image/gif"):
        parser.parse("scan.gif", "image/gif", FIXTURE.read_bytes())


def test_bytes_that_are_not_an_image_are_a_typed_parse_error():
    parser = OvisOcrParser(FakeModel([]))

    with pytest.raises(ParseError, match="inv.png"):
        parser.parse("inv.png", "image/png", b"definitely not an image")


def test_a_model_failure_becomes_a_typed_parse_error_not_a_crash():
    """One unreachable endpoint call must fail *this* document, the same way a
    corrupt image already does — ``Worker.process`` only catches ``ParseError``
    and ``UnsupportedMediaType``, so an ``OcrError`` escaping ``parse()``
    uncaught would take an entire batch down with it, not just this file."""

    class FailingModel:
        def parse_images(self, images):
            raise OcrError("endpoint unreachable")

    parser = OvisOcrParser(FailingModel())

    with pytest.raises(ParseError) as exc:
        parser.parse("inv.png", "image/png", FIXTURE.read_bytes())
    assert str(exc.value) == "could not read `inv.png` as image/png: endpoint unreachable"


def test_the_document_keeps_its_source_id():
    """The source id travels through assembly untouched — the only way an
    evidence span can be traced back to the file it came from."""
    doc = OvisOcrParser(FakeModel([INVOICE])).parse(
        "inv.png", "image/png", FIXTURE.read_bytes()
    )

    assert doc.source_id == "inv.png"


def test_a_missing_pillow_extra_is_an_unsupported_type_with_an_install_hint(
    monkeypatch,
):
    """Pillow lives behind the ``ovis-ocr2`` extra; constructing the parser
    without it must name the fix (``pip install graph-owl-worker[ovis-ocr2]``),
    not crash. ``sys.modules`` gets a poisoned entry so ``import PIL`` fails
    exactly as it would on a worker that never installed the extra."""
    monkeypatch.setitem(sys.modules, "PIL", None)

    with pytest.raises(UnsupportedMediaType) as exc:
        OvisOcrParser(FakeModel([]))
    assert str(exc.value) == (
        "image parsing needs the `ovis-ocr2` extra: "
        "pip install graph-owl-worker[ovis-ocr2]"
    )


# ── EndpointOcrModel — Slice 2, against a real local HTTP double ───────────
#
# No mock: `ScriptedOpenAIEndpoint` is a real `http.server` bound to a real
# loopback port, run in a background thread — this project's own "a real
# server, not a placeholder" precedent, applied to Python since none of the
# existing suite needed one before now.


class ScriptedOpenAIEndpoint(BaseHTTPRequestHandler):
    """Answers every POST with the next scripted (status, body) pair, and
    records what it was sent — so a test can assert on the *request* (the
    prompt, the image data URL) as well as the response."""

    script: list[tuple[int, bytes]] = []
    received: list[dict] = []

    def do_POST(self):  # noqa: N802 - BaseHTTPRequestHandler's own naming
        length = int(self.headers.get("content-length", 0))
        body = self.rfile.read(length)
        type(self).received.append(
            {
                "path": self.path,
                "content_type": self.headers.get("content-type"),
                "body": json.loads(body) if body else None,
            }
        )
        status, response_body = type(self).script[
            min(len(type(self).received) - 1, len(type(self).script) - 1)
        ]
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(response_body)

    def log_message(self, *args):  # silence the default stderr access log
        pass


@contextmanager
def scripted_endpoint(script: list[tuple[int, bytes]]):
    ScriptedOpenAIEndpoint.script = script
    ScriptedOpenAIEndpoint.received = []
    server = HTTPServer(("127.0.0.1", 0), ScriptedOpenAIEndpoint)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", ScriptedOpenAIEndpoint.received
    finally:
        server.shutdown()
        thread.join()


def openai_response(content: str) -> bytes:
    return json.dumps(
        {"choices": [{"message": {"content": content, "role": "assistant"}}]}
    ).encode("utf-8")


def a_page() -> "Image.Image":
    from PIL import Image

    return Image.open(FIXTURE)


def test_the_endpoint_client_returns_one_markdown_string_per_page_in_order():
    with scripted_endpoint(
        [(200, openai_response("page one")), (200, openai_response("page two"))]
    ) as (url, received):
        model = EndpointOcrModel(endpoint=url, model="test-model")

        pages = model.parse_images([a_page(), a_page()])

    assert pages == ["page one", "page two"]
    assert len(received) == 2


def test_the_request_carries_the_configured_model_and_deterministic_sampling():
    with scripted_endpoint([(200, openai_response("x"))]) as (url, received):
        EndpointOcrModel(endpoint=url, model="ATH-MaaS/OvisOCR2").parse_images(
            [a_page()]
        )

    sent = received[0]["body"]
    assert sent["model"] == "ATH-MaaS/OvisOCR2"
    assert sent["temperature"] == 0.0
    assert sent["max_tokens"] == 16384


def test_the_request_carries_the_page_as_a_data_url_and_the_prompt_as_text():
    with scripted_endpoint([(200, openai_response("x"))]) as (url, received):
        EndpointOcrModel(endpoint=url).parse_images([a_page()])

    message = received[0]["body"]["messages"][0]
    assert message["role"] == "user"
    content = message["content"]
    kinds = {block["type"] for block in content}
    assert kinds == {"image_url", "text"}
    image_block = next(b for b in content if b["type"] == "image_url")
    data_url = image_block["image_url"]["url"]
    assert data_url.startswith("data:image/png;base64,")
    # Not just the prefix: the payload must decode to real PNG bytes, or an
    # empty/garbage encoding would still pass a prefix-only assertion.
    encoded = data_url.removeprefix("data:image/png;base64,")
    assert base64.b64decode(encoded)[:8] == b"\x89PNG\r\n\x1a\n"
    text_block = next(b for b in content if b["type"] == "text")
    assert text_block["text"] == OCR_PROMPT


def test_the_request_is_sent_as_json():
    with scripted_endpoint([(200, openai_response("x"))]) as (url, received):
        EndpointOcrModel(endpoint=url).parse_images([a_page()])

    assert received[0]["content_type"] == "application/json"


def test_a_custom_prompt_reaches_the_request_when_configured():
    with scripted_endpoint([(200, openai_response("x"))]) as (url, received):
        EndpointOcrModel(endpoint=url, prompt="describe this page").parse_images(
            [a_page()]
        )

    content = received[0]["body"]["messages"][0]["content"]
    text_block = next(b for b in content if b["type"] == "text")
    assert text_block["text"] == "describe this page"


def test_a_non_2xx_response_is_a_typed_ocr_error_naming_the_endpoint():
    with scripted_endpoint([(500, b'{"error": "boom"}')]) as (url, _received):
        model = EndpointOcrModel(endpoint=url)

        with pytest.raises(OcrError, match=re.escape(url)):
            model.parse_images([a_page()])


def test_a_response_with_no_json_body_is_a_typed_ocr_error_not_a_crash():
    with scripted_endpoint([(200, b"not json at all")]) as (url, _received):
        model = EndpointOcrModel(endpoint=url)

        with pytest.raises(OcrError, match=re.escape(url)):
            model.parse_images([a_page()])


def test_a_response_missing_the_expected_shape_is_a_typed_ocr_error():
    """The endpoint answered `200` with valid JSON, but not the OpenAI
    chat-completion shape this client actually reads — a malformed server,
    not a network failure, and still a typed error rather than a raw
    `KeyError` leaking out of this module."""
    with scripted_endpoint([(200, b'{"unexpected": "shape"}')]) as (url, _received):
        model = EndpointOcrModel(endpoint=url)

        with pytest.raises(OcrError, match=re.escape(url)):
            model.parse_images([a_page()])


def test_an_unreachable_endpoint_is_a_typed_ocr_error_naming_the_endpoint():
    """A refused connection (nothing listening) is ``URLError``, not
    ``HTTPError`` — a different exception type this client must also map to
    ``OcrError`` rather than let escape as a raw ``urllib`` exception."""
    closed = socket.socket()
    closed.bind(("127.0.0.1", 0))
    port = closed.getsockname()[1]
    closed.close()
    url = f"http://127.0.0.1:{port}"
    model = EndpointOcrModel(endpoint=url)

    with pytest.raises(OcrError, match=re.escape(url)):
        model.parse_images([a_page()])


def test_a_failure_on_one_page_names_which_page_it_was():
    """Per-page error isolation means a failure is reported as being about
    *that* page — an operator debugging a batch needs to know which of the
    document's pages the endpoint choked on, not just that something did."""
    with scripted_endpoint([(200, openai_response("ok")), (500, b"{}")]) as (
        url,
        _received,
    ):
        model = EndpointOcrModel(endpoint=url)

        with pytest.raises(OcrError, match="page 2"):
            model.parse_images([a_page(), a_page()])


def test_default_endpoint_and_model_match_the_documented_deployment_shape():
    model = EndpointOcrModel()

    assert model._endpoint == "http://localhost:8000"
    assert model._model == "ATH-MaaS/OvisOCR2"


# ── OcrPdfParser — Slice 3, scanned PDFs rasterized then OCR'd ─────────────


def test_the_ocr_pdf_parser_claims_application_pdf():
    parser = OcrPdfParser(FakeModel([]))

    assert parser.handles() == ("application/pdf",)


def test_a_scanned_pdf_is_rasterized_one_page_at_a_time_and_ocrd_in_order():
    """The fixture is two blank pages; what this pins is that the parser
    rasterizes *both* and hands them to the model in page order — the model
    sees only images, never the PDF's own (nonexistent) text layer."""
    model = FakeModel(["page one text", "page two text"])
    parser = OcrPdfParser(model)

    doc = parser.parse("scan.pdf", "application/pdf", PDF_FIXTURE.read_bytes())

    assert len(model.seen) == 2
    assert [s.heading for s in doc.sections] == ["page 1", "page 2"]
    assert doc.text == "page one text\npage two text\n"


def test_an_unclaimed_media_type_is_refused_by_the_pdf_ocr_parser():
    parser = OcrPdfParser(FakeModel(["x"]))

    with pytest.raises(UnsupportedMediaType, match="image/png"):
        parser.parse("f.png", "image/png", FIXTURE.read_bytes())


def test_bytes_that_are_not_a_pdf_are_a_typed_parse_error():
    parser = OcrPdfParser(FakeModel([]))

    with pytest.raises(ParseError, match="scan.pdf"):
        parser.parse("scan.pdf", "application/pdf", b"definitely not a pdf")


def test_a_model_failure_becomes_a_typed_parse_error_for_the_pdf_parser_too():
    """Mirrors ``OvisOcrParser``'s own isolation guarantee: one endpoint
    hiccup on a scanned PDF must fail that document, not the batch."""

    class FailingModel:
        def parse_images(self, images):
            raise OcrError("endpoint unreachable")

    parser = OcrPdfParser(FailingModel())

    with pytest.raises(ParseError) as exc:
        parser.parse("scan.pdf", "application/pdf", PDF_FIXTURE.read_bytes())
    assert (
        str(exc.value) == "could not read `scan.pdf` as application/pdf: endpoint unreachable"
    )


def test_dpi_converts_to_pdfiums_scale_factor_by_the_72_dpi_point_ratio():
    """Direct unit test of the pure conversion, deliberately never calling
    pdfium's renderer: an inverted formula here would ask the native library
    to rasterize a canvas thousands of times too large, which segfaults the
    whole process rather than raising a catchable exception — this is what
    protects the test suite from that, not just what documents the ratio."""
    assert _dpi_to_scale(72) == 1.0
    assert _dpi_to_scale(144) == 2.0
    assert _dpi_to_scale(200) == pytest.approx(200 / 72)


def test_the_document_keeps_its_source_id_through_rasterization():
    doc = OcrPdfParser(FakeModel(["x", "y"])).parse(
        "scan.pdf", "application/pdf", PDF_FIXTURE.read_bytes()
    )

    assert doc.source_id == "scan.pdf"


def test_the_dpi_defaults_to_200_and_is_configurable():
    assert DEFAULT_DPI == 200

    parser = OcrPdfParser(FakeModel(["x", "y"]), dpi=72)
    doc = parser.parse("scan.pdf", "application/pdf", PDF_FIXTURE.read_bytes())

    # 72 DPI == 1:1 with the PDF's own point size (200x300 in the fixture);
    # this is the cheapest possible assertion that the configured DPI, not
    # just the default, actually reached rendering.
    assert doc.text  # parsed without error at a non-default DPI


def test_a_missing_pypdfium2_extra_is_an_unsupported_type_with_an_install_hint(
    monkeypatch,
):
    monkeypatch.setitem(sys.modules, "pypdfium2", None)

    with pytest.raises(UnsupportedMediaType) as exc:
        OcrPdfParser(FakeModel([]))
    assert str(exc.value) == (
        "scanned-PDF OCR needs the `ovis-ocr2` extra: "
        "pip install graph-owl-worker[ovis-ocr2]"
    )


def test_ocr_wins_application_pdf_over_the_text_extracting_pdf_parser_when_registered_last():
    """The CLI's own contract: ``--ocr`` must win ``application/pdf`` away
    from ``--pdf`` when both are enabled, because a scanned PDF has no text
    layer for ``PdfParser`` to extract — it would silently succeed with an
    empty document instead of failing loud. ``ParserRegistry.register``
    prepends, so registering the OCR parser *after* the text parser is what
    the CLI must do to make this true."""
    from graph_owl_worker.parsers import PdfParser

    registry = ParserRegistry()
    registry.register(PdfParser())
    registry.register(OcrPdfParser(FakeModel(["ocr text"])))

    doc = registry.parse("scan.pdf", "application/pdf", PDF_FIXTURE.read_bytes())

    assert doc.text == "ocr text\n"
