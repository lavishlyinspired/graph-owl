# graph-owl — Domain Model
**Crate scope**: `graph-owl-core` (types) · `graph-owl-engine` (triple projection) — this document is their specification.

The vocabulary and rules every entity obeys, in both representations: **relational** (source of truth) and **triples** (graph view). When code and this document disagree, this document is right.

## Identity: three names, three jobs

| Identifier | Type | Stable across rename? | Used for |
|---|---|---|---|
| `id` | UUID v4 | Yes | Every internal reference, every edge, every API path |
| `name` | String | No | The leaf name in the source system (`customers`) |
| `fully_qualified_name` | String | No | Human-addressable path (`snowflake_prod.warehouse.public.customers`) |

**Edges reference `id`, never FQN.** FQNs change when an ancestor is renamed; a rename must not orphan the graph.

FQN is **derived**, never client-settable — recomputed from the parent chain on create and on any ancestor rename. Uniqueness is a database constraint, so a duplicate is `409`.

### FQN construction

Dot-separated, root to leaf, each segment being that entity's `name`:

```
databaseService.database.databaseSchema.table
databaseService.database.databaseSchema.table.column
```

A segment containing a literal `.` is double-quoted: `service."my.db".schema.table`. Parsing therefore needs a real tokenizer — `split('.')` is wrong and fails silently.

## The entity envelope

One `EntityEnvelope` in `graph-owl-core`, `#[serde(flatten)]`-ed into every entity; one identical column set per table; one fixed predicate set in the graph projection.

| Field | Type | Semantics |
|---|---|---|
| `id` | `Uuid` | Immutable |
| `name` | `String` | Leaf name, non-empty, mutable (triggers FQN recompute) |
| `display_name` | `Option<String>` | Falls back to `name` |
| `fully_qualified_name` | `String` | Derived, unique, never client-settable |
| `description` | `Option<String>` | Markdown |
| `version` | `EntityVersion` | `Major.Minor`, starts `0.1` |
| `updated_at` | `DateTime<Utc>` | Set by the database, never the client |
| `updated_by` | `String` | Principal. Literal `system` until Epic 12 |
| `change_description` | `Option<ChangeDescription>` | Server-computed diff |
| `deleted` | `bool` | Soft-delete tombstone |
| `owners` | `Vec<EntityReference>` | Users or teams (Epic 11) |
| `tags` | `Vec<TagLabel>` | Classification and glossary labels (Epic 25) |
| `domain` | `Option<EntityReference>` | Accountability boundary (Epic 23) |
| `lifecycle` | `LifecycleState` | Draft / Active / Deprecated / Retired (Epic 26) |
| `certification` | `Option<Certification>` | Issuer, criteria, expiry (Epic 26) |
| `extension` | `Option<Value>` | Custom properties, schema-validated (Epic 22) |

`created_at` is deliberately absent — recoverable as the timestamp of version `0.1`, or of the earliest flake with this subject.

## Triple projection

Every entity is projected into flakes. Relational is the source of truth; flakes are the graph view (`00b-architecture.md`, decision 12).

### Flake

```rust
pub struct Flake {
    pub s:  Sid,            // subject
    pub p:  Sid,            // predicate
    pub o:  FlakeValue,     // object
    pub cx: Option<Sid>,    // named graph; None = default
    pub t:  i64,            // transaction time (logical clock)
    pub op: bool,           // true = assert, false = retract
}

pub struct Sid { pub namespace_code: u16, pub id: String }

pub enum FlakeValue {
    Ref(Sid), String(String), Boolean(bool),
    Int(i64), Float(f64), Instant(DateTime<Utc>), Json(String),
}
```

**`op = false` is a retraction, not a delete.** This is why time-travel is native: state at any past `t` is recoverable by construction. Removing the row would break it.

### Namespace registry

| Code | Prefix | IRI |
|---|---|---|
| `0x0001` | `dsc:` | `https://graph-owl.dev/ns/catalog#` |
| `0x0100` | `rdf:` | `http://www.w3.org/1999/02/22-rdf-syntax-ns#` |
| `0x0101` | `rdfs:` | `http://www.w3.org/2000/01/rdf-schema#` |
| `0x0102` | `xsd:` | `http://www.w3.org/2001/XMLSchema#` |
| `0x0103` | `owl:` | `http://www.w3.org/2002/07/owl#` |
| `0x0104` | `shacl:` | `http://www.w3.org/ns/shacl#` |
| `0x0106` | `schema:` | `https://schema.org/` |
| `0x0107` | `dcterms:` | `http://purl.org/dc/terms/` |

### Core predicates

Envelope fields map one-to-one: `dsc:type`, `dsc:name`, `dsc:displayName`, `dsc:fqn`, `dsc:description`, `dsc:version`, `dsc:createdAt`, `dsc:updatedAt`, `dsc:updatedBy`, `dsc:deleted`, `dsc:owner`, `dsc:tag`, `dsc:domain`, `dsc:lifecycle`, `dsc:extension`.

