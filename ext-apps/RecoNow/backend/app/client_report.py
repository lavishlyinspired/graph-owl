"""The client report — the LLM use the reference mockup leads with.

**The skeleton is ours; only the prose is the model's.** A model asked for "a
report" produces a different shape every time, and a document a client
receives every month must not be reorganised at random. Section order and the
figures in them are computed here; the model writes the sentences.

**Every figure is supplied and grounded.** A report stating a number the
reconciliation does not carry is refused and the computed version shown
instead — the same rule the case explainer uses. A client report is the worst
place in this product for an invented figure, because it leaves the building.
"""

from __future__ import annotations

from typing import Any

from .case_narrative import rupees

#: Fixed, and in this order. The reference mockup's own skeleton.
SECTIONS = [
    "EXECUTIVE SUMMARY",
    "KEY FINDINGS",
    "ISSUE BREAKDOWN",
    "RISK ASSESSMENT",
    "RECOMMENDED ACTIONS",
    "NEXT STEPS",
]


def build_facts(
    *,
    period: str,
    counts: dict[str, int],
    itc: dict[str, float],
    match_rate: float,
    outcomes: list[dict[str, Any]],
) -> dict[str, Any]:
    """Everything the report may state, computed once.

    `itc_at_risk` is **blocked plus disputed**, matching the reconcile screen's
    headline card exactly. A report using a different definition of the same
    phrase is how two numbers for one thing reach a client.
    """
    issues = sorted(
        (
            {
                "title": o.get("title") or o.get("label"),
                "label": o.get("label"),
                "found": int(o.get("found") or 0),
                "governed_by": o.get("governed_by"),
            }
            # A passed check is not an issue, and listing it pads a report a
            # client is meant to act on.
            for o in outcomes
            if o.get("status") == "flagged"
        ),
        key=lambda i: -i["found"],
    )

    return {
        "period": period,
        "total_invoices": sum(counts.values()),
        "matched": counts.get("matched", 0),
        "review": counts.get("review", 0),
        "only_books": counts.get("only_books", 0),
        "only_portal": counts.get("only_portal", 0),
        "match_rate_pct": round(match_rate * 100, 1),
        "itc_confirmed": itc.get("confirmed", 0.0),
        "itc_pending": itc.get("pending", 0.0),
        "itc_blocked": itc.get("blocked", 0.0),
        "itc_under_review": itc.get("under_review", 0.0),
        "itc_unclaimed": itc.get("unclaimed", 0.0),
        "itc_at_risk": itc.get("blocked", 0.0) + itc.get("under_review", 0.0),
        "issues": issues,
    }


def computed_report(facts: dict[str, Any]) -> str:
    """The report, from the figures alone.

    Stands entirely on its own — this is what a client receives when no model
    is reachable, and it has to be a document rather than a placeholder.
    """
    issue_lines = (
        "\n".join(
            f"- {i['title']}: {i['found']} invoice(s)"
            + (f" ({i['governed_by']})" if i.get("governed_by") else "")
            for i in facts["issues"]
        )
        or "- No statutory check raised a finding this period."
    )

    return f"""EXECUTIVE SUMMARY
This reconciliation covered {facts['total_invoices']} invoices for {facts['period']},
matching {facts['match_rate_pct']}% on the first pass. {rupees(facts['itc_at_risk'])} of input
tax credit needs attention, of which {rupees(facts['itc_blocked'])} is blocked outright.

KEY FINDINGS
- Invoices reconciled: {facts['total_invoices']}
- Matched on both sides: {facts['matched']}
- Values disagree: {facts['review']}
- In your books only: {facts['only_books']}
- On the portal only: {facts['only_portal']}

ISSUE BREAKDOWN
{issue_lines}

RISK ASSESSMENT
{rupees(facts['itc_blocked'])} is blocked and cannot be claimed at all.
{rupees(facts['itc_under_review'])} is disputed — only the difference is in doubt, not the
whole invoice. {rupees(facts['itc_pending'])} is deferred rather than lost: the supplier has
not filed, and it becomes recoverable once they do.

RECOMMENDED ACTIONS
1. Chase the suppliers behind the {facts['only_books']} invoice(s) they have not filed.
2. Settle the {facts['review']} value disagreements before filing.
3. Reverse the blocked credit in this period's return rather than carrying it forward.
4. Record every follow-up, so the position is defensible if it is questioned.

NEXT STEPS
- Complete supplier follow-ups before the filing deadline.
- Re-run the reconciliation once suppliers have corrected their filings.
- Keep the working paper with the return as documentation."""


def groundable(facts: dict[str, Any]) -> dict[str, Any]:
    """What the model may cite.

    **Includes the issue titles and their citations.** Found by generating a
    real report: it was refused for "states 180", from the title "Unpaid past
    180 days" that the computed report itself prints. Excluding lists left
    every statutory constant in them unsupported — a constant stated by a
    provision is supported by the text that states it.
    """
    supplied = {k: v for k, v in facts.items() if not isinstance(v, list)}
    for index, issue in enumerate(facts.get("issues") or []):
        supplied[f"issue_{index}_title"] = issue.get("title")
        supplied[f"issue_{index}_found"] = issue.get("found")
        supplied[f"issue_{index}_citation"] = issue.get("governed_by")
    return supplied


def build_prompt(facts: dict[str, Any], computed: str) -> str:
    return f"""You are writing the monthly GST reconciliation report a chartered accountant sends a client.

Rewrite the report below so it reads as fluent professional prose.

RULES:
- Keep EVERY section heading exactly as written, in the same order.
- Every figure is already computed. Do NOT calculate, derive, round or
  introduce any number that is not written below.
- Keep the distinction between credit that is LOST (blocked) and credit that is
  merely DEFERRED (supplier has not filed). Never merge them.
- No markdown, no preamble.

{computed}"""


__all__ = ["SECTIONS", "build_facts", "build_prompt", "computed_report", "groundable"]
