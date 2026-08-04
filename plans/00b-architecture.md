# graph-owl — Architecture
**Crate scope**: All crates — this document owns the crate map and layering rules.

Target architecture for the whole system. Sections marked **(built)** exist; everything else is designed but not implemented. `plans/ROADMAP.md` sequences the gap.

## What this system is

> A knowledge graph engine that stores, queries, reasons over, and validates enterprise metadata as a connected graph.

Three layers:

```
┌──────────────────────────────────────────────────────────┐
│ CONTEXT     MCP · semantic search · REST · graph query   │
│             SDKs · events · agent capabilities           │
├──────────────────────────────────────────────────────────┤
│ CATALOG     entities · hierarchy · ownership · domains   │
│             glossary · lineage · contracts · memory      │
├──────────────────────────────────────────────────────────┤
│ ENGINE      triples · time-travel · constraints          │
│             reasoning · query · vector index             │
└──────────────────────────────────────────────────────────┘
                          ↓
                    Postgres (only required service)
```

The engine is the substrate. The catalog is a domain model expressed *in* the graph. The context layer is how both are consumed.

## Layering

Dependencies point one direction. A crate may depend on anything above it in this list, never below.

```
graph-owl-core            pure types: entities, Flake, Sid, predicates. Zero I/O.
   ↑
ports        graph-owl-storage · graph-owl-engine · graph-owl-search
             graph-owl-events · graph-owl-authz
   ↑
pure logic   graph-owl-constraint · graph-owl-reasoning · graph-owl-query
   ↑
adapters     graph-owl-storage-postgres · graph-owl-engine-postgres
             graph-owl-search-hnsw
   ↑
graph-owl-api             Catalog facade — use cases, cross-port orchestration
   ↑
graph-owl-server          axum HTTP + MCP + composition root
```

Four rules that keep this honest:

1. **`graph-owl-api` never names a concrete adapter.** It holds `Arc<dyn Storage>`, `Arc<dyn TripleStore>`. If the facade needs `use graph_owl_engine_postgres::…`, the abstraction is wrong.
2. **Adapters never depend on each other.**
3. **Pure-logic crates have no I/O.** Constraint validation, reasoning, and query planning are functions over data the caller supplies. This makes them exhaustively testable without a database — and it is why they are separate crates rather than modules inside the adapter.
4. **Wiring happens in exactly one place** — `graph-owl-server/src/main.rs`.

## The engine

### Flake model

The unit of storage is a **flake** — a quad with transaction metadata:

```rust
struct Flake {
    g:  Option<Sid>,   // named graph; None = default
    s:  Sid,           // subject
    p:  Sid,           // predicate
    o:  FlakeValue,    // object (polymorphic)
    dt: Sid,           // datatype, or $id for references
    t:  i64,           // transaction time
    op: bool,          // true = assert, false = retract
    m:  Option<Meta>,  // language tag, list index
}
```

**`op` is why history is native.** A deletion asserts a retraction rather than removing a row, so the state at any past `t` is recoverable by construction. Time-travel is not a feature built on top of the store; it is a property of the store.

### Index orderings

Four sort orders over the same flakes:

| Index | Serves |
|---|---|
| `SPOT` | "everything about this subject" — entity reads |
| `PSOT` | "all values of this predicate" — schema-wide scans |
| `POST` | "who has this predicate with this value" — filtered lookup |
| `OPST` | "what points at this object" — reverse traversal (references only) |

The 4× storage cost is not optional. Without all four, common query shapes degrade to full scans — the single most consequential performance decision in the engine.

### Reified relationships

A relationship is a **node**, not a bare predicate assertion:

```
(rel)  rdf:type        dsc:Relationship
(rel)  dsc:fromEntity  (table_a)
(rel)  dsc:toEntity    (table_b)
(rel)  dsc:relType     "feeds"
(rel)  dsc:confidence  0.95
```

Relationships carry payloads — confidence, provenance, lineage detail, SQL. A direct predicate assertion cannot hold them. The cost is two hops to traverse; the benefit is queries like "show every relationship below 0.5 confidence", which the flat form cannot express at all.

### Hybrid storage: relational as source of truth

Entities are written **relationally first**, then projected into flakes.

