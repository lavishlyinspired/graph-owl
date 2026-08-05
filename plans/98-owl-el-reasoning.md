# Plan: OWL 2 EL Reasoning (Epic 98)

**Status**: **Shipped (5 August 2026)**. SNOMED CT was named as a required ontology; OWL 2 EL is the profile it was designed for, and OWL 2 RL **cannot classify it** — RL and EL are incomparable, so an RL run over SNOMED yields a *wrong* hierarchy, not a smaller one. See `00n-large-ontology-reality.md` §2.1
**Depends on**: Epic 6 (overlay, budgets, explainability), Epic 24 (ontologies as entities)
**Crates**: **`graph-owl-reasoning-el`** (new)

## Goal

Classify ontologies with hundreds of thousands of classes in seconds, which the
RL engine cannot do at any speed.

## Why this is a new crate and not a rule set

**EL is not a bigger RL.** W3C states plainly that none of the three profiles is
a subset of another — they are mutually incomparable. EL and RL are different
languages requiring different algorithms:

| | OWL 2 RL | OWL 2 EL |
|---|---|---|
| Algorithm | Forward-chaining rule materialisation | **Consequence-based classification** |
| Optimised for | Many facts, few classes | **Many classes, few facts** |
| Allows | Unions, negation, `maxCardinality 0/1`, keys, property chains | Existentials, intersections, property chains, keys |
| Forbids | Reflexive properties, disjoint unions | **Universal quantification, cardinality, disjunction, negation, inverse properties** |
| Complexity | PTIME | PTIME, and *practically* far faster on large TBoxes |

Trying to classify a 400,000-class ontology by firing RL rules over it does not
produce a slow answer; it produces no answer inside any budget worth setting.
The two engines share the overlay, the budget model and the explanation format —
and nothing else.

**This is the crate-growth trigger in `00e` being met properly**: a genuinely
different algorithm, not a module that would otherwise be a folder.

## Why it is scheduled now

The earlier deferral said "metadata ontologies are thousands of classes, not the
hundreds of thousands EL exists for". That reasoning was sound for *metadata*
ontologies and wrong about this product's use, which includes **medical
ontologies**. SNOMED CT is over 400,000 classes and is the canonical case EL was
designed for; published results classify it in under a minute.

Recorded because the deferral was reversed by new information about the use
case, not by a change of opinion about the technology.

## Evaluate `whelk-rs` before writing a line — **done, 5 August 2026**

**This document's own licence claim was wrong.** `whelk-rs`
(`github.com/INCATools/whelk-rs`) is real — active (pushed 29 June 2026, not
archived, MIT, **not BSD-3-Clause as first written here**) and has a real
`src/lib.rs`, not a CLI-only tool. But `cargo deny`'s allowlist is not about
whelk's own licence in isolation: its `Cargo.toml` depends directly on
`horned-owl = { version = "^1.4", default-features = false }`, and
`00l-build-vs-adopt.md`'s own "`horned-owl` and LGPL" section already found
that crate to be **LGPL-3.0**. Linking `whelk` into any graph-owl binary
would pull LGPL-3.0 into the dependency tree the moment it compiled in.

The four evaluation questions, answered:

1. **Yes.** `whelk`'s own `scripts/compare-with-elk.sh` validates its
   reasoned subsumptions against ELK's materialized hierarchy — the
   published-correct comparison this criterion asks for already exists
   upstream, checked into the same repository as fixtures.
