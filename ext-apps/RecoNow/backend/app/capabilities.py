"""Reco Now using what graph-owl already ships — Plan 123 Slice E.

graph-owl carries memories, waivers, threads, lineage and an explain endpoint,
and Reco Now used none of them. It kept its own approximations instead, so
knowledge that should outlive a period died with it.

**The memory case changes how the product behaves.** "This supplier always
files late" is exactly the judgement a CA builds over months and can record
nowhere today: next period the same supplier is flagged again with no
indication that this is the fourth time. A memory survives periods, is
corrected by *superseding* rather than editing, and is never destroyed — so a
wrong one can be withdrawn without erasing the fact that it was once believed,
which is what a reviewer asking "why did we treat them that way in March"
needs.

**A waiver that does not expire is a rule change nobody voted for.** Accepting
an exception permanently means the check stops running and nobody remembers
deciding that. graph-owl requires a reason *and* an expiry; Reco Now's own
`approval` table required neither.
"""

from __future__ import annotations

from collections import defaultdict
from datetime import datetime, timezone
from typing import Any

#: How many **distinct periods** a problem must recur across before it is a
#: characteristic of the supplier rather than an incident.
#:
#: Three is a quarter — the shortest span over which "always" is a defensible
#: word, and short enough to be useful within a financial year. Periods, not
#: occurrences: three invoices unfiled in one month is one bad month, and
#: counting occurrences would turn it into a permanent judgement about a
#: supplier who may have had a system outage.
MIN_PERIODS_FOR_A_PATTERN = 3

#: Confidence gained per period beyond the threshold, and the ceiling.
#:
#: Capped **below 1.0 deliberately**: an inference must stay distinguishable
#: from something a human confirmed. A memory at 1.0 is a statement of fact,
#: and this is a statement about a trend.
CONFIDENCE_BASE = 0.6
CONFIDENCE_PER_PERIOD = 0.04
CONFIDENCE_CEILING = 0.95


def supplier_pattern(observations: list[dict]) -> dict[str, Any] | None:
    """The one problem this supplier has across enough periods to be a habit.

    `observations` are `{period, reason_code}` pairs — every finding raised
    against this supplier, across every period held.

    Returns `None` unless a *single* label recurs across at least
    `MIN_PERIODS_FOR_A_PATTERN` distinct periods. Different problems in
    different periods are not one characteristic, and merging them would
    invent a claim nobody could defend.
    """
    periods_by_label: dict[str, set[str]] = defaultdict(set)
    for observation in observations:
        label = observation.get("reason_code")
        period = observation.get("period")
        if label and period:
            periods_by_label[label].add(str(period))

    if not periods_by_label:
        return None

    # Strongest first; ties broken by label so the result is stable rather
    # than dependent on dict ordering — a memory that changes its mind between
    # identical runs is worse than no memory.
    label, periods = max(
        periods_by_label.items(), key=lambda item: (len(item[1]), item[0])
    )
    if len(periods) < MIN_PERIODS_FOR_A_PATTERN:
        return None
    return {"label": label, "periods": len(periods), "seen": sorted(periods)}


def memory_for_supplier(*, gstin: str, name: str, pattern: dict[str, Any]) -> dict[str, Any]:
    """A `POST /memories` body recording what this supplier reliably does.

    Anchored to the **supplier**, never to a period — a memory linked to a
    period dies with it, which is the exact failure this replaces.
    """
    periods = int(pattern["periods"])
    confidence = min(
        CONFIDENCE_CEILING,
        CONFIDENCE_BASE + CONFIDENCE_PER_PERIOD * (periods - MIN_PERIODS_FOR_A_PATTERN),
    )
    problem = str(pattern["label"]).split(":")[-1]
    seen = ", ".join(pattern.get("seen") or [])

    return {
        # An inference from repeated findings, not something a person saw.
        # The distinction matters when someone later asks where this came from.
        "kind": "Observation",
        "content": (
            f"{name} ({gstin}) has raised {problem} in {periods} separate filing "
            f"periods{f' — {seen}' if seen else ''}. Treat a fresh finding of this "
            f"kind against them as a recurrence rather than a first occurrence, and "
            f"weigh chasing accordingly."
        ),
        "summary": f"{name}: recurring {problem} across {periods} periods",
        "confidence": round(confidence, 4),
        "links": [{"kind": "about", "target": f"urn:gstin:{gstin}"}],
    }


def waiver_request(
    *,
    shape: str,
    focus_node: str,
    constraint: str,
    reason: str,
    expires_at: datetime,
    path: str | None = None,
) -> dict[str, Any]:
    """A `POST /validation/waivers` body.

    Both guards below are enforced here as well as by graph-owl, and that
    duplication is deliberate: the caller gets a useful error at the point of
    the mistake rather than a 400 from a service two hops away.

    # Raises

    `ValueError` if the reason is blank — an exception accepted for no stated
    reason is indistinguishable from a check that was switched off — or if the
    expiry is not in the future, which is how a permanent waiver gets written
    while appearing to have one.
    """
    if not reason.strip():
        raise ValueError("a waiver needs a reason — one without is a disabled check")
    if expires_at <= datetime.now(timezone.utc):
        raise ValueError("a waiver's expiry must be in the future")

    request: dict[str, Any] = {
        # The finding's *identity*, never its row id: results are replaced
        # wholesale each pass and every row gets a fresh id, so a waiver keyed
        # on one would survive until the next run and then point at nothing.
        "shape": shape,
        "focusNode": focus_node,
        "constraint": constraint,
        "reason": reason.strip(),
        "expiresAt": expires_at.astimezone(timezone.utc).isoformat(),
    }
    if path:
        request["path"] = path
    return request


__all__ = [
    "CONFIDENCE_BASE",
    "CONFIDENCE_CEILING",
    "CONFIDENCE_PER_PERIOD",
    "MIN_PERIODS_FOR_A_PATTERN",
    "memory_for_supplier",
    "supplier_pattern",
    "waiver_request",
]
