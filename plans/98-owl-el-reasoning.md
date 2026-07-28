# Plan: OWL 2 EL Reasoning (Epic 98)

**Status**: Not started — **scheduled, and the trigger has fired (28 Jul 2026)**. SNOMED CT was named as a required ontology; OWL 2 EL is the profile it was designed for, and OWL 2 RL **cannot classify it** — RL and EL are incomparable, so an RL run over SNOMED yields a *wrong* hierarchy, not a smaller one. See `00n-large-ontology-reality.md` §2.1
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

## Evaluate `whelk-rs` before writing a line

**An OWL EL reasoner in Rust already exists under BSD-3-Clause** — a permissive
licence this project's `cargo deny` allowlist accepts. `whelk-rs` is a port of
the Whelk reasoner, itself an implementation of the consequence-based rules this
epic would otherwise implement from the papers.

Writing a second one without evaluating it would be the exact failure `00l`
exists to prevent. **This epic does not start until that evaluation is done**,
and the evaluation asks four questions:

1. Does it classify a known ontology correctly against published expected
   output?
2. Can it take a fact set we supply — already `as_of`-resolved and
   authorization-filtered — rather than owning its own store? (`00l`'s pattern.)
3. Does it report axioms outside EL, or silently ignore them? If it ignores
   them, we wrap it with the detection from Epic 100 rather than trusting it.
4. Can a subsumption be explained, or is explanation ours to add on top?

**The likely outcome is adopt-and-wrap**: `whelk-rs` classifies, this crate owns
the fact-set extraction, the profile gate, the budget and the explanation. That
is a small crate around a good library rather than a reimplementation of one.

If it is adopted, most of what follows becomes the wrapper's specification
rather than an algorithm to build — and the acceptance criteria below are
unchanged either way, because they are about *behaviour we promise*, not about
who implements the classification.

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

- [ ] A classic EL test ontology classifies correctly against its published
      expected hierarchy.
- [ ] A 100k-class synthetic ontology classifies inside the budget.
- [ ] An axiom outside EL is **reported, not ignored** — an ontology silently
      classified against a subset of its own axioms is a wrong hierarchy
      presented as a right one.
- [ ] A subsumption has an explanation naming the intermediate classes.
- [ ] Classification is cached and invalidated by an ontology edit, not a data
      edit — asserted by writing data and checking the cache survives.

## Explicitly deferred

- **EL instance retrieval** → decision 1; the RL engine handles ABox.
- **EL++ / OWL 2 EL profile extensions beyond the spec** → the spec is the
  boundary (`00i` rule 2).
