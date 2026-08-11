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
| Reasoning | **OWL 2 EL + RL + QL, profile-detected and routed, incrementally maintained** — see the position change below | Description-logic tableau reasoning, OWL 2 DL/Full |
| Validation | SHACL-like node and property shapes | Full SHACL-SPARQL, advanced features |
| Interop | JSON-LD, Turtle, N-Triples, RDF/XML at the boundary | RDF as the *only* model; RDF-star initially |

This is the single most important scope decision in the project. A production reference implementation of these specifications runs to hundreds of thousands of lines; the subsets that serve enterprise metadata are a fraction of that. **A rule a steward cannot read, or an inference nobody can explain, is not worth its implementation cost.**


## Position change, 28 July 2026: the engine framing is adopted

**The architecture was already this; the positioning had not caught up.** The
flake model with four index orderings, retraction-not-delete, time travel as a
native property, authorization compiled into queries, and reasoning as a
queryable overlay are not a catalog with a relationship table. They are a graph
engine, and they always were. This document now says so.

The consequence is that this project commits to **hosting and reasoning over
large external ontologies** — FIBO, UMLS, SNOMED CT, RxNorm, DBpedia — rather
than only annotating with them. `00n-large-ontology-reality.md` sets out what
that costs. The reasoning row above changes with it: one profile becomes three,
because SNOMED is an **EL** ontology and OWL 2 RL cannot classify it. RL and EL
are incomparable, so an RL run over SNOMED returns a *wrong* hierarchy rather
than a smaller one.

**The dangerous middle was the real problem.** Claiming an engine while planning
a catalog meant none of the necessary arguments could be made: why EL matters
(RL was declared sufficient), why the reasoning budgets need re-deriving (100k
facts was declared generous), why incremental maintenance matters (wholesale
replacement was declared fine). Each follows immediately once the position is
stated.

### Delivered in three phases, and the epics are already sequenced for it

| Phase | What ships | Entry condition |
|---|---|---|
| **1 — Catalog, honestly** | The catalog at ~1M flakes: RL reasoning (Epic 6), wholesale replacement, one profile. **Plus profile detection (Epic 100)** | Now |
| **2 — Clinical and cross-domain ontologies** | EL (98), QL (99), incremental maintenance (97), alignment and UMLS ingestion (104) | Profile detection *refuses* an ontology, or one is scheduled to load |
| **3 — Scale** | Partitioning (4, 37a), re-derived reasoning budgets (6), a second backend only if measured | The `37a` measurements, not a date |

**Two corrections to the obvious phasing, both learned the hard way above.**

**Epic 100 belongs in Phase 1, not Phase 2.** The tempting trigger for EL is
"when a clinical ontology loads" — but by then it is too late: the first SNOMED
load runs on RL and produces a wrong hierarchy *silently*, which is the exact
failure this change exists to prevent. Profile detection is a **syntactic scan,
cheap, and needs no reasoner**. It is what makes shipping Phase 1 with RL-only
an honest position rather than a lucky one: an ontology outside RL is refused by
name instead of quietly mis-reasoned. **Detection is the guard that makes the
phasing safe, so it ships with the thing it guards.**

**"Postgres only" survives; "operationally simple" is what erodes.** The
required-services budget above still holds — Postgres with declarative
partitioning reaches 10⁸–10⁹ flakes, and no second store is warranted until
`37a` measures one. But `00a`'s operational-simplicity claim is a *positioning*
claim, not just an architectural one, and a partitioned multi-hundred-million-row
database with an incrementally maintained materialisation is not "one binary and
a database" in the way this document sells it. The base store is not the binding
constraint at that scale — **the maintained inference set is**, and it can
approach the size of the base. That erosion is stated here rather than
discovered later.

### What is true today, so this change does not become its own dangerous middle

The table above is **committed scope, not shipped state**. As of 28 July 2026,
**Epic 6 has not started**: there is no reasoner of any profile in the codebase.
Claiming "EL + RL + QL" as a capability would repeat the error this section
corrects, pointing the other way. `plans/DEMOS.md` is the authority on what is
built, and `00k-standards-conformance.md` states per-specification conformance
with dates. **This row says what the engine is for. Those two say what it does.**

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

## A pack-level example: the GST pack vs. a reconciliation tool

Added 12 August 2026, prompted by a comparison against a real, focused
GSTR-2B-vs-purchase-register reconciliation product (GST Reconcile:
browser-local Excel matching, 21 deterministic rules, per-supplier
ITC-at-risk ranking). Worth recording because the comparison is easy to
state wrong in either direction — as "graph-owl also reconciles GST", or
as "graph-owl is a GST product" — and both are false for reasons that
follow directly from decisions already made elsewhere in this document.

**graph-owl is not a GST product.** The GST pack (`packs/gst/`) is one of
two proof packs for the domain-neutrality claim in `plans/00l-build-vs-
adopt.md` and `plans/105-domain-neutrality.md` — hospitality is the
other, deliberately unrelated domain, built to the same pack shape to
prove nothing GST-specific leaked into the engine. A reconciliation tool
built *for* GST can specialize its whole data model around GSTR-2B and a
purchase register. graph-owl's engine cannot do that without breaking
the thing DN-3 exists to prove.

**Within that constraint, the honest differentiation is findings vs.
matches, not "better matching."** A dedicated reconciliation tool's core
loop — row-level matching, explainable match reasons, supplier-level
risk ranking — is a genuinely good product shape, already built, and not
a gap graph-owl should try to out-build with more matching rules. What
the flake model, retract-not-delete history, and reasoning-as-overlay
(the four structural positions above) make possible instead is carrying
a match forward into *why*: `Catalog::reconcile_pack`'s findings already
carry `governed_by`/`evidence`/a rule id, and `finding_evidence_graph`
renders the graph that produced a conclusion — not a spreadsheet cell
that says "mismatch."

| | A reconciliation tool | The GST pack, via graph-owl's engine |
|---|---|---|
| Core question | Does this invoice match? | What happened, why, what's the evidence, what's next |
| Match reason | Row-level rule that fired | A finding with `governedBy`, evidence, and a rendered derivation graph |
| History | One reconciliation run | Retract-not-delete — every past run stays queryable |
| Cross-source reasoning | Two files (PR, 2B) | Whatever a pack's own connectors land in the graph — evidence chains span sources by construction, not by a purpose-built join |

**A real, currently-open gap this comparison surfaces honestly: filing
period is not a first-class entity.** `plans/00c-domain-model.md` has no
`FilingPeriod`/similar concept today — every GST fact is a flake with a
transaction time `t`, not an entity a query can traverse from ("show me
everything that changed between April and May"). That is a genuine,
previously-unnamed gap, not something this note should paper over by
claiming month-to-month reasoning already works. Scoped as its own epic
via story-splitting — see the roadmap for where it lands — rather than
asserted here as already true.

**What this note deliberately does not do**: propose repositioning
graph-owl *as* "a GST compliance operating system." That framing treats
one proof pack as the product, which is exactly the domain-neutrality
discipline this document's own scope section (and `00l`/`105-domain-
neutrality.md`) exists to prevent. If GST-specific packaging (a
vertical product built *on* graph-owl, sold to CAs) is ever a real
decision, it is a separate, explicit positioning call — not an implicit
one that falls out of building a better demo pack.

## How to read this alongside the roadmap

`plans/ROADMAP.md` marks differentiator epics with ★. Those are the reason the product exists; cutting one is a positioning decision, not a scope decision.

Everything else is **table stakes** — the capabilities without which the differentiators have nothing to sit on. An engine with excellent time-travel and no way to populate it is not a product.