| | Relational | Flakes |
|---|---|---|
| Role | Source of truth | Graph view |
| Serves | Entity CRUD, list, filter, FQN lookup | Graph query, reasoning, constraint validation, traversal |
| Consistency | Immediate | Eventually consistent, reconciled |

**This is a deliberate trade with a real cost.** Dual representation means two write paths, a reconciliation job, and a class of drift bugs that neither store has alone. It is chosen because entity CRUD against a triple store requires reassembling a row from N flakes on every read, and the catalog's commonest operation is exactly that read.

The invariant that makes it safe: **relational wins.** Any divergence is repaired by re-projecting from relational, never by writing to the relational store from flakes. Reconciliation is one-directional by construction.

### Scope: subsets, not specifications

| Capability | Implemented | Not implemented |
|---|---|---|
| Query | SPARQL subset: BGP, FILTER, OPTIONAL, UNION, property paths | Federation, entailment regimes |
| Reasoning | **OWL 2 EL + RL + QL, profile-detected and routed**, as a queryable **overlay** (`00a`, 28 Jul 2026) | Tableau reasoning, OWL 2 DL/Full |
| Validation | Node and property shapes | Full SHACL-SPARQL |
| Interop | JSON-LD, Turtle, N-Triples at the boundary | RDF as the sole internal model |

Derived facts are an **overlay**: queryable, never persisted into the base.

**One clause of that changes at engine scale, and it is worth separating the two properties it bundles.** "Never persisted into the base" — derived facts stay distinguishable from asserted ones and are never confused with them — **holds, permanently**; it is what makes explanation possible. "Recomputed wholesale on every run" does **not** hold above ~10⁸ triples, where re-deriving a materialised closure is hours rather than seconds. Those are separable, and only the second gives way: the overlay becomes **incrementally maintained** (Epic 97) while remaining a separate named graph. `00n-large-ontology-reality.md` §2.4 has the arithmetic. This keeps the base clean, keeps reasoning bounded, and makes "why do you believe this" answerable — every derived fact names the rule and the source facts that produced it.

## Crate plan

Crates are created when they **earn their keep** — a distinct dependency set, or a compile-time boundary that matters. They are not pre-allocated from a topic taxonomy.

**(built)**

| Crate | Holds |
|---|---|
| `graph-owl-core` | `Table`, `TableUpdate`, `Relationship` |
| `graph-owl-storage` | `Storage` trait, `StorageError` |
| `graph-owl-storage-postgres` | sqlx impl, refinery migrations |
| `graph-owl-api` | `Catalog` facade |
| `graph-owl-server` | axum router, handlers, `AppError` |

**Planned — added as the phase that needs them arrives**

| Crate | Holds | Phase |
|---|---|---|
| `graph-owl-engine` | `TripleStore`, `PredicateRegistry` traits; `Flake`, `Sid` | 1 |
| `graph-owl-engine-postgres` | Flake table, four index orderings, pattern matching | 1 |
| `graph-owl-ontology` | Shape and axiom types; ontology definition | 2 |
| `graph-owl-constraint` | Shape compilation and validation (pure) | 2 |
| `graph-owl-reasoning` | OWL 2 RL forward-chaining (pure) | 3 |
| `graph-owl-query` | SPARQL parse, plan, execute (pure) | 4 |
| `graph-owl-search` + `-hnsw` | Vector index port and adapter | 5 |
| `graph-owl-rdf-io` | JSON-LD, Turtle, N-Triples | 6 |
| `graph-owl-resolution` | Entity resolution, coreference, temporal | 7 |
| `graph-owl-connectors` | `Connector` trait + module per source | 8 |
| `graph-owl-mcp` | MCP server | 9 |
| `graph-owl-cli` | metadata-as-code, admin, DevOps | 10 |

**~14 crates at maturity.** Sizing sanity check: a production reference graph engine runs ~842k lines across 32 crates — roughly 26k lines per crate. A 28-crate plan for this project would average under a hundred lines per crate, which is a taxonomy, not an architecture.

Deliberately **not** crates: causal reasoning, summarization, compression, distributed processing, graph analytics, neuro-symbolic, RDF streaming. Each maps to a roadmap item that is out of scope; an empty crate is a maintenance cost with no offsetting benefit. Cypher, if built, is a module inside `graph-owl-query` because it lowers to the same plan.

