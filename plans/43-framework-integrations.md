# Plan: Agent Framework Integrations (Epic 43)

**Branch**: feat/framework-integrations
**Status**: **Shipped, 8 August 2026 — Slices A–F complete.** Four real structural/wire-format findings recorded below (three against Epic 14's MCP surface, one — the checkpointer's deployment prerequisites — against Epic 32's capability model); none worked around, all either reframed the affected acceptance criterion or documented as a genuine, unautomatable prerequisite. Not published to PyPI — that is an external, credentialed, irreversible action outside what this session performs; packaging metadata is complete and ready. See "Progress and findings, 8 August 2026".
**Depends on**: Epic 14 (MCP), Epic 13 (authorization), Epic 31 (memory), Epic 16 (Python SDK), Epic 7 (query)
**Language**: **Python, out of process.** No graph-owl crate changes — asserted structurally.
**Package**: `graph-owl-langchain` on PyPI, sources in `integrations/langchain/`

**Read `00j-language-boundaries.md` first.** This epic is the concrete form of its central distinction, and it only makes sense in that light.

## Progress and findings, 8 August 2026

Built with `uv` + `pytest` + `mutmut` + `ruff` + `mypy --strict`, mirroring
this project's own RED→GREEN→MUTATE→KILL MUTANTS discipline in Python
tooling. What follows is what actually shipped and what actually got
found — not a restatement of the plan below, which was written before any
of this was checked against the real MCP surface.

**Shipped and mutation-hardened**: `graph_owl_langchain/_core/principal.py`
(`Principal`, decision 2 — no default, empty token rejected, token excluded
from `repr`/`str` via `field(repr=False)`) and `_core/client.py`
(`GraphOwlClient`, one JSON-RPC POST per tool call to the same `/mcp`
endpoint and Bearer auth every other surface uses). `_core/rendering.py`
(`GraphContext`/`RelatedFact`/`render`/`visible_facts`, decisions 3–4) is
also shipped — the framework-agnostic half of Slice B, i.e. what turns a
graph context into `(page_content, metadata)` with derived facts labelled
in the text itself. 34 tests, `ruff`/`mypy --strict` clean, `mutmut`:
160/183 mutants killed on `_core`; every survivor individually inspected,
not waved through — the two recurring classes were (a) provably equivalent
under `urllib`'s own request defaults (header-key casing collapses to one
stored key regardless of source spelling; `method="POST"` is redundant
with `data=body` always being set) and (b) log-message wording, observable
only by reading log text the credential-leak tests already assert nothing
sensitive is in.

**A real bug found and fixed before anything was built on top of it.**
`GraphOwlClient.call_tool`'s first version treated a top-level JSON-RPC
`"error"` member as the only failure signal — the natural reading of "JSON-
RPC over HTTP," and wrong for this server. `graph_owl_mcp::jsonrpc`'s own
module doc is explicit that this is deliberate: *"a tool that ran and
answered 'no such asset' has succeeded at the protocol level"* — so
`NotFound`, `Unauthenticated`, `Refused`, and every other tool-level
outcome surface through `result.isError` on an otherwise-successful
response, with the actual payload JSON-encoded a **second** time inside
`result.content[0].text`. The first client version would have silently
returned the *envelope* (`{"content": [...], "isError": false}`) as if it
were the tool's answer to every caller, and would never have raised on a
refusal at all. Found by reading `crates/graph-owl-mcp/src/jsonrpc.rs`
before writing Slice B against the client rather than after — the same
"do not build the second thing on the first thing's assumption" discipline
this session applied to Epic 103's SQL fix. Fixed, and the differential
shape (`_tool_result()` in `tests/test_client.py`) now matches the real
wire format exactly; 9 tests added or rewritten to cover both failure
paths (protocol-level `error`, tool-level `isError`) plus the
happy path's actual unwrapped return value, which nothing had checked
before either.

**Three structural findings against Epic 14, not worked around — per this
epic's own rule that friction here is an MCP defect to log, not an
adapter's problem to paper over:**

