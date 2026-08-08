#!/usr/bin/env python3
"""An agent answering "is this table safe to build on?" using MCP alone —
Epic 36 Slice B.

**Exercises** Epic 14's read tools (search, asset context, memory recall),
trust summaries and gaps, policy filtering, and token budgets. Every
question makes exactly three tool calls — `search_assets` to resolve a name
to a real asset, `get_asset_context` for the trust signal that actually
answers the question, and `recall_memory` for any recorded institutional
notes — a fixed, low, and testable proxy for whether Epic 14's tools are
task-shaped rather than endpoint-shaped (`36-reference-apps.md`'s own
framing: if answering this took five calls, the tool surface needs
changing, not the app).

Run it:

    python triage.py --base-url http://localhost:8080 "orders table"
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass, field

from mcp_client import McpClient


@dataclass
class TriageResult:
    verdict: str
    reasons: list[str] = field(default_factory=list)
    tool_calls: int = 0

    def __str__(self) -> str:
        lines = [self.verdict]
        lines.extend(f"  - {reason}" for reason in self.reasons)
        return "\n".join(lines)


def triage(client: McpClient, query: str) -> TriageResult:
    """Three tool calls, always — see this module's own doc for why a
    fixed count rather than "as many as needed"."""
    calls = 0

    search = client.call_tool("search_assets", {"query": query, "limit": 1})
    calls += 1
    if not search["hits"]:
        return TriageResult(
            verdict=f"NOT FOUND: no asset matches {query!r}",
            reasons=["search_assets returned no hits — this is not the same as an asset that exists but is hidden"],
            tool_calls=calls,
        )

    fqn = search["hits"][0]["fullyQualifiedName"]

    context = client.call_tool("get_asset_context", {"fullyQualifiedName": fqn})
    calls += 1

    # A partial view is not a complete "safe" and not a complete "unsafe" —
    # it is its own answer, stated as such rather than silently upgraded to
    # either (`36-reference-apps.md`'s own acceptance criterion: "the
    # partially-visible case states its view is filtered rather than
    # asserting absence").
    if context["policyFiltered"]:
        memories = client.call_tool("recall_memory", {"fullyQualifiedName": fqn})
        calls += 1
        return TriageResult(
            verdict=f"PARTIAL VIEW: {fqn} — your access is filtered, this assessment may be incomplete",
            reasons=[
                "get_asset_context reported policyFiltered=true",
                f"{len(memories)} recorded note(s) visible under this filtered view",
            ],
            tool_calls=calls,
        )

    trust = context["trust"]
    memories = client.call_tool("recall_memory", {"fullyQualifiedName": fqn})
    calls += 1

    lifecycle = trust["lifecycle"]
    if lifecycle["state"] == "deprecated":
        successor = lifecycle.get("successor")
        reason = f"lifecycle is deprecated{f', successor is {successor}' if successor else ' with no recorded successor'}"
        return TriageResult(
            verdict=f"NOT SAFE: {fqn} is deprecated" + (f" — use {successor} instead" if successor else ""),
            reasons=[reason, f"{len(memories)} recorded note(s)"],
            tool_calls=calls,
        )

    certification = trust["certification"]
    if certification["state"] != "certified":
        return TriageResult(
            verdict=f"NOT SAFE: {fqn} is not certified",
            reasons=[f"certification state is {certification['state']!r}", *[f"gap: {g}" for g in trust["gaps"]]],
            tool_calls=calls,
        )

    if trust["quality"] == "unhealthy":
        return TriageResult(
            verdict=f"NOT SAFE: {fqn} has failing quality tests",
            reasons=["trust.quality is unhealthy"],
            tool_calls=calls,
        )

    return TriageResult(
        verdict=f"SAFE: {fqn} is certified and in production",
        reasons=[
            f"certified by {certification.get('by', 'unknown')}",
            f"quality: {trust['quality']}",
            f"{len(memories)} recorded note(s)",
        ],
        tool_calls=calls,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("query", help="a table name or description to search for")
    parser.add_argument("--base-url", default="http://localhost:8080")
    parser.add_argument("--token", default=None, help="a bot principal's token")
    args = parser.parse_args(argv)

    client = McpClient(base_url=args.base_url, token=args.token)
    result = triage(client, args.query)
    print(result)
    print(f"\n({result.tool_calls} tool calls)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
