# Plan: Domains & Data Products (Epic 23)
**Branch**: feat/domains
**Status**: Slices A–F shipped
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

- [x] `Domain` and `DataProduct` have full CRUD with the envelope.
- [x] Domains nest; cycles rejected at depth 1 and depth 3, and a rename or reparent moves the whole subtree's paths transactionally.
- [x] An asset resolves to exactly one domain, directly or by inheritance, flagged which.
- [x] An asset can belong to several data products.
- [x] Both are filterable on list and search — `?domain=` matches direct *and* inherited assignment.
- [x] Moving a database between domains reaches its descendants; one with its own assignment keeps it.
- [x] Deleting a domain with assigned assets is rejected with counts, or reassigns transactionally with `?reassignTo=`.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Domains exist and nest — **shipped**

**Value**: The accountability axis exists.
**Path**: `Domain { name, description, domain_type, experts }` + envelope; nesting via `parentOf`.
**Acceptance criteria**: CRUD; unique name; nesting with derived FQNs; cycles → `422` at any depth (reusing Epic 2's cycle detector); `experts` references users; deleting a domain with children → `409` unless recursive.
**RED**: Cycle tests at depth 1 and 3. Mutator watch: immediate-parent-only checking must fail depth 3.
**GREEN**: entity, nesting, cycle reuse.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Assets belong to a domain, with inheritance — **shipped**

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

### Slice C: Reassignment cascades — **shipped, and the design differs from this plan**

**Value**: Reorganizations are one operation, not a migration script.

**The plan's criteria assume a materialized cascade; the implementation derives
instead, and Slice B is why.** Slice B's own path says the assignment is
"resolved by walking `contains` upward when unset". Under that resolution a
container's descendants have no stored domain to update — so moving a database
*is* one row, the descendants move with it instantly, and "a descendant with an
explicit assignment is not moved" is true by construction rather than by a rule
the cascade has to remember.

Two of this slice's criteria therefore do not survive, and pretending otherwise
would be worse than saying so:

- **"Each affected entity's version bumps and emits an event"** — not done, and
  deliberately. Nothing on the descendant changed; its *resolved* domain changed,
  which is a consequence of an edit to its ancestor. Emitting five thousand
  version bumps for one edit would bury the ancestor's own history and is exactly
  the cost decision 2's "adoption is possible" argument refuses. The container's
  own version does bump, and that is the edit somebody made.
- **"Cascade is transactional"** — trivially true, because it is one `UPDATE`.

Kept, because it is genuinely useful: `GET /domains/{id}/assets/count` reports
how many assets resolve to a domain including inherited ones, so an operator can
see the size of what a move affects.

**The trade being made**: reads pay a recursive walk per asset instead of a
column lookup. Both list and search already pay the same walk for effective
ownership (`OWNERS_EXPR`), and containment is at most five levels deep. If a
measurement ever shows it dominating, a materialized `resolved_domain_id` with
an invalidation trigger is the escape hatch — and it owes an invalidation
problem that the derived version does not have, which is why it is not the
starting point.

**Mutants watched**: a resolver that accumulates every assigned ancestor must
fail `resolution_stops_at_the_nearest_assigned_ancestor`; a blanket cascade must
fail `a_descendant_with_its_own_assignment_is_not_moved`.

### Slice D: Data products bundle assets — **shipped**

**Value**: A consumable unit exists, spanning technical boundaries.
**Path**: `DataProduct { name, description, purpose, owners, domain }` + envelope; assets linked by edges.
**Acceptance criteria**: CRUD; a product references assets across services and schemas; an asset belongs to several products; `GET /data-products/{id}/assets` paginated; adding a nonexistent or soft-deleted asset → `400`; a product belongs to exactly one domain; removing an asset does not delete it.
**RED**: Multi-product membership test. Mutator watch: single-product exclusivity wrongly applied must fail it — the inverse of the domain rule, and easy to copy-paste wrong.
**GREEN**: entity, membership edges, endpoints.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Both axes are filterable and searchable — **shipped, minus facets**

Filters landed on both `GET /assets` and `GET /assets/search`, matching direct
and inherited assignment, composing with `kind` and with pagination. **The facet
counts did not**, and that is a real gap rather than a rounding: see "Explicitly
deferred" for why it needs its own slice rather than a line of code.

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

### Slice F: Deleting a domain does not orphan — **shipped**

**Value**: Accountability stays truthful through reorganizations.
**Path**: pre-delete check with reassignment, mirroring Epic 11's owner deletion.
**Acceptance criteria**: deleting a domain with assets → `409` with counts by type; `?reassignTo=` transfers then deletes transactionally; reassigning to a nonexistent domain → `400`; deleting an empty domain succeeds; child domains must be handled explicitly.
**RED**: Partial-reassignment failure test asserting atomicity. Mutator watch: non-transactional reassignment.
**GREEN**: check and transactional reassignment.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Search facets per domain and per product** → the filters landed; the facet
  counts did not. Epic 22 established the facet mechanism on `/assets/search`
  and adding two more buckets is additive, but a facet over the *visible page*
  (which is what the existing ones compute) is not the same as a facet over the
  whole filtered set, and getting that distinction wrong quietly under-reports.
  Worth its own slice with its own test rather than a line here.
- **A materialized `resolved_domain_id`** → only if a measurement shows the
  recursive walk dominating. See Slice C.
- **Contract enforcement between data products** → Epic 30's quality signals are the honest version; formal contracts only if genuinely required.
- **Domain-scoped access policies** → Epic 13 can condition on domain once both exist.
- **Cross-domain dependency analysis** → Epic 29's lineage already carries the edges; a domain-level rollup view is a reporting concern.
- **Product versioning and deprecation lifecycle** → the envelope gives versions; a formal lifecycle only if asked.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