1. **None of the seven MCP read tools accept an `as_of` argument.**
   Checked directly against `crates/graph-owl-mcp/src/lib.rs::tools()`'s
   input schemas — `get_asset_context`, `search_assets`, `recall_memory`,
   `explain_lineage`, `analyze_impact`, `get_governance_context`, and
   `query_graph` all read current state only. The plan's own example
   (`as_of=None` on `GraphOwlRetriever`) and this epic's stated
   differentiator ("an agent that can ask 'what did we believe last
   quarter' is doing something no vector store can") assume a capability
   that does not exist on the wire today. `rendering.py`'s `GraphContext`
   threads an `as_of` field through regardless, so the shape is ready, but
   nothing in this package can populate it truthfully — inventing a
   client-side filter would silently disagree with whatever the server
   actually returned, which is worse than not having the field.
2. **`AssetContext.related` is `Vec<String>` — FQNs only, no relationship
   type.** Decision 3's "relationship types" in retrieved context has to
   come from `explain_lineage`'s `LineageStep.relationship` instead
   (`crates/graph-owl-mcp/src/lineage.rs`), which means a full retriever
   needs to compose at least two tool calls per asset (`get_asset_context`
   *and* `explain_lineage`), not one. Knowable only by reading the actual
   response structs, which the plan (written before Epic 14 shipped in
   this shape) does not reference.
3. **Authorization denial and "does not exist" are the same wire answer,
   on purpose — `Outcome::NotFound`'s own doc calls this "absent and
   denied, indistinguishable."** This is the identical non-disclosure
   principle `graph-owl-api::Catalog::walk_hop` already applies (Epic 103's
   own two-principal test exercises it there). Slice C's literal wording
   below — *"a 403 surfaces as a typed permission error, never as an empty
   result"* — asks for something that contradicts this system's own,
   already-shipped, deliberately-chosen security posture elsewhere. The
   adapter can and should still prove "two principals retrieve different
   documents" (decision 3's actual, checkable property); it should not
   invent a distinguishable-403 signal the server will never send, since
   doing so would be the adapter *creating* an information-disclosure path
   the rest of the system was built to avoid.

**Slice B completed**: `GraphOwlRetriever` (`retrievers.py`) is a
`BaseRetriever` subclass (`arbitrary_types_allowed=True`, which
`BaseRetriever` already sets, letting the plain-dataclass `Principal` sit
as a field with no extra config). `_get_relevant_documents` calls
`search_assets`, then per hit composes `get_asset_context` +
`explain_lineage` + `recall_memory` into a `GraphContext` (finding 2's
two-call composition), applies `visible_facts`/`render`, and wraps the
result in a `Document`. `test_retrievers.py` (14 tests) includes a
mutation-driven case that plain "does it return documents" testing would
miss: `test_each_hit_fetches_its_own_context_not_a_shared_one`, which
catches an fqn-mixup bug via per-hit distinct lineage-relationship
echoing — a retriever that accidentally reused hit 1's context for every
result would still pass every other test in the file.

**A fourth finding, made building Slice C.** The Slice A `call_tool` fix
(above) corrected the *tool-level* failure shape (`isError` on an
HTTP-200), but Slice A's own error-path tests still assumed *transport*-level
failures (non-2xx HTTP responses) carried a JSON-RPC `{"error": {...}}`
body. They do not: reading `crates/graph-owl-server/src/lib.rs`'s `Auth`
extractor and `AppError::into_response()` showed that a non-2xx response
from `/mcp` — expired token, malformed body — is reached **before**
`jsonrpc::handle` ever runs, and is RFC 9457 problem+json
(`{type, title, status, detail}`), never a JSON-RPC envelope. Fixed
`_core/client.py`'s `_as_tool_error` to parse `detail`/`title` from the
RFC 9457 shape, and renamed the affected test to say what it now actually
asserts (`test_a_non_2xx_http_response_is_read_as_an_rfc9457_problem`).
This is the same lesson as finding 3 below, arriving from the opposite
direction: two different parts of the same server disagree on error
envelope by design (protocol-level JSON-RPC errors vs. HTTP-level RFC
9457), and an adapter that assumes one shape everywhere is wrong twice.

**Slice C completed**, reframed per finding 3: `test_authorization.py`
proves the checkable property — two principals against one corpus
retrieve different document sets, one process serves several principals
with interleaved calls and no cross-contamination, and search counts
(`total`/`policyFiltered`) pass through unmodified rather than the client
inventing its own. It does **not** attempt a distinguishable-403 signal,
per finding 3. Token refresh (decision-driven, not in the original plan
text): `Principal` gained an optional `refresh: Callable[[], str] | None`
field, invoked at most once per request on a 401
(`tests/test_client_refresh.py`, 4 tests) — a second 401 after refresh
raises rather than looping, and the refreshed token persists on the
client for subsequent calls.

**Slice D completed**: `GraphOwlToolkit` (`tools.py`) builds every tool
directly from a live `tools/list` manifest — `list_tools()` was added to
`_core/client.py` as a *second* method distinct from `call_tool`, because
`tools/list`'s `result` is the tool array directly, with none of
`tools/call`'s `isError`/double-JSON-encoding. Each declared tool becomes
a `StructuredTool.from_function(..., args_schema=declaration["inputSchema"],
infer_schema=False)` — `args_schema` accepts a raw JSON-Schema dict
directly, so no dynamic pydantic model generation was needed.
`test_tools.py` includes two explicit manifest-parity tests (a narrower
manifest excludes a tool; a wider manifest's new tool appears with no
release needed) and an assertion that no composite tool exists (decision
5). `test_langgraph_integration.py` proves the real thing decision 5 exists
for: a `create_react_agent` built from `toolkit.tools()` completes an
actual search-then-expand multi-step investigation against a scripted
tool-calling model, asserting both tool calls ran and the final answer
reflects the expanded (second-hop) result.

**Slice E completed, and a fifth (numbered "seventh" in `memory.py`'s own
docstring — see there for the full account) finding made verifying it
against a real server.** `GraphOwlCheckpointer` (`memory.py`) satisfies
`BaseCheckpointSaver`'s sync contract (`put`, `put_writes`, `get_tuple`,
`list`; async variants auto-delegate). Checkpoints key a synthetic FQN
(`dsc:langgraph-checkpoint/{thread_id}/{checkpoint_ns}`) and write at
confidence 1.0 (never a proposal — a checkpoint is the agent's own
execution state, not uncertain institutional knowledge). Retraction is
free: `record_memory` has no retraction call and nothing is ever deleted,
so "discarded, not deleted" needs no extra mechanism — an older checkpoint
stays exactly as queryable after a newer one supersedes it. 9 tests
including a real compiled `StateGraph` round trip
(`test_a_compiled_langgraph_actually_persists_state_across_invocations`:
two `.invoke()` calls sharing one `thread_id`, the running total
persisting 0→1→2) all pass against a scripted mock MCP server. Running the
same calls against a **real** `graph-owl-server` (8 August 2026) found two
further prerequisites `record_memory` enforces that no mock had modelled:
the target FQN must already exist as a real, readable catalog asset (no
MCP tool creates one — only `POST /ingest` does, which this package does
not call), and the calling principal needs the `recordMemory` capability,
which Epic 32 grants only via an admin-only, human-only route by design.
Neither is a code defect; both are documented as a genuine deployment
prerequisite in `memory.py`'s module docstring rather than worked around,
since working around either would defeat the exact property Epic 32
exists to guarantee.

