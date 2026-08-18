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


async def test_rollback_undoes_only_the_most_recent_migration(pool):
    """`rollback_last_migration` rolls back one migration at a time (the
    same shape `refinery` and every other migration runner uses) — rolling
    back once must undo only the *latest* applied migration's own effect,
    not reach back into an earlier one. Checked against whichever
    migration is actually last, not a hardcoded version string, so this
    does not need editing every time a new migration lands."""
    async with pool.acquire() as conn:
        before = sorted(row["version"] for row in await conn.fetch("SELECT version FROM schema_migrations"))
        await db.rollback_last_migration(conn)
        after = {row["version"] for row in await conn.fetch("SELECT version FROM schema_migrations")}
        columns = await conn.fetch(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'case_record'"
        )
    assert after == set(before[:-1])  # everything except the one just rolled back
    # 0001's own column survives any later migration's rollback.
    assert "invoice_no" in {row["column_name"] for row in columns}


async def test_repeated_rollback_eventually_drops_every_table(pool):
    async with pool.acquire() as conn:
        applied = await conn.fetch("SELECT version FROM schema_migrations")
        for _ in applied:
            await db.rollback_last_migration(conn)
        tables = await conn.fetch(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        )
        remaining = await conn.fetch("SELECT version FROM schema_migrations")
    names = {row["table_name"] for row in tables}
    assert "client" not in names
    assert "case_record" not in names
    assert len(remaining) == 0


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
