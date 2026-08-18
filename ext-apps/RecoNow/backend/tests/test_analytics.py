"""Period-over-period figures, and refusing to invent a trend.

The Analytics screen rendered a five-month Apr–Aug series (₹8.2L, ₹5.4L, …)
and an insight reading "Match rate improved 6 points since April, and
follow-up volume fell 75%" for a client whose only reconciled period is
March 2026. A trend needs more than one period; where there is one, saying
so is the answer.
"""

from __future__ import annotations

import os
import uuid

import asyncpg
import pytest
from fastapi.testclient import TestClient

ADMIN_DSN = os.environ.get(
    "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
)


@pytest.fixture
async def api():
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin.close()
    os.environ["DATABASE_URL"] = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as c:
            yield c
    finally:
        del os.environ["DATABASE_URL"]
        admin = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin.close()


def _client(c):
    return c.post(
        "/api/clients", json={"name": "ABC", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"}
    ).json()["id"]


def _period(c, cid, month, year=2026):
    return c.post(f"/api/clients/{cid}/periods", json={"month": month, "year": year}).json()["id"]


def _case(c, cid, pid, invoice, books, portal=None):
    c.post(
        f"/api/clients/{cid}/periods/{pid}/cases",
        json={
            "invoice_no": invoice, "reason_code": "gst:SupplierNotFiled",
            "books_amount": books, "portal_amount": portal,
        },
    )


def test_one_period_reports_itself_and_no_trend(api):
    cid = _client(api)
    pid = _period(api, cid, "March")
    _case(api, cid, pid, "INV-1", 17100)

    body = api.get(f"/api/clients/{cid}/analytics").json()

    assert body["has_trend"] is False, "a single period is not a trend"
    assert len(body["periods"]) == 1
    assert body["periods"][0]["label"] == "March 2026"
    assert body["periods"][0]["exposure"] == pytest.approx(17100)


def test_two_periods_give_a_trend_in_chronological_order(api):
    cid = _client(api)
    feb, mar = _period(api, cid, "February"), _period(api, cid, "March")
    _case(api, cid, feb, "INV-A", 5000)
    _case(api, cid, mar, "INV-B", 17100)

    body = api.get(f"/api/clients/{cid}/analytics").json()

    assert body["has_trend"] is True
    assert [p["label"] for p in body["periods"]] == ["February 2026", "March 2026"]
    assert [p["exposure"] for p in body["periods"]] == [pytest.approx(5000), pytest.approx(17100)]


def test_a_period_with_no_cases_reports_zero_not_absence(api):
    """A reconciled period with nothing wrong is a real, useful data point."""
    cid = _client(api)
    _period(api, cid, "March")

    body = api.get(f"/api/clients/{cid}/analytics").json()

    assert body["periods"][0]["exposure"] == 0
    assert body["periods"][0]["case_count"] == 0


def test_analytics_never_crosses_clients(api):
    cid_a, cid_b = _client(api), api.post(
        "/api/clients", json={"name": "XYZ", "gstin": "29BBBBB1111B2Z6", "state": "Karnataka"}
    ).json()["id"]
    pa = _period(api, cid_a, "March")
    _case(api, cid_a, pa, "INV-A", 17100)

    body = api.get(f"/api/clients/{cid_b}/analytics").json()

    assert body["periods"] == []
    assert body["has_trend"] is False
