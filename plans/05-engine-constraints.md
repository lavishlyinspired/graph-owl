# Plan: Constraint Validation (Epic 5)

**Branch**: feat/engine-constraints
**Status**: **Shipped** — corrected 8 August 2026, this line had never been
updated since the epic began. `plans/DEMOS.md`'s own Epic 5 section (the
authority per its rule 0) shows 9 of 10 items `[x]` and 1 `[~]`: shapes, all
six target kinds, seed shapes, and `sh:not`/`sh:and`/`sh:or` are built.
Pending: `sh:in` as an RDF list rather than a repeated predicate (a stated
departure, recorded in `00k`, not an omission), and
`RelationshipShape`/`EnvelopeShape` from the seed table, which need Epic 2's
relationship projection to have anything to target.
**Depends on**: Epic 4 (triples to validate)
**Crates**: `graph-owl-ontology` (types), `graph-owl-constraint` (pure logic)

## Goal

Shapes over the graph, so the system can say *where* it is inconsistent instead of quietly being wrong. An agent reasoning over a contradictory graph produces confident nonsense; knowing the graph is inconsistent *here* is what lets it hedge.

## Resolved decisions

1. **Report, do not reject.** A graph assembled from six asynchronous sources is *transiently* inconsistent by nature — a table arrives before its schema, a lineage edge before its target. Write-time rejection would make the graph unfillable. Validation runs continuously and produces a report.
2. **Shapes compile once, validate many.** Compiling a shape from its triples is expensive; the compiled form is cached and invalidated on shape change. Verified as a real cost in reference implementations.
3. **Repair suggestions, never automatic repair.** Auto-repair on a graph fed by six sources will fight the sources. Suggest; let a human or a declaration (Epic 20) decide.
4. **Pure crate, no I/O.** `validate(shapes, facts) -> Report` takes data and returns a verdict. The caller fetches. This makes the highest-stakes logic in the engine exhaustively mutation-testable without a database.
5. **Severity is part of the shape, not the engine.** The same constraint is a violation in one organization and a warning in another. `Violation | Warning | Info`.

## Implementation reference

### Types → `graph-owl-ontology`

```rust
pub struct Shape {
    pub id: Sid,
    pub target: Target,                 // what this shape applies to
    pub constraints: Vec<Constraint>,
    pub severity: Severity,
    pub message: Option<String>,
}

pub enum Target {
    Class(Sid),                         // all entities of this dsc:type
    Subjects(Vec<Sid>),                 // explicit list — covers SHACL's sh:targetNode
    SubjectsOf(Sid),                    // subjects having this predicate
    ObjectsOf(Sid),                     // objects of this predicate
    LiteralsOf(DataType),               // every literal of this datatype, wherever it appears
    ImplicitClass(Sid),                 // the shape is itself a class; its instances are the target
}

pub enum Constraint {
    MinCount { path: Sid, n: usize },
    MaxCount { path: Sid, n: usize },
    Datatype { path: Sid, dt: DataType },
    NodeKind { path: Sid, kind: NodeKind },     // Iri | Literal | Ref
    In       { path: Sid, allowed: Vec<FlakeValue> },
    Pattern  { path: Sid, regex: String },
    MinInclusive { path: Sid, v: f64 },
    MaxInclusive { path: Sid, v: f64 },
    MinLength { path: Sid, n: usize },
    MaxLength { path: Sid, n: usize },
    Class    { path: Sid, expected: Sid },      // object must be of this type
    HasValue { path: Sid, v: FlakeValue },
    Not      (Box<Constraint>),
    And      (Vec<Constraint>),
    Or       (Vec<Constraint>),
}

pub enum Severity { Violation, Warning, Info }
```

### Pure validator → `graph-owl-constraint`

```rust
pub struct CompiledShape { /* pre-indexed constraints by path */ }

pub fn compile(shape: &Shape) -> Result<CompiledShape, CompileError>;

/// Pure. `facts` is everything about the focus nodes; the caller fetched it.
pub fn validate(
    shapes: &[CompiledShape],
    facts:  &FactSet,
) -> ValidationReport;

pub struct ValidationReport {
    pub conforms: bool,
    pub violations: Vec<Violation>,
}

pub struct Violation {
    pub focus_node: Sid,
    pub path: Option<Sid>,
    pub constraint: String,     // machine-matchable, e.g. "minCount"
    pub severity: Severity,
    pub message: String,
    pub actual: Option<FlakeValue>,
    pub suggestion: Option<Repair>,
}

pub enum Repair {
    AssertMissing { path: Sid, hint: String },
    RetractExcess { path: Sid, keep: usize },
    RetypeValue   { path: Sid, to: DataType },
}
```

