# Plan: MCP + Outbound Events (Epic 14) ★

**Branch**: feat/mcp-activation
**Status**: Slices A–E and G built; F is decisions-only (no sender, no transport)
**Depends on**: Epic 7a (subgraph retrieval for agent context), Epic 13 (authorization — **hard gate**), Epic 4 (graph to expose)
**Crate**: `graph-owl-mcp`

## Goal

The thesis test. An agent, given only MCP access, can discover assets, read their context, inspect lineage, and know whether to trust what it found — and cannot see anything policy denies it.

## Why this is the highest-information epic

The value of a context layer is determined entirely by whether an agent can actually use it, and nothing about the graph's design tells you that. Shipping MCP over a deliberately thin graph — tables, lineage, ownership — teaches more about what to model next than any amount of further modelling. Every later context type is then added to a surface already proven in use.

## Resolved decisions

1. **Read-only.** Write-back is Epic 32, after the read surface is validated by real usage. An agent with write access to the governance layer is a large trust surface.
2. **Authorization is not optional and not bolted on.** Every response is filtered by Epic 13's policy for the calling principal, compiled into the query (Epic 7 Slice E). An MCP server over an ungoverned graph is a data-exfiltration surface — this is why Epic 13 gates the phase.
3. **Responses carry trust signals and gaps, not bare rows.** An agent needs to know *what is missing* as much as what is present. A table with no owner must say "unowned", not omit the field.
4. **Token-budget aware.** MCP responses go into a context window. Every tool has a bounded response size and a documented truncation strategy that drops detail before dropping entities.
5. **Tools are task-shaped, not endpoint-shaped.** `find_assets` and `explain_lineage`, not `get_table` and `list_relationships`. A one-to-one mapping of REST endpoints to MCP tools makes the agent do the orchestration the facade should.

## Implementation reference

### Tool surface (7 read capabilities)

| Tool | Input | Returns |
|---|---|---|
| `search_assets` | query, entity types, filters, limit | ranked hits with FQN, type, description snippet, trust summary |
| `get_asset_context` | FQN or id, depth | entity + owners + tags + domain + lifecycle + certification + quality summary |
| `explain_lineage` | FQN, direction, depth | bounded subgraph with nodes, edges, and per-edge provenance |
| `analyze_impact` | FQN, change kind | downstream assets, affected contracts, owning teams to notify |
| `get_governance_context` | FQN | policies applying, sensitivity classifications, access constraints, retention |
| `query_graph` | SPARQL (subset) | bindings, policy-filtered |
| `recall_memory` | FQN or topic, limit | memory objects with provenance (Epic 31) |

`recall_memory` returns empty until Epic 31 lands; the tool is declared from the start so the agent's affordances do not change under it.

### Trust summary

Attached to every asset in every response, because it is what turns retrieval into grounded answering:

```rust
pub struct TrustSummary {
    pub lifecycle: LifecycleState,
    pub certified: Option<CertificationSummary>,   // incl. expiry
    pub quality: HealthState,                      // Healthy|Unhealthy|Stale|Unknown
    pub owner_known: bool,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub gaps: Vec<Gap>,                            // NoOwner, NoDescription, NoTests, StaleResults...
}
```

`gaps` is the decision that makes the difference: an agent told "this asset has no owner and no tests" answers differently from one shown a partial record and left to assume.

### The retrieval interface

The seven tools are surfaces over one trait, so retrieval policy lives in one place rather than being re-derived per tool:

