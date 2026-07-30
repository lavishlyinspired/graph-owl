"""The ingestion client — Epic 16 Slice E.

Decision 5: "SDKs are generated from the OpenAPI contract, hand-wrapped for
ergonomics." The generated layer is types and paths; this file is the
ergonomics, and it exists to stop every adapter author writing the same four
things badly:

1. **batching**, because the service refuses more than 1000 items per request
2. **idempotency keys**, because a retry without one duplicates
3. **retry with backoff**, because at-least-once transport is the normal case
4. **a builder**, because assembling the envelope by hand invites field-name
   typos that only fail at the server

Stdlib only. See ``pyproject.toml`` for why.
"""

from __future__ import annotations

import json
import random
import time
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Iterable, Literal

#: How many items and edges the service accepts in one synchronous push.
MAX_ITEMS_PER_PUSH = 1000


@dataclass(frozen=True)
class EntityDraft:
    kind: str
    name: str
    parent_fqn: str | None = None
    description: str | None = None
    properties: dict[str, Any] | None = None

    def wire(self) -> dict[str, Any]:
        body: dict[str, Any] = {"kind": self.kind, "name": self.name}
        if self.parent_fqn is not None:
            body["parentFqn"] = self.parent_fqn
        if self.description is not None:
            body["description"] = self.description
        if self.properties is not None:
            body["properties"] = self.properties
        return body


@dataclass(frozen=True)
class EdgeDraft:
    from_fqn: str
    to_fqn: str
    relationship: str
    query: str | None = None
    description: str | None = None

    def wire(self) -> dict[str, Any]:
        body: dict[str, Any] = {
            "fromFqn": self.from_fqn,
            "toFqn": self.to_fqn,
            "relationship": self.relationship,
        }
        if self.query is not None:
            body["query"] = self.query
        if self.description is not None:
            body["description"] = self.description
        return body


@dataclass
class IngestRequest:
    items: list[EntityDraft] = field(default_factory=list)
    edges: list[EdgeDraft] = field(default_factory=list)

    def wire(self) -> dict[str, Any]:
        return {
            "items": [item.wire() for item in self.items],
            "edges": [edge.wire() for edge in self.edges],
        }


class IngestBuilder:
    """Assemble a push without hand-writing the envelope."""

    def __init__(self) -> None:
        self._items: list[EntityDraft] = []
        self._edges: list[EdgeDraft] = []

    def entity(self, kind: str, name: str, **rest: Any) -> "IngestBuilder":
        self._items.append(EntityDraft(kind=kind, name=name, **rest))
        return self

    def edge(
        self, from_fqn: str, to_fqn: str, relationship: str, **rest: Any
    ) -> "IngestBuilder":
        """An edge between two entities, named by FQN.

        Endpoints may be entities added to this same push: the service orders a
        batch itself, so a pusher never has to submit in dependency order.
        """
        self._edges.append(
            EdgeDraft(
                from_fqn=from_fqn, to_fqn=to_fqn, relationship=relationship, **rest
            )
        )
        return self

    def build(self) -> IngestRequest:
        # Copied, so a builder reused after ``build()`` cannot reach back into a
        # request already on its way to the server.
        return IngestRequest(items=list(self._items), edges=list(self._edges))


def chunk(
    request: IngestRequest, limit: int = MAX_ITEMS_PER_PUSH
) -> list[IngestRequest]:
    """Split a push into requests the service will accept.

    The ceiling counts items **and** edges together, because the service does —
    splitting a load across the two fields would otherwise double what one
    request costs it.

    Edges follow all the entities rather than riding along with them. An edge
    whose endpoints landed in an earlier chunk still resolves; one sent before
    its endpoints does not, and a pusher who had to think about that would be
    doing the catalog's job.
    """
    if limit < 1:
        raise ValueError("a chunk holds at least one item")

    parts: list[IngestRequest] = [
        IngestRequest(items=request.items[at : at + limit])
        for at in range(0, len(request.items), limit)
    ]
    parts.extend(
        IngestRequest(edges=request.edges[at : at + limit])
        for at in range(0, len(request.edges), limit)
    )
    # An empty push is still one request: the caller asked for something to
    # happen, and silently doing nothing is the least debuggable answer there is.
    return parts or [IngestRequest()]


def backoff_seconds(
    attempt: int,
    base: float = 0.2,
    cap: float = 30.0,
    jitter: Callable[[], float] = random.random,
) -> float:
    """How long to wait before attempt ``attempt`` (0-based).

    Exponential, capped, and jittered. The cap matters because an uncapped
    doubling reaches hours; the jitter matters because a fleet of adapters that
    all retry on the same schedule reconverges into the spike that made them
    retry.
    """
    ceiling = min(cap, base * 2**attempt)
    return ceiling * (0.5 + jitter() / 2)


def is_retryable(status: int) -> bool:
    """A status worth retrying: the server said "not now", not "not ever".

    409 is deliberately absent. An idempotency conflict means the key was used
    for *different* content, so retrying with it can never succeed — it is a bug
    in the caller, and looping hides that behind 30 seconds of silence.
    """
    return status == 429 or 500 <= status <= 599


