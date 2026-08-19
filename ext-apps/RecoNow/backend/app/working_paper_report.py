"""The GSTR-3B working paper as a document a CA can hand over.

A table of five figures answers "what is the number". A working paper has to
answer **"how did you get there, and what did you leave out"** — which is the
question a partner or an officer actually asks.

**The skeleton is computed; the prose may be generated.** Same split as the
client report: section order and every figure come from the chain, and a model
may only rewrite sentences. A working paper is filed evidence, and a figure
invented in one is worse than having no working paper at all.

**What was deliberately not deducted is part of the document.** A reader who
cannot see what was excluded cannot tell a complete paper from a partial one,
and the net figure looks equally confident either way.
"""

from __future__ import annotations

import re
from typing import Any

from .case_narrative import rupees

DIRECTION_LINE = {
    "excess": "The return claimed MORE than this paper supports. An excess claim carries "
    "interest under s.50 and a demand under s.73/74.",
    "unclaimed": "The return claimed LESS than this paper supports. The difference is "
    "recoverable until s.16(4) closes the window for this period.",
    "agrees": "The return's Table 4A agrees with the GSTR-2B it is auto-populated from.",
    "not_evaluated": "No GSTR-3B was supplied for this period, so nothing is being asserted "
    "about what was filed.",
}


def build_report(paper: dict[str, Any], *, period: str) -> str:
    """The working paper, written out."""
    by_key = {line["key"]: line for line in paper["lines"]}
    deductions = [line for line in paper["lines"] if line["kind"] == "deduction"]
    gross = by_key["gross"]["amount"]
    net = by_key["net"]["amount"]
    total_deducted = sum(line["amount"] for line in deductions)

    chain = "\n".join(
        f"  less  {line['label']:<44}{rupees(line['amount']):>14}"
        + (f"\n        {line['citation']}" if line.get("citation") else "")
        + (
            f"\n        NOTE: {line['unquantified']} finding(s) on this line could not be "
            "sized, so this deduction is understated."
            if line.get("unquantified")
            else ""
        )
        for line in deductions
    )

    excluded = (
        "\n".join(
            f"  - {entry['label']}: {rupees(entry['amount'])}"
            for entry in paper.get("unmodelled") or []
        )
        or "  None — every finding this period maps to a line above."
    )

    filed = paper.get("filed") or {}
    direction = str(filed.get("direction") or "not_evaluated")
    filed_block = DIRECTION_LINE.get(direction, DIRECTION_LINE["not_evaluated"])
    if filed.get("difference"):
        filed_block += f" Difference: {rupees(filed['difference'])}."
    if filed.get("arithmetic_ok") is False:
        filed_block += (
            " The filed return also fails its own arithmetic — Table 4C is not 4A less 4B — "
            "so every figure downstream of it is unreliable."
        )

    completeness = (
        ""
        if paper.get("complete", True)
        else "\n\nIMPORTANT\nAt least one deduction was found but could not be sized, so the net "
        "figure below is an UPPER BOUND on what is claimable, not the answer."
    )

    return f"""GSTR-3B WORKING PAPER — {period}

Every figure below is read from the period's own data. Each deduction names the
provision that requires it.{completeness}

TABLE 4 BUILD-UP
        {by_key['gross']['label']:<44}{rupees(gross):>14}
        {by_key['gross']['source']}

{chain}
        {'-' * 58}
        {by_key['net']['label']:<44}{rupees(net):>14}

  Check: {rupees(gross)} less {rupees(total_deducted)} of deductions = {rupees(net)}.

FINDINGS NOT DEDUCTED ABOVE
These are NOT subtracted. Deducting them would attribute a reduction to a
provision nobody chose; dropping them silently would make the net figure
overstate what is claimable. They are listed so the decision stays with you.
{excluded}

AGAINST THE FILED RETURN
{filed_block}
"""


def downloadable(paper: dict[str, Any], *, period: str) -> tuple[str, str]:
    """`(filename, body)`.

    The filename names the period: a file called `report.txt` in a folder of
    thirty is one nobody can find next March.
    """
    safe = re.sub(r"[^A-Za-z0-9]+", "-", period).strip("-").lower() or "period"
    return f"gstr3b-working-paper-{safe}.txt", build_report(paper, period=period)


def build_prompt(report: str) -> str:
    return f"""You are writing up a GSTR-3B working paper for a chartered accountant's file.

Rewrite the document below so it reads as professional prose a partner or a tax
officer would accept.

RULES:
- Keep EVERY section heading and the Table 4 build-up exactly as laid out.
- Every figure is already computed. Do NOT calculate, derive, round or
  introduce any number not written below.
- Keep every provision reference exactly as written.
- Do not remove the "findings not deducted" section, however short it is.
- No markdown, no preamble.

{report}"""


__all__ = ["DIRECTION_LINE", "build_prompt", "build_report", "downloadable"]
