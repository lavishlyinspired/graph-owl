"""A minimal MCP client — stdlib only, matching graph_owl_sdk's own choice
for the same reason: a consumer's dependency set is theirs to pick, and a
reference application depending on a third-party HTTP library would be
demonstrating that library, not graph-owl's API.

One JSON-RPC POST per tool call against ``POST /mcp`` — the same endpoint
``graph-owl-server``'s ``mcp_endpoint`` serves. No ``initialize`` handshake
is required first: the server's own dispatch matches on method name per
request, with no session state gating ``tools/call`` on a prior
``initialize`` (checked by reading ``graph-owl-mcp``'s ``jsonrpc.rs``
before writing this, not assumed).
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any


class McpError(RuntimeError):
    """A tool call reached the server and was refused, not found, or
    otherwise failed — as opposed to a transport failure, which raises
    :class:`McpConnectionError` instead. An agent that conflates the two
    treats a policy denial as an outage."""


class McpConnectionError(RuntimeError):
    """The transport itself failed — the server could not be reached at
    all, distinct from a well-formed refusal."""


class McpClient:
    """One HTTP POST per tool call, Bearer-authenticated if a token is
    given."""

    def __init__(self, base_url: str, token: str | None = None) -> None:
        self._endpoint = base_url.rstrip("/") + "/mcp"
        self._token = token
        self._next_id = 0

    def call_tool(self, name: str, arguments: dict[str, Any]) -> Any:
        """Call one MCP tool and return its unwrapped payload.

        Two distinct failure shapes, per ``graph_owl_mcp::jsonrpc``'s own
        design: the JSON-RPC ``error`` member (the call itself could not be
        made — a malformed request or unknown method) and ``result.isError``
        (the call *was* made and the tool refused, found nothing, or hit an
        unavailable backend — this is where "not found or not visible"
        actually surfaces). Conflating them would make an agent treat a
        policy denial as a transport outage.
        """
        self._next_id += 1
        body = json.dumps(
            {
                "jsonrpc": "2.0",
                "id": self._next_id,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            }
        ).encode("utf-8")

        headers = {"content-type": "application/json"}
        if self._token:
            headers["authorization"] = f"Bearer {self._token}"

        request = urllib.request.Request(self._endpoint, data=body, headers=headers, method="POST")
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                envelope = json.loads(response.read())
        except urllib.error.URLError as exc:
            raise McpConnectionError(f"could not reach graph-owl at {self._endpoint}: {exc}") from exc

        if "error" in envelope:
            raise McpConnectionError(f"tool call {name!r} was not accepted: {envelope['error']}")

        result = envelope["result"]
        # MCP tool results carry their payload JSON-encoded a second time,
        # inside `content[0].text` — unwrapping costs a second `json.loads`.
        payload = json.loads(result["content"][0]["text"])
        if result["isError"]:
            raise McpError(f"tool {name!r} failed: {payload.get('error', payload)}")
        return payload
