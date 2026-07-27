# Plan: Reasoning Overlay (Epic 6)

**Branch**: feat/engine-reasoning
**Status**: Not started
**Depends on**: Epic 4 (triples), Epic 5 (ontology types)
**Crate**: `graph-owl-reasoning` (pure logic)

## Goal

Derive facts the graph implies but does not state — and make every derivation explainable. "Why do you believe this table contains PII?" must have an answer naming the rule and the source facts.

## Resolved decisions

1. **OWL 2 RL forward-chaining, not a tableau reasoner.** OWL 2 RL is decidable in polynomial time and expressible as rules. OWL 2 DL brings a tractability cliff and, worse, an explainability loss — a tableau proof is not something a steward can read. **If a rule needs a tableau algorithm, it is out of scope.**
2. **Derived facts are an overlay: queryable, never persisted into the base.** They live in the `graph:reasoning` named graph, materialized per run and replaced wholesale. This keeps the base clean, bounds reasoning cost, and means a rule change cannot corrupt asserted data.
3. **Every derived fact carries its provenance** — the rule that fired and the facts that triggered it. Without this, reasoning is a black box and nobody will trust it.
4. **Bounded by budget, always.** Time and fact-count limits, with `capped: true` reported rather than silently truncating. Forward chaining over a real graph can explode; a reasoner that hangs is worse than one that gives a partial answer.
5. **Reasoning does not run on write.** It runs on demand and on a schedule. Making entity creation wait for a fixpoint is unacceptable.
6. **Pure crate, no I/O.** `derive(rules, facts, budget) -> DerivedFacts`. The caller fetches and stores.

## Implementation reference

### Rule model → `graph-owl-ontology`

```rust
pub struct Rule {
    pub id: Sid,
    pub name: String,
    pub body: Vec<Atom>,        // premises, conjunctive
    pub head: Atom,             // conclusion
    pub enabled: bool,
}

pub struct Atom {
    pub s: Term, pub p: Term, pub o: Term,
}

pub enum Term {
    Var(String),
    Const(Sid),
    Value(FlakeValue),
}
```

### OWL 2 RL axioms as built-in rules

Rather than a general rule engine plus an OWL encoding, the supported axioms are implemented directly. Fewer moving parts, and each is separately testable:

| Axiom | Rule |
|---|---|
| `rdfs:subClassOf` | `(a rdf:type C1), (C1 subClassOf C2) ⟹ (a rdf:type C2)` |
| `rdfs:subPropertyOf` | `(a p1 b), (p1 subPropertyOf p2) ⟹ (a p2 b)` |
| `owl:TransitiveProperty` | `(a p b), (b p c), (p type Transitive) ⟹ (a p c)` |
| `owl:SymmetricProperty` | `(a p b), (p type Symmetric) ⟹ (b p a)` |
| `owl:inverseOf` | `(a p1 b), (p1 inverseOf p2) ⟹ (b p2 a)` |
| `rdfs:domain` | `(a p b), (p domain C) ⟹ (a rdf:type C)` |
| `rdfs:range` | `(a p b), (p range C) ⟹ (b rdf:type C)` |
| `owl:sameAs` | `(a sameAs b), (a p o) ⟹ (b p o)` — powers Epic 17 |

### The eight are a subset, and the subset is the decision

W3C OWL 2 RL specifies roughly eighty entailment rules. Eight are listed above.
That is a deliberate choice, but the earlier version of this plan did not say
so — it presented eight rules as though they were OWL 2 RL, and a reader
comparing against the spec would reasonably conclude the plan had simply missed
seventy of them.

**What the eight cover**: class and property hierarchies, transitivity,
symmetry, inversion, domain and range, and identity. On a metadata catalog that
is close to all of the value, because a metadata ontology is mostly taxonomy
and mostly shallow.

**What is missing, and what each would buy:**

