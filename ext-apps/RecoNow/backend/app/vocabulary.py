"""A client's own column vocabulary, aligned to the pack's — Plan 123 Slice G.

Every client's ERP calls things something different. Today that is a per-file
column mapping: made at upload, used once, thrown away, and made again next
period. As an **alignment** it becomes a durable, reusable, inspectable fact —
"this client's `Party Code` *is* `gst:supplierGstin`" — with a source and a
confidence a reviewer can disagree with.

`graph-owl-ontology` already ships the machinery (`MatchPredicate`,
`AlignmentSource`, `Alignment`) and `POST /alignments` already exposes it.
This module only builds the requests; posting them is a thin shell, kept
separate so the decisions are testable without a server.

**Two decisions carry the weight here.**

*`closeMatch`, never `exactMatch`.* `Party Code` holds a GSTIN **in this
client's files**; it is not the same concept as `gst:supplierGstin`
universally. `skos:exactMatch` is transitive, so overclaiming would let two
unrelated clients' columns become equivalent *through* the pack term — a
silent, spreading error of exactly the kind an alignment is supposed to make
visible.

*A guess and a confirmation are never recorded identically.* graph-owl asserts
a direct triple at confidence >= 0.8 and holds 0.5..0.8 in a review band. A
human-confirmed mapping is curated at 1.0; an automated header guess sits in
the band — durable enough to reuse, not so durable that nobody looks at it.
An auto-mapping that became indistinguishable from a confirmation is how one
bad header propagates into every future period.
"""

from __future__ import annotations

from urllib.parse import quote

#: The pack's own namespace. A client term aligns *to* a term here.
PACK_NAMESPACE = "https://graph-owl.dev/packs/gst#"

#: Where a client's own vocabulary lives. Scoped per client because two
#: clients may both call a column `Party Code` and mean different things —
#: an unscoped term would let one client's vocabulary redefine another's.
CLIENT_NAMESPACE = "https://graph-owl.dev/vocab/client/"

#: `graphowl_client.PREDICATES` maps a row field to a pack predicate, and
#: those are exactly the fields a mapping can bind. Imported lazily inside the
#: function rather than at module scope to keep this module free of the
#: ingestion client's own import weight.
CLOSE_MATCH = "http://www.w3.org/2004/02/skos/core#closeMatch"

#: Below graph-owl's 0.8 assert threshold and above its 0.5 review floor, so
#: an automated mapping lands in the review band by construction rather than
#: by a caller remembering to put it there.
COMPUTED_CONFIDENCE = 0.6


def client_term(client_id: str, header: str) -> str:
    """One IRI per (client, header). Percent-encoded because a real export
    header contains spaces, slashes and parentheses."""
    return f"{CLIENT_NAMESPACE}{quote(client_id, safe='')}#{quote(header.strip(), safe='')}"


def _pack_predicates() -> dict[str, str]:
    from .graphowl_client import PREDICATES, SUMMARY_PREDICATES

    merged = dict(PREDICATES)
    merged.update(SUMMARY_PREDICATES)
    # Not in `PREDICATES` — the finding queries read it off the Supplier
    # subject rather than as a literal on the invoice — but it is the single
    # most-mapped column in any real file, so a vocabulary that could not
    # express it would miss the case it exists for.
    merged.setdefault("supplier_gstin", "supplierGstin")
    merged.setdefault("supplier_name", "supplierName")
    return merged


def alignment_requests(
    *,
    client_id: str,
    headers: list[str],
    mapping: dict[str, int | None],
    confirmed_by_human: bool,
) -> list[dict]:
    """One `POST /alignments` body per mapped column.

    `mapping` is `main._auto_map`'s shape: row field -> column index, or
    `None` where nothing was bound. An index past the end of `headers` — a
    stale template against a changed file — is **skipped rather than raised**:
    losing one alignment is recoverable, failing the upload is not.
    """
    predicates = _pack_predicates()
    source = (
        {"kind": "human", "detail": "confirmed at upload"}
        if confirmed_by_human
        else {"kind": "computed", "detail": "header match at upload"}
    )
    confidence = 1.0 if confirmed_by_human else COMPUTED_CONFIDENCE

    requests: list[dict] = []
    for field, index in sorted(mapping.items()):
        if index is None or not 0 <= index < len(headers):
            continue
        predicate = predicates.get(field)
        if predicate is None:
            continue
        requests.append(
            {
                "kind": "match",
                "left": client_term(client_id, headers[index]),
                "right": f"{PACK_NAMESPACE}{predicate}",
                "predicate": CLOSE_MATCH,
                "source": source,
                "confidence": confidence,
            }
        )
    return requests


__all__ = [
    "CLIENT_NAMESPACE",
    "CLOSE_MATCH",
    "COMPUTED_CONFIDENCE",
    "PACK_NAMESPACE",
    "alignment_requests",
    "client_term",
]
