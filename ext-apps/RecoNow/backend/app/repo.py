"""Plan 122b B0: repository access replacing the SESSION/AI_JOBS
module-level dicts. Every read that can cross a client or period boundary
takes `client_id` (and `period_id`, where the table has one) as a required
predicate, never an optional filter — the shape the plan's own mutation
note asks for: a query that can silently drop its client_id predicate and
still look correct against a single-client fixture is exactly the mutant
the isolation tests in `test_repo_isolation.py` exist to kill.
"""

from __future__ import annotations

import json
from typing import Any

import asyncpg


async def create_client(conn: asyncpg.Connection, *, name: str, gstin: str, state: str) -> str:
    row = await conn.fetchrow(
        "INSERT INTO client (name, gstin, state) VALUES ($1, $2, $3) RETURNING id",
        name, gstin, state,
    )
    return str(row["id"])


async def list_clients(conn: asyncpg.Connection) -> list[dict[str, Any]]:
    rows = await conn.fetch("SELECT id, name, gstin, state, created_at FROM client ORDER BY created_at")
    return [dict(row) for row in rows]


async def create_period(conn: asyncpg.Connection, *, client_id: str, month: str, year: int) -> str:
    row = await conn.fetchrow(
        "INSERT INTO period (client_id, month, year) VALUES ($1, $2, $3) RETURNING id",
        client_id, month, year,
    )
    return str(row["id"])


async def list_periods(conn: asyncpg.Connection, *, client_id: str) -> list[dict[str, Any]]:
    rows = await conn.fetch(
        "SELECT id, month, year, status, created_at FROM period WHERE client_id = $1 ORDER BY year, month",
        client_id,
    )
    return [dict(row) for row in rows]


async def create_case(
    conn: asyncpg.Connection,
    *,
    client_id: str,
    period_id: str,
    invoice_no: str,
    reason_code: str | None,
    supplier_name: str | None = None,
    supplier_gstin: str | None = None,
    books_amount: float | None = None,
    portal_amount: float | None = None,
    subject: str | None = None,
    summary: str | None = None,
    governed_by: str | None = None,
    evidence_count: int | None = None,
) -> str:
    row = await conn.fetchrow(
        "INSERT INTO case_record "
        "(client_id, period_id, invoice_no, reason_code, supplier_name, supplier_gstin, books_amount, "
        "portal_amount, subject, summary, governed_by, evidence_count) "
        "VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id",
        client_id, period_id, invoice_no, reason_code, supplier_name, supplier_gstin, books_amount, portal_amount,
        subject, summary, governed_by, evidence_count,
    )
    return str(row["id"])


_CASE_COLUMNS = (
    "id, invoice_no, reason_code, status, assigned_to, supplier_name, supplier_gstin, "
    "books_amount, portal_amount, subject, summary, governed_by, evidence_count, created_at, updated_at"
)


async def list_cases(conn: asyncpg.Connection, *, client_id: str, period_id: str) -> list[dict[str, Any]]:
    rows = await conn.fetch(
        f"SELECT {_CASE_COLUMNS} FROM case_record WHERE client_id = $1 AND period_id = $2 ORDER BY created_at",
        client_id, period_id,
    )
    return [dict(row) for row in rows]


async def get_case(conn: asyncpg.Connection, *, client_id: str, case_id: str) -> dict[str, Any] | None:
    row = await conn.fetchrow(
        f"SELECT {_CASE_COLUMNS}, period_id FROM case_record WHERE client_id = $1 AND id = $2",
        client_id, case_id,
    )
    return dict(row) if row is not None else None


async def create_ims_decision(
    conn: asyncpg.Connection, *, client_id: str, period_id: str, case_id: str | None, decision: str
) -> str:
    row = await conn.fetchrow(
        "INSERT INTO ims_decision (client_id, period_id, case_id, decision) VALUES ($1, $2, $3, $4) RETURNING id",
        client_id, period_id, case_id, decision,
    )
    return str(row["id"])


async def list_ims_decisions(conn: asyncpg.Connection, *, client_id: str, period_id: str) -> list[dict[str, Any]]:
    rows = await conn.fetch(
        "SELECT id, case_id, decision, decided_by, decided_at FROM ims_decision "
        "WHERE client_id = $1 AND period_id = $2 ORDER BY decided_at",
        client_id, period_id,
    )
    return [dict(row) for row in rows]


async def create_follow_up(
    conn: asyncpg.Connection,
    *,
    client_id: str,
    period_id: str,
    case_id: str | None,
    supplier_name: str | None,
    message: str | None,
) -> str:
    row = await conn.fetchrow(
        "INSERT INTO follow_up (client_id, period_id, case_id, supplier_name, message) "
        "VALUES ($1, $2, $3, $4, $5) RETURNING id",
        client_id, period_id, case_id, supplier_name, message,
    )
    return str(row["id"])


