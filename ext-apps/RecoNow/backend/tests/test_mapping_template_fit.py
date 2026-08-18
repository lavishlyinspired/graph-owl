"""A stored mapping template must not be applied to a file it does not fit.

Found live, uploading the government-format purchase register through the UI:
a template learned earlier from a differently-ordered books export was applied
to it, mapping `invoice_no` onto the GSTIN column and `supplier_gstin` onto the
invoice-number column. Reconciliation would then have compared GSTINs as
invoice numbers — every match wrong, every figure wrong — while the mapping
screen displayed the wrong pairing as though it were a considered default.

The template is a convenience. It must never override what the file says.
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
async def client():
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin_conn = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin_conn.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin_conn.close()

    test_dsn = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    os.environ["DATABASE_URL"] = test_dsn
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


def _client_and_period(client) -> tuple[str, str]:
    client_id = client.post(
        "/api/clients", json={"name": "ABC Manufacturing", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"}
    ).json()["id"]
    period_id = client.post(
        f"/api/clients/{client_id}/periods", json={"month": "March", "year": 2026}
    ).json()["id"]
    return client_id, period_id


# The layout the template gets learned from.
LEARNED = b"Invoice No,Invoice Date,Supplier GSTIN,Supplier Name,Taxable Amount\r\n" \
          b"INV-MAR-001,01-03-2026,27AABCS1429B1Z8,Sharma Infra,250000\r\n"

# The same fields, different column order — what the government file actually did.
REORDERED_HEADERS = ["Supplier GSTIN", "Supplier Name", "Invoice No", "Invoice Date", "Taxable Amount"]
REORDERED = b"Supplier GSTIN,Supplier Name,Invoice No,Invoice Date,Taxable Amount\r\n" \
            b"27AABCS1429B1Z8,Sharma Infra,INV-MAR-001,01-03-2026,250000\r\n"


def _upload(client, client_id, period_id, payload, name="f.csv"):
    return client.post(
        f"/api/clients/{client_id}/periods/{period_id}/datasets/books/upload",
        files={"file": (name, payload, "text/csv")},
    ).json()


def _learn_template(client, client_id, period_id):
    upload = _upload(client, client_id, period_id, LEARNED, "learned.csv")
    client.post(
        f"/api/clients/{client_id}/periods/{period_id}/datasets/books/mapping",
        json={"mapping": upload["mapping"], "tolerance": 1.0},
    )
    return upload["mapping"]


def test_template_is_not_applied_to_a_file_whose_columns_differ(client):
    client_id, period_id = _client_and_period(client)
    learned = _learn_template(client, client_id, period_id)
    assert learned["invoice_no"] == 0  # in the learned layout

    body = _upload(client, client_id, period_id, REORDERED, "reordered.csv")

    assert body["from_template"] is False, "a template from a different layout must not be reused"

    # The mapping must describe *this* file.
    mapping = body["mapping"]
    assert REORDERED_HEADERS[mapping["invoice_no"]] == "Invoice No"
    assert REORDERED_HEADERS[mapping["supplier_gstin"]] == "Supplier GSTIN"
    assert REORDERED_HEADERS[mapping["supplier_name"]] == "Supplier Name"
    assert REORDERED_HEADERS[mapping["invoice_date"]] == "Invoice Date"


def test_template_is_still_applied_when_the_columns_match(client):
    """The opposite error: refusing every template would remove the feature."""
    client_id, period_id = _client_and_period(client)
    _learn_template(client, client_id, period_id)

    body = _upload(client, client_id, period_id, LEARNED, "again.csv")

    assert body["from_template"] is True
    assert body["mapping"]["invoice_no"] == 0


def test_a_template_recorded_without_headers_is_not_trusted(client):
    """Rows written before the fit check existed record no headers, so there is
    no way to tell whether they fit. Those must fail safe to the auto-mapper."""
    import asyncio
    import asyncpg as pg

    client_id, period_id = _client_and_period(client)
    _learn_template(client, client_id, period_id)

    async def blank_the_headers():
        conn = await pg.connect(os.environ["DATABASE_URL"])
        try:
            await conn.execute("UPDATE mapping_template SET source_headers = NULL")
            # Also make the stored mapping obviously wrong for the file, so
            # "the template was ignored" is provable from the result.
            await conn.execute("""UPDATE mapping_template SET mapping = '{"invoice_no": 2}'::jsonb""")
        finally:
            await conn.close()

    asyncio.run(blank_the_headers())

    body = _upload(client, client_id, period_id, LEARNED, "again.csv")

    assert body["from_template"] is False
    assert body["mapping"]["invoice_no"] == 0
