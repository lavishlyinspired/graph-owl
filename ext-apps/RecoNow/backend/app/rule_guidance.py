"""Turning a rule's IRI into something a business reader can act on.

**`gst:AmountMismatch` means nothing to a business user.** It is a label a rule
author chose. A reviewer needs to know what is wrong, why it matters, and what
to do about it.

**The information already existed and nothing asked for it.** `packs/gst`
carries `[findings.guidance]` per rule — `title`, `meaning`, `next_action`,
`tone` — and graph-owl serves it at `GET /packs/{pack}/console`. Reco Now
rendered the IRI instead.

**It stays in the pack rather than in this file.** A healthcare or banking pack
names entirely different findings, and guidance compiled into the consumer is
guidance that only ever fits one domain. What lives here is the *fallback* —
how to be readable about a rule nobody has written guidance for yet, which is
a presentation concern rather than a domain one.
"""

from __future__ import annotations

import re
from typing import Any

#: Split before a capital that starts a new word, keeping runs of capitals
#: together so `ITCNotAvailable` does not become `I T C Not Available`.
_WORD_BOUNDARY = re.compile(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])")


def fallback_title(label: str) -> str:
    """A readable phrase for a rule with no authored guidance.

    Never as good as a title someone wrote, and much better than showing an
    IRI. The first word keeps its capitalisation — an acronym must survive —
    and the rest are lowered, so `ITCNotAvailable` reads "ITC not available"
    rather than "ITC Not Available", which looks like a heading rather than a
    sentence.
    """
    local = str(label).split(":")[-1]
    words = [w for w in _WORD_BOUNDARY.split(local) if w]
    if not words:
        return str(label)
    return " ".join([words[0]] + [w.lower() for w in words[1:]])


def decorate(rows: list[dict[str, Any]], guidance: dict[str, Any]) -> list[dict[str, Any]]:
    """Add `title`, `meaning`, `next_action` and `tone` to each row.

    Returns new dicts rather than mutating: these rows come from the database
    layer and are reused, and mutating in place makes the same object mean
    different things depending on which screen touched it first.

    The **raw label is kept alongside** the title. A CA defending a position
    needs the rule's actual identifier, and so does anyone reading a log —
    replacing it would trade one audience's problem for another's.
    """
    decorated = []
    for row in rows:
        label = row.get("label") or row.get("reason_code")
        if not label:
            decorated.append(dict(row))
            continue
        entry = guidance.get(str(label)) or {}
        decorated.append(
            {
                **row,
                "title": entry.get("title") or fallback_title(label),
                "meaning": entry.get("meaning"),
                "next_action": entry.get("next_action"),
                "tone": entry.get("tone"),
            }
        )
    return decorated


__all__ = ["decorate", "fallback_title"]
