# Plan: Labelled Property Graph Projection (Epic 7c) ★

**Branch**: feat/engine-lpg
**Status**: Slices A–C built (nodes, element ids, edges) in `graph-owl-lpg`; the reverse direction (`flakes_from_node`/`flakes_from_edge`) and Slice D+ are not
**Depends on**: Epic 4 (flakes), Epic 1 (relationship taxonomy)
**Unblocks**: Epic 7b (Cypher), Epic 7d (Bolt), Epic 9a (LPG interchange), Epic 40 (graph explorer UI)
**Crates**: **`graph-owl-lpg`** (new — pure model + bidirectional mapping)

## Goal

Expose the graph as a **labelled property graph** — nodes with labels and properties, edges with types and properties — without a second store, so property-graph tooling, drivers, and query languages work against graph-owl directly.

## Why this is worth doing, and why it is cheap

The two data models are usually presented as a choice. They are not, for this graph specifically:

- **Edge properties are the defining LPG feature**, and the thing plain RDF cannot do without reification. Epic 4 decision 4 **already reifies every relationship** — each edge is a node carrying `dsc:confidence`, provenance, and lineage detail. The expensive half of the LPG mapping is therefore already built and paid for.
- **Node labels** map to `dsc:type` assertions; a node may hold several, which RDF permits and LPG expects.
- **Properties** are literal-valued flakes on the subject. Same thing, different vocabulary.

**Correction to an earlier claim in this file.** It previously read *"neither reference implementation does LPG"*. That was wrong, and a re-inspection of the source found the opposite: the engine reference carries a **~10,000-line openCypher front end** (lexer, parser, validator, diagnostics) and a **~2,600-line Bolt server** with exactly the module decomposition planned in `07d-engine-bolt.md` — PackStream, handshake, session, chunking, message, value.

That changes the framing in three ways, all of which strengthen the plan rather than weaken it:

1. **The design is validated, not speculative.** An independent implementation over a flake-shaped store converged on the same decomposition. Convergence is evidence.
2. **The cost is now bounded by something real.** ~10k lines for a Cypher front end and ~2.6k for Bolt are the reference points to estimate against, rather than guesses.
3. **This is no longer a differentiator claim, and this plan must not make one.** What remains genuinely distinctive is not "a graph engine that speaks Cypher" — it is a *metadata catalog* that does, over a store with time travel, OWL 2 RL inference, and constraint validation. The catalog reference has no graph query surface at all.

The capability is still worth building for the reasons in this plan. It is not worth claiming novelty for.

What the mapping buys, concretely: openCypher (Epic 7b), the Bolt protocol (Epic 7d), and with it the entire property-graph tool ecosystem — browsers, visualizers, BI connectors, drivers in every language — none of which requires graph-owl to write an adapter.

## Resolved decisions

1. **A projection, not a second store.** Flakes stay the single source of graph truth. `graph-owl-lpg` is a pure mapping evaluated on demand. A materialized parallel property graph would be a second thing to keep consistent, which is the failure mode Epic 4 decision 1 already spent its complexity budget avoiding.
2. **The mapping is bidirectional and its losses are enumerated, not discovered.** Round-tripping RDF → LPG → RDF must be lossless for everything graph-owl itself stores. Where a general RDF graph cannot survive the trip (blank nodes, literal-valued predicates used as edges, named graphs), the plan says so explicitly and the code reports it rather than dropping it silently.
3. **Reified relationships surface as edges with properties, not as nodes.** The two-hop reification is an implementation detail of the flake layer. An LPG consumer that sees `(:Table)-[:FEEDS {confidence: 0.9}]->(:Table)` is seeing the truth of the model; one that sees three nodes is seeing the encoding.
4. **A relationship node is still addressable as a node when asked for.** Provenance, review workflow, and Epic 31 memory all link *to* a relationship. `MATCH (r:Relationship)` must therefore work. Both views of the same object are legitimate; the mapping supports both and the plan says which one is default (edge).
5. **Named graphs become a reserved node property, not a fourth element.** LPG has no quad. `_graph` on nodes and edges preserves Epic 4's `graph:extraction` / `graph:reasoning` / `graph:import:{source}` scoping through the projection. Reserved names are prefixed `_` and rejected as user property names.
6. **Labels are derived from `dsc:type`, never stored twice.** A node's label set is exactly its type assertions plus its entity-kind. Storing a separate label list would drift from the types on the first schema change.
7. **Type coercion is explicit and total.** Every `FlakeValue` variant maps to a declared LPG property type, and every LPG property type maps back. `FlakeValue::Ref` in a *property* position (not an edge) becomes an element-id string, which is the one genuinely lossy direction and is documented as such.

