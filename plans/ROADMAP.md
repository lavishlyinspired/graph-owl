# graph-owl — End-to-End Roadmap

> **A knowledge graph engine that stores, queries, reasons over, and validates enterprise metadata as a connected graph.**

Three layers, one product: an **engine** (triples, time-travel, constraints, reasoning, query), a **catalog** domain model expressed in the graph, and a **context layer** activating both for humans and agents.

**43 epics, 9 phases** (48 counting the lettered sub-epics 7a–7d and 9a). Companions: `plans/00a-product-position.md` (what this competes on), `plans/00b-architecture.md` (crate map, flake model, layering), `plans/00c-domain-model.md` (entities, envelope, relationships), `plans/00d-api-conventions.md` (wire contract), `plans/00e-crate-architecture.md` (which crates exist and why), `plans/00f-ui-architecture.md` (the console), `plans/00g-operations.md` (migration, DR, retention, runbooks, journey tests), `plans/00h-ui-design-system.md` (tokens, patterns, the screen inventory), `plans/00i-licensing.md` (clean-room rules), `plans/00j-language-boundaries.md` (Rust vs Python vs neither).

## Revision note — engine repositioning

This supersedes roadmaps scoped to a "metadata catalog" and then an "enterprise context layer". Both described the surface; neither described the substrate. Adopting the engine framing has one large consequence and several simplifications.

**The consequence**: the engine moves to **Phase 1**, immediately after foundation. It changes the storage model, and retrofitting triples after twenty entity types is the same retrofit-cost argument that put the entity envelope first.

**The simplifications** — five previously separate epics collapse into engine capabilities:

| Was | Now |
|---|---|
| Metadata time-travel (its own epic) | **Native** — flake `t` + `op` give it by construction (Epic 4) |
| Graph integrity & consistency | **Constraint validation** (Epic 5) |
| Inference & derived facts | **OWL 2 RL reasoning overlay** (Epic 6) |
| Graph API & multi-hop traversal | **SPARQL subset** (Epic 7) |
| Semantic search (as a bolt-on index) | **Engine vector index** (Epic 8) |

Net epic count is unchanged; the structure is substantially better. Capabilities that were going to be built awkwardly on top of a relational catalog are now properties of the substrate.

## The scope decision that governs everything

The engine implements **useful subsets, not specifications.**

Measured against a production reference implementation in Rust: ~842,000 lines across 32 crates, of which the query engine alone is ~143,000, SPARQL ~29,000, the reasoner ~13,000, SHACL ~7,000. graph-owl today is 2,429 lines.

Full SPARQL 1.1, OWL 2 DL, and SHACL conformance is a multi-year project serving few enterprise-metadata needs. What serves them is:

| Capability | In | Out |
|---|---|---|
| Query | BGP, FILTER, OPTIONAL, UNION, property paths | Federation, entailment regimes, full algebra |
| Reasoning | OWL 2 RL forward-chaining, queryable overlay | Tableau reasoning, OWL 2 DL/Full |
| Validation | Node and property shapes | Full SHACL-SPARQL |
| Interop | JSON-LD, Turtle, N-Triples, RDF/XML | RDF as the sole internal model |

**A rule a steward cannot read, or an inference nobody can explain, is not worth its implementation cost.** Every engine epic states its subset boundary explicitly.

## Where the project is

**Table CRUD** and **Table↔Table relationships** — complete, tested, mutation-verified, 59 tests on real Postgres, 2,429 lines. A walking skeleton proving HTTP → facade → port → Postgres. The engine, collection, and activation layers do not exist.

## Sequencing principles

1. **Retrofit cost.** The envelope applied to 1 entity type is a day; to 25, a quarter. The triple model applied to 4 entity types is a phase; to 25, a rewrite. Both go early.
2. **Unblocking.** FQN needs hierarchy. Reasoning needs an ontology. Memory needs entities. Every ingestion path after the second needs identity resolution.
3. **Proving the thesis.** Among equally cheap and unblocked work, ship what tests whether agents can use this — hence MCP at Phase 3, not Phase 8.

**Authorization precedes activation.** An MCP server or SPARQL endpoint over an ungoverned graph is a data-exfiltration surface. Epic 13 gates Phase 3.

## Phases

