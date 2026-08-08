"""Epic 36 Slice C: `csv_adapter.py` against a real service — the two
properties `test_example_adapter.py`'s mocked-server tests cannot see:
genuine convergence (a re-run creates no new versions) and a real,
server-side per-item rejection (not just the adapter's own local
row-skipping, which never reaches the server at all).

Run via `scripts/verify-examples.sh`, same live-service pattern as
`scripts/verify-sdks.sh`. Skipped unless `GRAPH_OWL_TEST_ENDPOINT` is set,
matching this repo's other live-service suites.
"""

from __future__ import annotations

import csv
import importlib.util
import json
import os
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

import pytest

EXAMPLES = Path(__file__).resolve().parents[1] / "examples"
ADAPTER = EXAMPLES / "csv_adapter.py"

pytestmark = pytest.mark.skipif(
    "GRAPH_OWL_TEST_ENDPOINT" not in os.environ,
    reason="live-service test — set GRAPH_OWL_TEST_ENDPOINT (see scripts/verify-examples.sh)",
)


def load_adapter() -> Any:
    spec = importlib.util.spec_from_file_location("csv_adapter_live", ADAPTER)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules["csv_adapter_live"] = module
    spec.loader.exec_module(module)
    return module


def base_url() -> str:
    return os.environ["GRAPH_OWL_TEST_ENDPOINT"]


def search_one(name: str) -> dict:
    """`GET /assets/search` full-text-searches `name`/fqn/description — a
    leaf name, not a dotted FQN, is what actually matches (found running
    this against a real server while building `examples/agent-triage`: a
    dotted-FQN query returns no hits, a bare name does)."""
    url = f"{base_url()}/assets/search?q={urllib.parse.quote(name)}&limit=1"
    with urllib.request.urlopen(url, timeout=10) as response:
        body = json.loads(response.read())
    assert body["data"], f"no asset found for {name!r}: {body}"
    return body["data"][0]


def test_a_second_run_of_the_identical_csv_creates_no_new_versions(tmp_path) -> None:
    # A fresh, uniquely-named fixture per test run — the shared server
    # persists between test files in one verify-examples.sh run, and a
    # fixed FQN would pick up a stale version from an earlier run.
    import uuid

    suffix = uuid.uuid4().hex[:8]
    leaf = f"orders-{suffix}"
    rows = [
        {
            "service": f"live-svc-{suffix}",
            "database": "db",
            "schema": "public",
            "table": leaf,
            "description": "one row per order",
        }
    ]
    csv_path = tmp_path / "live.csv"
    with open(csv_path, "w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["service", "database", "schema", "table", "description"])
        writer.writeheader()
        writer.writerows(rows)

    adapter = load_adapter()

    first_exit = adapter.main([str(csv_path), "--base-url", base_url()])
    assert first_exit == 0
    first = search_one(leaf)
    first_version = (first["version"]["major"], first["version"]["minor"])

    second_exit = adapter.main([str(csv_path), "--base-url", base_url()])
    assert second_exit == 0
    second = search_one(leaf)
    second_version = (second["version"]["major"], second["version"]["minor"])

    assert second_version == first_version, (
        f"an unchanged re-run must produce zero new versions: {first_version} -> {second_version}"
    )


def test_a_batch_with_one_bad_row_still_lands_the_rest() -> None:
    """A real, server-side per-item rejection — not `csv_adapter.py`'s own
    local row-skipping (a row missing a level is silently dropped before
    any request is sent, already covered by the mocked-server suite).

    **Two whole-batch shapes turned out not to produce this at all, found
    by checking directly rather than assumed**: a duplicate FQN within one
    batch, and an unrecognised `kind` string, are each a whole-request
    `400` (every item refused together, no `results` array at all) — not
    the per-item `207` shape `report()` is written to read. The genuine
    per-item case is a row whose `parentFqn` resolves to nothing already
    in the catalog *and* nothing else in the same batch: that failure is
    local to one item, so the rest of the batch still lands."""
    import uuid

    from graph_owl_sdk import GraphOwlClient, IngestBuilder

    suffix = uuid.uuid4().hex[:8]
    ok_leaf = f"ok-service-{suffix}"
    builder = IngestBuilder()
    builder.entity("table", f"orphan-{suffix}", parent_fqn=f"nonexistent-parent-{suffix}")
    builder.entity("service", ok_leaf)

    client = GraphOwlClient(base_url=base_url())
    result = client.push(builder.build())

    assert result["accepted"] == 1, result
    assert result["rejected"] == 1, result
    assert "parent" in result["results"][0]["problem"], result

    ok = search_one(ok_leaf)
    assert ok["name"] == ok_leaf, "a rejection elsewhere in the batch must not abort the rest of it"

    # `report()`'s own exit-code behaviour on exactly this result shape is
    # already asserted directly, synthetically, in
    # `test_example_adapter.py::test_the_example_exits_non_zero_when_anything_was_rejected`
    # — combined with the real per-item response just proven above, the
    # property Slice C's own acceptance criterion asks for ("a
    # deliberately-invalid row is reported per-item without aborting") is
    # covered end to end: the server really does produce this shape, and
    # the adapter really does read it correctly. Threading a third,
    # CSV-path-only test through `main()` was attempted and dropped —
    # `csv_adapter.py`'s own hierarchy derivation always builds a
    # self-consistent parent chain from one row, so it structurally cannot
    # reach the server's per-item rejection path itself; forcing one
    # through the CSV surface would have tested something other than what
    # it claimed to.
    assert load_adapter().report(result) == 1