### Storage backends vs. connectors

Different problems, different granularity:

- **Storage backends** — where graph-owl's own data lives. Bounded to one. Deep integration: transactions, cascades, index management.
- **Source connectors** — systems graph-owl *describes*. Potentially 100+. Shallow, read-only introspection.

Postgres is both, in opposite roles: it stores the graph, and it can be catalogued. One crate per backend; one *module* per connector.

## Cross-cutting patterns

### The entity envelope

Every entity carries the same metadata-about-metadata — version, timestamps, authorship, tombstone, owners, tags. One `EntityEnvelope` in `graph-owl-core`, flattened with `#[serde(flatten)]`, one identical column set per entity table, and a fixed predicate vocabulary in the graph projection.

Retrofitting this after twenty entity types is the most expensive thing the project could defer, which is why it lands in Phase 0.

### Error model

```
StorageError / EngineError   (ports)
   ↓ From
CatalogError                 (facade)  — domain-meaningful failures
   ↓ From
AppError → HTTP              (server)  — status + RFC 9457 problem+json
```

The facade is where a port-level conflict becomes "this fully-qualified name is taken", because only the facade knows which operation was attempted. Handlers map; they do not decide.

### Testing strategy

Four levels, no mocks:

| Level | Backed by | Covers |
|---|---|---|
| Pure | Nothing — plain functions | Constraint evaluation, reasoning, query planning, FQN derivation |
| Repository | Real Postgres via testcontainers | SQL, constraints, migrations, index behavior |
| Facade | In-memory `Storage` + `TripleStore` fakes | Orchestration, invariants, error mapping |
| HTTP | Real Postgres + full router | Status codes, wire format, routing |

The pure level is why constraint, reasoning, and query are separate crates: they are the highest-stakes logic in the system and they are testable exhaustively without I/O.

Every production change follows RED → GREEN → MUTATE (`cargo mutants`) → KILL MUTANTS → REFACTOR, at 0 missed mutants.

## Decision log

