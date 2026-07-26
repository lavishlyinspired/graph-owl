# Plan: Table Entity Walking Skeleton

**Branch**: feat/table-entity
**Status**: Active — Slices A-B done; Slices C-E remain

## Goal

Prove the real end-to-end path (HTTP → domain → Postgres) for a single entity type, `Table`, with no auth, search, or versioning yet. The original scope for this ("Slice 1": one entity end-to-end — struct, Postgres table, CRUD REST endpoints, no auth, no search, no versioning) bundled Create+Read+Update+Delete into one unit; this plan breaks it into PR-sized vertical slices A-E per the planning skill.

## Resolved decisions

1. **Postgres in tests**: `testcontainers-rs` — a real ephemeral Postgres in Docker per test run, torn down after. No mocks.
2. **Minimal `Table` fields for this skeleton**: `id (Uuid)`, `name (String)`, `fully_qualified_name (String)`, `description (Option<String>)`, `created_at`, `updated_at`. Column schema, owners, tags, lineage are explicitly out of scope until later slices.
3. **`fully_qualified_name` uniqueness**: enforced now via a DB-level unique constraint; `POST /tables` with a duplicate returns `409`.
4. **Migration tool**: `refinery` — migrations live under `crates/graph-owl-storage-postgres/migrations/` as Rust-embedded SQL, run via `refinery::embed_migrations!` at startup/test-setup.

## Acceptance Criteria (feature-level)

- [ ] An API client can create a Table via a real HTTP request and it is durably persisted in Postgres (survives process restart).
- [ ] An API client can retrieve a previously created Table by id.
- [ ] An API client can list all Tables.
- [ ] An API client can update a Table's mutable fields.
- [ ] An API client can delete a Table, after which it is no longer retrievable.

## Slices

Every slice follows RED-GREEN-MUTATE-KILL MUTANTS-REFACTOR. No production code without a failing test first.

### Slice A: API client creates a Table and it is durably persisted — DONE

**Value**: Any caller of the catalog HTTP API can register a new Table entity that survives a server restart (real Postgres, not in-memory).
**Path**: `POST /tables` → axum handler (`graph-owl-server`) → `graph-owl-api::Catalog::create_table` → `graph-owl-storage::Storage` trait → `graph-owl-storage-postgres` sqlx impl → `tables` row inserted → 201 response with the created Table (generated `id`, submitted fields, timestamps).
**Required implementation skills**: load `tdd`, `testing`, `mutation-testing`, `refactoring` before any code.
**Acceptance criteria** (to confirm with human before RED):
  - `POST /tables` with a valid body `{name, fully_qualified_name}` (description optional) returns `201` with a JSON body containing a generated UUID `id`, the submitted fields, and `created_at`/`updated_at` timestamps.
  - The row is verifiably present in Postgres afterward — test asserts via a repository-level integration test against a real Postgres (per the resolved testcontainers-rs decision above), not a mock.
  - `POST /tables` with a missing required field (e.g. no `name`) returns `400` with an error body, not a panic or 500.
  - `POST /tables` with a duplicate `fully_qualified_name` returns `409`.
