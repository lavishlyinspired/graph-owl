"""Epic 36 Slice B's own acceptance criteria, against a real live service —
mirrors `scripts/verify-sdks.sh`/`verify-langchain.sh`'s own shape: a unit
test of this module tests its own opinion of the MCP contract, only a live
round trip tests the contract itself.

Run via `scripts/verify-examples.sh`, which starts a real server (JWT auth
enabled — the filtered-view scenario needs a real, distinguishable
principal, not open mode) against a real Postgres first.
"""

from __future__ import annotations

import sys
import time
import uuid
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent.parent.parent / "sdk" / "python"))
from graph_owl_sdk import GraphOwlClient, IngestBuilder  # noqa: E402

from conftest import (  # noqa: E402
    allow_all_for_role,
    base_url,
    deny_fqn_prefix_for_role,
    grant_role,
    mint_token,
)
from mcp_client import McpClient  # noqa: E402
from triage import triage  # noqa: E402


def unique(name: str) -> str:
    # Each test run gets its own names — the server and its Postgres are
    # shared across the whole verify-examples.sh run, and a fixed name
    # would collide with a previous run against a reused container.
    return f"{name}-{uuid.uuid4().hex[:8]}"


@pytest.fixture(scope="module")
def service_name() -> str:
    return unique("triage-svc")


@pytest.fixture(scope="module", autouse=True)
def seed(service_name: str) -> None:
    # service -> database -> schema -> table, the containment chain the
    # catalog actually enforces (found by running this against a real
    # server, not assumed — a table directly under a service was refused:
    # "a `schema` is contained by a `database`, not a `service`").
    ingest = GraphOwlClient(base_url=base_url(), token=mint_token("admin"))
    builder = IngestBuilder()
    builder.entity("service", service_name)
    db = "db"
    db_fqn = f"{service_name}.{db}"
    builder.entity("database", db, parent_fqn=service_name)
    schema = "main"
    schema_fqn = f"{db_fqn}.{schema}"
    builder.entity("schema", schema, parent_fqn=db_fqn)

    safe = "safe-table"
    deprecated = "deprecated-table"
    successor = "successor-table"
    uncertified = "uncertified-table"
    filtered_schema = "filtered"
    filtered_schema_fqn = f"{db_fqn}.{filtered_schema}"
    visible_child = "visible-table"
    secret_child = "secret-table"

    builder.entity(
        "table",
        safe,
        parent_fqn=schema_fqn,
        properties={"lifecycle": "production", "certifiedBy": "data-team", "testsPassing": True},
    )
    builder.entity(
        "table",
        successor,
        parent_fqn=schema_fqn,
        properties={"lifecycle": "production", "certifiedBy": "data-team", "testsPassing": True},
    )
    builder.entity(
        "table",
        deprecated,
        parent_fqn=schema_fqn,
        properties={"lifecycle": "deprecated", "successor": f"{schema_fqn}.{successor}"},
    )
    builder.entity(
        "table",
        uncertified,
        parent_fqn=schema_fqn,
        properties={"lifecycle": "production"},
    )
    builder.entity("schema", filtered_schema, parent_fqn=db_fqn)
    builder.entity(
        "table",
        visible_child,
        parent_fqn=filtered_schema_fqn,
        properties={"lifecycle": "production", "certifiedBy": "data-team", "testsPassing": True},
    )
    builder.entity(
        "table",
        secret_child,
        parent_fqn=filtered_schema_fqn,
        properties={"lifecycle": "production", "certifiedBy": "data-team", "testsPassing": True},
    )

    result = ingest.push(builder.build())
    assert result["rejected"] == 0, result

    # Authorization denies by default (see `allow_all_for_role`'s own
    # comment) — `alice`, the default querying principal below, needs an
    # explicit baseline "can see everything" grant before any of the
    # safe/deprecated/uncertified scenarios mean anything, the same way a
    # real deployment needs a viewer policy before anyone but an admin can
    # use the catalog at all.
    grant_role("alice", ["viewer"])
    allow_all_for_role(unique("viewer-baseline"), "viewer")

    grant_role("contractor", ["contractor"])
    deny_fqn_prefix_for_role(
        unique("deny-secret"), "contractor", f"{filtered_schema_fqn}.{secret_child}"
    )
    # The policy cache invalidates on write, but a moment's grace against a
    # freshly-provisioned role avoids a flaky first read racing it.
    time.sleep(0.2)

    return {
        "safe": f"{schema_fqn}.{safe}",
        "successor": f"{schema_fqn}.{successor}",
        "deprecated": f"{schema_fqn}.{deprecated}",
        "uncertified": f"{schema_fqn}.{uncertified}",
        "filtered_schema": filtered_schema_fqn,
    }


def client(subject: str = "alice") -> McpClient:
    return McpClient(base_url=base_url(), token=mint_token(subject))


def test_a_healthy_certified_asset_is_reported_safe(seed: dict[str, str]) -> None:
    # A leaf name, not the full FQN — `search_assets` full-text-searches
    # names, and a dotted FQN does not tokenize the way a name does (found
    # running this against a real server: every dotted-FQN query returned
    # no hits). Safe within this one test: the container is recreated
    # fresh per run, so leaf names never collide with an earlier run's data.
    result = triage(client(), "safe-table")
    assert result.verdict.startswith("SAFE:"), result
    assert seed["safe"] in result.verdict, result
    assert result.tool_calls <= 3, result


def test_a_deprecated_asset_surfaces_its_successor(seed: dict[str, str]) -> None:
    result = triage(client(), "deprecated-table")
    assert result.verdict.startswith("NOT SAFE:"), result
    assert "deprecated" in result.verdict.lower(), result
    assert seed["successor"] in result.verdict, (
        f"the successor must be named, not just 'deprecated': {result}"
    )
    assert result.tool_calls <= 3, result


def test_an_uncertified_asset_is_reported_not_safe(seed: dict[str, str]) -> None:
    result = triage(client(), "uncertified-table")
    assert result.verdict.startswith("NOT SAFE:"), result
    assert "certif" in result.verdict.lower(), result
    assert result.tool_calls <= 3, result


def test_a_principal_with_a_filtered_view_is_told_so_not_given_a_false_all_clear(
    seed: dict[str, str],
) -> None:
    result = triage(client("contractor"), seed["filtered_schema"])
    assert result.verdict.startswith("PARTIAL VIEW:"), (
        f"a filtered principal must never be told the asset is SAFE or NOT SAFE "
        f"as if their view were complete: {result}"
    )
    assert result.tool_calls <= 3, result


def test_an_unfiltered_principal_sees_the_same_schema_without_the_partial_flag(
    seed: dict[str, str],
) -> None:
    # The negative control: the same asset, an unrestricted principal, no
    # PARTIAL VIEW verdict — proving the flag is about *this* principal's
    # policy, not something structurally true of the schema itself.
    result = triage(client("alice"), seed["filtered_schema"])
    assert not result.verdict.startswith("PARTIAL VIEW:"), result


def test_a_nonexistent_asset_is_reported_not_found_not_asserted_absent_or_unsafe() -> None:
    result = triage(client(), unique("no-such-table"))
    assert result.verdict.startswith("NOT FOUND:"), result