| Missing | Buys | Verdict |
|---|---|---|
| `owl:propertyChainAxiom` | Derive a relationship from a composition — "column feeds column, column belongs to table ⟹ table feeds table" | **Wanted.** This is lineage rollup, which Epic 29 needs and would otherwise hand-code |
| `owl:FunctionalProperty` / `owl:InverseFunctionalProperty` | Uniqueness, and identity inference from a key | **Wanted.** IFP is how Epic 17 resolves duplicates from a shared identifier without a bespoke matcher |
| `owl:hasKey` | Key-based identity across sources | **Wanted**, same reason |
| `owl:disjointWith` | Consistency checking — an asset that is two mutually exclusive things | **Wanted**, but as a *violation*, which is Epic 5's job, not a derivation |
| `owl:someValuesFrom` / `owl:allValuesFrom` | Existential and universal restrictions | Deferred. Metadata ontologies rarely use them, and they are the rules most likely to derive surprising facts |
| `owl:minCardinality` / `owl:maxCardinality` | Cardinality constraints | Deferred to Epic 5 — these are constraints people want *reported*, not silently materialised |

The pattern in that table is the real decision: **rules that derive facts belong
here; rules that detect contradictions belong in Epic 5.** OWL treats both as
entailment. A catalog should not, because a user who declares two disjoint
classes and an asset in both wants to be told, not to have the graph quietly
become inconsistent.

### Pure reasoner → `graph-owl-reasoning`

```rust
pub fn derive(
    rules:  &[Rule],
    facts:  &FactSet,
    budget: &ReasoningBudget,
) -> DerivedFacts;

pub struct ReasoningBudget {
    pub max_duration: Duration,     // default 30s
    pub max_facts: usize,           // default 100_000
    pub max_iterations: usize,      // default 20
    pub max_memory_bytes: usize,    // default 512MB — accounted, not sampled; see below
}

pub struct DerivedFacts {
    pub facts: Vec<DerivedFact>,
    pub capped: Option<CappedReason>,   // Some(_) = incomplete; None = ran to fixpoint
    pub iterations: usize,
    pub duration: Duration,
}

pub enum CappedReason { Duration, Facts, Iterations, Memory }
```

**How `max_memory_bytes` is actually measured**, because a budget nobody can evaluate is a field rather than a limit. Not a process-RSS reading — that includes everything else in the binary and moves under an allocator this crate does not control. Instead the working set is **accounted**: each derived fact's size is a computed function of its flake and premise list, summed as facts are added, checked at the iteration boundary alongside the other three limits. The accounting is approximate and deliberately over-estimates, because the purpose is to refuse before exhaustion rather than to report a precise number.

This keeps `graph-owl-reasoning` pure — no allocator hooks, no platform calls, and the same result on every run for the same input, which is what makes the budget testable at all.

```rust
pub struct DerivedFact {
    pub flake: Flake,               // cx = graph:reasoning
    pub rule: Sid,                  // which rule fired
    pub premises: Vec<Flake>,       // what triggered it
    pub confidence: f64,            // min of premise confidences
}
```

**Semi-naive evaluation**: each iteration only joins against facts derived in the *previous* iteration, not the whole set. Naive re-derivation of the full closure every round is the difference between seconds and minutes on a real graph.

**Fixpoint detection**: stop when an iteration derives nothing new. Deduplication is by `(s, p, o, cx)` — a fact derivable by two rules is one fact with two provenance entries, not two facts.

### Standard rule set (seeded)

The catalog rules that currently exist as special cases in other epics, generalized:

| Rule | Replaces |
|---|---|
| Classification propagates along `feeds` | Epic 25's deliberate non-propagation, now opt-in and explainable |
| Ownership inherits down `contains` | Epic 11's hand-coded upward walk |
| Domain inherits down `contains` | Epic 23's hand-coded upward walk |
| Certification invalidated by upstream breaking change | Epic 26 |
| `sameAs` merges properties | Epic 17 |

Epics 11 and 23 keep their query-time resolution for the read path (it is faster for a single entity); the rules make the same conclusions queryable in SPARQL and explainable. **Both must agree** — that is a test.

## Acceptance criteria (feature level)

- [ ] Each OWL 2 RL axiom above derives correctly, tested in isolation.
- [ ] Derived facts land in `graph:reasoning`, never in the default graph.
- [ ] Every derived fact names its rule and premises.
- [ ] Reasoning terminates on a cyclic graph.
- [ ] Budget exhaustion reports the **matching** `CappedReason` with a partial result, not an error.
- [ ] A rule change re-derives without touching asserted facts.
- [ ] Query-time inheritance (Epics 11, 23) and rule-derived inheritance agree.
- [ ] `GET /reasoning/explain?fact=...` returns the derivation chain.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Each axiom derives (pure)

