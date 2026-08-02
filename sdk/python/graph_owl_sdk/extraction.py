"""Submitting extracted claims — Epic 21, the Python half.

**This is the module that makes decision 0 real.** PDF layout analysis, OCR and
LLM extraction have a Python ecosystem Rust does not come close to matching, and
none of it belongs on graph-owl's read path — so the worker runs out of process,
in Python, and hands over JSON. What is in this file is that JSON, as
dataclasses, plus the one client call that sends it.

**What is deliberately *not* here is the policy.** A worker does not decide
whether its confidence is high enough to assert, whether its predicate exists,
or whether a human already rejected the same claim. graph-owl decides all three,
on its side of the boundary, for every claim from every source — including its
own in-process extractor, which gets no exemption for being local. A worker
proposes; graph-owl disposes. Putting the bands in this file would move the
decision to the component least able to promise anything about it, which for an
LLM extractor is not a hypothetical.

Stdlib only, like the rest of this SDK. See ``pyproject.toml`` for why.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any


@dataclass(frozen=True)
class TextSpan:
    """Byte offsets into the parsed text.

    **Bytes, not characters, and not line/column.** A PDF worker and an OCR
    worker have no meaningful notion of a line, and the Rust side resolves these
    against the exact string this worker sent — so the only representation both
    can agree on is an offset into that string.
    """

    start: int
    end: int

    def wire(self) -> dict[str, int]:
        return {"start": self.start, "end": self.end}

    def resolve(self, text: str) -> str | None:
        """The text this span refers to, or ``None`` if it does not fit.

        Worth calling before submitting: a span the worker cannot resolve is one
        graph-owl cannot resolve either, and the reviewer sees "(the evidence
        span does not resolve against the source)" instead of a sentence.
        """
        raw = text.encode("utf-8")
        if self.start > self.end or self.end > len(raw):
            return None
        try:
            return raw[self.start : self.end].decode("utf-8")
        except UnicodeDecodeError:
            # The span cut a multi-byte character in half. Not an error to
            # raise — it is a miscount, and the caller wants to know it failed
            # rather than to handle an exception per claim.
            return None


@dataclass(frozen=True)
class Section:
    heading: str | None
    span: TextSpan

    def wire(self) -> dict[str, Any]:
        return {"heading": self.heading, "span": self.span.wire()}


@dataclass(frozen=True)
class ParsedDocument:
    """A document reduced to what every parser can agree on.

    Deliberately *not* an AST. Markdown, PDF, OCR and a chat export have nothing
    structural in common, and a representation rich enough for one is wrong for
    the others — but all of them produce text, and all of them can say which part
    of it a claim came from.
    """

    source_id: str
    media_type: str
    text: str
    sections: list[Section] = field(default_factory=list)

    def wire(self) -> dict[str, Any]:
        body: dict[str, Any] = {
            "sourceId": self.source_id,
            "mediaType": self.media_type,
            "text": self.text,
        }
        if self.sections:
            body["sections"] = [section.wire() for section in self.sections]
        return body

    def fingerprint(self) -> str:
        """The content hash graph-owl judges idempotence on.

        Exposed so a worker can skip parsing a document it already submitted —
        the server would answer ``alreadyExtracted`` anyway, but a PDF worker
        that can skip the OCR pass saves considerably more than a round trip.
        """
        return hashlib.sha256(self.text.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class Provenance:
    source_id: str
    extractor: str
    extractor_version: str
    evidence: TextSpan
    extracted_at: datetime | None = None

    def wire(self) -> dict[str, Any]:
        when = self.extracted_at or datetime.now(timezone.utc)
        return {
            "sourceId": self.source_id,
            "extractor": self.extractor,
            "extractorVersion": self.extractor_version,
            # RFC 3339 with an explicit offset. A naive datetime would serialize
            # without one and be rejected, which is a confusing failure to debug
            # from the server's side of the boundary.
            "extractedAt": when.astimezone(timezone.utc).isoformat(),
            "evidence": self.evidence.wire(),
        }


@dataclass(frozen=True)
class Claim:
    """One proposed assertion.

    ``confidence`` is a **proposal, never a decision**. graph-owl's bands decide
    what it buys: at or above 0.8 it asserts, between 0.5 and 0.8 it waits for a
    human, below 0.5 it is discarded and counted. A worker that inflates this
    number does not gain anything it could not have got honestly — but it does
    lose the review that would have caught it being wrong.
    """

    subject: str
    predicate: str
    object: str
    confidence: float
    provenance: Provenance

    def wire(self) -> dict[str, Any]:
        return {
            "subject": self.subject,
            "predicate": self.predicate,
            "object": self.object,
            "confidence": self.confidence,
            "provenance": self.provenance.wire(),
        }


@dataclass
class ExtractionResult:
    claims: list[Claim] = field(default_factory=list)

    def wire(self) -> dict[str, Any]:
        return {"claims": [claim.wire() for claim in self.claims]}


#: The predicates graph-owl will accept. Mirrored here so a worker can drop an
#: off-vocabulary claim before spending a round trip on it — **not** so it can
#: decide the question. The server checks again regardless, which is what makes
#: this a convenience rather than a trust boundary, and a worker running against
#: a newer server may simply find this list short.
CATALOG_PREDICATES = (
    "description",
    "owner",
    "tag",
    "term",
    "feeds",
    "derivedFrom",
    "dependsOn",
)

#: What graph-owl does with a proposed confidence. Documented, not enforced —
#: see the module note.
ASSERT_THRESHOLD = 0.8
SURFACE_THRESHOLD = 0.5


def submit_extraction(
    client: Any,
    document: ParsedDocument,
    result: ExtractionResult,
    extractor: str,
    extractor_version: str,
) -> dict[str, Any]:
    """Hand a run to graph-owl and return what it did with it.

    ``client`` is a :class:`~graph_owl_sdk.ingest.GraphOwlClient` — taken as a
    parameter rather than made a method on it so this module stays importable
    without the ingestion path, and so a worker can be tested against a fake
    that records what it was sent.

    The answer is one of two shapes, and the difference matters to a retrying
    worker:

    - ``{"outcome": "recorded", "runId": ..., "asserted": n, "surfaced": n,
      "discarded": n}`` — this submission produced a run.
    - ``{"outcome": "alreadyExtracted", "runId": ...}`` — this exact document
      has already been through this exact extractor, so nothing was done.

    A worker retrying after a timeout gets the second, which is how it tells
    "graph-owl already had this" from "graph-owl found nothing in it". Those look
    identical if you only count claims.

    :raises GraphOwlError: if the catalog refuses the submission.
    """
    return client.request(
        "POST",
        "/extraction/runs",
        body={
            "document": document.wire(),
            "result": result.wire(),
            "extractor": extractor,
            "extractorVersion": extractor_version,
        },
    )


def review_queue(client: Any) -> list[dict[str, Any]]:
    """Claims waiting for a human, each with the sentence it came from."""
    answer = client.request("GET", "/extraction/queue")
    return answer if isinstance(answer, list) else []


def decide(client: Any, claim_id: str, confirmed: bool) -> dict[str, Any]:
    """Confirm or reject a queued claim.

    ``confirmed`` is required and has no default on either side of the wire.
    Both directions of a default are wrong: true asserts what nobody approved,
    false rejects what nobody refused.
    """
    return client.request(
        "POST",
        f"/extraction/claims/{claim_id}/decision",
        body={"confirmed": confirmed},
    )
