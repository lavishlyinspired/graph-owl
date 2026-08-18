"""Plan 122b B0: a fresh Postgres database per test, on the same shared,
reusable container the Rust workspace's own suite already uses
(`graph-owl-tests`, named + `ReuseDirective::Always` — see this repo's
CLAUDE.md build/test-loop notes) — one `CREATE DATABASE` per test is
milliseconds, and it means this Python suite adds no second container of
its own.

`RECONOW_TEST_DSN` overrides the admin connection for a CI environment
where the shared container has a different host/port; the default matches
the container's own fixed mapping (`docker port graph-owl-tests`).
"""

from __future__ import annotations

import os
import uuid

import asyncpg
import pytest

from app import db

ADMIN_DSN = os.environ.get(
    "RECONOW_TEST_ADMIN_DSN", "postgresql://postgres:postgres@localhost:55000/postgres"
)


@pytest.fixture
async def pool():
    db_name = "reconow_test_" + uuid.uuid4().hex[:12]
    admin_conn = await asyncpg.connect(ADMIN_DSN)
    try:
        await admin_conn.execute(f'CREATE DATABASE "{db_name}"')
    finally:
        await admin_conn.close()

    test_dsn = ADMIN_DSN.rsplit("/", 1)[0] + f"/{db_name}"
    test_pool = await asyncpg.create_pool(test_dsn, min_size=1, max_size=4)
    try:
        async with test_pool.acquire() as conn:
            await db.run_migrations(conn)
        yield test_pool
    finally:
        await test_pool.close()
        admin_conn = await asyncpg.connect(ADMIN_DSN)
        try:
            await admin_conn.execute(f'DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)')
        finally:
            await admin_conn.close()