**RED**: Two levels of failing test:
  1. `graph-owl-storage-postgres`: a repository test (real Postgres via `testcontainers-rs`, schema applied via `refinery` migrations) asserting `insert_table` persists, a follow-up raw query finds the row, and a second `insert_table` with the same `fully_qualified_name` returns a conflict error.
  2. `graph-owl-server`: an axum integration test (`tower::ServiceExt::oneshot` or `axum-test`) asserting `POST /tables` returns 201 with the expected shape, wiring through a real `Catalog` backed by the real Postgres impl (via `testcontainers-rs`).
  Likely mutants to pre-empt (see `mutation-testing` skill's mutator-rules): boundary mutants on validation (empty string name), off-by-one on timestamp fields, swapped `id`/`fully_qualified_name` in the insert statement.
**GREEN**: `Table` struct in `graph-owl-core`; `Storage::insert_table` trait method in `graph-owl-storage`; sqlx impl + migration creating the `tables` table in `graph-owl-storage-postgres`; `Catalog::create_table` in `graph-owl-api`; `POST /tables` handler in `graph-owl-server`. Minimum code only — no update/delete/list yet.
**MUTATE**: run `mutation-testing` skill, produce report.
**KILL MUTANTS**: address survivors; ask human when value is ambiguous.
**REFACTOR**: assess only if it adds value (e.g. extracting a shared "insert row" helper is premature with one entity — likely skip).
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

### Slice B: API client retrieves a Table by id — DONE

**Value**: A caller can look up a specific Table they (or someone else) created.
**Path**: `GET /tables/:id` → `Catalog::get_table` → `Storage::get_table` → Postgres `SELECT` → 200 with Table body, or 404 if absent.
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring`.
**Acceptance criteria** (to confirm before RED):
  - `GET /tables/:id` for an id created in a prior request returns 200 with the full Table body.
  - `GET /tables/:id` for a non-existent id returns 404, not 500.
**RED/GREEN/MUTATE/KILL MUTANTS/REFACTOR**: same shape as Slice A, scoped to the read path only.
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

### Slice C: API client lists all Tables

**Value**: A caller can discover what Tables already exist without knowing ids in advance.
**Path**: `GET /tables` → `Catalog::list_tables` → `Storage::list_tables` → Postgres `SELECT *` → 200 with a JSON array (empty array when none exist).
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring`.
**Acceptance criteria** (to confirm before RED):
  - `GET /tables` with zero rows returns 200 with `[]`.
  - `GET /tables` with N created rows returns all N, each shaped like Slice A/B's Table body.
  - [DECIDE later, not blocking]: pagination is explicitly out of scope for this slice — full unpaginated list only.
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

### Slice D: API client updates a Table's mutable fields

**Value**: A caller can correct/evolve a Table's `name`/`description` after creation.
**Path**: `PATCH /tables/:id` → `Catalog::update_table` → `Storage::update_table` → Postgres `UPDATE` (bumps `updated_at`) → 200 with the updated Table, or 404 if the id doesn't exist.
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring`.
**Acceptance criteria** (to confirm before RED):
  - `PATCH /tables/:id` with `{description: "new"}` returns 200 with the field updated and `updated_at` advanced, `created_at` unchanged.
  - `PATCH /tables/:id` for a non-existent id returns 404.
  - `fully_qualified_name` and `id` are immutable via this endpoint (attempting to change them either is rejected or ignored — [DECIDE at slice start]).
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

### Slice E: API client deletes a Table

**Value**: A caller can remove a Table that no longer exists in the source system.
**Path**: `DELETE /tables/:id` → `Catalog::delete_table` → `Storage::delete_table` → Postgres `DELETE` (hard delete — soft-delete/versioning is an explicitly deferred later slice, see below) → 204, or 404 if absent.
**Required implementation skills**: `tdd`, `testing`, `mutation-testing`, `refactoring`.
**Acceptance criteria** (to confirm before RED):
  - `DELETE /tables/:id` for an existing id returns 204, and a subsequent `GET /tables/:id` returns 404.
  - `DELETE /tables/:id` for a non-existent id returns 404.
**Done when**: acceptance criteria met, mutation report reviewed, human approves commit.

## Explicitly deferred (later slices, not this plan)

- Relationships / `entity_relationship` table (e.g. `Table belongsTo DatabaseSchema`)
- Search indexing (Elasticsearch/OpenSearch)
- Versioning + change events, soft-delete
- Auth (JWT) + RBAC
- Connectors (Postgres/Snowflake introspection)
- Pluggable storage backend selection: a second `graph-owl-storage-mongodb` crate implementing the existing `Storage` trait, plus a factory/dispatch point (e.g. in `graph-owl-server`'s `main.rs`) choosing a backend by config at startup — the `Storage` trait already makes this additive, no rearchitecting needed when the concrete need arises

## Pre-PR Quality Gate (each slice)

1. Mutation testing — run `mutation-testing` skill
2. Refactoring assessment — run `refactoring` skill
3. `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass
4. `cargo fmt --check`

---
*Delete this file when the plan is complete. If `plans/` is empty, delete the directory.*