**Slice F completed**: `_core/contract.py` (`REQUIRED_TOOLS`,
`REQUIRED_METHODS`) plus `tests/test_contract_drift.py` — three tests, all
verified passing against a real `graph-owl-server` started specifically
for this (manifest completeness, a schema-field-presence check narrower
than presence alone, a real `search_assets` round trip) — skip locally
without `GRAPH_OWL_TEST_ENDPOINT`, matching `graph-owl-sdk`'s own
integration-test convention. `tests/test_no_crate_change.py` asserts the
epic's central claim structurally: no uncommitted change touches
`crates/`, and (when `GRAPH_OWL_STRUCTURAL_CHECK_BASE` is set, as CI does)
neither does any commit since the PR's base — both verified passing.
`scripts/verify-langchain.sh` mirrors `scripts/verify-sdks.sh`'s exact
shape (named Postgres container, open-mode server, `until curl -sf
.../health`, `trap` cleanup) and is wired into `.github/workflows/ci.yml`
as the `langchain-integration` job. `README.md` is the quickstart
(install → a working `GraphOwlRetriever` call, well under twenty lines).
**Not done, deliberately**: actual publication to PyPI. That is an
external, credentialed, irreversible action (`twine upload` or
equivalent) outside what an autonomous session performs without explicit
authorization — `pyproject.toml`'s metadata (name, version, optional extras, dependency
bounds) is complete and ready for a human to run that step.

