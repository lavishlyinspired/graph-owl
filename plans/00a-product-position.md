# graph-owl — Product Position
**Crate scope**: All crates — this document defines what the workspace as a whole competes on.

## Definition

> **A knowledge graph engine that stores, queries, reasons over, and validates enterprise metadata as a connected graph.**

Three layers, one product:

| Layer | What it is |
|---|---|
| **Engine** | Triple store with time-travel, constraint validation, reasoning, and graph query |
| **Catalog** | The enterprise metadata domain model — entities, hierarchy, governance — expressed *in* the graph |
| **Context layer** | Activation for humans and agents: MCP, semantic search, APIs |

The engine is the foundation, not an add-on. The catalog is a domain model over it. The context layer is how both are consumed.

## What changed, and why it matters

This supersedes a positioning as "metadata catalog" and then "enterprise context layer". Both described the *surface*; neither described the *substrate*. The substrate is a graph engine, and that difference is architectural rather than cosmetic:

| | Catalog framing | Engine framing |
|---|---|---|
| Storage | Rows with a relationship table | Triples with four index orderings |
| History | A version-history table | Native — retract-not-delete on every fact |
| Validation | Application-level checks | Constraint shapes evaluated against the graph |
| Derived facts | Special-cased inheritance | Rule-based reasoning over the graph |
| Query | REST endpoints per resource | Graph query, with REST as one projection |

Several capabilities that were separate roadmap epics **collapse into the engine**: time-travel, graph integrity, inference, graph API, and part of standards interop. That is a simplification, not an expansion — it is why the engine goes early despite being large.

## What graph-owl competes on

Four positions, each **structural** — falling out of decisions already made, so a competitor cannot adopt them without changing their architecture.

### 1. A real graph engine underneath the catalog

**Claim**: metadata is stored, queried, and reasoned over as a graph, not as rows with a join table bolted on.

**Why it holds**: the flake model (`g, s, p, o, dt, t, op, m`) with SPOT/PSOT/POST/OPST index orderings, reified relationships carrying confidence and provenance, and retract-not-delete semantics.

**Why it matters**: catalogs that model relationships as a side table can list edges but cannot *reason* over them. Constraint validation, inference, and multi-hop query are engine capabilities, and retrofitting them onto a relational catalog means rebuilding it.

### 2. Time-travel as a native property

**Claim**: query the graph as of any instant, and diff across a range.

**Why it holds**: every flake carries a transaction time `t` and an assert/retract flag `op`. History is not a parallel table that can drift — it *is* the storage model.

**Deliberately not claimed**: cryptographic verifiability. Auditability, not tamper-evidence.

### 3. Metadata-as-code

**Claim**: declare the graph in version-controlled files and reconcile continuously; review metadata in a pull request, roll back with `git revert`.

**Why it holds**: the connector sink already requires FQN-keyed idempotent upsert with converging re-runs. Reconciling a directory is the same machinery pointed at a different source.

### 4. Operational simplicity

**Claim**: one static binary plus Postgres. No JVM, no separate Python runtime, no required search cluster or graph database.

**Why it holds**: Rust, single-process design, and a strict port boundary — the triple store is a trait with a Postgres adapter, so there is no second database to operate.

**The budget**, asserted in CI so the claim cannot decay:

| Metric | Budget |
|---|---|
| Cold start to serving | < 1s |
| Idle RSS | < 100 MB |
| Stripped binary | < 50 MB |
| Required services | Postgres only |
| Direct dependencies | Tracked; increases reviewed |

Budgets are revised deliberately with the reason recorded — never silently raised to make a build pass.

## Scope discipline: the subset principle

The engine implements **useful subsets**, not specifications:

| Capability | In scope | Out of scope |
|---|---|---|
| Query | SPARQL 1.1 subset: BGP, FILTER, OPTIONAL, UNION, property paths | Federation, entailment regimes, full algebra |
| Reasoning | OWL 2 RL forward-chaining as a queryable overlay | Description-logic tableau reasoning, OWL 2 DL/Full |
| Validation | SHACL-like node and property shapes | Full SHACL-SPARQL, advanced features |
| Interop | JSON-LD, Turtle, N-Triples, RDF/XML at the boundary | RDF as the *only* model; RDF-star initially |

This is the single most important scope decision in the project. A production reference implementation of these specifications runs to hundreds of thousands of lines; the subsets that serve enterprise metadata are a fraction of that. **A rule a steward cannot read, or an inference nobody can explain, is not worth its implementation cost.**

## What graph-owl refuses to compete on

| Not competing on | Why | Instead |
|---|---|---|
| Being a general-purpose graph database | The domain is enterprise metadata; general-purpose graph storage is a solved, crowded market | An engine specialized for metadata semantics |
| Connector breadth | A 100-connector library is years of maintenance and the wrong first fight | One excellent connector, a trait others implement, and metadata-as-code for the rest |
| Profiling and test execution | A product in its own right with its own compute story | Ingest results produced elsewhere |
| Full W3C specification conformance | Certification is a multi-year project serving few enterprise needs | Documented, tested subsets |
| Cryptographic verifiability | Ledger machinery is a different product | Time-travel and a complete audit trail |
| A polished web UI | The API and MCP are the product | Published OpenAPI, generated clients, reference apps |
| Multi-tenancy | Single-tenant per organization assumed throughout | — |
| Being the agent runtime | graph-owl supplies context; something else supplies compute and orchestration | MCP and SDKs |

The last one matters most. The temptation in an AI-adjacent product is to grow into the agent runtime. Holding that line is what keeps this composable.

## How to read this alongside the roadmap

`plans/ROADMAP.md` marks differentiator epics with ★. Those are the reason the product exists; cutting one is a positioning decision, not a scope decision.

Everything else is **table stakes** — the capabilities without which the differentiators have nothing to sit on. An engine with excellent time-travel and no way to populate it is not a product.
