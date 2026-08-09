"""OCR parsing behaviour, tested against a scripted model.

The model is never real here. ``OvisOcrParser`` depends only on the ``OcrModel``
seam, so a test injects a fake that returns scripted Markdown — the endpoint
client itself is tested separately against a real HTTP double (Slice 2). What
these tests pin is the assembly: byte offsets, text hygiene, routing. A GPU
never needs to exist for any of it, and a test that imports one is a test that
cannot run where the rest of the suite does.
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import pytest
from graph_owl_worker import ParseError, ParserRegistry, UnsupportedMediaType
from graph_owl_worker.ocr import OvisOcrParser, assemble_pages, clean_repeats, filter_imgtags

FIXTURE = Path(__file__).parent / "fixtures" / "page.png"


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