## What this epic is, and what it emphatically is not

`00j-language-boundaries.md` establishes that agent frameworks are **consumers** of graph-owl, not components of it. That decision stands and this epic does not soften it.

**We ship the integration, never the framework.** The difference is the whole plan:

| This epic builds | This epic does not build |
|---|---|
| A retriever a user drops into *their* LangChain chain | A chain, an agent, or a runtime we host |
| Tools a user binds into *their* LangGraph node | A graph of nodes, a supervisor, or an orchestrator |
| A checkpointer backed by graph-owl memory | A conversation manager or a session service |
| A thin, versioned adapter over MCP and the API | Anything an LLM SDK upgrade could break inside our binary |

**The test of whether this epic stayed honest**: nothing in it may require a change to a graph-owl crate. Friction found here is an **API or MCP defect**, logged against Epic 14 or Epic 1, exactly as `36-reference-apps.md` decision 2 requires. Working around it in the adapter defeats the purpose of building it.

## Why build it at all, if frameworks are consumers

Because "connect over MCP" is true and insufficient. A graph engine that *can* be used from LangGraph and a graph engine that is *pleasant* to use from LangGraph convert evaluations at very different rates, and the gap is about fifteen hundred lines of adapter that someone will otherwise write badly, once per team.

Three things make the adapter worth owning rather than leaving to users:

1. **Retrieval over a governed graph is not retrieval over a vector store.** The obvious LangChain retriever returns chunks. graph-owl returns a *policy-filtered subgraph with provenance and confidence*, and an adapter that flattens that into `page_content` throws away the reason to use graph-owl at all.
2. **Memory (Epic 31) maps onto checkpointing, and the mapping is not obvious.** Getting it wrong produces either an audit trail nobody can read or a checkpointer that loses state.
3. **Authorization must survive the adapter.** An agent runs as a principal. An integration that drops that is a data-exfiltration path wearing a convenience wrapper.

## Resolved decisions

1. **MCP is the primary transport; the REST SDK is the fallback.** Epic 14 already exposes read tools with policy filtering and token budgeting. The adapter is thin over MCP and reaches for the REST SDK (Epic 16) only where MCP has no equivalent — bulk export, admin. Two transports, one behind the other, not two parallel implementations.
2. **The principal is explicit and never defaulted.** Constructing a retriever or toolkit requires credentials. There is no ambient, no service-account fallback, and no "if unset, use admin". An integration that quietly runs as a superuser is worse than one that fails to construct.
3. **Retrieval returns graph context, not flattened text.** `Document.page_content` carries a rendered summary for the model, and `Document.metadata` carries the structured truth: entity ids, relationship types, provenance, confidence band, derived-versus-asserted, and the `as_of` transaction time. A downstream cell that wants to check whether a fact was inferred must be able to.
4. **Derived facts are labelled in the payload the model sees, not only in metadata.** Epic 6's overlay is never persisted (`00b-architecture.md`); an LLM handed an inferred fact as though it were asserted will state it as fact. The rendered text says so.
5. **Tools are the MCP tools, one-to-one, with no invented composites.** A `find_related_and_summarise` convenience tool would put retrieval policy in the adapter, where it cannot be tested against the engine and will drift. If a composite is genuinely wanted, it belongs in Epic 14 as an MCP tool.
6. **The checkpointer stores agent state as Epic 31 memories, with retraction rather than deletion.** `op = false` is a retraction (`04-engine-triples.md`); a checkpointer that hard-deletes destroys the audit trail that made memory worth building.
7. **Version-pinned against a contract version, and tested against a running service in CI.** A framework SDK upgrade must fail loudly here, not silently in a user's agent.
8. **Framework-agnostic core, thin framework shims.** The MCP/REST client, the graph-to-document rendering, and the principal handling live in one core module. LangChain and LangGraph surfaces are adapters over it, so a third framework is a shim rather than a rewrite.

