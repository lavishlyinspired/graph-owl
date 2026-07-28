# Plan: Incremental & Parallel Reasoning (Epic 97)

**Status**: Not started — **the measurement now demands it (28 Jul 2026)**. A stated requirement of 10⁸–10⁹ triples makes `06`'s wholesale-replacement-per-run arithmetically unviable, so incremental maintenance moves from optional to prerequisite. See `00n-large-ontology-reality.md` §2.4
**Depends on**: Epic 6 (semi-naive fixpoint), Epic 37a (the measurement)
**Crates**: `graph-owl-reasoning`

## Goal

Make reasoning affordable at a scale that does not yet exist here.

## Why this plan exists before the problem does

To stop it being invented badly under pressure. Both techniques below are
well-established with known names; the failure mode is a future session
inventing an ad-hoc delta scheme because nobody wrote down that DRed exists.

**This epic has an entry condition, and it is not a date.** Epic 6 re-derives
fully on every run, which is affordable at the target scale in `00a`. Building
either technique before measurement says it is needed spends complexity on a
problem nobody has — and both make the reasoner harder to debug, which is a
real cost against a feature whose whole value is explainability.

## Entry conditions

| Technique | Build it when |
|---|---|
| Incremental (DRed) | Epic 37a shows a full re-derivation pass exceeding the reasoning budget in `00a` on a realistic estate |
| Parallel derivation | The reasoning pass dominates a run *and* is not already fixed by DRed |

DRed first if both apply. Not deriving a fact at all beats deriving it faster,
and a parallel full re-derivation is still a full re-derivation.

## Incremental: DRed

**Delete/Rederive**, the established algorithm for maintaining a materialised
fixpoint under retraction:

1. On retraction, **over-delete**: remove every fact that *might* have depended
   on the removed one, following derivation chains forward.
2. **Re-derive** what still holds from the surviving facts.

Over-deleting then re-deriving is cheaper than computing exact dependencies,
and it is correct — the second pass restores anything with another support.

Two things make it fit here rather than fighting the existing design:

- Epic 6 already keeps derivation chains for explainability. DRed needs exactly
  that graph, so the expensive prerequisite is already paid for.
- Retraction is already how this system removes facts (`04-engine-triples.md`
  decision 3), so there is a retraction event to hang the algorithm on.

**The trap**: over-deletion has to follow the derivation graph, not the *data*
graph. Following data edges deletes facts that were never derived from the
retracted one, and the re-derive pass restores them — so the bug is invisible
except as inexplicable slowness.

## Parallel derivation

Rule application across disjoint subjects is embarrassingly parallel, and Rust
makes it cheap. The constraint is the fixpoint: a round must complete before
the next begins, or a rule reads a partially-populated set and derivation
becomes non-deterministic.

**Determinism is not negotiable here.** A reasoner that derives a different set
depending on thread scheduling makes every explanation unreproducible, and
explainability is the feature. Parallelise *within* a round, synchronise
*between* rounds, and assert that a parallel run derives exactly what a
single-threaded run does — same facts, same chains.

## Acceptance criteria

- [ ] DRed under retraction produces the same fixpoint as full re-derivation.
      Asserted by running both and comparing, on a graph with multiply-supported
      facts — the case that catches an over-delete without a re-derive.
- [ ] A fact with two independent derivations survives the retraction of one.
- [ ] A parallel run derives the identical fact set *and* identical derivation
      chains as a single-threaded run, over repeated runs.
- [ ] Both remain inside Epic 6's budgets, which do not relax because the
      implementation got cleverer.

## Explicitly deferred

- **Distributed reasoning** → single-node is the deployment model (`00a`).
- **Approximate or anytime reasoning** → returning a partial fixpoint presented
  as complete is the failure this project refuses everywhere else.
