"""An agent that genuinely needs the graph, and therefore uses MCP.

**The gap I flagged twice and did not close.** graph-owl exposes 22 MCP tools —
`query_graph`, `traverse`, `explain_lineage`, `recall_memory`, `find_evidence`,
`analytics`, `resolve_entity`, `calculate_risk` — and neither agent used any of
them. Triage and Vendor work from the case rows they already have, so for them
MCP would have been ceremony.

**Supplier Risk is the one that cannot work without it.** "Is this supplier a
repeat offender" is a question about *other periods*, and the case rows for
this period cannot answer it. It needs the graph: what this supplier has done
before, and whether anyone recorded a judgement about them.

Two MCP calls, and the failure of either degrades rather than breaks:
`recall_memory` for a judgement someone already wrote down, `query_graph` for
what the graph holds across periods. An agent that cannot reach the graph
should say what it could not check, not report a clean supplier.
"""

from __future__ import annotations

from app.agent_runtime import Registry, Subscription
from app.mcp_client import McpError
from app.agents_graph import run_supplier_risk


def _registry() -> Registry:
    registry = Registry()
    registry.subscribe(Subscription(agent="risk", event="reconciliation.finished"))
    registry.grant("risk", "propose")
    return registry


CASES = [
    {"id": "1", "invoice_no": "INV-1", "reason_code": "gst:SupplierNotFiled",
     "supplier_gstin": "29AACCG0527D1Z8", "supplier_name": "Phantom", "books_amount": 8640.0},
]


class TestItActuallyCallsMcp:
    def test_it_recalls_what_was_already_known_about_the_supplier(self):
        calls: list[str] = []

        def mcp(tool: str, _args: dict) -> dict:
            calls.append(tool)
            return {"memories": []} if tool == "recall_memory" else {"rows": []}

        run_supplier_risk(cases=CASES, registry=_registry(), mcp=mcp, model=None)

        assert "recall_memory" in calls

    def test_it_asks_the_graph_what_it_holds_across_periods(self):
        calls: list[str] = []

        def mcp(tool: str, _args: dict) -> dict:
            calls.append(tool)
            return {"rows": []}

        run_supplier_risk(cases=CASES, registry=_registry(), mcp=mcp, model=None)

        assert "query_graph" in calls

    def test_every_mcp_call_is_a_tool_span_a_reader_can_inspect(self):
        """The trace has to show what it asked the graph, or "the agent decided
        X" is unarguable."""
        run = run_supplier_risk(
            cases=CASES, registry=_registry(), mcp=lambda t, a: {"rows": []}, model=None
        )

        tool_spans = [s for s in run.spans if s["kind"] == "tool"]
        assert any(s["name"].startswith("mcp:") for s in tool_spans)

    def test_the_span_records_what_was_asked_and_what_came_back(self):
        run = run_supplier_risk(
            cases=CASES,
            registry=_registry(),
            mcp=lambda t, a: {"rows": [{"period": "2026-01"}, {"period": "2026-02"}]},
            model=None,
        )

        span = next(s for s in run.spans if s["name"] == "mcp:query_graph")
        assert span["input"]
        assert span["output"]


class TestWhenTheGraphCannotBeReached:
    def test_a_failed_call_does_not_fail_the_run(self):
        """A graph that is down costs a check, not the agent."""
        def mcp(_t: str, _a: dict) -> dict:
            raise McpError("unreachable")

        run = run_supplier_risk(cases=CASES, registry=_registry(), mcp=mcp, model=None)

        assert run.summary()["status"] == "completed"

    def test_it_says_what_it_could_not_check_rather_than_reporting_clean(self):
        """**The dangerous direction.** An agent that cannot reach the graph
        and reports a clean supplier has told a reviewer something it never
        established."""
        def mcp(_t: str, _a: dict) -> dict:
            raise McpError("unreachable")

        run = run_supplier_risk(cases=CASES, registry=_registry(), mcp=mcp, model=None)

        assessment = run.writes[0]["payload"]["suppliers"][0]
        assert assessment["checked"] is False
        assert assessment["unchecked_because"]

    def test_the_failed_span_is_recorded_as_failed(self):
        def mcp(_t: str, _a: dict) -> dict:
            raise McpError("unreachable")

        run = run_supplier_risk(cases=CASES, registry=_registry(), mcp=mcp, model=None)

        assert any(s["status"] == "failed" for s in run.spans)


class TestTheAssessment:
    def test_a_supplier_seen_across_periods_is_marked_as_recurring(self):
        run = run_supplier_risk(
            cases=CASES,
            registry=_registry(),
            mcp=lambda t, a: {"rows": [{"period": "2026-01"}, {"period": "2026-02"}, {"period": "2026-03"}]},
            model=None,
        )

        assessment = run.writes[0]["payload"]["suppliers"][0]
        assert assessment["recurring"] is True
        assert assessment["periods"] == 3

    def test_a_supplier_seen_once_is_not_marked_recurring(self):
        run = run_supplier_risk(
            cases=CASES,
            registry=_registry(),
            mcp=lambda t, a: {"rows": [{"period": "2026-03"}]},
            model=None,
        )

        assert run.writes[0]["payload"]["suppliers"][0]["recurring"] is False

    def test_one_assessment_per_supplier_not_per_invoice(self):
        """A supplier with four unfiled invoices is one conversation."""
        many = [dict(CASES[0], id=str(i), invoice_no=f"INV-{i}") for i in range(4)]

        run = run_supplier_risk(
            cases=many, registry=_registry(), mcp=lambda t, a: {"rows": []}, model=None
        )

        assert len(run.writes[0]["payload"]["suppliers"]) == 1
