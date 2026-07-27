# Plan: Read/Write Partition Split (Epic 102)

**Status**: Not started — **planned, entry condition is a measurement**
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

- [ ] A query returns identical results before and after compaction. Asserted by
      running it, compacting, and running it again.
- [ ] A retraction in delta supersedes an assertion in main — decision 2, and
      the test that matters most.
- [ ] `as_of` returns identical results across a compaction boundary.
- [ ] Compaction is interruptible and leaves a consistent state at any point of
      interruption.
- [ ] **Measured** write throughput improves and read latency does not regress
      past budget. If either fails, the epic is abandoned rather than tuned.

## Explicitly deferred

- **A compressed columnar main partition** → a further optimisation on an
  optimisation nobody has needed yet.
- **Distributed partitions** → single-node is the deployment model (`00a`).