| Phase | Epics | Proves |
|---|---|---|
| **0 · Foundation** | 1–3 | The contract, the model, the audit trail |
| **1 · Engine** ★ | 4–9a | **It is a graph engine, not a table with a join** — RDF *and* property graph, over one store |
| **2 · Governed graph** | 10–13 | Identity, ownership, policy — safe to expose |
| **3 · Activation v1** ★ | 14 | **Agents can use it** — the thesis test |
| **4 · Collection** | 15–21 | It fills itself, from every source shape, without duplicating |
| **5 · Semantics & trust** | 22–30 | Business meaning and grounds for confidence |
| **6 · Memory** ★ | 31–32 | **Why**, not just what — the differentiator |
| **7 · Breadth** | 33–38 | Coverage, scale, portability, structural analytics |
| **8 · Console** | 39–42 | **The differentiators become demonstrable** — see `00f-ui-architecture.md` |

## The epics

★ = differentiator.

| # | Epic | Layer | Delivers |
|---|---|---|---|
| 1 | API conventions, contract & relationship model | Norm | Wire contract, OpenAPI + JSON Schema, full taxonomy |
| 2 | Entity hierarchy & columns | Norm | Assets have addresses and shape |
| 3 | Envelope, versioning, soft delete | Norm | What changed, who changed it, undo |
| 4 | **Triple storage & time-travel** ★ | Engine | Flakes, four indexes, entity projection, as-of query |
| 5 | **Constraint validation** | Engine | Shapes over the graph; violations reported, not guessed |
| 6 | **Reasoning overlay** | Engine | OWL 2 RL derived facts, queryable and explainable |
| 7 | **Graph query (SPARQL subset)** ★ | Engine | Multi-hop query the REST surface cannot express |
| 7a | **Graph traversal** | Engine | Shortest path, all paths, cycles, subgraph — what property paths cannot express |
| 7b | **openCypher front end** | Engine | **Scheduled** (was optional). Lowers onto the same plan; no second engine |
| 7c | **Labelled property graph** ★ | Engine | Bidirectional flake ⇄ LPG mapping; edge properties come nearly free from reified relationships |
| 7d | **Bolt protocol server** ★ | Engine | One protocol buys the entire property-graph driver and tool ecosystem |
| 8 | **Vector & hybrid search** ★ | Engine | Search by meaning, ranked with lexical |
| 9 | **RDF interop & open standards** | Engine | JSON-LD, Turtle, DCAT, PROV-O, OpenLineage, ODCS |
| 9a | **Property-graph interchange** | Engine | GraphML, bulk CSV, Cypher script; one-way sync to external LPG stores |
| 10 | Operability & resource budget ★ | — | Config, health, metrics — enforced footprint |
| 11 | Users, teams, ownership | Catalog | Who is accountable |
| 12 | Authentication | — | Verified identity, human and machine |
| 13 | Authorization & policy | Catalog | Policy-aware access — **gate on activation** |
| 14 | **MCP + outbound events** ★ | Context | **Agents discover, read, and react** |
| 15 | Source connectors | Coll | Scheduled pull from known sources |
| 16 | Ingestion APIs, SDKs, batch & custom adapters | Coll | Push from anything you can write code against |
| 17 | Entity resolution & deduplication | Norm | One asset, one node, however many paths reported it |
| 18 | Inbound events & webhooks | Coll | Sources notify graph-owl on change |
| 19 | Streaming ingestion | Coll | Continuous consumption from brokers |
| 20 | Metadata-as-code ★ | Coll | Declare the graph in git; reconcile continuously |
| 21 | Document & conversation ingestion | Coll | Knowledge out of runbooks, tickets, chats |
| 22 | Custom properties | Norm | Org-specific fields without forking the schema |
| 23 | Domains & data products | Catalog | Organizational structure |
| 24 | Business semantics | Catalog | Glossary, taxonomies, ontologies, metrics as entities |
| 25 | Classification & policies | Catalog | Tags, sensitivity, retention, masking |
| 26 | Lifecycle & certification | Catalog | Draft→retired; certification with issuer and expiry |
| 27 | Data contracts | Catalog | Producer/consumer expectations, compatibility |
| 28 | Usage & popularity | Catalog | What is actually used, by whom |
| 29 | Lineage | Catalog | Movement, dependency, impact |
| 30 | Quality signals & incidents | Catalog | Freshness, validity, incidents, alerts, trust |
| 31 | **Organizational memory** ★ | Catalog | **Why decisions were made** |
| 32 | Agent capabilities ★ | Context | Write-back, investigation, remediation |
| 33 | Domain ontology packs | Catalog | Industry starter vocabularies |
| 34 | Entity expansion | Catalog | Dashboards, pipelines, topics, models, APIs |
| 35 | Collaboration | Catalog | Discussion and proposals on assets |
| 36 | Reference applications | Context | Proof the activation stack works end to end |
| 37 | Scale, portability & embedding | — | 100k entities, export/restore, library use |
| 38 | Graph analytics | Engine | Degree, components, PageRank — orphans, silos, blast radius. **A narrowed reversal**, see below |
| 39 | Console foundation, discovery & entity pages | UI | The shell, auth, search, one composable entity page |
| 40 | **Graph explorer, lineage & time travel** ★ | UI | **The differentiators made visible** — the screen nothing comparable can show |
| 41 | Query workbench, governance & admin | UI | Dual-language query, violations as a workflow, memory, admin |
| 42 | Semantic browse, review queues & agent activity | UI | Glossary/tags/domains as one browser, four review queues as one queue, agent audit |
| 43 | Agent framework integrations | Context | LangChain retriever + LangGraph toolkit and checkpointer, in Python, outside the binary |

