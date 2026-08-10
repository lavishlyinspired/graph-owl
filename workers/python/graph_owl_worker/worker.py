"""The run loop: read, parse, extract, submit.

Four steps and one decision — **the worker never decides what its output is
worth**. It proposes; graph-owl's bands, vocabulary and rejection history
decide. That is why this file has no thresholds in it.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from graph_owl_sdk.extraction import submit_extraction

from .extract import ClaimExtractor, MentionExtractor
from .parsers import ParserRegistry, ParseError, UnsupportedMediaType

#: Extension to media type. Small and explicit: guessing from content is a
#: research problem, and an operator who names the type is never wrong.
MEDIA_TYPES = {
    ".md": "markdown",
    ".markdown": "markdown",
    ".txt": "text",
    ".pdf": "application/pdf",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
}


@dataclass
class DocumentOutcome:
    source_id: str
    outcome: str
    detail: dict[str, Any]


class Worker:
    def __init__(
        self,
        client: Any,
        registry: ParserRegistry | None = None,
        extractor: ClaimExtractor | None = None,
    ) -> None:
        self.client = client
        self.registry = registry or ParserRegistry()
        self.extractor = extractor or MentionExtractor()

    def process(
        self, path: Path, subjects: Iterable[str], media_type: str | None = None
    ) -> DocumentOutcome:
        """One document, end to end.

        The **path is the source id**, so re-running over the same tree is
        recognised as the same documents. A generated id would make every run
        look like a first run, and idempotence would silently never fire — the
        expensive failure, since the whole point of the fingerprint is to skip
        an OCR pass.
        """
        source_id = str(path)
        resolved = media_type or MEDIA_TYPES.get(path.suffix.lower())
        if resolved is None:
            return DocumentOutcome(
                source_id,
                "skipped",
                {"reason": f"no media type known for `{path.suffix}`"},
            )

        try:
            document = self.registry.parse(source_id, resolved, path.read_bytes())
        except (ParseError, UnsupportedMediaType) as refused:
            # A document that cannot be read fails *that document*. A batch of a
            # thousand runbooks must not be lost to one corrupt PDF.
            return DocumentOutcome(source_id, "failed", {"reason": str(refused)})

        result = self.extractor.extract(document, subjects)
        answer = submit_extraction(
            self.client,
            document,
            result,
            self.extractor.name(),
            self.extractor.version(),
        )
        return DocumentOutcome(source_id, answer.get("outcome", "unknown"), answer)

    def process_tree(
        self, root: Path, subjects: Iterable[str]
    ) -> list[DocumentOutcome]:
        """Every document under ``root`` whose extension is known.

        ``subjects`` is materialised once: it is consumed per document, and a
        generator would be empty after the first.
        """
        known = list(subjects)
        outcomes: list[DocumentOutcome] = []
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.suffix.lower() in MEDIA_TYPES:
                outcomes.append(self.process(path, known))
        return outcomes


def catalog_subjects(client: Any, limit: int = 1000) -> list[str]:
    """The FQNs a claim may be about.

    Fetched from the catalog rather than configured, because a worker with a
    stale list proposes claims about entities that no longer exist and every one
    of them is discarded — quietly, and looking exactly like an extractor that
    found nothing.
    """
    answer = client.request("GET", f"/assets?limit={limit}")
    data = answer.get("data", []) if isinstance(answer, dict) else []
    return [
        asset["fullyQualifiedName"]
        for asset in data
        if isinstance(asset, dict) and "fullyQualifiedName" in asset
    ]
