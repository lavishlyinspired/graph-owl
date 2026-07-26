# Plan: Crate Architecture

**Status**: Standing decision, revised
**Governs**: every epic that would add a crate
**Crate scope**: All crates — this document is the authority on which exist and why.

## The rule

**A crate is created when it earns its keep** — when it has a genuinely distinct dependency set, or a compile-time boundary that buys something. Crates are not pre-allocated from a topic taxonomy.

This plan exists because a 28-crate architecture was proposed by mapping the 54-pillar research corpus onto crate names. That conflates two different things: a **pillar is a topic area**; a **crate is a unit of compilation and dependency**. The mapping produces crates like `graph-owl-causal`, `graph-owl-summarization`, and `graph-owl-distributed` that correspond to roadmap items which are explicitly out of scope — they would sit empty for years, each carrying a `Cargo.toml`, a version, a CI job, and a place in the dependency graph, with nothing inside.

**Sizing check.** A production reference graph engine in Rust runs ~842,000 lines across 32 crates — roughly 26,000 lines per crate. graph-owl is 2,429 lines today. A 28-crate plan would average under 90 lines per crate.

## Target: ~24 crates at maturity

### Built

| Crate | Holds | Deps |
|---|---|---|
| `graph-owl-core` | Domain entities, `Flake`, `Sid`, predicate vocabulary. **Zero I/O** | serde, chrono, uuid |
| `graph-owl-storage` | `Storage` trait, `StorageError` | core |
| `graph-owl-storage-postgres` | sqlx impl, refinery migrations | core, storage |
| `graph-owl-api` | `Catalog` facade | core, ports |
| `graph-owl-server` | axum HTTP, composition root | core, api, ports |

### Planned — created by the epic that needs them

| Crate | Holds | Epic | Distinct dependency justification |
|---|---|---|---|
| `graph-owl-engine` | `TripleStore`, `PredicateRegistry` traits | 4 | Port. Trait-only, no deps |
| `graph-owl-engine-postgres` | Flake table, four index orderings | 4 | sqlx, distinct schema from relational |
| `graph-owl-ontology` | Shape and axiom types | 5 | Pure types |
| `graph-owl-constraint` | Shape compilation and validation | 5 | **Pure logic — exhaustively testable without I/O** |
| `graph-owl-reasoning` | OWL 2 RL forward-chaining | 6 | **Pure logic** |
| `graph-owl-query` | SPARQL parse, plan, execute; Cypher lowering (7b) | 7 | Parser dependency; **pure logic** |
| `graph-owl-traversal` | Graph algorithms over `TripleStore` | 7a | **Pure logic, no parser dep** — four consumers, none of which should pull in SPARQL |
| `graph-owl-lpg` | LPG model; bidirectional flake ⇄ property-graph mapping | 7c | **Pure logic** — no I/O, and three consumers (7b, 7d, 9a) that must not depend on each other |
| `graph-owl-bolt` | PackStream codec, handshake, connection state machine | 7d | **Feature-gated off by default** — it opens a second listening port; distinct wire-protocol deps |
| `graph-owl-lpg-io` | GraphML, CSV bulk, Cypher script; sync to external LPG stores | 9a | Per-format and per-target deps, each its own feature — symmetric with `rdf-io` |
| `graph-owl-analytics` | Degree, components, PageRank over a caller-supplied projection | 38 | **Pure logic** — algorithms over an in-memory projection; zero I/O |
| `graph-owl-ui` | Embeds and serves the built console SPA | 39 | **Feature-gated** — a headless deployment must compile the assets out; `rust-embed` dep |
| `graph-owl-search` | `VectorIndex`, `TextIndex` traits | 8 | Port |
| `graph-owl-search-hnsw` | HNSW adapter | 8 | Vector index library |
| `graph-owl-rdf-io` | JSON-LD, Turtle, N-Triples, RDF/XML | 9 | rio, json-ld — heavy parser deps |
| `graph-owl-resolution` | Entity resolution, coreference, temporal | 17 | Distinct algorithms; possible ML deps |
| `graph-owl-connectors` | `Connector` trait, run machinery, Postgres reference connector | 15 | Governance concerns (scheduling, scope, deletion detection, identity) stay in the binary. **Connectors beyond Postgres are Python, out of process** — `00j-language-boundaries.md` |
| `graph-owl-mcp` | MCP server | 14 | Protocol implementation |
| `graph-owl-cli` | metadata-as-code, admin, DevOps tooling | 20 | clap; binary not library |
| `graph-owl-storage-memory` | In-memory `Storage` impl, promoted from the test fake | 37c | **Publishability** — it is what an embedding consumer links instead of Postgres. Named in `37c-embeddable.md`'s header and previously missing from this table |

**Why the three pure-logic crates are separate**: constraint validation, reasoning, and query planning are the highest-stakes logic in the system and each is a function over data the caller supplies. Separating them makes them testable without a database and keeps I/O out of code that must be exhaustively mutation-tested. That is a real boundary, not a taxonomic one.

## Crates I failed to address in the first pass

