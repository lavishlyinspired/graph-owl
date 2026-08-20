"""RED tests for the 19 August 2026 functional-audit findings (AUDIT.md).

Each class is one finding; the docstring names what the code did before and
why it was wrong. Kept in one file so the audit's fixes are reviewable as a
set — every one of these failed against the pre-fix code.
"""

from __future__ import annotations

import os

import pytest
from fastapi.testclient import TestClient

from app import agents_graph, main, reconciliation as rc, repo
from app.agent_runtime import Registry
from app.main import app


def _row(status: str, itc: float, tax_diff: float = 0.0, gstin: str = "27AAAFN2938K1Z2",
         supplier: str = "Nimbus", invoice_no: str = "INV-1") -> dict:
    view = {"gstin": gstin, "supplier": supplier, "invoice_no": invoice_no,
            "voucher_no": "", "taxable": 0.0, "tax": itc, "hsn": "", "ims_status": ""}
    return {
        "status": status, "reason": "", "diff": None, "tax_diff": tax_diff,
        "itc": itc, "book": view, "portal": view,
    }


class TestSupplierHealthReportsFactsNotCharacterisations:
    """F1 — supplier_health labelled *every* supplier with any at-risk row
    "Chronic Non-Filer" with `filing_6mo` blank. One ₹500 rounding dispute
    became a fabricated judgement about a third party, printed on the
    dashboard and in exports. A rollup may report what the period showed;
    it may not invent a filing history it never looked at."""

    def test_no_characterisation_is_fabricated_from_one_periods_rows(self):
        rows = [_row(rc.STATUS_ONLY_BOOKS, 500.0)]

        health = rc.supplier_health(rows)

        assert health[0]["gstin"] == "27AAAFN2938K1Z2"
        assert "risk" not in health[0]
        assert "filing_6mo" not in health[0]

    def test_the_rollup_still_reports_what_the_period_actually_showed(self):
        rows = [
            _row(rc.STATUS_ONLY_BOOKS, 500.0, invoice_no="INV-1"),
            _row(rc.STATUS_REVIEW, 0.0, tax_diff=250.0, invoice_no="INV-2"),
        ]

        health = rc.supplier_health(rows)

        assert health[0]["at_risk_invoices"] == 2
        assert health[0]["itc"] == 750.0


class TestUnclaimedCreditIsNotReportedAsZero:
    """F9 — classify_mismatches hardcoded `itc: 0.0` for the Only-in-Portal
    bucket while the row-level data carried the real tax. Credit available
    on the portal that nobody booked is the good news of a reconciliation;
    reporting it as zero understates it to exactly the person who should
    claim it."""

    def test_only_in_portal_carries_the_real_unclaimed_tax(self):
        rows = [_row(rc.STATUS_ONLY_GSTR2B, 8100.0)]

        classes = rc.classify_mismatches(rows)

        portal = next(c for c in classes if c["key"] == "only_in_portal")
        assert portal["itc"] == 8100.0


class TestGrossItcIsNeverCalledNet:
    """F8 — match_stats' `gross_itc` (matched + review + portal-only tax)
    was presented as "Net ITC for GSTR-3B Table 4" in the AI prompts and as
    the claim ceiling in the template report. It is neither net nor a 4C
    figure, and recommending a claim "up to" it tells a preparer to claim
    credit that is under review or not even booked."""

    STATS = {"total": 4, "matched": 2, "review": 1, "only_books": 1,
             "only_gstr2b": 0, "match_rate": 50.0, "confirmed_itc": 36000.0,
             "at_risk_itc": 9000.0, "gross_itc": 58500.0}

    def test_template_report_recommends_claiming_confirmed_itc_not_gross(self):
        report = main._template_report(self.STATS, [], {"month": "March", "year": 2026})
        assert "₹36,000" in report
        assert "₹58,500" not in report

    def test_fallback_summary_never_labels_gross_as_net(self):
        client = TestClient(app)
        client.post("/api/reset")
        main.SESSION["results"] = []
        # Build a summary straight from the fallback branch's own inputs.
        stats = self.STATS
        text = main._fallback_summary(stats, [])
        assert "58,500" not in text or "gross" in text.lower()