`conforms` is `true` when no `Violation`-severity entries exist; warnings and info do not fail conformance.

### Seed shapes (migration)

The core entity model gets shapes so validation has something to say on day one:

| Shape | Constraint |
|---|---|
| `TableShape` | `dsc:name` minCount 1, `dsc:fqn` minCount 1 + maxCount 1, `dsc:parentSchema` maxCount 1 nodeKind Ref |
| `ColumnShape` | `dsc:parentTable` minCount 1, `dsc:ordinalPosition` datatype Int minInclusive 0 |
| `RelationshipShape` | `dsc:fromEntity` + `dsc:toEntity` minCount 1 nodeKind Ref, `dsc:relType` In(taxonomy) |
| `ConfidenceShape` | `dsc:confidence` datatype Float, minInclusive 0.0, maxInclusive 1.0 |
| `EnvelopeShape` | `dsc:version` pattern `^\d+\.\d+$`, `dsc:deleted` datatype Boolean |

## Acceptance criteria (feature level)

- [ ] Shapes are defined as triples in the graph and compiled to an executable form.
- [ ] Validation produces a report; it never blocks a write.
- [ ] Every constraint kind above is implemented and independently tested.
- [ ] Severity is honoured — a warning does not fail conformance.
- [ ] Violations carry the actual offending value and a repair suggestion.
- [ ] Compiled shapes are cached; a shape change invalidates the cache.
- [ ] `GET /validation/report` returns current violations, filterable by severity and entity type.

## Slices

Every slice runs the full RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR cycle with implementation skills loaded first.

### Slice A: Constraints evaluate correctly (pure)

**Value**: The validator's core, exhaustively tested without a database.
**Path**: `Constraint` enum + `validate()` over an in-memory `FactSet`.
**Acceptance criteria**: a table-driven test per constraint kind, each with a conforming case, a violating case, and a boundary case:
- `MinCount 1` — 0 values violates, 1 conforms.
- `MaxCount 1` — 2 violates, 1 conforms.
- `MinInclusive 0.0` — `-0.1` violates, `0.0` **conforms** (inclusive).
- `MaxLength 10` — 11 chars violates, 10 conforms.
- `Pattern` — a non-matching value violates; the regex is anchored.
- `In` — a value outside the set violates.
- `Not`, `And`, `Or` — de Morgan cases; `Or` with all branches failing violates once, not per branch.
- Absent path with `MinCount 0` conforms; with `MinCount 1` violates.
**RED**: The boundary cases are the specification. `MinInclusive 0.0` accepting `0.0` and rejecting `-0.1` catches the off-by-one that a "greater than" implementation introduces. Mutator watch: `>` for `>=` must fail the inclusive boundary; a validator returning `conforms: true` unconditionally must fail every violating case; `Or` reporting per-branch must fail the single-violation assertion.
**GREEN**: constraint evaluation.
**REFACTOR**: assess whether `validate` should short-circuit on first violation. **No** — a report listing every problem is the point, mirroring Epic 1's multi-error validation. Record the decision.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Shapes compile from graph triples

**Value**: Shapes are data, versioned and auditable like everything else.
**Path**: shapes stored as triples under `shacl:`; `compile()` reads them into `CompiledShape`.
**Acceptance criteria**:
- A shape defined as triples compiles to an equivalent `CompiledShape`.
- A malformed shape (missing target, unknown constraint) → `CompileError` naming the shape and the problem.
- Compilation is deterministic — same triples, identical compiled form.
- Seed shapes from the migration compile without error.
- A shape targeting a nonexistent class compiles but matches nothing (not an error — the class may arrive later).
**RED**: Round-trip test: shape triples → compile → validate a known-violating fact set → expected violation. Malformed-shape tests per failure mode. Mutator watch: silently skipping an unparseable constraint must fail — assert the `CompileError`, since a silently-dropped constraint is a validation hole.
**GREEN**: shape vocabulary, compiler, seed migration.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Validation runs against the real graph