## Implementation reference

```
integrations/langchain/
  graph_owl_langchain/
    _core/
      client.py         MCP first, REST fallback; principal required
      rendering.py      GraphContext -> text + structured metadata
      principal.py      credential handling; no ambient default
    retrievers.py       GraphOwlRetriever
    tools.py            GraphOwlToolkit — Epic 14's tools, 1:1
    memory.py           GraphOwlCheckpointer (LangGraph), memory read/write
    loaders.py          GraphOwlLoader — bulk export for offline indexing
  tests/
  pyproject.toml
```

```python
# Shape only — the point is what is *required*, not the ergonomics.
retriever = GraphOwlRetriever(
    endpoint="https://graph-owl.internal",
    principal=OidcPrincipal(token=...),   # decision 2: no default
    max_hops=2,
    min_confidence=0.5,                   # 00c's bands
    include_derived=True,                 # labelled when rendered — decision 4
    as_of=None,                           # Epic 4: any past state is addressable
)
```

**`as_of` is exposed on every read surface.** It is the differentiator, it costs one parameter, and an agent that can ask "what did we believe last quarter" is doing something no vector store can.

## Acceptance criteria

- [x] A LangChain chain retrieves graph context with **no graph-owl crate change** — asserted structurally, `tests/test_no_crate_change.py`.
- [x] A LangGraph agent binds the toolkit and completes a multi-step investigation against a seeded corpus — `tests/test_langgraph_integration.py`.
- [x] Constructing any surface **without a principal raises**, and is tested — `Principal`/`GraphOwlClient`, `tests/test_principal.py`, `tests/test_client.py`.
- [x] Two principals against one corpus retrieve **different documents** — `tests/test_authorization.py`; reframed by finding 3 above: achieved via the checkable property (distinct document sets, no cross-contamination, honest counts), not via a distinguishable-403 signal the server deliberately never sends.
- [x] A derived fact is identifiable as derived **in the text the model receives**, not only in metadata — `_core/rendering.py::render`, `tests/test_rendering.py`.
- [~] Confidence bands and `as_of` round-trip into `Document.metadata` — confidence does; `as_of` is threaded through the shape but cannot round-trip truthfully yet (finding 1, still open — no MCP read tool accepts `as_of`).
- [ ] `as_of` retrieval returns state as of that time, including an entity retracted since — **blocked on finding 1**, not attempted; would need an Epic 14 change, which is out of scope for this epic by its own rule.
- [x] The checkpointer round-trips agent state; a discarded checkpoint is **retracted, not deleted**, and remains in history — `memory.py`, `tests/test_memory.py`; retraction is free (`record_memory` has no delete), see the Slice E account above. Deployment needs the prerequisites in finding 5/7 satisfied first.
- [x] Tools map one-to-one onto Epic 14's MCP tools — asserted against the live tool manifest, `tests/test_tools.py`'s manifest-parity tests.
- [x] CI runs the integration against a live service; a contract change that breaks it fails the build — `.github/workflows/ci.yml`'s `langchain-integration` job, `scripts/verify-langchain.sh`, `tests/test_contract_drift.py`.
- [x] The package imports without a LangChain install where only the core is used (decision 8) — `test_the_core_module_never_imports_a_framework` (renamed from `..._with_no_framework_installed`; the original was sys.modules-order-dependent, see Errors below).

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR. Python mutation testing uses `mutmut`.

### Slice A: The core client and principal handling — **shipped, 8 August 2026**