| # | Decision | Rationale | Revisit if |
|---|---|---|---|
| 1 | Rust, edition 2024, workspace | Type safety and performance for long-lived infrastructure | — |
| 2 | axum 0.8, not 0.7 | 0.7's `FromRequest` is edition-2021 RPITIT; fails `E0195` from an edition-2024 crate | — |
| 3 | Postgres only; `Storage`/`TripleStore` traits from day one | Traits cost little and keep a second backend additive | — |
| 4 | Real Postgres in tests, no mocks | Catches SQL, constraint, and migration bugs mocks hide | Suite wall-clock becomes intolerable |
| 5 | refinery migrations | Embedded SQL, no deploy-time tooling. Needs a separate `tokio_postgres` client | — |
| 6 | 400, not axum's 422, for malformed bodies | Consistency across the error surface, via `AppJson<T>` | — |
| 7 | PATCH via DTO shape, not JSON Patch | Immutable fields excluded structurally; server diffs state for change tracking | Deeply nested partial updates make DTOs unwieldy |
| 8 | Cursor pagination | Offset drifts under concurrent insert and degrades at scale | — |
| 9 | RFC 9457 problem+json | Standard, extensible, no consumers to break | — |
| 10 | **Flake model with four index orderings** | Native time-travel; without all four, common queries full-scan | — |
| 11 | **Reified relationships** | Edges carry confidence, provenance, lineage detail | — |
| 12 | **Relational source of truth, flakes as view** | Entity CRUD from a triple store needs N-flake reassembly per read | Graph query becomes the dominant access pattern |
| 13 | **Subsets, not specifications** | Full SPARQL/OWL/SHACL is hundreds of thousands of lines serving few enterprise needs | A customer requires certified conformance |
| 14 | **Reasoning as a queryable overlay, never persisted** | Keeps the base clean, bounds reasoning cost, keeps derivations explainable | — |
| 15 | **~14 crates, grown on demand** | A crate is a dependency boundary, not a topic label | A crate's dependency set genuinely diverges |
| 16 | **Property-graph-with-triples, not RDF-native** | RDF is the interchange format; the internal model keeps transactional and authorization capability | A semantic-web toolchain becomes the primary consumer |
| 17 | **Cytoscape.js for exploration, superseding Sigma.js** | Sigma was chosen for being the WebGL option; Cytoscape has shipped a WebGL renderer since v3.31, so the deciding property is now common to both. Cytoscape adds deterministic built-in layouts, which `00f` requires for testing and Sigma would make us hand-write. Both MIT | Cytoscape's WebGL renderer misses the 10k-node budget on a CI fixture — a measurement, not an assumption |
| 18 | **d3-dag for lineage layout, not `elkjs`** | `elkjs` is EPL-2.0; `00i` rejects copyleft by default. d3-dag is MIT, TypeScript-first, and far smaller. ELK's edge-routing advantage is mostly unrealised in React Flow, which consumes node positions and draws its own edges | A measured legibility failure at a fork on a real lineage fixture that d3-dag cannot fix |
| 19 | **Still exactly two graph renderers; the threshold-switching hybrid is rejected** | Two libraries for one graph *shape* is the accretion `00f` consequence 2 forbids, and a mid-session swap discards the layout exactly when the user's mental map is what keeps a large graph legible | A third graph *shape* appears that neither renderer handles |
| 24 | **A second flake backend sits beside Postgres, never instead of it** | Decision 12 makes the relational store the source of truth, so only the `TripleStore` port has a plausible second adapter — `Storage` does not. A diagram showing Postgres/RocksDB/native as alternatives implies a migration that removes Postgres, and that migration does not exist | — |
| 25 | **The relational store stays the transaction-clock authority across backends** | `next_time()`/`time_at()` mint a monotonic `t`. With two stores, a `t` minted outside the source of truth could not be compared against it. Invisible until a second adapter exists, expensive to discover then | — |
| 21 | **The engine positioning is adopted; reasoning is EL + RL + QL, profile-routed** | The architecture was already an engine — flakes, four orderings, native time travel, authz-in-query, reasoning overlay. Claiming a catalog pre-refuted every argument the engine needs (why EL, why bigger budgets, why incremental). SNOMED is EL and RL **cannot** classify it: incomparable profiles, so RL yields a wrong hierarchy rather than a partial one | A customer set never includes an ontology outside RL — which `00n` argues is already false |
| 22 | **Profile detection (Epic 100) ships in Phase 1, before any reasoner it routes to** | It is a syntactic scan needing no reasoner, and it is what makes shipping RL-only honest: an out-of-profile ontology is refused by name rather than silently mis-reasoned. Triggering EL on "when a clinical ontology loads" is one load too late | — |
| 23 | **"Postgres only" holds; "operationally simple" is what erodes at scale** | Partitioned Postgres reaches 10⁸–10⁹ flakes, so no second store is warranted until `37a` measures one. But the binding constraint at that scale is the **maintained inference set**, which can approach the size of the base — and that is not "one binary and a database" in the way `00a` sells simplicity | `37a` measures a partitioned Postgres missing the write-latency target |
| 20 | **The npm allowlist is the crate allowlist, checked in CI** | The console is embedded in the binary, so an npm package ships in the artifact `cargo deny` protects — by a path `cargo deny` cannot see. Found via the `elkjs` near-miss in decision 18 | — |
| 26 | **`graph-owl-api` is embeddable in the narrow sense Epic 37c Slice A checks, not in the sense "compiles light"** | Slice A's CI check (`scripts/check-embedding-boundary.py`) asserts `api` reaches every storage/search backend only through the `graph-owl-storage` port — true today, and enforced. But `api` also depends directly on `graph-owl-connectors`, which pulls `tokio`, `sqlx`, `rdkafka`, `pulsar` and `csv` — real I/O and a second async runtime dependency, none of it behind a port. And `Catalog::cypher_stream` (Epic 7d) calls `tokio::task::spawn_blocking`, which panics outside an active tokio runtime — so an embedder on a different executor cannot call that one method, even though nothing in `api` *constructs* a runtime (decision 3's actual bar). Recorded rather than fixed per `37c-embeddable.md`'s own instruction: connectors are a source-ingestion concern with 100+ future modules, and moving that boundary is a real design question — which trait the storage-agnostic embedding surface actually needs — not a dependency to quietly strip | An embedder reports the connector/runtime weight as a real blocker, or Epic 34's entity-family growth makes `api`'s surface itself worth splitting |

