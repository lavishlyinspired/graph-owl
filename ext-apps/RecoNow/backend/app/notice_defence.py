"""The derivation behind a case — Plan 123 Slice E, over `/reasoning/explain`.

**The plan calls this "notice defence", and that names the requirement
exactly.** When an officer asks why a credit was treated the way it was, "the
system flagged it" is not an answer. What is an answer: *this rule*, resting
on *this provision*, fired because *these facts* held, and those facts came
from *this row* of *this file*, uploaded on *this date*.

Reco Now had the finding and the citation and stopped there. graph-owl has
carried the derivation since Epic 6 and nothing asked for it.

**Assembled here rather than in the UI**, because the chain spans three
sources — the case, the graph's own explanation, and the upload that produced
the facts — and a chain assembled per-screen would differ per screen. A
defence pack that disagrees with the screen it was exported from is worse than
no defence pack.

**Every step reports whether it is actually known.** The source step is the
one most likely to be missing, since a case can outlive its upload record, and
omitting a step silently would make the chain look complete when its last link
is absent — which is exactly the moment somebody relies on it.
"""

from __future__ import annotations

from typing import Any


def explain_query(case: dict[str, Any], *, predicate: str, value: str) -> dict[str, str]:
    """The `GET /reasoning/explain` parameters for one of a case's own facts.

    # Raises

    `ValueError` if the case has no recorded subject. Guessing an IRI from the
    invoice number would produce a defence pack citing facts about a subject
    that may not exist — an error which reads as thoroughness.
    """
    subject = case.get("subject")
    if not subject:
        raise ValueError(
            "this case has no recorded subject, so its facts cannot be traced"
        )
    return {"s": str(subject), "p": predicate, "o": value}


def _figure_step(case: dict[str, Any]) -> dict[str, Any]:
    books = case.get("books_amount")
    portal = case.get("portal_amount")
    return {
        "kind": "figure",
        "label": "The amount in question",
        # Both sides where both are known: a mismatch is a *disagreement*, and
        # a chain naming one number invites "against what?".
        "books": books,
        "portal": portal,
        # Absent is not zero — the same distinction the working paper draws.
        # A case with no amount evidence must not appear in a defence pack
        # asserting that nothing was at stake.
        "known": books is not None or portal is not None,
    }


def defence_chain(
    *,
    case: dict[str, Any],
    explanation: dict[str, Any] | None,
    upload: dict[str, Any] | None,
) -> dict[str, Any]:
    """figure → finding → provision → derivation → source.

    That order, and not the other way round: a chain starting at the rule
    leaves the reader asking where the number came from, which is the first
    thing an officer asks.
    """
    derivation = (explanation or {}).get("derivation") or []

    steps: list[dict[str, Any]] = [
        _figure_step(case),
        {
            "kind": "finding",
            "label": case.get("reason_code"),
            "detail": case.get("summary"),
            "known": bool(case.get("reason_code")),
        },
        {
            "kind": "provision",
            "label": "Governed by",
            # The citation itself, not merely its name — a reader has to be
            # able to look it up.
            "citation": case.get("governed_by"),
            "known": bool(case.get("governed_by")),
        },
        {
            "kind": "derivation",
            "label": "Why the rule fired",
            "steps": derivation,
            # An empty explanation and an unattempted one are different, and
            # silently omitting this step implies there was nothing to give.
            "known": bool(derivation),
        },
        {
            "kind": "source",
            "label": "Where the facts came from",
            "filename": (upload or {}).get("filename"),
            "uploaded_at": (upload or {}).get("uploaded_at"),
            "row": (upload or {}).get("row"),
            "known": bool(upload),
        },
    ]

    return {
        "case_id": case.get("id"),
        "invoice_no": case.get("invoice_no"),
        "steps": steps,
        # A chain is only a defence if every link holds. Stated rather than
        # left for a reader to notice a blank.
        "complete": all(step["known"] for step in steps),
    }


__all__ = ["defence_chain", "explain_query"]
