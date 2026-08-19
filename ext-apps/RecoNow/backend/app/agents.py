"""Agents that actually do something.

**What was here before**: `wake_agents` created a run and recorded its empty
summary. No agent read anything, decided anything, or produced anything — the
activity screen showed that an agent *would have* run.

Each agent below does work a reviewer would otherwise do by hand, and leaves a
trace of **what it looked at before it decided**. That last part is the whole
point of the span model: a trace of model calls alone cannot answer the
question anyone actually asks after a bad outcome.

**The model is never asked for a number or an ordering.** Ranking is judgement
with money attached and must be identical across two runs of identical data —
an ordering that moves is not a ranking, and nobody could defend it to a
client. The model writes prose from figures already computed. That is the split
current practice supports and the one this product's own incident history
demands.
"""

from __future__ import annotations

from typing import Any, Callable

from . import case_narrative, grounding
from .agent_runtime import AgentRun, GrantRevoked, Registry

#: How urgent each finding kind is, and why — the ranking's actual content.
#:
#: **Not by amount.** ₹58,300 of blocked credit is *gone*; ₹1,80,000 under
#: review is a ₹500 disagreement about an invoice both sides carry. Ranking by
#: raw amount puts the wrong one first every time, which is precisely the
#: judgement a reviewer wants made for them.
#:
#: Lower sorts first.
URGENCY: dict[str, tuple[int, str]] = {
    "gst:DuplicateClaim": (0, "credit taken twice — an excess claim, entirely in your own records"),
    "gst:ITCNotAvailable": (1, "credit the law blocks — lost, and no chasing changes it"),
    "gst:ItcTimeBarApproaching": (1, "the window to claim this closes and does not reopen"),
    "gst:PaymentOverdue": (2, "must be reversed now, reclaimable once you pay"),
    "gst:ReverseChargeContradiction": (2, "you and the supplier are treating this differently"),
    "gst:GoodsReceiptTiming": (3, "deferred to the period of receipt, not lost"),
    "gst:SupplierNotFiled": (4, "a phone call — the credit is deferred, not lost"),
    "gst:PotentialMismatch": (4, "not in any 2B, and no GSTR-1 loaded to say why"),
    "gst:AmountMismatch": (5, "only the difference is in doubt, not the invoice"),
    "gst:TaxHeadMismatch": (5, "the total agrees; the split does not"),
}
DEFAULT_URGENCY = (6, "flagged, with no ranking rule for this kind yet")


def _at_stake(case: dict[str, Any]) -> float:
    books = case.get("books_amount")
    portal = case.get("portal_amount")
    if books is not None and portal is not None:
        # A mismatch risks the difference, never the whole invoice.
        return abs(books - portal)
    return float(books if books is not None else portal or 0)


def run_triage(
    *,
    cases: list[dict[str, Any]],
    registry: Registry,
    model: Callable[[str], str] | None,
    context: dict[str, Any] | None = None,
) -> AgentRun:
    """Rank the period's findings so a reviewer knows what to open first."""
    run = AgentRun(registry=registry, agent="triage", event="reconciliation.finished")
    run.context = context or {}

    with run.span("tool", "read_cases") as span:
        span.record(input={"period": run.context.get("period_id")}, output={"count": len(cases)})

    with run.span("decision", "rank_by_recoverability") as span:
        ranked = []
        for case in cases:
            rank, because = URGENCY.get(str(case.get("reason_code")), DEFAULT_URGENCY)
            ranked.append(
                {
                    "case_id": case.get("id"),
                    "invoice_no": case.get("invoice_no"),
                    "rule": case.get("reason_code"),
                    "at_stake": _at_stake(case),
                    "rank": rank,
                    # A ranking with no reasons is an opinion. This one has to
                    # be arguable.
                    "because": because,
                }
            )
        ranked.sort(key=lambda r: (r["rank"], -r["at_stake"]))
        span.record(
            output={"ranked": len(ranked)},
            because="recoverability first, then amount — lost credit outranks a "
            "larger sum that is merely deferred",
        )

    try:
        run.write("propose", {"ranked": ranked})
    except GrantRevoked:
        # Already traced by `write`. The run completed and produced nothing,
        # which is a different outcome from failing and must not read as one.
        pass

    run.finish()
    return run


