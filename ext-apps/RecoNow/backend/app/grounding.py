"""An agent may only state a number that appears in a fact it cites.

**Plan 123 §5 calls this "the rule that makes this safe (unchanged, and
load-bearing)", and it is the only safety property in this product that is
mechanical rather than aspirational.**

This console has already shipped a fabricated "₹8.2 L sits inside the s.16(4)
window" once. An LLM will do that by default, confidently, and about a tax
position — the worst possible subject for a confident invention, because the
reader has no way to tell it apart from the figures the reconciliation
actually produced.

The rule is not "the model should try to be accurate":

1. every claim carries the ids of the facts supporting it;
2. every **number** in the claim's text must appear in one of *those* facts —
   cited, not merely present somewhere in the database;
3. a claim that fails is **rejected before render**, and the rejection is
   recorded.

**Numbers only, deliberately.** "The supplier has not filed" is a qualitative
statement the surrounding finding already supports. Demanding a fact id for
every sentence would make the rule unusable, and an unusable safety rule gets
switched off — which is worse than a narrow one that stays on.

**Refusals are logged rather than silently dropped.** For an agentic product
the record of what an agent tried and was refused is worth more than the
record of what it produced: a refusal nobody counts is a safety property
nobody can audit.
"""

from __future__ import annotations

import re
from typing import Any

#: A number a claim asserts. Requires a digit-led run, optionally with Indian
#: or Western grouping and a decimal part.
#:
#: **Bounded by non-alphanumerics on both sides on purpose.** `INV-2026-001`
#: contains digits that are part of an identifier rather than a claim about an
#: amount, and treating them as free numbers would refuse every claim that
#: names an invoice — which is most of them.
_NUMBER = re.compile(r"(?<![0-9A-Za-z\-/])(\d[\d,]*(?:\.\d+)?)(?![0-9A-Za-z\-/])")


class GroundingError(ValueError):
    """A claim stated a figure no cited fact supports."""


def _canonical(raw: str) -> str:
    """`1,80,000` and `180000` are the same amount; so are `45000.00` and
    `45000`. A model writes one form and the graph holds another, and a rule
    that rejected true statements over formatting would be switched off."""
    cleaned = raw.replace(",", "")
    if "." in cleaned:
        cleaned = cleaned.rstrip("0").rstrip(".")
    return cleaned or "0"


def numbers_in(text: str) -> set[str]:
    """Every figure `text` asserts, in canonical form."""
    return {_canonical(match.group(1)) for match in _NUMBER.finditer(text)}


def check_claim(*, text: str, fact_ids: list[str], facts: dict[str, Any]) -> None:
    """Verify every figure in `text` appears in one of the cited facts.

    # Raises

    `GroundingError` naming the first unsupported figure, an unknown fact id,
    or the absence of any citation for a claim that states a figure.
    """
    claimed = numbers_in(text)
    if not claimed:
        # A qualitative statement. The surrounding finding is its support.
        return

    unknown = [fid for fid in fact_ids if fid not in facts]
    if unknown:
        # A citation to nothing is worse than no citation: it looks like
        # support and survives a casual review.
        raise GroundingError(f"cites unknown fact(s): {', '.join(sorted(unknown))}")

    if not fact_ids:
        raise GroundingError(
            f"states {', '.join(sorted(claimed))} with no facts cited"
        )

    supported = set()
    for fid in fact_ids:
        supported |= numbers_in(str(facts[fid].get("value", "")))

    unsupported = sorted(claimed - supported)
    if unsupported:
        raise GroundingError(
            f"states {', '.join(unsupported)}, which no cited fact carries"
        )


def render_claim(
    *,
    text: str,
    fact_ids: list[str],
    facts: dict[str, Any],
    log: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """A claim, or a refusal — never the claim's own text when it fails.

    **The refused text is not echoed back.** A refusal that repeats the
    fabricated figure has published it anyway: a reader skims, sees ₹82,00,000
    beside the word "unsupported", and remembers the number.
    """
    try:
        check_claim(text=text, fact_ids=fact_ids, facts=facts)
    except GroundingError as exc:
        if log is not None:
            log.append({"refused": True, "reason": str(exc), "fact_ids": list(fact_ids)})
        return {
            "grounded": False,
            "text": "Not enough evidence to state this.",
            "reason": str(exc),
        }
    return {"grounded": True, "text": text, "reason": None}


def ground_draft(*, draft: str, supplied: dict[str, Any], log: list | None = None) -> dict[str, Any]:
    """Check a model's draft against the figures it was actually given.

    **The control the system prompt only asks for.** `ai.draft_follow_up`
    already tells the model "no invented figures"; that is a request. This
    checks. The values passed *into* the prompt are the only ones the draft
    may state — which is exactly the fact-citation rule with the prompt's own
    inputs standing in for the cited facts.

    An identifier among the supplied values (an invoice number) contributes no
    figures, and the draft may name it freely: `numbers_in` already refuses to
    read digits inside an identifier as an amount.
    """
    facts = {
        str(key): {"value": str(value)} for key, value in supplied.items() if value is not None
    }
    return render_claim(text=draft, fact_ids=list(facts), facts=facts, log=log)


__all__ = [
    "GroundingError",
    "check_claim",
    "ground_draft",
    "numbers_in",
    "render_claim",
]