Structural: `dsc:parentSchema`, `dsc:parentTable`, `dsc:dataType`, `dsc:ordinalPosition`, `dsc:nullable`.

Provenance: `dsc:sourceType`, `dsc:sourceUrl`, `dsc:confidence`, `dsc:lastVerifiedAt`.

Custom predicates are definable at runtime through `PredicateRegistry`, stored with datatype and cardinality. Core predicates are fixed in Rust.

### Named graphs

`cx` isolates provenance and scopes reasoning:

| Graph | `cx` | Holds |
|---|---|---|
| Default | `None` | Core catalog facts |
| Extraction | `graph:extraction` | Facts from document ingestion (Epic 21) |
| Reasoning | `graph:reasoning` | Derived facts — **overlay, never persisted into base** |
| Import | `graph:import:{source}` | One connector run, so a failed import is deletable wholesale |

Selective reasoning matters: reason over the default graph, not over unconfirmed extractions.

## Relationships

One generic polymorphic edge serves every relationship:

```
(from_entity_type, from_entity_id, relationship_type, to_entity_type, to_entity_id)
```

Uniqueness on the whole 5-tuple; a duplicate is `409`. Adding an entity type needs no migration — only a new `entity_type` value.

### Reified in the graph

A relationship is a **node**, not a bare predicate assertion:

```
(rel)  rdf:type        dsc:Relationship
(rel)  dsc:fromEntity  (table_a)
(rel)  dsc:toEntity    (table_b)
(rel)  dsc:relType     "feeds"
(rel)  dsc:confidence  0.95
```

Edges carry payloads — confidence, provenance, SQL, lineage detail. A flat predicate cannot hold them. The cost is two hops to traverse; the benefit is queries like "every relationship below 0.5 confidence", which the flat form cannot express at all.

### Taxonomy

One stored direction per pair; the inverse is exposed on read. Storing both doubles every edge and creates a consistency burden.

| Stored | Inverse | Purpose |
|---|---|---|
| `contains` | `belongsTo` | Hierarchy |
| `parentOf` | `childOf` | Teams, domains, terms |
| `owns` | `ownedBy` | Accountability |
| `uses` | `usedBy` | Consumption |
| `dependsOn` | `dependencyOf` | Non-data dependency |
| `produces` | `producedBy` | Pipeline output |
| `consumes` | `consumedBy` | Pipeline input |
| `feeds` | `fedBy` | Directional data flow (lineage) |
| `derivedFrom` | `derives` | Transformation provenance |
| `implements` | `implementedBy` | Contract fulfilment |
| `governedBy` | `governs` | Policy application |
| `definedBy` | `defines` | Metric → glossary term |
| `validatedBy` | `validates` | Asset → quality test |
| `documents` | `documentedBy` | Runbook → asset |
| `mentionedIn` | `mentions` | Conversation → asset |
| `capturedAs` | `capturedFrom` | Source → memory object |
| `sameAs` | — | Resolved duplicate identity (Epic 17), reversible |
| `appliedTo`, `follows`, `relatedTo`, `testedBy`, `reviews`, `expert`, `joinedWith` | — | — |

`feeds` and `derivedFrom` together replace a single `upstream` — lineage explainability needs *flow* and *provenance* separated.

Not every `(from_type, type, to_type)` triple is legal. Epic 1 adds a validation table; `Table contains Database` is `400`, not silently stored.

### Hierarchy is a relationship, not a foreign key

Containment is a `contains` edge, not a `parent_id` column. One traversal mechanism for the whole graph, and an entity can be reachable via several relationship types without a column per type. The cost is that "list tables in this schema" is a join — accepted and indexed. The one denormalization is each entity's own derived FQN, so the commonest read needs no traversal.

## Versioning

`Major.Minor` from `0.1`. **Minor** for backward-compatible change (description, tag, owner). **Major** for breaking change (field removed, column dropped, type changed, rename).

```rust
struct ChangeDescription {
    fields_added:   Vec<FieldChange>,
    fields_updated: Vec<FieldChange>,   // { name, old_value, new_value }
    fields_deleted: Vec<FieldChange>,
    previous_version: EntityVersion,
}
```

Server-computed by diffing before/after — not client-supplied. This is why PATCH stays DTO-shaped: a state diff describes *effect*, a patch document describes *intent*, and an audit trail wants effect.

**A no-op update produces no version and no event.** This is what makes connector idempotency (Epic 15) observable and therefore testable.

## Soft delete

`DELETE` sets `deleted = true` and retracts the entity's flakes. It does not remove rows.

- Excluded from list and search unless `?include=deleted|all`.
- `GET` on a tombstoned entity is `200` with `deleted: true`, never `404` — the metadata is still the truth about a table that used to exist.
- Deleting a container cascades to children; edges are retained so restore is lossless.
- `PUT /{id}/restore` clears the tombstone and bumps the version. A child deleted *independently before* its parent stays deleted.
- Hard delete (`?hardDelete=true`) removes rows, history, flakes, and edges. The only irreversible operation.

## Confidence

One `ConfidenceScore` across extraction, resolution, and reasoning:

