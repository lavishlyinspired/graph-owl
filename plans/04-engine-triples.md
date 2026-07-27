# Plan: Triple Storage & Time-Travel (Epic 4) ★

**Branch**: feat/engine-triples
**Status**: Slices A and B complete (see *Implementation findings* below). C–H not started
**Depends on**: Epic 3 (four entity types with an envelope to project)
**Unblocks**: Epics 5, 6, 7, 8, 9, and the time-travel differentiator
**Crates**: `graph-owl-engine` (port), `graph-owl-engine-postgres` (adapter)

## Goal

Make the graph real. Every entity is projected into triples with a transaction time and an assert/retract flag, so the graph is queryable, validatable, reasonable-over — and its history is recoverable by construction rather than by a parallel table that can drift.

## Why here

The triple model changes the storage layer. Applied to four entity types it is a phase; applied to twenty-five it is a rewrite. Same retrofit-cost argument that put the entity envelope in Phase 0.

Everything in Phase 1 depends on this: constraints validate triples, reasoning derives triples, SPARQL queries triples, RDF export serializes triples.

## Resolved decisions

1. **Relational is the source of truth; flakes are the graph view.** Entity CRUD from a triple store means reassembling a row from N flakes on every read, and that read is the catalog's commonest operation. Cost: two write paths, a reconciliation job, and a drift-bug class. Contained by one invariant — **relational wins**; reconciliation only ever re-projects *from* relational, never writes *to* it.
2. **Four index orderings, non-negotiable.** SPOT, PSOT, POST, OPST. The 4× storage cost buys the difference between an index seek and a full scan on every common query shape. Verified against a production reference implementation where this is the single most consequential storage decision.
3. **`op = false` is a retraction, not a delete.** Deleting the row breaks time-travel. This is why history is native rather than a feature.
4. **Relationships are reified nodes.** Edges carry confidence, provenance, and lineage detail; a bare predicate assertion cannot. Costs two hops to traverse; enables "every relationship below 0.5 confidence", which the flat form cannot express at all.
5. **Named graphs are internal, not a user-facing feature** in this epic. They scope provenance and reasoning (`graph:extraction`, `graph:reasoning`, `graph:import:{source}`), and let a failed import be deleted wholesale. Exposed via API only when something needs it.
6. **Flake write failure does not roll back the relational write.** The entity exists; the graph view catches up. The alternative — failing entity creation because a projection failed — makes the graph a single point of failure for the catalog.
7. **Authorization is never evaluated over the flake projection.** This follows from decisions 1 and 6 and was previously left implicit, which is dangerous: flakes lag relational by design, so a policy input read from a flake — a tag, an owner, a domain — can be **stale by exactly the window in which a permission was revoked**. Epic 13's predicate therefore resolves its inputs against relational even when the query itself runs over flakes; Epic 7 lowers the predicate to a subject-scope computed from the source of truth, then applies it to the flake scan. The graph view may be stale about *facts*; it is never the authority on *access*.
8. **Every query result over flakes carries the projection's freshness.** A SPARQL, Cypher, or Bolt result is stamped with the transaction time it was computed at and the current projection lag. An eventually-consistent answer presented as current is the failure mode of this whole design, and the stamp is what makes it honest rather than hidden.
9. **`Sid` is `(namespace_code: u16, id: String)`**, not an interned integer. Interning is a real optimization and a real complication (a dictionary that must stay consistent with the flake table). Postgres composite indexes on `(smallint, text)` are adequate at target scale; revisit if measurement says otherwise.

## Implementation reference

### Core types → `graph-owl-core`