An audit of this plan against the 28-crate proposal found four gaps. Recorded because a silent omission is worse than a rejection.

| Crate | What happened | Resolution |
|---|---|---|
| `graph-owl-search-opensearch` | **Omitted entirely.** The proposal had two search adapters; I listed only HNSW | **Yes, as a deferred adapter.** HNSW is in-process and preserves the operational-simplicity budget (Postgres as the only required service), so it ships first. OpenSearch is the escape hatch when a deployment already runs a cluster or outgrows in-process indexing. Added below |
| `graph-owl-events` | **Inconsistency in my own docs.** Listed as a port in `plans/00b-architecture.md`'s layering diagram, absent from the crate table | **Yes.** It is a real port — the `EventSink` trait introduced by Epic 3 (`03-versioning.md`), consumed by Epic 8's indexer and Epic 14's outbound webhooks. Added below |
| `graph-owl-authz` | **Same inconsistency.** In the layering diagram, missing from the table | **Yes, and it is one of the pure-logic crates.** Policy evaluation is `(principal, action, resource, policies) → Decision` with no I/O — the same purity argument as `constraint` and `reasoning`, and the highest-stakes of the three. Added below |
| `graph-owl-engine-storage` | **Silently renamed** to `graph-owl-engine` with no explanation | **Renamed deliberately, now stated.** The proposal's name implies the port is only about storage; it also carries `PredicateRegistry` and the `Flake`/`Sid` contract. `graph-owl-engine` is the port for the engine as a whole, matching `graph-owl-storage`'s naming. The Postgres adapter keeps `graph-owl-engine-postgres` |

### Additions to the target set

| Crate | Holds | Epic | Justification |
|---|---|---|---|
| `graph-owl-events` | `EventSink` trait, `ChangeEvent` | 3 | Port. Trait-only |
| `graph-owl-authz` | `Policy`, `Rule`, decision engine | 13 | **Pure logic** — exhaustively testable; a surviving mutant here is a security bug |
| `graph-owl-search-opensearch` | OpenSearch adapter | 8 (deferred) | Distinct HTTP client dep; only compiled when selected |

### Reversal: `graph-owl-traversal` is a crate

This plan previously listed traversal as *"a module in `query`; property paths are a SPARQL feature"*. That was wrong and is reversed. Property paths answer **reachability**; they cannot express `shortest_path`, `all_paths`, `detect_cycles`, or `subgraph`, and multi-hop expressed as repeated BGP joins degrades to O(n^2). It passes the purity trigger (algorithms over the `TripleStore` port, zero parser dependency) and has four consumers — Epic 7 property paths, Epic 29 lineage, Epic 14 MCP subgraph, Epic 6 `sameAs` closure — none of which should compile a SPARQL parser to walk a graph. Epic 7a (`07a-engine-traversal.md`) owns it.

The `graph-owl-cypher` verdict stands: it shares `query`'s AST, planner, and physical operators, so a separate crate would depend on the whole of `query` anyway. Epic 7b's promotion from optional to scheduled (`07b-engine-cypher.md`) does not change that — Cypher remains a **module** in `query`.

### Second pass: the property-graph and console review

A capability review of labelled-property-graph support and of the web console added five crates. Each is recorded against the growth trigger rather than against a topic:

| Crate | Trigger it passes | The alternative, and why it loses |
|---|---|---|
| `graph-owl-lpg` (7c) | **Purity boundary** | A module in `query` would force `bolt` and `lpg-io` to depend on the SPARQL parser to describe a node. Three consumers, none of which should depend on the others |
| `graph-owl-bolt` (7d) | **Feature gating** | A module in `server` cannot be compiled out, and this one opens a second listening port. A deployment that does not want a Bolt endpoint must not link one |
| `graph-owl-lpg-io` (9a) | **Distinct dependency set** | Format and driver dependencies, per-feature. Exactly the argument that already separates `rdf-io`; putting both in one crate would make a GraphML export pull in an RDF parser |
| `graph-owl-analytics` (38) | **Purity boundary** | Algorithms over a caller-supplied in-memory projection. Inside `traversal` they would blur the line between a bounded walk and an unbounded whole-graph computation, which is a real operational distinction |
| `graph-owl-ui` (39) | **Feature gating** | Embedded assets must be compilable out entirely for a headless deployment — asserted by binary inspection in `39-ui-foundation.md` Slice A, not by a route guard |

Note what this review did **not** add: no crate per property-graph database, no Cypher crate, no second query-engine crate, no per-UI-feature crate. Five crates for two large capability areas.

**Target is therefore ~24 crates at maturity**, not 14. Five of them (`constraint`, `reasoning`, `authz`, `lpg`, `analytics`) are pure-logic crates whose I/O-freedom is the boundary that earns them their place — `query` and `traversal` are pure too, but carry a parser and a port dependency respectively; four are trait-only ports; the rest are adapters, feature-gated surfaces, or have genuinely distinct dependency sets.

## Explicitly not crates

