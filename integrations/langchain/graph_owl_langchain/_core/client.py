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
        #: The tool's own reason, undecorated — `str(self)` carries the
        #: `tool 'x' failed:` prefix for a human reading a log; a caller
        #: that hands this back to a model as a tool result (`tools.py`'s
        #: `_call`) wants the raw reason, not a sentence about itself.
        self.message = message


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
        response = self._call_method("tools/call", {"name": name, "arguments": arguments})
        if "error" in response:
            error = response["error"]
            raise GraphOwlToolError(name, error.get("message", "unknown error"))
        return self._unwrap(name, response.get("result") or {})

    def list_tools(self) -> list[dict[str, Any]]:
        """The live ``tools/list`` manifest — Slice D builds every exposed
        tool from this, never from a hardcoded name list, so a new MCP tool
        appears here without a release of this package.

        **Not the same unwrapping as ``call_tool``**: ``tools/list``'s own
        handler returns ``{"tools": [...]}`` directly as the JSON-RPC
        ``result`` — no ``content[0].text`` double-encoding, which is
        specific to ``tools/call``'s response construction
        (``graph_owl_mcp::jsonrpc::tool_response``). Reusing ``_unwrap``
        here would silently look for a shape this method never has.
        """
        response = self._call_method("tools/list", None)
        if "error" in response:
            error = response["error"]
            raise GraphOwlToolError("tools/list", error.get("message", "unknown error"))
        result = response.get("result") or {}
        tools: list[dict[str, Any]] = result.get("tools") or []
        return tools

    def _call_method(self, method: str, params: dict[str, Any] | None) -> dict[str, Any]:
        self._next_id += 1
        payload: dict[str, Any] = {"jsonrpc": "2.0", "id": self._next_id, "method": method}
        if params is not None:
            payload["params"] = params
        logger.debug("calling graph-owl method %s", method)
        return self._send(payload)

    def _unwrap(self, name: str, result: dict[str, Any]) -> dict[str, Any]:
        content = result.get("content") or []
        text = content[0].get("text", "") if content else ""
        inner: dict[str, Any] = json.loads(text) if text else {}
        if result.get("isError"):
            raise GraphOwlToolError(name, inner.get("error", "unknown error"))
        return inner

    def _send(self, payload: dict[str, Any]) -> dict[str, Any]:
        return self._attempt(payload, allow_refresh=True)

    def _attempt(self, payload: dict[str, Any], *, allow_refresh: bool) -> dict[str, Any]:
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
            # 401 covers `Unauthenticated`/`TokenExpired`/`TokenInvalid`
            # alike (`AppError::status`) — a client cannot tell which from
            # the status code, so it always tries the refresh once; if the
            # token was invalid rather than merely expired, the retry 401s
            # again and `allow_refresh=False` stops it there rather than
            # looping.
            if error.code == 401 and allow_refresh and self._principal.refresh is not None:
                self._principal = Principal(
                    token=self._principal.refresh(), refresh=self._principal.refresh
                )
                return self._attempt(payload, allow_refresh=False)
            raise self._as_tool_error(error) from None
        except (OSError, urllib.error.URLError) as error:
            raise GraphOwlConnectionError(self._endpoint, str(error)) from None
        parsed: dict[str, Any] = json.loads(raw)
        return parsed

    def _as_tool_error(self, error: urllib.error.HTTPError) -> GraphOwlToolError:
        """A non-2xx response from `/mcp` — reached before `jsonrpc::handle`
        ever runs (an unauthenticated request, a body axum itself rejects),
        never a JSON-RPC envelope. This server's own error responses are
        RFC 9457 problem+json (`AppError::into_response`), so ``detail`` is
        read from *that* shape, not from a JSON-RPC ``error`` member — the
        two are never the same response.
        """
        raw = error.read()
        try:
            problem = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            problem = {}
        detail = problem.get("detail") or problem.get("title") or f"HTTP {error.code}"
        return GraphOwlToolError("<transport>", str(detail))
