# Plan: Incremental & Parallel Reasoning (Epic 97)

**Status**: Incremental (DRed) shipped, 7 August 2026 — see Slice A write-up below. Parallel derivation not yet started. Entry condition met 28 Jul 2026: a stated requirement of 10⁸–10⁹ triples makes `06`'s wholesale-replacement-per-run arithmetically unviable, so incremental maintenance moved from optional to prerequisite. See `00n-large-ontology-reality.md` §2.4
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

### Slice A — **shipped, 7 August 2026**

`graph_owl_reasoning::derive_incremental(previous: &Reasoning, retracted:
&[Flake], budget: &Budget) -> Reasoning`.

**Why this is not a re-derivation wearing a different name.**
[`derive_within`]'s own semi-naive loop already records *every* distinct
`(rule, premises)` route to a fact, not a first-found sample — its
`!existing.derivations.contains(&route)` check is exhaustive over the input
it ran against. This reasoner is a positive, monotonic Horn-like fixpoint:
no rule ever fires *because* a fact is absent, so a retraction can only ever
remove derivability, never grant it. A route recorded against the fuller
input is therefore still exactly as valid against any subset that still
contains its premises. Checking which already-recorded routes survive a
retraction is consequently **exact**, not an approximation — it reproduces
precisely what a full re-derivation over the surviving asserted facts would
conclude, without re-running a single rule join. This is also why the
"re-derive" phase classical DRed describes for logics with negation does not
apply here at all: retraction cannot make a *new* derivation possible in a
purely positive, monotonic rule set, so there is nothing to search for
beyond checking existing routes.

**Over-delete, following the derivation graph rather than the data graph.**
A fact is removed once every one of its routes has lost at least one
premise, checked against literal premise identity — never against "shares a
node with something retracted". Removing a fact can itself invalidate
routes that cited *it* as a premise, so the pass iterates to a fixpoint
rather than stopping after the directly retracted flakes — the plan's own
named trap. The loop's *only* continuation signal, after two rounds of
mutation testing narrowed it down by elimination, is whether the `removed`
set grew this pass: a route surviving with fewer derivations than before
needs no extra pass (the pruned list is already in `next` this same pass),
and confidence needs no separate trigger either, because `previous.facts`
is topologically ordered (a premise always appears before anything citing
it, and that order survives every pass) and confidence has exactly one
source of change — a route disappearing — which growing `removed` already
tracks.

**Bounded independently of the correctness argument.** The fixpoint is
provably bounded by `previous.facts.len()` passes under correct logic, but
trusting that proof alone is exactly the mistake this project's CLAUDE.md
already records twice (Epic 19's consume loop, Epic 20's YAML decoder) — so
the loop is also capped by `budget.max_iterations`, `derive_within`'s own
field reused rather than duplicated, and reports `CappedReason::Iterations`
honestly if hit. **Mutation testing found this the hard way**: the first
version had no independent cap, and inverting the loop's own termination
check (`delete !`) produced five mutants that hung for the full 20s
cargo-mutants timeout rather than failing fast — a live demonstration of
the exact failure mode the cap exists to prevent, not merely a hypothetical
one. Two further rounds of mutation testing against `derive_incremental`'s
diff eliminated two more findings by *removing* code rather than adding
tests: a `changed` flag driven by derivation-count and confidence deltas
independently of `removed` growth turned out to be provably redundant given
the topological-order and monotonicity arguments above — a mutant surviving
because the mutated branch could be proven unreachable, not because a test
was missing. Final mutation report: 5/5 viable mutants caught, 1 unviable
(2 unbounded-loop rounds and a redundant-branch round preceded it).

**Confidence is recomputed alongside removal, not left stale.** A fact
surviving on a weaker route is only as certain as that route; `budget` is
read for exactly one field, `named_graph_confidence`, needed because an
asserted premise's own confidence is not stored on the `Flake` and must be
derived from its `cx` the same way `derive_within` does, or a surviving
route through a named-graph premise would silently look more certain than
the run that first derived it.

**Acceptance criteria, verified**: DRed matches a full re-derivation over
the surviving asserted facts, asserted on a graph built specifically so more
than one fact has more than one supporting route
(`dred_matches_a_full_rederivation_on_a_graph_with_multiply_supported_facts`);
a fact with two independent derivations survives the retraction of one,
losing exactly the broken route and no more
(`a_fact_with_two_independent_derivations_survives_the_retraction_of_one`);
over-deletion follows the derivation graph rather than shared
class-hierarchy nodes, proven with two subjects sharing the same
`subClassOf` axioms but independent premises
(`over_deletion_follows_the_derivation_graph_not_shared_hierarchy_nodes`);
budgets are respected under adversarial conditions, not merely under
realistic ones (`a_low_iteration_cap_still_terminates_and_reports_that_it_was_capped`).

**Not yet wired into `Catalog::run_reasoning`.** This slice is the pure
algorithm only — `graph-owl-api`'s reasoning pass still replaces
`graph:reasoning` wholesale on every run, matching this plan's own
"Crates: `graph-owl-reasoning`" scope. Wiring incremental maintenance into
the actual invocation lifecycle (subscribing to retraction events, deciding
when a full run is still cheaper than tracking `previous` across many small
retractions, and the `maintained_to` freshness-stamp obligation below) is
separate, larger, cross-crate work not undertaken in this pass.

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

- [x] DRed under retraction produces the same fixpoint as full re-derivation.
      Asserted by running both and comparing, on a graph with multiply-supported
      facts — the case that catches an over-delete without a re-derive. (Slice A)
- [x] A fact with two independent derivations survives the retraction of one. (Slice A)
- [ ] A parallel run derives the identical fact set *and* identical derivation
      chains as a single-threaded run, over repeated runs.
- [x] DRed remains inside Epic 6's budgets, which do not relax because the
      implementation got cleverer — `max_iterations` enforced independently of
      the loop's own correctness argument, proven under an adversarially low
      cap. (Slice A; parallel derivation's own budget behaviour still open)

## Explicitly deferred

- **Distributed reasoning** → single-node is the deployment model (`00a`).
- **Approximate or anytime reasoning** → returning a partial fixpoint presented
  as complete is the failure this project refuses everywhere else.