## Phase 1 · The engine

### Build order ≠ label order

The letters are **labels, not a sequence**, and a dependency audit found four places where they disagree. Reading `7a → 7b → 7c → 7d` as a build order would deadlock on the second step.

| Build in this order | Because |
|---|---|
| 4 → 5 → 6 | Triples, then shapes over them, then rules over both |
| **7a before 7** | Property paths call `graph-owl-traversal`; Epic 7 does not implement its own walk (`07-engine-query.md` decision 2a). The label says 7a is a sub-part of 7; the dependency runs the other way |
| **7c before 7b** | Cypher lowers onto the LPG projection. Lowering it onto reified triples directly was rejected as an impedance mismatch (`07b-engine-cypher.md`), so 7c is a prerequisite despite sorting after |
| 7b before 7d | Bolt carries Cypher. No Cypher, no Bolt |
| 8, 9, 9a | Independent of the 7-family; schedulable in parallel |

**One dependency crosses a phase boundary**: Epic 7d requires Epic 12's authentication, which is Phase 2. This is deliberate rather than an oversight — a Bolt endpoint is a second listening port on the graph, and shipping it before there is an identity to attach to a session would mean shipping an unauthenticated one. **7d is therefore the one Phase-1 epic that lands after Phase 2**, and its feature flag stays off until then.

Eleven epics — six numbered, five lettered — detailed here because they are new. Phases 0 and 2–7 keep their existing plan files; only numbering changed.

**4 · Triple storage & time-travel ★**

The substrate. `Flake { g, s, p, o, dt, t, op, m }` in `graph-owl-core`; `TripleStore` and `PredicateRegistry` ports in `graph-owl-engine`; a Postgres adapter with **four index orderings** — SPOT, PSOT, POST, OPST. The 4× storage cost is not optional: without all four, common query shapes degrade to full scans.

Entity → flake projection for all four Phase-0 entity types, using the fixed `dsc:` predicate vocabulary. **Reified relationships** — a relationship is a node carrying confidence, provenance, and lineage detail, which a bare predicate assertion cannot hold.

**Time-travel arrives here, free.** `op = false` is a retraction, not a delete, so state at any past `t` is recoverable by construction. This epic ships the as-of query API over that property.

*Subset boundary*: no content-addressed storage, no binary columnar format, no consensus. Postgres handles persistence.

*The trade this epic makes*: relational stays the source of truth and flakes are the graph view, eventually consistent and reconciled one-directionally. Entity CRUD from a triple store would mean reassembling a row from N flakes on every read, and that read is the catalog's commonest operation. The cost is a reconciliation job and a class of drift bugs; the invariant that contains it is **relational wins, always**.

**5 · Constraint validation**

SHACL-like node and property shapes, compiled once and evaluated many times. Continuous validation with a **violation report rather than write-time rejection** — a graph assembled from six asynchronous sources is transiently inconsistent by nature, and rejecting writes would make it unfillable. Severity classification and repair *suggestions*, never automatic repair.

*Why an agent needs it*: reasoning over a contradictory graph produces confident nonsense. Knowing the graph is inconsistent *here* is what lets an agent hedge.

*Subset boundary*: node and property shapes, cardinality, datatype, value-range, pattern. No SHACL-SPARQL.

**6 · Reasoning overlay**

OWL 2 RL forward-chaining: subclass and subproperty hierarchies, transitivity, symmetry, inverse, domain and range.

**Derived facts are an overlay — queryable, never persisted into the base.** This keeps the base clean, bounds reasoning cost, and makes "why do you believe this" answerable: every derived fact names the rule and the source facts that produced it.

