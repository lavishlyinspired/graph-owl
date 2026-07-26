# Plan: Domains & Data Products (Epic 23)
**Branch**: feat/domains
**Status**: Not started
**Depends on**: Epic 11 (domains and products are owned)
**Crates**: `graph-owl-core` (Domain, DataProduct) · `graph-owl-storage` · `graph-owl-storage-postgres` (recursive assignment cascade) · `graph-owl-api` · `graph-owl-server`

## Goal

Give the catalog a grouping axis that matches how organizations are structured, orthogonal to how systems are structured.

## Why it is separate from the hierarchy

The technical hierarchy (service → database → schema → table) reflects where data *lives*. Domains reflect who is *accountable* for it, and data products reflect what is *consumable*. A single domain spans several services; a data product bundles assets from several schemas. Forcing either into the containment hierarchy would mean one of the three is wrong.

## Resolved decisions

1. **An asset belongs to at most one domain** — domains are an accountability boundary, and shared accountability is no accountability. Assets belong to any number of data products.
2. **Domain assignment inherits down the hierarchy.** Assigning a database to a domain assigns its schemas and tables unless individually overridden. Otherwise adoption requires tagging thousands of assets.
3. **Domains nest**; sub-domains inherit their parent's assignment unless overridden.
4. **A data product is an entity, not a tag.** It has an owner, a description, a stated purpose, and a lifecycle — it needs an envelope.
5. **No contract enforcement between products** here. Declaring a product does not create guarantees; Epic 30's quality signals are the honest version of that.

## Acceptance criteria (feature level)

- [ ] `Domain` and `DataProduct` have full CRUD with the envelope.
- [ ] Domains nest; cycles rejected.
- [ ] An asset resolves to exactly one domain, directly or by inheritance, flagged which.
- [ ] An asset can belong to several data products.
- [ ] Both are filterable and searchable across every entity type.
- [ ] Moving a database between domains cascades to its descendants.
- [ ] Deleting a domain with assigned assets is rejected or reassigns.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Domains exist and nest

**Value**: The accountability axis exists.
**Path**: `Domain { name, description, domain_type, experts }` + envelope; nesting via `parentOf`.
**Acceptance criteria**: CRUD; unique name; nesting with derived FQNs; cycles → `422` at any depth (reusing Epic 2's cycle detector); `experts` references users; deleting a domain with children → `409` unless recursive.
**RED**: Cycle tests at depth 1 and 3. Mutator watch: immediate-parent-only checking must fail depth 3.
**GREEN**: entity, nesting, cycle reuse.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Assets belong to a domain, with inheritance

**Value**: Adoption is possible — assign a database, not five thousand tables.
**Path**: `domain: Option<EntityReference>` on the envelope, backed by an edge; resolved by walking `contains` upward when unset.
**Acceptance criteria**:
- Assigning a table directly reports it `inherited: false`.
- A table with no assignment under an assigned schema reports the schema's domain, `inherited: true`.
- Multi-hop: assignment on the database reaches the table.
- Resolution stops at the first assigned ancestor.
- An explicit assignment overrides an inherited one.
- Assigning a second domain directly → `409` (decision 1).
- Assignment bumps the version Minor.
**RED**: Multi-hop and override tests. A test asserting a direct assignment is *not* supplemented by the ancestor's. Mutator watch: single-hop resolution must fail multi-hop; accumulate-all must fail the stop-at-first test.
**GREEN**: assignment edge, upward resolution, exclusivity.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Reassignment cascades

**Value**: Reorganizations are one operation, not a migration script.
**Path**: changing a container's domain cascades to descendants without an explicit override.
**Acceptance criteria**:
- Moving a database moves its schemas and tables.
- A descendant with an explicit assignment is **not** moved.
- Cascade is transactional.
- Each affected entity's version bumps and emits an event.
- The response reports how many entities moved.
**RED**: The explicit-override survival test is the sharp one — assert a table with its own domain keeps it when its database moves. Mutator watch: blanket cascade must fail it.
**GREEN**: transactional cascade respecting overrides.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Data products bundle assets

**Value**: A consumable unit exists, spanning technical boundaries.
**Path**: `DataProduct { name, description, purpose, owners, domain }` + envelope; assets linked by edges.
**Acceptance criteria**: CRUD; a product references assets across services and schemas; an asset belongs to several products; `GET /data-products/{id}/assets` paginated; adding a nonexistent or soft-deleted asset → `400`; a product belongs to exactly one domain; removing an asset does not delete it.
**RED**: Multi-product membership test. Mutator watch: single-product exclusivity wrongly applied must fail it — the inverse of the domain rule, and easy to copy-paste wrong.
**GREEN**: entity, membership edges, endpoints.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Both axes are filterable and searchable

**Value**: "Show me everything in the payments domain" — the query the epic exists for.
**Path**: `?domain=` and `?dataProduct=` on list endpoints; both as search facets.
**Acceptance criteria**:
- Domain filter matches direct and inherited assignment.
- Data product filter matches membership.
- Both compose with other filters and with pagination.
- Search facets return counts per domain and per product, respecting active filters.
- Filters work uniformly across entity types.
- `paging.total` respects the filter.
**RED**: Test asserting the domain filter returns inherit-only assets. Facet-count test under a second active filter. Mutator watch: direct-only matching must fail the inheritance case.
**GREEN**: filters, search facets.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Deleting a domain does not orphan

**Value**: Accountability stays truthful through reorganizations.
**Path**: pre-delete check with reassignment, mirroring Epic 11's owner deletion.
**Acceptance criteria**: deleting a domain with assets → `409` with counts by type; `?reassignTo=` transfers then deletes transactionally; reassigning to a nonexistent domain → `400`; deleting an empty domain succeeds; child domains must be handled explicitly.
**RED**: Partial-reassignment failure test asserting atomicity. Mutator watch: non-transactional reassignment.
**GREEN**: check and transactional reassignment.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Contract enforcement between data products** → Epic 30's quality signals are the honest version; formal contracts only if genuinely required.
- **Domain-scoped access policies** → Epic 13 can condition on domain once both exist.
- **Cross-domain dependency analysis** → Epic 29's lineage already carries the edges; a domain-level rollup view is a reporting concern.
- **Product versioning and deprecation lifecycle** → the envelope gives versions; a formal lifecycle only if asked.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
