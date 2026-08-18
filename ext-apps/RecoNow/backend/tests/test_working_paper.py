"""The GSTR-3B working paper — Plan 123 Slice D.

The plan's own words for what this is: *"gross → §17(5) → Rule 42/43 → net
Table 4, every figure traced"*, and §4 calls it **the deliverable**.

**A working paper is not a dashboard.** A dashboard shows a number; a working
paper shows how the number was reached, so that someone else — a partner
reviewing, or an officer asking — can follow each step and disagree with a
specific one. Every line therefore carries its own source and, where the
figure came from a statutory reversal, the provision that required it.

The chain it has to show:

    gross ITC available (from GSTR-2B)
      − s.17(5) blocked
      − Rule 42/43 proportionate reversal
      − Rule 37 (180-day) reversal
      = net ITC claimable

and then, separately, what the **filed** GSTR-3B actually said — because the
gap between the two is the exposure.
"""

from __future__ import annotations

import os
import uuid
from decimal import Decimal

import asyncpg
import pytest
from fastapi.testclient import TestClient

from app.working_paper import build_working_paper

ADMIN_DSN = os.environ.get(
    "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
)


def _line(tax: str, **kw) -> dict:
    row = {"tax_amount": Decimal(tax), "invoice_no": kw.pop("invoice_no", "INV-1")}
    row.update(kw)
    return row


class TestTheChainIsTraceable:
    def test_every_line_names_where_its_figure_came_from(self):
        """The property that makes it a working paper rather than a summary.
        A figure with no source cannot be checked by the person reviewing it."""
        paper = build_working_paper(
            gstr2b=[_line("100000")], findings=[], gstr3b=None
        )

        for line in paper["lines"]:
            assert line["source"], f"{line['label']} has no source"

    def test_the_gross_figure_is_the_2b_total(self):
        paper = build_working_paper(
            gstr2b=[_line("100000"), _line("45000", invoice_no="INV-2")],
            findings=[],
            gstr3b=None,
        )

        gross = next(l for l in paper["lines"] if l["key"] == "gross")
        assert gross["amount"] == Decimal("145000")
        assert "GSTR-2B" in gross["source"]

    def test_a_blocked_credit_is_deducted_and_cites_its_provision(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:ITCNotAvailable", "tax_amount": Decimal("18000")}],
            gstr3b=None,
        )

        blocked = next(l for l in paper["lines"] if l["key"] == "blocked_17_5")
        assert blocked["amount"] == Decimal("18000")
        assert "17(5)" in blocked["citation"]

    def test_an_overdue_payment_is_deducted_and_cites_rule_37(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:PaymentOverdue", "tax_amount": Decimal("9000")}],
            gstr3b=None,
        )

        overdue = next(l for l in paper["lines"] if l["key"] == "reversal_rule_37")
        assert overdue["amount"] == Decimal("9000")
        assert "37" in overdue["citation"]

    def test_the_net_figure_is_gross_minus_every_deduction(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[
                {"label": "gst:ITCNotAvailable", "tax_amount": Decimal("18000")},
                {"label": "gst:PaymentOverdue", "tax_amount": Decimal("9000")},
            ],
            gstr3b=None,
        )

        net = next(l for l in paper["lines"] if l["key"] == "net")
        assert net["amount"] == Decimal("73000")

    def test_the_arithmetic_of_the_chain_is_self_checking(self):
        """Gross minus the deductions must equal net, computed independently
        of how net was derived. A working paper that does not add up is worse
        than none — it looks checked."""
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:ITCNotAvailable", "tax_amount": Decimal("18000")}],
            gstr3b=None,
        )

        by_key = {l["key"]: l for l in paper["lines"]}
        deductions = sum(
            l["amount"] for l in paper["lines"] if l["kind"] == "deduction"
        )
        assert by_key["gross"]["amount"] - deductions == by_key["net"]["amount"]

    def test_a_finding_type_the_paper_does_not_model_is_not_silently_dropped(self):
        """A deduction the chain has no line for would otherwise vanish, and
        net would overstate what is claimable."""
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:SomethingNew", "tax_amount": Decimal("5000")}],
            gstr3b=None,
        )

        assert paper["unmodelled"], "an unmodelled finding must be surfaced"
        assert paper["unmodelled"][0]["label"] == "gst:SomethingNew"


class TestAnAmountNobodyEstablished:
    """**Found 19 August 2026 by running the endpoint against real data.**
    `gst:PaymentOverdue` was flagged for the period and deducted zero, because
    that rule's evidence carries `atTime` bindings and no tax amount at all —
    so the case's `books_amount` was `None` and the chain coerced it to zero.

    A deduction of zero and a deduction whose amount nobody established are
    different claims, and the difference is the whole discipline of this
    product. Silently zeroing it makes the net figure **overstate** what is
    claimable, which is the direction that costs money.
    """

    def test_a_finding_with_no_amount_is_not_deducted_as_zero(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:PaymentOverdue", "tax_amount": None}],
            gstr3b=None,
        )

        overdue = next(l for l in paper["lines"] if l["key"] == "reversal_rule_37")
        assert overdue["unquantified"] == 1
        assert paper["complete"] is False

    def test_a_chain_with_every_amount_established_reports_itself_complete(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:PaymentOverdue", "tax_amount": Decimal("9000")}],
            gstr3b=None,
        )

        assert paper["complete"] is True

    def test_the_net_figure_is_still_produced_alongside_the_warning(self):
        """A working paper with one unquantified line is still worth having —
        it is the best available position. It just must not claim to be the
        final one."""
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[{"label": "gst:PaymentOverdue", "tax_amount": None}],
            gstr3b=None,
        )

        net = next(l for l in paper["lines"] if l["key"] == "net")
        assert net["amount"] == Decimal("100000")

    def test_goods_not_yet_received_is_a_deduction_not_an_unmodelled_finding(self):
        """s.16(2)(b) — no credit before the goods arrive. Found in the same
        live run: it was landing in `unmodelled`, which is where a genuine
        statutory deduction must never sit."""
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[
                {"label": "gst:GoodsReceiptTiming", "tax_amount": Decimal("12000")}
            ],
            gstr3b=None,
        )

        line = next(l for l in paper["lines"] if l["key"] == "timing_16_2_b")
        assert line["amount"] == Decimal("12000")
        assert "16(2)(b)" in line["citation"]
        assert paper["unmodelled"] == []


