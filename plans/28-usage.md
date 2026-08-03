# Plan: Usage & Popularity (Epic 28)

**Branch**: feat/usage
**Status**: Slices A–E shipped; Slice F (discovery) deferred with a reason
**Depends on**: Epic 16 (push ingestion), Epic 11 (consumers are principals)
**Crates**: `graph-owl-core` (UsageObservation, rollup, trend — pure) · `graph-owl-storage-postgres` (time-series + rollup tables) · `graph-owl-search` (popularity ranking term) · `graph-owl-api` · `graph-owl-server`

## Goal

Record what is actually used, by whom, and how often — so recommendations reflect reality rather than mere existence.

## Why an agent needs this

Recommending a technically-matching but abandoned table is worse than returning nothing: it looks like an answer. Usage is how "trusted" becomes operational — a table twelve teams query daily is a different proposition from one last read eight months ago, and no amount of metadata distinguishes them.

## Resolved decisions

1. **Usage is a time series, aggregated on read.** Storing pre-computed popularity would go stale silently. Raw observations plus rollups.
2. **Query text is optional and off by default.** Query bodies contain literals — customer identifiers, filter values. Ingesting them is a data-protection decision, not a default.
3. **Consumers are resolved to principals where possible, retained as opaque strings where not.** A warehouse username that maps to no `User` is still useful as a distinct-consumer count.
4. **Raw observations are pruned; rollups are retained.** Per-query rows at warehouse scale are enormous. Daily rollups per (asset, consumer) survive; raw rows expire.
5. **graph-owl does not read query logs itself.** They are pushed (Epic 16) or streamed (Epic 19). Reaching into a warehouse's log tables would need credentials and a connector per engine.

## Implementation reference

```rust
pub struct UsageObservation {
    pub asset: EntityReference,
    pub consumer: Consumer,
    pub operation: UsageOperation,           // Read|Write|Delete|SchemaRead
    pub occurred_at: DateTime<Utc>,
    pub row_count: Option<u64>,
    pub duration_ms: Option<u64>,
    pub query_id: Option<String>,            // engine's id, not the text
    pub query_text: Option<String>,          // opt-in only
}

pub enum Consumer {
    Principal(EntityReference),              // resolved user/team/service
    Opaque { identifier: String, kind: String },  // unresolved warehouse user
}

pub struct UsageRollup {                     // daily, per (asset, consumer, operation)
    pub asset: Uuid, pub consumer_key: String,
    pub day: NaiveDate, pub operation: UsageOperation,
    pub count: u64, pub total_rows: Option<u64>,
}

pub struct PopularitySummary {               // computed on read
    pub queries_last_7d: u64,
    pub queries_last_30d: u64,
    pub distinct_consumers_30d: u64,
    pub last_accessed: Option<DateTime<Utc>>,
    pub trend: Trend,                        // Rising|Stable|Declining|Dormant
}
```

### Trend

Compares the last 7 days against the previous 7, with a minimum-volume floor so a table queried twice does not register as "Rising 100%". `Dormant` is no access in 90 days — the signal that most changes a recommendation.

## Acceptance criteria

- [x] Observations ingest in batches via the push path.
- [x] Rollups are computed incrementally on ingest, not by re-scanning raw rows.
- [x] Popularity is computed on read from rollups — never stored, for the reason decision 1 gives.
- [x] Consumers stay opaque when nothing matches, and resolution is retroactive.
- [x] Query text is only stored when explicitly enabled, and is **dropped at the boundary** when it is not.
- [x] Raw observations prune on a retention schedule; rollups survive, and so does `last_accessed`.
- [ ] Usage feeds search ranking (Epic 8) and `TrustSummary` (Epic 14) — Slice F, deferred; see below.
- [~] Producer and consumer lists are derivable per asset — the rollups carry consumer keys, so the data is there; no endpoint presents them as a list yet.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Ingest observations — **shipped**

**Acceptance criteria**: batch ingest via Epic 16's path; asset resolved by FQN, unknown FQN recorded as an unmatched observation rather than rejected (the asset may not be catalogued yet); `occurred_at` in the future → rejected; duplicate `(asset, query_id)` → ignored; ingest does not bump the asset's version or emit change events; throughput sufficient for a realistic daily volume.
**RED**: A test asserting the asset's version is unchanged after ingesting a thousand observations — usage is not a metadata change. An unmatched-FQN test asserting the observation is retained for later reconciliation rather than dropped. Mutator watch: version bumping must fail the first; rejecting unknown assets must fail the second, which would discard usage for anything not yet catalogued.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Rollups — **shipped**

