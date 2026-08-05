# Plan: OWL 2 QL Reasoning (Epic 99)

**Status**: **Shipped (5 August 2026)**. DBpedia is the QL shape — a vast ABox against a thin TBox, where materialising inference produces more data than the source. See `00n-large-ontology-reality.md` §2.3
**Depends on**: Epic 7 (query algebra to rewrite), Epic 6 (explanation contract)
**Crates**: **`graph-owl-reasoning-ql`** (new)

## Goal

Answer ontology-aware queries over data that is never materialised, by rewriting
the query instead of deriving facts.

## Read this first: QL does not do what RL does, and its explanations are a
## different thing

Two corrections, because the request that scheduled this epic carried an
assumption worth checking.

**1. QL forbids much of what a catalog uses.** Per W3C, OWL 2 QL forbids
property chains, transitive properties, functional properties, keys, and
cardinality restrictions. Those are exactly the constructs Epics 17 and 29
depend on — `hasKey` and inverse-functional identity, and property chains for
lineage rollup. **QL cannot express them.** It is not a superset of RL and
cannot replace it.

**2. QL does not produce derivation chains, because it derives nothing.** RL
materialises a fact and can then say which rule produced it from which premises.
QL rewrites `?x a :DataAsset` into a larger query that also matches every
subclass, runs *that*, and returns rows. There is no derived fact to explain.

That is not a loss of explainability — it is a **different kind of
explanation**, and arguably a more direct one:

> *"You asked for `DataAsset`. Because `Table ⊑ DataAsset` and
> `View ⊑ DataAsset`, the query actually executed was: `?x a Table UNION ?x a
> View UNION ?x a DataAsset`. This row came from the second branch."*

The user sees the expanded query and which branch matched. Epic 6's requirement
is met by showing **the rewriting**, not a chain. Stated here because promising
"explanations" and delivering a different shape of answer is the failure this
project avoids elsewhere.

## What QL is genuinely good for

Its complexity result is the point: query answering is **first-order rewritable
to SQL** (AC0 in data complexity). Concretely, that means an ontology-aware
query can run **directly against a relational database that graph-owl does not
own** — no import, no projection, no copy.

That is *virtual integration*: a bank's core-banking database answers "which
tables hold a `MonetaryAmount`" through the ontology, without a single row
moving into graph-owl. Nothing else in this roadmap can do that, and it is the
reason to build QL rather than a reason to prefer it over RL.

## Resolved decisions

1. **Rewriting targets the algebra, not the string.** Epic 7 parses to standard
   SPARQL algebra (`07` decision 8); QL expands algebra nodes and hands them
   back to the same planner. Rewriting query *text* would mean a second parser.
2. **The rewritten query is always retrievable.** `?explain=true` returns it.
   Without that this epic is a black box that returns more rows than the user
   asked for.
3. **Rewriting is bounded.** A deep hierarchy expands one pattern into hundreds
   of branches. The same budget model as Epic 6, with truncation reported —
   never a silently narrowed query, which would return *fewer* rows and look
   like a correct answer.
4. **QL and RL may both apply; RL wins for derivation.** If an ontology is in
   both profiles, materialised RL facts are already there and re-deriving them
   by rewriting is waste. QL is used where data is external or unmaterialised.

## Acceptance criteria

- [x] A subclass query returns instances of subclasses without those facts being
      materialised — asserted by checking the overlay is empty afterwards.
- [x] The rewritten query is retrievable and names why each branch exists.
- [x] An axiom outside QL is reported, not silently dropped.
- [x] Rewriting that exceeds the budget reports truncation. **The critical
      test**: a truncated rewrite must not return a narrowed result presented as
      complete.
- [x] Authorization survives rewriting — the predicate applies to every branch,
      asserted with two principals. A rewrite that expanded past the access
      predicate would be a read-anything primitive.

## Where this hooks in — verified against the real code, not assumed

