# Plan: Custom Properties (Epic 22)
**Branch**: feat/custom-properties
**Status**: Slice A shipped; Slice B partial (create path only); Slices C and D not started
**Depends on**: Epic 3 (the envelope's `extension` field)
**Crates**: `graph-owl-core` (CustomProperty, typed validation — pure) · `graph-owl-storage-postgres` (JSONB + GIN index) · `graph-owl-api` · `graph-owl-server`

## Goal

Let an organization add its own fields to entity types — `costCenter`, `retentionDays`, `sourceOfTruth` — without forking the schema or waiting for a release.

## Why this is not optional

Every organization has fields the catalog's authors did not anticipate. Without a supported mechanism they end up encoded in the description as free text, which is unsearchable, unvalidatable, and impossible to report on. The envelope already reserves `extension`; this epic gives it a schema.

## Resolved decisions

1. **Definitions are typed and validated on write.** An untyped bag is a description field with extra steps.
2. **Definitions are per entity type.** `costCenter` on `Table` need not exist on `User`.
3. **Definitions are themselves catalog entities** — versioned, ownable, auditable. Schema changes to an organization's metadata model deserve the same audit trail as the metadata.
4. **A supported type set, not arbitrary JSON Schema.** String, integer, number, boolean, date, timestamp, enum, entity-reference, and arrays of those. Arbitrary JSON Schema would make validation, indexing, and filtering unbounded problems.
5. **Removing a definition does not silently delete data.** It is rejected while values exist, exactly as with tags in Epic 8 (`08-engine-search.md`).
6. **Custom properties are searchable and filterable** — otherwise they are write-only, which is the failure mode this epic exists to prevent.

## The field this epic actually got, and why it is not the one the plan named

**The plan says "the envelope already reserves `extension`". It did not.** What
the envelope had was `properties` — and `properties` turns out to be a
*different thing wearing a similar shape*, which is why this epic added a column
rather than giving an existing one a schema.

`properties` is what the **source system** reported: a column's data type, a
service's engine. A connector writes it, and the upsert replaces it wholesale
(`properties = COALESCE(EXCLUDED.properties, assets.properties)`). `extension`
is what the **organization** added. Had custom properties gone into
`properties`, the next nightly connector run would have silently wiped every
hand-curated `costCenter` in the catalog — on the first night, with no error.

That distinction also forced a guard on the upsert itself:

```sql
extension = CASE WHEN $11 IS NULL THEN assets.extension ELSE EXCLUDED.extension END
```

A connector that sends no `extension` leaves it alone. `properties` above it
keeps its wholesale-replace semantics, because for source-reported metadata that
is correct. The two columns want opposite rules, which is the clearest possible
evidence they are not the same field.

## Acceptance criteria (feature level)

- [x] A custom property can be defined on an entity type with a type and optional constraints.
- [x] Values are validated on write against the definition.
- [~] Values round-trip through create, read, patch, and version history. **Create and read are done.** PATCH is not: `AssetUpdate` carries no `extension`, so a value can be set at creation and not yet changed in isolation. Version history is not: the change is not classified, so setting a value does not appear in `changeDescription`.
- [ ] Custom properties are filterable on list endpoints and searchable. Slice D — the GIN index exists, nothing queries through it yet. **Until this lands the feature is write-only**, which the plan itself calls worse than none.
- [ ] Changing a definition's type is rejected while values exist. Slice C — there is no update endpoint at all yet, so the unsafe change is impossible rather than guarded. Safe by absence, which is not the same as safe.
- [~] Deleting a definition with values → `409` unless forced. **The `409` is done and reports the count**; `?force=true` is not implemented, so a definition with values currently cannot be removed at all.
- [ ] A value change bumps the version and appears in the change description.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Properties can be defined — **shipped 2 August 2026**

**Value**: The vocabulary exists before anything uses it.
**Path**: `CustomProperty { name, entity_type, property_type, description, constraints }` + envelope; CRUD.
**Acceptance criteria**:
- Define a property on an entity type with one of the supported types.
- Duplicate name on the same entity type → `409`; on a different type → allowed.
- Unsupported type → `400` listing supported types.
- A name colliding with a built-in envelope field (`name`, `description`, `owners`) → `400`.
- `GET /custom-properties?entityType=table` lists definitions for a type.
- Enum type requires a non-empty value list.
**RED**: The built-in-collision test is the important one — a custom `description` would shadow the real field. Scoped-uniqueness pair. Mutator watch: absent collision check must fail; globally-scoped uniqueness must fail the different-type case.
**GREEN**: entity, type enum, validation, endpoints.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Values are set and validated — **partial**: validated on create, not yet on PATCH, and no version classification

**Value**: The property does something.
**Path**: `extension: Map<String, Value>` on the envelope; validated against definitions on write.
**Acceptance criteria**:
- Setting a defined property with a correct value succeeds and round-trips.
- Wrong type (string into integer) → `400` naming the property and both types.
- Undefined property name → `400`.
- Enum value outside the list → `400` listing valid values.
- Constraint violation (min/max, pattern, length) → `400` naming the constraint.
- Entity-reference type validates the target exists and is of the right type.
- Setting to null clears it; omitting leaves it unchanged (consistent with Epic 3's PATCH semantics).
- A value change bumps the version Minor and appears in `changeDescription`.
**RED**: Table-driven validation over every supported type with valid and invalid values. Omit-vs-null test. Mutator watch: validation that checks presence but not type must fail the wrong-type cases; an unconditional pass must fail all of them.
**GREEN**: validation engine, storage in `extension`, change tracking.
**REFACTOR**: validation is a pure function of (definition, value). Assess placing it in `core` rather than the facade — it is domain knowledge and exhaustively testable without I/O.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Definitions evolve safely

**Value**: The metadata model can change without silently corrupting data.
**Path**: guarded updates on definitions.
**Acceptance criteria**:
- Changing the description or display name is always allowed.
- Changing the type while values exist → `409` reporting the count.
- Widening a constraint (raising a max) is allowed; narrowing it while violating values exist → `409` reporting how many.
- Adding an enum value is allowed; removing one in use → `409`.
- Renaming a definition migrates existing values transactionally.
- Deleting a definition with values → `409`; `?force=true` removes definition and values transactionally, bumping affected versions.
**RED**: Narrowing-constraint test asserting the `409` reports the violating count. Force-delete test asserting every value is removed *and* every affected entity's version advanced. Mutator watch: allowing a narrowing that orphans values must fail; non-transactional force must fail.
**GREEN**: change classification, usage counting, transactional migration.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Custom properties are queryable

**Value**: Without this the feature is write-only, and write-only metadata is worse than none.
**Path**: `?extension.costCenter=CC-1234` on list endpoints; indexed in search.
**Acceptance criteria**:
- Equality filtering on string, integer, boolean, and enum properties.
- Range filtering (`gte`/`lte`) on numeric and date properties.
- Filtering on an undefined property → `400`, not silently empty.
- Filters compose with other filters and with pagination; `paging.total` respects them.
- Custom properties are indexed and returned in search results.
- Search facets are available for enum-typed properties.
- Filtering performance is acceptable — a supporting index exists, verified by query plan.
**RED**: Range-filter tests over numeric and date types. A test asserting an undefined property filter is `400`, not an empty page — the silent-empty failure mode is a data-leak-shaped bug (Epic 1's unknown-parameter rule). Mutator watch: silent-empty must fail it.
**GREEN**: JSONB indexing and filtering, search mapping, facets.
**REFACTOR**: assess whether filtering should be generic over `extension` or generated per definition. Generic with a GIN index — per-definition columns would mean a migration per property, defeating the purpose.
**Done when**: criteria met, query plan reviewed, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Computed / derived properties** → needs an expression language; revisit if requested.
- **Cross-entity property references beyond entity-reference type** → the reference type covers the realistic need.
- **Per-property access control** → Epic 13 operates on entities; property-level granularity only if asked.
- **Arbitrary JSON Schema** → deliberately excluded per decision 4. The supported type set can grow additively.
- **Required custom properties** (blocking create until set) → likely to break connector ingestion; revisit only with a clear workflow for it.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Query plans reviewed for custom-property filtering — a sequential scan here degrades every list endpoint.