**Acceptance criteria**: daily rollups per (asset, consumer, operation) updated incrementally on ingest; a late-arriving observation for a past day updates that day's rollup; rollup computation is idempotent; rollups are queryable directly; a rollup rebuild from raw rows produces identical results to incremental accumulation.
**RED**: The rebuild-equivalence test: accumulate incrementally, then rebuild from raw, assert identical — the only way to know the incremental path is correct. A late-arrival test. Mutator watch: an incremental path that drifts from the rebuild must fail the equivalence test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Popularity and trend — **shipped**

**Acceptance criteria**: `PopularitySummary` computed on read from rollups; `?fields=popularity` opts in; trend uses a minimum-volume floor so tiny counts do not produce extreme percentages; `Dormant` after 90 days with no access; an asset with no observations reports zeros and `Unknown` trend, never `Dormant` (absence of data is not absence of use).
**RED**: The floor test: 1 query last week vs 2 this week must not report "Rising 100%". The no-data test asserting `Unknown` rather than `Dormant` — claiming an asset is unused when nothing was ever ingested is a false negative that would get assets wrongly retired. Mutator watch: no floor must fail the first; defaulting to `Dormant` must fail the second.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Consumer resolution and privacy — **shipped**

**Acceptance criteria**: a warehouse identifier matching a `User` email or name resolves to that principal; unresolved identifiers stay `Opaque` and still count toward distinct consumers; resolution is retroactive — creating a matching `User` later resolves historical observations; `query_text` stored only when enabled per source; when disabled, ingested query text is dropped at the boundary and never persisted; consumer identity is omitted from responses to principals lacking permission to see it.
**RED**: A test asserting ingested `query_text` is absent from storage when the flag is off — dropping it at the boundary, not filtering it on read, is the difference between not storing data and storing then hiding it. A retroactive-resolution test. Mutator watch: persisting-then-hiding must fail the storage assertion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Retention — **shipped**

**Acceptance criteria**: raw observations pruned after a configurable window (default 90 days); rollups retained indefinitely; pruning is incremental and does not lock; the most recent observation per asset is always retained so `last_accessed` survives pruning; pruning is observable via metrics.
**RED**: The last-observation-survives test — pruning `last_accessed` out of existence would blank the single most useful signal. Mutator watch: unconditional age-based deletion must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Usage informs discovery — **deferred, with the sharp half named**

The data is all here: `GET /usage/{fqn}` returns the summary and the trend. What
is not built is the *ranking* change, and the reason it is not a small addition
is this slice's own RED test — **ranking with the weight at zero must reproduce
the prior ordering exactly**. That is a property of Epic 8's ranking formula, not
of this epic's data, and proving it needs a before/after comparison over a real
corpus rather than an assertion about a number. Adding a popularity term without
that test is how a ranking change ships that nobody can turn off.

The dormant-marking half is cheaper and is worth doing with it, for the same
reason Epic 26's deprecated marker was: filtering hides reality, unmarking
misleads.


**Acceptance criteria**: search ranking accepts a popularity term with a configurable weight (Epic 8's ranking formula); `TrustSummary` carries popularity and trend; `?sort=popularity` on list endpoints; a dormant asset is visibly marked in search results, not filtered out; ranking with the weight at zero reproduces pre-usage ordering exactly.
**RED**: The zero-weight test asserting exact reproduction of prior ordering — proves the term is additive and disableable rather than entangled. A dormant-marking test. Mutator watch: a non-zeroable weight must fail; filtering dormant assets must fail the marking test.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Reading query logs directly from engines** → push or stream only (decision 5).
- **Lineage inference from query text** → needs SQL parsing, deliberately off the roadmap.
- **Cost attribution** (query spend per asset) → a FinOps surface; the duration and row-count fields keep it possible.
- **Per-column usage** → asset-level for now; column-level needs query parsing.
- **User-level recommendation personalization** → the data supports it; the ranking change needs evidence it beats popularity alone.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. Rollup rebuild-equivalence verified (Slice B).
5. Verify query text is never persisted when disabled (Slice D) — a data-protection assertion, not a feature test.