Standard rule set: classification propagates along lineage, ownership and domain inherit down containment, certification is invalidated by an upstream breaking change. Epics 11 and 23 currently special-case two of these; this generalizes them instead of accumulating one-offs.

*Subset boundary*: **not a tableau reasoner.** OWL 2 RL is decidable in polynomial time and expressible as forward-chaining rules. OWL 2 DL brings a tractability cliff and an explainability loss. If a rule needs a tableau algorithm, it is out of scope.

**7 · Graph query (SPARQL subset) ★**

Parser, planner, and executor for the subset that serves metadata: basic graph patterns, `FILTER`, `OPTIONAL`, `UNION`, and **property paths** — essential for traversal, and the thing REST endpoints fundamentally cannot express.

Filter pushdown into index scans; BGP matching is homomorphism-based per spec, not subgraph isomorphism. A SPARQL endpoint with the same authorization filtering as every other read path (Epic 13), compiled into the query rather than applied after.

*Subset boundary*: no federation, no entailment regimes, no full algebra. Cypher is a **module** lowering onto the same plan — not a second engine.

**7a · Graph traversal**

Five algorithms over one shared frontier primitive: neighbours, shortest path, all paths, cycle detection, and bounded subgraph. Property paths answer *reachability* and cannot express any of the other four; multi-hop as repeated BGP joins degrades to O(n²).

Its own crate (`graph-owl-traversal`) because four consumers — property paths, lineage, MCP subgraph retrieval, and `sameAs` closure — each need to walk a graph and none should compile a SPARQL parser to do it.

**7b · openCypher front end**

**Status changed from optional to scheduled.** The original reasoning — *SPARQL already covers the capability; a second language is surface without capability* — was sound in isolation and wrong in context, for three reasons: Epic 7d's wire protocol carries Cypher, so no Cypher means no Bolt and no driver ecosystem; Epic 7c gives it a direct lowering target instead of an impedance mismatch against reified triples; and the Epic 41 workbench needs both languages to be credible to both audiences.

Targets **GQL (ISO/IEC 39075)** where openCypher and GQL differ. Remains a module in `graph-owl-query`, sharing its AST, planner, and physical operators.

*Subset boundary*: read-only `MATCH`/`WHERE`/`RETURN` with variable-length paths. No `CREATE`/`MERGE`/`DELETE` — the write path is the API. No APOC-equivalent procedure library.

**7c · Labelled property graph ★**

A bidirectional, loss-enumerated mapping between flakes and the property-graph model: node labels from `dsc:type`, edges from reified relationships, named graphs onto a reserved `_graph` property, element ids **derived from `Sid`** rather than assigned.

*Why this is cheap here and expensive elsewhere*: edge properties are the defining LPG feature, and Epic 4 already reifies every relationship. The expensive half of the mapping is already built and paid for. A store that models relationships as bare predicate assertions would have to retrofit that.

*Subset boundary*: a projection, not a second store. Losses are **enumerated in the mapping report**, not discovered by users.

**7d · Bolt protocol server ★**

PackStream codec, handshake and version negotiation, and a connection state machine, behind a feature flag that is **off by default** because it opens a second listening port.

*Why it is the highest-leverage integration on this roadmap*: every other integration costs one unit of work per integration. Bolt costs one unit of work for an entire ecosystem — a client connects with the driver it already has, runs the Cypher it already knows, and gets time-travel, OWL 2 RL inference, and constraint validation that the database it thinks it is talking to does not have.

*Subset boundary*: server only, never a client. **Read-only.** Same `Principal` and the same compiled authorization predicate as the HTTP and SPARQL paths — a three-way equivalence asserted by test.

**8 · Vector & hybrid search ★**

HNSW vector index and BM25 lexical index as engine capabilities rather than an external cluster. Hybrid ranking; embeddings over entity descriptions, column names, and glossary terms.

*Why in the engine*: "certified financial metrics" and "customer revenue datasets" are the queries agents issue, and neither is a keyword match. Keeping the index in-process preserves the operational-simplicity budget — no required search cluster.

*Subset boundary*: exact-match FQN lookup stays exact. Vector search augments lexical, never replaces it.

**9 · RDF interop & open standards**

JSON-LD expand/compact/frame, Turtle, N-Triples, RDF/XML at the boundary — plus vocabulary conformance: DCAT and DPROD for datasets and products, PROV-O for provenance, OpenLineage for lineage exchange, ODCS for contracts, SHACL shape export.