`Catalog::sparql` (`graph-owl-api/src/lib.rs`) parses to `spargebra::Query`
and calls `execute_algebra`, which calls `scoped_facts` (builds the
authorized, `as_of`-scoped dataset) and only then runs
`spareval::QueryEvaluator` against the **same** `spargebra::Query`. Decision
1's "hands them back to the same planner" is literal: rewriting means
producing a new `spargebra::Query` and calling the existing
`execute_algebra` with it, unchanged. Authorization is therefore structural
for free — the rewrite happens *before* `scoped_facts` builds the predicate,
identically to how Epic 101's federation leak-proof property holds because
`SERVICE` scoping happens before `spareval` ever runs (Slice E is the test
that proves this, not new production logic to make it true).

**TBox axioms are ordinary flakes, not a Rust type.** `graph-owl-ontology`
holds SHACL shape types (Epic 5), not OWL axioms — `rdfs:subClassOf`,
`owl:TransitiveProperty`, `owl:hasKey` and the rest are triples using
standard vocabulary, read the same way `graph-owl-reasoning`'s RL engine
reads them (`Reasoning::derive_within` takes a plain `&[Flake]` the caller
already fetched — see "the pattern that lets us adopt reasoners anyway" in
`00l-build-vs-adopt.md`). QL rewriting fetches the relevant subclass/
subproperty triples the same way `Catalog::reasoning_base` fetches RL's base
set: `graph.query_pattern(&TriplePattern { predicate: Some(rdfs:subClassOf),
..Default::default() })`. No new axiom type is introduced; the crate reads
the vocabulary RL already established.

**Crate**: `graph-owl-reasoning-ql`, new — added to `00e-crate-architecture.md`
because it has a dependency `graph-owl-reasoning` genuinely should not gain:
`spargebra`'s algebra types. RL never needs SPARQL algebra (it reasons over
Flakes directly); QL rewriting cannot exist without it. Depends on
`graph-owl-core` (`Flake`, `Sid`) and `spargebra` (the same version
`graph-owl-query` already pins) — not on `graph-owl-query` itself, whose own
contents (`pushdown.rs`, the Cypher module, `dataset.rs`) are Cypher-lowering
concerns QL rewriting has no reason to depend on. **Pure logic** — the
rewrite function takes an algebra tree and a slice of subclass edges, and
returns a rewritten tree; no I/O, exhaustively unit-testable, the same
purity argument as `constraint`/`reasoning`/`authz`.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with `tdd`,
`testing`, `mutation-testing`, and `refactoring` loaded first, batched and
gated once per this project's own convention (not per slice).

### Slice A: A subclass query answers without materializing anything

**Value**: An analyst asks for `?x a :DataAsset` and gets `Table` and `View`
instances too, without a single fact written to the reasoning overlay —
proving decision 12 ("relational source of truth, flakes as view") holds for
QL exactly as it does for RL.
**Path**: `Catalog::sparql` fetches `rdfs:subClassOf` triples reachable from
every class named in the parsed query's `rdf:type` patterns, via
`TripleStore::query_pattern`. `graph_owl_reasoning_ql::rewrite(pattern,
&subclass_edges, &budget)` walks the `spargebra::algebra::GraphPattern`,
and for each BGP triple pattern shaped `?x rdf:type <Class>` (object bound,
predicate `rdf:type`) replaces it with a `Union` across `<Class>` and its
transitive subclasses. The rewritten `spargebra::Query` goes into the
existing, unmodified `execute_algebra`.
**Family-specific acceptance criteria**:
- `Table`/`View`, each `rdfs:subClassOf :DataAsset`, with instances typed
  **only** as `Table` or `View` — never directly as `DataAsset` — so a
  rewrite that silently returns the pattern unchanged cannot pass by
  accident.
- `SELECT ?x WHERE { ?x a :DataAsset }` returns every `Table` and `View`
  instance.
- `graph.query_pattern` against the reasoning graph
  (`graph_owl_reasoning::reasoning_graph()`) is empty before and after —
  nothing was materialized.
- A query naming a class with no subclasses rewrites to itself (the
  identity case), proving the rewrite is conditional, not blanket.
