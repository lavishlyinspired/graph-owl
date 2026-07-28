# Plan: Vector & Hybrid Search (Epic 8) ★
**Branch**: feat/search
**Status**: **In progress** — facets and discovery shipped (Demo 2). BM25 + HNSW + RRF fusion specified here, not yet built
**Depends on**: Epic 3 (change events to subscribe to), Epic 2 (FQNs to rank on), Epic 25 (tags, for facets — soft: search ships before them and gains the facet later)
**Unblocks**: Epic 34 (each new entity type indexes for free)
**Crates**: **`graph-owl-search`** (new — VectorIndex/TextIndex ports) · **`graph-owl-search-hnsw`** (new — in-process adapter) · **`graph-owl-search-opensearch`** (new, deferred) · `graph-owl-api` · `graph-owl-server`

## Goal

Let a consumer find an asset they did not know existed. Until this ships, the catalog only answers questions you already knew how to ask.

## Resolved decisions

1. **The index is a derived, eventually-consistent store. Postgres is authoritative.** Search results carry only enough to render a hit; clicking through reads from Postgres. Any divergence is repaired by reindexing, never by writing to the index directly. This one rule prevents the entire class of "the index and the database disagree and nobody knows which is right" failures.
2. **Indexing is driven by Epic 3's change events**, not by polling and not by dual-writes inside the request path. A failed index write must never fail a catalog write.
3. **OpenSearch as the first adapter**, behind a `SearchIndex` port. The port keeps the facade honest and makes a different engine additive.
4. **One index per entity type, queried through an alias.** Alias swap gives zero-downtime mapping changes — the operation you need most and can least afford to take downtime for.
5. **Relevance ordering is explicit and tested**: exact FQN > exact name > name prefix > name fuzzy > description > column names > tags.
6. **Lexical search only.** Semantic/vector search is a real later option, but lexical relevance must be good before embeddings are worth their operational cost.
7. **graph-owl stores and searches vectors; it never produces them.** Embedding *generation* is model inference and lives out of process — a hosted API or a Python worker — feeding vectors in through the ingestion path. Loading a model into the binary would forfeit the `00a-product-position.md` footprint budget for a workload that is not on the read path, and would pin the deployment to one model's runtime. The index is in-process because *searching* a vector is on the read path; generating one is not. See `00j-language-boundaries.md`.

## Hybrid ranking

Lexical (BM25) and vector (HNSW) results are combined by **Reciprocal Rank Fusion**:

```
score(d) = Σ_over_rankers  weight_r / (k + rank_r(d))        k = 60 (standard)
```

RRF over score normalization, deliberately: BM25 and cosine scores are on incomparable scales, and any normalization scheme needs recalibration whenever either ranker changes. RRF uses only rank, so it is stable across corpus and model changes — which matters because the embedding model *will* be swapped.

Exact-FQN matches bypass fusion entirely and rank first (`00a-product-position.md`: exact-match lookup stays exact).

## Acceptance criteria (feature level)

- [ ] Creating a table makes it findable by partial name within one second.
- [ ] Updating a description makes the new text findable and the old text not.
- [ ] Soft-deleting removes it from results; restoring returns it.
- [ ] Results are faceted by entity type, service, tag, and owner.
- [ ] An exact FQN match outranks a description match.
- [ ] Search survives an index outage: catalog writes still succeed, and the backlog is recoverable.
- [ ] A full reindex runs with zero downtime via alias swap.
- [ ] Results are paginated consistently with `plans/00d-api-conventions.md`.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: A table is findable by name

**Value**: The thinnest end-to-end search path — proves port, adapter, indexing, and query together.
**Path**: `graph-owl-search` (port) + `graph-owl-search-opensearch` (adapter); `POST /search?q=` → `SearchIndex::search` → OpenSearch; index written synchronously on create for this slice only.
**Acceptance criteria**:
- Creating a table named `customer_orders` makes it findable by `customer`, `orders`, and `customer_ord`.
- Results carry id, entity type, name, FQN, description snippet, and score.
- No match → empty results, `200`, not `404`.
- Empty `q` → `400`.
- Results paginated per house conventions.
**RED**: Integration test with an OpenSearch testcontainer: create via HTTP, search, assert the hit. Mutator watch: a query matching everything must fail — assert a non-matching term returns zero hits, not just that the target is present.
**GREEN**: port trait, adapter, index mapping for `Table`, search endpoint.
**REFACTOR**: assess the boundary between port and adapter — the port must express *what* is searched, never OpenSearch query DSL. If query DSL leaks into `graph-owl-search`, the abstraction is wrong.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Indexing follows change events

**Value**: The index stays current without the write path depending on it — a search outage stops being a catalog outage.
**Path**: an `EventSink` implementation subscribing to Epic 3's `ChangeEvent` stream, translating each to an index operation, running outside the request path.

**"Incremental indexing" means this, and not the other thing** (clarified 28 July 2026, because an indexing review conflated them):

| | What it means | Status |
|---|---|---|
| **Search index** — this epic | Keeping a *derived* index in step with entity changes without re-indexing the estate. The index lives outside the write transaction, so it can fall behind and must be told what changed | **Real, and the actual blocker is Epic 3's `EventSink`.** Nothing emits `ChangeEvent` yet, so there is nothing to subscribe to |
| **Flake B-tree indexes** — Epic 4 | Batching index maintenance into segments merged later | **Not a thing to build.** Postgres maintains B-trees per row and since 14 does bottom-up index deletion; there is no rebuild to amortise. See `04-engine-triples.md`, "Indexing review" |