*The standing decision*: **conform at the boundary, stay property-graph inside.** RDF is how the graph interoperates, not how it is stored. Adopting RDF/OWL as the internal model would trade transactional cascades and predicate-compiled authorization for a serialization property obtainable with a mapping layer.

**9a · Property-graph interchange**

The symmetric other half of Epic 9: GraphML, bulk CSV, Cypher script, JSON Graph, and JSON Lines, each behind its own cargo feature so a deployment compiles only what it uses. Plus a `GraphProjectionTarget` port for pushing a projection into an external property-graph store.

*Two standing decisions*, both narrow on purpose:

- **Sync to an external store is one-directional and lossy by design.** Two writable copies of the graph is two sources of truth, and the second one always wins an argument it should not be in.
- **An external property-graph database is a projection *target*, never a backend.** As a *source* — something graph-owl catalogs — it is a **module in `graph-owl-connectors`**, exactly like every other source. The 100-connectors rule applies unchanged: a crate per graph database reproduces precisely the sprawl the rule exists to prevent.

## Phase 8 · The console

Three epics, detailed here because they reverse a standing "not doing". `plans/00f-ui-architecture.md` is the reference document; it carries the stack, the two-renderer rule, the non-negotiables, and the CI budgets.

**What stands**: the API and MCP surfaces are the product. Every console capability exists in the public API first. The console is a client, never a privileged one, and **never gets an endpoint of its own** — asserted against the OpenAPI document.

**What changed**: a graph engine whose output you cannot see is very hard to evaluate, adopt, or trust. Time travel, inference, and blast radius are all obvious in a picture and nearly incommunicable in JSON. Shipping the differentiators API-only means shipping them undemonstrable.

**The discipline that keeps this from becoming a second product**: a mature console in this category runs to roughly 109 page components and 199 npm dependencies. That is the warning, not the target. The console covers the differentiators and the daily path — not the endpoint list. CI enforces a 40-dependency budget, a 250KB initial bundle, and zero axe violations, as build failures.

**39 · Console foundation, discovery & entity pages** — the shell, OIDC/PKCE auth with tokens in memory only, a **generated** API client, search-first discovery, and **one composable entity page for every entity type** driven by the Epic 3 envelope. A new entity type must be viewable without a UI release; that is asserted by test.

**40 · Graph explorer, lineage & time travel ★** — the starred epic, and the screen that decides evaluations. Budgeted expand-on-click exploration at 10k nodes, layered lineage including column level, impact analysis, derived edges visibly derived, and a **time slider** that renders the estate as it stood on any past date. Every canvas has a keyboard-navigable, screen-reader-labelled non-visual equivalent — in this epic, not later.

**41 · Query workbench, governance & admin** — SPARQL and openCypher in one editor with results as table *or* graph, constraint violations as an **assignable workflow** rather than a report to admire, memory that a human can audit and retract, and admin forms generated from JSON Schema rather than hand-written per connector.

## Scope reality

**37 epics.** Phase 1 alone is substantial — the reference implementation's equivalent capabilities run to ~61,000 lines for SPARQL, SHACL, reasoning, and Cypher combined, before storage and indexing.

The defensible thin vertical is **Epics 1–4, 11–14, 15–17, 31**: foundation, triple storage with time-travel, governance, MCP, one pull connector, one push path, identity resolution, and memory. That yields a governed graph with native history that an agent can query and contribute memory to, populated from a real source.

Note what that path defers: **constraints, reasoning, and SPARQL**. Triple storage alone earns its keep through time-travel and the graph projection; the reasoning and query layers can follow once real usage shows which queries matter. Building all six engine epics before the first agent touches it would be the most expensive possible ordering.

## Not on this roadmap

