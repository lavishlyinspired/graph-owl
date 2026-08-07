"""Render graph context to text a model reads plus metadata a program reads.

Decision 3 (`43-framework-integrations.md`): retrieval returns graph context,
not flattened text. ``page_content`` carries a rendered summary; ``metadata``
carries the structured truth. Decision 4: a derived fact is labelled in the
text itself, not only in metadata — an LLM handed an inference as though it
were an assertion states it as fact.

**Known gap against Epic 14, found building this**: the plan's own example
exposes ``as_of`` on every read surface. None of the seven MCP read tools'
input schemas (`crates/graph-owl-mcp/src/lib.rs::tools()`) accept an
``as_of`` argument today — `get_asset_context`, `search_assets`,
`recall_memory` and the rest all read current state only. This module
threads an ``as_of`` field through regardless (so the shape is ready), but
nothing in this package can *populate* it truthfully yet. Per this epic's
own rule ("friction found here is an API or MCP defect, logged against
Epic 14, not worked around"), this is exactly that — logged, not patched
over by inventing a client-side filter that would silently disagree with
whatever the server actually returned.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

#: `MemoryContext.confidence` below this is the ignore band (`00c-domain-
#: model.md`'s generic bands) — excluded from retrieval by default.
IGNORE_BAND_CEILING = 0.5


@dataclass(frozen=True)
class RelatedFact:
    """One edge or recalled memory contributing to an asset's context.

    ``derived`` is true for anything the catalog inferred rather than
    something a human or a connector asserted — reasoning conclusions
    (Epic 6) and machine-authored memories both qualify.
    """

    text: str
    derived: bool
    confidence: float | None = None
    relationship: str | None = None
    source: str | None = None


@dataclass(frozen=True)
class GraphContext:
    """Everything retrieved about one asset, before rendering.

    Built from real MCP tool responses (``get_asset_context``,
    ``explain_lineage``, ``recall_memory``) by the retriever; this type
    itself makes no MCP calls, which is what keeps rendering testable
    without a client.
    """

    fully_qualified_name: str
    kind: str
    description: str | None
    facts: list[RelatedFact] = field(default_factory=list)
    truncated: bool = False
    as_of: str | None = None


def render(context: GraphContext) -> tuple[str, dict[str, Any]]:
    """``(page_content, metadata)`` for one asset's context.

    Decision 4 in the text itself: every derived fact's line is prefixed
    ``[inferred]``, never left to metadata alone — metadata is read by code,
    and the model reads ``page_content``.
    """
    lines = [f"{context.fully_qualified_name} ({context.kind})"]
    if context.description:
        lines.append(context.description)

    for fact in context.facts:
        prefix = "[inferred] " if fact.derived else ""
        lines.append(f"{prefix}{fact.text}")

    if context.truncated:
        lines.append(
            "[truncated] Not all related facts fit the response budget; "
            "this is a partial picture, not the complete one."
        )

    metadata: dict[str, Any] = {
        "fullyQualifiedName": context.fully_qualified_name,
        "kind": context.kind,
        "truncated": context.truncated,
        "asOf": context.as_of,
        "facts": [
            {
                "text": fact.text,
                "derived": fact.derived,
                "confidence": fact.confidence,
                "relationship": fact.relationship,
                "source": fact.source,
            }
            for fact in context.facts
        ],
    }
    return "\n".join(lines), metadata


def visible_facts(
    facts: list[RelatedFact], include_derived: bool, min_confidence: float
) -> list[RelatedFact]:
    """Apply decision 3's default exclusions before rendering.

    A fact with no ``confidence`` at all (a lineage edge, not a memory) is
    never excluded by the confidence floor — the floor is a memory-band
    concept and has nothing to say about an edge that was simply asserted.
    """
    kept = []
    for fact in facts:
        if fact.derived and not include_derived:
            continue
        if fact.confidence is not None and fact.confidence < min_confidence:
            continue
        kept.append(fact)
    return kept