## Implementation reference

```rust
// graph-owl-lpg — pure, no I/O
pub struct LpgNode {
    pub element_id: ElementId,          // stable, derived from Sid — not an integer counter
    pub labels: Vec<Label>,             // from dsc:type + entity kind
    pub properties: PropertyMap,
}

pub struct LpgEdge {
    pub element_id: ElementId,          // the reified relationship's Sid
    pub edge_type: EdgeType,            // from dsc:relType
    pub start: ElementId,               // dsc:fromEntity
    pub end: ElementId,                 // dsc:toEntity
    pub properties: PropertyMap,        // confidence, provenance, lineage detail
}

pub struct PropertyMap(BTreeMap<PropertyKey, PropertyValue>);   // BTree: deterministic order

pub enum PropertyValue {
    Null, Boolean(bool), Integer(i64), Float(f64), String(String),
    Bytes(Vec<u8>), Date(NaiveDate), DateTime(DateTime<Utc>), Duration(Duration),
    List(Vec<PropertyValue>), Map(BTreeMap<String, PropertyValue>),
    ElementRef(ElementId),              // a Ref in property position
}

pub trait LpgProjection {
    fn node_from_flakes(&self, subject: &Sid, flakes: &[Flake]) -> Result<LpgNode, MappingError>;
    fn edge_from_reified(&self, rel: &Sid, flakes: &[Flake]) -> Result<LpgEdge, MappingError>;
    fn flakes_from_node(&self, node: &LpgNode, t: i64) -> Result<Vec<Flake>, MappingError>;
    fn flakes_from_edge(&self, edge: &LpgEdge, t: i64) -> Result<Vec<Flake>, MappingError>;
}

pub struct MappingReport {          // decision 2: losses are reported, never silent
    pub lossy: Vec<LossyMapping>,   // BlankNode | LiteralPredicate | NamedGraphCollapse | RefInProperty
}
```

### The mapping, stated once

| Property graph | Flake model | Round-trips |
|---|---|---|
| Node | Subject `Sid` with ≥1 `dsc:type` | Yes |
| Node label | `dsc:type` object | Yes |
| Node property | Literal-valued flake on the subject | Yes |
| Edge | Reified relationship node | Yes |
| Edge type | `dsc:relType` | Yes |
| Edge property | Literal-valued flake on the relationship | Yes |
| Element id | Derived from `Sid`, stable across restarts | Yes |
| Named graph | `_graph` reserved property | Yes |
| Transaction time | `_t` reserved property | Yes (read-only) |
| — | Blank node | **No** — reported `BlankNode` |
| — | Literal-valued predicate used as an edge | **No** — reported `LiteralPredicate` |
| Multi-valued property | Repeated flakes on one predicate | As a `List`, order not preserved |

**Element ids are derived from `Sid`, not assigned.** An auto-incrementing integer id — the conventional property-graph choice — would not survive a restart, would not be stable across replicas, and would give a Bolt client (Epic 7d) a handle that silently means something different after a reindex. Derivation costs an encode on every projection and is worth it.

### Time-travel through the projection

`as_of` passes straight through: the projection takes the flakes the caller resolved, so a historical LPG view is the same code over different input. This is the capability no property-graph database in the landscape offers, and it falls out of Epic 4 for free rather than needing design here.

## Acceptance criteria

