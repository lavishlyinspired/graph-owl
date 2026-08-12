"""In-memory uploaded-file store for the agent service — the same
in-process-only posture `server.py`'s own `_THREADS` already has (no
persistence across a restart; this is a chat-session convenience, not a
catalog entry). Shared between the HTTP upload/preview routes
(`POST /files`, `GET /files/{id}`) and the `reconcile_uploaded_files`
tool in `reconcile_uploaded.py`, so a tool call reads exactly what a
`GET /files/{id}` preview would show — one store, not two copies that
could drift.
"""

from __future__ import annotations

import json
import uuid
from dataclasses import dataclass


@dataclass(frozen=True)
class FileRecord:
    file_id: str
    name: str
    content_type: str
    content: str  # raw text, exactly as uploaded — no parsing at store time


_FILES: dict[str, FileRecord] = {}


def store_file(name: str, content_type: str, content: str) -> FileRecord:
    record = FileRecord(
        file_id=str(uuid.uuid4()), name=name, content_type=content_type, content=content
    )
    _FILES[record.file_id] = record
    return record


def get_file(file_id: str) -> FileRecord | None:
    return _FILES.get(file_id)


def parse_json_file(file_id: str) -> dict:
    """Look up an uploaded file and parse it as JSON, raising a
    `ValueError` (never `KeyError`/`JSONDecodeError` directly) for
    either failure mode — a single, callable-friendly exception type a
    tool wrapper can catch without needing to know which of "no such
    file" or "not valid JSON" occurred to handle it safely."""
    record = get_file(file_id)
    if record is None:
        raise ValueError(f"no such uploaded file: {file_id}")
    try:
        return json.loads(record.content)
    except json.JSONDecodeError as bad_json:
        raise ValueError(f"{record.name} is not valid JSON: {bad_json}") from bad_json
