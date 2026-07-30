"""The end-to-end criterion: a push through the SDK against a running service.

Skipped unless one is pointed at, so ``pytest`` stays runnable without Docker.
``scripts/verify-sdks.sh`` is what supplies the URL.
"""

from __future__ import annotations

import os
import uuid

import pytest

from graph_owl_sdk import GraphOwlClient, IngestBuilder

BASE_URL = os.environ.get("GRAPH_OWL_BASE_URL")

pytestmark = pytest.mark.skipif(
    not BASE_URL, reason="GRAPH_OWL_BASE_URL is not set; no live service to push at"
)

SUFFIX = uuid.uuid4().hex[:6]


def client(**kwargs) -> GraphOwlClient:
    return GraphOwlClient(base_url=BASE_URL or "", **kwargs)


def test_a_hierarchy_and_an_edge_land_in_one_push() -> None:
    root = f"py-{SUFFIX}"
    request = (
        IngestBuilder()
        .entity("service", root)
        .entity("database", "core", parent_fqn=root)
        .entity("schema", "public", parent_fqn=f"{root}.core")
        .entity("table", "orders", parent_fqn=f"{root}.core.public")
        .entity("table", "shipments", parent_fqn=f"{root}.core.public")
        .edge(f"{root}.core.public.orders", f"{root}.core.public.shipments", "feeds")
        .build()
    )

    result = client().push(request)

    assert result["rejected"] == 0
    assert result["accepted"] == 6


def test_a_replayed_push_creates_nothing_the_second_time() -> None:
    key = str(uuid.uuid4())
    fixed = client(new_key=lambda: key)
    request = IngestBuilder().entity("service", f"py-idem-{SUFFIX}").build()

    first = fixed.push(request)
    second = fixed.push(request)

    assert second == first


def test_a_batch_file_uploads_and_polls_to_a_verdict() -> None:
    root = f"py-batch-{SUFFIX}"
    body = (
        f'{{"kind":"service","name":"{root}"}}\n'
        f'{{"kind":"database","name":"core","parentFqn":"{root}"}}\n'
        "this line is not json\n"
    )

    handle = client().push_file(body, "jsonl")
    job = client().await_job(handle["id"])

    assert job["state"] == "partial"
    assert job["accepted"] == 2
    assert job["failures"][0]["row"] == 3
