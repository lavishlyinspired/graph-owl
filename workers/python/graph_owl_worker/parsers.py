"""Turning bytes into a ``ParsedDocument`` — the Python side of Epic 21 Slice A.

**The protocol is the deliverable; the parsers are proof it holds.** A parser's
whole job is to produce text plus offsets into that text, because that is the
only representation a markdown file, a PDF, a scanned image and a chat export
can all agree on. Anything richer would be a shape only one of them could speak,
and the boundary would have to move the first time a new format appeared.

Every parser here returns byte offsets. That is not an implementation detail:
graph-owl resolves each evidence span against the exact string the worker sent,
so a parser that counted characters would produce spans that silently point at
the wrong words in any document containing a non-ASCII character — an accent, a
smart quote, an emoji in a chat export.
"""

from __future__ import annotations

import re
from typing import Protocol

from graph_owl_sdk.extraction import ParsedDocument, Section, TextSpan


class ParseError(RuntimeError):
    """The bytes are not the media type they claimed to be."""


class UnsupportedMediaType(RuntimeError):
    """No parser handles this type.

    A routing answer, not a failure of the document — and deliberately a
    distinct exception, because "install the pdf extra" and "this PDF is
    corrupt" are different things for an operator to do.
    """


class DocumentParser(Protocol):
    """Bytes in, neutral document out.

    ``bytes``, not ``str``: an OCR or PDF worker starts from bytes, and a
    protocol demanding valid UTF-8 up front would be one an image-based source
    could not implement.
    """

    def handles(self) -> tuple[str, ...]:
        """Media types this parser claims, for routing."""
        ...

    def parse(self, source_id: str, media_type: str, raw: bytes) -> ParsedDocument:
        ...


_HEADING = re.compile(rb"^(#{1,6})\s+(.*)$")


class TextParser:
    """Markdown and plain text.

    Deliberately not a full markdown implementation. What extraction needs is
    the text and where the headings are — enough to say "this claim came from
    the *Rollback* section". Rendering, links, tables and emphasis are all
    irrelevant to a claim's meaning, and pulling in a parser to discard its
    output would be cost without benefit.
    """

    def handles(self) -> tuple[str, ...]:
        return ("text", "text/plain", "markdown", "text/markdown")

    def parse(self, source_id: str, media_type: str, raw: bytes) -> ParsedDocument:
        if media_type not in self.handles():
            raise UnsupportedMediaType(f"no parser for media type `{media_type}`")

        # **Lossy on purpose.** A runbook with one mangled byte is still a
        # runbook; refusing the whole document over an encoding slip would lose
        # every claim in it to protect nothing. The replacement character stays
        # visible in the evidence span if anyone looks.
        text = raw.decode("utf-8", errors="replace")
        return ParsedDocument(
            source_id=source_id,
            media_type=media_type,
            text=text,
            sections=_sections_of(text),
        )


def _sections_of(text: str) -> list[Section]:
    """ATX headings and the body under each.

    A section runs to the next heading of **any** level, not the next of the
    same level. Nesting would be a tree, and the only thing downstream wants is
    "which heading was this under" — the nearest one, which is what this gives.
    """
    sections: list[Section] = []
    offset = 0
    for line in text.encode("utf-8").splitlines(keepends=True):
        match = _HEADING.match(line.strip())
        if match:
            if sections:
                previous = sections[-1]
                sections[-1] = Section(
                    previous.heading, TextSpan(previous.span.start, offset)
                )
            sections.append(
                Section(
                    match.group(2).decode("utf-8", errors="replace").strip(),
                    TextSpan(offset, len(text.encode("utf-8"))),
                )
            )
        offset += len(line)
    return sections


class PdfParser:
    """PDF text extraction, behind the ``pdf`` extra.

    **This is the parser that justifies the whole out-of-process design.** There
    is no comparable Rust option, it changes at the pace of a format nobody
    controls, and it is nowhere near graph-owl's read path — the three tests
    ``00j-language-boundaries.md`` sets for moving something out of the binary.

    The import is deferred to construction rather than module load so that
    importing this module never requires ``pypdf``. A worker deployed to handle
    only markdown must not fail to start because of a format it will never see.
    """

    def __init__(self) -> None:
        try:
            import pypdf  # noqa: F401
        except ImportError as missing:  # pragma: no cover - exercised by the message
            raise UnsupportedMediaType(
                "PDF parsing needs the `pdf` extra: pip install graph-owl-worker[pdf]"
            ) from missing

    def handles(self) -> tuple[str, ...]:
        return ("pdf", "application/pdf")

    def parse(self, source_id: str, media_type: str, raw: bytes) -> ParsedDocument:
        import io

        import pypdf

        if media_type not in self.handles():
            raise UnsupportedMediaType(f"no parser for media type `{media_type}`")

        try:
            reader = pypdf.PdfReader(io.BytesIO(raw))
            pages = [page.extract_text() or "" for page in reader.pages]
        except Exception as broken:  # pypdf raises a wide family
            # A typed error, not a crash. A corrupt PDF in a batch of a thousand
            # must fail that document, not the run.
            raise ParseError(f"could not read `{source_id}` as PDF: {broken}") from broken

        # Pages become sections, because "page 4" is the coordinate a human
        # checking a claim against the original will actually use. A PDF has no
        # headings this can rely on — extracting them needs layout analysis,
        # which is a different and much less reliable job.
        text_parts: list[str] = []
        sections: list[Section] = []
        offset = 0
        for number, page in enumerate(pages, start=1):
            body = page if page.endswith("\n") else page + "\n"
            size = len(body.encode("utf-8"))
            sections.append(Section(f"page {number}", TextSpan(offset, offset + size)))
            text_parts.append(body)
            offset += size

        return ParsedDocument(
            source_id=source_id,
            media_type=media_type,
            text="".join(text_parts),
            sections=sections,
        )


class ParserRegistry:
    """Routes a media type to the parser that claims it.

    Refusing by name is the point: "no parser configured for application/pdf"
    tells an operator to install an extra, where a generic failure tells them to
    open a debugger.
    """

    def __init__(self, parsers: list[DocumentParser] | None = None) -> None:
        # `TextParser` is always present. That is decision 4 one level down:
        # a worker with no optional extras installed still handles the formats
        # runbooks and decision records are actually written in.
        self._parsers: list[DocumentParser] = list(parsers or []) + [TextParser()]

    def register(self, parser: DocumentParser) -> "ParserRegistry":
        # Prepended, so a registered parser wins over the built-in text one for
        # a type they both claim. A worker that installed a better markdown
        # parser did so deliberately.
        self._parsers.insert(0, parser)
        return self

    def parse(self, source_id: str, media_type: str, raw: bytes) -> ParsedDocument:
        for parser in self._parsers:
            if media_type in parser.handles():
                return parser.parse(source_id, media_type, raw)
        raise UnsupportedMediaType(f"no parser configured for {media_type}")
