# Plan: Tags & Classification (Epic 25)
**Branch**: feat/classification
**Status**: Slices A–D, H, I shipped. Slices E–G were **delivered by Epic 24** — see below.
**Depends on**: Epic 3 (envelope carries `tags`), Epic 11 (term reviewers are users)
**Unblocks**: Epic 8 (tag facets), Epic 13 (tag-conditioned policies)
**Crates**: `graph-owl-core` (Classification, Tag, TagLabel) · `graph-owl-storage-postgres` · `graph-owl-api` · `graph-owl-server`

## Goal

Turn a column named `cust_ssn` into one labelled `PII.Sensitive` and linked to a business definition. This is what makes the catalog a governance tool rather than a directory.

## Resolved decisions

1. **Two vocabularies, different jobs.** `Classification → Tag` is flat and operational (`PII.Sensitive`, `Tier.Gold`). `Glossary → GlossaryTerm` is hierarchical and semantic, with synonyms and a review workflow. Collapsing them into one tree conflates "how do I handle this" with "what does this mean".
2. **`TagLabel` carries `label_type` and `state` from day one.** An automated scanner must be able to *suggest* `PII.Sensitive` without a human having confirmed it, and the UI must show the difference. Merging automation with human curation is the hard part of classification, and a model that cannot express provenance forces a rewrite the moment automation arrives.
3. **Tag propagation from table to columns is not automatic.** Propagation rules that surprise stewards are worse than no propagation. An explicit propagate action ships instead.
4. **Mutually exclusive classifications are enforced.** A `Tier` classification marked exclusive rejects a second `Tier.*` tag on the same entity — otherwise "Tier" means nothing.
5. **Deleting a Tag in use is rejected**, with a usage count. Silent removal of a governance label is a compliance hazard.

## What Epic 24 already delivered, and why three slices are not repeated here

**Slices E, F and G — glossaries, terms, attachment and the review workflow —
shipped as Epic 24.** `V20__glossary_and_metrics.sql` carries `glossaries`,
`glossary_terms`, `term_relations`, `term_reviewers`, `term_transitions` and
`term_attachments`; `graph_owl_core::glossary` carries the status machine
(`can_transition`, `is_attachable`), the SKOS relations with their inverses, and
`would_cycle`. Rebuilding any of it here would have produced a second glossary
that disagreed with the first.

What this epic adds is the **operational** half that decision 1 keeps separate.

## Acceptance criteria (feature level)

- [x] `Classification` and `Tag` CRUD with the envelope; tag FQN is `{classification}.{tag}`, derived.
- [x] `Glossary` and `GlossaryTerm` CRUD; terms nest; cycles rejected — **Epic 24**.
- [x] Tags attach to entities and to individual columns.
- [x] A suggested tag is distinguishable from a confirmed one and can be confirmed or rejected.
- [x] Mutually exclusive classifications reject a second tag from the same classification, naming it.
- [x] Deleting a tag in use → `409` with a usage count **by entity kind**.
- [x] A term moves through `Draft → InReview → Approved` with a reviewer — **Epic 24**.
- [ ] `GET /tables?tags=PII.Sensitive` filters, including column-level matches. **Not done** — see "Explicitly deferred".

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Classifications and tags exist — **shipped**

