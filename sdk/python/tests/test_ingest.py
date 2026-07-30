"""What the Python SDK decides before it ever reaches a server — Epic 16 Slice E.

Same behaviours as the TypeScript suite, deliberately: two SDKs that disagree
about chunking or idempotency are two different products with one name.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

import pytest

from graph_owl_sdk import (
    MAX_ITEMS_PER_PUSH,
    GraphOwlClient,
    GraphOwlError,
    IngestBuilder,
    IngestRequest,
    backoff_seconds,
    chunk,
    is_retryable,
)
from graph_owl_sdk.ingest import EntityDraft, entities_from_rows


def entities(count: int) -> list[EntityDraft]:
    return [EntityDraft(kind="service", name=f"svc-{n}") for n in range(count)]


# ---- chunking ----


def test_a_push_that_already_fits_is_one_request() -> None:
    assert len(chunk(IngestRequest(items=entities(3)))) == 1


def test_a_push_larger_than_the_ceiling_is_split() -> None:
    parts = chunk(IngestRequest(items=entities(MAX_ITEMS_PER_PUSH + 1)))

    assert len(parts) == 2
    assert len(parts[0].items) == MAX_ITEMS_PER_PUSH
    assert len(parts[1].items) == 1


def test_every_entity_is_sent_before_any_edge() -> None:
    """An edge whose endpoints landed earlier resolves; one sent first does not."""
    request = IngestBuilder().edge("a", "b", "feeds").build()
    request.items = entities(MAX_ITEMS_PER_PUSH + 5)

    parts = chunk(request)

    first_edge = next(i for i, part in enumerate(parts) if part.edges)
    last_items = max(i for i, part in enumerate(parts) if part.items)
    assert first_edge > last_items


def test_an_empty_push_is_still_one_request() -> None:
    """Silently doing nothing is the least debuggable answer there is."""
    assert len(chunk(IngestRequest())) == 1


def test_a_chunk_of_zero_is_refused_rather_than_looping_forever() -> None:
    with pytest.raises(ValueError):
        chunk(IngestRequest(items=entities(1)), limit=0)


# ---- retry policy ----


def test_what_the_server_said_it_could_not_do_now_is_retried() -> None:
    assert is_retryable(429)
    assert is_retryable(503)


def test_an_answer_that_can_never_change_is_not_retried() -> None:
    """409 means the key was used for different content. Looping hides a bug."""
    assert not is_retryable(409)
    assert not is_retryable(400)
    assert not is_retryable(404)


def test_backoff_is_exponential_up_to_a_cap() -> None:
    assert backoff_seconds(0, jitter=lambda: 1.0) == pytest.approx(0.2)
    assert backoff_seconds(1, jitter=lambda: 1.0) == pytest.approx(0.4)
    assert backoff_seconds(40, jitter=lambda: 1.0) == pytest.approx(30.0)


def test_retries_are_spread_out_rather_than_synchronised() -> None:
    assert backoff_seconds(3, jitter=lambda: 0.0) < backoff_seconds(3, jitter=lambda: 1.0)


# ---- the client ----


@dataclass
class Sent:
    path: str
    key: str | None
    body: Any


class FakeResponse:
    def __init__(self, status: int, body: dict[str, Any]) -> None:
        self.status = status
        self._body = json.dumps(body).encode()

    def read(self) -> bytes:
        return self._body

    def __enter__(self) -> "FakeResponse":
        return self

    def __exit__(self, *_: object) -> None:
        return None


def recorder(statuses: list[int], results: list[dict[str, Any]] | None = None):
    sent: list[Sent] = []
    calls = {"n": 0}

    def opener(request: Any) -> FakeResponse:
        sent.append(
            Sent(
                path=request.full_url,
                key=request.headers.get("Idempotency-key"),
                body=json.loads(request.data) if request.data else None,
            )
        )
        index = min(calls["n"], len(statuses) - 1)
        status = statuses[index]
        payload = (results or [{"accepted": 1, "rejected": 0, "results": []}])[
            min(calls["n"], len(results or [None]) - 1)
        ]
        calls["n"] += 1
        return FakeResponse(status, payload)

    return sent, opener


def client(opener: Any, keys: list[str] | None = None) -> GraphOwlClient:
    supply = list(keys or ["k1", "k2", "k3"])
    state = {"n": 0}

    def next_key() -> str:
        key = supply[min(state["n"], len(supply) - 1)]
        state["n"] += 1
        return key

    return GraphOwlClient(
        base_url="http://catalog.test",
        opener=opener,
        sleep=lambda _: None,
        new_key=next_key,
    )


def test_every_push_carries_an_idempotency_key() -> None:
    sent, opener = recorder([207])

    client(opener).push(IngestRequest(items=entities(1)))

    assert sent[0].key == "k1"


def test_one_key_is_reused_across_the_retries_of_a_chunk() -> None:
    """A key per *attempt* makes the retry a second push — the exact duplication
    the key exists to prevent, and the most common way this is got wrong."""
    sent, opener = recorder([503, 503, 207])

    client(opener).push(IngestRequest(items=entities(1)))

    assert len(sent) == 3
    assert {s.key for s in sent} == {"k1"}


def test_each_chunk_gets_its_own_key() -> None:
    sent, opener = recorder([207])

    client(opener).push(IngestRequest(items=entities(MAX_ITEMS_PER_PUSH + 1)))

    assert len(sent) == 2
    assert sent[0].key != sent[1].key


def test_an_answer_that_cannot_change_is_raised_not_looped() -> None:
    sent, opener = recorder([409])

    with pytest.raises(GraphOwlError):
        client(opener).push(IngestRequest(items=entities(1)))

    assert len(sent) == 1


def test_item_indexes_are_reported_against_the_submitted_list() -> None:
    """Three reports each starting at 0 would leave a client doing the
    arithmetic this SDK exists to hide."""
    _, opener = recorder(
        [207],
        [
            {"accepted": 0, "rejected": 1, "results": [{"index": 7, "status": 400}]},
            {"accepted": 0, "rejected": 1, "results": [{"index": 3, "status": 400}]},
        ],
    )

    merged = client(opener).push(IngestRequest(items=entities(MAX_ITEMS_PER_PUSH + 1)))

    assert [r["index"] for r in merged["results"]] == [7, MAX_ITEMS_PER_PUSH + 3]


# ---- the builder ----


def test_the_builder_assembles_entities_and_edges() -> None:
    request = (
        IngestBuilder()
        .entity("service", "payments")
        .entity("database", "core", parent_fqn="payments")
        .edge("payments.core", "payments", "feeds")
        .build()
    )

    assert len(request.items) == 2
    assert len(request.edges) == 1
    assert request.wire()["items"][1]["parentFqn"] == "payments"


def test_a_built_envelope_is_a_snapshot() -> None:
    builder = IngestBuilder().entity("service", "one")
    first = builder.build()

    builder.entity("service", "two")

    assert len(first.items) == 1


def test_an_absent_field_is_omitted_rather_than_sent_as_null() -> None:
    """A null description would overwrite a real one. Absence must stay absent."""
    wire = IngestBuilder().entity("service", "payments").build().wire()

    assert "description" not in wire["items"][0]
    assert "parentFqn" not in wire["items"][0]


def test_rows_map_unknown_keys_into_properties() -> None:
    """The common adapter shape: a source yields dictionaries and only some of
    their keys are catalog fields. Dropping the rest loses facts."""
    drafts = entities_from_rows([{"kind": "table", "name": "orders", "row_count": 41000}])

    assert drafts[0].properties == {"row_count": 41000}
