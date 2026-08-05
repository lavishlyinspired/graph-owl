# Plan: Entity Expansion (Epic 34)
**Branch**: feat/entity-expansion (one branch per entity family)
**Status**: **Shipped** — six families (dashboards, topics, pipelines, ML models, storage containers, plus the cross-family interop slice), one generic `Asset`/`upsert_asset`, no per-kind Rust struct. Two acceptance-criteria gaps found and recorded rather than silently closed: Slice E's "cascades on rename" (Epic 2 Slice E never shipped) and decision 28 in `00b-architecture.md` (the `Container`/`Column` parent special-case).
**Depends on**: Epic 8 (each new type indexes for free — the property being demonstrated), Epic 3 (envelope), Epic 3 (search), Epic 3 (tags)
**Unblocks**: nothing — this is breadth, not depth
**Crates**: `graph-owl-core` (five entity families) · `graph-owl-storage-postgres` (migrations per family) · `graph-owl-api` · `graph-owl-server` — **no new crates, and no change to `graph-owl-engine`, `graph-owl-search`, or `graph-owl-authz`. A change to any of those is the architectural finding this epic exists to surface.**

## Goal

Widen the catalog from database assets to the rest of the data platform: dashboards, topics, pipelines, ML models, and object storage.

## Why last

Every pattern this epic needs — hierarchy, envelope, versioning, search indexing, tagging, lineage — is built and proven by Epics 2, 3, 8, 25, and 29. Each new entity family is then largely mechanical. Doing it earlier would mean building five entity types before the envelope existed and migrating all five afterwards.

**This epic is the real test of the architecture.** If adding `Topic` requires changing the relationship table, the envelope, the search port, or the authz resource model, then Epics 3–3 got something wrong, and this is where it surfaces. Each slice therefore carries an explicit "no core change required" acceptance criterion — treat a violation as a signal to stop and reconsider, not as a line to edit.

## Resolved decisions

1. **Each entity family is an independent slice.** Any one can be dropped, reordered, or deprioritized without affecting the others. There is no shared prerequisite left to build.
2. **Every family follows the identical template** (below). Deviations must be justified in the slice, not absorbed silently.
3. **No new relationship types are added** unless a family genuinely needs one. The Epic 24 (`24-business-semantics.md`) taxonomy is expected to suffice; a needed addition is a finding worth recording.
4. **Scope is exhaustive.** The five families below are the complete list. Anything beyond them is new work, not deferred work.

## The template

Each family repeats this, and the acceptance criteria below are assumed for every slice rather than restated:

1. Service entity (root of the FQN, connection config).
2. Asset entities, hierarchically related by `contains`.
3. Full CRUD with the envelope (Epic 3 — envelope, versioning, soft delete): versioning, change description, soft delete, restore, history.
4. FQN derivation and cascade-on-rename (Epic 2 — entity hierarchy & columns machinery, reused unchanged).
5. Search indexing and facets (Epic 8 — `08-engine-search.md`), including the relevance-ordering assertions from its Slice D.
6. Tag and glossary-term attachment (Epic 11).
7. Ownership, inherited (Epic 11).
8. Lineage participation (Epic 11) where the family has upstream/downstream semantics.
9. Authorization as a distinct resource type (Epic 13 — authorization & policy).
10. **No change required** to the relationship table, envelope, search port, or authz resource model.

## Acceptance criteria (feature level)

- [x] Five entity families exist with full CRUD and the envelope.
- [x] Each is searchable, taggable, ownable, and access-controlled.
- [x] Each participates in lineage where semantically meaningful.
- [x] Adding each required zero changes to the relationship table, envelope, search port, or authz resource model. Two `Storage` trait methods were added (`bump_version`, `lineage_edges_by_pipeline`) and two purely-additive migrations (a fourth `search_vector` weight, a nullable `lineage_edges.pipeline_asset_id` column) — neither is the relationship table, the envelope's shape, the search port's trait, or the authz resource model, so the criterion holds on its own literal terms. Recorded here rather than left to be discovered by a later reader diffing the `Storage` trait.
- [x] Cross-family lineage works (pipeline → table → dashboard). Proven as `Container → Table → Table (pipeline-attributed) → Dashboard`: Pipeline itself is an attribution on an edge (`LineageDetails.pipeline`), not an edge endpoint, matching Slice C's own design — there is no `(Pipeline, Feeds, X)` row in `LEGAL_LINEAGE` and there should not be one.
- [x] A single search returns hits across all families with correct facet counts.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first. Each slice ends with the template's ten criteria verified.

### Slice A: Dashboards and charts