**Value**: A steward defines the operational vocabulary before anything is labelled.
**Path**: `Classification { name, description, mutually_exclusive: bool }`; `Tag { name, description, classification }`, FQN `{classification}.{tag}`.
**Acceptance criteria**: CRUD for both; tag FQN derived (reusing Epic 2's derivation); duplicate tag under the same classification → `409`, under different classifications → both succeed; `GET /classifications/{id}/tags` paginated; deleting a classification with tags → `409` unless `?recursive=true`.
**RED**: Repository and HTTP tests per operation, plus the scoped-uniqueness pair. Mutator watch: globally-scoped tag uniqueness must fail the different-classifications case.
**GREEN**: entities, storage, derivation reuse, handlers.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Tags attach to entities with provenance — **shipped**

**Value**: A table can be labelled, and the label says where it came from.
**Path**: `TagLabel { tag_fqn, source, label_type, state }` on the envelope, backed by `appliedTo` edges carrying the label metadata.
**Acceptance criteria**:
- Apply one or many tags on create and PATCH.
- Nonexistent tag FQN → `400` naming it.
- `labelType` ∈ `Manual|Propagated|Automated|Derived`; `state` ∈ `Suggested|Confirmed`; manual application defaults to `Manual`/`Confirmed`.
- Applying the same tag twice is idempotent — one label, not two.
- Applying a tag from a mutually-exclusive classification when another from it is present → `409` naming the conflicting tag.
- Tag change bumps the version Minor with a `changeDescription` entry.
**RED**: Exclusivity test asserting the `409` names the existing conflicting tag. Idempotency test asserting one label after two applications. Mutator watch: exclusivity checked against the wrong classification, or not at all.
**GREEN**: label type, edges with metadata, exclusivity check.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Columns are taggable — **shipped**

**Value**: `PII` belongs on the SSN column, not on the whole table — table-level PII labelling is too coarse to act on.
**Path**: labels attach to a column by its FQN (`{tableFqn}.{columnName}`, from Epic 2).
**Acceptance criteria**:
- Tag a single column; visible on the parent table's column list.
- Nonexistent column name → `400`.
- Column tags survive a table PATCH that reorders columns (matched by name, consistent with Epic 2 Slice G).
- Renaming a column carries its tags.
- Removing a column removes its labels.
**RED**: Reorder test asserting the tag stays on the right column, not the right position. Mutator watch: position-based rather than name-based matching must fail it.
**GREEN**: column-FQN-keyed labels, rename/removal handling.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Suggested tags can be confirmed or rejected — **shipped**

**Value**: An automated scanner (later) becomes usable — a human triages suggestions rather than trusting or ignoring them wholesale.
**Path**: `PUT /{collection}/{id}/tags/{tagFqn}/confirm` and `.../reject`.
**Acceptance criteria**:
- Confirm flips `Suggested → Confirmed` and records the confirming principal.
- Reject removes the label and records the rejection so the same suggestion is not re-proposed.
- Confirming an already-confirmed label → `409`.
- Confirming a nonexistent label → `404`.
- `?state=suggested` filters lists to entities with pending suggestions — the steward triage queue.
**RED**: Test asserting a rejected suggestion is not re-created by a subsequent automated application of the same tag. Mutator watch: rejection that merely deletes without recording must fail it.
**GREEN**: state transitions, rejection ledger, filter.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Glossaries and terms exist — **delivered by Epic 24**

**Value**: A business definition has a home, so "active customer" means one thing across the organization.
**Path**: `Glossary { name, description }`; `GlossaryTerm { name, description, synonyms, glossary, parent? }`; nesting via `parentOf`; `relatedTo` between terms.
**Acceptance criteria**: CRUD for both; terms nest arbitrarily deep with derived FQNs; cycles → `422` at any depth (reusing Epic 2's cycle detector); synonyms are a string list; `relatedTo` is symmetric — creating A→B makes B→A visible; deleting a term with children → `409` unless recursive.
**RED**: Cycle tests at depth 1 and 3. Symmetry test asserting B lists A without a second edge being created. Mutator watch: asymmetric `relatedTo` must fail.
**GREEN**: entities, nesting, symmetric relation handling.
**REFACTOR**: this is the second consumer of cycle detection (after Epic 11) — extract to core now if not already done.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Terms attach to assets — **delivered by Epic 24**

**Value**: A column links to the definition of what it holds — the semantic half of classification.
**Path**: `TagLabel` with `source: Glossary` and the term's FQN.
**Acceptance criteria**: attach terms to entities and columns; a term and a tag can coexist on one entity; `GET /glossary-terms/{id}/usage` lists assets using the term, paginated; only `Approved` terms are attachable — `Draft` → `400`.
**RED**: Test asserting a `Draft` term cannot be attached and an `Approved` one can. Mutator watch: an unconditional status check must fail one of the two.
**GREEN**: glossary-sourced labels, usage endpoint, status gate.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Terms have a review workflow — **delivered by Epic 24**

**Value**: Business definitions get an owner and an approval before they become authoritative.
**Path**: `status: Draft|InReview|Approved|Deprecated`; `reviews` edges to users; transition endpoints.
**Acceptance criteria**:
- `Draft → InReview → Approved`; `Approved → Deprecated`.
- Illegal transition (`Draft → Approved`) → `422`.
- Approval requires at least one reviewer assigned.
- Only an assigned reviewer may approve — anyone else → `403`.
- Deprecating a term in use → allowed, but the usage list flags it.
- Each transition bumps the version and emits a change event.
**RED**: Table-driven transition matrix covering legal and illegal moves. Test asserting a non-reviewer approval is rejected. Mutator watch: an always-permit transition check must fail the illegal moves.
**GREEN**: status machine, reviewer edges, authorization check (principal-based, pre-Epic-11).
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice H: Tags in use cannot vanish silently — **shipped**

**Value**: A governance label cannot disappear from a thousand columns by accident.
**Path**: pre-delete usage check on `Tag` and `GlossaryTerm`.
**Acceptance criteria**: deleting a tag applied anywhere → `409` with counts by entity type; `?force=true` removes the tag and all its labels transactionally, bumping each affected entity's version; deleting an unused tag succeeds; soft-deleted entities do not count toward usage.
**RED**: Test asserting the `409` body carries per-type counts. Force test asserting every label is gone *and* every affected entity's version advanced. Mutator watch: non-transactional force-removal; counting soft-deleted entities.
**GREEN**: usage query, transactional force path.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice I: Tags propagate on request — **shipped**

**Value**: Labelling a table's forty columns is one action, without surprising anyone with automatic behavior.
**Path**: `POST /{collection}/{id}/tags/{tagFqn}/propagate` applying to children with `labelType: Propagated`.
**Acceptance criteria**:
- Propagating a table tag applies it to every column as `Propagated`.
- An existing `Manual` label on a child is not downgraded.
- Response reports how many children were affected.
- Propagation is one level unless `?recursive=true`.
- Propagated labels are removable independently.
- Removing the parent tag does not auto-remove propagated children — they are independent once created.
**RED**: Test asserting a manually-tagged column keeps `Manual` after propagation. Mutator watch: blanket overwrite must fail it.
**GREEN**: propagation with precedence rules.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **`?tags=` filtering on list endpoints, including column-level matches** →
  the one feature-level criterion this epic did not deliver. The labels, the
  usage query and the index all exist; what is missing is the filter itself, and
  the reason it is not a line of code is the **column-level** half: matching a
  table because one of its *columns* carries `PII.Sensitive` is a different
  query from matching the table's own label, and shipping only the first would
  quietly under-report exactly the case the epic exists for (`PII` belongs on
  the SSN column). Epic 23's `?domain=` established the filter plumbing; this
  needs its own slice and its own test for the descendant case.
- **Automated PII detection** → a `labelType: Automated` producer. The model supports it; nothing emits it. Add when a scanner exists.
- **Tag-based access policies** → Epic 13, as a policy condition.
- **Bulk tagging via CSV** → alongside Epic 20's import/export.
- **Multilingual glossaries** → not planned; single-language assumed. Named so the assumption is visible.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