class TestLegacyReconcileIsScopedToWhatWasUploaded:
    """F2 — `_run_graphowl_reconcile` called `run_findings` without the
    `graphs` scope that `reconcile_route` passes, so on the legacy SESSION
    path the native engine read the *whole store* — another client's data
    could satisfy or trigger a rule about this session's files."""

    def test_run_findings_is_called_with_this_sessions_graphs(self, monkeypatch):
        captured: dict = {}

        def fake_run_findings(pack_id, server, token=None, graphs=None):
            captured["graphs"] = graphs

            class Result:
                evaluated, found, opened, already_open = 0, 0, 0, 0
            return Result()

        monkeypatch.setattr(main, "run_findings", fake_run_findings)
        monkeypatch.setattr(main.graphowl_client, "list_findings", lambda *a, **k: [])
        main.SESSION["datasets"] = {"books": {}, "gstr2b": {}}

        main._run_graphowl_reconcile()

        assert set(captured["graphs"]) == {
            "reco-books", "reco-gstr2b",
            "gst-ontology", "gst-law", "gst-law-rule-36-4",
        }


class TestSubscriptionsSayWhatIsNotBuilt:
    """F3 — seven of the ten subscribed agents have no implementation; the
    subscriptions screen listed them indistinguishably from the real ones,
    advertising a fleet that is 70% aspirational."""

    def test_unimplemented_agents_are_marked(self):
        client = TestClient(app)
        body = client.get("/api/agents/subscriptions").json()

        by_agent = {s["agent"]: s for s in body["subscriptions"]}
        assert by_agent["triage"]["implemented"] is True
        assert by_agent["explainer"]["implemented"] is False
        assert by_agent["drift"]["implemented"] is False


class TestRiskAgentNeverInterpolatesAMalformedGstin:
    """F5 — SUPPLIER_HISTORY string-interpolated a supplier GSTIN into
    SPARQL. GSTIN shape is only *warned* on at ingestion, so a value
    containing a quote reached the query intact."""

    def test_a_malformed_gstin_is_never_put_in_a_query(self):
        asked: list[str] = []

        def fake_mcp(tool, args):
            asked.append(args.get("query", ""))
            return {"rows": []}

        registry = Registry()
        registry.grant("risk", "propose")
        run = agents_graph.run_supplier_risk(
            cases=[{"supplier_gstin": '27AAAFN2938K1Z2" . } injected { "',
                    "supplier_name": "Injector", "reason_code": "gst:SupplierNotFiled",
                    "invoice_no": "INV-9"}],
            registry=registry,
            mcp=fake_mcp,
            model=None,
        )

        assessment = run.writes[0]["payload"]["suppliers"][0]
        assert assessment["checked"] is False
        assert not any("injected" in query for query in asked)


class TestOptionalBearerAuth:
    """F6 — every route trusted the client_id in the URL. Row-level
    isolation was tested; identity was absent. `RECONOW_API_TOKEN` now gates
    every /api route when set; unset keeps the single-firm desktop default
    open."""

    def test_requests_without_the_token_are_refused_when_one_is_configured(self, monkeypatch):
        monkeypatch.setenv("RECONOW_API_TOKEN", "test-token")
        client = TestClient(app)

        assert client.get("/api/clients").status_code == 401
        assert client.get("/api/clients", headers={
            "authorization": "Bearer wrong"}).status_code == 401
        assert client.get("/api/clients", headers={
            "authorization": "Bearer test-token"}).status_code != 401

    def test_health_stays_open_for_load_balancers(self, monkeypatch):
        monkeypatch.setenv("RECONOW_API_TOKEN", "test-token")
        client = TestClient(app)

        assert client.get("/api/health").status_code == 200


class TestAgentRunsSurviveTheProcess:
    """F4 — agent runs, grants and refusals lived in module globals, capped
    and dropped on restart. The trace is the product's audit trail; it now
    persists beside the cases it was produced from."""

    async def test_a_run_recorded_is_read_back(self, pool):
        record = {
            "id": "run-test-0001", "agent": "triage",
            "event": "reconciliation.finished", "status": "completed",
            "error": None, "ms": 12, "span_counts": {"tool": 1, "decision": 1},
            "writes": 1, "refusals": 0, "tokens": None, "cost": None,
            "context": {"period_id": "p1"},
        }
        async with pool.acquire() as conn:
            await repo.insert_agent_run(conn, record=record)
            runs = await repo.list_agent_runs(conn)

        assert [r["id"] for r in runs] == ["run-test-0001"]
        assert runs[0]["span_counts"] == {"tool": 1, "decision": 1}
        assert runs[0]["tokens"] is None
