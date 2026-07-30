"""The contract-drift test — Epic 16 Slice E.

Reads the committed ``openapi.json`` rather than a vendored copy: a vendored
spec would make this test agree with itself forever, which is the exact failure
it exists to catch.
"""

from __future__ import annotations

import json
from pathlib import Path

from graph_owl_sdk import CONTRACT_VERSION, REQUIRED_PATHS

CONTRACT = json.loads(
    (Path(__file__).resolve().parents[3] / "openapi.json").read_text(encoding="utf-8")
)


def test_the_sdk_is_pinned_to_the_contract_the_service_publishes() -> None:
    assert CONTRACT["info"]["version"] == CONTRACT_VERSION


def test_every_path_this_sdk_calls_is_still_declared() -> None:
    for path in REQUIRED_PATHS:
        assert path in CONTRACT["paths"]


def test_a_push_still_answers_207_and_a_batch_upload_202() -> None:
    """Both are 2xx, so a contract that swapped them would break the client
    silently — `push` treats 207 as per-item outcomes and `push_file` treats 202
    as a handle."""
    assert "207" in CONTRACT["paths"]["/ingest"]["post"]["responses"]
    assert "202" in CONTRACT["paths"]["/ingest/batch"]["post"]["responses"]
