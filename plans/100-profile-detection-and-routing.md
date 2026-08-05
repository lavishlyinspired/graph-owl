# Plan: Ontology Profile Detection & Routing (Epic 100)

**Status**: **Shipped (backend detection + routing), 5 August 2026** — console work (this plan's own two UI acceptance criteria) deferred, see "Scope of this pass" under Slices. With SNOMED (EL, Epic 98 — shipped), DBpedia (QL, Epic 99 — shipped) and FIBO (constructs outside RL) all in scope, "which reasoner ran and what could it therefore not conclude" separates a trustworthy answer from a confident wrong one. See `00n-large-ontology-reality.md` §2.3
**Depends on**: Epic 6 (RL engine — shipped), ~~Epic 24 (ontologies as entities)~~ — **phantom**, the identical finding [[epic-100-blocked-on-real-gaps]] already recorded: Epic 24 shipped glossaries/SKOS, not an ontology-entity type; reasoning (RL, EL, QL) already works directly over `owl:`/`rdfs:` flakes with no such layer. Real prerequisites are Epic 6, 98, and 99, all shipped.
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

- [x] A known-RL ontology detects as RL and not as EL — the profiles are
      incomparable and the detector must not report a superset relationship
      that does not exist.
- [x] An ontology using `owl:maxCardinality 2` is reported outside RL, EL and
      QL, naming the axiom.
- [x] A cardinality-free, negation-free ontology with existentials detects as
      EL.
- [x] Reasoning over an ontology in no profile is refused, and the refusal names
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
- [x] The override permits it and the result is **marked partial**, carrying
      what was ignored.
- [x] Detection over a 400k-axiom ontology completes in seconds — it is a
      syntactic scan and must not become a reasoning pass.

## The three grammars — verified verbatim against the spec, not summarised

**Methodology note, because it changed mid-research.** Asking the fetch
tool to *interpret* "is construct X supported" produced a contradiction
this session found live: one call said OWL 2 RL does not support property
chains; the shipped, spec-cited `graph_owl_reasoning` code (`RuleName::
PropertyChain`, citing rule `prp-spo2`) already proves it does. A third,
verbatim-only request ("quote the table, do not interpret") resolved it
correctly and matched the codebase. **Every table below came from a
verbatim-only request, cross-checked against `graph_owl_reasoning`'s own
already-shipped, spec-cited rule set where one exists.** This is the same
failure mode [[epic-100-blocked-on-real-gaps]] already found once; the fix
— quote, don't ask for interpretation — is worth keeping for any future
spec check on this document.

| | Sub-class position | Super-class position | Property axioms permitted | Cardinality |
|---|---|---|---|---|
| **RL** (§4.2.3, verbatim) | `Class≠Thing, IntersectionOf, UnionOf, OneOf, SomeValuesFrom, HasValue, DataSomeValuesFrom, DataHasValue` | `Class≠Thing, IntersectionOf, ComplementOf, AllValuesFrom, HasValue, MaxCardinality(0/1), DataAllValuesFrom, DataHasValue, DataMaxCardinality(0/1)` | `SubObjectPropertyOf` (incl. chains — `prp-spo2`), `Functional`, `InverseFunctional`, `Transitive`, `Symmetric`, `Asymmetric`, `Irreflexive`; **not** `Reflexive`. `HasKey` permitted, restricted to sub-class position | `MaxCardinality` only, **0 or 1 only**; `MinCardinality`/`ExactCardinality` absent from the grammar entirely |
| **EL** (§4.2's own ClassExpression, verbatim, cross-checked twice) | Same production both positions — EL has no sub/super asymmetry | `Class, IntersectionOf, OneOf, SomeValuesFrom, HasValue, HasSelf, DataSomeValuesFrom, DataHasValue` | No `SubObjectPropertyOf` chains beyond simple sub-property; `HasKey` permitted | **None at all**, either position |
| **QL** (§3.2.3, verbatim) | `Class, ObjectSomeValuesFrom(OPE, owl:Thing), DataSomeValuesFrom` | `Class, IntersectionOf, ComplementOf, ObjectSomeValuesFrom(OPE, Class), DataSomeValuesFrom` | `SubObjectPropertyOf` (**simple only, no chains** — "OWL 2 QL disallows... property chains"), `Equivalent`, `Disjoint`, `Inverse`, `Domain`, `Range`, `Reflexive`, `Symmetric`, `Asymmetric`; **not** `Functional`/`InverseFunctional`/`Transitive`/`HasKey` | **None at all**, either position — found missing from `graph-owl-reasoning-ql`'s own check mid-research, fixed separately (see the `fix(99)` commit) |

**Scope decision, recorded rather than silently narrowed**: detecting the
sub/super-class *positional* asymmetry (RL's `AllValuesFrom` legal as a
superclass restriction, illegal as part of a subclass's own definition;
QL/EL's own analogous restrictions) would need a full class-expression
tree walker over skolemized restriction nodes, position-aware at every
level of nesting. This epic's detector instead checks *construct
presence*, the same shape `graph-owl-reasoning-ql`/`-el`'s own forbidden
-axiom detection already established, extended with a *value* check for
RL's `maxCardinality` (0/1 legal, everything else not) — not a full
grammar parse. This is conservative in one direction only: a construct
this detector flags as out-of-profile always genuinely is; a construct
used in the position-sensitive gap could theoretically be reported as
out-of-profile when the fuller grammar would in fact permit it (an
`allValuesFrom` restriction, wherever it appears, is treated as neither
confirming nor forbidding RL membership — see Slice A). Revisit if a real
ontology's profile detection disagrees with an external validator.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`,
`testing`, `mutation-testing`, and `refactoring` loaded first, batched and
gated once.

**Scope of this pass: detection and routing only.** Two of this plan's own
acceptance criteria are console work (the profile badge, the partial
-result marking on screen) — deferred the same way Epic 34/98/99 already
left "no UI of their own" as an honest, separate concern rather than
folding frontend work into a backend-only implementation pass. Recorded
here, not silently dropped: those two criteria stay unchecked below until
a UI-focused pass (the Epic 41 precedent) picks them up.

### Slice A: RL and EL membership, from real predicate presence

**Value**: Given a known-RL ontology, the detector reports RL membership
and *not* EL — the profiles are incomparable, proven by a fixture that
would wrongly detect as both if the check conflated "some restrictions
present" with "these specific restrictions".
**Path**: New module in `graph-owl-ontology` (the crate this plan's own
header already names). `detect_rl(tbox: &RlTbox) -> ProfileMembership`
scans for RL-forbidden constructs — `owl:disjointUnionOf`,
`ReflexiveProperty`-typed subjects, `owl:minCardinality`/
`owl:qualifiedCardinality`/`owl:cardinality`/`owl:maxQualifiedCardinality`
(any value), and `owl:maxCardinality` **only when its literal value is
not 0 or 1**. `detect_el` reuses `graph_owl_reasoning_el::
find_forbidden_axioms` directly — EL's own crate already has the correct
check, verified in Epic 98; this epic wraps it in a `ProfileMembership`
verdict rather than re-deriving it.
**Family-specific acceptance criteria**:
- A TBox with only `rdfs:subClassOf` edges and simple property axioms
  detects RL: `true`, EL: `true` (both profiles genuinely permit this —
  the fixture proving "not conflated" needs a *forbidden* construct on
  one side only, not an absent one).
- A TBox with `owl:hasKey` detects RL: `true` (permitted), EL: `false`
  (forbidden) — the incomparability the acceptance criterion asks for,
  shown by a single real construct rather than asserted in prose.
- A TBox with `owl:maxCardinality` valued `1` detects RL: `true`; valued
  `2` detects RL: `false`.
**RED**: The `hasKey` fixture (RL yes, EL no). The `maxCardinality`
0/1-vs-2 fixture. Mutator watch: a check that flags `owl:maxCardinality`
regardless of value would fail the `1`-valued positive; one that never
reads the value at all would fail both directions identically, which a
test asserting only one direction could miss.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: QL membership, and the axiom outside all three

**Value**: An ontology using `owl:maxCardinality 2` is reported outside
RL, EL *and* QL, naming the axiom — decision 2's "actionable, not just a
verdict" requirement, checked against a construct that is forbidden
everywhere rather than assumed to generalise from one profile.
**Path**: `detect_ql` reuses `graph_owl_reasoning_ql::find_forbidden_axioms`
(now including the cardinality check the `fix(99)` commit added).
`detect_profiles(tbox) -> Vec<(Profile, ProfileMembership)>` runs all
three and returns one verdict per profile, never collapsed to a single
answer — decision 1's own requirement, "in RL and QL, in none, or in all
three".
**Family-specific acceptance criteria**:
- The `maxCardinality 2` fixture detects `false` for RL, EL, *and* QL,
  each verdict naming the axiom independently — not one verdict copied
  three times.
- A TBox with only existentials (no cardinality, no negation) detects EL:
  `true` and QL: `true` simultaneously — decision 1's "in multiple
  profiles" case, shown rather than assumed impossible.
**RED**: The three-way-forbidden fixture, asserting each profile's own
named axiom independently. The both-EL-and-QL fixture. Mutator watch: a
`detect_profiles` that short-circuits on the first forbidden verdict
(rather than checking all three) would still pass a test that checks only
one profile's result.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Routing, refusal, and the override that marks a result partial

**Value**: A caller gets one reasoner chosen for them when the ontology
permits several (decision 5: RL preferred, then EL, then QL), a named
refusal when it fits none, and — if they explicitly ask anyway — a result
they cannot mistake for complete.
**Path**: `route(profiles: &[(Profile, ProfileMembership)]) -> RoutingDecision`
— `Route(Profile)` for the preferred choice, or `Refused { first_offending_axiom }`
naming the first out-of-every-profile axiom found. A `force` flag on the
caller side (`Catalog`-level, not this crate's own concern) permits
proceeding anyway; the result this epic's own API returns then carries an
explicit `partial: true` plus the axioms ignored to get there.
**Family-specific acceptance criteria**:
- An ontology in both RL and EL routes to RL — decision 5's preference,
  proven rather than asserted.
- An ontology in no profile refuses, naming the *first* offending axiom
  found (not "an" axiom, not a count) — actionable per decision 2.
- Overriding a refusal returns a result carrying `partial: true` and the
  specific axioms that were ignored, never a result indistinguishable from
  a complete one.
**RED**: The RL-over-EL preference fixture. The no-profile refusal,
asserting the named axiom is the first one in TBox iteration order, not
an arbitrary one. The override fixture, asserting `partial` and the
ignored-axiom list are both present. Mutator watch: a router that always
returns `Refused` regardless of input would still pass a test that only
checks the refusal path; the preference fixture is what catches it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Detection over 400k axioms completes in seconds — measured, not assumed

**Value**: Detection is a gate on every ontology write (decision 4); a
gate that itself takes minutes defeats the point of being a syntactic
scan rather than a reasoning pass.
**Path**: A synthetic 400k-axiom fixture (a mix of ordinary subclass edges
and a scattering of each forbidden construct, so the scan cannot shortcut
by finding nothing to check). Timed for real, the same "measured, not
assumed" discipline Epic 98's own Slice C used after its 100k-class run
first hung and then, once fixed, measured at 2.77s.
**Family-specific acceptance criteria**:
- Detection across all three profiles, over 400k axioms, completes in
  single-digit seconds.
**RED**: The timed fixture, asserting an actual wall-clock bound, not
merely "returns without panicking".
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred

- **Automatic ontology repair** ("remove the 3 axioms and it would be EL") →
  suggesting is fine, rewriting someone's ontology is not.
- **OWL 2 DL detection** → there is no DL engine to route to. See `00k`.
- **The sub/super-class positional asymmetry** → see the scope decision
  above. `AllValuesFrom`'s legality depends on which side of an axiom it
  sits on; this pass checks presence, not position.
- **Console** (profile badge, partial-result marking, explanation-panel
  reasoner attribution) → this plan's own two UI acceptance criteria,
  left unchecked below for a UI-focused pass, the Epic 41 precedent.