```rust
pub struct Flake {
    pub s:  Sid,            // subject
    pub p:  Sid,            // predicate
    pub o:  FlakeValue,     // object
    pub cx: Option<Sid>,    // named graph; None = default
    pub t:  i64,            // transaction time (logical clock)
    pub op: bool,           // true = assert, false = retract
}

pub struct Sid { pub namespace_code: u16, pub id: String }

pub enum FlakeValue {          // discriminants pinned once shipped — see the scope decision below
    Ref(Sid), String(String), Boolean(bool),
    Int(i64), Float(f64), Instant(DateTime<Utc>), Json(String),
    Bytes(Vec<u8>), Uuid(Uuid), Duration(Duration),
}

pub struct TriplePattern {
    pub s: Option<Sid>, pub p: Option<Sid>, pub o: Option<FlakeValue>,
    pub cx: Option<Option<Sid>>,   // outer None = any graph
    pub as_of: Option<i64>,        // None = current state
}
```

### The two fields this `Flake` does not have

A production reference flake carries eight fields; this one carries six. The two omissions are deliberate, and writing down *why* matters more than the omission itself, because both are load-bearing on the hot path.

| Reference field | Purpose | Decision here |
|---|---|---|
| `dt: Sid` | Datatype pointer per value | **Replaced by a `value_type` discriminant column.** The datatype set is closed and small; a `smallint` discriminant indexes better than a composite `Sid` and removes a join from every scan. An open datatype system is what `dt` buys, and this project does not have one |
| `m: Meta` | Language tag, list index | **Moved off the flake into a sparse `flake_meta` side table**, keyed by the flake's identity |

**The language-tag hole, stated plainly.** With neither field, `"hello"@en` has nowhere to put `en` today. That is a real gap and this is where it closes: a sparse side table, populated only for the small fraction of values that need it, joined only when a query asks for language or list position. Widening the flake row — the single hottest, most-replicated structure in the system — to serve a minority of values is the wrong trade, and denying the need is worse than paying for it narrowly.

*Consequence to accept*: a language-tagged literal costs one extra join. Multilingual labels are an Epic 33 concern; if they become common, revisit and measure before widening the row.

### FlakeValue scope decision

The enum's **discriminants are pinned once shipped** (Slice A's pinning test). Adding a variant later is cheap; renumbering is a data migration over every flake ever written. So the v1 set must be decided deliberately, not discovered.

| Variant | v1 | Why |
|---|---|---|
| `Ref(Sid)` | ✔ | Every edge in the graph |
| `String`, `Boolean`, `Int`, `Float`, `Instant`, `Json` | ✔ | The metadata catalog's actual value shapes |
| `Bytes(Vec<u8>)` | ✔ | **Added.** Without it binary data has no home at all, and retrofitting the one variant that cannot be encoded as a string is the worst case |
| `Uuid`, `Duration` | ✔ | **Added.** Both are already in the domain (entity ids, freshness SLAs) and both round-trip badly through `String` |
| `UInt`, `BigInt`, `BigDecimal`, `Double` | ✗ | `Int(i64)` and `Float(f64)` cover every value this catalog stores. Exact decimal matters for money; this system stores metadata *about* money, not money |
| `GeoPoint` | ✗ | No spatial use case (`ROADMAP.md` not-doing) |

**Ten variants in v1.** The migration path for an eleventh: append with the next discriminant, never reuse a retired one, and add it to the pinning test in the same commit.

### Namespace code allocation

`Sid.namespace_code: u16` is meaningless without a registry, and an ad-hoc allocation becomes a compatibility problem the first time two deployments disagree.

The allocation is derived from this project's own vocabulary layout rather than adopted from elsewhere, and its shape follows one rule: **the codes that appear on the most flakes get the smallest numbers**, because `dsc:` predicates dominate this graph by a wide margin and a compact code keeps the composite index narrow.

| Range | Owner | Contents |
|---|---|---|
| `0` | Reserved | Unset. Never a valid stored namespace — makes an uninitialized `Sid` a detectable bug rather than a silent default |
| `1–255` | graph-owl | The `dsc:` predicate vocabulary (Epic 4) and entity types. The hot path, deliberately given the low byte |
| `256–511` | Standards | `rdf`, `rdfs`, `xsd`, `owl`, `sh`, `dcat`, `dprod`, `prov`, `skos`, `odcs` — allocated in `graph-owl-core` as constants, one per vocabulary, in the order Epic 9 adopts them |
| `512–1023` | graph-owl future | Reserved for vocabularies this project introduces later — memory (31), ontology packs (33) |
| `1024–65534` | Runtime | Deployment-defined namespaces, allocated by `PredicateRegistry` (Slice H) and persisted |
| `65535` | Reserved | Sentinel for "namespace not found" in lookup paths, so a miss is never confusable with code 0 |