**Value**: The reasoner's core, exhaustively tested without a database.
**Path**: the eight axioms above as built-in rules over an in-memory `FactSet`.
**Acceptance criteria**, one test per axiom with a positive and a negative case:
- `subClassOf` at depth 3 derives the transitive type (`a:C1`, `C1⊑C2`, `C2⊑C3` ⟹ `a:C3`).
- `transitive` at depth 3 derives the far edge.
- `symmetric` derives the reverse but **not** for a non-symmetric predicate.
- `inverseOf` derives the inverse; the inverse of the inverse is not re-derived as new.
- `domain`/`range` derive types on the correct side — a swapped implementation must fail.
- `sameAs` copies properties in both directions.
- No axiom derives anything when its triggering axiom triple is absent.
**RED**: The depth-3 cases are the specification — a single-step implementation passes depth 1 and fails depth 3. The domain/range swap is the other classic bug. Mutator watch: single-step chaining must fail depth 3; swapping domain and range must fail their tests; a symmetric rule applied unconditionally must fail the non-symmetric case.
**GREEN**: the eight rules, semi-naive iteration.
**REFACTOR**: assess built-in rules vs. a general engine with an OWL encoding. Built-in — eight testable functions beat a rule interpreter plus an encoding nobody can debug. Record it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Fixpoint terminates and deduplicates

**Value**: The property that makes reasoning safe to run on real data.
**Path**: iteration to fixpoint with a visited set and per-iteration deltas.
**Acceptance criteria**:
- A cyclic `subClassOf` (`C1⊑C2⊑C1`) terminates and derives each type once.
- A fact derivable by two rules appears once with two provenance entries.
- Fixpoint is reached — `iterations` is reported and is less than `max_iterations` on a small graph.
- Semi-naive: iteration N only joins against iteration N−1's output, verified by a join counter.
- Deriving over an empty fact set returns empty, not an error.
**RED**: The cycle test with an explicit timeout so a regression fails CI rather than wedging it. A dedup test asserting one fact, two provenances. A join-counter test proving semi-naive rather than naive — the difference is invisible in output and enormous in time. Mutator watch: a removed visited set must hang (timeout catches it); naive re-derivation must fail the join count.
**GREEN**: fixpoint loop, dedup, delta tracking.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Budgets bound the run

**Value**: A reasoner that cannot hang, and that says *which* wall it hit.
**Path**: time, fact-count, iteration, and memory limits checked per iteration.
**Acceptance criteria**:
- Each of the four limits stops the run and reports the **matching** `CappedReason` — `Facts`, `Duration`, `Iterations`, `Memory` — with the facts derived so far.
- A run that reaches fixpoint reports `capped: None`. There is no other way to signal completeness.
- Capping is **not** an error — the partial result is returned.
- A capped result is marked in the graph so consumers know it is incomplete, **and carries the reason**.
- `max_memory_bytes` is measured against the working fact set, not the process; a run that would exceed it stops before allocating.
- Budget is configurable per invocation.
**RED**: Four tests, one per limit, each asserting the *specific* reason rather than merely that capping occurred. `capped: true` told an operator nothing actionable: hitting the iteration cap means the rule set has a cycle to fix, hitting the fact cap means the graph outgrew the budget, and the two demand opposite responses. Mutator watch: an unchecked budget must hang; returning `Err` on cap must fail the partial-result assertion — a partial answer is the designed behaviour; **a single hard-coded `CappedReason` must fail three of the four tests**, which is what makes them worth writing separately.
**GREEN**: budget checks, capping semantics, reason attribution.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Derivations are explainable

