# Plan: Envelope, Versioning, Soft Delete & Change Events (Epic 3)
**Branch**: feat/entity-envelope
**Status**: **Shipped** — envelope, history, `If-Match`/412. Demo 2
**Depends on**: Epic 2 (four entity types to apply the envelope to)
**Unblocks**: Epics 4 (the envelope is what the triple projection projects), 7, 8, 12, 13
**Crates**: `graph-owl-core` (EntityEnvelope, ChangeDescription, version arithmetic) · **`graph-owl-events`** (new — EventSink port) · `graph-owl-storage` · `graph-owl-storage-postgres` (history table) · `graph-owl-api` · `graph-owl-server`

## Goal

Give every entity the same block of metadata-about-metadata — version, history, authorship, tombstone — so a steward can answer "what changed, who changed it, and can I undo it".

## Why here and not later

This is the most expensive epic in the roadmap to defer. Applying the envelope to four entity types is one migration; applying it to twelve is twelve migrations plus twelve retrofits plus a backfill for every existing row. The cost curve is why this precedes ownership, tags, search, and connectors even though each of those is more visible to a user.

## Resolved decisions

1. **`Major.Minor` versions from `0.1`.** Minor for backward-compatible changes (description, tags, owners); Major for breaking ones (field removed, column dropped, type changed, rename).
2. **`ChangeDescription` is computed server-side by diffing before/after.** Not supplied by the client. This is the reason PATCH stays DTO-shaped rather than JSON Patch: a state diff describes *effect*, a patch document describes *intent*, and the audit trail wants effect.
3. **A no-op update produces no version and no event.** Connectors re-running against unchanged sources must not inflate history — this is what makes Epic 15's connector idempotency observable.
4. **Soft delete replaces hard delete** on the existing `DELETE /tables/{id}`. The only backward-incompatible change in the roadmap, landed deliberately before anyone depends on the old behavior.
5. **`GET` on a soft-deleted entity returns `200` with `deleted: true`, not `404`.** The metadata is still the truth about a table that used to exist. `404` would make tombstones invisible and restore undiscoverable.
6. **History is unbounded.** No retention policy until a real dataset shows one is needed. Pruning an audit trail is a decision that needs evidence.
7. **`EventSink` ships with a logging adapter only.** The port exists so Epic 8 can subscribe; a durable bus adapter waits for a consumer that justifies it.
8. **`updated_by` is the literal `system`** until Epic 12 (authentication) supplies real identity via the Epic 1 `Principal` seam.

## Acceptance criteria (feature level)

- [ ] Every entity carries the full envelope from `plans/00c-domain-model.md`.
- [ ] Editing a description bumps `0.1` → `0.2` and records the field diff.
- [ ] A PATCH that changes nothing returns `200`, does not bump the version, and emits no event.
- [ ] Dropping a column bumps Major.
- [ ] `GET /{collection}/{id}/versions` lists history; `.../versions/0.1` returns that snapshot.
- [ ] `DELETE` tombstones and cascades to children; `PUT /{id}/restore` is lossless including edges.
- [ ] `DELETE ?hardDelete=true` removes the row, its history, and its edges.
- [ ] `include=deleted|all` controls list visibility.
- [ ] `If-Match` with a stale version → `412` with the current version in the body.
- [ ] A `ChangeEvent` is emitted for every create, update, soft delete, restore, and hard delete.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`, `testing`, `mutation-testing`, `refactoring` loaded first, ending awaiting commit approval.

### Slice A: Entities carry a version that advances on change

**Value**: A steward can tell whether the description they read is the one they approved.
**Path**: `EntityEnvelope` in core, `#[serde(flatten)]` into all four entities; envelope columns added to all four tables by migration; facade computes the next version on update.
**Acceptance criteria**:
- Create → `version: "0.1"`.
- Description edit → `"0.2"`.
- Second edit → `"0.3"`.
- No-op PATCH → version unchanged, `200`.
- `updated_at` set by the database `now()`, never the client.
- `updated_by` is `system`.
- Existing rows are backfilled to `0.1` with their current `updated_at`.
**RED**: Facade tests over the sequence create → edit → edit → no-op, asserting the exact version string at each step. Migration test asserting pre-existing rows land at `0.1`. Mutator watch: an unconditional bump must fail the no-op case; a bump-by-major must fail the `0.2` case.
**GREEN**: envelope struct, migration, `next_version()`, change detection.
**REFACTOR**: change detection is the heart of the epic — assess whether it belongs in core (pure, testable, entity-agnostic) rather than the facade. Core.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice B: Every change records what changed

