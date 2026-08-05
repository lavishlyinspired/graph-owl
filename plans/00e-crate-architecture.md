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
| `graph-owl-reasoning-ql` | OWL 2 QL query rewriting over `spargebra` algebra | 99 | **Distinct dependency set** — it needs `spargebra`'s algebra types, which `graph-owl-reasoning` (RL, forward-chaining over `Flake`s directly) never should: RL reasons over facts, QL rewrites queries, and the two need different inputs entirely. **Also pure logic** — rewrite takes an algebra tree and a slice of subclass/subproperty edges, returns a rewritten tree, no I/O |

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

## The multi-backend path, reviewed 28 July 2026

A staged proposal was put: Postgres now → partitioning, background workers,
incremental reasoning and caching next → a storage abstraction with
Postgres / RocksDB / a native triple store only if measurement demands it. Plus
the recommendation not to write a storage engine.

**Endorsed, and mostly already done.** The abstraction is not something to adopt
later; it shipped on day one and `00b` decision 3 records why — *"traits cost
little and keep a second backend additive"*. Verified rather than assumed: the
`TripleStore` port in `graph-owl-engine` mentions no `sqlx`, `Postgres`, `Pool`
or `Transaction` anywhere in its signatures. It speaks `Flake`, `TriplePattern`,
`Sid` and `EngineError` — domain terms only. **It is a real port, not a port in
name**, which is the failure mode worth checking for and the reason this was
checked rather than asserted.

### One correction to the diagram

The proposal draws Postgres, RocksDB and a native store as **alternatives** under
one core. They are not, because of `00b` decision 12 — *relational source of
truth, flakes as view*. Entity CRUD, FQN lookup, list and filter run against the
relational store; the flake store is the graph view projected from it. So:

```
graph-owl-storage  (Storage port)      →  Postgres            entity CRUD, source of truth
graph-owl-engine   (TripleStore port)  →  Postgres | RocksDB | native    the flake view
```

**A second backend sits beside Postgres, not instead of it.** Anyone reading the
original diagram would plan a migration that removes Postgres, and that migration
does not exist. Two ports, and only one of them has a plausible second adapter.

### What already makes the swap possible, and it is not the trait

The trait is necessary and insufficient. The thing that would actually block a
second flake backend is an **atomicity assumption** — if flakes were written in
the same database transaction as the relational rows, moving them to another
store would break a guarantee the system depended on.

That assumption was never made. Epic 4 slice G shipped **reconciliation and
drift detection**, computing divergence by comparison rather than from a queue,
with repair one-directional from relational. The two stores are already treated
as eventually consistent with an explicit repair path. **The design that makes a
second backend feasible is already built and was built for another reason** —
which is the strongest form of readiness, because it is load-bearing today rather
than speculative.

The one genuine cross-backend constraint: `next_time()` and `time_at()` are a
monotonic transaction clock. With two stores, **the relational store stays the
clock authority** — it is the source of truth, so a `t` minted anywhere else
could not be compared against it. Recorded now because it is invisible until the
second adapter exists and expensive to discover then.

### One note on the staging

Stage 2's four items are not peers, and calling them one stage understates the
third by a lot. Partitioning and background workers are Postgres configuration.
Caching is a measurement question — and `00i` records that a cache-tier design
was already reproduced near-verbatim once and reverted, so it is the item most
likely to be added without evidence. **Incremental reasoning is Epic 97**, an
entire epic carrying an algorithm choice (DRed, because retraction is how facts
leave this store) and two resolved blockers. It is not a Stage-2 line item beside
a config change.

### Not writing a storage engine

Endorsed without reservation, and it is already the stated policy rather than a
new one. `00l-build-vs-adopt.md` draws the line at *flake scan, `as_of`, access
predicate, derivation chains and budgets* — the query layer over storage. A
B-tree, an LSM, crash recovery and compaction are all below that line and all
adopted. Concurrency control and crash recovery are exactly the multi-year
problems that argument names, and Postgres has solved them.

### The table-by-table split, reviewed 28 July 2026

A follow-up proposal broke the storage question down per data category and
recommended: Postgres remains the system of record for entities, ontology
metadata, provenance, explanations, jobs and operational state; **base and
inferred triples live behind the `TripleStore` abstraction**, with a
Postgres adapter now and a specialised one later if a customer's scale demands
it.

**The split is right and is what this workspace already has.** Three of the six
categories it lists need correcting, and two of the three point the same way:
some data the proposal assigns to relational actually belongs *with* the flakes.

| Category | Verdict |
|---|---|
| Base triples | ✅ `TripleStore` port. As designed |
| Inferred triples | ✅ Already there — `graph:reasoning` is a named graph, so a derived fact **is** a flake with `cx` set. No new home needed |
| Explanations | ❌ **Larger than the inferred set, not smaller** — see below |
| Change log | ❌ **Already exists, and is the flake store** — see below |
| Queue | ✅ Postgres is a good queue at this scale; no reason to add a broker |
| Materialized views | ⚠️ **Not used, and deliberately so** — see below |

**Explanations are the largest derived artefact, not a small relational side
table.** The proposal reasons that an explanation is a compact relational row —
*fact A came from rule R7 using facts B and C* — and is therefore smaller than
the triples. `DerivedFact` in `06-engine-reasoning.md` carries
`premises: Vec<Flake>` and `rule: Sid`, so a complete explanation is
`O(derived × premises-per-derivation)` — **strictly larger than the derived set
it explains**, since each derived fact contributes one row plus a link per
premise. At 400M inferred triples the explanation data is the biggest thing in
the system.

That inverts the placement argument. Putting explanations in Postgres while
inferred triples move to another backend creates a **cross-store join on the
hottest explainability path** — `GET /reasoning/explain` would have to read
premises from one store and their content from another. Explanations belong
**with the derived facts**, on whichever backend holds them. This is the one
place where the proposal's split, followed literally, would hurt.

**A separate change log duplicates the central design.** `00b` states that
`op = false` is a retraction rather than a delete, so the state at any past `t`
is recoverable by construction — *"time travel is not a feature built on top of
the store; it is a property of the store"*. The flake store **is** the change
log. A second one would be a parallel audit trail that can drift from the thing
it audits, which is exactly the failure `00i` records as already having been
argued once. Operational logs (job runs, connector executions) are a different
thing and do belong in Postgres.

**Materialized views are not in use and were declined on purpose.** Nothing in
the migrations creates one, and `07c-engine-lpg.md` decision 1 explicitly refuses
a materialized property-graph projection because *"a materialized parallel
property graph would be a second thing to keep consistent"*. The relational →
flake projection is application-level with reconciliation, **not** a materialized
view. Worth stating so nobody optimises a mechanism this system does not use.

**On the inference multiplier.** The proposal estimates 100M base triples
producing 400M inferred. The ratio is **ontology-dependent and unbounded in
principle** — a transitive property over a deep hierarchy is quadratic in the
worst case — so 4× is a plausible illustration and not a number to size hardware
against. `00i` rule 4 applies: the figure that governs is the one in Epic 6's
**budget**, because the budget is what actually bounds the derivation, and it is
enforced rather than estimated.

**The two-customer framing is the right one.** 20M triples: Postgres for
everything. 10B: Postgres for metadata and governance, a specialised adapter for
the graph. The engine does not change; the adapter does. That is precisely what
the two ports already permit, and it is why `00b` decision 3 took them on day
one.
