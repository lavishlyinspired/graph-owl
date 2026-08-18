"""Plan 122b B0: a minimal, dependency-free migration runner.

Hand-rolled rather than adopted — `plans/00l-build-vs-adopt.md`'s B0 entry
records why (psycopg was rejected on licence, yoyo-migrations was blocked on
auditability, and a full ORM/Alembic stack is more than 10 small tables and
direct SQL need). Numbered `.up.sql` / `.down.sql` pairs in `migrations/`,
tracked in `schema_migrations` — the same shape the Rust workspace's own
`refinery` migrations use, reimplemented at the size this backend needs.
"""

from __future__ import annotations

from pathlib import Path

import asyncpg

MIGRATIONS_DIR = Path(__file__).resolve().parent.parent / "migrations"


def _migration_versions() -> list[str]:
    """Every migration's version stem (`0001_initial`), sorted — the sort
    is lexicographic on purpose, so the numeric prefix must stay
    zero-padded for as many migrations as this project ever has."""
    return sorted({path.name.split(".", 1)[0] for path in MIGRATIONS_DIR.glob("*.up.sql")})


async def run_migrations(conn: asyncpg.Connection) -> None:
    await conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version TEXT PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())"
    )
    applied = {row["version"] for row in await conn.fetch("SELECT version FROM schema_migrations")}
    for version in _migration_versions():
        if version in applied:
            continue
        sql = (MIGRATIONS_DIR / f"{version}.up.sql").read_text(encoding="utf-8")
        async with conn.transaction():
            await conn.execute(sql)
            await conn.execute("INSERT INTO schema_migrations (version) VALUES ($1)", version)


async def rollback_last_migration(conn: asyncpg.Connection) -> None:
    row = await conn.fetchrow("SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1")
    if row is None:
        return
    version = row["version"]
    sql = (MIGRATIONS_DIR / f"{version}.down.sql").read_text(encoding="utf-8")
    async with conn.transaction():
        await conn.execute(sql)
        await conn.execute("DELETE FROM schema_migrations WHERE version = $1", version)


async def create_pool(dsn: str) -> asyncpg.Pool:
    pool = await asyncpg.create_pool(dsn, min_size=1, max_size=10)
    async with pool.acquire() as conn:
        await run_migrations(conn)
    return pool