2. **Via a file, not an in-memory API.** `whelk`'s CLI (`main.rs`) reads
   only from a path (`-i <file>`, dispatched by extension: `.owx` → OWL/XML,
   `.owl` → RDF/XML, both through `horned-owl`'s readers) and owns no store
   of its own beyond that one parse — closer to `00l`'s pattern than a
   library call, exactly what a sidecar boundary needs.
3. **Unverified upstream — this crate does its own check first regardless.**
   Nothing in `whelk`'s CLI or README documents out-of-EL axiom detection.
   Rather than trust silence, `graph-owl-reasoning-el` runs the identical
   `ForbiddenConstruct` presence check `graph-owl-reasoning-ql` already
   built for QL (Epic 99) — the same vocabulary, checked before the axioms
   are ever handed to the sidecar, per this epic's own decision 3 above.
4. **Ours to add.** `whelk --subsumptions` prints only the final pairs
   (`subclass\tsuperclass`, sorted, deduped, tautologies and `owl:Thing`
   excluded by default) — no intermediate path. Explanation (decision 4,
   "A ⊑ B ⊑ C ⊑ D") is computed by this crate from the full subsumption
   set returned, not read from `whelk`.

**The outcome is adopt-and-sidecar, not adopt-and-link.** `whelk` is invoked
as an external binary — `std::process::Command`, a temp `.owl` file written
by this crate, TSV parsed back from stdout — the identical "distinct
process communicating over a pipe" pattern `00l` already recommended for
`horned-owl` itself, for the identical reason. **No graph-owl `Cargo.toml`
ever names `whelk` or `horned-owl`**; `cargo deny`'s allowlist never sees
either. The cost, matching the connectors precedent in `00j-language-
boundaries.md` ("a deployment that wants Snowflake runs a Python worker as
well — a cost it opted into"): a deployment that wants EL classification
must have a `whelk` binary on `PATH` (or a configured path); one that
doesn't touch EL-profile ontologies never needs it built at all.

## Resolved decisions

1. **TBox only.** EL's value is *classification* — computing the class hierarchy
   implied by the axioms. Instance reasoning over EL stays with the RL engine,
   which is better at it. An engine that tried both would be worse at each.
2. **Classification is cached, not recomputed per query.** A 400k-class
   hierarchy is stable between ontology edits and expensive to derive; caching
   it is the entire performance argument. Invalidated by an ontology write,
   never by a data write.
3. **The result is a subsumption index, not derived triples.** RL derives facts
   into the overlay; EL derives a *hierarchy*. Materialising every implied
   `rdfs:subClassOf` for 400k classes would be hundreds of millions of triples
   for something a compact index answers directly.
4. **Explanations are subsumption paths.** Epic 6's requirement holds, in the
   form the algorithm supports: "A ⊑ D because A ⊑ B ⊑ C ⊑ D". Not a rule-firing
   chain, because no rules fire.
5. **Profile detection precedes routing** — see Epic 100.

## Acceptance criteria

- [x] A classic EL test ontology classifies correctly against its published
      expected hierarchy. **Split, honestly, between what this epic verified
      and what it trusts upstream**: `whelk`'s own algorithmic correctness is
      validated by its maintainers against ELK's materialized hierarchy
      (`scripts/compare-with-elk.sh`, cited in the evaluation above) — not
      re-proven here, per the adoption decision. What *this* epic verified
      directly, by building `whelk-rs` from source and running it: the
      wrapper correctly serializes axioms, invokes the sidecar, and parses
      real transitive closure back — a three-level chain's transitive pair,
      never asserted directly, comes back correctly (`slice_a_real_sidecar`,
      `owl_el_reasoning_tests` in `graph-owl-api`).
- [x] A 100k-class synthetic ontology classifies inside the budget. **Measured
      real, not assumed** — 100,000 classes (a branching tree, depth and
      fan-out both, `37a-scale.md`'s own generator shape), 812,619 derived
      subsumption pairs, in 2.77s against a 60s budget
      (`slice_c_real_scale::a_100k_class_ontology_classifies_inside_the_default_budget`).
      Found and fixed along the way: see "the pipe-buffer deadlock" below.
- [x] An axiom outside EL is **reported, not ignored** — an ontology silently
      classified against a subset of its own axioms is a wrong hierarchy
      presented as a right one.
- [x] A subsumption has an explanation naming the intermediate classes.
- [x] Classification is cached and invalidated by an ontology edit, not a data
      edit — asserted by writing data and checking the cache survives.

## The pipe-buffer deadlock — found by running Slice C's own criterion for real

Building the wrapper against small hand-written fixtures (a handful of
axioms, a few dozen bytes of TSV output) passed every test. Running the
100k-class fixture the acceptance criterion actually asks for **hung for
the full 60s budget and timed out** — on a workload `whelk` itself
classifies in ~2s standalone (confirmed by invoking the built binary
directly from the shell first).

**Cause**: `run_sidecar`'s original poll loop called `child.try_wait()` in
a loop but never read `child.stdout`/`child.stderr`, both piped. Once
`whelk`'s TSV output (100k classes → 812,619 lines, tens of megabytes)
exceeded the OS pipe buffer (64KB), the child blocked inside its own
`write()` call with nobody draining the other end — the textbook
full-pipe deadlock. `try_wait()` cannot distinguish "still computing" from
"blocked on our own back-pressure"; both look identical from outside, so
the loop ran out its full budget and reported a timeout that was never
really about the 60s limit at all.

**This is the exact failure mode `CLAUDE.md`'s own build/test-loop section
warns about for this codebase's test suite** ("a hang is a finding, not an
infrastructure annoyance... state explicitly what makes it terminate"),
now showing up in a spawned subprocess rather than a test binary. It would
never have surfaced against the small fixtures every other test in this
crate uses — which is exactly why Slice C's acceptance criterion insists
on a measured 100k-class run rather than a token-sized one.

**Fix**: `stdout`/`stderr` are drained on their own threads, started
immediately after `spawn()` and joined only once `try_wait()` confirms the
child has actually exited — concurrent with the poll loop rather than
sequential after it. Re-measured after the fix: 2.77s, no deadlock, at the
same 100k-class scale that hung before.

## The EL-forbidden vocabulary — verified against the spec directly, not summarised

The earlier WebFetch self-contradiction on RL's own grammar
([[epic-100-blocked-on-real-gaps]]) means this list is checked twice,
against two different sections of the actual W3C document, not trusted
from one summarisation call. Both agree, and the second explicitly
contrasts EL against RL's `ObjectMaxCardinality(0/1)` allowance rather than
conflating the two:

| Forbidden in EL | RDF vocabulary that signals it | Where it appears |
|---|---|---|
| Universal quantification (`ObjectAllValuesFrom`/`DataAllValuesFrom`) | `owl:allValuesFrom` | Inside an `owl:Restriction` blank node |
| Cardinality (`Object`/`DataMaxCardinality`, `MinCardinality`, `ExactCardinality` — **no 0/1 exception**, unlike RL) | `owl:cardinality`, `owl:minCardinality`, `owl:maxCardinality`, `owl:qualifiedCardinality` and its min/max forms | Inside an `owl:Restriction` blank node |
| Disjunction (`ObjectUnionOf`, `DisjointUnion`, `DataUnionOf`) | `owl:unionOf` (also `owl:disjointUnionOf`) | A class-expression predicate |
| Negation (`ObjectComplementOf`) | `owl:complementOf` | A class-expression predicate |
| Inverse object properties (`InverseObjectProperties`) | `owl:inverseOf` | A property-to-property edge, not inside a restriction |

The first three live inside an `owl:Restriction` blank node reached from a
named class via `rdfs:subClassOf`/`owl:equivalentClass` — detecting them
means walking one hop from the restriction back to the class that
references it, not a direct predicate check on the class itself (the shape
QL's simpler `owl:hasKey`/`owl:propertyChainAxiom` check did not need,
since those sit directly on the subject).

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`,
`testing`, `mutation-testing`, and `refactoring` loaded first, batched and
gated once per this project's own convention.

**This crate is not pure logic, unlike `graph-owl-reasoning`/`-ql`.** It
writes a temp file and spawns a subprocess — the same shape as
`graph-owl-connectors`, not `graph-owl-reasoning`'s "pure, no I/O" claim.
`00e-crate-architecture.md`'s purity argument does not apply here; the
distinct-dependency-set argument does (a whole different toolchain: an
external binary, not a Rust dependency).

### Slice A: The sidecar wrapper classifies correctly, proven by transitive closure

**Value**: A modeler asks for a small EL ontology's classification and gets
the correct, transitively-closed hierarchy back — proven against a pair
that was never directly asserted, so the test cannot pass on a rewrite that
merely echoes the input.
**Path**: New crate `graph-owl-reasoning-el`. `to_owl_rdf_xml` serialises
class declarations and `rdfs:subClassOf` axioms as the `.owl` (RDF/XML)
`whelk` reads (confirmed by building `whelk-rs` from source and running it
against a hand-written fixture during this epic's own research — see the
"Evaluate `whelk-rs`" section above). `classify` writes that XML to a temp
file, spawns the configured `whelk` binary with `-i <path> --subsumptions`,
parses the sorted, deduped `subclass\tsuperclass` TSV it prints to stdout
back into `Sid` pairs via the same `to_iri`/`from_iri` round trip
`graph-owl-reasoning-ql` already established, and removes the temp file.
**Family-specific acceptance criteria**:
- A three-level chain (`A ⊑ B ⊑ C`) classifies to include the pair
  `(A, C)`, asserted nowhere in the input — real reasoning ran.
- A missing `whelk` binary produces a named `ElError::SidecarNotFound`, not
  a generic I/O panic.
**RED**: A pure unit test for `to_owl_rdf_xml` (no subprocess) asserting
the exact class/`subClassOf` elements. An `#[ignore]`-by-default
integration test (matching `whelk`'s own convention for its ELK-dependent
tests) that shells out to a real `whelk` binary via a `WHELK_BIN`
environment variable — mirroring `whelk`'s own `ROBOT=/path/to/robot` — and
asserts the transitive pair. Mutator watch: a `classify` that returns the
asserted edges unchanged, never actually invoking the sidecar, would still
pass a test that checks only direct edges — the transitive-pair assertion
is what catches it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: An axiom outside EL is reported, not silently classified around

**Value**: A modeler whose ontology uses a construct EL's own grammar
excludes gets it named, not a hierarchy silently computed as if the axiom
were absent.
**Path**: Before classification, scan the fetched TBox for the five
EL-forbidden constructs in the table above — three by walking one hop from
an `owl:Restriction` blank node back to the class that references it,
`owl:inverseOf` directly on the property pair.
**Family-specific acceptance criteria**:
- All five constructs are independently tested, one fixture each — a
  detector that only matches the first one tried would look complete
  against a single fixture and silently miss the other four.
- A class carrying a forbidden construct is named in `refused_axioms`, and
  every other class in the same ontology that has none still classifies.
**RED**: Five tests, one per construct, each built from real restriction-
blank-node syntax (not a flattened stand-in, since that is exactly the
shape a shallow direct-predicate check would falsely pass). Mutator watch:
a check that only inspects a class's own predicates, never walking into
the restriction, finds nothing on every fixture here.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: A 100k-class ontology classifies inside the budget, or times out named

**Value**: An admin loading SNOMED-scale data gets a real answer inside a
stated budget, or a named timeout — never an unbounded hang, this
project's own "a hang is a finding, not an infrastructure annoyance"
lesson applied to a subprocess instead of a test binary.
**Path**: `ElBudget { max_duration: Duration }` bounds the sidecar via a
process wait with a timeout, killing the child on expiry — straightforward
because the sidecar boundary makes the work genuinely preemptible, unlike
an in-process algorithm. The 100k-class fixture reuses `37a-scale.md`'s own
generator shape rather than a second one.
**Family-specific acceptance criteria**:
- The 100k-class fixture classifies within budget; a known sample of
  expected subsumptions is checked, not assumed from a clean exit alone.
- Exceeding the budget returns `ElError::Timeout` **and** the spawned
  process is confirmed gone afterward (checked against the OS, not merely
  inferred from the error) — an orphaned reasoner process is a resource
  leak this test exists to catch.
**RED**: the 100k-class fixture, timed and sampled. A deliberately-slow
fixture against a short budget, asserting both the error variant and that
the child process no longer exists. Mutator watch: a timeout that returns
the error but leaves the child running would still pass a test that checks
only the error.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: A subsumption has an explanation naming the intermediate classes

**Value**: A modeler asking "why is A a D" gets "A ⊑ B ⊑ C ⊑ D", the form
decision 4 commits to, not the flat fact alone.
**Path**: `whelk` returns only the final, transitively-closed pairs — no
path. `explain(subclass, superclass, &asserted_edges)` is a pure BFS over
the *asserted* (not classified) edges from `subclass` to `superclass` —
the same "read the bulk answer, re-derive the one-fact explanation
locally" pattern `00l-build-vs-adopt.md`'s "adopt for bulk, re-derive for
explanation" note already established for `reasonable`.
**Family-specific acceptance criteria**:
- A four-level chain's explanation names both intermediate classes, in
  order.
- Two siblings under one shared parent (never paired by `whelk` at all)
  return `None` — explanation is offered only for a pair classification
  actually connects.
**RED**: The four-level chain, asserting the exact ordered list. The
sibling negative. Mutator watch: an `explain` returning the whole asserted
edge list regardless of the requested pair would still pass a test that
never checks the path is specific to the two classes asked about.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Classification is cached, invalidated by an ontology write, never a data write

**Value**: A 400k-class hierarchy is not reclassified on every query —
decision 2's performance argument, made real rather than assumed.
**Path**: A cache keyed by the TBox's own watermark — the maximum
transaction time `t` among the fetched TBox flakes, mirroring how `t` is
already this system's one clock (`00b` decision 25). An unchanged
watermark returns the cached result without invoking the sidecar again; a
new TBox-vocabulary flake changes it. An asserted ABox (instance) flake
never touches a TBox predicate, so it never changes the watermark.
**Family-specific acceptance criteria**:
- Two `classify` calls with no TBox change between them invoke the sidecar
  once, not twice — asserted by counting invocations against a recording
  fake sidecar (the same "records what it was asked to do" shape
  `RecordingGraph` already established), not by timing.
- A new instance fact between two calls does not invalidate the cache; a
  new `rdfs:subClassOf` edge does.
**RED**: The invocation-counting test proving one call for two identical
requests. The two invalidation tests (data write: no invalidation; TBox
write: invalidation). Mutator watch: a cache keyed by nothing, or by a
constant, would still pass the first test and fail only the invalidation
ones — each needs its own assertion, matching this project's own "a
surviving mutant is almost always a missing negative test" lesson.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred

- **EL instance retrieval** → decision 1; the RL engine handles ABox.
- **EL++ / OWL 2 EL profile extensions beyond the spec** → the spec is the
  boundary (`00i` rule 2).