**Value**: "Someone edited this" becomes "Alice changed the description from X to Y" — the difference between an audit trail and a timestamp.
**Path**: `ChangeDescription { fields_added, fields_updated, fields_deleted, previous_version }` computed by diffing before/after; stored on the entity row.
**Acceptance criteria**:
- Adding a previously-null description → `fieldsAdded: [{name: "description", newValue: "..."}]`.
- Changing it → `fieldsUpdated` with both old and new.
- Clearing it → `fieldsDeleted` with the old value.
- Changing two fields at once records both.
- `previousVersion` is the version before the change.
**RED**: Table-driven facade tests over null→value, value→value, value→null, and multi-field. Mutator watch: swapped old/new values must fail — assert both, not just presence; a diff that reports every field regardless of change must fail the single-field case.
**GREEN**: diff function in core, storage column.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice C: Breaking changes bump the major version

**Value**: A consumer can tell at a glance whether an update could have broken their query.
**Path**: classify each field change as compatible or breaking; any breaking change → Major bump.
**Acceptance criteria**:
- Description/tag/owner change → Minor.
- Column removed → Major (`0.3` → `1.0`).
- Column data type changed → Major.
- Entity renamed → Major.
- Column added → Minor.
- A mixed change containing one breaking field → Major.
**RED**: Table-driven test over the six cases above, asserting exact resulting versions. Mutator watch: a classifier returning Minor unconditionally must fail the removal case; returning Major unconditionally must fail the description case.
**GREEN**: `is_breaking(field_change)` classifier; version arithmetic.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice D: History is queryable

**Value**: A steward can read the description as it stood three versions ago and see who changed it.
**Path**: `entity_versions` table keyed `(entity_type, entity_id, version)` storing the full snapshot; written on every version bump; `GET /{collection}/{id}/versions` and `.../versions/{v}`.
**Acceptance criteria**:
- Version list is newest-first, paginated.
- `.../versions/0.1` returns the entity exactly as it was.
- Unknown version → `404`.
- History survives soft delete.
- Hard delete removes it.
**RED**: Repository test making three edits then asserting each historical snapshot matches what was current at that time — not merely that three rows exist. Mutator watch: storing the post-change state under the pre-change version number must fail — assert snapshot content against version number.
**GREEN**: history table, write-on-bump, endpoints.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice E: Delete tombstones instead of destroying

**Value**: An accidental delete is recoverable. Currently it is not.
**Path**: `DELETE` sets `deleted = true`, bumps version, writes history. Cascades to children through `contains` edges, transactionally.
**Acceptance criteria**:
- `DELETE /tables/{id}` → `200` with `deleted: true`; the row still exists.
- `GET` afterwards → `200` with `deleted: true`, not `404`.
- Deleting a schema tombstones its tables.
- Deleting a database tombstones schemas and tables — the cascade is recursive, not one level.
- Relationship edges to a tombstoned entity are retained.
- Cascade is transactional.
**RED**: Repository test building the four-level hierarchy, deleting at the database level, asserting the deepest table is tombstoned. Test asserting edges survive. Mutator watch: a one-level cascade must fail — assert the deepest descendant.
**GREEN**: tombstone column, recursive cascade, transaction.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice F: Restore is lossless

**Value**: Recovery from the mistake Slice E made survivable.
**Path**: `PUT /{collection}/{id}/restore` clears the tombstone, bumps version, cascades to children tombstoned by the same operation.
**Acceptance criteria**:
- Restore returns `200` with `deleted: false` and a bumped version.
- Restoring a database restores the schemas and tables tombstoned with it.
- A child deleted *independently before* the parent stays deleted — restore does not resurrect it.
- Restoring a non-deleted entity → `409`.
- Relationship edges are intact afterwards.
**RED**: The independent-deletion case is the subtle one — delete a table, then delete its schema, then restore the schema, and assert the table stays deleted. Mutator watch: a restore that clears all descendant tombstones unconditionally must fail this test.
**GREEN**: cascade correlation — record which delete operation tombstoned each row, restore only that set.
**REFACTOR**: this needs a `deleted_by_operation` correlation id on the tombstone. Assess whether that belongs on the entity row or in a side table.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice G: Hard delete is explicit and complete

**Value**: A genuine purge path exists for mistakes that must not be recoverable — test data, accidental PII.
**Path**: `DELETE /{collection}/{id}?hardDelete=true` → `204`; removes row, history, and edges.
**Acceptance criteria**:
- Row, version history, and all incident edges removed.
- `GET` afterwards → `404`.
- Cascades to children.
- Hard-deleting a nonexistent entity → `404`.
- No event references a purged entity's content.
**RED**: Repository test asserting all three stores are clean, including edges pointing *at* the entity, not just from it. Mutator watch: deleting the row but leaving history or inbound edges must fail — assert each store separately.
**GREEN**: cascading purge in a transaction.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice H: Lists control tombstone visibility