| Not doing | Why | Reconsider if |
|---|---|---|
| General-purpose graph database | Solved, crowded market; the domain is metadata semantics | Never |
| Full SPARQL 1.1 / OWL 2 DL / SHACL conformance | Hundreds of thousands of lines serving few enterprise needs | A customer requires certified conformance |
| Description-logic / tableau reasoning | Tractability cliff and explainability loss; OWL 2 RL covers the useful subset | A regulated domain requires certified inference |
| RDF/OWL as the **internal** model | Trades transactional and authorization capability for a serialization property | A semantic-web toolchain becomes the primary consumer |
| Second storage backend | Port justified by the in-memory fake; the domain is a transactional graph | Traversal becomes the bottleneck — then a **graph** database |
| Consensus / distributed storage | Single-node serves a single-tenant deployment | Horizontal scale becomes a hard requirement |
| Spatial indexing | Not needed for metadata | A geospatial catalog use case appears |
| RDF streaming (C-SPARQL, RSP-QL) | Epic 19 handles broker consumption; stream algebra is unneeded | — |
| Graph **analytics as a ranking mechanism** — **narrowed, see Epic 38** | The original entry rejected analytics wholesale. That was too broad: three of Epic 38's four algorithms are not ranking at all but **structural** questions no usage signal can answer — orphans, silos, blast radius. What stands is the ranking argument: a table's query count beats its PageRank as an importance signal. PageRank ships **on probation** with a written exit criterion (`38-graph-analytics.md` Slice E) | Already reconsidered. The remaining open question is PageRank's, and the bake-off decides it |
| Community detection, betweenness, embeddings, link prediction | The narrowing above does **not** extend to these. Domains (Epic 24) are the human-assigned answer to "which things belong together", and a human-assigned grouping beats an inferred one for governance | A question appears that degree and components cannot answer |
| Summarization, compression, causal reasoning, neuro-symbolic | Research frontiers with no near-term enterprise application | — |
| A hosted **agent runtime** or prebuilt agents | `00j-language-boundaries.md`: agent frameworks are consumers, not components. Epic 43 ships the *integration* — a retriever, a toolkit, a checkpointer — so a user's LangGraph agent runs in their repo on their schedule | Never. This is the layer `00a-product-position.md` refuses |
| **A workflow / approval engine** (BPMN-style definitions, instances, state machines) | A general workflow engine is a product, not a feature, and it attracts every "could we also route this for approval" request in the building. The approval cases that actually arise are covered without one: Epic 26's lifecycle transitions carry an issuer and an expiry, Epic 35's proposals carry a reviewer, and Epic 20 puts the rest behind pull-request review, which is a workflow engine every organization already runs | A governance requirement needs multi-step routing that lifecycle transitions and PR review demonstrably cannot express |
| A general CLI surface mirroring the API | `20-metadata-as-code.md` bounds the CLI to git-shaped and file-shaped operations. A verb per API capability doubles the maintained surface to reach parity with `curl` | Never as parity; individual commands on their merits |
| Web UI **as the product** — **partially reversed, see Epics 39–41** | What stands: the API and MCP surfaces are the product, every console capability exists in the API first, and the console never gets a private endpoint. What changed: a graph engine whose output you cannot see is very hard to evaluate, adopt, or trust — and the differentiators are disproportionately visual. Recorded in full in `00f-ui-architecture.md` | Already reconsidered. Scope is capped by that document's page-count, dependency, and bundle budgets |
| UI parity with the API surface | Anything reachable by API but rare in daily use stays API-only. Chasing parity is how a console reaches 109 pages | Never |
| A second query front end beyond SPARQL and openCypher | Epic 7d's Bolt server already reaches the whole property-graph tool ecosystem for one unit of work; a third language reaches strictly less for the same cost | A named integration requires it and cannot speak Bolt |
| An external property-graph database as a **backend** | `09a-lpg-interchange.md` decision 7: an external store is a one-directional, lossy **projection target**. Two writable copies of the graph is two sources of truth | Never as a backend; as a target it is already planned |
| Test execution / data profiling | Epic 30 ingests results; computing them crosses into the data plane | Never as compute |
| Reading open **table formats** directly (Iceberg and similar) as an engine capability | A second reference implementation ships this as a first-class integration. Here it is a **connector concern** (Epic 15): graph-owl catalogs the table's metadata, it does not read the table's data. Reading data files is the data plane (`00a-product-position.md`) | Never as a read path; the connector module is already in scope |
| WebSocket real-time push | `35-collaboration.md` decision 4 — polling plus change events suffice for an hours-scale workflow | A sub-second workflow appears |
| **Server-sent events** for live UI updates | A distinct verdict from WebSockets, because SSE is genuinely lighter — one-way, plain HTTP, no second protocol. Still deferred: the console's freshness need is measured in seconds and TanStack Query's background refresh already meets it, so SSE would add a long-lived connection per open tab for no observable gain | The console needs sub-second freshness on a shared surface — the graph explorer during an active ingestion run is the plausible case |
| Object-storage archive destinations (S3 and similar) | `37b-portability.md` writes a stream; where it is written is the operator's pipe. Building destination adapters puts cloud SDKs in the binary and re-implements what every backup tool does | A managed offering needs it |
| Open information extraction | Free-form triples produce an unqueryable graph | Never — extraction stays ontology-constrained |
| Being an identity provider | graph-owl validates tokens, never issues them | Never |
| In-process adapter plugins | Out-of-process via SDK wins on isolation and release cadence | Never |
| Cryptographic verifiability | Epic 4's time-travel delivers auditability | Regulatory tamper-evidence is required |
| Multi-tenancy | Single-tenant per organization assumed | A hosted offering is pursued |
| Training or hosting models | graph-owl provides context *to* models | Never |
| Agent orchestration / workflow engine | graph-owl is the context agents use, not the runtime | Never |

