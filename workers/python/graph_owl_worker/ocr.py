"""OCR parsing — images and scanned PDFs through a vision model.

**The model is never here.** ``OcrModel`` is the seam a worker hands the parser:
a deployed worker passes an object that talks to a served endpoint over
OpenAI-compatible HTTP. The parser never loads weights, never spawns a process,
and the seam is what lets every test run with a scripted fake — no GPU, no
model, no network in the suite.

Mirrors ``PdfParser``'s decisions. Pages become sections with byte offsets (the
coordinate a human checking a claim against the original will actually use),
and hygiene is applied to model output **before** the fingerprint is pinned, so
a figure placeholder or a looped tail can never make the same document hash
differently — the expensive failure, since the whole point of the fingerprint
is to skip the OCR pass.
"""

from __future__ import annotations

import re
from typing import TYPE_CHECKING, Protocol

from graph_owl_sdk.extraction import ParsedDocument, Section, TextSpan

from .parsers import ParseError, UnsupportedMediaType

if TYPE_CHECKING:
    from PIL import Image

#: The model's figure-placeholder convention: a bbox-sized tag in place of a
#: figure. Figures carry no extractable claims, so the tags are removed rather
#: than kept as noise in evidence text and in the fingerprint. The bbox scale
#: ``[0, 1000)`` is the model's documented convention, consumed (never produced)
#: by this parser.
_IMG_TAG = re.compile(r'<img\s+src="images/bbox_\d+_\d+_\d+_\d+\.jpg"\s*/>')

# Repeat-cleaner thresholds, from the model's documented post-processing for a
# truncated generation. Each has a stated reason: ``min_text_len`` — only a
# page near the output ceiling can have looped, so a short page is never
# touched; ``max_period`` / ``min_period`` — the repeated unit's length bounds;
# ``min_repeat_times`` / ``min_repeat_chars`` — a repeat must be big enough to
# be a failure, not a coincidence. Every one is configurable by an operator who
# finds a page that trips it wrongly.
MIN_TEXT_LEN = 8000
MAX_PERIOD = 200
MIN_PERIOD = 1
MIN_REPEAT_TIMES = 5
MIN_REPEAT_CHARS = 100


class OcrModel(Protocol):
    """The seam: page images in, one Markdown string per page, in order.

    One method, no exceptions in the protocol: how a model is reached (a served
    endpoint today, an in-process load tomorrow) is an implementation detail of
    the object behind this, and a parser must not know which it is.
    """

    def parse_images(self, images: list[Image.Image]) -> list[str]:
        ...


def filter_imgtags(markdown: str) -> str:
    """Drop figure-placeholder tags from model output."""
    return _IMG_TAG.sub("", markdown)


def clean_repeats(text: str) -> str:
    """Clip a truncated generation that looped at the tail.

    A vision model pushing a long page toward its output ceiling can repeat the
    final block instead of stopping. The repeat is the absence of real content,
    so it is trimmed — but only when it is big enough to be a failure. A short
    page cannot have looped and is returned untouched.
    """
    if len(text) < MIN_TEXT_LEN:
        return text

    best_clip = 0
    for period in range(MAX_PERIOD, MIN_PERIOD - 1, -1):
        unit = text[-period:]
        count = 0
        index = len(text)
        while index - period >= 0 and text[index - period : index] == unit:
            count += 1
            index -= period
        if count >= MIN_REPEAT_TIMES and count * period >= MIN_REPEAT_CHARS:
            best_clip = max(best_clip, count * period)
    return text[: len(text) - best_clip] if best_clip else text


def assemble_pages(pages: list[str]) -> tuple[str, list[Section]]:
    """Model output per page → one text and its page sections, byte-exact.

    Hygiene is applied per page before assembly, so the fingerprint pins the
    cleaned text. A page is its own section (``page N``), the same coordinate
    ``PdfParser`` uses — a model's inferred headings are exactly the unreliable
    layer this pipeline exists to not depend on, so they are not section
    boundaries.
    """
    text_parts: list[str] = []
    sections: list[Section] = []
    offset = 0
    for number, page in enumerate(pages, start=1):
        body = clean_repeats(filter_imgtags(page))
        if not body.endswith("\n"):
            body += "\n"
        size = len(body.encode("utf-8"))
        sections.append(Section(f"page {number}", TextSpan(offset, offset + size)))
        text_parts.append(body)
        offset += size
    return "".join(text_parts), sections


class OvisOcrParser:
    """Page images parsed by a vision model, behind the ``ovis-ocr2`` extra.

    The import is deferred to construction so that importing this module never
    requires ``PIL`` — a worker deployed to handle only markdown must not fail
    to start because of a format it will never see. (Same decision as
    ``PdfParser``.)
    """

    def __init__(self, model: OcrModel) -> None:
        try:
            import PIL  # noqa: F401
        except ImportError as missing:  # pragma: no cover - exercised by the message
            raise UnsupportedMediaType(
                "image parsing needs the `ovis-ocr2` extra: "
                "pip install graph-owl-worker[ovis-ocr2]"
            ) from missing
        self._model = model

    def handles(self) -> tuple[str, ...]:
        return ("image/png", "image/jpeg", "image/webp")

    def parse(self, source_id: str, media_type: str, raw: bytes) -> ParsedDocument:
        if media_type not in self.handles():
            raise UnsupportedMediaType(f"no parser for media type `{media_type}`")

        try:
            import io

            from PIL import Image

            image = Image.open(io.BytesIO(raw))
            image.load()
        except Exception as broken:
            raise ParseError(
                f"could not read `{source_id}` as {media_type}: {broken}"
            ) from broken

        pages = self._model.parse_images([image])
        text, sections = assemble_pages(pages)
        return ParsedDocument(
            source_id=source_id,
            media_type=media_type,
            text=text,
            sections=sections,
        )
