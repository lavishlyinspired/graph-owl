"""An uploaded file must survive a restart, and stay reviewable afterwards.

Uploads lived in a module-level `WORKSPACES` dict. A browser refresh was
fine, but a backend restart dropped every file, and there was no way to
reopen a file's mapping once you navigated away from the screen that
uploaded it — the mapping table only ever showed the file just picked.
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

BOOKS = (
    b"Invoice No,Invoice Date,Supplier GSTIN,Supplier Name,Taxable Amount\r\n"
    b"INV-MAR-001,01-03-2026,27AABCS1429B1Z8,Sharma Infra,250000\r\n"
)


@pytest.fixture
async def db():
    """A database that outlives the app, so the app can be restarted against it."""
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin_conn = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin_conn.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin_conn.close()
    dsn = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    try:
        yield dsn
    finally:
        admin_conn = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin_conn.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin_conn.close()


def _boot(dsn):
    """Start the app fresh against `dsn` — stands in for a process restart.

    Python caches imported modules, so simply re-importing `app.main` keeps
    any module-level state alive and a persistence test would pass without
    anything being persisted. Clearing that state is what makes the second
    boot a real restart rather than the same process with a new TestClient.
    """
    os.environ["DATABASE_URL"] = dsn
    import app.main as main

    for attr in ("WORKSPACES", "SESSION"):
        state = getattr(main, attr, None)
        if isinstance(state, dict):
            state.clear()

    return TestClient(main.app)


def _client_and_period(c):
    cid = c.post(
        "/api/clients", json={"name": "ABC", "gstin": "27AAAAA0000A1Z5", "state": "Maharashtra"}
    ).json()["id"]
    pid = c.post(f"/api/clients/{cid}/periods", json={"month": "March", "year": 2026}).json()["id"]
    return cid, pid


def test_an_uploaded_dataset_survives_a_restart(db):
    with _boot(db) as c:
        cid, pid = _client_and_period(c)
        c.post(
            f"/api/clients/{cid}/periods/{pid}/datasets/books/upload",
            files={"file": ("books.csv", BOOKS, "text/csv")},
        )
        before = c.get(f"/api/clients/{cid}/periods/{pid}/datasets").json()
        assert [d["kind"] for d in before] == ["books"]

    # Fresh app object against the same database — module-level state is gone.
    with _boot(db) as c2:
        after = c2.get(f"/api/clients/{cid}/periods/{pid}/datasets").json()

    assert [d["kind"] for d in after] == ["books"]
    assert after[0]["name"] == "books.csv"
    assert after[0]["total_rows"] == 1


def test_a_previously_uploaded_dataset_can_be_reopened_for_review(db):
    """Switching screens and coming back must show the file and its mapping,
    not an empty upload prompt."""
    with _boot(db) as c:
        cid, pid = _client_and_period(c)
        upload = c.post(
            f"/api/clients/{cid}/periods/{pid}/datasets/books/upload",
            files={"file": ("books.csv", BOOKS, "text/csv")},
        ).json()

    with _boot(db) as c2:
        detail = c2.get(f"/api/clients/{cid}/periods/{pid}/datasets/books")
        assert detail.status_code == 200
        body = detail.json()

    assert body["headers"] == upload["headers"]
    assert body["mapping"] == upload["mapping"]
    assert body["preview"], "a reopened file shows its own rows"
    assert body["total_rows"] == 1


def test_a_confirmed_mapping_is_still_confirmed_after_a_restart(db):
    with _boot(db) as c:
        cid, pid = _client_and_period(c)
        upload = c.post(
            f"/api/clients/{cid}/periods/{pid}/datasets/books/upload",
            files={"file": ("books.csv", BOOKS, "text/csv")},
        ).json()
        c.post(
            f"/api/clients/{cid}/periods/{pid}/datasets/books/mapping",
            json={"mapping": upload["mapping"], "tolerance": 1.0},
        )

    with _boot(db) as c2:
        datasets = c2.get(f"/api/clients/{cid}/periods/{pid}/datasets").json()

    assert datasets[0]["confirmed"] is True


def test_re_uploading_replaces_rather_than_duplicates(db):
    with _boot(db) as c:
        cid, pid = _client_and_period(c)
        for _ in range(2):
            c.post(
                f"/api/clients/{cid}/periods/{pid}/datasets/books/upload",
                files={"file": ("books.csv", BOOKS, "text/csv")},
            )
        datasets = c.get(f"/api/clients/{cid}/periods/{pid}/datasets").json()

    assert len(datasets) == 1


def test_datasets_do_not_leak_between_periods(db):
    with _boot(db) as c:
        cid, pid = _client_and_period(c)
        other = c.post(f"/api/clients/{cid}/periods", json={"month": "April", "year": 2026}).json()["id"]
        c.post(
            f"/api/clients/{cid}/periods/{pid}/datasets/books/upload",
            files={"file": ("books.csv", BOOKS, "text/csv")},
        )
        assert c.get(f"/api/clients/{cid}/periods/{other}/datasets").json() == []


# The real government-format purchase register, whose empty `Note Type` and
# `Original Invoice No` cells pandas reads as float NaN. Storing that as JSON
# failed outright ("invalid input syntax for type json / Token NaN"), which
# only appeared once uploads were persisted rather than held in memory.
GOV_FORMAT = (
    b"Invoice No,Invoice Date,Supplier GSTIN,Supplier Name,Taxable Amount,Note Type,Original Invoice No\r\n"
    b"INV-MAR-001,01-03-2026,27AABCS1429B1Z8,Sharma Infra,250000,,\r\n"
)


def test_a_file_with_empty_cells_can_be_uploaded_and_reopened(db):
    with _boot(db) as c:
        cid, pid = _client_and_period(c)
        upload = c.post(
            f"/api/clients/{cid}/periods/{pid}/datasets/books/upload",
            files={"file": ("gov.csv", GOV_FORMAT, "text/csv")},
        )
        assert upload.status_code == 200, upload.text

    with _boot(db) as c2:
        body = c2.get(f"/api/clients/{cid}/periods/{pid}/datasets/books").json()

    assert body["total_rows"] == 1
    # An empty cell reads as null, not as a NaN that no JSON parser accepts.
    assert body["preview"][0]["Note Type"] is None
    assert body["preview"][0]["Original Invoice No"] is None
