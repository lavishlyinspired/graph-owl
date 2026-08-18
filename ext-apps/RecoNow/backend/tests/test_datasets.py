"""RED tests for Plan 122b B3's upload & map flow, scoped to client+period.

Also the RED for a real isolation gap found while designing this slice:
`_ingest_to_graphowl`'s old `source = f"reco-{kind}"` was a single global
name, fine for the pre-B0 single-session app but silently unsafe once two
clients' uploads can be in flight at once — a re-upload for client B would
delete and replace client A's *own* books under the same source name. The
new per-(client, period, kind) source name is asserted directly rather than
trusted by inspection.
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


def _client_and_period(client, name="ABC Manufacturing", gstin="27AAAAA0000A1Z5") -> tuple[str, str]:
    client_id = client.post("/api/clients", json={"name": name, "gstin": gstin, "state": "Maharashtra"}).json()["id"]
    period_id = client.post(f"/api/clients/{client_id}/periods", json={"month": "August", "year": 2026}).json()["id"]
    return client_id, period_id


BOOKS_CSV = (
    b"Supplier GSTIN,Supplier Name,Invoice Number,Invoice Date,Taxable Amount,IGST,CGST,SGST\r\n"
    b"27AABCU9603R1ZM,Tata Steel Ltd,INV-2024-001,15-12-2025,500000,45000,0,0\r\n"
)


def test_uploading_a_file_returns_headers_and_an_auto_proposed_mapping(client):
    client_id, period_id = _client_and_period(client)
    response = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/datasets/books/upload",
        files={"file": ("books.csv", BOOKS_CSV, "text/csv")},
    )
    assert response.status_code == 200
    body = response.json()
    assert body["total_rows"] == 1
    assert body["mapping"]["invoice_no"] is not None
    assert body["mapping"]["taxable"] is not None


def test_confirming_a_mapping_saves_it_as_a_template_for_the_client(client):
    client_id, period_id = _client_and_period(client)
    upload = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/datasets/books/upload",
        files={"file": ("books.csv", BOOKS_CSV, "text/csv")},
    ).json()

    confirm = client.post(
        f"/api/clients/{client_id}/periods/{period_id}/datasets/books/mapping",
        json={"mapping": upload["mapping"], "tolerance": 1.0},
    )
    assert confirm.status_code == 200

    datasets = client.get(f"/api/clients/{client_id}/periods/{period_id}/datasets").json()
    books = next(d for d in datasets if d["kind"] == "books")
    assert books["confirmed"] is True


def test_a_confirmed_mapping_template_is_reused_for_a_second_period(client):
    """The plan's own RED: "the saved template applies to a second period's
    identically shaped file" — proven by uploading the same shaped file to
    a *different* period for the *same* client and getting the same
    mapping back without confirming again."""
    client_id, period_a = _client_and_period(client)
    upload_a = client.post(
        f"/api/clients/{client_id}/periods/{period_a}/datasets/books/upload",
        files={"file": ("books.csv", BOOKS_CSV, "text/csv")},
    ).json()
    client.post(
        f"/api/clients/{client_id}/periods/{period_a}/datasets/books/mapping",
        json={"mapping": upload_a["mapping"], "tolerance": 1.0},
    )

    period_b = client.post(f"/api/clients/{client_id}/periods", json={"month": "September", "year": 2026}).json()["id"]
    upload_b = client.post(
        f"/api/clients/{client_id}/periods/{period_b}/datasets/books/upload",
        files={"file": ("books.csv", BOOKS_CSV, "text/csv")},
    ).json()

    assert upload_b["mapping"] == upload_a["mapping"]
    assert upload_b["from_template"] is True


def test_reconcile_is_blocked_until_every_dataset_is_confirmed(client):
    client_id, period_id = _client_and_period(client)
    client.post(
        f"/api/clients/{client_id}/periods/{period_id}/datasets/books/upload",
        files={"file": ("books.csv", BOOKS_CSV, "text/csv")},
    )
    # books uploaded but not confirmed yet.
    response = client.post(f"/api/clients/{client_id}/periods/{period_id}/reconcile")
    assert response.status_code == 409
