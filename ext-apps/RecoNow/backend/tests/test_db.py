"""RED tests for Plan 122b B0's migration runner (`app.db`), against a real,
freshly created Postgres database (`conftest.py`'s `pool` fixture) — the
plan's own AC: "migrations run forward and roll back."
"""

from __future__ import annotations

from app import db


async def test_migrations_create_every_table(pool):
    async with pool.acquire() as conn:
        tables = await conn.fetch(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        )
    names = {row["table_name"] for row in tables}
    expected = {
        "schema_migrations",
        "client",
        "period",
        "app_user",
        "case_record",
        "ims_decision",
        "follow_up",
        "approval",
        "note",
        "deliverable",
        "mapping_template",
    }
    assert expected <= names


async def test_applying_migrations_twice_is_a_no_op(pool):
    # The `pool` fixture already ran migrations once on setup — running
    # again must not error (e.g. on a duplicate CREATE TABLE) and must not
    # duplicate the tracking rows.
    async with pool.acquire() as conn:
        await db.run_migrations(conn)
        applied = await conn.fetch("SELECT version FROM schema_migrations ORDER BY version")
    versions = [row["version"] for row in applied]
    assert versions == sorted(set(versions))


async def test_rollback_drops_the_tables_the_matching_migration_created(pool):
    async with pool.acquire() as conn:
        await db.rollback_last_migration(conn)
        tables = await conn.fetch(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        )
        applied = await conn.fetch("SELECT version FROM schema_migrations")
    names = {row["table_name"] for row in tables}
    assert "client" not in names
    assert "case_record" not in names
    assert len(applied) == 0


async def test_rollback_then_reapply_recreates_the_schema(pool):
    async with pool.acquire() as conn:
        await db.rollback_last_migration(conn)
        await db.run_migrations(conn)
        tables = await conn.fetch(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        )
    names = {row["table_name"] for row in tables}
    assert "client" in names
    assert "case_record" in names