| Proposed | Verdict | Why |
|---|---|---|
| `graph-owl-causal` | **No** | Causal KG is a research frontier, off the roadmap |
| `graph-owl-summarization` | **No** | Off the roadmap; revisit only if MCP responses prove too large |
| `graph-owl-compression` | **No** | HDT is an export-format concern; a module in `rdf-io` if ever |
| `graph-owl-distributed` | **No** | Single-node is the stated deployment model |
| `graph-owl-analytics` | **Reversed → yes, narrowly** | The rejection stood for *ranking*, and for ranking it still holds. Three of the four algorithms in Epic 38 answer **structural** questions no usage signal can — orphans, silos, blast radius. See `38-graph-analytics.md`; PageRank is included on probation with a written exit criterion |
| `graph-owl-neuro-symbolic` | **No** | Research frontier |
| `graph-owl-streaming` (RDF) | **No** | Broker ingestion is Epic 15 in `connectors`; RDF stream algebra is out of scope |
| `graph-owl-completion` | **No** | ML link prediction is deferred; a module in `reasoning` if it lands |
| `graph-owl-quality` | **Split** | Graph integrity → `constraint` (Epic 5 — `05-engine-constraints.md`); data quality → `api` (Epic 5 — `05-engine-constraints.md`) |
| `graph-owl-provenance` | **Merge** | Provenance is the flake's `t`/`op`/`m` — it *is* `core` |
| `graph-owl-cypher` | **Module** | Lowers to the same plan as SPARQL. A `cypher` module in `query` (Epic 7b — `07b-engine-cypher.md`), not a second engine |
| `graph-owl-graphdb` | **No** | A "graph database" crate above the engine is a taxonomic layer, not a build artifact. `engine` + `query` + `traversal` already are the graph database |
| `graph-owl-tinkerpop` | **No — reaffirmed, with better reasoning** | Previously rejected as "a third query front end". The property-graph review gives a stronger reason: Epic 7d's Bolt server (`07d-engine-bolt.md`) already reaches the entire property-graph driver and tool ecosystem for one unit of work. Gremlin would reach a strictly smaller ecosystem for the same cost. Revisit only if a named integration requires Gremlin and cannot speak Bolt |
| `graph-owl-extraction` | **Merge** | Extraction is an ingestion concern → `connectors` (Epic 15) |
| `graph-owl-pdf` | **Merge** | Document parsing is a `DocumentParser` port with adapters in `connectors` |
| `graph-owl-agent-memory` | **Merge** | Memory is a domain model in `core` + facade methods in `api`. It is data, not an engine |
| `graph-owl-devops` | **Merge** | CLI tooling → `cli` (Epic 4 — `04-engine-triples.md`) |
| `graph-owl-engine-oxigraph` | **Defer** | A second triple-store backend before the first is proven adds no information |
| One crate per external property-graph store | **No — modules, not crates** | An external property-graph database is either a **source** (a module in `connectors`, per the 100-connectors rule) or a **projection target** (a feature-gated module in `lpg-io`, per `09a-lpg-interchange.md`). Neither role justifies a crate, and there are enough such stores that a crate each reproduces exactly the sprawl the connector rule exists to prevent |
| A second graph backend crate | **Defer** | The second-backend decision stands: not now. `09a-lpg-interchange.md` decision 7 is explicit that an external store is a projection target, never a backend |
| `graph-owl-human-in-loop` | **Merge** | Confirmation workflows belong to the epics that need them (17, 21, 35) |

## Dependency rules

1. `graph-owl-core` depends on no other graph-owl crate and performs **no I/O**. Enforced by a CI dependency check, not by discipline.
2. `graph-owl-api` never names a concrete adapter — trait objects only.
3. Adapters never depend on each other.
4. Pure-logic crates (`constraint`, `reasoning`, `query`, `authz`, `traversal`, `lpg`, `analytics`) never perform I/O; they take data and return results.
5. Only `graph-owl-server` reads environment variables or constructs adapters.
6. `graph-owl-ui` contains no business logic and exposes no API surface of its own — it embeds assets and serves them. Every capability the console uses is a public, documented endpoint (`00f-ui-architecture.md` non-negotiable 1).

## Growth trigger

Before adding a crate, one of these must be true:

- **Distinct dependency set** — it pulls in deps the rest of the workspace should not compile (heavy parsers, ML frameworks, protocol clients).
- **Feature gating** — consumers must be able to compile it out.
- **Purity boundary** — it is I/O-free logic that benefits from being unable to reach I/O.
- **Publishability** — it will be released independently (Epic 37c's embedding work).

If none applies, it is a module.

## The read/write trait split

`Storage` and `TripleStore` each carry reads and writes in one trait. `00g-operations.md` §6 records the split (`TripleStoreRead` / `TripleStoreWrite`) and its trigger: **Epic 7d**, the first genuinely read-only consumer, and therefore the first point at which the dependency sets actually diverge. Splitting earlier would be a boundary drawn without information — which is what the revisit trigger below exists to prevent.

## Revisit trigger

Split a crate when its dependency set genuinely diverges — when part of it needs something the rest should not compile. Merge two when neither has a distinct dependency set and neither is separately published.

Record either in `plans/00b-architecture.md`'s decision log with the reasoning.
