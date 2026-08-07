"""The core MCP client: one JSON-RPC POST per tool call, Bearer-authenticated.

Decision 1 (`43-framework-integrations.md`): MCP is the primary transport.
This talks to the same ``POST /mcp`` JSON-RPC-over-HTTP endpoint
``graph-owl-server``'s ``mcp_endpoint`` serves — the same Bearer-token
authentication as every other surface, since MCP is not a second auth
lowering here (``00j-language-boundaries.md``). Stdlib only, matching
``graph-owl-sdk``'s own choice for the same reason: a consumer's dependency
set is theirs to pick.
"""

from __future__ import annotations

import json
import logging
import urllib.error
import urllib.request
from collections.abc import Callable
from typing import Any

from graph_owl_langchain._core.principal import Principal

logger = logging.getLogger(__name__)


class GraphOwlConnectionError(RuntimeError):
    """The transport itself failed. Never carries the token — only the
    endpoint and the underlying reason, both of which are safe to log."""

    def __init__(self, endpoint: str, reason: str) -> None:
        super().__init__(f"could not reach graph-owl at {endpoint}: {reason}")
        self.endpoint = endpoint


class GraphOwlToolError(RuntimeError):
    """The server answered, but the tool call itself failed."""

    def __init__(self, tool: str, message: str) -> None:
        super().__init__(f"tool {tool!r} failed: {message}")
        self.tool = tool


class GraphOwlClient:
    """One JSON-RPC-over-HTTP call per tool invocation.

    ``principal`` has no default (decision 2) — a caller must always name
    who is asking; there is no ambient credential and no admin fallback.
    """

    def __init__(
        self,
        endpoint: str,
        principal: Principal,
        opener: Callable[[urllib.request.Request], Any] | None = None,
    ) -> None:
        self._endpoint = endpoint.rstrip("/")
        self._principal = principal
        self._open = opener or urllib.request.urlopen
        self._next_id = 0

    def __repr__(self) -> str:
        # The principal is deliberately absent: even its own redacted repr
        # is one more place a credential-adjacent object could regress into
        # printing something it should not.
        return f"GraphOwlClient(endpoint={self._endpoint!r})"

    def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        """Call one MCP tool, returning its unwrapped payload.

        Two distinct failure shapes, per ``graph_owl_mcp::jsonrpc``'s own
        doc — conflating them means an agent treats a policy denial as an
        outage, or a broken connection as "no such asset":

        - **JSON-RPC's own ``error`` member** — a malformed request or an
          unknown method. The call itself could not be made.
        - **``result.isError``** — the call *was* made and the tool refused,
          found nothing, or hit an unavailable backend. This is where
          ``NotFound``/``Unauthenticated``/``Refused``/etc. actually surface;
          MCP tool results carry their payload JSON-encoded a second time,
          inside ``result.content[0].text``, so unwrapping it costs a second
          parse — deliberately, so a client cannot mistake a *string* the
          tool legitimately returned for the envelope around it.
        """
        self._next_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self._next_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }
        logger.debug("calling graph-owl tool %s", name)
        response = self._send(payload)
        if "error" in response:
            error = response["error"]
            raise GraphOwlToolError(name, error.get("message", "unknown error"))
        return self._unwrap(name, response.get("result") or {})

    def _unwrap(self, name: str, result: dict[str, Any]) -> dict[str, Any]:
        content = result.get("content") or []
        text = content[0].get("text", "") if content else ""
        inner: dict[str, Any] = json.loads(text) if text else {}
        if result.get("isError"):
            raise GraphOwlToolError(name, inner.get("error", "unknown error"))
        return inner

    def _send(self, payload: dict[str, Any]) -> dict[str, Any]:
        url = f"{self._endpoint}/mcp"
        body = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=body,
            method="POST",
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {self._principal.token}",
            },
        )
        try:
            with self._open(request) as response:
                raw = response.read()
        except urllib.error.HTTPError as error:
            # A JSON-RPC error still arrives with an HTTP body worth reading —
            # `mcp_endpoint`'s own doc explains why a protocol-level error is
            # still HTTP 200, but a caller reusing this client against a
            # differently-configured proxy should not lose the body just
            # because *something* answered non-2xx.
            raw = error.read()
            parsed: dict[str, Any] = json.loads(raw) if raw else {}
            return parsed
        except (OSError, urllib.error.URLError) as error:
            raise GraphOwlConnectionError(self._endpoint, str(error)) from None
        parsed = json.loads(raw)
        return parsed
