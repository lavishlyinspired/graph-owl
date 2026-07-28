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

## Open: incremental maintenance meets time travel

**Raised 28 July 2026 and not yet resolved.** This epic and Epic 4 are each
sound alone and their intersection is unexamined — a search of this plan for
`as_of` returns nothing, which is the tell.

Two facts collide. Reasoning becomes **materialised and incrementally
maintained** (this epic). And any query may specify **`as_of` any past
instant** (Epic 4) — the product's headline differentiator. So: *what does the
overlay mean at time `t`?* The maintained materialisation reflects **now**.

Three options, none costless, and one must be chosen before Slice A:

| Option | Cost |
|---|---|
| Re-derive per historical query | Defeats the point of materialising; a time-travel query becomes the slowest thing in the system |
| Version the overlay alongside the base | Storage multiplies by the number of retained instants, over a set already the size of the base |
| **Refuse reasoning on historical queries**, and say so | Cheapest and most honest, but weakens "reason over any past state" — which some will read as the differentiator's whole point |

### Resolved, 28 July 2026: refuse reasoning on historical queries

**When `as_of` is set, the overlay is skipped.** The caller gets asserted facts
at `t` and no inferences. Adopted because the alternatives are worse in ways
that are not close: re-deriving per query makes a time-travel query the slowest
thing in the system, and versioning the overlay multiplies storage over a set
already the size of the base — *and* versioning derived data is a cache
invalidation problem wearing a storage costume.

**The differentiator survives the narrowing.** It becomes "time travel over
asserted facts, with explainable inference on the current state" rather than
"time travel over inferred facts". Nothing in `00a-product-position.md` or
`40-ui-graph-explorer.md` ever claimed the latter — checked before adopting
this, precisely because narrowing a claim nobody made is free and narrowing one
already sold is not.

**Revisit trigger**: a concrete request for *"what did the system believe at
time T?"* — which is a genuinely different question from *"what were the
asserted facts at time T?"* and does require a versioned overlay. A
hypothetical does not qualify.

#### Two refinements this resolution needs to be safe

**1. The signal rides the freshness stamp, not a new flag.** Epic 4 decision 8
already requires every flake-backed result to carry "the transaction time it was
computed at and the current projection lag", for exactly this class of problem —
*an eventually-consistent answer presented as current is the failure mode of this
whole design*. "Reasoning was not applied" belongs in that stamp. A second,
parallel mechanism would be two ways to say one thing, and the one a client
forgets to read is the one that matters.

It has to be **structural, not advisory**. A historical query that silently
returns asserted-only results is the absence-versus-omission failure this project
ranks worst: the caller sees fewer rows and concludes *the estate was smaller
then*, when in fact the reasoner was skipped. That is the same bug as Epic 40's
silent truncation and Epic 101's invisible `SILENT`, in a third place. Where a
query could only be satisfied by inference, returning zero rows is not an
acceptable answer.

**2. Incremental maintenance introduces a *third* time, and it must be visible.**
The resolution above handles two — the query's `as_of` and the base's `t`. A
maintained overlay adds `maintained_to`: the base transaction time the inference
set has been brought up to. **With incremental maintenance the overlay lags the
base even at "now"**, so "current inferences" means *inferences current as of
`maintained_to`*, which is not the same statement. Callers must be able to read
it, and this epic owns it — the lag does not exist under wholesale replacement,
so it arrives with this epic and must not arrive silently.

**Whichever is chosen, silence is the one unacceptable answer.** Returning the
*current* overlay against a *historical* base would produce facts derived from
premises that did not hold at `t` — a wrong answer wearing the provenance of a
right one, which is precisely what `06`'s explainability contract exists to
prevent.

**DRed is not optional here, and the reason is structural.** DRed exists to
handle *deletion* from a materialisation. In this store `op = false` is a
retraction, so facts leaving is not an edge case — it is the normal way the
graph changes, and it is how time travel works at all. An additions-only
incremental scheme (semi-naive over new facts) is therefore insufficient by
construction, not merely incomplete.

**Sequencing note: incremental before parallel.** Parallelising a wholesale
re-derivation optimises the work incremental maintenance is about to delete.
The two are listed together in this epic's title; they are not equal, and the
order is not interchangeable.

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