| Score | Action |
|---|---|
| ≥ 0.8 | **Assert** — enters the graph as a regular fact |
| 0.5–0.8 | **Surface** — stored, flagged uncertain, shown for confirmation |
| < 0.5 | **Ignore** — not stored |

Aggregation: several sources asserting the same fact multiply (capped at 1.0); a derived fact inherits the *minimum* of its sources; human confirmation sets 1.0; human rejection sets 0.0 and retracts.

## Entity taxonomy

**Service entities** — the root of every FQN: `DatabaseService`, `DashboardService`, `MessagingService`, `PipelineService`, `MlModelService`, `StorageService`, `ApiService`.

**Data entities**

| Entity | Parent | Notes |
|---|---|---|
| `Database` | `DatabaseService` | |
| `DatabaseSchema` | `Database` | |
| `Table` | `DatabaseSchema` | **(built)** — parentless today; Epic 2 attaches it |
| `Column` | `Table` | Ordered child collection, not a standalone entity |
| `Dashboard` → `Chart` | `DashboardService` | Epic 34 |
| `Topic` | `MessagingService` | Message schema is column-analogous |
| `Pipeline` → `Task` | `PipelineService` | Tasks form a DAG |
| `MlModel` → `Feature` | `MlModelService` | Features reference source columns |
| `Container` | `StorageService` | Nests |
| `ApiEndpoint` | `ApiService` | Epic 34 |

**Semantic entities** — `Glossary`, `GlossaryTerm`, `Classification`, `Tag`, `Metric`, `Taxonomy`.

`Metric` is **first-class**, not a chart attribute: definition, formula, owner, and lineage to source assets. "Which certified revenue metric should I use" is unanswerable if metrics live only inside dashboards.

**Governance entities** — `User`, `Team`, `Domain`, `DataProduct`, `Policy`, `Role`, `Contract`, `Certification`.

**Operational entities** — `TestCase`, `TestResult`, `Incident`, `Alert`, `ConnectorRun`, `UsageRecord`.

`Incident` is a first-class entity because memory links to incidents; without it that reference dangles.

**Knowledge entities** — `Memory`, `Document`, `Conversation`, `Thread`, `Proposal`, `Announcement`.

### Columns are a child collection, not entities

Standalone columns cost a join on every table read and buy no capability the catalog needs. Consequence: a column cannot be independently soft-deleted, and column removal is a table-level Major bump. Revisit only if column-level ownership is required.

## Ownership

`owners` is a list of `EntityReference` pointing at `User` or `Team`. Single-owner models fail immediately — every real asset has a producing team and an accountable individual.

Inheritable: a `Table` with no explicit owner reports its `DatabaseSchema`'s owner, flagged `inherited: true` so the UI distinguishes "nobody set this" from "deliberately owned here". Resolution stops at the first owned ancestor.

## Lifecycle and certification

```rust
enum LifecycleState { Draft, Active, Deprecated, Retired }

struct Certification {
    issuer: EntityReference,
    criteria: String,
    issued_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}
```

Deprecation carries a reason and an optional successor reference, so "use this instead" is machine-readable. **Certification expires** — an unexpiring trust stamp becomes a lie within a year.

## Tags, classification, glossary

Two vocabularies, different jobs. `Classification → Tag` is flat and operational (`PII.Sensitive`, `Tier.Gold`). `Glossary → GlossaryTerm` is hierarchical and semantic, with synonyms, `broader`/`narrower`/`related` relations, and a review workflow.

```rust
struct TagLabel {
    tag_fqn: String,
    source: TagSource,      // Classification | Glossary
    label_type: LabelType,  // Manual | Propagated | Automated | Derived
    state: State,           // Suggested | Confirmed
}
```

`label_type` and `state` exist so an automated scanner can *suggest* `PII.Sensitive` without human confirmation, and the UI can show the difference. Merging automation with curation is the hard part of classification; a model that cannot express provenance forces a rewrite the moment automation arrives.

## Lineage

A specialization of the edge (`feeds` / `derivedFrom`) with a payload:

```rust
struct LineageDetails {
    sql_query: Option<String>,
    column_lineage: Vec<ColumnMapping>,   // many source cols -> one target col
    pipeline: Option<EntityReference>,
    source: LineageSource,                // Manual | QueryParser | Connector | DbtModel
}
```

Edges are keyed by `(from, to, source)` so a connector re-run replaces only the edges *it* asserted, never hand-curated ones. Column lineage is many-to-one (`first_name + last_name → full_name`). Traversal is depth-bounded with cycle detection — real lineage graphs contain cycles, and an unbounded traversal hangs in production, not in tests.

## Entity references

Wherever one entity points at another in a response:

```rust
struct EntityReference {
    id: Uuid, entity_type: String, name: String,
    fully_qualified_name: String, display_name: Option<String>,
    description: Option<String>, deleted: bool,
    inherited: bool,   // relationship comes from a parent
    href: String,
}
```

This denormalized shape makes a detail page renderable from one request instead of N. It is a read-model projection — the authoritative edge is still the row and the reified triple.