**Value**: A consumer finds the dashboard built on a table, and — via lineage — learns what breaks if the table changes. The highest-value family, because dashboards are where non-engineers meet data.
**Path**: `DashboardService` → `Dashboard` → `Chart`.
**Family-specific acceptance criteria**:
- `Dashboard { name, description, url, charts, dataModels }`.
- `Chart { name, chart_type, url, description }`; chart types are a closed enum.
- A dashboard may reference charts from several sources.
- Lineage: `Table upstream Dashboard`; a chart inherits its dashboard's upstream by default.
- Search ranks a dashboard by its own name and by its charts' names.
- Deleting a dashboard cascades to its charts.
**RED**: Cross-family lineage test (table → dashboard) asserting traversal from either end. Search test asserting a dashboard is findable by a chart's name. Mutator watch: chart names excluded from the indexed document must fail the search test.
**Done when**: template criteria plus the above met, mutation report reviewed, commit approved.

### Slice B: Topics

**Value**: Streaming data becomes catalogable, closing the gap where event pipelines are invisible.
**Path**: `MessagingService` → `Topic`.
**Family-specific acceptance criteria**:
- `Topic { name, partitions, replication_factor, retention_time, cleanup_policy, message_schema }`.
- `message_schema` holds fields analogous to columns — taggable individually by FQN, exactly as columns are (Epic 2 — entity hierarchy & columns Slice C machinery reused).
- Schema field types cover Avro/JSON-Schema/Protobuf primitives.
- Lineage: `Topic upstream Table` and `Table upstream Topic` both valid.
- Partition and retention changes are Minor; a schema field removal is Major (Epic 3 Slice C classifier extended, not replaced).
**RED**: Test asserting a schema field removal bumps Major. Test asserting a schema field is independently taggable. Mutator watch: schema changes classified Minor must fail; a classifier extension that breaks the existing column cases must fail Epic 22's tests.
**Done when**: template criteria plus the above met, mutation report reviewed, commit approved.

### Slice C: Pipelines

**Value**: The jobs that move data become visible, and lineage gains its missing middle — *how* data got from A to B, not just that it did.
**Path**: `PipelineService` → `Pipeline`, with tasks.
**Family-specific acceptance criteria**:
- `Pipeline { name, description, tasks, schedule, url }`.
- `Task { name, task_type, description, downstream_tasks }` — tasks form a DAG.
- Task DAG cycles → `422` (reusing the cycle detector from Epic 11).
- Lineage: a pipeline is the `pipeline` field in `LineageDetails` (Epic 29), connecting the tables it reads to those it writes.
- Pipeline run status and last-run timestamp are recorded.
- A pipeline referenced by lineage cannot be hard-deleted without `?force=true`.
**RED**: Test asserting a task DAG cycle is rejected at depth 3. Test asserting a pipeline referenced in `LineageDetails` resists hard delete. Mutator watch: absent cycle detection; unguarded hard delete.
**Done when**: template criteria plus the above met, mutation report reviewed, commit approved.

### Slice D: ML models

**Value**: Models become first-class assets, and their training-data lineage becomes traceable — increasingly a compliance requirement.
**Path**: `MlModelService` → `MlModel`.
**Family-specific acceptance criteria**:
- `MlModel { name, algorithm, features, target, hyperparameters, ml_store, server }`.
- `Feature { name, data_type, feature_sources, description }` — sources reference table columns by FQN.
- Lineage: `Table upstream MlModel` derived from feature sources.
- A feature source naming a nonexistent column → `400`.
- Hyperparameter changes are Minor; a feature set change is Major.
**RED**: Test asserting lineage edges are derived from feature sources rather than asserted separately. Mutator watch: a model that stores feature sources without creating lineage must fail it.
**Done when**: template criteria plus the above met, mutation report reviewed, commit approved.

### Slice E: Storage containers

**Value**: Object-store data — the lake half of a lakehouse — stops being invisible.
**Path**: `StorageService` → `Container`, nesting.
**Family-specific acceptance criteria**:
- `Container { name, prefix, file_formats, size, number_of_objects, data_model }`.
- Containers nest (a bucket contains prefixes), reusing `contains`.
- `data_model` holds columns where the format is structured (Parquet, Avro), reusing the column machinery.
- Lineage: `Container upstream Table` for external-table patterns.
- Deep nesting (5+ levels) derives FQNs correctly and cascades on rename.
**RED**: Deep-nesting FQN cascade test at 5 levels — Epic 2's cascade was proven at 4. Mutator watch: a depth-limited cascade must fail it.
**Done when**: template criteria plus the above met, mutation report reviewed, commit approved.

