"""Proposing claims from a parsed document.

**Deterministic and rule-based, on purpose.** The interesting extractor is an
LLM one, and this is not it — but it is the shape that one has to fit, and
having a working non-probabilistic extractor first means the pipeline can be
tested for correctness rather than for plausibility. When the LLM extractor
arrives it replaces this class and nothing else: same protocol, same output,
same policy applied on graph-owl's side.

**The confidence here is 0.6, and that is a claim about evidence rather than a
tuning knob.** A name appearing in prose near a predicate phrase is *evidence*
that the document says something about that entity — it is not proof. 0.6 lands
in graph-owl's surface band, so every claim from this extractor waits for a
human. An extractor claiming 0.9 for a substring match would be asserting into
the graph on the strength of a string match, and would be believed.
"""

from __future__ import annotations

import re
from typing import Iterable, Protocol

from graph_owl_sdk.extraction import (
    Claim,
    ExtractionResult,
    ParsedDocument,
    Provenance,
    TextSpan,
)

#: What a rule-based match is worth. See the module note — this is deliberately
#: inside the surface band, never the assert band.
MENTION_CONFIDENCE = 0.6

#: Phrases that suggest the sentence is describing the entity rather than merely
#: naming it. A small, explicit list rather than a model: every entry is here
#: because it was chosen, and an operator reading this file can tell why a
#: sentence did or did not produce a claim.
_DESCRIBES = (
    " is ",
    " are ",
    " holds ",
    " contains ",
    " stores ",
    " records ",
)


class ClaimExtractor(Protocol):
    def name(self) -> str:
        """The extractor's identity, carried into every claim's provenance.

        A string the worker chooses for itself, because graph-owl has no enum of
        extractor kinds — adding a worker must be a deployment, not a migration
        of a type that has already been persisted.
        """
        ...

    def version(self) -> str:
        ...

    def extract(
        self, document: ParsedDocument, subjects: Iterable[str]
    ) -> ExtractionResult:
        ...


def sentences(text: str) -> list[tuple[int, int]]:
    """Byte spans of each sentence.

    **A period only ends a sentence when whitespace or the end of the text
    follows it**, because fully-qualified names contain periods. Splitting on
    every period tears ``svc.db.orders`` into three fragments, so no sentence
    ever contains the subject and the extractor finds *nothing at all* — silent
    and total, and indistinguishable from a document that mentions nothing. The
    Rust extractor had exactly this bug and its test suite caught it; this is
    the same rule, written down where the next person to touch it will see it.
    """
    raw = text.encode("utf-8")
    spans: list[tuple[int, int]] = []
    start = 0
    for index, byte in enumerate(raw):
        if byte not in b".!?":
            continue
        following = raw[index + 1 : index + 2]
        if following and not following.isspace():
            continue
        end = index + 1
        if end > start:
            spans.append((start, end))
        start = end
    if start < len(raw):
        spans.append((start, len(raw)))
    return spans


class MentionExtractor:
    """Claims from entity names appearing in descriptive sentences."""

    def name(self) -> str:
        return "python-mention-rules"

    def version(self) -> str:
        return "1"

    def extract(
        self, document: ParsedDocument, subjects: Iterable[str]
    ) -> ExtractionResult:
        known = list(subjects)
        raw = document.text.encode("utf-8")
        claims: list[Claim] = []

        for start, end in sentences(document.text):
            sentence = raw[start:end].decode("utf-8", errors="replace")
            lowered = sentence.lower()
            if not any(marker in lowered for marker in _DESCRIBES):
                continue

            for subject in known:
                # The *last* segment, because a document says "the orders
                # table", not "the svc.db.orders table". Matching the whole FQN
                # would find almost nothing in real prose.
                leaf = subject.rsplit(".", 1)[-1]
                if not re.search(rf"\b{re.escape(leaf)}\b", sentence, re.IGNORECASE):
                    continue
                claims.append(
                    Claim(
                        subject=subject,
                        predicate="description",
                        object=sentence.strip(),
                        confidence=MENTION_CONFIDENCE,
                        provenance=Provenance(
                            source_id=document.source_id,
                            extractor=self.name(),
                            extractor_version=self.version(),
                            # The sentence, not the matched name. A reviewer
                            # needs what the document said, and a span covering
                            # only the entity's name would show them the word
                            # they already know.
                            evidence=TextSpan(start, end),
                        ),
                    )
                )

        return ExtractionResult(claims=claims)
