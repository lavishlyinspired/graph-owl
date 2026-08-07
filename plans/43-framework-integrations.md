# Plan: Agent Framework Integrations (Epic 43)

**Branch**: feat/framework-integrations
**Status**: **In progress, 8 August 2026 — Slice A shipped and hardened; Slice B's rendering half shipped; three real structural findings against Epic 14 recorded below and not yet resolved.** See "Progress and findings, 8 August 2026".
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

**What remains**: `GraphOwlRetriever` (Slice B's `langchain-core`-facing
half — a `pydantic.BaseModel` subclass over `BaseRetriever`, composing
`search_assets` + `get_asset_context` + `explain_lineage` +
`recall_memory` per hit, now that the composition need from finding 2
above is known) and Slices C–F in full (the two-principal authorization
proof reframed per finding 3; the MCP-manifest-parity toolkit; the
retraction-based checkpointer; packaging, live-service CI, and the
no-crate-change structural test). Each remaining slice should re-check its
own assumptions against the real MCP/API surface the same way Slices A–B
just did, rather than trusting the plan text below at face value — every
divergence found so far was real, not a misreading.

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

- [ ] A LangChain chain retrieves graph context with **no graph-owl crate change** — asserted structurally.
- [ ] A LangGraph agent binds the toolkit and completes a multi-step investigation against a seeded corpus.
- [x] Constructing any surface **without a principal raises**, and is tested — `Principal`/`GraphOwlClient`, `tests/test_principal.py`, `tests/test_client.py`.
- [ ] Two principals against one corpus retrieve **different documents** — reframed by finding 3 above: achievable and worth proving, but not via a distinguishable-403 signal the server deliberately never sends.
- [x] A derived fact is identifiable as derived **in the text the model receives**, not only in metadata — `_core/rendering.py::render`, `tests/test_rendering.py`.
- [~] Confidence bands and `as_of` round-trip into `Document.metadata` — confidence does; `as_of` is threaded through the shape but cannot round-trip truthfully yet (finding 1).
- [ ] `as_of` retrieval returns state as of that time, including an entity retracted since — **blocked on finding 1**, not yet attempted.
- [ ] The checkpointer round-trips agent state; a discarded checkpoint is **retracted, not deleted**, and remains in history.
- [ ] Tools map one-to-one onto Epic 14's MCP tools — asserted against the tool manifest, so a new MCP tool fails this test until exposed.
- [ ] CI runs the integration against a live service; a contract change that breaks it fails the build.
- [x] The package imports without a LangChain install where only the core is used (decision 8) — `test_the_core_module_imports_with_no_framework_installed`.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR. Python mutation testing uses `mutmut`.

### Slice A: The core client and principal handling — **shipped, 8 August 2026**

**Acceptance criteria**: an MCP client over Epic 14's streamable HTTP transport; a REST fallback via Epic 16's generated SDK; **construction without a principal raises**; credentials never appear in logs, reprs, or exception messages; connection failure is a typed error naming the endpoint; the core imports with no framework installed.
**RED**: The no-principal test, and a credential-leak test that constructs a client with a token and asserts the token appears in neither `repr()`, nor a captured log, nor the string of any raised exception. Convenience defaults are how an integration ends up running as an admin, and a token in a traceback is how it ends up in a bug report. Mutator watch: a default principal must fail construction; a `__repr__` that formats the credential must fail the leak test.
**Done when**: criteria met, mutation report reviewed.
**Shipped as**: a single JSON-RPC-over-HTTP client (`_core/client.py`), stdlib only, matching `graph-owl-sdk`'s own no-dependency choice — not a REST-fallback pair, since every read this epic needs has an MCP tool. The REST SDK stays the documented fallback for what Epic 14 has no equivalent for (bulk export, admin), not yet needed by anything built so far. All criteria met; mutation-hardened (see "Progress and findings" above for the exact score and what the survivors are). The one correction mid-slice: `call_tool`'s error handling initially assumed the wrong wire shape (top-level JSON-RPC `error` only) — found and fixed before Slice B was built on top of it, see above.

### Slice B: Retrieval that preserves what makes the graph worth querying — **rendering half shipped, retriever not yet built**