```rust
#[async_trait]
pub trait AgentMemory: Send + Sync {
    async fn retrieve_context(&self, query: &str, opts: &RetrievalOptions)
        -> Result<AgentContext, ContextError>;
    async fn writeback(&self, c: &AgentContribution)   // Epic 32; refused here
        -> Result<Vec<Uuid>, ContextError>;
}

pub struct RetrievalOptions {
    pub max_tokens: usize,             // context-window budget
    pub confidence_threshold: f64,     // omit facts below it
    pub include_derived: bool,         // Epic 6 reasoning overlay, off by default
    pub max_hops: usize,               // traversal depth (Epic 7a)
    pub as_of: Option<DateTime<Utc>>,  // Epic 4 time-travel
}

pub struct AgentContext {
    pub entities: Vec<ContextEntity>,      // each with relevance + confidence
    pub relationships: Vec<ContextEdge>,
    pub provenance: Vec<ProvenanceEntry>,  // where each fact came from
    pub token_count: usize,
    pub truncated: bool,
    pub policy_filtered: bool,
}
```

`writeback` is declared here and **refused** until Epic 32 — the trait shape is fixed from the start so the agent's affordances do not change under it. `include_derived: false` by default keeps unconfirmed inference out of agent context unless asked for.

### Response envelope

```rust
pub struct McpResponse<T> {
    pub data: T,
    pub truncated: bool,
    pub truncation_reason: Option<String>,   // "token budget" | "depth limit" | "node budget"
    pub policy_filtered: bool,               // something was withheld
    pub as_of: Option<DateTime<Utc>>,
}
```

`policy_filtered: true` tells the agent its view is partial **without revealing what was withheld**. An agent that does not know it is seeing a filtered view will confidently assert absence.

### Outbound events

`EventSink` (Epic 3) gains an HTTP webhook adapter: registered endpoints, HMAC-signed payloads, at-least-once with exponential backoff, dead-letter after N attempts, per-endpoint event-type filters. Payloads are thin — entity type, id, FQN, event type, versions — and consumers fetch. A fat payload would leak data past the policy layer that a fetch would have applied.

## Acceptance criteria

- [ ] All seven tools are implemented and declared with JSON schemas.
- [ ] Every response is policy-filtered for the calling principal.
- [ ] `policy_filtered` is set when anything was withheld; withheld content is not identifiable.
- [ ] Every asset carries a `TrustSummary` including `gaps`.
- [ ] Responses respect a token budget and report truncation with its reason.
- [ ] `explain_lineage` terminates on cycles and reports depth truncation.
- [ ] Outbound webhooks deliver at-least-once with HMAC signatures and a dead-letter path.
- [ ] An unauthenticated MCP session is refused.
- [ ] The end-to-end test: an agent answers "what feeds this table, who owns it, is it safe to query" from MCP alone.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: MCP server with one tool, authenticated and filtered

**Value**: The thinnest end-to-end proof — protocol, auth, and policy together.
**Path**: `graph-owl-mcp` implementing the protocol; `get_asset_context` only; principal from Epic 12; filtering from Epic 13.
**Acceptance criteria**: tool discovery returns the declared schema; a valid call returns the entity with its trust summary; an unauthenticated session is refused; an asset the principal cannot read returns not-found, **not** a permission error naming it; `policy_filtered` set when related entities were withheld.
**RED**: The not-found-vs-forbidden test is the security-relevant one: a `403` naming an asset the caller cannot see leaks its existence. Assert an indistinguishable response for "absent" and "denied". Mutator watch: returning `403` with the FQN must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Trust summaries and gaps

**Value**: Turns retrieval into grounded answering.
**Acceptance criteria**: every asset carries lifecycle, certification (with expiry evaluated, not just present), quality state, and `gaps`; an untested asset reports `Unknown`, **never** `Healthy`; an expired certification reports as expired, not certified; a deprecated asset reports the deprecation and its successor; `gaps` is empty only when genuinely complete.
**RED**: The untested-asset test asserting `Unknown` — reporting `Healthy` for an asset nobody tested is the most damaging possible bug here, because it manufactures confidence. Expired-certification boundary test at exactly the expiry instant. Mutator watch: defaulting to `Healthy` must fail; ignoring expiry must fail the boundary.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Search and lineage tools

