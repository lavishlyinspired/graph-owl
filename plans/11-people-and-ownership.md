# Plan: Users, Teams & Ownership (Epic 11)
**Branch**: feat/ownership
**Status**: **In progress** — shipped into Demo 2
**Depends on**: Epic 3 (envelope carries `owners`)
**Unblocks**: Epic 24 (term reviewers), Epic 13 (roles attach to users/teams)
**Crates**: `graph-owl-core` (User, Team, EntityReference, cycle detection) · `graph-owl-storage` · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server`

## Goal

Answer the question asked of a catalog more than any other: *who do I talk to about this table?*

## Resolved decisions

1. **`owners` is a list, not a single field.** Every real asset has both a producing team and an accountable individual. Single-owner models fail on contact with an organization.
2. **Ownership is an `owns` edge**, with `owners` on the entity as a read-model projection — consistent with hierarchy being edges (Epic 11).
3. **Ownership inherits down the hierarchy.** A table with no explicit owner reports its schema's owner, flagged `inherited: true` so the UI distinguishes "nobody set this" from "deliberately owned here". Without the flag, inheritance hides gaps rather than filling them.
4. **`User` is a catalog entity, not an authentication record.** It holds identity-adjacent metadata. Epic 12 binds a JWT subject to a `User`; this epic does not touch credentials.
5. **Teams are hierarchical** via `parentOf`. Inheritance walks up the team tree for permission purposes in Epic 11.
6. **`follows` edges are recorded; nothing is delivered.** Notification transport has no consumer yet and would be speculative.

## Acceptance criteria (feature level)

- [ ] `User` and `Team` have full CRUD with the envelope.
- [ ] A team can contain teams and users; cycles are rejected.
- [ ] Any entity can have multiple owners, mixing users and teams.
- [ ] A table with no explicit owner reports its schema's owner with `inherited: true`.
- [ ] `GET /tables?owner={id}` matches both direct and inherited ownership.
- [ ] A user can follow an entity and list what they follow.
- [ ] Deleting a team with owned assets is rejected or reassigns — not silently orphaning.

## Slices

Every slice runs the full RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR cycle with implementation skills loaded first.

### Slice A: Users exist

**Value**: The catalog can name a person.
**Path**: `User { name, email, display_name?, is_bot }` + envelope; full CRUD.
**Acceptance criteria**: create/get/list/patch/delete; unique `name` and `email` → `409` on duplicate; invalid email → `400`; `isBot` distinguishes service accounts (used by Epic 15 connectors and Epic 5 (`05-engine-constraints.md`)).
**RED**: Repository and HTTP tests per operation, including both uniqueness constraints separately. Mutator watch: a single uniqueness constraint covering both fields must fail — assert duplicate-email-different-name is rejected *and* duplicate-name-different-email is rejected.
**GREEN**: entity, storage, facade, handlers.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Teams exist and nest

**Value**: Ownership can name an organizational unit, which is how ownership actually works.
**Path**: `Team { name, team_type, display_name? }` + envelope; `parentOf` edges; membership as `has` edges to users.
**Acceptance criteria**:
- CRUD; unique name.
- A team may parent teams and contain users.
- `GET /teams/{id}/children` and `/members`, paginated.
- A cycle (`A parentOf B`, then `B parentOf A`) → `422`.
- Self-parenting → `422`.
- Deep cycle (A→B→C→A) → `422`.
**RED**: Cycle tests at depth 1, 2, and 3 — a check that only compares immediate parent passes depth-1 and fails depth-3. Mutator watch: exactly that.
**GREEN**: entity, edges, ancestor-walk cycle detection.
**REFACTOR**: cycle detection will be needed again for glossary terms (Epic 11) and lineage (Epic 11). Assess extracting a generic `would_create_cycle(edge_type, from, to)` into core.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Entities have owners

**Value**: The catalog answers "who owns this".
**Path**: `owners: Vec<EntityReference>` on the envelope, backed by `owns` edges; settable on create and PATCH.
**Acceptance criteria**:
- Set one or many owners; mix users and teams.
- Owner referencing a nonexistent principal → `400` naming the index (`owners[1].id`).
- Owner referencing a soft-deleted principal → `400`.
- Response carries denormalized `EntityReference`s (name, FQN, type), not bare ids.
- Removing all owners is allowed — an unowned asset is a real, reportable state.
- Owner change bumps the version Minor with a `changeDescription` entry.
**RED**: Facade tests for multi-owner, mixed-type, invalid-index reporting, and the version bump. Mutator watch: validating only the first owner must fail the invalid-index-1 case.
**GREEN**: edges, projection, validation.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Ownership inherits

**Value**: A 5,000-table catalog is navigable without tagging every table individually, while still showing where ownership is genuinely absent.
**Path**: on read, if no direct `owns` edge, walk `contains` upward to the nearest owned ancestor; project with `inherited: true`.
**Acceptance criteria**:
- Table with no owner, schema owned → table reports the schema's owner, `inherited: true`.
- Table with its own owner → reports only that, `inherited: false`.
- Neither owned, database owned → inherits from the database (multi-hop).
- Nothing owned anywhere → empty `owners`, not an error.
- Inheritance stops at the first owned ancestor — it does not accumulate up the chain.
**RED**: Multi-hop test (table → schema → database) asserting the database's owner is reported. A test asserting a table with its own owner does *not* also list the schema's. Mutator watch: single-hop-only must fail the multi-hop case; accumulate-all-ancestors must fail the stop-at-first case.
**GREEN**: upward walk, projection flag.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Assets are filterable by owner

**Value**: "Show me everything my team owns" — the primary steward workflow.
**Path**: `?owner={id}` on list endpoints, matching direct and inherited ownership.
**Acceptance criteria**:
- Matches directly-owned entities.
- Matches inherited ownership (table owned via its schema).
- `?owner={team}` includes assets owned by that team, not by its members individually.
- Combines with other filters and with pagination.
- Unknown owner id → empty page, not `404`.
**RED**: Test with a directly-owned table and an inherit-only table, asserting both appear. Mutator watch: direct-only matching must fail the inherited case.
**GREEN**: recursive-CTE filter or a materialized effective-owner projection.
**REFACTOR**: assess whether inherited-owner filtering should be a maintained projection rather than a query-time walk. Query-time until measurement says otherwise — a premature projection adds an invalidation problem.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Users can follow assets

**Value**: A consumer records interest, and Epic 3's change events gain a meaningful audience.
**Path**: `PUT/DELETE /{collection}/{id}/followers/{userId}` → `follows` edges; `GET /users/{id}/follows`.
**Acceptance criteria**: follow is idempotent (double-follow → `200`, one edge); unfollow removes; `GET /users/{id}/follows` paginated across entity types; follower count on entity read; following a soft-deleted entity → `400`.
**RED**: Idempotency test asserting exactly one edge after two follows. Mutator watch: a non-idempotent insert must fail (or surface as `409`, which the test forbids).
**GREEN**: edges, endpoints, count projection.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Deleting a principal does not orphan assets

**Value**: Ownership stays truthful when someone leaves.
**Path**: `DELETE /users/{id}` and `/teams/{id}` check for owned assets; reject with a count, or reassign via `?reassignTo={id}`.
**Acceptance criteria**:
- Deleting an owner of assets → `409` reporting how many assets and of which types.
- `?reassignTo={id}` transfers ownership then deletes, in one transaction.
- Reassigning to a nonexistent or soft-deleted principal → `400`.
- Deleting a principal owning nothing succeeds.
- Deleting a team with child teams → `409` unless children are reassigned.
- Reassignment bumps each affected asset's version.
**RED**: Test asserting the `409` body carries the asset count. Reassign test asserting every asset moved *and* the principal is gone — a partial reassign must fail. Mutator watch: non-transactional reassignment.
**GREEN**: pre-delete check, transactional reassignment.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Notification delivery to followers** → needs a transport and a consumer; edges are recorded now.
- **SCIM / directory sync** → Epic 11 territory, once an IdP is in the picture.
- **Ownership approval workflow** → add if stewards report unilateral reassignment is a problem.
- **Domains / data products** (a second, orthogonal grouping axis) → not planned; revisit if org-scale grouping is requested. Named here so its absence is a decision.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