**RED**: A unit test in `graph-owl-reasoning-ql` proving `rewrite` turns one
`rdf:type` BGP into the expected `Union` tree, pure (no store, no Catalog).
An integration test in `graph-owl-api` proving the end-to-end query answers
correctly and the overlay stays empty. Mutator watch: swapping `Union` for
returning the original pattern unchanged must fail the multi-instance test;
a rewrite that fires unconditionally (even with no subclasses) must fail
the identity-case negative.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: The rewritten query is retrievable, naming why each branch exists

**Value**: A user who sees more rows than they typed can ask "why" and get
the actual query that ran, not a black box — Epic 6's explanation
requirement, met by showing the rewriting rather than a derivation chain
(see this plan's own "different kind of explanation" note above).
**Path**: `SparqlOutcome` gains `ql_rewrite: Option<QlRewrite>` —
`QlRewrite { expanded_query: String, branches: Vec<QlBranch> }`, each
`QlBranch` naming the class it matched and the `rdfs:subClassOf` triple
that produced it. Populated whenever Slice A's rewrite actually changed the
query (never populated for the identity case — nothing to explain).
**Acceptance criteria**:
- The response for a rewritten query names, per branch, the subclass and
  the axiom that produced it.
- A query that did not rewrite carries no `ql_rewrite` — silence is the
  signal that nothing was expanded, not an empty list a client has to
  interpret.
**RED**: A test asserting `ql_rewrite.branches` names `Table` and `View`
specifically (not just "two branches"), for the fixture built in Slice A.
A negative test: a query with no rewritable pattern carries `ql_rewrite:
None`. Mutator watch: a branch list that reports a count without naming the
classes would still pass a test that only checked `.len()`.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: An axiom outside QL is reported, not silently dropped

**Value**: A modeler whose ontology uses `owl:hasKey` or a property chain —
exactly the constructs this plan's own opening section says QL forbids —
gets a named refusal instead of a query that quietly under-answers because
QL cannot express what the axiom means.
**Path**: Before rewriting a class, `graph-owl-reasoning-ql` checks whether
any TBox triple touching that class or its properties uses a
QL-forbidden construct — the same vocabulary `graph-owl-reasoning::RuleName`
already names as *rules* (`PropertyChain`, `TransitiveProperty`,
`FunctionalProperty`, `InverseFunctionalProperty`, `HasKey`), checked here
as a presence test on triples, not executed as a rule. A hit adds to
`RewriteOutcome::refused_axioms: Vec<RefusedAxiom>` naming the construct and
the class/property it was found on; rewriting proceeds for everything else
rather than refusing the whole query.
**Acceptance criteria**:
- A class with an `owl:hasKey` axiom is named in `refused_axioms`, and the
  rest of the query still answers for classes that have no such axiom.
- The HTTP response surfaces `refused_axioms` distinctly from `ql_rewrite`
  — a client can tell "expanded, and also incomplete" from "expanded,
  fully".
**RED**: A test with `owl:hasKey` on the queried class; assert it is named,
by construct and by class, and that an unrelated sibling class in the same
query still rewrites normally. Mutator watch: a check that only looks for
*any* forbidden triple in the whole graph (rather than one touching the
queried class specifically) would over-refuse and must fail a test with an
unrelated `hasKey` axiom elsewhere in the ontology.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Rewriting that exceeds the budget reports truncation

**Value**: An admin querying a hierarchy deeper or wider than is safe to
expand gets a result that says so, never a narrowed answer that looks
complete — the plan's own "critical test".
**Path**: `RewriteBudget { max_branches: usize, max_depth: usize,
max_duration: Duration }`, mirroring the shape of
`graph_owl_reasoning::Budget` without reusing its fields (a fact-count/
iteration budget answers a different question than a branch-count/depth
one). The subclass walk in `rewrite` stops and sets `truncated: true` on
`RewriteOutcome` the moment any limit is hit, keeping only the branches
already found — never silently completing a narrower walk and reporting it
as full.
**Acceptance criteria**:
- A subclass chain deeper than `max_depth` (or wider than `max_branches`)
  truncates; `SparqlOutcome.truncated` (the existing field the budget
  system already reports through, per `SparqlBudget`'s own pattern) is
  `true`.
- The returned rows correspond exactly to the branches actually included —
  never to the full hierarchy presented as if the walk had finished.
**RED**: A subclass chain one level deeper than `max_depth`; assert
`truncated == true` and that the deepest, excluded subclass's instances are
absent from the rows (not just that the flag is set). Mutator watch: a
truncation check on branch *count* alone would still pass a test that only
varies *depth*, and vice versa — both dimensions need their own test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Authorization survives rewriting

**Value**: A restricted principal never sees a row leaked through an
expanded branch their policy would have hidden from a direct query — **the
important test in this epic**, per the same reasoning `101-sparql-federation.md`
gave for its own leak-proof test: a result-side assertion passes even when
the data already left, so this has to capture what the evaluator actually
saw, not merely what came back.
**Path**: No new production code is expected — Slice A's own design means
the rewrite runs before `scoped_facts` compiles the predicate, so every
branch of the `Union` is subject to the identical `AccessPredicate` a
direct query would face. This slice is the test that proves it rather than
assumes it, mirroring Epic 101 Slice E.
**Acceptance criteria**:
- Two principals, one admin and one restricted to an FQN prefix that
  excludes some subclass instances: the restricted principal's rewritten
  query returns only what their policy admits, while the admin sees
  everything the rewrite expanded to.
- If this fails, it fails as a **production bug in the rewrite ordering**,
  not a missing test-only guard — worth stating because a wrong finding
  here (an intentional per-branch authz check) would be solving a problem
  the architecture already prevents.
**RED**: The two-principal test above. Mutator watch: a rewrite that
constructed its own dataset (bypassing `scoped_facts`) rather than handing
the rewritten query back to `execute_algebra` would leak — this is exactly
the shape of bug the test exists to catch, and it is a structural
possibility a careless implementation could introduce even though Slice
A's design does not.
**Done when**: criteria met, mutation report reviewed, commit approved.

**Found while writing Slice A's integration test, not before**: `Asset`'s own
`dsc:type` (`graph_owl_core::projection::fields`) is a `FlakeValue::String`
— the `AssetKind` wire name, e.g. `"table"` — never an `rdf:type` triple
pointing at a class node. Both this crate's rewrite and RL's own
`rule_sub_class_of` require a genuine `rdf:type` *reference* triple to fire
(`with_predicate(new, &rdf_type())` in `graph_owl_reasoning`). Neither QL
nor RL therefore reasons over the catalog's own `AssetKind` hierarchy —
`AssetKind::parent_kind` is a separate, purpose-built containment
mechanism, not an OWL subsumption one, and the two were never meant to be
the same thing. Both engines reason over whatever *business ontology* a
caller has separately asserted (a SPARQL `INSERT`, an Epic 33 pack import,
Epic 24 business-semantics data) — the plan's own `:Table ⊑ :DataAsset`
worked example is that kind of assertion, independent of the catalog's
internal five-to-nineteen-kind taxonomy. Every test in this epic seeds an
*additional* `rdf:type` fact on a real asset's own `Sid` for exactly this
reason, rather than relying on `dsc:type`.

A second, related finding: `scoped_facts` (Epic 13) keeps a flake only when
its *subject* is a currently-visible asset id, so a free-standing ontology
node (`:Table`, with no asset behind it) never reaches the evaluator at
all — which is correct, since `TBox` axioms are read directly by
`fetch_ql_tbox` via `TripleStore::query_pattern`, bypassing that filter
entirely (schema data, not row data with an owner to check). An *instance*
triple's subject, by contrast, must be a real asset for its row to survive
and come back — the reason every positive test in this epic creates a real
asset via `upsert_asset` first, rather than asserting instance data against
a synthetic subject the way the `TBox` fixtures do.

## Explicitly deferred

- **R2RML mappings to external databases** → QL makes virtual integration
  *possible*; the mapping language that points it at a foreign schema is its own
  epic.
- **QL for anything RL already covers** → decision 4. Not a separate slice:
  `run_reasoning`'s materialized facts already satisfy any `rdf:type` query
  through the existing evaluator with no rewrite needed, and Slice A's
  identity-case test (a class already answerable without rewriting is left
  alone) is the same mechanism that keeps QL from doing redundant work
  where RL already has an answer — proven as a consequence of Slice A's
  own design rather than needing its own slice.