**Allocation is persisted and monotonic.** A code, once assigned to an IRI prefix, is never reused for a different one — a reused code silently rewrites the meaning of every historical flake that carries it, which is a corruption that time-travel makes permanent rather than transient.

### Index management

Postgres maintains the four orderings inline on write, which removes an entire indexer subsystem from this design. What it does not remove is the maintenance:

- **Which indexes exist and why**: SPOT, PSOT, POST, and a partial OPST (`WHERE value_type = 0`, reference objects only). The partial index is the reason OPST costs a fraction of the other three — object-position lookup is only meaningful for references.
- **Bloat**: an append-only flake table with retractions never updates a row in place, so index bloat comes from deletion of superseded flakes during archival, not from normal operation. `REINDEX CONCURRENTLY` on a schedule, triggered by measured bloat, not by the calendar.
- **Partitioning**: `PARTITION BY LIST (namespace_s)` with the trigger measured in `37a-scale.md`.
- **Relationship to Slice G's reconciliation job**: reconciliation repairs *content* drift between the relational tables and the flake projection. It does not repair index consistency — Postgres owns that. Conflating the two produces a reconciliation job that appears to fix problems it never touched.

### Port → `graph-owl-engine`

```rust
#[async_trait]
pub trait TripleStore: Send + Sync {
    async fn assert(&self, flakes: &[Flake]) -> Result<(), EngineError>;
    async fn retract(&self, flakes: &[Flake]) -> Result<(), EngineError>;
    async fn query_pattern(&self, p: &TriplePattern) -> Result<Vec<Flake>, EngineError>;
    async fn exists(&self, f: &Flake) -> Result<bool, EngineError>;
    async fn count(&self, p: &TriplePattern) -> Result<u64, EngineError>;
    async fn predicates_for(&self, s: &Sid) -> Result<Vec<Sid>, EngineError>;
    async fn subjects_with(&self, p: &Sid) -> Result<Vec<Sid>, EngineError>;
    async fn objects_for(&self, s: &Sid, p: &Sid) -> Result<Vec<FlakeValue>, EngineError>;
    async fn current_time(&self) -> Result<i64, EngineError>;
}

#[async_trait]
pub trait PredicateRegistry: Send + Sync {
    async fn define(&self, def: &PredicateDef) -> Result<(), EngineError>;
    async fn lookup(&self, ns: u16, name: &str) -> Result<Option<PredicateDef>, EngineError>;
    async fn list(&self, ns: Option<u16>) -> Result<Vec<PredicateDef>, EngineError>;
}
```

### Postgres schema → `V3__create_flakes.sql`

```sql
CREATE TABLE flakes (
    id           BIGSERIAL PRIMARY KEY,
    namespace_s  SMALLINT NOT NULL,
    sid_s        TEXT     NOT NULL,
    namespace_p  SMALLINT NOT NULL,
    sid_p        TEXT     NOT NULL,
    value_type   SMALLINT NOT NULL,   -- 0=ref 1=str 2=bool 3=int 4=float 5=instant 6=json
    value_ref_ns SMALLINT,
    value_ref_id TEXT,
    value_str    TEXT,
    value_bool   BOOLEAN,
    value_int    BIGINT,
    value_float  DOUBLE PRECISION,
    value_inst   TIMESTAMPTZ,
    value_json   JSONB,
    cx_namespace SMALLINT,            -- NULL = default graph
    cx_id        TEXT,
    t            BIGINT   NOT NULL,
    op           BOOLEAN  NOT NULL
);

CREATE INDEX idx_flakes_spot ON flakes (namespace_s, sid_s, namespace_p, sid_p, t DESC);
CREATE INDEX idx_flakes_psot ON flakes (namespace_p, sid_p, namespace_s, sid_s, t DESC);
CREATE INDEX idx_flakes_post ON flakes (namespace_p, sid_p, value_type, value_str, namespace_s, sid_s, t DESC);
CREATE INDEX idx_flakes_opst ON flakes (value_type, value_ref_ns, value_ref_id, namespace_p, sid_p, namespace_s, sid_s, t DESC)
    WHERE value_type = 0;   -- OPST is references only
```

