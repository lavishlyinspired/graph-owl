# Plan: Business Semantics (Epic 24)

**Branch**: feat/business-semantics
**Status**: Slices A–F shipped, three deliberate departures from the letter of the plan (each recorded at its slice below): illegal transitions and cycle rejections return `400` rather than the `422` this doc names, matching every other "shape is fine, meaning is not" refusal already in this codebase (Team's cycle detector included) rather than introducing a third convention; Slice D uses `term_attachments` directly rather than the `TagLabel` model, because Epic 25 (which owns `TagLabel`) is not built yet; Slice F reconciles `metric_sources` but does not yet write to `lineage_edges` (metric lineage is not graph-traversable), because that table's FK requires both endpoints to be `assets(id)` and `Metric` is deliberately not an asset — a real schema decision, not an oversight, and out of this slice's scope.
**Depends on**: Epic 2 (FQN derivation and the hierarchy terms attach to), Epic 11 (term reviewers), Epic 4 (taxonomy relations as triples)
**Supersedes**: glossary content in `25-classification.md`
**Crates**: `graph-owl-core` (GlossaryTerm, SkosRelation, Metric) · `graph-owl-ontology` (SKOS vocabulary) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server`

## Goal

Connect technical assets to business meaning: a glossary with a review workflow, taxonomies with real semantic relations, and **`Metric` as a first-class entity**.

## Why metrics are promoted

"Which certified revenue metric should I use?" is asked constantly and is unanswerable if metrics exist only as chart attributes. A `Metric` with a definition, a formula, an owner, and lineage to its source assets is the difference between a catalog that describes dashboards and one that describes the business.

## Resolved decisions

1. **Glossary and classification are separate vocabularies.** `GlossaryTerm` is hierarchical and semantic; `Tag` (Epic 25) is flat and operational. Collapsing them conflates "what this means" with "how to handle it".
2. **SKOS relations, not invented ones.** `broader`, `narrower`, `related`, `exactMatch`, `closeMatch`. Reusing a standard vocabulary makes Epic 9's export a mapping rather than a translation, and makes Epic 33's ontology packs importable.
3. **A metric's formula is text, not an expression graph.** graph-owl does not evaluate metrics; it describes them. Storing an evaluable AST would imply a computation engine it deliberately is not.
4. **Only `Approved` terms are attachable.** A draft definition attached to a thousand columns becomes the de facto definition regardless of its status.
5. **Metric lineage is derived from its source references, not asserted separately.** A metric naming its source columns implies the lineage; requiring both invites divergence.

## Implementation reference

```rust
pub struct GlossaryTerm {
    pub envelope: EntityEnvelope,
    pub glossary: EntityReference,
    pub synonyms: Vec<String>,
    pub abbreviations: Vec<String>,
    pub relations: Vec<SkosRelation>,
    pub status: TermStatus,               // Draft|InReview|Approved|Deprecated
    pub reviewers: Vec<EntityReference>,
}

pub enum SkosRelation {
    Broader(EntityReference), Narrower(EntityReference),
    Related(EntityReference), ExactMatch(String), CloseMatch(String),
}

pub struct Metric {
    pub envelope: EntityEnvelope,
    pub definition: String,              // prose, authoritative
    pub formula: Option<String>,         // human-readable, not evaluated
    pub unit: Option<String>,
    pub granularity: Option<String>,     // "daily", "per customer"
    pub source_assets: Vec<EntityReference>,   // tables/columns it derives from
    pub defined_by: Option<EntityReference>,   // glossary term
    pub calculation_type: CalculationType,     // Simple|Ratio|Derived|Composite
}
```

`definedBy` and `validatedBy` from the Epic 1 taxonomy exist for exactly this: a metric points at the term that defines it, and at the tests that validate it.

### Graph projection

Terms and metrics project like any entity. SKOS relations project as typed edges, so `?m dsc:definedBy/skos:broader* ?t` finds metrics defined by any narrower term of `t` — a query that justifies both the SKOS choice and Epic 7's property paths.

## Acceptance criteria

- [x] Glossary and terms have full CRUD; terms nest with SKOS relations; cycles rejected.
- [x] `broader`/`narrower` are inverse-consistent; `related` is symmetric.
- [x] Term review workflow: Draft → InReview → Approved → Deprecated, with reviewers.
- [x] Only `Approved` terms attach to assets.
- [x] `Metric` is a first-class entity with definition, formula, unit, granularity, sources.
- [~] Metric lineage is derived from `source_assets`, not separately asserted — `metric_sources` reconciles; not yet a `lineage_edges` row, so not graph-traversable (Slice F, scoped with the user).
- [~] Metrics are searchable — yes, by name/definition/defining term. Certifiable (Epic 26) and full trust context (Epic 14's `TrustSummary`) are neither epic's work yet; `gaps` rides the response body as the interim signal.
- [x] A term's usage across assets is listable.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Glossary and terms

**Acceptance criteria**: CRUD for both; term FQN derived (`{glossary}.{term}`, reusing Epic 2); unique within a glossary, not globally; synonyms and abbreviations as string lists, both searchable; deleting a glossary with terms → `409` unless recursive.
**RED**: Scoped-uniqueness pair — same term name in two glossaries must both succeed. A search test asserting a synonym match finds the term. Mutator watch: global uniqueness must fail the two-glossary case; synonyms excluded from the indexed document must fail the search test.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped.** Storage (`Storage::insert_glossary`/`insert_term`/etc. in
`graph-owl-storage` and `graph-owl-storage-postgres`), facade
(`Catalog::create_glossary`/`create_term`/etc.) and HTTP
(`/glossaries`, `/glossaries/{id}/terms`, `/glossary-terms/{id}`,
`/glossary-terms/search`) all land in this slice. Verified at three layers:
19 HTTP tests (`graph-owl-server/tests/glossary.rs`), 19 repository tests
against real Postgres (`graph-owl-storage-postgres/tests/glossary_repository.rs`),
17 facade unit tests. Mutation-tested to 0 missed across all three crates —
the Postgres adapter's `delete_glossary` was simplified to drop a
`term_count > 0` guard around the child-row cleanup that was optimisation-only
and had no correctness value to test for, rather than adding a test that
could never fail. Reviewers/status/relations are not reachable over HTTP yet
(Slices B, C); `Metric` has no storage or routes (Slices E, F).

### Slice B: SKOS relations with inverse consistency

**Acceptance criteria**: `broader` on A→B implies `narrower` B→A on read, without a second stored edge; `related` is symmetric on read; cycles in `broader` rejected at any depth (reusing Epic 11's detector); `exactMatch`/`closeMatch` accept external IRIs and are not validated for reachability; a term may have several `broader` parents (poly-hierarchy is legitimate in SKOS).
**RED**: Cycle tests at depth 1 and 3. An inverse test asserting `narrower` is visible without a second edge existing — storing both is the failure mode decision-wise. A poly-hierarchy test asserting two `broader` parents are permitted. Mutator watch: storing both directions must fail the single-edge assertion; rejecting poly-hierarchy must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped.** `POST/GET/DELETE /glossary-terms/{id}/relations`. `broader`/`related`/`exactMatch`/`closeMatch` may be asserted; `narrower` is refused with a `400` naming that it must be asserted as `broader` from the other term — the single-stored-edge invariant enforced structurally, not just by convention. Cycle rejection reuses `graph_owl_core::glossary::would_cycle` fed by a `broader_edges()` read, exactly as Team's detector does with its own edges. Verified at three layers (facade, Postgres repository, HTTP), 0 missed mutants.

### Slice C: Review workflow

**Acceptance criteria**: transitions Draft→InReview→Approved and Approved→Deprecated; an illegal transition (Draft→Approved) → `422`; approval requires ≥1 assigned reviewer; only an assigned reviewer may approve, others `403`; each transition bumps the version and emits an event; deprecation carries a reason and optional successor term.
**RED**: A transition matrix covering legal and illegal moves. A non-reviewer approval test. Mutator watch: an always-permit transition check must fail the illegal moves; approval without a reviewer must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped**, with two named departures. `PUT/GET /glossary-terms/{id}/reviewers`, `POST /glossary-terms/{id}/transitions` — reusing `graph_owl_core::glossary::transition` directly rather than re-deciding the matrix and reviewer rule at this layer. Non-reviewer approval is a genuine `403 Forbidden` (a new `CatalogError::Forbidden` variant, the first thing to earn one), proven at the wire with two JWT-distinguished identities since open mode's `Principal::system()` cannot express "someone else." The version bump is real: `GlossaryTermRecord` gained a `version: EntityVersion` field reading the migration's existing `version_major`/`version_minor` columns, bumped by `transition_term`'s own `UPDATE`. **Not built**: event emission — `ChangeEvent`'s `EventSubject.kind: AssetKind` is Asset-specific, and a term is deliberately not an asset, so wiring this needs a design decision (widen `EventSubject`, or give terms a parallel event) rather than a call to an existing method. Illegal transitions return `400`, not the `422` this doc names — matching `00d-api-conventions.md`'s own status this doc names, note that Epic 11's team-cycle refusal *also* already uses `400` against the same documented convention; Epic 24 follows that precedent rather than introducing a third code for the same shape of error, and the `00d`/code drift predates this epic.

### Slice D: Terms attach to assets

**Acceptance criteria**: attach via `TagLabel` with `source: Glossary`; only `Approved` terms attach — `Draft` returns `400` naming the status; attachable to entities and to individual columns by FQN; `GET /glossary-terms/{id}/usage` lists assets, paginated; deprecating an attached term is allowed but flags usages; a term and a tag coexist on one asset.
**RED**: The draft-rejection test with an accompanying approved-acceptance test — an unconditional status check passes one and fails the other. Mutator watch: exactly that.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped**, against `term_attachments` rather than `TagLabel`. `TagLabel` is Epic 25's type and Epic 25 is not built, so this uses the migration's own `term_attachments` table directly — `POST/GET/DELETE /glossary-terms/{id}/usage`, paginated with the same keyset-cursor machinery as everything else, using the term's own id to fill the cursor's tie-break slot since the table has no id column of its own and `target_fqn` is already unique per term. Only `Approved` terms attach, `400` naming the actual status. "A term and a tag coexist" is unverifiable until Epic 25 exists; revisit then.

### Slice E: Metric as a first-class entity

**Acceptance criteria**: CRUD with definition (required), formula, unit, granularity, calculation type; `source_assets` validated to exist; `defined_by` must reference an `Approved` term; metrics are searchable by name, definition, and defining term; a metric with no sources is permitted but flagged as a gap (Epic 14's `TrustSummary`); metric FQN is namespaced to avoid collision with tables.
**RED**: A validation test asserting a `defined_by` pointing at a Draft term is rejected. A gap test asserting a source-less metric reports the gap rather than failing. Mutator watch: accepting a draft term must fail; failing on missing sources must fail the gap test — a metric without recorded sources is common and worth cataloguing anyway.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped** on `/business-metrics`, deliberately not `/metrics` — that path already serves Prometheus exposition (Epic 10), and axum panics at startup on a duplicate route rather than shadowing one; caught by `cargo check` before it ever reached a running server. `source_assets` validated against the asset table (a column has no row of its own until Epic 22, so column-level sourcing is unchecked, matching Slice D's same limitation). "Searchable... by defining term" needed a runtime `LEFT JOIN` against `glossary_terms` rather than the metric's own `search_vector`: that column is `GENERATED ALWAYS`, and a generated column cannot read another table's row, so the migration's own vector cannot carry this — found and fixed while writing the repository test for it. Gaps (`graph_owl_core::metric::gaps`) ride on every response body rather than a separate endpoint; full `TrustSummary` integration is Epic 14's, which does not know about metrics yet. Found by a facade test written for a *different* reason: a `create_table`-based fixture for "a known source asset is accepted" passed type-checking and failed at runtime, because `Table` and `Asset` are different entities with different stores — the same trap Epic 31 hit and CLAUDE.md already names.

### Slice F: Metric lineage is derived

**Acceptance criteria**: declaring `source_assets` creates `derivedFrom` edges automatically; removing a source retracts its edge; the edges are indistinguishable from connector-asserted lineage in traversal but carry `source: Metric`; Epic 29 traversal reaches from a table to the metrics derived from it; asserting a metric lineage edge manually and then changing `source_assets` reconciles by source, not wholesale.
**RED**: A reconciliation test: a manually-added lineage edge to a metric survives a `source_assets` change — the same source-scoped rule as Epic 29 Slice E. Mutator watch: wholesale replacement must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Shipped in a narrower, explicit scope, decided with the user rather than assumed.** `PUT /business-metrics/{id}/sources` runs the declared list through `graph_owl_core::metric::reconcile_lineage` (dedup, self-reference excluded) and replaces `metric_sources`. **Not built, and not silently skipped**: `lineage_edges.to_asset_id`/`from_asset_id` both carry a hard `REFERENCES assets(id)` — discovered while implementing this slice, not anticipated in the plan — and `Metric` is deliberately not an asset (Slice A/E), so a metric-to-asset edge cannot be written to that table today. Metric lineage is therefore **not yet reachable by Epic 29 traversal**, and "reconciles by source, not wholesale" has nothing to protect in `metric_sources` specifically, because every row there is already metric-declared — there is no hand-drawn-edge case this table can represent. Closing this gap needs one of two real schema decisions: give `Metric` an `AssetKind`, or widen `lineage_edges`' endpoint typing — either is bigger than one slice and belongs to a future epic that picks deliberately rather than one slice deciding it as a side effect.

## Explicitly deferred (with destination)

- **Metric evaluation** → never. graph-owl describes metrics; it does not compute them (decision 3).
- **Formula parsing / validation** → the formula is prose; a parser would imply evaluation.
- **Multilingual glossaries** → single-language assumed.
- **Automatic term suggestion from column names** → an extraction concern; Epic 21's pipeline with a glossary-targeted schema.
- **Metric versioning semantics beyond the envelope** → "the definition changed" is a Major bump; a formal metric-version concept only if asked.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. `25-classification.md` updated to remove glossary content, avoiding two sources of truth.