async def list_follow_ups(conn: asyncpg.Connection, *, client_id: str) -> list[dict[str, Any]]:
    rows = await conn.fetch(
        "SELECT id, period_id, case_id, supplier_name, status, message, created_at, sent_at "
        "FROM follow_up WHERE client_id = $1 ORDER BY created_at",
        client_id,
    )
    return [dict(row) for row in rows]


async def create_approval(
    conn: asyncpg.Connection,
    *,
    client_id: str,
    period_id: str,
    decision_type: str,
    amount: float | None,
    requested_by: str | None,
) -> str:
    row = await conn.fetchrow(
        "INSERT INTO approval (client_id, period_id, decision_type, amount, requested_by) "
        "VALUES ($1, $2, $3, $4, $5) RETURNING id",
        client_id, period_id, decision_type, amount, requested_by,
    )
    return str(row["id"])


async def list_approvals(
    conn: asyncpg.Connection, *, client_id: str, period_id: str, status: str | None = None
) -> list[dict[str, Any]]:
    if status is None:
        rows = await conn.fetch(
            "SELECT id, decision_type, amount, requested_by, status, decided_by, decided_at, created_at "
            "FROM approval WHERE client_id = $1 AND period_id = $2 ORDER BY created_at",
            client_id, period_id,
        )
    else:
        rows = await conn.fetch(
            "SELECT id, decision_type, amount, requested_by, status, decided_by, decided_at, created_at "
            "FROM approval WHERE client_id = $1 AND period_id = $2 AND status = $3 ORDER BY created_at",
            client_id, period_id, status,
        )
    return [dict(row) for row in rows]


async def decide_approval(conn: asyncpg.Connection, *, client_id: str, approval_id: str, status: str) -> dict[str, Any] | None:
    row = await conn.fetchrow(
        "UPDATE approval SET status = $1, decided_at = now() WHERE id = $2 AND client_id = $3 "
        "RETURNING id, decision_type, amount, status, decided_at",
        status, approval_id, client_id,
    )
    return dict(row) if row is not None else None


async def create_note(
    conn: asyncpg.Connection,
    *,
    client_id: str,
    period_id: str | None,
    case_id: str | None,
    author: str | None,
    body: str,
) -> str:
    row = await conn.fetchrow(
        "INSERT INTO note (client_id, period_id, case_id, author, body) VALUES ($1, $2, $3, $4, $5) RETURNING id",
        client_id, period_id, case_id, author, body,
    )
    return str(row["id"])


async def list_notes(conn: asyncpg.Connection, *, client_id: str, case_id: str) -> list[dict[str, Any]]:
    rows = await conn.fetch(
        "SELECT id, author, body, created_at FROM note WHERE client_id = $1 AND case_id = $2 ORDER BY created_at",
        client_id, case_id,
    )
    return [dict(row) for row in rows]


async def create_deliverable(
    conn: asyncpg.Connection, *, client_id: str, period_id: str, kind: str, content: str | None
) -> str:
    row = await conn.fetchrow(
        "INSERT INTO deliverable (client_id, period_id, kind, content) VALUES ($1, $2, $3, $4) RETURNING id",
        client_id, period_id, kind, content,
    )
    return str(row["id"])


async def list_deliverables(conn: asyncpg.Connection, *, client_id: str, period_id: str) -> list[dict[str, Any]]:
    rows = await conn.fetch(
        "SELECT id, kind, status, content, generated_at FROM deliverable "
        "WHERE client_id = $1 AND period_id = $2 ORDER BY generated_at",
        client_id, period_id,
    )
    return [dict(row) for row in rows]


async def upsert_mapping_template(
    conn: asyncpg.Connection, *, client_id: str, dataset_kind: str, mapping: dict[str, Any], tolerance: float
) -> None:
    await conn.execute(
        "INSERT INTO mapping_template (client_id, dataset_kind, mapping, tolerance, updated_at) "
        "VALUES ($1, $2, $3::jsonb, $4, now()) "
        "ON CONFLICT (client_id, dataset_kind) DO UPDATE "
        "SET mapping = EXCLUDED.mapping, tolerance = EXCLUDED.tolerance, updated_at = now()",
        client_id, dataset_kind, json.dumps(mapping), tolerance,
    )


async def get_mapping_template(conn: asyncpg.Connection, *, client_id: str, dataset_kind: str) -> dict[str, Any] | None:
    row = await conn.fetchrow(
        "SELECT mapping, tolerance FROM mapping_template WHERE client_id = $1 AND dataset_kind = $2",
        client_id, dataset_kind,
    )
    if row is None:
        return None
    return {"mapping": json.loads(row["mapping"]), "tolerance": row["tolerance"]}


async def create_user(conn: asyncpg.Connection, *, name: str, email: str, role: str) -> str:
    row = await conn.fetchrow(
        "INSERT INTO app_user (name, email, role) VALUES ($1, $2, $3) RETURNING id",
        name, email, role,
    )
    return str(row["id"])


async def list_users(conn: asyncpg.Connection) -> list[dict[str, Any]]:
    rows = await conn.fetch("SELECT id, name, email, role, created_at FROM app_user ORDER BY created_at")
    return [dict(row) for row in rows]