`t DESC` on every index so "current state" — the newest flake per `(s,p,o)` — is a leading-edge read rather than a sort. OPST is partial (`WHERE value_type = 0`) because reverse traversal only makes sense for references; indexing literal objects there would triple the index for no query.

### Current-state resolution

A pattern query must return the *latest* flake per `(s, p, o, cx)` and exclude retracted ones:

```sql
SELECT DISTINCT ON (namespace_s, sid_s, namespace_p, sid_p, value_type, value_str, value_ref_id)
       *
FROM flakes
WHERE namespace_s = $1 AND sid_s = $2
  AND ($3::bigint IS NULL OR t <= $3)          -- as_of
ORDER BY namespace_s, sid_s, namespace_p, sid_p,
         value_type, value_str, value_ref_id, t DESC
```

then filter `op = true` in the outer query. **Filtering `op = true` inside the `WHERE` is the bug to avoid** — it would return a superseded assertion after a retraction, because the retraction row is the one being excluded.

### Transaction clock

A single-row `graph_clock` table with `SELECT ... FOR UPDATE` gives a monotonic `t` per transaction. Not per-flake: every flake in one logical change shares a `t`, which is what makes "the state after change N" well-defined.

## Acceptance criteria (feature level)

- [ ] Creating an entity projects it into flakes; deleting retracts them.
- [ ] All four index orderings exist and are used — verified by query plan, not by assumption.
- [ ] A pattern query with any combination of bound/unbound terms returns correct results.
- [ ] `as_of` returns the state at that transaction time, including retracted-since facts.
- [ ] A retraction hides a fact from current state without deleting the assertion row.
- [ ] Relationships project as reified nodes carrying confidence.
- [ ] A flake-write failure leaves the relational entity intact and is reconciled later.
- [ ] Reconciliation is one-directional: it never writes to the relational store.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`, `testing`, `mutation-testing`, `refactoring` loaded first, ending awaiting commit approval.

### Slice A: Flakes round-trip through Postgres

**Value**: The storage substrate exists and is provably correct.
**Path**: `Flake`/`Sid`/`FlakeValue` in core; `TripleStore` in `graph-owl-engine`; Postgres adapter with the schema above.
**Acceptance criteria**:
- Assert then `query_pattern` returns the flake byte-identical.
- Every `FlakeValue` variant round-trips — including `Float` NaN/infinity handling, and `Instant` at microsecond precision (Postgres `TIMESTAMPTZ` is µs; `chrono` is ns).
- Batch assert of 1,000 flakes is one statement, not 1,000.
- `count` matches `query_pattern().len()` for the same pattern.
- Asserting the same flake twice at the same `t` is idempotent.
**RED**: Round-trip test per `FlakeValue` variant, deliberately including a `Float` and an `Instant`. A batch test asserting one statement (observable via a query counter or `EXPLAIN`). Mutator watch: a `value_type` discriminant that collapses two variants must fail the per-variant round-trip; truncating `Instant` must fail the precision assertion.
**GREEN**: types, trait, adapter, migration.
**REFACTOR**: assess where `FlakeValue` ↔ column mapping lives. It is pure and belongs in the adapter, but the *discriminant* numbering is a wire contract — put it in core with a test pinning each number, so a reordering cannot silently change stored data.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: All four index orderings serve their query shape

**Value**: The decision that makes the engine viable, verified rather than assumed.
**Path**: the four indexes; pattern-query planning that picks one.
**Acceptance criteria**, each verified by `EXPLAIN` asserting an index scan and **naming the expected index**:
- `(s, ?, ?)` → SPOT.
- `(?, p, ?)` → PSOT.
- `(?, p, o)` → POST.
- `(?, ?, o)` where `o` is a reference → OPST.
- `(s, p, ?)` → SPOT.
- No pattern produces a sequential scan on a 100k-flake table.
**RED**: A query-plan test per shape asserting the index *by name*. Mutator watch: a dropped index must fail its shape; a planner that always picks SPOT must fail the POST and OPST shapes. This is the slice where "it works" and "it works fast" are different tests, and only the plan assertion catches the difference.
**GREEN**: indexes, plan-directed pattern dispatch.
**Done when**: criteria met, all six plans verified, mutation report reviewed, commit approved.

### Slice C: Retraction hides without deleting

**Value**: The property that makes time-travel native.
**Path**: `retract` asserts `op = false`; current-state resolution excludes superseded facts.
**Acceptance criteria**:
- Assert, retract, then query current state → fact absent.
- The assertion row still exists in the table.
- Assert, retract, assert again → fact present (three rows, one visible).
- Retracting a nonexistent fact is a no-op, not an error.
- `count` reflects visible facts, not row count.
**RED**: The assert-retract-assert sequence, asserting visibility after each step *and* row count in the table. Mutator watch: filtering `op = true` in the `WHERE` rather than the outer query must fail the assert-retract-assert case — the exact bug called out in the implementation notes above.
**GREEN**: retraction semantics, `DISTINCT ON` current-state resolution.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Entities project into flakes

**Value**: The catalog becomes a graph.
**Path**: `entity_to_flakes()` in core for all four Phase-0 entity types, using the `dsc:` vocabulary from `00c-domain-model.md`; called by the facade after each relational write.
**Acceptance criteria**:
- A `Table` projects to flakes for every populated envelope field, one per field.
- `None` fields produce no flake — absence is not a null assertion.
- The hierarchy projects as `dsc:parentSchema` references, matching the `contains` edges.
- Columns project with `dsc:parentTable` and `dsc:ordinalPosition`.
- Update projects a retraction of the old value and an assertion of the new, sharing one `t`.
- Projection is pure — a function from entity to `Vec<Flake>`, no I/O, exhaustively testable.
**RED**: Golden-file test pinning the exact flake set for a fully-populated `Table`. A test asserting a `None` description produces no flake. An update test asserting exactly one retraction plus one assertion, same `t`. Mutator watch: emitting a flake for `None` must fail; an update that asserts without retracting must fail current-state resolution.
**GREEN**: projection function, facade wiring.
**REFACTOR**: projection is the highest-traffic pure function in the engine. Assess a derive macro or a declarative field→predicate table rather than hand-written per-entity code — twenty-five entity types are coming.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Relationships project as reified nodes

**Value**: Edges can carry confidence and provenance, which the flat form cannot express.
**Path**: a relationship projects to a node with `rdf:type dsc:Relationship`, `dsc:fromEntity`, `dsc:toEntity`, `dsc:relType`, plus optional `dsc:confidence`.
**Acceptance criteria**:
- A relationship projects to ≥ 4 flakes sharing the relationship's own `Sid` as subject.
- `dsc:fromEntity` and `dsc:toEntity` are `Ref` values, not strings — so OPST reverse traversal works.
- Querying `(?, dsc:toEntity, Ref(table_b))` finds relationships pointing at `table_b`.
- Confidence is queryable: pattern `(?, dsc:confidence, ?)` with a filter returns low-confidence edges.
- Deleting a relationship retracts all its flakes.
**RED**: A reverse-traversal test via OPST. A confidence-filter test asserting only edges below the threshold return. Mutator watch: storing endpoints as `String` rather than `Ref` must fail the reverse-traversal test, because OPST is reference-only.
**GREEN**: reified projection, `Sid` derivation for relationship ids.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Time-travel query

**Value**: The differentiator, exposed. "What did this look like on 1 January" in one request.
**Path**: `as_of` on `TriplePattern`; `?asOf=<rfc3339>` on entity reads, resolved to a `t` via the clock table.
**Acceptance criteria**:
- Three timed changes: `asOf` at each point returns that state exactly.
- `asOf` at exactly a transaction's `t` returns that transaction's state (inclusive boundary).
- `asOf` before the entity existed → `404`.
- `asOf` after a soft delete → the tombstoned state, not absence.
- `asOf` reflects relationships and hierarchy as they were, not as they are.
- A renamed ancestor shows the **historical FQN**.
- The response names the resolved `t`, so the answer is auditable.
**RED**: The historical-FQN case is the sharpest — rename a database, then assert `asOf` before the rename returns the old FQN. Inclusive-boundary test at exactly `t`. Mutator watch: `<` instead of `<=` must fail the boundary; resolving edges at current time while resolving fields as-of must fail the FQN and relationship cases.
**GREEN**: `as_of` in pattern resolution, timestamp→`t` mapping, API parameter.
**REFACTOR**: `as_of` now threads through entity, edge, and hierarchy reads. Assess a single `AsOf(Option<i64>)` context carried through the facade rather than a parameter on every method.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Projection failure is survivable and reconcilable

**Value**: The graph view cannot take down the catalog, and drift is repairable.
**Path**: flake write in a separate transaction from the relational write; failures logged and queued; a reconciler that re-projects from relational.
**Acceptance criteria**:
- With the triple store stubbed to fail, entity creation still succeeds `201`.
- The failure is logged and recorded as pending reconciliation.
- The reconciler re-projects and converges.
- Reconciliation is **idempotent** — running it twice yields one flake set.
- Reconciliation is **one-directional**: a test asserting it never issues a relational write.
- A drift metric is exposed (entities whose projection is stale).
**RED**: Failure-injection test asserting `201` plus a queued reconciliation. An idempotency test running the reconciler twice. A test asserting zero relational writes during reconciliation — decision 1's invariant, and the one that prevents the two stores fighting. Mutator watch: propagating the flake error must fail the first; a reconciler that writes relational must fail the third.
**GREEN**: split transactions, pending queue, reconciler, drift metric.
**REFACTOR**: assess whether the reconciler belongs in `graph-owl-api` (has the projection function and both ports) or as a standalone task. Facade-owned, invoked by a background task — the projection logic must not be duplicated.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice H: Predicates are definable at runtime

**Value**: An organization can extend the vocabulary without a release.
**Path**: `PredicateRegistry` + `V4__create_predicate_registry.sql`.
**Acceptance criteria**:
- Define a predicate with namespace, name, datatype, cardinality.
- Duplicate `(namespace, name)` → `409`.
- Asserting a flake with an unregistered predicate → error naming it.
- Core `dsc:` predicates are seeded by migration and **cannot be redefined**.
- Cardinality `one` rejects a second value for the same `(s, p)`; `many` allows it.
- Registry lookups are cached; a definition change invalidates the cache.
**RED**: Cardinality test asserting `one` rejects a second assertion and `many` accepts. A test asserting a core predicate cannot be redefined. Mutator watch: an unenforced cardinality must fail; a mutable core predicate must fail.
**GREEN**: registry table, trait, seed migration, cache.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Implementation findings (Slices A–B)

Four things this plan got wrong, found by building it. Recorded here rather than
silently fixed, because each was a *stated* design decision above.

### 1. `SMALLINT` cannot hold a namespace code

The schema above declares `namespace_s SMALLINT`. Postgres `SMALLINT` is signed
`i16`, maximum 32767; `Sid.namespace_code` is `u16`, maximum 65535. Every
runtime-allocated code above 32767 — half the range this plan assigns to the
predicate registry — would overflow.

**Now**: `INTEGER` with `CHECK (x BETWEEN 0 AND 65535)`. The two extra bytes are
noise next to the `TEXT` sid columns that dominate all four indexes.

### 2. A batch assert is not one statement

"Batch assert of 1,000 flakes is one statement" understates the constraint. The
Postgres wire protocol carries its parameter count as an `int16`, so **one
statement binds at most 65535 values** — about 3,200 flakes at twenty columns
each. Past that the driver refuses outright. The acceptance criterion's 1,000
happens to sit under the ceiling, so it would have passed while leaving the
defect in place; the 100k-flake load in Slice B is what exposed it.

**Now**: chunked at `MAX_BIND_PARAMETERS / COLUMNS_PER_FLAKE`, with the whole
batch in one transaction so chunking does not cost atomicity. A large
projection is all-or-nothing, as it must be for Slice G's reconciler to be safe.

### 3. SPOT and the identity index are the same index

Idempotency needs a unique index over the fact identity, which necessarily
leads with subject-then-predicate. A separate SPOT is therefore a strict prefix
of it: redundant for lookups and paid for on every write. The planner settled
it — given both, it never chose SPOT.

**Now**: one `UNIQUE` index named `idx_flakes_spot` doing both jobs. Still four
indexes, still one per ordering, one fewer to maintain.

### 4. `(s, p, ?)` is served by PSOT, not SPOT

Slice B's criteria name SPOT for this shape. Both SPOT and PSOT bind subject and
predicate completely, and PSOT is the narrower row, so the planner prefers it —
correctly. The criterion asserted the planner's cost arithmetic rather than
anything about this schema.

**Now**: that shape asserts an index scan on *either*, plus no sequential scan.
The test still fails if both indexes disappear, which is the regression that
matters.

### Also settled while building

- **`FlakeValue::Duration` holds `i64` seconds**, not `chrono::Duration`.
  Postgres `INTERVAL` carries months, which have no fixed length — "30 days"
  and "1 month" must not compare equal.
- **`value_key`**, a deterministic text encoding of the object, is stored
  alongside the typed columns. It gives the identity index something comparable
  for every value type, and lets POST serve object lookups for *any* literal
  rather than only for strings.
- **The two migration runners** (storage and engine) share a database, so the
  engine's uses its own history table. Refinery's default would make each
  runner treat the other's migrations as unknown and refuse to run.

## Explicitly deferred (with destination)

- **Interned `Sid` dictionary** → an optimization; revisit if Epic 37a's benchmarks show `(smallint, text)` indexes are the bottleneck.
- **Content-addressed storage, binary columnar format, consensus** → not planned. Postgres handles persistence; single-node is the deployment model.
- **Named graphs as a user-facing API** → internal in this epic; exposed when Epic 21 or 32 needs callers to select a graph.
- **Spatial indexing** → not needed for metadata.
- **RDF-star (statement-level annotation)** → reified relationships cover the need at lower cost. Revisit only if standards interop (Epic 9) demands it.
- **Table partitioning** → start unpartitioned. `flakes` is `PARTITION BY LIST (namespace_s)`-ready because the column already exists and leads three of the four indexes. Trigger: **10M flakes**, measured by Epic 37a. Cross-partition SPARQL needs a UNION, so partitioning is not free — it buys partition pruning for namespace-scoped queries, which is the common shape.
- **RDF-star** → reified relationships cover annotation at lower cost, and RDF 1.2 is still Candidate Recommendation. The extension point is a `FlakeValue::QuotedTriple(QuotedTriple)` variant; adding it later is additive because the `value_type` discriminant is already an open enum with a pinned numbering test.
- **Retention / pruning of retracted flakes** → history is unbounded until evidence says otherwise. This epic makes the storage cost visible, which is the input that decision needs.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. **Query plans verified by name for all six pattern shapes** (Slice B) — a missing index degrades silently and is the most expensive possible regression here.
5. Performance smoke against the targets in `00a-product-position.md`: batch assert < 1ms/1000 flakes, pattern query < 5ms p50.
6. Reconciliation one-directionality asserted, not assumed (Slice G).