## Plan file work queue

**No code starts on an epic whose plan file is not written and reviewed.**

### Conventions

Match `15-connectors.md`:

```
# Plan: <Epic Name> (Epic N)
**Branch** · **Status** · **Depends on** · **Unblocks**
## Goal · Why here · Resolved decisions · Acceptance criteria
## Slices    Value · Path · Acceptance criteria · RED (incl. mutator watch) ·
             GREEN · REFACTOR assessment · Done when
## Explicitly deferred    each with a destination epic
## Pre-PR quality gate    0 missed mutants, clippy, fmt, epic-specific checks
```

1. Every slice names **the mutant to watch for** — this is what has held 0 missed mutants.
2. Every deferral names its **destination epic**.
3. Every decision carries its **reasoning and revisit trigger**.
4. No slice spans more than one PR.
5. **Never name the reference systems** — see `CLAUDE.md`.

### Tier 1 · Engine + thin vertical

| Epic | File | Action |
|---|---|---|
| 4 | `04-engine-triples.md` | **CREATE** — flake model, four indexes, entity projection, time-travel API, reconciliation |
| 1 | `01-api-conventions.md` | **UPDATE** — JSON Schema publication, full relationship taxonomy with inverses, `sameAs` |
| 14 | `14-mcp-activation.md` | **CREATE** — MCP read capabilities, outbound events, policy filtering per response |
| 16 | `16-ingestion-apis.md` | **CREATE** — push API, SDKs, async batch, custom-adapter guide |
| 17 | `17-entity-resolution.md` | **CREATE** — deterministic + probabilistic matching, reversible `sameAs` merge |
| 31 | `31-memory.md` | **CREATE** — memory objects, link model, provenance and confidence |

### Tier 2 · Engine capabilities

| Epic | File | Action |
|---|---|---|
**File-to-epic exceptions**: Epic 13 has no `13-*.md` — authentication and authorization ship together in `12-13-security.md`. Epic 37 has no `37-*.md` — it is split across `37a-scale.md`, `37b-portability.md`, and `37c-embeddable.md`, which are three independently shippable pieces rather than one epic.

| 5 | `05-engine-constraints.md` | **CREATE** — shape compilation, continuous validation, violation reporting |
| 6 | `06-engine-reasoning.md` | **CREATE** — OWL 2 RL overlay, explainable derivations, standard rule set |
| 7 | `07-engine-query.md` | **CREATE** — SPARQL subset, property paths, six algebra optimizations, authz compilation |
| 7a | `07a-engine-traversal.md` | **CREATE** — `TraversalEngine`, one frontier primitive, five algorithms, `graph-owl-traversal` crate |
| 7b | `07b-engine-cypher.md` | **DONE** — openCypher subset lowering onto the same plan. **Status changed from optional to scheduled**: Epic 7d cannot exist without it, Epic 7c gives it a direct lowering target, and the Epic 41 workbench needs two languages. Targets GQL (ISO/IEC 39075) where the two differ |
| 7c | `07c-engine-lpg.md` | **DONE** — bidirectional flake ⇄ LPG mapping, `graph-owl-lpg` crate; losses enumerated, not discovered |
| 7d | `07d-engine-bolt.md` | **DONE** — PackStream, handshake, state machine, `graph-owl-bolt` crate; read-only, feature-gated off |
| 8 | `08-engine-search.md` | **UPDATE** `08-engine-search.md` — move into engine, add HNSW + hybrid ranking |
| 9 | `09-engine-rdf-io.md` | **CREATE** — JSON-LD, Turtle, DCAT, PROV-O, OpenLineage, ODCS |
| 9a | `09a-lpg-interchange.md` | **DONE** — GraphML, bulk CSV, Cypher script, `graph-owl-lpg-io` crate; external stores are projection targets, and as *sources* they are connector modules |

### Tier 3 · Collection & catalog

