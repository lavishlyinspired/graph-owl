"""The MCP contract this package was built against.

Kept as values, mirroring `graph-owl-sdk`'s own `contract.py` (Epic 16
Slice E) for the same reason: a package that silently keeps working
against a contract it was not built for is the failure mode, not the
success. `graph-owl-mcp` has no single version number the way the REST
OpenAPI contract does, so the contract here is the concrete thing this
package actually depends on — the tool names it calls.
"""

#: Every MCP tool name this package calls, anywhere. If a live server's
#: `tools/list` manifest ever stops declaring one of these, every class
#: that depends on it (`GraphOwlRetriever`, `GraphOwlCheckpointer`) is
#: silently broken the next time it runs — Slice F's own live-CI job checks
#: this before anything else.
REQUIRED_TOOLS = (
    "search_assets",
    "get_asset_context",
    "explain_lineage",
    "recall_memory",
    "record_memory",
)

#: The two JSON-RPC methods this package's transport (`_core.client`)
#: depends on directly, distinct from the tool names above.
REQUIRED_METHODS = (
    "tools/list",
    "tools/call",
)
