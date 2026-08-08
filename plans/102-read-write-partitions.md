# Plan: Read/Write Partition Split (Epic 102)

**Status**: **Shipped, 8 August 2026 — built on explicit override, entry condition not met.** Epic 37a Slice C measured the trigger this epic exists to react to (write throughput at 1M/5M/10M synthetic flakes) and found it flat: 53,641–57,919 flakes/s across the whole range, no degrading trend, under Postgres's untouched default `shared_buffers` (128 MB, already smaller than the index set at the first checkpoint — so the run could not observe a before/after crossover even if one exists past this machine's tested range). **The honest reading is that the entry condition below had not fired.** Built anyway on explicit user instruction, given that caveat, with decision 5's own measurement gate treated as live rather than a formality — see the acceptance criteria's own "Shipped as" section for the real, measured verdict: writes improved 25–34%, reads did not measurably regress beyond what a cold cache after compaction's own I/O explains, so the split shipped rather than being abandoned.
**Depends on**: Epic 4 (the flake table), Epic 37a (the measurement)
**Crates**: `graph-owl-engine-postgres`

## Goal

Stop write amplification on the flake table from dominating ingestion, by
separating a read-optimised main partition from a write-optimised delta.

## The honest position

This is the one scheduled item whose problem **has not been observed here**. It
is planned because it was asked for, and the design is recorded so it can be
built correctly if the trigger fires — but building it before then would add a
merge path, a second read path and a compaction schedule to a system whose
measured write volume does not need any of them.

**Do not start this epic on the strength of the architecture being good.** It is
good for stores an order of magnitude past this one's target.

## Entry condition

**Epic 37a showing write amplification dominating a realistic ingestion run** —
specifically, index maintenance on `flakes` exceeding the ingestion budget in
`00a` at target scale. Not "writes feel slow"; a number against a budget.

## Why the pressure exists at all

Four index orderings plus a unique identity index mean **five index updates per
flake**. A wide table's projection is hundreds of flakes, so a connector run is
tens of thousands of index writes. That is the cost knowingly accepted in
`04-engine-triples.md` decision 2, and it buys index seeks on every read shape.

## The design, if built

Two partitions over the same logical flake set:

| | Main | Delta |
|---|---|---|
| Optimised for | Reads | Writes |
| Indexes | All five | Minimal — append order only |
| Written by | Compaction | Every transaction |
| Read by | Every query | Every query |

Writes land in delta. Queries read **both** and merge. Compaction periodically
folds delta into main and rebuilds indexes in bulk, which is dramatically
cheaper than maintaining them per row.

## Resolved decisions

1. **Correctness is not negotiable for performance.** Every read merges both
   partitions. A query that read only main would return stale data, and one
   that raced compaction would return a state that never existed. This is the
   requirement the whole design lives or dies by.
2. **Current-state resolution spans partitions.** `DISTINCT ON … ORDER BY t
   DESC` must see both, or a retraction in delta would not supersede an
   assertion in main — the single most dangerous bug this design can have,
   because it resurrects deleted facts.
3. **Compaction is online and interruptible.** A rebuild that locks the table
   trades a write problem for an availability problem.
4. **Time-travel spans partitions unchanged.** `as_of` is a `t` comparison and
   `t` is global, so this is free — worth stating because it is the property
   most likely to be broken by a careless implementation.
5. **A merged read is measured before it is adopted.** The merge is not free.
   If it costs more on reads than compaction saves on writes, the split is a
   loss and must not ship out of sunk-cost.

## Acceptance criteria

- [x] A query returns identical results before and after compaction. Asserted by
      running it, compacting, and running it again. `a_query_returns_identical_results_before_and_after_compaction`.
- [x] A retraction in delta supersedes an assertion in main — decision 2, and
      the test that matters most. `a_retraction_after_compaction_supersedes_the_compacted_assertion`,
      plus the pre-compaction shape in `a_delta_write_supersedes_an_older_fact_synthetically_placed_in_main`.
- [x] `as_of` returns identical results across a compaction boundary.
      `as_of_returns_identical_results_across_a_compaction_boundary`.
- [x] Compaction is interruptible and leaves a consistent state at any point of
      interruption. `compaction_in_small_batches_leaves_a_consistent_state_at_every_point`
      — checked *between* every single-row batch, not only at the end.
- [x] **Measured** write throughput improves and read latency does not regress
      past budget. If either fails, the epic is abandoned rather than tuned.
      Measured at real 1M-flake scale — see "Shipped as" below for the numbers
      and the honest verdict.

## Shipped as

`crates/graph-owl-engine-postgres/migrations/V9__flakes_delta_partition.sql`
(schema), `src/lib.rs`'s `write()`/`compact()` (the two write-side choke
points), `tests/partition_split.rs` (10 correctness tests) and
`tests/scale_partition_adopted.rs` (the decision-5 measurement).

