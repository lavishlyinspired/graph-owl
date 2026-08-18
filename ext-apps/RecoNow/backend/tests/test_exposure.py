"""How much money a period actually has at risk, and in what JSON type.

Both defects here were found against the real reconciled March 2026 data,
not constructed:

1. Two rules can flag the same invoice. `gst:SupplierNotFiled` and
   `gst:PotentialMismatch` both fired on INV-MAR-013 for the same ₹17,100,
   and the total added it twice — the register reported ₹52,070 at risk
   where ₹26,240 is. Every screen that quotes a total inherited it.

2. `case_record.books_amount` is NUMERIC, asyncpg returns Decimal, and
   FastAPI serialises Decimal as a JSON *string* — `"17100"`, and
   `"1.8E+5"` for 180000. The TypeScript type says `number`, so the UI
   called `.toLocaleString()` on a string and rendered "1.8E+5" to a user
   reading a tax position.
"""

from __future__ import annotations

import json

import pytest

from app.main import period_exposure


@pytest.fixture
def client_with_cases():
    """A period holding one case with a large amount, through the real API."""
    import os, uuid, asyncio, asyncpg as pg
    from fastapi.testclient import TestClient

    admin = os.environ.get(
        "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
    )
    name = "reconow_test_" + uuid.uuid4().hex[:12]

    async def make():
        c = await pg.connect(admin)
        try:
            await c.execute(f'CREATE DATABASE "{name}"')
        finally:
            await c.close()

    async def drop():
        c = await pg.connect(admin)
        try:
            await c.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')
        finally:
            await c.close()

    asyncio.run(make())
    os.environ["DATABASE_URL"] = admin.rsplit("/", 1)[0] + f"/{name}"
    try:
        from app.main import app as fastapi_app

        with TestClient(fastapi_app) as tc:
            cid = tc.post(
                "/api/clients",
                json={"name": "ABC", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"},
            ).json()["id"]
            pid = tc.post(
                f"/api/clients/{cid}/periods", json={"month": "March", "year": 2026}
            ).json()["id"]
            tc.post(
                f"/api/clients/{cid}/periods/{pid}/cases",
                json={
                    "invoice_no": "INV-MAR-011",
                    "reason_code": "gst:AmountMismatch",
                    "books_amount": 180000,
                    "portal_amount": 180500,
                },
            )
            yield tc, cid, pid
    finally:
        del os.environ["DATABASE_URL"]
        asyncio.run(drop())


def _case(invoice: str, reason: str, books: float | None, portal: float | None = None) -> dict:
    return {
        "invoice_no": invoice,
        "reason_code": reason,
        "books_amount": books,
        "portal_amount": portal,
    }


def test_one_invoice_flagged_by_two_rules_is_counted_once():
    cases = [
        _case("INV-MAR-013", "gst:SupplierNotFiled", 17100),
        _case("INV-MAR-013", "gst:PotentialMismatch", 17100),
    ]
    # The same ₹17,100 identified twice is still ₹17,100 at risk.
    assert period_exposure(cases) == pytest.approx(17100)


def test_different_invoices_do_add_up():
    cases = [
        _case("INV-MAR-013", "gst:SupplierNotFiled", 17100),
        _case("INV-MAR-014", "gst:SupplierNotFiled", 8640),
    ]
    assert period_exposure(cases) == pytest.approx(25740)


def test_the_largest_discrepancy_on_an_invoice_is_the_one_that_counts():
    """Two rules disagreeing about how much of one invoice is at risk is not
    two separate exposures. Taking the larger states the strongest claim the
    rules make about that invoice without inventing a sum nobody computed."""
    cases = [
        _case("INV-MAR-011", "gst:AmountMismatch", 180000, 180500),   # 500
        _case("INV-MAR-011", "gst:TaxHeadMismatch", 32400, 32490),    # 90
    ]
    assert period_exposure(cases) == pytest.approx(500)


def test_the_real_march_2026_figure():
    """The whole reconciled period, hand-totalled from the six real findings:
    17100 (INV-MAR-013) + 8640 (INV-MAR-014) + 500 (INV-MAR-011) = 26240."""
    cases = [
        _case("INV-MAR-013", "gst:SupplierNotFiled", 17100),
        _case("INV-MAR-013", "gst:PotentialMismatch", 17100),
        _case("INV-MAR-014", "gst:SupplierNotFiled", 8640),
        _case("INV-MAR-014", "gst:PotentialMismatch", 8640),
        _case("INV-MAR-011", "gst:AmountMismatch", 180000, 180500),
        _case("INV-MAR-011", "gst:TaxHeadMismatch", 32400, 32490),
    ]
    assert period_exposure(cases) == pytest.approx(26240)


def test_a_case_with_no_amount_contributes_nothing_rather_than_erroring():
    assert period_exposure([_case("INV-1", "gst:X", None)]) == pytest.approx(0)


def test_decimal_amounts_reach_the_wire_as_json_numbers(client_with_cases):
    """A rupee figure must arrive as a number. As a string the UI cannot
    format it, and 180000 renders to the user as "1.8E+5"."""
    client, cid, pid = client_with_cases

    raw = client.get(f"/api/clients/{cid}/periods/{pid}/register").text
    body = json.loads(raw)
    row = body["rows"][0]

    assert isinstance(row["books_amount"], (int, float)), f"got {type(row['books_amount'])}"
    assert not isinstance(row["books_amount"], str)
    assert isinstance(body["total_exposure"], (int, float))
    assert "1.8E+5" not in raw and "1.8e+5" not in raw
