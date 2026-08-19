"""The agent that cannot work without the graph — Plan 123 §5.

Triage and Vendor work from the case rows they already have, so for them MCP
would have been ceremony. **Supplier Risk is different**: "is this supplier a
repeat offender" is a question about *other periods*, and this period's rows
cannot answer it. It needs what the graph holds across all of them, and what
anyone has already recorded about the supplier.

Two MCP calls, and **the failure of either degrades rather than breaks**:
`recall_memory` for a judgement someone already wrote down, `query_graph` for
what the graph holds across periods. An agent that cannot reach the graph must
say what it could not check — reporting a clean supplier on the strength of a
failed call is the dangerous direction, and the one a reviewer would act on.
"""

from __future__ import annotations

from typing import Any, Callable

from .agent_runtime import AgentRun, GrantRevoked, Registry
from .mcp_client import McpError

#: How many distinct periods make a supplier's problem a pattern rather than an
#: incident. Same threshold and same reasoning as `capabilities.supplier_pattern`
#: — a quarter is the shortest span over which "always" is defensible, and
#: stating it twice with two values would be worse than stating it twice.
RECURRING_PERIODS = 3

#: What to ask the graph about one supplier. Every finding it has ever been
#: named in, with the period, so recurrence is counted rather than guessed.
SUPPLIER_HISTORY = """PREFIX gst: <https://graph-owl.dev/packs/gst#>
SELECT ?period WHERE {
  GRAPH ?g {
    ?supplier gst:supplierGstin "%s" .
    ?invoice gst:issuedBy ?supplier ;
             gst:belongsToPeriod ?filingPeriod .
    ?filingPeriod gst:period ?period .
  }
}"""


def run_supplier_risk(
    *,
    cases: list[dict[str, Any]],
    registry: Registry,
    mcp: Callable[[str, dict], dict],
    model: Callable[[str], str] | None,
    context: dict[str, Any] | None = None,
) -> AgentRun:
    """Which suppliers are repeat offenders, judged against the whole graph."""
    run = AgentRun(registry=registry, agent="risk", event="reconciliation.finished")
    run.context = context or {}

    with run.span("tool", "group_by_supplier") as span:
        # One assessment per supplier, not per invoice: a supplier with four
        # unfiled invoices is one conversation.
        by_gstin: dict[str, dict[str, Any]] = {}
        for case in cases:
            gstin = case.get("supplier_gstin")
            if not gstin:
                continue
            by_gstin.setdefault(
                str(gstin),
                {"gstin": gstin, "name": case.get("supplier_name"), "invoices": 0},
            )["invoices"] += 1
        span.record(output={"suppliers": len(by_gstin)})

    assessments = []
    for gstin, supplier in by_gstin.items():
        periods: set[str] = set()
        memories: list[Any] = []
        unchecked: str | None = None

        try:
            with run.span("tool", "mcp:recall_memory") as span:
                span.record(input={"about": f"urn:gstin:{gstin}"})
                recalled = mcp("recall_memory", {"about": f"urn:gstin:{gstin}", "limit": 5})
                memories = recalled.get("memories") or []
                span.record(output={"memories": len(memories)})
        except McpError as exc:
            unchecked = f"could not recall prior judgements: {exc}"

        try:
            with run.span("tool", "mcp:query_graph") as span:
                span.record(input={"gstin": gstin})
                answer = mcp("query_graph", {"query": SUPPLIER_HISTORY % gstin})
                rows = answer.get("rows") or []
                periods = {str(r.get("period")) for r in rows if r.get("period")}
                span.record(output={"rows": len(rows), "periods": len(periods)})
        except McpError as exc:
            unchecked = f"could not read this supplier's history: {exc}"

        assessments.append(
            {
                "gstin": gstin,
                "name": supplier["name"],
                "invoices_this_period": supplier["invoices"],
                "periods": len(periods),
                "recurring": len(periods) >= RECURRING_PERIODS,
                "prior_judgements": len(memories),
                # **Said out loud.** A supplier the agent could not check must
                # never read the same as one it checked and cleared.
                "checked": unchecked is None,
                "unchecked_because": unchecked,
            }
        )

    with run.span("decision", "assess_recurrence") as span:
        span.record(
            output={"assessed": len(assessments)},
            because=f"a supplier named in {RECURRING_PERIODS} or more distinct periods is a "
            "pattern; fewer is an incident",
        )

    try:
        run.write("propose", {"suppliers": assessments})
    except GrantRevoked:
        pass

    run.finish()
    return run


__all__ = ["RECURRING_PERIODS", "SUPPLIER_HISTORY", "run_supplier_risk"]