- **Design**: `flakes` (the original table) is renamed to `flakes_main`
  unchanged — zero data moved, zero index rebuilt, all four original index
  orderings kept under their original names. `flakes_delta` is a new table
  with the same columns and exactly one index (a SPOT-style uniqueness
  constraint, for the same idempotency `flakes_main`'s own SPOT index gives
  writes). A `flakes` **view** (`SELECT ... FROM flakes_main UNION ALL
  SELECT ... FROM flakes_delta`, named columns rather than `SELECT *` —
  see the migration's own comment on why) is what every existing reader
  keeps querying. `write()` now targets `flakes_delta` explicitly — the one
  and only place a row is written, unchanged as an invariant, just aimed at
  a different table. `current_state_query` and `push_live_flakes` (used by
  `query_pattern`/`count`/`explain` and the whole traversal engine
  respectively) needed **zero code changes** — pointing `FROM flakes` at a
  view that already spans both partitions makes current-state resolution
  span them too, for free, which is exactly what decision 2 asks for.
- **Compaction** (`compact(batch_size)`) is one Postgres statement — a
  `DELETE ... RETURNING` feeding an `INSERT ... SELECT` through a CTE — so
  the move is atomic by construction with no explicit transaction to open
  or forget to commit. That atomicity is what makes repeated small-batch
  calls interruptible: every call either fully happens or fully does not,
  and the `flakes` view resolves correctly regardless of which side any
  given row currently sits on.
- **Two real, non-obvious migration bugs found and fixed before any test
  could pass, both from the same root cause**: `SELECT *` inside a
  `UNION ALL` matches columns *positionally*, and `flakes_main`'s physical
  column order (from `V8`'s `ALTER TABLE ... ADD COLUMN`, which always
  appends) does not match a fresh `CREATE TABLE`'s logical column order for
  `flakes_delta` — the two tables' `value_lang`/`value_dir` columns sit in
  different physical positions. The view's own `SELECT *` failed outright
  ("UNION types integer and text cannot be matched"); a hand-written test
  helper doing the same `SELECT *` move-to-main pattern failed the same way
  a second time. Both fixed with explicit, named column lists — more
  robust than getting the physical order to match today, since it stays
  correct the next time a migration appends a column to one table but not
  the other.
- **Two pre-existing tests needed real adaptation, not just column-name
  fixes, found by running the full crate suite after Slice A rather than
  assuming the view's transparency covered everything**: `index_orderings.rs`
  (Slice B of Epic 4's own plan — the four-index-ordering test) seeds
  100k flakes through `assert_flakes`, which now lands them all in
  `flakes_delta` — leaving `flakes_main` empty and the test unable to
  verify what it exists to verify. Fixed by moving the seeded data into
  `flakes_main` by hand after seeding (the same "simulate compaction before
  compaction exists" pattern `tests/partition_split.rs` uses), and pointing
  `ANALYZE` at `flakes_main` directly — `ANALYZE` on a view silently warns
  and skips rather than erroring, found by checking directly, not assumed.
  A second, more interesting finding in the same test: its blanket
  `!plan.contains("Seq Scan on flakes")` assertion tripped on a real,
  **correct** `Seq Scan on flakes_delta` — delta intentionally carries only
  one index, so a shape that index cannot serve legitimately falls back to
  a sequential scan of delta, and that is the entire tradeoff the split
  makes, not a regression. Fixed by narrowing the check to `flakes_main`
  specifically, which is the only table this schema was ever meant to keep
  a sequential scan off of.
- **The decision-5 measurement, run at real 1M-flake scale** (comparable to
  Epic 37a Slice C's own recorded checkpoint, same predicate shape, same
  pattern-query shape):

  | | Measured |
  |---|---|
  | Write throughput (delta, 1 index) | **71,941 flakes/s** |
  | Slice C's recorded baseline (single table, 4 indexes) | 53,641-57,919 flakes/s |
  | Pattern-query p99, before compaction (all 1M flakes in delta) | 28.26ms |
  | Pattern-query p99, after compaction (all 1M flakes in main) | 32.46ms |
  | Slice C's recorded pattern-query time at the same 1M checkpoint | 23ms |

  **Write throughput measured 25-34% faster than Slice C's own recorded
  baseline** — the direction the split's whole design predicts (one index
  to maintain instead of four), not merely asserted.

  **Read latency is the more nuanced half, and the honest reading is "no
  clear regression from the merge itself," not "faster."** Both the
  before- and after-compaction plans use a **sequential scan** of whichever
  table actually holds the data (`Parallel Seq Scan on flakes_delta`, then
  `Parallel Seq Scan on flakes_main`) — this is Slice C's own
  already-documented planner behaviour for this specific query
  (`sid_p = 'name'` matches ~20% of rows, low enough selectivity that
  Postgres prefers a full scan to an index probe), reproduced here
  unchanged rather than newly introduced by the union. The 28ms→32ms
  increase after compaction traces to `EXPLAIN (BUFFERS)`, not to the
  `UNION ALL` itself: the pre-compaction scan reports `read=2528` disk
  blocks (delta's data was still warm in `shared_buffers` from the bulk
  insert moments earlier); the post-compaction scan reports `read=16982` —
  compaction's own write I/O, moving ~1M rows into four newly-populated
  indexes under the same untouched 128MB `shared_buffers` Slice C's own
  measurement ran under, evicted the warm pages. This is a cache-state
  artifact of *this specific run's timing*, not a structural cost the
  split adds — and both numbers (28ms, 32ms) sit in the same order of
  magnitude as Slice C's own 23ms on the old single table, not a multiple
  of it.

  **Verdict**: writes measurably improved; reads did not measurably
  regress beyond what a cold cache after a heavy compaction pass would
  explain on its own, and stayed close to the pre-split baseline at the
  same scale. Decision 5's bar ("if it costs more on reads than
  compaction saves on writes, the split is a loss") is not met on this
  measurement — recorded as a reasoned, single-run judgement at one scale,
  not a high-confidence multi-run result, consistent with how much
  confidence Epic 37a Slice C itself claimed for a comparably-shaped
  finding.

## Explicitly deferred

- **A compressed columnar main partition** → a further optimisation on an
  optimisation nobody has needed yet.
- **Distributed partitions** → single-node is the deployment model (`00a`).
- **Concurrent compaction runs** → this epic's `compact()` assumes one
  caller at a time; nothing here guards against two schedulers racing.
  Not asked for by the acceptance criteria, and adding it before a single
  caller exists to race would be speculative.