class GraphOwlError(RuntimeError):
    def __init__(self, status: int, body: Any) -> None:
        super().__init__(f"graph-owl responded {status}: {body!r}")
        self.status = status
        self.body = body


class GraphOwlClient:
    def __init__(
        self,
        base_url: str,
        token: str | None = None,
        max_attempts: int = 5,
        opener: Callable[[urllib.request.Request], Any] | None = None,
        sleep: Callable[[float], None] = time.sleep,
        new_key: Callable[[], str] = lambda: str(uuid.uuid4()),
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.max_attempts = max_attempts
        # Injected so the retry schedule and the key discipline are testable
        # without a server and without waiting for the backoff.
        self._open = opener or urllib.request.urlopen
        self._sleep = sleep
        self._new_key = new_key

    def push(self, request: IngestRequest) -> dict[str, Any]:
        """Push, splitting and retrying as needed.

        **Each chunk gets one key, and every retry of that chunk reuses it.** A
        key per attempt would make the retry a second push, which is the exact
        duplication the key exists to prevent — the single most common way a
        hand-written client gets this wrong.
        """
        merged: dict[str, Any] = {"accepted": 0, "rejected": 0, "results": []}
        offset = 0
        for part in chunk(request):
            answer = self._send(
                "POST", "/ingest", body=part.wire(), idempotency_key=self._new_key()
            )
            merged["accepted"] += answer.get("accepted", 0)
            merged["rejected"] += answer.get("rejected", 0)
            # Indexes are renumbered against the *caller's* list. A client that
            # sent 2500 items and got three reports each starting at 0 would have
            # to reconstruct the arithmetic this SDK exists to hide.
            for result in answer.get("results", []):
                merged["results"].append({**result, "index": result["index"] + offset})
            offset += len(part.items) + len(part.edges)
        return merged

    def push_file(
        self, body: str | bytes, fmt: Literal["jsonl", "csv"]
    ) -> dict[str, Any]:
        """Upload a batch file. Returns immediately with a handle to poll."""
        content_type = "application/x-ndjson" if fmt == "jsonl" else "text/csv"
        raw = body.encode("utf-8") if isinstance(body, str) else body
        return self._send("POST", "/ingest/batch", raw=raw, content_type=content_type)

    def job(self, job_id: str) -> dict[str, Any]:
        return self._send("GET", f"/ingest/jobs/{job_id}")

    def cancel_job(self, job_id: str) -> dict[str, Any]:
        return self._send("DELETE", f"/ingest/jobs/{job_id}")

    def await_job(self, job_id: str, attempts: int = 600) -> dict[str, Any]:
        """Poll until the job stops moving."""
        for attempt in range(attempts):
            job = self.job(job_id)
            if job.get("state") not in ("queued", "running"):
                return job
            self._sleep(backoff_seconds(min(attempt, 4), base=0.1, cap=2.0))
        raise TimeoutError(f"job {job_id} never settled")

    def _send(
        self,
        method: str,
        path: str,
        body: Any | None = None,
        raw: bytes | None = None,
        idempotency_key: str | None = None,
        content_type: str | None = None,
    ) -> dict[str, Any]:
        headers: dict[str, str] = {}
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        if idempotency_key:
            headers["Idempotency-Key"] = idempotency_key

        payload: bytes | None = raw
        if raw is not None:
            headers["Content-Type"] = content_type or "application/octet-stream"
        elif body is not None:
            headers["Content-Type"] = "application/json"
            payload = json.dumps(body).encode("utf-8")

        last: GraphOwlError | None = None
        for attempt in range(self.max_attempts):
            status, text = self._attempt(method, path, payload, headers)
            parsed = json.loads(text) if text else None
            if 200 <= status < 300 or status == 207:
                return parsed if isinstance(parsed, dict) else {}
            last = GraphOwlError(status, parsed)
            if not is_retryable(status):
                raise last
            self._sleep(backoff_seconds(attempt))
        assert last is not None
        raise last

    def _attempt(
        self, method: str, path: str, payload: bytes | None, headers: dict[str, str]
    ) -> tuple[int, str]:
        request = urllib.request.Request(
            f"{self.base_url}{path}", data=payload, headers=headers, method=method
        )
        try:
            with self._open(request) as response:
                return response.status, response.read().decode("utf-8")
        except urllib.error.HTTPError as error:
            # A 4xx/5xx arrives as an exception in urllib. Reading it back into a
            # status and a body is what lets the retry policy above be one
            # decision rather than a try/except in every caller.
            return error.code, error.read().decode("utf-8")


def entities_from_rows(rows: Iterable[dict[str, Any]]) -> list[EntityDraft]:
    """Map plain dictionaries onto drafts, ignoring what is not an entity field.

    Useful for the common adapter shape: a source yields dictionaries, and only
    some of their keys are catalog fields.
    """
    drafts: list[EntityDraft] = []
    for row in rows:
        known = {"kind", "name", "parentFqn", "parent_fqn", "description"}
        drafts.append(
            EntityDraft(
                kind=str(row["kind"]),
                name=str(row["name"]),
                parent_fqn=row.get("parentFqn") or row.get("parent_fqn"),
                description=row.get("description"),
                properties={k: v for k, v in row.items() if k not in known} or None,
            )
        )
    return drafts
