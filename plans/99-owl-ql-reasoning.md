# Plan: OWL 2 QL Reasoning (Epic 99)

**Status**: Not started — **scheduled**
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

- [ ] A subclass query returns instances of subclasses without those facts being
      materialised — asserted by checking the overlay is empty afterwards.
- [ ] The rewritten query is retrievable and names why each branch exists.
- [ ] An axiom outside QL is reported, not silently dropped.
- [ ] Rewriting that exceeds the budget reports truncation. **The critical
      test**: a truncated rewrite must not return a narrowed result presented as
      complete.
- [ ] Authorization survives rewriting — the predicate applies to every branch,
      asserted with two principals. A rewrite that expanded past the access
      predicate would be a read-anything primitive.

## Explicitly deferred

- **R2RML mappings to external databases** → QL makes virtual integration
  *possible*; the mapping language that points it at a foreign schema is its own
  epic.
- **QL for anything RL already covers** → decision 4.
