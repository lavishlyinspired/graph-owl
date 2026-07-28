# Plan: Entity Hierarchy & Columns (Epic 2)
**Branch**: feat/entity-hierarchy
**Status**: **Shipped** — service → database → schema → table → column, 34 assets from the connector. Demo 1
**Depends on**: Epic 1 (conventions, relationship taxonomy)
**Unblocks**: Epics 24 (`24-business-semantics.md`), 7, 8
**Crates**: `graph-owl-core` (service/database/schema/column types, FQN tokenizer) · `graph-owl-storage` (+ trait methods) · `graph-owl-storage-postgres` (migrations, cascade) · `graph-owl-api` · `graph-owl-server`

## Goal

Place every `Table` in its real context — service → database → schema → table — with a derived fully-qualified name, and show the table's shape via its columns.

## Why this matters

A `Table` named `customers` is nearly useless: a mid-size organization has a dozen. `snowflake_prod.warehouse.public.customers` is an address. FQN derivation is also a prerequisite for search relevance (Epic 2 — entity hierarchy & columns) and for the connector upsert key (Epic 8 — `08-engine-search.md`), so this cannot be postponed behind them.

## Resolved decisions

1. **Containment is a `contains` edge, not a `parent_id` column.** One traversal mechanism for the whole graph; an entity can be reachable via several relationship types without a column per type. Cost: "list tables in schema" is a join. Accepted and indexed.
2. **FQN is stored, denormalized, on each entity.** The commonest read ("show this table's path") then needs no traversal. The cost is cascade-on-rename, handled in Slice E.
3. **FQN parsing uses a real tokenizer.** Segments containing `.` are double-quoted. `split('.')` is wrong and will be wrong silently.
4. **Columns are an ordered child collection, not standalone entities.** Standalone columns cost a join on every table read and buy no capability the catalog needs. Consequence: a column cannot be independently soft-deleted. Revisit only if column-level ownership is required.
5. **`Table` requires a parent schema.** No orphan tables. Existing rows are migrated under a synthetic `default` service/database/schema so the migration is non-destructive.

## Acceptance criteria (feature level)

- [ ] Creating a table under a schema derives the four-part FQN automatically.
- [ ] Client-supplied `fullyQualifiedName` on create is rejected — it is derived, never settable.
- [ ] Renaming a database cascades the FQN change to every descendant schema, table, and column.
- [ ] `GET /tables/name/{fqn}` resolves a table by FQN, including quoted segments.
- [ ] `GET /tables/{id}?fields=columns` returns columns in source order.
- [ ] Creating a schema under a nonexistent database → `404`.
- [ ] Two schemas with the same name under the same database → `409`; under different databases → both succeed.

## Slices

Every slice runs the full RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR cycle with `tdd`, `testing`, `mutation-testing`, `refactoring` loaded first, and ends awaiting commit approval.

### Slice A: A DatabaseService can be registered

**Value**: A platform engineer registers the system that will be catalogued — the root every FQN hangs from.
**Path**: `POST /database-services` → `Catalog::create_database_service` → `Storage::insert_database_service` → row. FQN is the bare `name`.
**Acceptance criteria**:
- `POST` with `{name, serviceType, description?}` → `201`, FQN equals `name`.
- Duplicate name → `409`.
- Empty name → `400`.
- `serviceType` outside the known enum → `400`.
- Full CRUD: `GET` by id, `GET` list (paginated), `PATCH`, `DELETE`.
**RED**: Repository tests for insert/conflict/get/list/update/delete; HTTP tests for status codes and problem+json shape. Mutator watch: uniqueness scoping — assert the duplicate case *and* that two services with different names both succeed.
**GREEN**: `DatabaseService` in core, `Storage` methods, migration, facade, handlers.
**REFACTOR**: this is the first entity added after `Table`. Assess whether CRUD wiring is genuinely duplicated knowledge or merely similar shape — extract only if the former. Premature extraction here would fight the next four entity types.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice B: A Database belongs to a service

**Value**: The second level exists, and the first parent-child FQN is derived rather than typed.
**Path**: `POST /databases` with `{name, service: {id}}` → facade verifies the service exists, derives `FQN = service.fqn + "." + name`, inserts, creates the `contains` edge.
**Acceptance criteria**:
- FQN derived as `{serviceName}.{databaseName}`.
- Client-supplied `fullyQualifiedName` → `400`.
- Nonexistent service → `404`.
- Same database name under two different services → both succeed, distinct FQNs.
- Same name under the same service → `409`.
- A `contains` edge exists from service to database.
**RED**: Facade test asserting the derived FQN string exactly; repository test asserting the edge row. Mutator watch: the separator and the operand order — assert the full expected FQN literal, and assert the reversed-operand form is *not* produced.
**GREEN**: `Database` entity, derivation helper, edge creation.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice C: A DatabaseSchema belongs to a database

