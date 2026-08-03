# Plan: Data Contracts (Epic 27)

**Branch**: feat/contracts
**Status**: Slices A–D shipped; Slice E (ODCS) deferred with a reason
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

- [x] Contracts have full CRUD with producer, consumers, guarantee, SLAs, mode.
- [x] A schema change evaluates every **enforced** contract on the asset — `Draft` and `Terminated` are not facts about the world; `Violated` still is, because breaches accumulate.
- [x] The compatibility matrix is implemented and table-tested, all 24 cells, in `graph-owl-core` with no database.
- [x] A breach marks the contract `Violated`, records which column and why, and announces to the parties.
- [~] SLAs are evaluated against Epic 30's signals. **Epic 30 is not built, so every SLA reports `Unknown`** — which is the correct answer rather than a stub, and the plan's own RED test.
- [ ] Contract state is in Epic 14's `TrustSummary` — Epic 14 is 3/8; deferred with it.
- [ ] ODCS import and export round-trip — Slice E, deferred; see below.
- [ ] `implements` edges link an asset to the contracts it fulfils — the relation is `contracts.asset_fqn`; projecting it into the graph is a separate concern.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Contracts exist — **shipped**

**Acceptance criteria**: CRUD; producer and consumers validated to exist as teams; asset validated; a contract on a soft-deleted asset → `400`; several contracts per asset permitted (different consumers, different modes); `implements` edge created; terminating a contract does not delete it.
**RED**: A multi-contract test asserting two contracts with different modes coexist on one asset — the realistic case, and the one a single-contract model breaks on. Mutator watch: enforcing one contract per asset must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Compatibility checking (pure) — **shipped**

**Acceptance criteria**: the full matrix above implemented as a pure function; a table-driven test covering all 24 cells; `allow_additional: false` makes any addition a breach regardless of mode; a change touching a column not in `required_columns` is not a breach under `Backward`; the checker reports *which* guarantee was breached and how.
**RED**: The 24-cell matrix is the specification. The `allow_additional` interaction is the subtle one — it overrides mode. Mutator watch: a checker returning "compatible" unconditionally must fail 12 cells; swapping Backward and Forward must fail 8 — and that swap is the classic error, since the terms are counterintuitive.
**REFACTOR**: keep the checker pure and in `graph-owl-core`. It is the highest-stakes logic in the epic and must be testable without a database.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Breach detection and reporting — **shipped**

**Acceptance criteria**: a schema-affecting version bump triggers evaluation of active contracts; a breach sets `Violated`, records the breaching version and the specific guarantee, and emits an event naming producer and consumers; the asset change is **not** blocked; a subsequent compatible change does not clear the violation — clearing is explicit; several breaches accumulate rather than overwriting.
**RED**: A test asserting the asset change succeeds despite the breach — decision 3's guarantee. A test asserting a later compatible change leaves the violation standing, since silent clearing would hide the incident. Mutator watch: blocking the change must fail the first; auto-clearing must fail the second.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: SLA evaluation — **shipped, and every answer is `Unknown`**

**That is the delivery, not a gap.** Decision 5 says SLAs are evaluated against
Epic 30's freshness, completeness and quality signals rather than measured
independently — and Epic 30 is not built, so nothing has been measured. The
three-valued result exists precisely for this: reporting `Met` for an SLA nobody
measured manufactures confidence out of missing data, which is the same failure
Epic 26's certification status and Epic 30's own health are shaped to avoid.
When Epic 30 lands, the `Unknown` arm becomes a lookup and nothing else moves.


**Acceptance criteria**: SLAs evaluated against Epic 30's freshness, completeness, and quality-pass signals; an SLA with no corresponding signal reports `Unknown`, never `Met`; evaluation is on read, not stored; a breached SLA marks the contract `Violated` with which SLA and the observed value; the evaluation window is honoured.
**RED**: The no-signal test asserting `Unknown` rather than `Met` — the same principle as Epic 30's health, and the same danger: reporting a satisfied SLA nobody measured manufactures confidence. Mutator watch: defaulting to `Met` must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: ODCS interop — **deferred, and the reason is a standing rule**

`plans/00l-build-vs-adopt.md` has to be read before implementing any
standard-shaped component, and `00k-standards-conformance.md` is what this
project may claim conformance to. ODCS is an external specification with its own
version history; mapping it at the boundary (decision 4, Epic 9) is a genuinely
separate piece of work from the internal model, and doing it inside this epic
would have meant either skipping that reading or claiming a conformance nobody
verified. Neither is a trade worth making for an interchange format nothing yet
consumes.


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