**Value**: A consumer sees live assets by default; a steward can find deleted ones to restore.
**Path**: `?include=non-deleted|deleted|all` on every list endpoint, defaulting to `non-deleted`.
**Acceptance criteria**: default excludes tombstoned; `deleted` returns only tombstoned; `all` returns both; invalid value → `400`; pagination counts respect the filter.
**RED**: Test with two live and one tombstoned entity asserting counts under each of the three values, including `paging.total`. Mutator watch: a default that includes deleted must fail.
**GREEN**: filter parameter and predicate.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice I: Concurrent edits are detected

**Value**: Two connector runs against the same source stop silently overwriting each other.
**Path**: `If-Match: "0.2"` on mutating requests; mismatch → `412` with the current version.
**Acceptance criteria**:
- Matching version → succeeds.
- Stale version → `412`, entity unmodified, body carries the current version.
- Absent header → last-write-wins.
- Malformed header value → `400`.
**RED**: HTTP test doing read → concurrent edit → stale write, asserting `412` *and* that the entity was not modified. Mutator watch: a check that compares but proceeds anyway must fail the unmodified assertion.
**GREEN**: header parsing, compare-and-set in the update statement.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice J: Changes are emitted as events

**Value**: The search index (Epic 8 — `08-engine-search.md`) has something to subscribe to instead of polling.
**Path**: `graph-owl-events` crate with `EventSink` trait and `ChangeEvent`; logging adapter; facade emits after every successful mutation.
**Acceptance criteria**:
- Events for create, update, soft delete, restore, hard delete, each with the correct `eventType`.
- Event carries entity type, id, FQN, previous and current version, `changeDescription`, timestamp, principal.
- No event on a no-op update.
- Emission failure does not fail the request — logged, not propagated.
- Events are emitted after the transaction commits, never before.
**RED**: Facade tests with a recording sink asserting event type and payload per operation. A failing-sink test asserting the request still succeeds. Mutator watch: emitting before commit must fail a test that injects a commit failure and asserts no event; emitting on no-op must fail.
**GREEN**: crate, trait, logging adapter, post-commit emission.

**Part 1 shipped 28 July 2026** — `graph-owl-events` now carries `EventKind`, `EventSubject`, `ChangeEvent` and the `EventSink` trait. Two acceptance criteria are met **structurally**, which is stronger than meeting them by convention:

- *No event on a no-op update* — `ChangeEvent::updated()` returns `Option<Self>` and yields `None` on an empty diff. A facade that forgot to check cannot emit an empty event, because there is none to emit.
- *Emission failure does not fail the request* — `EventSink::emit()` returns `()`. There is no error for a caller to propagate by accident, so the rule holds without every call site remembering it.

Soft delete and restore are their own `EventKind`s rather than updates carrying a flag: a subscriber's reaction differs categorically (a search index *removes* on one and *adds* on the other), and a flag inside `Updated` makes that difference something every consumer must remember to look for. They also carry an empty diff, so routing them through `updated()` would silently drop them — which is why they have their own constructors.

9 tests, `fmt`/`clippy` clean, `cargo mutants` 6 mutants / 1 caught / 5 unviable / **0 survived**. The low mutant count is honest rather than reassuring: most of the file is struct construction, which `cargo mutants` cannot meaningfully mutate, so the score says little that the tests do not.

**Still to do in this slice**: the facade does not call any of it, so no real mutation produces an event yet. That remainder is the half where ordering can be got wrong — emission must follow the commit, and the RED for it is the injected-commit-failure test asserting no event was emitted.
**REFACTOR**: post-commit emission is easy to get subtly wrong. Assess whether emission belongs in the facade (has the domain context) or in a storage decorator (guaranteed post-commit). Facade, with the transaction boundary made explicit.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Durable event bus adapter** (Kafka, webhooks) → when a consumer outside the process needs events. The port makes this additive.
- **Version retention / pruning** → when a real dataset shows unbounded history is a problem.
- **`updated_by` with real identity** → Epic 12 (authentication), via the Epic 1 `Principal` seam.
- **Restoring a *specific historical version* as current** (rollback) → not planned; history is readable, not restorable. Add only if stewards ask.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Migration verified against a database seeded with pre-envelope rows.
5. `plans/00c-domain-model.md` **(built)** markers updated.