**Value**: The third level; proves the derivation generalises past one hop.
**Path**: as Slice B, one level down. FQN = `service.database.schema`.
**Acceptance criteria**: three-part FQN derived; nonexistent database → `404`; uniqueness scoped to the parent database; `contains` edge created.
**RED**: As Slice B at depth 3. Mutator watch: a derivation that only prepends the immediate parent (producing `database.schema`) must fail — assert the full three-part string.
**GREEN**: `DatabaseSchema`; generalise the derivation helper to walk the parent chain.
**REFACTOR**: with three levels built, the derivation logic is now genuinely shared knowledge. Extract `derive_fqn(parent_fqn, name)` and the parent-existence check if not already done.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice D: Tables live in schemas

**Value**: The existing `Table` gains its address. This is the slice that makes the catalog navigable.
**Path**: `POST /tables` now requires `databaseSchema: {id}`; FQN derived four-deep. A migration backfills existing rows under a synthetic `default.default.default` schema.
**Acceptance criteria**:
- `POST /tables` without a schema → `400`.
- FQN derived as `{service}.{database}.{schema}.{table}`.
- `GET /database-schemas/{id}/tables` lists tables in the schema, paginated.
- Existing tables survive the migration with a synthetic parent and a recomputed FQN.
- Table name uniqueness is scoped to the schema.
**RED**: Migration test asserting a pre-existing row is reparented and its FQN recomputed, not dropped. HTTP tests for the new required field and the listing endpoint. Mutator watch: uniqueness scoping — same table name in two schemas must both succeed.
**GREEN**: required parent, derivation, backfill migration, listing endpoint.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice E: Renaming an ancestor cascades FQNs

**Value**: Renaming a database in the source system does not silently break every descendant's address.
**Path**: `PATCH /databases/{id}` with a changed `name` → recompute own FQN → recursively recompute descendants, in one transaction.
**Acceptance criteria**:
- Renaming a database updates its schemas' and tables' FQNs.
- The cascade is transactional: a failure mid-cascade leaves no partially-renamed subtree.
- Entity `id`s are unchanged — relationship edges survive intact.
- A rename producing an FQN that collides with an existing one → `409`, nothing mutated.
- Renaming a leaf table touches only that row.
**RED**: Repository test building service→database→schema→table, renaming the database, asserting all four FQNs. A failure-injection test asserting rollback. A collision test asserting no partial mutation. Mutator watch: a cascade that stops after one level must fail — assert the deepest descendant, not just the child.
**GREEN**: recursive CTE update, or an explicit transactional walk.
**REFACTOR**: assess whether the recursive update belongs in the Postgres adapter or as a facade-orchestrated walk. Adapter-side is one round trip but is the first genuinely Postgres-shaped method — note it as a risk for Epic 5 (`05-engine-constraints.md`).
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice F: Entities resolve by fully-qualified name

**Value**: A user who knows the path does not need to know the UUID — and connectors get their upsert key.
**Path**: `GET /{collection}/name/{fqn}` → indexed lookup on the FQN column.
**Acceptance criteria**:
- Resolves for all four entity types.
- A quoted segment (`svc."my.db".schema.tbl`) resolves correctly.
- Unknown FQN → `404`.
- Reserved path collision: an entity literally named `name` does not shadow the route.
**RED**: Tokenizer unit tests over quoted segments, escaped quotes, and empty segments — this is where `split('.')` fails. Round-trip property: `parse(render(segments)) == segments`. Mutator watch: a tokenizer ignoring quotes must fail on the `"my.db"` case.
**GREEN**: FQN tokenizer/renderer in core; lookup endpoints; index on the FQN column.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice G: Tables expose their columns

**Value**: A consumer sees the shape of the data, not just its name — the difference between a catalog entry and a bookmark.
**Path**: `columns` as an ordered collection on `Table`; returned via `?fields=columns`; set on create and PATCH.
**Acceptance criteria**:
- `Column { name, data_type, data_length?, nullable, description?, ordinal_position }`.
- Returned in `ordinal_position` order regardless of storage order.
- `?fields=columns` opts in; omitted by default.
- Duplicate column name within a table → `400`.
- Column FQN is `{tableFqn}.{columnName}`, addressable for Epic 25 tagging.
- Replacing the column list on PATCH preserves descriptions for columns whose names are unchanged.
**RED**: Repository test inserting columns out of order and asserting sorted retrieval. Test asserting a PATCH that reorders columns preserves per-column descriptions by name. Mutator watch: a retrieval that returns insertion order must fail — insert deliberately shuffled.
**GREEN**: column storage (child table keyed by table id), ordered read, field selection.
**REFACTOR**: assess whether description-preserving merge belongs in the facade (domain rule) or the adapter (storage detail). Facade — it is a domain rule about not destroying human curation.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Non-database services** (dashboard, messaging, pipeline, ML, storage) → Epic 34. The hierarchy pattern is proven here first on one branch.
- **Table constraints, partitioning, table type** → modelled in `plans/00c-domain-model.md`, populated in Epic 15 when a connector supplies them. Adding fields nothing writes is speculative.
- **Column-level soft delete** → not planned; a consequence of decision 4. Column removal is a table-level version bump.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Verify the backfill migration on a database seeded with pre-migration rows — not only on an empty one.