def run_vendor(
    *,
    cases: list[dict[str, Any]],
    registry: Registry,
    model: Callable[[str], str] | None,
    context: dict[str, Any] | None = None,
) -> AgentRun:
    """Draft the chase message for suppliers who have not filed."""
    run = AgentRun(registry=registry, agent="vendor", event="reconciliation.finished")
    run.context = context or {}

    with run.span("tool", "select_unfiled") as span:
        # **One draft per invoice, however many rules fired on it.** Found in a
        # real run's trace: `SupplierNotFiled` and `PotentialMismatch` both
        # fire on the same invoice, so the agent drafted — and would have sent
        # — the same chase twice. One invoice is one conversation with one
        # supplier.
        unfiled: list[dict[str, Any]] = []
        seen: set[str] = set()
        for case in cases:
            if str(case.get("reason_code")) not in {
                "gst:SupplierNotFiled",
                "gst:PotentialMismatch",
            }:
                continue
            key = str(case.get("invoice_no"))
            if key in seen:
                continue
            seen.add(key)
            unfiled.append(case)
        span.record(output={"count": len(unfiled), "from_cases": len(cases)})

    drafts = []
    for case in unfiled:
        computed = (
            f"Dear {case.get('supplier_name') or 'Sir or Madam'},\n\n"
            f"Invoice {case.get('invoice_no')} carrying "
            f"{case_narrative.rupees(case.get('books_amount'))} of input tax credit "
            "has not appeared in our GSTR-2B. Please confirm the period in which it "
            "was, or will be, declared in your GSTR-1.\n\n"
            "Section 16(2)(aa) makes the credit available to us only once you have "
            "furnished those details."
        )
        draft = {"invoice_no": case.get("invoice_no"), "message": computed, "source": "computed"}

        if model is not None:
            with run.span("model", f"draft:{case.get('invoice_no')}") as span:
                supplied = {
                    "invoice_no": case.get("invoice_no"),
                    "books_amount": case.get("books_amount"),
                    "supplier_name": case.get("supplier_name"),
                    # **The provision the message rests on.** Found in a real
                    # trace: every draft was refused for "states 16, 2" — the
                    # model quoting Section 16(2)(aa), which the computed
                    # template itself contains. A provision reference is not an
                    # invented figure, and refusing it made the agent produce
                    # nothing while appearing to work.
                    "provision": "Section 16(2)(aa) of the CGST Act",
                    "template": computed,
                }
                try:
                    written = model(
                        "Rewrite this supplier chase email to read naturally. Use ONLY "
                        "the figures given; introduce no number.\n\n" + computed
                    )
                except Exception as exc:  # noqa: BLE001
                    span.record(status="failed", error=str(exc))
                    written = None
                if written:
                    checked = grounding.ground_draft(draft=written, supplied=supplied)
                    span.record(grounded=checked["grounded"], output={"chars": len(written)})
                    if checked["grounded"]:
                        draft = {
                            "invoice_no": case.get("invoice_no"),
                            "message": written.strip(),
                            "source": "model",
                        }
                    else:
                        span.record(error=checked["reason"])
        drafts.append(draft)

    try:
        run.write("propose", {"drafts": drafts})
    except GrantRevoked:
        pass

    run.finish()
    return run


AGENTS: dict[str, Callable[..., AgentRun]] = {
    "triage": run_triage,
    "vendor": run_vendor,
}

__all__ = ["AGENTS", "DEFAULT_URGENCY", "URGENCY", "run_triage", "run_vendor"]
