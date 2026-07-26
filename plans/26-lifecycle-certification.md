# Plan: Lifecycle & Certification (Epic 26)

**Branch**: feat/lifecycle-certification
**Status**: Not started
**Depends on**: Epic 11 (issuers are principals), Epic 24 (metrics are certifiable)
**Crates**: `graph-owl-core` (LifecycleState, Certification, CertificationType, derived status) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server`

## Goal

Say whether an asset is trustworthy and current — a lifecycle state with a machine-readable successor, and a certification that expires.

## Why this earns an epic

"Certified financial metrics" is a first-class agent query. An agent recommending a deprecated asset without saying so is the most damaging failure this system can produce: it is confidently wrong in a way the user cannot detect.

## Resolved decisions

1. **Certification expires.** An unexpiring trust stamp becomes a lie within a year. Expiry is required, with a default period per certification type.
2. **Deprecation carries a successor reference, not prose.** "Use `orders_v2` instead" must be machine-readable so an agent can redirect rather than merely warn.
3. **Lifecycle and certification are orthogonal.** An asset can be Active-uncertified, Active-certified, or Deprecated-certified (still trustworthy, but going away). Collapsing them loses the distinction that matters most.
4. **Certification is issued by a principal, not a system.** Accountability requires a name. Automated certification is possible but the issuer is the bot principal that granted it.
5. **Expiry does not change lifecycle.** An expired certification means "no longer vouched for", not "deprecated". Conflating them would retire assets nobody re-certified.

## Implementation reference

```rust
pub enum LifecycleState { Draft, Active, Deprecated, Retired }

pub struct Deprecation {
    pub reason: String,
    pub successor: Option<EntityReference>,
    pub deprecated_at: DateTime<Utc>,
    pub sunset_at: Option<DateTime<Utc>>,     // when it becomes Retired
}

pub struct Certification {
    pub certification_type: EntityReference,  // e.g. "Gold", "Finance-Approved"
    pub issuer: EntityReference,              // user or team
    pub criteria: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,            // required
    pub evidence: Vec<EntityReference>,       // tests, reviews, contracts
}

pub struct CertificationType {
    pub envelope: EntityEnvelope,
    pub default_validity: Duration,
    pub required_evidence: Vec<EvidenceKind>, // e.g. QualityTests, OwnerConfirmed
    pub authorized_issuers: Vec<EntityReference>,
}
```

### Derived status

`CertificationStatus` is computed on read, never stored: `Valid` | `ExpiringSoon(days)` | `Expired` | `None`. Storing it would go stale without the entity changing — the same reasoning as Epic 30's health and Epic 31's staleness.

## Acceptance criteria

- [ ] Lifecycle transitions with rules; illegal transitions rejected.
- [ ] Deprecation requires a reason and accepts a machine-readable successor.
- [ ] `sunset_at` transitions Deprecated → Retired on schedule.
- [ ] Certification requires an expiry and an authorized issuer.
- [ ] Certification status is computed on read, including `ExpiringSoon`.
- [ ] Required evidence is enforced per certification type.
- [ ] Recertification workflow with reviewer assignment.
- [ ] Both are filterable, searchable as facets, and in Epic 14's `TrustSummary`.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Lifecycle state machine

**Acceptance criteria**: Draft→Active, Active→Deprecated, Deprecated→Retired, Deprecated→Active (un-deprecate); illegal transitions (Draft→Retired, Retired→anything) → `422`; default state on create is configurable per entity type (connector-ingested assets default Active, hand-created default Draft); each transition bumps the version and emits an event; Retired assets are excluded from search by default but remain readable.
**RED**: A full transition matrix. A test asserting Retired assets are absent from search but present on direct `GET` — the same distinction as soft delete. Mutator watch: an always-permit transition must fail the illegal moves; hiding Retired from direct reads must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Deprecation with a successor

**Acceptance criteria**: deprecation requires a reason; `successor` optional but validated to exist and not itself be Deprecated or Retired; a successor chain (A→B, B→C) is traversable to the live terminal asset; a cycle in successors → `422`; deprecating an asset that others depend on (Epic 29 lineage) reports the dependents; `sunset_at` in the past → `400`.
**RED**: The successor-chain test asserting traversal reaches C from A. A test asserting a deprecated successor is rejected — pointing users at another dead asset is worse than pointing nowhere. Cycle test. Mutator watch: accepting a deprecated successor must fail; a one-hop chain resolution must fail the A→C test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Certification types and issuance

**Acceptance criteria**: `CertificationType` CRUD with default validity, required evidence, authorized issuers; issuing requires the principal to be an authorized issuer, else `403`; `expires_at` defaults from the type's validity if omitted; an expiry in the past → `400`; required evidence must be present and reference real entities, else `422` naming what is missing; issuing over an existing valid certification → `409` unless it is a renewal.
**RED**: An evidence-enforcement test asserting a certification is refused when a required quality test is absent — the criterion that makes certification mean something. An unauthorized-issuer test. Mutator watch: skipping evidence enforcement must fail; ignoring the issuer allowlist must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Status is computed, including expiry

**Acceptance criteria**: status computed on read from `expires_at` and now; `ExpiringSoon` within a configurable window (default 30 days) reporting days remaining; `Expired` past the expiry; boundary tested at exactly the expiry instant; status changes without the entity changing — verified by reading twice across a simulated clock advance; expiry does **not** alter lifecycle.
**RED**: The clock-advance test: read an asset before and after its certification expires with no write in between, asserting the status changes. A stored status would fail this. A test asserting lifecycle is untouched by expiry. Mutator watch: storing status must fail the clock-advance test; expiry cascading to Deprecated must fail the lifecycle assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Recertification

**Acceptance criteria**: `POST /{collection}/{id}/recertify` with reviewer assignment; recertification re-checks required evidence at the time of renewal, not at original issuance; renewal extends `expires_at` and records both issuances in history; an expired certification can be renewed; recertification by an unauthorized issuer → `403`; a recertification queue lists assets expiring within the window.
**RED**: A re-check test asserting renewal fails if the evidence has since disappeared (a quality test was deleted) — renewing on stale grounds is how certification decays into theatre. Mutator watch: skipping the re-check must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Discoverability

**Acceptance criteria**: `?lifecycle=` and `?certification=` filters on list endpoints; both as search facets with counts respecting active filters; `TrustSummary` (Epic 14) carries lifecycle, certification status with expiry, and successor; a deprecated asset in search results is visibly marked, never silently ranked normally; certified assets are boostable in ranking (configurable).
**RED**: A search test asserting a deprecated asset is returned **with its marker**, not filtered out and not unmarked — filtering hides reality, unmarking misleads. Mutator watch: either behaviour must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Automated certification from quality signals** → possible once Epic 30 exists; the issuer would be a bot principal. Needs a policy decision about machine-granted trust first.
- **Certification revocation workflow** (distinct from expiry) → expiry plus re-issuance covers it; add if a compliance process demands explicit revocation.
- **Cross-organization certification recognition** → single-tenant assumption.
- **Lifecycle-driven access control** ("Retired assets are read-only") → Epic 13 can condition on lifecycle once both exist.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. Clock-advance test verified (Slice D) — a stored status is the failure mode here.
5. Evidence enforcement verified (Slice C) — certification without it is decoration.