**Found while implementing, not before**: "cascades on rename" was written assuming Epic 2 Slice E ("Renaming an ancestor cascades FQNs", `02-entity-hierarchy.md`) had shipped — this plan's own dependency line 4 says the machinery is "reused unchanged". It has not: `AssetUpdate` (the only PATCH DTO any asset kind has) carries `description` and `extension` alone, no `name` field, and no rename/reparent endpoint exists for `Asset` anywhere in `graph-owl-api` or `graph-owl-server`. Five-level FQN *derivation* (the RED test above) is fully implemented and tested — it needs only the create-time machinery Epic 2 actually shipped. "Cascades on rename" is not implementable by this epic without first building Epic 2 Slice E itself, which is out of this epic's scope. Left unchecked below rather than silently marked done.

### Slice F: The families interoperate

**Value**: The catalog behaves as one graph rather than five silos — the payoff for the whole epic.
**Path**: cross-family search, lineage, and filtering.
**Acceptance criteria**:
- One search query returns hits across all families with correct per-type facet counts.
- A lineage traversal crosses families in one graph: `Container → Table → Pipeline → Dashboard`.
- `?owner={team}` filters across families.
- A tag applied across families filters across all of them.
- Traversal depth limits and cycle detection hold across family boundaries.
- Authorization filters consistently across families — no family leaks through the predicate.
**RED**: A four-family lineage chain test traversing end to end. A search test asserting facet counts across five types. An authz test asserting a principal denied on one family sees no hits from it in a cross-family search. Mutator watch: a per-family authz predicate that omits one family must fail the last test.
**REFACTOR**: with five families built, assess how much CRUD wiring is genuinely duplicated *knowledge* versus merely similar shape. This is the right moment for that judgment — with one entity type it was premature, with six the answer is evidence-based.

**Found while implementing the authz RED test**: `InMemoryStorage::policies_for_roles` (the fake `Storage` adapter graph-owl-api's own tests run against) was checking `roles.contains(&policy.name)` — a policy's own name, treated as if it were a role — instead of joining through the `role_policies` attachment table the way `graph-owl-storage-postgres`'s real implementation does (`policies p JOIN role_policies rp ON rp.policy = p.name WHERE rp.role = ANY($1)`). Every existing authz test in this file had, by coincidence, named its test policy and its test role identically (both called `"analyst"`, or both `"agent"`), which is exactly what made the bug invisible: `roles.contains(&policy.name)` and the real role-attachment join produce the same answer whenever the two names happen to match, and diverge the moment they do not — which is what this slice's own policy (`"topics-only"`, attached to role `"restricted"`) finally was. Fixed in the fake (`policies_for_roles` now reads `role_policies`, matching the Postgres contract), and every test call site that pre-seeded `storage.policies` directly was moved onto the real `upsert_policy` path so the attachment table is populated the way a caller would populate it. The real backend was never wrong — this was a test-double-only bug, caught by design principle 10's own authz test working exactly as the plan intended it to.

**REFACTOR, done**: the CRUD wiring is *not* duplicated across families — there is exactly one `Catalog::upsert_asset`, one `check_family_properties`, one `LEGAL_LINEAGE` table, gaining a match arm or a row per family rather than a struct, handler, or endpoint per family. That is design principle 10 working as intended, not an accretion to clean up: a fifth family cost roughly the same lines as the first (containment entry, an optional closed-enum match arm, a lineage row, a test module), and none of it lives in five different places the way five separate handlers would. The one place *near*-duplication actually shows up is the test helpers — `a_dashboard`, `a_topic`, `a_pipeline`, `a_model`, `a_storage_service` each build a two-level parent chain that looks identical in shape. Judged not worth extracting: each is 8–12 lines, the kind and the two names are the only things that differ, and a generic `a_family_chain(catalog, root_kind, root_name, leaf_kind, leaf_name)` would trade a reader matching a helper's name to the family it builds for one more indirection at every call site, to save what amounts to a handful of lines total across five test modules. Similar shape, not shared knowledge — leaving it alone is the correct call, not a deferral.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

Nothing in this epic is deferred — the five families are the exhaustive scope. Entity types beyond them (API endpoints, reports, notebooks, feature stores, data contracts) are **new work**, planned when a user needs them, not deferred work.

Recorded so their absence is a decision:

| Not built | Would add when |
|---|---|
| `ApiService` → `ApiEndpoint` | API-as-data-product becomes a use case |
| `Report` (as distinct from Dashboard) | Reporting tools are catalogued and the distinction matters |
| Notebooks | Notebook-driven analysis needs cataloguing |
| Feature store entities | A feature store is adopted and `MlModel.features` proves insufficient |
| Data contracts | Contract enforcement becomes a requirement |

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment — deferred to Slice F for the cross-cutting judgment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. **Architecture check**: confirm each family required no change to the relationship table, envelope, search port, or authz resource model. Record any violation in `plans/00b-architecture.md`'s decision log — it is evidence an earlier abstraction was wrong, and that is worth more than a clean checkbox.