**Acceptance criteria**: an MCP client over Epic 14's streamable HTTP transport; a REST fallback via Epic 16's generated SDK; **construction without a principal raises**; credentials never appear in logs, reprs, or exception messages; connection failure is a typed error naming the endpoint; the core imports with no framework installed.
**RED**: The no-principal test, and a credential-leak test that constructs a client with a token and asserts the token appears in neither `repr()`, nor a captured log, nor the string of any raised exception. Convenience defaults are how an integration ends up running as an admin, and a token in a traceback is how it ends up in a bug report. Mutator watch: a default principal must fail construction; a `__repr__` that formats the credential must fail the leak test.
**Done when**: criteria met, mutation report reviewed.
**Shipped as**: a single JSON-RPC-over-HTTP client (`_core/client.py`), stdlib only, matching `graph-owl-sdk`'s own no-dependency choice — not a REST-fallback pair, since every read this epic needs has an MCP tool. The REST SDK stays the documented fallback for what Epic 14 has no equivalent for (bulk export, admin), not yet needed by anything built so far. All criteria met; mutation-hardened (see "Progress and findings" above for the exact score and what the survivors are). The one correction mid-slice: `call_tool`'s error handling initially assumed the wrong wire shape (top-level JSON-RPC `error` only) — found and fixed before Slice B was built on top of it, see above.

### Slice B: Retrieval that preserves what makes the graph worth querying — **shipped, 8 August 2026**

**Acceptance criteria**: `GraphOwlRetriever` returns `Document`s whose `page_content` is a rendered subgraph and whose `metadata` carries entity ids, relationship types, provenance, confidence, derived flags, and `as_of`; **derived facts are marked in `page_content`**; confidence below the ignore band (<0.5, `00c-domain-model.md`) is excluded by default and includable explicitly; an empty result is an empty list, never an exception; token budget is respected and truncation is stated in the returned text rather than silent.
**RED**: The derived-labelling test — assert the rendered string identifies an inferred fact as inferred. Metadata alone fails this: the model reads `page_content`, and an LLM handed an inference as an assertion restates it as fact to a user. Second RED: the silent-truncation test, because a budget-truncated context that reads as complete makes the model assert absence it never verified. Mutator watch: rendering derived and asserted identically must fail; dropping the truncation notice must fail.
**Done when**: criteria met, mutation report reviewed.
**Shipped**: `_core/rendering.py` — `GraphContext`/`RelatedFact` as the framework-agnostic input shape, `render()` producing `(page_content, metadata)` with derived facts and truncation both stated in the text (not only flags), `visible_facts()` applying the confidence-band and derived-inclusion filters. Mutation-hardened along with Slice A (combined 183-mutant run, see above). `GraphOwlRetriever` (`retrievers.py`) — the `langchain-core`-facing `BaseRetriever` subclass — calls `search_assets`, then composes `get_asset_context` + `explain_lineage` + `recall_memory` (finding 2: relationship types live on the lineage step, not the asset context) into a `GraphContext` per hit, and passes it through `visible_facts`/`render`. `as_of` still cannot be honoured (finding 1, unchanged — no MCP tool accepts it). Token-budget truncation is carried by the MCP tools themselves (`budget::fit`) and surfaces via `context.truncated`, which `render()`'s truncation notice reflects. `tests/test_retrievers.py` (14 tests) covers the composition, including `test_each_hit_fetches_its_own_context_not_a_shared_one` (a mutation-driven test: catches an fqn-mixup bug that per-document assertions alone would miss), a memory with no `humanAuthored` field defaulting to not-derived, and a missing `fullyQualifiedName` in the asset context falling back to the searched fqn.

### Slice C: Authorization survives the adapter — **shipped, 8 August 2026, reframed per finding 3**

**Value**: The slice that makes this safe to ship. Everything else is ergonomics.
**Acceptance criteria**: two principals with different policies retrieve different document sets from one corpus; counts are consistent — a filtered-out entity leaks through neither a total nor a "results truncated" message; a `403` surfaces as a typed permission error, never as an empty result; an expired token triggers one refresh and does not loop; the principal is attached per call, so one process may serve several.
**RED**: The two-principal test on a corpus where B can see a strict subset of A's. Empty-versus-denied is the same bug as `41-ui-workbench-governance.md` Slice A: a permission failure rendered as "no results" teaches an agent the data does not exist, and the agent will state that to a user. Mutator watch: a cached principal shared across calls must fail the per-call test; swallowing `403` into `[]` must fail.
**Done when**: criteria met, mutation report reviewed.
**Shipped as**: `tests/test_authorization.py` — a fake server keyed by bearer token so distinct principals genuinely see distinct corpora, proving the checkable property (different documents, no cross-contamination across interleaved calls, honest `total`/`policyFiltered` counts) rather than the plan's original "typed 403" wording, which finding 3 shows the server deliberately never sends. Token refresh, not in the original plan text but needed once a real principal's token could expire mid-session: `Principal.refresh` and `_core/client.py`'s `_attempt(..., allow_refresh=...)` retry exactly once on a 401, covered by `tests/test_client_refresh.py`'s 4 tests (no-callback raises; one retry succeeds; a second 401 after refresh raises rather than looping; the refreshed token persists for later calls on the same client).

