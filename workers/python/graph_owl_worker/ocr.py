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

import base64
import io
import json
import re
import urllib.error
import urllib.request
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

#: Where an OCR-capable endpoint is expected to listen by default — a served
#: local model on its conventional port, overridable by the operator.
DEFAULT_ENDPOINT = "http://localhost:8000"

#: The model this parser is built against. Overridable per call; the value is
#: only a default so an unconfigured worker names something plausible in logs
#: rather than an empty string.
DEFAULT_MODEL = "ATH-MaaS/OvisOCR2"

#: Render resolution for ``OcrPdfParser``'s rasterization step. 200 DPI is the
#: commonly-cited floor for legible OCR text recognition — below it, small
#: body text degrades into ambiguous glyphs; well above it, page images grow
#: faster than recognition accuracy improves. Configurable per call for a
#: corpus that needs otherwise.
DEFAULT_DPI = 200

#: Originally authored for this project (never read from, or copied out of,
#: any vendor source — see plans/00i-licensing.md). Written to match the two
#: things this module already assumes about a page's transcription: tables
#: and formulas need a shape ``assemble_pages`` can pass through untouched
#: (HTML, LaTeX), and a figure must come back as exactly the placeholder tag
#: ``_IMG_TAG`` already knows how to strip, or a figure silently survives
#: into the fingerprinted text as noise.
OCR_PROMPT = (
    "Transcribe this page image to Markdown, preserving reading order. "
    "Render every table as an HTML <table> element, and every mathematical "
    "formula as LaTeX. For each figure or embedded image, do not describe or "
    "transcribe its contents — instead emit exactly one placeholder tag in "
    'the form <img src="images/bbox_{left}_{top}_{right}_{bottom}.jpg" />, '
    "with the figure's bounding box on a 0-1000 scale. Output only the "
    "transcription: no preamble, no commentary, no code fence."
)


class OcrModel(Protocol):
    """The seam: page images in, one Markdown string per page, in order.

    One method, no exceptions in the protocol: how a model is reached (a served
    endpoint today, an in-process load tomorrow) is an implementation detail of
    the object behind this, and a parser must not know which it is.
    """

    def parse_images(self, images: list[Image.Image]) -> list[str]:
        ...


class OcrError(RuntimeError):
    """A served OCR endpoint could not be reached, refused a page, or
    answered with something this client does not recognise as a chat
    completion. Always names the endpoint and the page, since a worker
    running a real document has no other way to tell which page to retry."""


