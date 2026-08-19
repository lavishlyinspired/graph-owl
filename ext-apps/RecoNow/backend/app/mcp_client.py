"""Calling graph-owl's MCP tools over JSON-RPC.

graph-owl exposes 22 tools at `POST /mcp` — `query_graph`, `traverse`,
`explain_lineage`, `recall_memory`, `find_evidence`, `analytics`,
`resolve_entity`, `calculate_risk` and more. Nothing in Reco Now called any of
them.

**Kept thin on purpose.** This is a transport, not a client library: the tools
and their arguments belong to graph-owl, and a wrapper that enumerated them
here would be a second copy of a contract that already exists — one that goes
stale the first time a tool gains an argument.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any


class McpError(RuntimeError):
    """An MCP call did not return a result."""


def call(server: str, tool: str, arguments: dict[str, Any], *, timeout: int = 20) -> dict[str, Any]:
    """Invoke one tool, returning its parsed content.

    # Raises

    `McpError` on transport failure or a JSON-RPC error. Callers are expected
    to catch it and **say what they could not check** — an agent that cannot
    reach the graph and reports a clean result has told a reviewer something it
    never established.
    """
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }
    ).encode()
    request = urllib.request.Request(
        f"{server.rstrip('/')}/mcp", data=body, method="POST",
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            payload = json.loads(response.read() or b"{}")
    except urllib.error.URLError as exc:
        raise McpError(f"{tool} unreachable: {exc.reason}") from exc
    except (TimeoutError, json.JSONDecodeError) as exc:
        raise McpError(f"{tool} failed: {exc}") from exc

    if "error" in payload:
        raise McpError(f"{tool} refused: {payload['error']}")

    result = payload.get("result") or {}
    # MCP returns content as a list of typed blocks; the text block carries the
    # JSON these tools actually produce.
    for block in result.get("content") or []:
        if block.get("type") == "text":
            try:
                return json.loads(block["text"])
            except json.JSONDecodeError:
                return {"text": block["text"]}
    return result


__all__ = ["McpError", "call"]