- [ ] Every row of the mapping table is implemented and tested in both directions.
- [ ] RDF → LPG → RDF round-trips losslessly for everything graph-owl stores.
- [ ] Lossy cases are **reported** in a `MappingReport`, never dropped silently.
- [ ] A reified relationship projects as an edge with properties by default, and as a node on request.
- [ ] Element ids are derived from `Sid`, stable across restart, and reversible.
- [ ] Named graphs survive as `_graph`; reserved property names are rejected as user keys.
- [ ] `as_of` produces a historical property-graph view.
- [ ] `graph-owl-lpg` performs **zero I/O** — asserted by the dependency check.
- [ ] Property ordering is deterministic, so serializations are byte-stable.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`, `testing`, `mutation-testing`, `refactoring` loaded first.

### Slice A: Nodes project, with labels and properties

**Acceptance criteria**: a subject's flakes project to an `LpgNode`; multiple `dsc:type` assertions produce multiple labels; every `FlakeValue` variant maps to its declared `PropertyValue`; a repeated predicate becomes a `List`; property order is deterministic across two projections of the same input; a subject with no type assertion → a specific error, not an unlabelled node.
**RED**: A total mapping test — one case per `FlakeValue` variant, so adding a variant without mapping it fails to compile or fails the test. A determinism test projecting twice and comparing bytes. Mutator watch: a `HashMap` instead of `BTreeMap` must fail determinism; a missing variant arm must fail totality.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Element ids are derived and reversible

**Value**: Every downstream surface — Bolt, Cypher, the UI, GraphML export — hands these to clients as handles. Getting them wrong is a correctness bug that only shows up after a restart.
**Acceptance criteria**: `Sid → ElementId → Sid` round-trips exactly; ids are stable across process restarts (asserted by computing from a fixture, not from state); two different `Sid`s never collide; the encoding survives namespace codes and ids containing separator characters; an unparseable id → error, never a silent miss.
**RED**: A separator-injection test — an entity id containing whatever character the encoding uses as a delimiter. This is the classic encoding bug and it produces cross-entity id collisions, which is the worst possible failure here. Mutator watch: naive concatenation must fail the injection test; an id derived from anything process-local must fail the restart-stability test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Reified relationships project as edges

**Value**: The decision that makes graph-owl an LPG at all.
**Acceptance criteria**: a relationship node with `dsc:fromEntity`/`dsc:toEntity`/`dsc:relType` projects to one `LpgEdge` with the correct endpoints and direction; `dsc:confidence` and provenance appear as edge properties; the relationship is **also** addressable as a node when requested; a relationship missing an endpoint → a reported error naming the missing side; direction is never inverted.
**RED**: A direction test asserting `from` and `to` are not swapped — an inverted lineage edge is a wrong answer that looks entirely plausible. A dual-view test asserting the same relationship is reachable both as an edge and as a node. Mutator watch: swapping the endpoint fields must fail; projecting the relationship only as three nodes must fail the edge assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: The reverse direction, with a loss report

**Acceptance criteria**: `LpgNode`/`LpgEdge` → flakes, at a caller-supplied `t`; a full RDF → LPG → RDF round trip over a fixture covering every entity type is byte-identical; each enumerated lossy case produces its named `LossyMapping` entry; a `MappingReport` with entries does **not** fail the operation — it annotates it; writing a reserved `_` property → rejection naming the key.
**RED**: The round-trip fixture is the specification for decision 2. One test per lossy case asserting the *specific* variant is reported — a generic "something was lost" is useless to a caller deciding whether to proceed. Mutator watch: dropping a lossy case silently must fail; failing the whole operation on a loss must fail the annotate-not-fail assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Named graphs and time-travel survive

**Acceptance criteria**: `graph:reasoning`-scoped facts project with `_graph` set, so a consumer can tell a derived edge from an asserted one; `_t` exposes transaction time read-only; an `as_of` projection returns the historical property graph; a node deleted after `as_of` is present in the historical view; `_graph` and `_t` are rejected as user-supplied property keys on the write path.
**RED**: The derived-edge test matters most: an agent or a UI must be able to distinguish an inferred edge (Epic 6) from an asserted one, and losing `_graph` in the projection makes inference indistinguishable from fact. Mutator watch: dropping `_graph` must fail it; accepting a user-supplied `_t` must fail the reserved-key test.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Materialized LPG cache** → decision 1. Revisit only if Epic 37a shows projection cost dominating Bolt query latency.
- **Writes through the LPG surface** → Epic 7b decision 3: writes go through the catalog API so validation, versioning, and authorization apply. `flakes_from_*` exists for import (Epic 9a), not for a live write path.
- **Hypergraph / property-hypergraph models** → the reified-relationship encoding can express n-ary relations already; a distinct hypergraph model needs a use case first.
- **RDF-star as the reification syntax** → Epic 4's `QuotedTriple` extension point; the LPG mapping would gain a shorter path but no capability.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. This crate is pure and total; there is no excuse for a survivor.
2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Zero I/O dependencies** asserted by the CI dependency check.
5. Round-trip fixture covers every entity type in `00c-domain-model.md` (Slice D).
