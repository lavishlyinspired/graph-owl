# Plan: Data Contracts (Epic 27)

**Branch**: feat/contracts
**Status**: Not started
**Depends on**: Epic 2 (schemas to guarantee), Epic 3 (version diffs detect breakage)
**Crates**: `graph-owl-core` (Contract, SchemaGuarantee, **pure compatibility checker**) · `graph-owl-rdf-io` (ODCS mapping) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server`

## Goal

Make producer/consumer expectations explicit and checkable, so "is it safe to change this column?" has an answer that is not a guess.

## Resolved decisions

1. **A contract is an entity with parties**, not an annotation on a table. It has a producer, consumers, an owner, a version, and a lifecycle — all of which need an envelope.
2. **Compatibility is checked against the contract, not inferred from the schema.** A column addition is breaking under a strict contract and compatible under a lenient one. The contract states which.
3. **Breach is reported, not blocked.** graph-owl observes metadata; it cannot prevent a warehouse DDL. Blocking would be a promise it cannot keep. It reports the breach to the parties and marks the contract violated.
4. **ODCS (Open Data Contract Standard) is the interchange format**, mapped at the boundary (Epic 9) — not the internal model.
5. **SLAs are recorded and evaluated against Epic 30's signals**, not measured independently. A freshness SLA is checked against ingested freshness observations.

## Implementation reference

```rust
pub struct Contract {
    pub envelope: EntityEnvelope,
    pub producer: EntityReference,           // team owning the asset
    pub consumers: Vec<EntityReference>,     // teams depending on it
    pub asset: EntityReference,
    pub schema_guarantee: SchemaGuarantee,
    pub slas: Vec<Sla>,
    pub compatibility: CompatibilityMode,
    pub status: ContractStatus,              // Draft|Active|Violated|Terminated
}

pub enum CompatibilityMode {
    None, Backward, Forward, Full,           // Avro-style semantics
}

pub struct SchemaGuarantee {
    pub required_columns: Vec<ColumnGuarantee>,   // name + type + nullability
    pub allow_additional: bool,
}

pub enum Sla {
    Freshness { max_age: Duration },
    Availability { min_uptime_pct: f64 },
    Completeness { min_row_count: u64 },
    QualityPassRate { min_pct: f64, window: Duration },
}
```

### Compatibility checking

On every schema-affecting version bump (Epic 3 supplies the diff), each active contract on that asset is evaluated:

| Change | None | Backward | Forward | Full |
|---|---|---|---|---|
| Add nullable column | ok | ok | ok | ok |
| Add required column | ok | ok | **breach** | **breach** |
| Remove column | ok | **breach** | ok | **breach** |
| Widen type (int→bigint) | ok | ok | **breach** | **breach** |
| Narrow type | ok | **breach** | ok | **breach** |
| Rename column | ok | **breach** | **breach** | **breach** |

Checking is a **pure function** of (diff, guarantee, mode) — a table-driven test over this matrix is the specification.

## Acceptance criteria

- [ ] Contracts have full CRUD with producer, consumers, guarantee, SLAs, mode.
- [ ] A schema change evaluates every active contract on the asset.
- [ ] The compatibility matrix above is implemented and table-tested.
- [ ] A breach marks the contract `Violated` and notifies the parties via events.
- [ ] SLAs are evaluated against Epic 30's signals, not measured separately.
- [ ] Contract state is in Epic 14's `TrustSummary`.
- [ ] ODCS import and export round-trip.
- [ ] `implements` edges link an asset to the contracts it fulfils.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Contracts exist

**Acceptance criteria**: CRUD; producer and consumers validated to exist as teams; asset validated; a contract on a soft-deleted asset → `400`; several contracts per asset permitted (different consumers, different modes); `implements` edge created; terminating a contract does not delete it.
**RED**: A multi-contract test asserting two contracts with different modes coexist on one asset — the realistic case, and the one a single-contract model breaks on. Mutator watch: enforcing one contract per asset must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Compatibility checking (pure)

**Acceptance criteria**: the full matrix above implemented as a pure function; a table-driven test covering all 24 cells; `allow_additional: false` makes any addition a breach regardless of mode; a change touching a column not in `required_columns` is not a breach under `Backward`; the checker reports *which* guarantee was breached and how.
**RED**: The 24-cell matrix is the specification. The `allow_additional` interaction is the subtle one — it overrides mode. Mutator watch: a checker returning "compatible" unconditionally must fail 12 cells; swapping Backward and Forward must fail 8 — and that swap is the classic error, since the terms are counterintuitive.
**REFACTOR**: keep the checker pure and in `graph-owl-core`. It is the highest-stakes logic in the epic and must be testable without a database.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Breach detection and reporting

**Acceptance criteria**: a schema-affecting version bump triggers evaluation of active contracts; a breach sets `Violated`, records the breaching version and the specific guarantee, and emits an event naming producer and consumers; the asset change is **not** blocked; a subsequent compatible change does not clear the violation — clearing is explicit; several breaches accumulate rather than overwriting.
**RED**: A test asserting the asset change succeeds despite the breach — decision 3's guarantee. A test asserting a later compatible change leaves the violation standing, since silent clearing would hide the incident. Mutator watch: blocking the change must fail the first; auto-clearing must fail the second.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: SLA evaluation

**Acceptance criteria**: SLAs evaluated against Epic 30's freshness, completeness, and quality-pass signals; an SLA with no corresponding signal reports `Unknown`, never `Met`; evaluation is on read, not stored; a breached SLA marks the contract `Violated` with which SLA and the observed value; the evaluation window is honoured.
**RED**: The no-signal test asserting `Unknown` rather than `Met` — the same principle as Epic 30's health, and the same danger: reporting a satisfied SLA nobody measured manufactures confidence. Mutator watch: defaulting to `Met` must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: ODCS interop

**Acceptance criteria**: export a contract as ODCS; import ODCS creating or updating a contract; round-trip preserves guarantees, SLAs, and mode; unmappable ODCS fields are reported, not silently dropped; import validates parties exist and creates stubs flagged Draft if not.
**RED**: Round-trip test. An unmappable-field test asserting a report rather than a silent drop. Mutator watch: silent dropping must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Blocking schema changes** → impossible by design (decision 3); graph-owl does not control the warehouse.
- **Contract negotiation workflow** (propose → accept) → Epic 35's proposals are the mechanism if wanted.
- **Automatic contract generation from usage** → Epic 28's usage data makes it possible; inferring a guarantee from observed behaviour is risky and needs a human gate.
- **Per-column SLAs** → asset-level for now.
- **Contract templates** → a small addition once several contracts exist to generalize from.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. The compatibility checker is the highest-stakes pure logic.
2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. All 24 matrix cells covered by explicit tests (Slice B).
5. Verify a breach never blocks the underlying change (Slice C).