**Value**: Discovery and impact — the two questions agents ask most.
**Acceptance criteria**: `search_assets` returns ranked hits with trust summaries, policy-filtered, with correct totals under filtering; `explain_lineage` returns a bounded subgraph with per-edge provenance and confidence; a cyclic graph terminates; depth truncation is reported; `analyze_impact` names affected assets, contracts, and owning teams; lineage across a policy boundary reports `policy_filtered` rather than a broken chain.
**RED**: The policy-boundary lineage test: a chain A→B→C where B is denied must not silently present A→C as a direct edge. Assert the chain reports truncation instead. Mutator watch: stitching across a denied node must fail it — that fabricates a relationship that does not exist.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Governance and graph-query tools

**Value**: Policy-aware agents, and an escape hatch for questions the task-shaped tools do not cover.
**Acceptance criteria**: `get_governance_context` returns applicable policies, classifications, retention, and access constraints — *readable*, so an agent can reason about masking rather than merely being denied; `query_graph` accepts the Epic 7 subset with authorization compiled in; a query the principal cannot answer returns empty, not an error; query timeouts return partial results flagged truncated; unsupported SPARQL returns a specific "unsupported" message the agent can act on.
**RED**: A test asserting a masked column is reported *as masked* rather than omitted — an agent that cannot see a column exists will not know to ask for access. Mutator watch: omitting masked columns entirely must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Token budget and truncation

**Value**: Responses that fit in a context window without silently losing entities.
**Acceptance criteria**: each tool has a configurable token budget; exceeding it truncates **detail before entities** — descriptions shorten, then related-entity lists shorten, then the entity list truncates last; `truncated` and `truncation_reason` always set when truncation occurred; a truncated response is still valid JSON matching the schema; the budget is measured, not estimated by character count.
**RED**: A large-fixture test asserting the truncation *order* — that a response over budget drops description text before dropping an entity. Losing an entity silently is what makes an agent assert false absence. Mutator watch: truncating the entity list first must fail the ordering test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Outbound events and webhooks

**Value**: Downstream systems and agents react to change instead of polling.
**Acceptance criteria**: webhook registration with URL, secret, and event-type filter; payloads HMAC-signed with a documented canonicalization; at-least-once with exponential backoff; dead-letter after N attempts, replayable; payloads are **thin** — no entity content beyond identifiers and versions; delivery failure never affects the originating write; `localhost` and private-IP targets refused unless explicitly allowlisted.
**RED**: A thin-payload test asserting no description or column data appears in the webhook body — a fat payload bypasses the policy layer that a fetch would apply. An SSRF test asserting a private-IP target is refused. Mutator watch: including entity content must fail the thin-payload test; accepting a private IP must fail the SSRF test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: The thesis test

**Value**: Proof the epic did its job.
**Acceptance criteria**: an end-to-end test drives a real MCP client against a seeded graph and asserts the three-part question is answerable — what feeds this table, who owns it, is it safe to query — using only declared tools; the same test with a restricted principal asserts the answer is correctly narrower and flagged filtered; the test asserts no tool needed more than three calls to answer its part (a proxy for whether the tools are task-shaped or endpoint-shaped).
**RED**: The call-count assertion is the design feedback: if answering "who owns this" takes five calls, the tools are endpoint-shaped and Slice A's decision 5 was not honoured. Mutator watch: n/a — this test is an architectural assertion.
**REFACTOR**: whatever this test finds awkward is a real API defect. Fix the tool surface rather than the test.
**Done when**: criteria met, commit approved.

## Explicitly deferred (with destination)

- **Write capabilities** → Epic 32, after read is validated.
- **Streaming / incremental MCP responses** → when a response genuinely cannot fit a budget.
- **Agent session memory** → Epic 31 provides durable memory; per-session state belongs to the agent runtime, which graph-owl is deliberately not.
- **Multi-tenant MCP** → single-tenant assumed.
- **Tool-use analytics** (which tools agents actually call) → valuable input for Epic 32's design; add once there is real traffic.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Run the `security-review` skill** — this epic exposes the graph to autonomous callers.
5. Verify absent and denied are indistinguishable on every tool (Slice A).
6. Verify no webhook payload carries entity content (Slice F).