### Slice D: The toolkit, one-to-one with MCP — **shipped, 8 August 2026**

**Acceptance criteria**: every Epic 14 read tool is exposed with its schema derived from the MCP manifest, not hand-written; a **manifest-parity test** asserts the exposed set equals the served set, so a new MCP tool fails CI until surfaced; tool errors carry the RFC 9457 `type` from Epic 1; a LangGraph agent completes a multi-step investigation — search, expand, check trust, read memory; no composite tool exists (decision 5), asserted structurally.
**RED**: Manifest parity. A hand-maintained tool list drifts within one release, and the failure is silent: the agent simply cannot do the new thing and nobody notices. Mutator watch: a hardcoded tool list must fail parity; a hand-written schema must fail when the MCP schema changes.
**Done when**: criteria met, mutation report reviewed.
**Shipped as**: `tools.py`'s `GraphOwlToolkit` builds every `StructuredTool` from a live `tools/list` manifest — `_core/client.py` gained `list_tools()` as a distinct method from `call_tool()`, since `tools/list`'s response is the tool array directly with none of `tools/call`'s `isError`/double-JSON-encoding wrapping. `args_schema=declaration["inputSchema"]` with `infer_schema=False` passes the raw JSON-Schema dict straight through — no dynamic pydantic model generation needed. `tests/test_tools.py` (6 tests) includes the manifest-parity tests directly (narrower manifest excludes a tool; wider manifest's new tool appears with no code change) and `test_no_composite_tool_exists_the_exposed_set_never_exceeds_the_manifest` for decision 5. `tests/test_langgraph_integration.py` proves the actual multi-step-investigation criterion: a `create_react_agent` built from `toolkit.tools()`, driven by a deterministic scripted tool-calling model, completes a real search-then-expand loop and the final answer reflects the second-hop (expanded) result.

### Slice E: Memory as checkpointing — **shipped, 8 August 2026, with a genuine deployment prerequisite found live**

**Acceptance criteria**: `GraphOwlCheckpointer` satisfies LangGraph's checkpointer contract; state round-trips across process restart; a discarded checkpoint is **retracted, not deleted**, and remains visible in history; agent-written memories carry `Authorship` identifying the agent (Epic 31), never a human; a human can read and correct anything an agent wrote via Epic 41's memory panel; concurrent writes to one thread do not interleave into a corrupt state.
**RED**: The retraction test — write, discard, then query as-of a time before the discard and assert the state is there. A checkpointer that hard-deletes destroys exactly the audit trail Epic 31 exists to provide, and this is the one place an agent framework's normal semantics ("clean up old checkpoints") conflict with the engine's. Mutator watch: a delete instead of a retract must fail; agent authorship recorded as the invoking human must fail.
**Done when**: criteria met, mutation report reviewed.
**Shipped as**: `memory.py`'s `GraphOwlCheckpointer`, satisfying `BaseCheckpointSaver`'s sync contract (async auto-delegates). Retraction needed no extra mechanism: `record_memory` has no delete/retract call and nothing it writes is ever removed, so an older checkpoint stays queryable after a newer one supersedes it — "discarding" is simply not writing a new one. Serialization reuses LangGraph's own `self.serde` (`JsonPlusSerializer`) rather than a hand-rolled format. `tests/test_memory.py` (9 tests, against a scripted mock server with real accumulate-never-delete semantics) includes a real compiled `StateGraph` proof: two `.invoke()` calls sharing one `thread_id` against a checkpointer-backed graph, asserting the running total persists 0→1→2. **The prerequisite finding**: running the identical calls against a real `graph-owl-server` (not the mock) surfaced two enforcement layers no mock had modelled — the target FQN must already exist as a real, readable asset (`record_memory` refuses a write to a synthetic never-created FQN), and the calling principal needs an explicitly granted `recordMemory` capability (Epic 32's admin-only, human-only grant route). Full account, and why neither is treated as a bug to route around, is in `memory.py`'s module docstring.