**Value**: The engine validates actual catalog data.
**Path**: facade method fetching facts for target nodes via `TripleStore`, then calling the pure validator.
**Acceptance criteria**:
- A table missing `dsc:fqn` produces a `TableShape` violation.
- A confidence of `1.5` produces a `ConfidenceShape` violation naming the actual value.
- A conforming graph produces `conforms: true` with an empty violation list.
- Validation of one entity fetches only that entity's facts — not the whole graph.
- Validation never writes.
**RED**: A test asserting the fetch is scoped (query counter or pattern assertion). A test asserting zero writes during validation. Mutator watch: an unscoped fetch must fail the scoping assertion — it is the difference between validating one entity and scanning the graph.
**GREEN**: facade orchestration, scoped fetch.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Shapes are cached

**Value**: The performance decision that makes continuous validation affordable.
**Path**: compiled-shape cache keyed by shape `Sid` + its latest `t`.
**Acceptance criteria**:
- Validating twice compiles once.
- Changing a shape invalidates its cache entry; the next validation recompiles.
- Cache is bounded; eviction is deterministic.
- Concurrent validation is safe.
- A cache metric is exposed (hits, misses, size).
**RED**: A test asserting one compile across two validations (compile counter). An invalidation test asserting recompile after a shape change. Mutator watch: an always-miss cache must fail the compile-count assertion; a never-invalidating cache must fail the change test — and that second one is a correctness bug, not just a staleness one.
**GREEN**: cache with `t`-based invalidation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Violations are queryable and actionable

**Value**: A steward can find and fix what is broken.
**Path**: `GET /validation/report` with filters; violations carry repair suggestions.
**Acceptance criteria**:
- Report is paginated per `00d-api-conventions.md`.
- Filterable by severity, entity type, shape, and entity id.
- Each violation names the focus node, path, constraint, actual value, and a suggestion.
- A `MinCount` violation suggests `AssertMissing`; a `MaxCount` violation suggests `RetractExcess`.
- Report generation does not block on full-graph validation — it reads stored results.
- Validation results are stored with the `t` they were computed at, so staleness is visible.
**RED**: Suggestion-per-constraint-kind test. A staleness test asserting the report names the `t` it reflects. Mutator watch: a report without `t` must fail the staleness assertion — a validation report with unknown currency is unactionable.
**GREEN**: results table, endpoint, filters, suggestion generation.
**REFACTOR**: assess whether validation results belong in the flake store (as facts about facts) or a separate table. Separate table — they are derived and re-computable, and putting them in the graph would make them subject to their own validation.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **SHACL-SPARQL constraints** (arbitrary query as a constraint) → needs Epic 7. Revisit once SPARQL exists and a real constraint cannot be expressed declaratively.

  Published as **SHACL 1.2 SPARQL Extensions**, Working Draft, 24 July 2026. It carries three things, and this plan previously named only the first:
  - `sh:SPARQLConstraint` — arbitrary SPARQL as a constraint. The escape hatch that stops every unusual rule becoming a feature request against this epic.
  - SPARQL-based constraint **components** — parameterised and reusable, so the escape hatch does not become a pile of one-off queries.
  - SPARQL-based **inference rules** — shapes-driven derivation. This one is out of scope for a *validation* epic and overlaps Epic 6; `00k-standards-conformance.md` decision 4 records how the two derivation engines coexist, and the short version is that a derived fact must say which engine produced it.

  Building against a Working Draft is accepting churn, and that is the reason to wait rather than the spec being unavailable.

- **Cardinality enforcement on write** → Epic 4 slice H records cardinality per predicate but does not enforce it. Enforcement is a constraint, not a registry feature, and belongs here: `owl:minCardinality`/`maxCardinality` are things a user wants *reported as violations*, not silently materialised (`06-engine-reasoning.md`).
- **Disjointness as a violation** → `owl:disjointWith` detects a contradiction rather than deriving a fact, so it is this epic's rather than Epic 6's. An asset in two mutually exclusive classes should be told to someone, not quietly make the graph inconsistent.
- **Automatic repair** → deliberately not planned (decision 3). Epic 20's declarative apply is the safe way to bulk-fix.
- **Shape inference from data** ("what shape does this data actually have") → a research direction; revisit if authoring shapes proves to be the adoption barrier.
- **Cross-entity constraints** (e.g. "every table in a certified schema must be certified") → expressible once Epic 6's reasoning exists; a rule, not a shape.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed. The pure validator is the highest-stakes logic here; a surviving mutant is a validation hole.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. `graph-owl-constraint` has **zero I/O dependencies** — asserted by the dependency check from Epic 37c.
5. Validation latency < 10ms per shape against the target in `00a-product-position.md`.