## Reference research

Architecture research used local clones under `.claude/docs/referenceRepo/` (gitignored, never committed). Per `CLAUDE.md`, those systems are never named in committed files — where their design informed a decision, this document records the pattern and the reasoning instead of the source.

### Systematic review: 22 patterns

A full cross-reference of both reference architectures against every plan file produced 22 patterns present there and not obviously present here. Each is recorded with a verdict, because **an unexamined absence is worse than a rejection** — a rejected pattern is a decision, an absent one is a surprise during an incident.

Two of these reference systems occupy different layers: one is an engine (storage, indexing, query, policy), the other a catalog (entities, governance, ingestion). graph-owl spans both, which is why the review had to cover both and why roughly half the patterns were already present under different names.

**Engine-layer patterns**

| Pattern | Verdict | Where |
|---|---|---|
| Resource metering during query execution | **Adopted, simplified** | `Tracker` with atomic counters in `07-engine-query.md`. A micro-fuel schedule with per-operation weights is **rejected for now**: inventing weights without benchmarks produces a number that looks authoritative and is not. Revisit in `37a-scale.md`, where there are measurements to calibrate against |
| Idempotency cache | **Already present** | `16-ingestion-apis.md` Slice B — key, TTL, replay semantics, different-body `409` |
| **Admission control under overload** | **Gap — adopted** | Added to `10-operability.md`. Idempotency answers "did I already do this"; it does not answer "should I accept this at all right now" |
| Policy deny-overrides | **Already present** | `12-13-security.md` decision 4 |
| **Policy/schema bypass for bootstrap** | **Gap — adopted** | `12-13-security.md` decisions 8–9. Policies live in the graph, so evaluating one requires reading one; without a cut the first policy is unenforceable |
| RAM-aware, cgroup-aware cache budget | **Gap — adopted** | `10-operability.md`. A container sizing its cache from host memory is OOM-killed on first load |
| OTLP telemetry, graceful shutdown, request-id propagation | **Already present** | `10-operability.md` decision 6, Slices C and E |
| Health states beyond binary | **Gap — adopted** | `10-operability.md`: required vs optional checks, `200 degraded`. Forcing a dead search index into "not ready" converts a degraded feature into an outage |
| Dynamic multi-ledger registry with per-ledger health | **Rejected** | The reference hosts many independent ledgers in one process; graph-owl is **one graph per deployment**. A registry keyed by a name there is always one entry here, and the health it tracks is the process health `/ready` already reports |
| Pluggable connection abstraction (file / memory / cloud backends) | **Rejected — already have it** | `Storage` and `TripleStore` *are* that abstraction, one layer up. A second pluggable layer beneath them would abstract `sqlx::PgPool`, which is already an abstraction over a connection |
| Content-addressed storage, binary columnar format, novelty overlay, snapshot isolation | **Rejected — Postgres owns these** | Recorded in `04-engine-triples.md`. Each exists because that engine owns its storage layer; this one does not, and reimplementing MVCC on top of MVCC is a losing trade |

**Catalog-layer patterns**