class EndpointOcrModel:
    """An ``OcrModel`` that talks to a served endpoint over OpenAI-compatible
    chat completions — one request per page, image as a base64 data URL.

    stdlib HTTP only (``urllib``): a network client is not something the
    worker's core install should carry for a model most deployments never
    call, and every other dependency in this module is deferred the same
    way for the same reason.
    """

    def __init__(
        self,
        endpoint: str = DEFAULT_ENDPOINT,
        model: str = DEFAULT_MODEL,
        prompt: str = OCR_PROMPT,
    ) -> None:
        self._endpoint = endpoint
        self._model = model
        self._prompt = prompt

    def parse_images(self, images: list[Image.Image]) -> list[str]:
        return [
            self._parse_one(number, image)
            for number, image in enumerate(images, start=1)
        ]

    def _parse_one(self, number: int, image: Image.Image) -> str:
        payload = json.dumps(
            {
                "model": self._model,
                "temperature": 0.0,
                "max_tokens": 16384,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "image_url",
                                "image_url": {"url": self._data_url(image)},
                            },
                            {"type": "text", "text": self._prompt},
                        ],
                    }
                ],
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            f"{self._endpoint}/v1/chat/completions",
            data=payload,
            headers={"content-type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(request) as response:
                body = json.loads(response.read())
        except urllib.error.HTTPError as refused:
            raise OcrError(
                f"OCR endpoint {self._endpoint} refused page {number}: "
                f"HTTP {refused.code}"
            ) from refused
        except urllib.error.URLError as unreachable:
            raise OcrError(
                f"OCR endpoint {self._endpoint} was unreachable for page "
                f"{number}: {unreachable.reason}"
            ) from unreachable
        except json.JSONDecodeError as not_json:
            raise OcrError(
                f"OCR endpoint {self._endpoint} returned a non-JSON response "
                f"for page {number}"
            ) from not_json

        try:
            return body["choices"][0]["message"]["content"]
        except (KeyError, IndexError, TypeError) as unexpected_shape:
            raise OcrError(
                f"OCR endpoint {self._endpoint} returned an unexpected "
                f"response shape for page {number}"
            ) from unexpected_shape

    @staticmethod
    def _data_url(image: Image.Image) -> str:
        buffer = io.BytesIO()
        image.save(buffer, format="PNG")
        encoded = base64.b64encode(buffer.getvalue()).decode("ascii")
        return f"data:image/png;base64,{encoded}"


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


def _run_ocr(
    model: OcrModel, images: list[Image.Image], source_id: str, media_type: str
) -> tuple[str, list[Section]]:
    """Shared by every OCR-backed parser: call the model, map its one
    exception type to ``ParseError``, assemble the result.

    A batch of a thousand documents must not be lost to one endpoint hiccup —
    the same reason a corrupt page is a ``ParseError`` rather than an
    uncaught exception. ``Worker.process`` only catches
    ``ParseError``/``UnsupportedMediaType``, so this is the boundary where an
    ``OcrError`` has to stop being an ``OcrError``.
    """
    try:
        pages = model.parse_images(images)
    except OcrError as failed:
        raise ParseError(
            f"could not read `{source_id}` as {media_type}: {failed}"
        ) from failed
    return assemble_pages(pages)


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

        text, sections = _run_ocr(self._model, [image], source_id, media_type)
        return ParsedDocument(
            source_id=source_id,
            media_type=media_type,
            text=text,
            sections=sections,
        )


def _dpi_to_scale(dpi: int) -> float:
    """PDF canvas units are 1/72 inch; pdfium's render ``scale`` parameter is
    a factor on that unit, so a DPI value converts by that ratio.

    Kept as its own pure function so this arithmetic is unit-testable without
    ever calling pdfium's renderer: an inverted formula here (``dpi * 72``
    instead of ``dpi / 72``) asks a native C++ library to rasterize a canvas
    thousands of times too large and segfaults the whole test process rather
    than raising a catchable Python exception — found by mutation testing,
    not designed for in advance.
    """
    return dpi / 72


class OcrPdfParser:
    """Scanned PDFs: each page rasterized to an image, then the same
    ``OcrModel`` path ``OvisOcrParser`` uses — behind the ``ovis-ocr2``
    extra, which carries both Pillow and ``pypdfium2`` (see
    ``plans/00l-build-vs-adopt.md`` for the licence/maintenance check).

    A PDF's own text layer is never read here: a scanned PDF usually has
    none, and one that does still gets rasterized, because there is no
    reliable way to tell "this page has no text" from "this page's text
    extraction failed" without attempting extraction and being wrong
    silently. Registering this parser wins ``application/pdf`` away from
    ``PdfParser`` (``ParserRegistry.register`` prepends), which is the CLI's
    job when ``--ocr`` is set — see ``cli.py``.
    """

    def __init__(self, model: OcrModel, dpi: int = DEFAULT_DPI) -> None:
        try:
            import pypdfium2  # noqa: F401
        except ImportError as missing:  # pragma: no cover - exercised by the message
            raise UnsupportedMediaType(
                "scanned-PDF OCR needs the `ovis-ocr2` extra: "
                "pip install graph-owl-worker[ovis-ocr2]"
            ) from missing
        self._model = model
        self._dpi = dpi

    def handles(self) -> tuple[str, ...]:
        return ("application/pdf",)

    def parse(self, source_id: str, media_type: str, raw: bytes) -> ParsedDocument:
        if media_type not in self.handles():
            raise UnsupportedMediaType(f"no parser for media type `{media_type}`")

        try:
            import pypdfium2 as pdfium

            scale = _dpi_to_scale(self._dpi)
            document = pdfium.PdfDocument(raw)
            images = [page.render(scale=scale).to_pil() for page in document]
        except Exception as broken:
            raise ParseError(
                f"could not read `{source_id}` as {media_type}: {broken}"
            ) from broken

        text, sections = _run_ocr(self._model, images, source_id, media_type)
        return ParsedDocument(
            source_id=source_id,
            media_type=media_type,
            text=text,
            sections=sections,
        )