| Epic | File | Action |
|---|---|---|
| 18, 19, 21 | `18-inbound-events.md`, `-streaming.md`, `-document-ingestion.md` | **CREATE** |
| 24 | `24-business-semantics.md` | **CREATE** — supersedes glossary in `25-classification.md`; adds Metric entity |
| 25 | `25-classification.md` | **UPDATE** — narrow to tags/sensitivity/policies |
| 26, 27, 28 | `26-lifecycle-certification.md`, `-contracts.md`, `-usage.md` | **CREATE** |
| 30 | `30-quality-results.md` | **UPDATE** — add `Incident` entity, alerts |
| 32, 33, 36 | `32-agent-capabilities.md`, `-ontology-packs.md`, `-reference-apps.md` | **CREATE** |

### Tier 4 · Consistency pass

| Action | Notes |
|---|---|
| Renumber all existing plan files | **DONE.** A cross-reference audit found the earlier pass had corrupted headers and body references — titles carrying another epic's name, seven plans depending on themselves, and cross-references renumbered to the citing file's own number. All 46 plan titles, every `Depends on`/`Unblocks` line, and every in-body `Epic N` reference are now verified |
| **CREATE `plans/00g-operations.md`** | **DONE.** Migration and rollback, backup/DR with RPO and RTO, data retention including personal-data erasure, runbook ownership, the testing levels above unit, and the read/write trait split. Cross-cutting concerns that belonged to no epic and were therefore in no plan |
| Delete superseded plans | `ROADMAP.md (Epic 4 — absorbed)` deleted — absorbed by Epic 4. Graph-integrity and inference plans were never written; they are now Epics 5 and 6 |
| **UPDATE `plans/00c-domain-model.md`** | Add the flake model, `Sid`, predicate vocabulary, reified relationships; lifecycle, certification, incidents, metrics, memory, contracts, usage |
| **DONE** — subset boundaries and standing decisions live in `00a-product-position.md` and `00b-architecture.md`'s decision log |
| **CREATE `plans/graph-owl-crate-architecture.md`** | The ~14-crate growth plan with the earn-your-keep rule |

### Tier 5 · Analytics and console

| Epic | File | Action |
|---|---|---|
| 38 | `38-graph-analytics.md` | **DONE** — four algorithms over a CSR projection, `graph-owl-analytics` crate; PageRank on probation with a bake-off exit criterion |
| — | `00f-ui-architecture.md` | **DONE** — console reference doc: stack, two-renderer rule, non-negotiables, CI budgets, and the explicit not-in-the-console list |
| 39 | `39-ui-foundation.md` | **DONE** — `graph-owl-ui` crate, generated API client, search, one composable entity page, shared trust components |
| 40 | `40-ui-graph-explorer.md` | **DONE** — exploration canvas, lineage, column lineage, time travel and diff, impact, non-visual equivalent |
| 41 | `41-ui-workbench-governance.md` | **DONE** — dual-language workbench, violations as a workflow, memory administration, schema-driven admin forms |
| — | `00h-ui-design-system.md` | **DONE** — tokens, chrome, the five reusable patterns, and a screen inventory mapping every epic to a surface or an explicit "no UI" |
| 42 | `42-ui-semantic-surfaces.md` | **DONE** — the fifteen surfaces the inventory found unassigned, resolved into three patterns |
| 43 | `43-framework-integrations.md` | **DONE** — we ship the integration, never the framework. Python, out of process, zero crate changes asserted |
| — | `00j-language-boundaries.md` | **DONE** — the process boundary is the language boundary. Reverses the Rust-connector decision; keeps MCP in Rust |

### Existing files that stay

`02-entity-hierarchy.md` (2) · `03-versioning.md` (3) · `10-operability.md` (10) · `11-people-and-ownership.md` (11) · `12-13-security.md` (12, 13) · `15-connectors.md` (15) · `20-metadata-as-code.md` (20) · `22-custom-properties.md` (22) · `23-domains.md` (23) · `29-lineage.md` (29) · `34-entity-expansion.md` (34) · `35-collaboration.md` (35) · `37a-scale.md`, `-portability.md`, `-embeddable.md` (37)

## Working agreement

Unchanged from `CLAUDE.md`:

- TDD is non-negotiable: RED → GREEN → MUTATE (`cargo mutants`) → KILL MUTANTS → REFACTOR.
- Never commit without explicit approval, including during autonomous runs.
- Every slice leaves the workspace green: `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
- Mutation target: 0 missed mutants.
- Before implementing any slice, load `tdd`, `testing`, `mutation-testing`, `refactoring`.