| Pattern | Verdict | Where |
|---|---|---|
| Schema-first design with code generation | **Adopted at the boundary, rejected inside** | Types are Rust-native; the *contract* is generated — OpenAPI from code (`01`), console client from OpenAPI (`39`), connector config from JSON Schema (`15`), admin forms from the same (`41`). Generating Rust types from JSON Schema would invert the source of truth and lose the type system's expressiveness |
| FQN hierarchy, `EntityReference` lightweight refs | **Already present** | `00c-domain-model.md` |
| Enumerated relationship taxonomy | **Already present** | `00c-domain-model.md` — 26 stored types with inverses, versus the reference's 26. Convergent, not copied |
| **Relationship-type wire stability** | **Gap — adopted** | `01-api-conventions.md`. An enum persisted by ordinal cannot be reordered or have members removed |
| **A shared entity-resource pattern** | **Gap — adopted** | `01-api-conventions.md`. 25 entity types × hand-written CRUD is the same avoidable cost as hand-written connector forms |
| Cursor pagination | **Already present** | `01-api-conventions.md` decision 2 |
| Change events with field-level diffs | **Already present** | `03-versioning.md` — `ChangeDescription` |
| WebSocket push for real-time UI | **Rejected** | `35-collaboration.md` decision 4. A large operational addition for a workflow measured in hours |
| Table- and column-level lineage with SQL and pipeline payload | **Already present** | `29-lineage.md` |
| Hierarchical glossary with SKOS relations | **Already present, and stronger** | `24-business-semantics.md` uses actual SKOS (`broader`/`narrower`/`related`/`exactMatch`) rather than invented relation names, so Epic 9's export is a mapping rather than a translation |
| Hierarchical classifications with mutual exclusivity | **Already present** | `25-classification.md` |
| **Operation-level RBAC and decision caching** | **Gap — adopted** | `12-13-security.md`. Row and column filtering were planned; a named operation vocabulary was not, and "can this principal edit tags on this asset" is unanswerable without one |
| **BPMN-like workflow and approval engine** | **Rejected, now on the record** | `ROADMAP.md`. A workflow engine is a product; the approval cases that actually arise are covered by Epic 26 lifecycle transitions and Epic 35 proposals |
| **Reusable test definitions and test suites** | **Gap — adopted** | `30-quality-results.md`. Every test case being bespoke means the same freshness check is registered a thousand times with a thousand names |
| Lifecycle created/updated/accessed tracking | **Already present, deliberately split** | `updated_at`/`updated_by` in the envelope; `created_at` recoverable from version `0.1` (`00c`); *accessed* is Epic 28's usage signals, which is a time series rather than a field — a single `last_accessed` on the entity is a write on every read |
| **Declarative ingestion topology** | **Gap — adopted** | `15-connectors.md`. Traversal order was implicit in each connector's code |
| Source fingerprinting for incremental sync | **Gap — adopted** | `15-connectors.md` decision 7 |
| Per-connector connection schema and dynamic discovery | **Gap — adopted** | `15-connectors.md` decisions 8–9 |

**Score**: 11 already present, 9 real gaps now closed, 6 rejected with reasoning, 1 adopted in simplified form. The gaps clustered in exactly the places one would predict — cross-cutting operational concerns owned by no epic, and the second-order costs of scale (25 entity types, 100+ connectors, 1000s of test cases) that only appear once the first instance works.

### Open questions the review raised, and their answers

| # | Question | Answer |
|---|---|---|
| D1 | Does `Flake` need a `dt` datatype field? | **No.** A `value_type` discriminant column indexes better and the datatype set is closed (`04`) |
| D2 | Does `Flake` need a metadata field for language tags and list indices? | **Not on the flake** — a sparse `flake_meta` side table (`04`). The need is real; widening the hottest row in the system to serve a minority of values is not the way to meet it |
| D3 | Which `FlakeValue` variants ship in v1? | **Ten.** The original seven plus `Bytes`, `Uuid`, `Duration`; big-number and geo variants rejected with reasoning (`04`) |
| D4 | Should `sameAs` use union-find? | **No.** Tuple dedup is sufficient at metadata scale, and union-find would need to be rebuilt per reasoning run since the overlay is never persisted (`06`) |
| D5 | Should reasoning have a memory budget? | **Yes** — `max_memory_bytes`, because the fact set is held in memory and Postgres's `work_mem` does not bound it (`06`) |
| D6 | How should the console epics be numbered? | **Their own epics, 39–41**, in a ninth phase (`ROADMAP.md`) |
| D7 | Does graph-owl need change events and an audit trail? | **Already had them** — `03-versioning.md`. The review's premise was stale |
| D8 | Should lineage be table-level only? | **Table and column, with SQL payload** — already planned (`29`). Column-level is the differentiator half |
| D9 | Does graph-owl need a glossary model? | **Already had one**, on SKOS (`24`) |
| D10 | Should the envelope track `created_by` and `accessed_by`? | **Split.** Creation is recoverable from version `0.1`; access is a time series (Epic 28), not a field. A `last_accessed` column turns every read into a write |
| D11 | Cursor or offset pagination? | **Cursor**, decided at Epic 1 (`01` decision 2) |
| D12 | Should there be a CLI? | **Yes, and its scope is now bounded** — `20-metadata-as-code.md`. It was already a crate with no stated boundary, which is how a CLI reaches 40 subcommands |
