# graph-owl — Language Boundaries

**Status**: Standing decision.
**Governs**: every epic that would introduce a second language or a second process.
**Companion to** `00a-product-position.md` (the operational-simplicity budget this exists to protect) and `00e-crate-architecture.md` (which governs Rust crates specifically).

## The question this answers

"Should X be in Python?" has been asked about MCP, machine learning, analytics, agent frameworks, connectors, and plugins. Answering it case-by-case produces a system that is half of each language for no stated reason. This document states the rule once.

## The rule

**The language boundary is the process boundary, and the process boundary is drawn by the operational-simplicity budget.**

`00a-product-position.md` commits to one binary with Postgres as the only required service. That budget is a claimed differentiator, and it is what makes the boundary decidable rather than a matter of taste:

- **In the binary → Rust.** Anything on the read/write hot path, anything that must share the authorization predicate, anything whose failure is a correctness failure rather than a job failure.
- **Out of process → whatever fits the job, usually Python.** Anything that talks to the outside world, changes at the pace of someone else's API, or needs a library ecosystem Rust does not have.
- **Neither → it is a consumer, not a component.** The largest category, and the one most often misfiled.

A thing does not go in the binary because it is written in Rust. It goes in the binary because it must not be a separate deployable — and it is Rust *because* it is in the binary.

## The third category, first, because it is the one that matters most

**LangChain, LangGraph, agent frameworks, LLM applications, orchestration, and "agents" are not part of graph-owl.** They are **clients** of it.

This is not a language decision at all, and treating it as one is a category error with a real cost. graph-owl is the **substrate** an agent reasons over: it stores the graph, answers queries, enforces policy, and exposes both through an API and MCP. An agent framework is the thing on the other end of that connection.

`00a-product-position.md` refuses to compete on the agent-framework layer, and embedding one would reverse that:

| If LangGraph lived inside graph-owl | Consequence |
|---|---|
| graph-owl becomes an agent framework | Now competing with well-funded, faster-moving projects on their ground |
| Its release cadence binds ours | An LLM-framework ecosystem moves weekly; a graph engine's storage format must not |
| A Python runtime enters the deployment | The operational-simplicity budget is gone, and it is a stated differentiator |
| Every LLM dependency becomes ours to patch | Including its transitive supply chain |

**What we build instead is the surface those frameworks connect to** — Epic 14 (MCP), Epic 32 (agent write-back), Epic 31 (memory). A user's LangGraph agent connects over MCP, calls `retrieve_context`, gets a policy-filtered subgraph, and writes a memory back. That agent lives in *their* repository, in *their* Python, on *their* release schedule.

The same applies to skills, plugins, and prompt tooling built on top of an agent framework. **Reference applications demonstrating this are Epic 36, and those are Python — because they are examples, and examples should look like what a user would actually write.**

## MCP: the assumption worth correcting

**MCP does not have to be Python, and for this project it should not be.**

There is an [official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) (`rmcp`), maintained by the Model Context Protocol organization, at 1.x. It tracks the current draft spec while staying compatible with the stable release, and supports stdio, streamable HTTP, and child-process transports with OAuth support for servers. This is not a community port with a bus factor of one — it is the reference Rust implementation.

That removes the only strong argument for a Python MCP server here. What remains argues for Rust:

| Consideration | In-binary Rust MCP | Out-of-process Python MCP |
|---|---|---|
| Authorization | **The same compiled `AccessPredicate`** (`12-13-security.md` decision 6a) as HTTP, SPARQL, and Bolt | A fifth surface, either re-implementing policy or calling back over HTTP for every tool invocation |
| Deployment | One binary, feature-gated | A second deployable, its own runtime, its own supervision |
| Latency | In-process call into the query engine | Network hop per tool call, on the path an agent uses most |
| Ecosystem | Smaller, but MCP is a thin protocol — this is not where Python's advantage lies | Larger, and largely irrelevant to serving a graph |
| Iteration speed | Slower | Faster |

**Decision: MCP stays in `graph-owl-mcp`, in Rust, in the binary** (Epic 14 unchanged). The deciding factor is authorization. Epic 13 requires one predicate lowered to every surface with a four-way equivalence test; a Python MCP server either becomes a fifth lowering — the exact duplication that decision exists to prevent — or makes every tool call an HTTP round trip through a surface it already has.

**Where this decision would flip**: if MCP tool implementations start needing Python-only libraries (an embedding model, a document parser), those move out of process behind the ingestion API and MCP keeps calling the graph. The tool surface stays Rust; the heavy lifting moves.

## Connectors: where Python genuinely wins

`15-connectors.md` originally specified one `graph-owl-connectors` crate with a feature-gated Rust module per source. **That decision has been reversed** — the reasoning is below and the reversal is recorded in that plan.

The case against Rust connectors is strong:

- **Client-library coverage.** Warehouse, BI, orchestration, and SaaS metadata APIs have mature, maintained Python clients and often only partial or unmaintained Rust ones. Writing a connector should be an afternoon, not a week spent writing an HTTP client someone already wrote.
- **Contributor pool.** A connector is the most likely thing an outside contributor writes. The data-engineering population writes Python.
- **Blast radius.** A connector is I/O against someone else's flaky API. It should fail as a *job*, not as a fault inside the process holding the graph.
- **Release cadence.** A source changes its API; the connector must ship without rebuilding and redeploying the engine.

The case for keeping Rust connectors is narrower but real: no second runtime, one build, shared types, and the `Connector` trait's compile-time guarantees.

**Resolution — applied to `15-connectors.md` decision 1:**

