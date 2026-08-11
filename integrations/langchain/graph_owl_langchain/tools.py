"""``GraphOwlToolkit`` — every MCP tool exposed as a LangChain
`StructuredTool`, built from the live manifest (``tools/list``), never a
hand-maintained list.

Decision 5 (`43-framework-integrations.md`): no invented composite tools.
That property falls out of the design here for free — every tool this
class exposes is built from one entry in ``list_tools()``'s response, and
nothing else in this module contributes to the set, so there is no code
path that could add one the server never declared.

**A fifth finding, made building this**: Slice D's plan text asks for tool
errors to "carry the RFC 9457 ``type`` from Epic 1." They cannot — RFC 9457
is an HTTP-API-only convention (`graph-owl-server`'s `AppError`); MCP tool
failures carry `{"error": ..., "kind": "refused"/"unsupported"/
"unavailable"}` instead (`graph_owl_mcp::jsonrpc`'s own error construction),
a real but different shape. `GraphOwlToolError` from `_core.client` is the
actual analogue on this transport, and every tool here raises it as-is
rather than inventing an RFC 9457 field the server never sends over MCP.
"""

from __future__ import annotations

import json
from typing import Any

from langchain_core.tools import StructuredTool

from graph_owl_langchain._core.client import GraphOwlClient, GraphOwlToolError
from graph_owl_langchain._core.principal import Principal


class GraphOwlToolkit:
    """Every MCP tool the connected server declares, one-to-one.

    ``principal`` has no default (decision 2). Construction alone makes no
    network call — tools are built from a fresh ``tools/list`` each time
    :meth:`tools` is called, so a server-side manifest change is visible
    the next time an agent asks for the toolkit rather than only at
    process start.
    """

    def __init__(
        self,
        endpoint: str,
        principal: Principal,
        opener: Any = None,
    ) -> None:
        self._client = GraphOwlClient(endpoint=endpoint, principal=principal, opener=opener)

    def tools(self) -> list[StructuredTool]:
        return [self._build_tool(declaration) for declaration in self._client.list_tools()]

    def _build_tool(self, declaration: dict[str, Any]) -> StructuredTool:
        name = declaration["name"]
        client = self._client

        def _call(**kwargs: Any) -> str:
            # `GraphOwlToolError` means the call *reached* the server and
            # the tool refused, found nothing, or hit an unavailable
            # backend (`GraphOwlClient.call_tool`'s own doc: `isError`,
            # not a JSON-RPC-level failure) — a legitimate, recoverable
            # outcome an exploratory agent must see and route around, not
            # a reason to abort the whole investigation. Left uncaught
            # before this: LangGraph's tool node does not catch a generic
            # `RuntimeError`, so a single refused tool call — a normal
            # "not found" — crashed the entire run instead of becoming
            # the model's next observation. `GraphOwlConnectionError` is
            # deliberately not caught here: an unreachable server is not
            # something a different tool choice can route around.
            try:
                result = client.call_tool(name, kwargs)
            except GraphOwlToolError as refused:
                return json.dumps({"error": refused.message})
            return json.dumps(result)

        return StructuredTool.from_function(
            func=_call,
            name=name,
            description=declaration["description"],
            args_schema=declaration["inputSchema"],
            infer_schema=False,
        )