**Acceptance criteria**: `GraphOwlRetriever` returns `Document`s whose `page_content` is a rendered subgraph and whose `metadata` carries entity ids, relationship types, provenance, confidence, derived flags, and `as_of`; **derived facts are marked in `page_content`**; confidence below the ignore band (<0.5, `00c-domain-model.md`) is excluded by default and includable explicitly; an empty result is an empty list, never an exception; token budget is respected and truncation is stated in the returned text rather than silent.
**RED**: The derived-labelling test — assert the rendered string identifies an inferred fact as inferred. Metadata alone fails this: the model reads `page_content`, and an LLM handed an inference as an assertion restates it as fact to a user. Second RED: the silent-truncation test, because a budget-truncated context that reads as complete makes the model assert absence it never verified. Mutator watch: rendering derived and asserted identically must fail; dropping the truncation notice must fail.
**Done when**: criteria met, mutation report reviewed.
**Shipped so far**: `_core/rendering.py` — `GraphContext`/`RelatedFact` as the framework-agnostic input shape, `render()` producing `(page_content, metadata)` with derived facts and truncation both stated in the text (not only flags), `visible_facts()` applying the confidence-band and derived-inclusion filters. Mutation-hardened along with Slice A (combined 183-mutant run, see above). **Not yet built**: `GraphOwlRetriever` itself — the `langchain-core`-facing `BaseRetriever` subclass that calls `search_assets`, then composes `get_asset_context` + `explain_lineage` (finding 2: relationship types live on the lineage step, not the asset context) into a `GraphContext` per hit. `as_of` cannot be honoured yet (finding 1). Token-budget truncation is already carried by the MCP tools themselves (`budget::fit`, checked in `graph-owl-mcp`'s own tests) and surfaces via `context.truncated` — `render()`'s truncation-notice behaviour is done; wiring a real `truncated` flag through from a live `search_assets`/`get_asset_context` call is not.

### Slice C: Authorization survives the adapter

**Value**: The slice that makes this safe to ship. Everything else is ergonomics.
**Acceptance criteria**: two principals with different policies retrieve different document sets from one corpus; counts are consistent — a filtered-out entity leaks through neither a total nor a "results truncated" message; a `403` surfaces as a typed permission error, never as an empty result; an expired token triggers one refresh and does not loop; the principal is attached per call, so one process may serve several.
**RED**: The two-principal test on a corpus where B can see a strict subset of A's. Empty-versus-denied is the same bug as `41-ui-workbench-governance.md` Slice A: a permission failure rendered as "no results" teaches an agent the data does not exist, and the agent will state that to a user. Mutator watch: a cached principal shared across calls must fail the per-call test; swallowing `403` into `[]` must fail.
**Done when**: criteria met, mutation report reviewed.

### Slice D: The toolkit, one-to-one with MCP

**Acceptance criteria**: every Epic 14 read tool is exposed with its schema derived from the MCP manifest, not hand-written; a **manifest-parity test** asserts the exposed set equals the served set, so a new MCP tool fails CI until surfaced; tool errors carry the RFC 9457 `type` from Epic 1; a LangGraph agent completes a multi-step investigation — search, expand, check trust, read memory; no composite tool exists (decision 5), asserted structurally.
**RED**: Manifest parity. A hand-maintained tool list drifts within one release, and the failure is silent: the agent simply cannot do the new thing and nobody notices. Mutator watch: a hardcoded tool list must fail parity; a hand-written schema must fail when the MCP schema changes.
**Done when**: criteria met, mutation report reviewed.

### Slice E: Memory as checkpointing

**Acceptance criteria**: `GraphOwlCheckpointer` satisfies LangGraph's checkpointer contract; state round-trips across process restart; a discarded checkpoint is **retracted, not deleted**, and remains visible in history; agent-written memories carry `Authorship` identifying the agent (Epic 31), never a human; a human can read and correct anything an agent wrote via Epic 41's memory panel; concurrent writes to one thread do not interleave into a corrupt state.
**RED**: The retraction test — write, discard, then query as-of a time before the discard and assert the state is there. A checkpointer that hard-deletes destroys exactly the audit trail Epic 31 exists to provide, and this is the one place an agent framework's normal semantics ("clean up old checkpoints") conflict with the engine's. Mutator watch: a delete instead of a retract must fail; agent authorship recorded as the invoking human must fail.
**Done when**: criteria met, mutation report reviewed.

### Slice F: Packaging, CI, and the no-crate-change proof

**Acceptance criteria**: published to PyPI with a version pinned to a contract version; optional extras so `pip install graph-owl-langchain` does not force a LangGraph install; CI runs the full suite against a live service in a container; a contract change that breaks the adapter fails the build; a **structural test asserts this epic changed no graph-owl crate**; a quickstart that goes from install to a working retrieval in under twenty lines.
**RED**: The no-crate-change assertion is the honest test of `00j-language-boundaries.md`'s claim. If this epic needed a crate change, the boundary was drawn wrong and the document should be amended rather than the test relaxed. Second RED: the contract-drift test — change a field type and assert CI fails here.
**Done when**: criteria met, mutation report reviewed.

## Explicitly deferred (with destination)

- **A LlamaIndex / DSPy / Semantic Kernel integration** → decision 8 makes each a shim over the same core. Build the second one when someone asks, not before.
- **Prebuilt agents or chains** → `36-reference-apps.md`. Examples belong in examples, where nobody mistakes them for a supported product.
- **A hosted agent runtime** → not planned, at any point. That is the framework layer `00a-product-position.md` refuses.
- **Vector store adapter interface** (graph-owl as a drop-in `VectorStore`) → tempting and wrong: it presents a graph as a flat chunk store, which is the modelling error this whole product exists to fix. Revisit only with a use case that genuinely wants chunks.
- **Streaming / incremental retrieval** → after Epic 37a shows retrieval latency matters at scale.
- **Automatic graph construction from documents** → Epic 21, which is a different Python worker with a different job.

## Pre-PR quality gate

1. `mutmut` on `_core` and the rendering path — 0 survivors.
2. `ruff` and `mypy --strict` clean.
3. **No principal default**, and no credential in `repr`, logs, or exceptions (Slice A).
4. **Two principals retrieve different sets**; `403` never becomes `[]` (Slice C).
5. **Derived facts labelled in `page_content`**, truncation stated (Slice B).
6. **Tool manifest parity** with the live MCP server (Slice D).
7. **Discarded checkpoints retract, not delete** (Slice E).
8. **Zero graph-owl crate changes**, asserted structurally (Slice F).