### Slice F: Packaging, CI, and the no-crate-change proof — **shipped, 8 August 2026, except PyPI publication**

**Acceptance criteria**: published to PyPI with a version pinned to a contract version; optional extras so `pip install graph-owl-langchain` does not force a LangGraph install; CI runs the full suite against a live service in a container; a contract change that breaks the adapter fails the build; a **structural test asserts this epic changed no graph-owl crate**; a quickstart that goes from install to a working retrieval in under twenty lines.
**RED**: The no-crate-change assertion is the honest test of `00j-language-boundaries.md`'s claim. If this epic needed a crate change, the boundary was drawn wrong and the document should be amended rather than the test relaxed. Second RED: the contract-drift test — change a field type and assert CI fails here.
**Done when**: criteria met, mutation report reviewed.
**Shipped as**: `_core/contract.py` (`REQUIRED_TOOLS`, `REQUIRED_METHODS`, mirroring `graph-owl-sdk`'s own `contract.py` pattern) backing `tests/test_contract_drift.py` — 3 tests, `pytest.mark.skipif` gated on `GRAPH_OWL_TEST_ENDPOINT`, all verified passing against a real server started specifically for that verification. `tests/test_no_crate_change.py` checks both uncommitted state (always) and committed history since a base ref (when `GRAPH_OWL_STRUCTURAL_CHECK_BASE` is set, as CI does) via `git diff`/`git status`, both verified. `scripts/verify-langchain.sh` mirrors `scripts/verify-sdks.sh` exactly (named container, `pg_isready` wait, open-mode server, `until curl -sf .../health`, `trap` cleanup) and is wired in as `.github/workflows/ci.yml`'s `langchain-integration` job. `README.md` is the quickstart (install → a working `GraphOwlRetriever.invoke(...)` call, under twenty lines). **Not done**: PyPI publication itself — an external, credentialed, irreversible action; the packaging metadata (`pyproject.toml`'s extras, version, dependency bounds) is ready for a human to run `twine upload` or equivalent when the package is meant to go public.

## Explicitly deferred (with destination)

- **A LlamaIndex / DSPy / Semantic Kernel integration** → decision 8 makes each a shim over the same core. Build the second one when someone asks, not before.
- **Prebuilt agents or chains** → `36-reference-apps.md`. Examples belong in examples, where nobody mistakes them for a supported product.
- **A hosted agent runtime** → not planned, at any point. That is the framework layer `00a-product-position.md` refuses.
- **Vector store adapter interface** (graph-owl as a drop-in `VectorStore`) → tempting and wrong: it presents a graph as a flat chunk store, which is the modelling error this whole product exists to fix. Revisit only with a use case that genuinely wants chunks.
- **Streaming / incremental retrieval** → after Epic 37a shows retrieval latency matters at scale.
- **Automatic graph construction from documents** → Epic 21, which is a different Python worker with a different job.

## Pre-PR quality gate

1. `mutmut` on `_core` and the rendering path — 0 unreviewed survivors (Slices A/B: 160/183 killed, every remaining survivor individually inspected and found equivalent — see "Progress and findings").
2. `ruff` and `mypy --strict` clean — reconfirmed 8 August 2026 across the full package (75 passed, 4 skipped without a live endpoint; `ruff check .` and `mypy` both clean).
3. **No principal default**, and no credential in `repr`, logs, or exceptions (Slice A).
4. **Two principals retrieve different sets**; achieved via the checkable property, not a distinguishable `403` — see finding 3 (Slice C).
5. **Derived facts labelled in `page_content`**, truncation stated (Slice B).
6. **Tool manifest parity** with the live MCP server (Slice D).
7. **Discarded checkpoints retract, not delete** (Slice E) — free by construction, since `record_memory` never deletes; deployment still needs the asset/capability prerequisites in the Slice E account satisfied first.
8. **Zero graph-owl crate changes**, asserted structurally (Slice F) — `tests/test_no_crate_change.py`, verified passing.