**Value**: The property that makes reasoning trustworthy. Without it nobody enables it.
**Path**: provenance on every `DerivedFact`; `GET /reasoning/explain?fact=...`.
**Acceptance criteria**:
- A depth-3 derived fact returns the full chain: rule, premises, and each premise's own derivation if it too was derived.
- A fact derived from asserted facts terminates the chain at those facts.
- A fact derivable two ways returns both chains.
- Explaining a non-derived fact → `404`.
- Explaining an asserted fact returns "asserted", not a chain.
- Confidence on a derived fact is the **minimum** of its premises, not the product — reasoning does not compound uncertainty the way independent sources do.
**RED**: A depth-3 explanation test asserting the full recursive chain, not just the immediate rule. A confidence test asserting minimum rather than product. Mutator watch: a one-level explanation must fail depth 3; using product for confidence must fail — that is the subtle modelling error, and it makes deep derivations look worthless.
**GREEN**: provenance capture, recursive explanation, endpoint.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: The overlay is queryable and replaceable

**Value**: Derived facts are usable without contaminating the base.
**Path**: write derivations to `cx = graph:reasoning`; replace the graph wholesale per run.
**Acceptance criteria**:
- Derived facts are queryable via pattern and (later) SPARQL.
- Default-graph queries **exclude** derived facts unless the caller opts in.
- A re-run replaces the reasoning graph entirely — no accumulation across runs.
- Asserted facts are untouched by a run, verified by comparing the default graph before and after.
- Disabling a rule and re-running removes its derivations.
- Reasoning over `graph:extraction` is **off by default** — unconfirmed extractions do not feed inference.
**RED**: A test asserting the default graph is byte-identical before and after a run — decision 2's guarantee. A re-run test asserting no accumulation. A test asserting extraction-graph facts do not trigger derivations. Mutator watch: writing derivations to the default graph must fail the byte-identity test, which is the failure mode that silently corrupts asserted data.
**GREEN**: named-graph targeting, wholesale replacement, opt-in inclusion.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Standard rules agree with hand-coded inheritance

**Value**: Generalizing the special cases without changing behaviour.
**Path**: seed the standard rule set; assert equivalence with Epics 11 and 23.
**Acceptance criteria**:
- Ownership derived by rule matches Epic 11's query-time resolution, for direct, single-hop, and multi-hop cases.
- Domain derived by rule matches Epic 23's resolution, including the explicit-override case.
- Where they disagree, the test fails — this is an equivalence assertion, not a smoke test.
- Classification propagation along `feeds` is opt-in per classification, not global.
- Certification invalidation fires on an upstream Major bump.
**RED**: A differential test running both paths over the same fixture and asserting identical results, including the override case where inheritance must *stop*. Mutator watch: a rule that propagates past an explicit override must fail it — the same bug Epic 23 Slice C guards against, now in a second implementation.
**GREEN**: seeded rules, differential test harness.
**REFACTOR**: two implementations of the same rule is duplicated knowledge. Assess retiring the hand-coded walk in favour of the rule — but only if the rule path meets the read-latency budget, since query-time resolution exists for speed. Record the outcome either way.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **OWL 2 DL / tableau reasoning** → not planned, permanently (decision 1).
- **Rule learning from data** (AnyBURL-style) → a research direction; the rule set is authored for now.
- **ML link prediction** (embedding-based suggestion) → revisit after rule-based derivation proves itself; it would be a separate confidence band, not a rule.
- **Incremental reasoning** (deriving only from changed facts) → full re-derivation is affordable at target scale. Revisit if Epic 37a shows otherwise; the delta machinery from semi-naive evaluation is the foundation. The established approach is **DRed** (delete/rederive): on retraction, over-delete everything that *might* have depended on the removed fact, then re-derive what still holds. Named here so the eventual implementation starts from a known algorithm rather than an invented one.
- **Parallel derivation** → single-threaded until measured. Rule application across disjoint subjects is embarrassingly parallel and Rust makes it cheap, but a reasoner that is fast and wrong is worse than one that is slow, and the budget in `00a` is not currently threatened. Trigger: Epic 37a showing the reasoning pass dominating a run.
- **SHACL rules as a second derivation engine** → `00k-standards-conformance.md` decision 4. Shapes-driven derivation overlaps this epic; if both ship, derived facts must record which engine produced them, or the explainability requirement in Slice D cannot be met for either.
- **Reasoning over extraction graph** → off by default; enable per-deployment once extraction confidence is trusted.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. `graph-owl-reasoning` has **zero I/O dependencies** — asserted.
5. All cycle tests carry explicit timeouts so a termination regression fails CI rather than wedging it.
6. Reasoning latency < 100ms for 10K triples per `00a-product-position.md`.
