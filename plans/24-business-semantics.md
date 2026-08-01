# Plan: Business Semantics (Epic 24)

**Branch**: feat/business-semantics
**Status**: In progress — Slice A shipped (glossary + term CRUD, scoped uniqueness, search); Slices B–F not started
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

- [ ] Glossary and terms have full CRUD; terms nest with SKOS relations; cycles rejected.
- [ ] `broader`/`narrower` are inverse-consistent; `related` is symmetric.
- [ ] Term review workflow: Draft → InReview → Approved → Deprecated, with reviewers.
- [ ] Only `Approved` terms attach to assets.
- [ ] `Metric` is a first-class entity with definition, formula, unit, granularity, sources.
- [ ] Metric lineage is derived from `source_assets`, not separately asserted.
- [ ] Metrics are searchable, certifiable (Epic 26), and returned with trust context.
- [ ] A term's usage across assets is listable.

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

### Slice C: Review workflow

**Acceptance criteria**: transitions Draft→InReview→Approved and Approved→Deprecated; an illegal transition (Draft→Approved) → `422`; approval requires ≥1 assigned reviewer; only an assigned reviewer may approve, others `403`; each transition bumps the version and emits an event; deprecation carries a reason and optional successor term.
**RED**: A transition matrix covering legal and illegal moves. A non-reviewer approval test. Mutator watch: an always-permit transition check must fail the illegal moves; approval without a reviewer must fail.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Terms attach to assets

**Acceptance criteria**: attach via `TagLabel` with `source: Glossary`; only `Approved` terms attach — `Draft` returns `400` naming the status; attachable to entities and to individual columns by FQN; `GET /glossary-terms/{id}/usage` lists assets, paginated; deprecating an attached term is allowed but flags usages; a term and a tag coexist on one asset.
**RED**: The draft-rejection test with an accompanying approved-acceptance test — an unconditional status check passes one and fails the other. Mutator watch: exactly that.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Metric as a first-class entity

**Acceptance criteria**: CRUD with definition (required), formula, unit, granularity, calculation type; `source_assets` validated to exist; `defined_by` must reference an `Approved` term; metrics are searchable by name, definition, and defining term; a metric with no sources is permitted but flagged as a gap (Epic 14's `TrustSummary`); metric FQN is namespaced to avoid collision with tables.
**RED**: A validation test asserting a `defined_by` pointing at a Draft term is rejected. A gap test asserting a source-less metric reports the gap rather than failing. Mutator watch: accepting a draft term must fail; failing on missing sources must fail the gap test — a metric without recorded sources is common and worth cataloguing anyway.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Metric lineage is derived

**Acceptance criteria**: declaring `source_assets` creates `derivedFrom` edges automatically; removing a source retracts its edge; the edges are indistinguishable from connector-asserted lineage in traversal but carry `source: Metric`; Epic 29 traversal reaches from a table to the metrics derived from it; asserting a metric lineage edge manually and then changing `source_assets` reconciles by source, not wholesale.
**RED**: A reconciliation test: a manually-added lineage edge to a metric survives a `source_assets` change — the same source-scoped rule as Epic 29 Slice E. Mutator watch: wholesale replacement must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Metric evaluation** → never. graph-owl describes metrics; it does not compute them (decision 3).
- **Formula parsing / validation** → the formula is prose; a parser would imply evaluation.
- **Multilingual glossaries** → single-language assumed.
- **Automatic term suggestion from column names** → an extraction concern; Epic 21's pipeline with a glossary-targeted schema.
- **Metric versioning semantics beyond the envelope** → "the definition changed" is a Major bump; a formal metric-version concept only if asked.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. `25-classification.md` updated to remove glossary content, avoiding two sources of truth.