The distinction matters because the first is a **consistency** problem — a derived store that can drift from its source — and the second is a **write-amplification** problem that this storage engine does not have. Solving the second would not help the first by even a little.
**Acceptance criteria**:
- Create → indexed. Update → reindexed with new content, old text no longer matching. Soft delete → removed. Restore → re-added. Hard delete → removed.
- Index failure is logged and does **not** fail the catalog write.
- Failed operations land on a retry queue with bounded backoff.
- Indexing is idempotent — replaying an event twice yields one document.
- No dual-write inside the HTTP request path.
**RED**: Test asserting a catalog write succeeds with the index adapter stubbed to fail, *and* that the failure is queued for retry. Idempotency test replaying one event twice. Mutator watch: propagating the index error must fail the first test; a non-idempotent index write must fail the second.
**GREEN**: event-driven indexer, retry queue, idempotent upsert by entity id.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Search spans entity types with facets

**Value**: One search box over the whole catalog, with the counts a user needs to narrow down.
**Path**: index per entity type behind a shared read alias; aggregations for facet counts.
**Acceptance criteria**:
- One query searches tables, schemas, databases, and services.
- `entityType` facet returns counts per type.
- `?entityType=table` filters.
- Facet counts reflect the other active filters, not the unfiltered corpus.
- Multi-select within a facet is OR; across facets is AND.
- Zero-count buckets are omitted.
**RED**: Test with a mixed corpus asserting facet counts change correctly when a second filter is applied — the subtle case. Mutator watch: facets computed against the unfiltered corpus must fail it.
**GREEN**: alias, per-type mappings, aggregations, filter composition.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Relevance is deliberate and tested

**Value**: The right answer is first. A search that returns the right result on page three has failed.
**Path**: field boosts, exact-match sub-fields, and a scoring configuration exercised by tests.
**Acceptance criteria**, given a corpus containing a table named `customers`, another whose description mentions "customers", and a third named `customer_archive`:
- Query `customers` ranks the exact name first.
- Full FQN query ranks that entity first.
- Prefix query `custom` matches all three, exact-name-first.
- A term appearing only in a column name matches, ranked below name matches.
- A tag name match ranks below name, above description.
**RED**: A fixed corpus and a table of (query → expected ordered ids). This is the slice where the test *is* the specification. Mutator watch: uniform boosts must fail the ordering assertions.
**GREEN**: mapping sub-fields (`keyword` + `text` + `edge_ngram`), boosts.
**REFACTOR**: assess extracting the ranking corpus into a reusable fixture — Epic 34 will re-run these assertions for five more entity types.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Reindex runs without downtime

**Value**: A mapping change or index corruption is recoverable without a maintenance window.
**Path**: `POST /admin/search/reindex` → build into a new index → verify → atomically repoint the alias → drop the old.
**Acceptance criteria**:
- Reindex from Postgres as the source of truth.
- Searches continue serving from the old index throughout.
- Alias swap is atomic — no window with zero or two active indices.
- Failure mid-build leaves the old index serving and the partial one cleaned up.
- Progress is reportable.
- Reindex is restartable after a crash.
- Concurrent reindex requests → `409`.
**RED**: Test searching continuously during a reindex, asserting no request fails and none returns an empty result set mid-swap. Mutator watch: a delete-then-create swap must fail this; a non-atomic alias update must fail it.
**GREEN**: builder, verification step, atomic alias action, lock.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Results respect visibility rules

**Value**: Search does not become the hole through which deleted assets leak back.
**Path**: `deleted` in the index; default filter excludes it; `include` honored as elsewhere.
**Acceptance criteria**: soft-deleted excluded by default; `include=deleted` returns only them; `include=all` returns both; facet counts respect the filter; a restored entity reappears within one second.
**RED**: Test asserting a soft-deleted entity is absent by default *and* that facet counts exclude it. Mutator watch: filtering results but not facets must fail the count assertion.
**GREEN**: indexed tombstone flag, filter applied to query and aggregations alike.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: Suggestions are fast

**Value**: Type-ahead makes the catalog feel navigable rather than interrogable.
**Path**: `GET /search/suggest?q=` backed by a completion/edge-ngram field.
**Acceptance criteria**: returns up to 10 suggestions with id, type, name, FQN; p95 under 50 ms on a 100k-document corpus; matches on name and FQN, not description; respects the tombstone filter; fewer than 2 characters → empty, not `400`.
**RED**: A seeded-corpus latency assertion plus content assertions. Mutator watch: falling back to the full search query must fail the latency bound.
**GREEN**: completion mapping, dedicated endpoint.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Semantic / vector search** → after lexical relevance plateaus below expectations.
- **Personalized ranking from usage** → needs query-log ingestion, itself not on the roadmap.
- **Saved searches and alerts** → a product surface, not a search-engine concern.
- **Cross-entity aggregation queries** ("count tables per service") → a reporting concern; the facet counts cover the immediate need.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Search integration tests run against an OpenSearch testcontainer, not a stub.
