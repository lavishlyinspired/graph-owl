# Plan: Table-to-Table Relationships

**Branch**: feat/entity-relationships
**Status**: Active

## Goal

Prove a generic entity-relationship pattern (polymorphic `from_entity_type`/`to_entity_type` + id pairs, so future entity types beyond `Table` can plug in without a schema change) while the only real entity in the system is still `Table`. Exercised end-to-end as Table→Table links (e.g. lineage: "this table is derived_from that table").

## Resolved decisions

1. **Scope**: relationships connect two `Table`s for now. The storage schema and domain type are generic (`entity_type` + `entity_id` string/uuid pairs on both ends) so a second entity type (e.g. `DatabaseSchema`) can participate later without a migration — but the HTTP API surface stays table-scoped (`POST /tables/:id/relationships`) since exposing raw `entity_type` params to clients would be meaningless while `Table` is the only entity type that exists.
2. **`relationship_type`**: a free-form non-empty string (e.g. `"derived_from"`), not a fixed enum — the taxonomy of relationship types isn't known yet and isn't this plan's job to invent.
3. **Uniqueness**: a DB-level unique constraint on `(from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id)` — creating the exact same relationship twice returns `409`, mirroring the `fully_qualified_name` conflict pattern from the Table entity plan.
4. **Referential validity**: both the source table (path param) and target table (`to_table_id` in the body) must exist as real `Table` rows — creating a relationship pointing at a nonexistent table returns `404`.
5. **No update**: relationships are immutable once created — no PATCH. To change one, delete and recreate.
6. **Direction-agnostic retrieval**: `GET /tables/:id/relationships` returns relationships where the table is on either the `from` or `to` side.

## Acceptance Criteria (feature-level)

- [ ] An API client can create a relationship between two existing tables via a real HTTP request, and it is durably persisted in Postgres.
- [ ] An API client can retrieve all relationships involving a given table (as either side).
- [ ] An API client can delete a relationship.
- [ ] Creating a relationship against a nonexistent table returns `404`, not a panic or 500.
- [ ] Creating a duplicate relationship returns `409`.

## Slices

Every slice follows RED-GREEN-MUTATE-KILL MUTANTS-REFACTOR. No production code without a failing test first.

### Slice A: API client creates a relationship between two tables — DONE

**Value**: A caller can record that one table relates to another (e.g. lineage/derivation) and have it durably persisted.
**Path**: `POST /tables/{id}/relationships` → axum handler (`graph-owl-server`) → `graph-owl-api::Catalog::create_relationship` (looks up both tables via existing `Storage::get_table` to confirm they exist) → `graph-owl-storage::Storage::create_relationship` → `graph-owl-storage-postgres` sqlx impl → new `entity_relationships` row inserted → `201` with the created relationship.
**Required implementation skills**: load `tdd`, `testing`, `mutation-testing`, `refactoring` before any code.
**Acceptance criteria** (to confirm with human before RED):
  - `POST /tables/{id}/relationships` with `{to_table_id, relationship_type}` where both tables exist returns `201` with a JSON body containing a generated UUID `id`, `from_entity_type: "table"`, `from_entity_id: id`, `to_entity_type: "table"`, `to_entity_id: to_table_id`, `relationship_type`, and `created_at`.
  - The row is verifiably present in Postgres afterward — repository-level integration test against real Postgres (testcontainers-rs), not a mock.
  - `POST /tables/{id}/relationships` where `id` (the path param) doesn't exist as a table returns `404`.
  - `POST /tables/{id}/relationships` where `to_table_id` doesn't exist as a table returns `404`.
  - `POST /tables/{id}/relationships` with an empty `relationship_type` returns `400`.
  - `POST /tables/{id}/relationships` creating the exact same `(from, relationship_type, to)` twice returns `409` on the second call.
**RED**: Two levels of failing test:
  1. `graph-owl-storage-postgres`: repository test asserting `create_relationship` persists, a follow-up query finds the row, and a duplicate insert of the same tuple returns a conflict error.
  2. `graph-owl-server`: axum integration test asserting `POST /tables/{id}/relationships` returns `201` with the expected shape, and the 404/400/409 error paths.
  Likely mutants to pre-empt: boundary mutants on the empty-string check, swapped `from`/`to` in the insert statement, the existence-check short-circuiting to always-true/always-false.
**GREEN**: `Relationship` struct in `graph-owl-core`; `Storage::create_relationship` trait method in `graph-owl-storage`; sqlx impl + migration creating `entity_relationships` in `graph-owl-storage-postgres`; `Catalog::create_relationship` in `graph-owl-api` (existence checks + delegation); `POST /tables/{id}/relationships` handler in `graph-owl-server`. Minimum code only — no list/delete yet.
**MUTATE**: run `mutation-testing` skill, produce report.
**KILL MUTANTS**: address survivors; ask human when value is ambiguous.
**REFACTOR**: assess only if it adds value.
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

### Slice B: API client retrieves all relationships for a table

**Value**: A caller can see everything a given table is related to, in either direction, without knowing relationship ids in advance.
**Path**: `GET /tables/{id}/relationships` → `Catalog::list_relationships_for_table` → `Storage::list_relationships_for_entity("table", id)` → Postgres `SELECT ... WHERE (from_entity_type, from_entity_id) = ('table', $1) OR (to_entity_type, to_entity_id) = ('table', $1)` → `200` with a JSON array (empty array when none exist). Returns `404` if the table itself doesn't exist.
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring`.
**Acceptance criteria** (to confirm before RED):
  - `GET /tables/{id}/relationships` for a table with no relationships returns `200` with `[]`.
  - `GET /tables/{id}/relationships` returns relationships where the table is the `from` side, and separately ones where it's the `to` side.
  - `GET /tables/{id}/relationships` for a nonexistent table returns `404`.
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

### Slice C: API client deletes a relationship

**Value**: A caller can remove a relationship that's no longer accurate.
**Path**: `DELETE /relationships/{relationship_id}` → `Catalog::delete_relationship` → `Storage::delete_relationship` → Postgres `DELETE` → `204`, or `404` if absent.
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring`.
**Acceptance criteria** (to confirm before RED):
  - `DELETE /relationships/{relationship_id}` for an existing relationship returns `204`, and a subsequent `GET /tables/{id}/relationships` on either endpoint no longer includes it.
  - `DELETE /relationships/{relationship_id}` for a nonexistent id returns `404`.
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

## Explicitly deferred (later slices, not this plan)

- A second real entity type (e.g. `DatabaseSchema`) actually using the polymorphic `entity_type` column with a value other than `"table"`
- A fixed taxonomy/enum of `relationship_type` values with type-specific validation (e.g. only allowing `belongsTo` between a `Table` and a `DatabaseSchema`)
- Cascading delete of relationships when a referenced table is deleted (currently: deleting a table does not clean up its relationships — dangling relationships are left behind)
- Pagination on the relationships list endpoint

## Pre-PR Quality Gate (each slice)

1. Mutation testing — run `mutation-testing` skill
2. Refactoring assessment — run `refactoring` skill
3. `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass
4. `cargo fmt --check`