class TestAgainstTheFiledReturn:
    def test_the_filed_figure_is_shown_beside_the_computed_one(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[],
            gstr3b={"itc_4a": Decimal("100000"), "itc_net_4c": Decimal("100000")},
        )

        assert paper["filed"]["gross_claimed"] == Decimal("100000")
        assert paper["filed"]["direction"] == "agrees"

    def test_with_no_filed_return_the_comparison_is_not_evaluated(self):
        """A working paper is still useful without a 3B — it is what you build
        *before* filing. It must not report the absence as agreement."""
        paper = build_working_paper(gstr2b=[_line("100000")], findings=[], gstr3b=None)

        assert paper["filed"]["direction"] == "not_evaluated"

    def test_a_claim_above_what_the_paper_supports_is_visible_as_excess(self):
        paper = build_working_paper(
            gstr2b=[_line("100000")],
            findings=[],
            gstr3b={"itc_4a": Decimal("130000"), "itc_net_4c": Decimal("130000")},
        )

        assert paper["filed"]["direction"] == "excess"
        assert paper["filed"]["difference"] == Decimal("30000")


@pytest.fixture
async def client():
    """The same real-database fixture `test_client_routes.py` uses — a fresh
    database per test, with `DATABASE_URL` set before `app.main` is imported
    because that module reads the env var at startup, not at import."""
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin_conn = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin_conn.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin_conn.close()

    os.environ["DATABASE_URL"] = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as test_client:
            yield test_client
    finally:
        del os.environ["DATABASE_URL"]
        admin_conn = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin_conn.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin_conn.close()


def _period(client) -> tuple[str, str]:
    created = client.post(
        "/api/clients",
        json={"name": "WP Co", "gstin": "27AABCU9603R1ZM", "state": "Maharashtra"},
    ).json()
    period = client.post(
        f"/api/clients/{created['id']}/periods", json={"month": "March", "year": 2026}
    ).json()
    return created["id"], period["id"]


class TestTheEndpoint:
    """The route, against a real database — the layer the pure tests above
    cannot reach. Its own risk is different: not the arithmetic, but whether
    the figures it feeds the computation are the ones the rest of the product
    already counts."""

    def test_a_period_with_no_data_returns_a_zero_chain_not_an_error(self, client):
        client_id, period_id = _period(client)

        response = client.get(
            f"/api/clients/{client_id}/periods/{period_id}/working-paper"
        )

        assert response.status_code == 200, response.text
        paper = response.json()
        by_key = {line["key"]: line for line in paper["lines"]}
        assert by_key["gross"]["amount"] == 0
        assert by_key["net"]["amount"] == 0
        # No 3B was uploaded, so the comparison must say so rather than
        # reporting agreement between two zeroes.
        assert paper["filed"]["direction"] == "not_evaluated"

    def test_every_amount_serializes_as_a_number_not_a_decimal_string(self, client):
        """`Decimal` is exact and `json` cannot carry it. A working paper whose
        amounts arrive as strings renders as text and cannot be summed by
        anything downstream."""
        client_id, period_id = _period(client)

        response = client.get(
            f"/api/clients/{client_id}/periods/{period_id}/working-paper"
        )

        for line in response.json()["lines"]:
            assert isinstance(line["amount"], (int, float)), line

    def test_a_case_with_no_amount_reaches_the_chain_as_unknown_not_zero(self, client):
        """**The bug the pure tests could not see.** `build_working_paper`
        distinguishes an unquantified deduction from a zero one, and the route
        was flattening `None` to `0` with `or 0` before handing it over — so
        the endpoint reported a complete chain while a real `PaymentOverdue`
        case sat in the database with no amount at all.

        Same lesson as every other synthesis bug here: the unit test proves
        the function, and only a test through the real entry point proves the
        caller feeds it honestly."""
        client_id, period_id = _period(client)
        response = client.post(
            f"/api/clients/{client_id}/periods/{period_id}/cases",
            json={
                "invoice_no": "INV-UNPAID-1",
                "reason_code": "gst:PaymentOverdue",
                "supplier_gstin": "27AABCU9603R1ZM",
                "supplier_name": "Tata Steel Ltd",
                "books_amount": None,
            },
        )
        assert response.status_code in (200, 201), response.text

        paper = client.get(
            f"/api/clients/{client_id}/periods/{period_id}/working-paper"
        ).json()

        overdue = next(l for l in paper["lines"] if l["key"] == "reversal_rule_37")
        assert overdue["unquantified"] == 1, paper
        assert paper["complete"] is False, paper

    def test_every_line_reaches_the_client_with_its_source_intact(self, client):
        """The property that makes it a working paper survives serialization —
        a source lost at the HTTP boundary is a figure a reviewer cannot
        trace, and the pure tests above cannot see that."""
        client_id, period_id = _period(client)

        response = client.get(
            f"/api/clients/{client_id}/periods/{period_id}/working-paper"
        )

        for line in response.json()["lines"]:
            assert line["source"], line
