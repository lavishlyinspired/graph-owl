# Plan: Custom Properties (Epic 22)
**Branch**: feat/custom-properties
**Status**: Slices A–D shipped
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
- [x] Values round-trip through create, read, patch, and version history. PATCH merges **per key**: a patch naming `costCenter` leaves `retentionDays` alone, and an explicit `null` clears the one key it names. The merged bag — not the patch — is what gets validated, so a patch cannot reach storage carrying a value a create would have refused.
- [x] Custom properties are filterable on list endpoints and searchable. `?extension.costCenter=CC-1234` on `GET /assets` and `GET /assets/search`, with `.gte`/`.lte` for ranges. Equality is written as JSONB containment so the GIN index can serve it, verified by an `EXPLAIN` test.
- [x] Changing a definition's type is rejected while values exist, reporting the count.
- [x] Deleting a definition with values → `409` naming `?force=true`; `?force=true` removes the definition and its values transactionally, bumping every affected entity's version.
- [x] A value change bumps the version and appears in the change description.

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

### Slice B: Values are set and validated — **shipped**

**Value**: The property does something.
**Path**: `extension: Map<String, Value>` on the envelope; validated against definitions on write.
**What it settled**: PATCH merges **per key**. A whole-bag replace would make a client that wants to change one field send all of them, which is a race every other client is also running — and the loser's value disappears with nothing failing. An explicit `null` clears the one key it names; an absent `extension` leaves the column alone, which is why a description edit does not wipe the organization's fields.
**And the ordering that matters**: the **merged** bag is validated, not the patch. Validating only what the client sent would let a patch add a key beside existing ones without ever revalidating them, so the check would pass for a bag no create would have accepted.

### Slice C: Definitions evolve safely — **shipped**

**Value**: The metadata model can change without silently corrupting data.
**Path**: `PATCH /custom-properties/{id}`; `DELETE ...?force=true`.

**One rule, not a classification table.** The criteria list four cases — type change, constraint narrowing, enum-value removal, and the widenings that are always fine — and the obvious implementation is four predicates over the *shape* of the change. That table has to be right for every combination of bound, type and enum member, and the first combination it gets wrong silently orphans data.

So the check is: apply the change, then re-run the **write-path validator** over every value that already exists. A widening admits everything it did before and passes; a narrowing that strands values fails and reports how many. It cannot disagree with what a write would do, because it is the same function — and no case can be forgotten, because there are no cases. The cost is reading one property's values; a description edit skips it entirely, since nothing about what a value must satisfy moved.

**`entityType` is immutable by DTO shape**, the pattern Epic 3 used for `TableUpdate`'s id. Moving a definition between types would orphan every value under the old one, and there is nothing a client can send that would do it.

**`?force=true` is row by row on purpose, and it is the expensive choice.** One `UPDATE ... SET extension = extension - $name` would strip every value in a single statement and record none of it — no version bump, no history row, no diff. An entity whose `costCenter` vanished has changed, and a catalog that cannot say when is the catalog this epic exists to replace. Force-deleting a definition is rare, admin-only and deliberately typed; paying per row for an auditable operation is the right side of that trade.

**Mutants watched**: a check that classified the change rather than validating the values must fail `removing_an_unused_enum_value_is_allowed`; a non-transactional force must fail the version assertions in `force_deleting_removes_the_values_and_bumps_every_affected_version`.

### Slice D: Custom properties are queryable — **shipped**

**Value**: Without this the feature is write-only, and write-only metadata is worse than none.
**Path**: `?extension.costCenter=CC-1234` on `GET /assets` and `GET /assets/search`; `.gte`/`.lte` for ranges.

**The syntax**: `extension.<name>` for equality, `extension.<name>.gte` / `.lte` for bounds. A dotted suffix rather than `[gte]` because brackets need percent-encoding in strict clients, and the whole filter stays one flat `name[.op]=value` grammar that reads the same as the `extension.` prefix in front of it. A range is two filters on one property, which falls out of the conventions doc's "repeated params are AND" rather than needing a grammar of its own. An unrecognised suffix is a `400`: `?extension.retentionDays.gt=30` is somebody meaning `gte`, and reading it as a property called `retentionDays.gt` answers with an empty page and no hint.

**Why they cannot go through `AppQuery`.** Custom property names are defined at runtime, so no struct can name them, and the only serde shape that accepts them is a flattened map — which absorbs *every* unrecognised parameter and silently repeals `deny_unknown_fields` for the endpoint. `?ownr=alice` would go back to being a filter that matches everything. So `extension.*` pairs are split off the raw query first and the remainder goes through the same strict extractor as before; a typo'd property name is caught one layer down, against the definitions.

**Coercion belongs in the facade.** A query string carries only text, and `retentionDays=30` means the number thirty because the *definition* says so — not because it happens to parse as one. Guessing would make a string property whose values are digits unfilterable.

**The index, and the honest half.** Equality is written as JSONB **containment** (`@>`), not `extension -> name = value`. The two are equivalent and only one is indexable: `jsonb_path_ops`, the operator class `assets_extension` uses, supports `@>` and nothing else — written the other way, the most common filter there is becomes a sequential scan of the whole table. `equality_filtering_uses_the_extension_index` asserts the plan with `enable_seqscan = off`, which is what makes the assertion about the *operator* rather than about the row count.

**Ranges are deliberately not index-backed.** A btree on `(extension -> 'retentionDays')` supports one property, so a generic range index means an index per definition — precisely the per-property migration decision 4 refuses. They filter what the indexable predicates (`kind`, visibility, any equality filter) already narrowed. When one property becomes hot enough to matter, an expression index on that one property is the escape hatch and it needs no code change.

**Facets are enum-only.** A facet is a short closed list somebody clicks; a facet over free text is one bucket per value, which is a report.

## Explicitly deferred (with destination)

- **Range filters served by an index** → an expression index per hot property, added when a measurement names one. See Slice D.
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
