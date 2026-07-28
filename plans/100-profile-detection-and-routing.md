# Plan: Ontology Profile Detection & Routing (Epic 100)

**Status**: Not started — **prerequisite for Epics 98 and 99**
**Depends on**: Epic 6 (RL engine), Epic 24 (ontologies as entities)
**Crates**: `graph-owl-ontology`

## Goal

Given an ontology, decide which reasoner can handle it — and say so before
anything reasons over it.

## Why this epic exists at all

Three engines will exist: RL (Epic 6 + 95), EL (98), QL (99). Something must
choose. Two facts from the W3C profiles specification make this a real task
rather than a switch statement:

1. **None of the profiles is a subset of another.** An ontology is not "at
   least RL"; it either satisfies each profile's grammar or it does not, and it
   may satisfy none.
2. **The specification provides no detection mechanism.** Membership must be
   checked against each profile's grammar and global restrictions. Nobody hands
   you the answer.

Without this, the failure mode is silent and severe: an ontology with axioms
outside RL gets loaded into the RL engine, the out-of-profile axioms are
ignored, and the user receives a **confidently wrong hierarchy** — fewer
inferences than the ontology states, with nothing saying so.

## Resolved decisions

1. **Detection reports every profile the ontology satisfies**, not one. An
   ontology may be in RL and QL, in none, or in all three. Returning a single
   "the profile" would force a choice the ontology does not determine.
2. **An out-of-profile axiom is named, with its position.** "This ontology is
   not in EL" is unactionable. "Axiom 47 uses `owl:maxCardinality`, which EL
   forbids" is something an ontologist can fix or knowingly accept.
3. **Reasoning over an ontology outside every profile is refused by default**,
   with an override. Partial results from a partially-understood ontology are
   the failure this whole epic exists to prevent, and the override exists
   because a user who understands the loss may still want the inferences that
   *are* sound.
4. **Detection is cheap and runs on every ontology write.** It is a syntactic
   check over axioms — no reasoning — so it can be a gate rather than a report
   someone remembers to run.
5. **Routing prefers RL where an ontology is in multiple profiles**, because RL
   materialises and materialised facts are explainable as chains, which is the
   project's default contract. EL is chosen for classification when the class
   count makes RL impractical; QL when the data is external.

## Acceptance criteria

- [ ] A known-RL ontology detects as RL and not as EL — the profiles are
      incomparable and the detector must not report a superset relationship
      that does not exist.
- [ ] An ontology using `owl:maxCardinality 2` is reported outside RL, EL and
      QL, naming the axiom.
- [ ] A cardinality-free, negation-free ontology with existentials detects as
      EL.
- [ ] Reasoning over an ontology in no profile is refused, and the refusal names
      the first offending axiom.
- [ ] **Console: the detected profile and the reasoner it routes to are shown
      wherever reasoned facts are.** A profile badge on the ontology, and on the
      explanation panel the reasoner that produced the derivation. Users ask
      "which profile is this, and what could the reasoner therefore *not*
      conclude" **before** trusting an answer, not after — and Epics 98 and 99
      add reasoners with different completeness guarantees, so an unlabelled
      conclusion is one whose strength cannot be assessed. `00f` non-negotiable
      4 in a new place: a derived fact whose derivation strength is invisible is
      not meaningfully explainable.
- [ ] **Console: an out-of-profile ontology, and an override-permitted partial
      result, are visibly marked as such** — not colour alone, per `00h`. A
      partial reasoning result presented like a complete one is this epic's
      worst outcome on a screen, exactly as it is in the API.
- [ ] The override permits it and the result is **marked partial**, carrying
      what was ignored.
- [ ] Detection over a 400k-axiom ontology completes in seconds — it is a
      syntactic scan and must not become a reasoning pass.

## Explicitly deferred

- **Automatic ontology repair** ("remove the 3 axioms and it would be EL") →
  suggesting is fine, rewriting someone's ontology is not.
- **OWL 2 DL detection** → there is no DL engine to route to. See `00k`.