| Layer | Language | Why |
|---|---|---|
| `Connector` **trait** and the run/scope/deletion machinery | Rust, in the binary | Scheduling, run history, deletion-threshold guards, and identity are governance concerns, and `15-connectors.md` decision 4 makes deletion detection the sharpest edge in the epic |
| **Postgres connector** (the reference implementation) | Rust | Proves the trait, ships in the binary, needs no runtime |
| **Everything else** — warehouses, BI, SaaS, orchestration | **Python, out of process**, pushing through Epic 16's ingestion API | Where the libraries and the contributors are |
| Connector **configuration schema** | JSON Schema, language-neutral | Already decided (`15-connectors.md` decision 8) and this is exactly why it matters — it lets a Python connector describe itself to a Rust server and a TypeScript admin form |

This preserves the operational-simplicity budget honestly: **a deployment that only catalogs Postgres still runs one binary.** A deployment that wants Snowflake runs a Python worker as well — and that is a cost it opted into, not one imposed on everyone.

Epic 16 (ingestion APIs and SDKs) already exists for exactly this path, which suggests the two epics were always meant to meet here.

## The full map

### Rust, in the binary

| Area | Why not Python |
|---|---|
| Flake store, four indexes, time travel (4) | The hot path. Every read passes through it |
| Constraint validation (5), reasoning (6) | Pure logic, exhaustively mutation-tested. A GC pause inside a fixpoint is a budget violation |
| SPARQL and Cypher (7, 7b), traversal (7a), LPG (7c) | Parser and planner on the query path |
| Bolt server (7d) | A wire protocol with strict framing and bounded memory |
| Authorization (13) | One predicate, four lowerings. A second language means a fifth |
| Vector and text index (8) | In-process is the requirement — an external cluster breaks the budget |
| **Graph analytics (38)** | Degree, components, PageRank over a CSR projection. Serializing a 100k-node graph out to Python to run PageRank costs more than running it. **This is not a data-science workload; it is four algorithms over an in-memory array** |
| MCP (14) | Authorization, per above |
| Entity resolution (17), first cut | Deterministic and simple probabilistic matching is string work. Escalate only if a learned matcher earns its keep |
| CLI (20), export/restore (37b) | Bounded by `20-metadata-as-code.md`'s scope rule |
| Console asset embedding (39) | `graph-owl-ui` embeds a build artifact; it runs no logic |

### Python, out of process

| Area | Why not Rust |
|---|---|
| **Connectors beyond the reference** (15) | Library coverage and contributor pool, per above |
| **Embedding generation** (8) | Model inference. Either a hosted API or a Python worker; never in the binary. graph-owl stores and searches vectors — it does not produce them |
| **Document and conversation ingestion** (21) | PDF layout, OCR, chunking, LLM extraction. Python's ecosystem here is not close to matched |
| **Learned entity resolution** (17), if it ships | Training and evaluation are Python work by default |
| **Reference applications** (36) | They are examples. An example should look like what a user would write |
| Benchmarking harnesses, corpus generation, evaluation | Scripts, not product |

### TypeScript

| Area | |
|---|---|
| The console (39–42) | `00f-ui-architecture.md`. Sources in `ui/`, built to static assets, embedded by `graph-owl-ui` |

### Neither — consumers, not components

Agent frameworks (LangChain, LangGraph, and successors), orchestrators, notebooks, BI tools, LLM applications, agent "skills" and plugins. They connect via MCP (14), the HTTP API, or Bolt (7d). **Every one of these is a reason Epic 7d exists**: speak the protocols, and the ecosystem connects itself.

## How the two sides talk

Exactly three ways, all of them already-planned public surfaces. **No private channel, no shared database access, no Python reaching into Postgres directly.**

1. **The HTTP API** (Epic 1, 16) — how a Python connector pushes metadata in.
2. **MCP** (Epic 14) — how an agent reads and writes back.
3. **Bolt** (Epic 7d) — how a property-graph driver connects.

A Python component that reads graph-owl's Postgres schema directly would couple an external process to an internal storage layout, and every migration in `00g-operations.md` §1 would then have to consider it. **The schema is not an interface.**

## What would change this document

- **A Rust connector ecosystem matures** to the point that writing a Snowflake or dbt connector in Rust is not slower than Python. Revisit the connector split.
- **`rmcp` stalls** or falls behind the protocol. Revisit MCP.
- **A learned component becomes central** rather than peripheral — if entity resolution or classification depends on a model in the hot path, the boundary moves and the budget needs restating.
- **PyO3 embedding is proposed.** It is not currently: embedding a Python interpreter *in* the binary gets Python's ecosystem and Python's deployment problems simultaneously, and forfeits the one-binary claim while still holding the GIL. If it is ever proposed, it needs a decision here first.

## Open decisions

| # | Decision | Status |
|---|---|---|
| L1 | MCP in Rust, in the binary | **Decided** — official Rust SDK plus the authorization argument |
| L2 | Analytics (38) in Rust | **Decided** — four algorithms over an array, not a data-science workload |
| L3 | Agent frameworks are consumers | **Decided** — restating `00a`. Made concrete by Epic 43 (`43-framework-integrations.md`): we ship the *integration*, never the framework |
| L4 | **Connectors: Rust trait + Postgres, Python for the rest** | **Decided and applied** — `15-connectors.md` decision 1, `00e-crate-architecture.md` |
| L5 | Embeddings out of process | **Decided and applied** — `08-engine-search.md` decision 7 |
| L6 | Document ingestion (21) in Python | **Decided and applied** — `21-document-ingestion.md` decision 0 |
