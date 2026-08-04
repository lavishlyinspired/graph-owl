# Plan: Cypher Query Support (Epic 7b)

**Branch**: feat/engine-cypher
**Status**: **Slices A, B, C, E, F built** (4 August 2026) — `decypher` adopted, subset gate on its lossless CST; lowering to `spargebra::algebra::GraphPattern` with relationship reification, isomorphism constraints and implicit-grouping aggregates; `POST /cypher` sharing `sparql`'s own authorization path. Slices A2 (TCK oracle) and D (variable-length patterns via traversal) not started — see their sections below for why. **Slice A was re-scoped 4 August 2026**: adopt an existing Rust Cypher parser if one survives a controlled spike; generate from the Apache-2.0 grammar only if none does. See Slice A and `00l-build-vs-adopt.md`
**Depends on**: Epic 7 (SPARQL plan is the lowering target), Epic 7a (traversal), **Epic 7c (LPG projection — 7c ships before 7b; the letters are labels, not a sequence)**
**Unblocks**: Epic 7d (Bolt), Epic 41 (query workbench)
**Crates**: `graph-owl-query` (new `cypher` module — **not a separate crate**) · consumes `graph-owl-lpg` (7c) · consumed by `graph-owl-bolt` (7d)

## Goal

Accept Cypher for callers who know it, by lowering it to the existing SPARQL plan — one query engine, two front ends.

## Status change: optional → scheduled

This epic was written as optional, on the reasoning that *"Cypher adds no capability the SPARQL subset lacks; it adds familiarity. That is a real adoption argument and a poor correctness argument."*

That reasoning was sound in isolation and wrong in context. Labelled-property-graph support became a first-class goal, and Cypher stopped being a matter of familiarity:

1. **Epic 7d (Bolt) requires it.** The wire protocol carries Cypher. No Cypher, no Bolt; no Bolt, no property-graph driver or tool ecosystem — the single highest-leverage integration in the roadmap.
2. **Epic 7c gives it a home.** Lowering Cypher onto a *reified triple* model would have been an impedance mismatch. Lowering it onto an explicit LPG projection is a direct mapping, which removes most of the risk that made it look expensive.
3. **The query workbench (Epic 41) needs two languages** to serve both audiences from one console.

So it is scheduled, and the dependency runs the other way from what the original plan assumed: Cypher is no longer a nice-to-have on top of SPARQL, it is the gateway to the property-graph half of the product.

**openCypher, targeting GQL.** The subset below is openCypher-shaped. GQL (ISO/IEC 39075) is the standardized successor and the two are close by design; where they differ, the plan follows GQL, and the divergences are noted in the parser rather than discovered later. Full conformance to either is out of scope, per `00a-product-position.md` — this is a documented subset, as with SPARQL.

## Resolved decisions

1. **Lower to the SPARQL plan; never a second execution engine.** Parse Cypher → `QueryAst` → the same planner, optimizer, and physical operators from Epic 7. Two engines would mean two authorization paths, two optimizers, and two sets of correctness bugs.
2. **A module in `graph-owl-query`, not a crate.** It shares the AST, planner, and operators. A separate crate would either duplicate them or depend on the whole of `graph-owl-query` anyway — the earn-your-keep rule (`00e-crate-architecture.md`) fails.
3. **Read-only.** `CREATE`, `MERGE`, `SET`, `DELETE` are **not** supported, for the same reason SPARQL Update is not (Epic 7): writes go through the catalog API so validation, versioning, and authorization apply. A Cypher write path would bypass all three.
4. **A documented subset, stated up front.** `MATCH`, `OPTIONAL MATCH`, `WHERE`, `RETURN`, `WITH`, `UNWIND`, `ORDER BY`, `SKIP`, `LIMIT`, `DISTINCT`, and variable-length patterns. Not: `CALL`, procedures, `FOREACH`, path predicates beyond variable-length, or shortest-path functions (Epic 7a's API serves those directly).
5. **Aggregates are in scope here even though Epic 7 defers them for SPARQL.** Cypher is unusable without `count`, and implementing them once in the shared planner benefits both front ends. This is the one place where Cypher drives a SPARQL capability rather than the reverse.

## Implementation reference

```rust
// graph-owl-query/src/cypher/mod.rs
pub fn parse(query: &str) -> Result<CypherAst, CypherError>;
pub fn lower(ast: &CypherAst) -> Result<QueryAst, LoweringError>;   // -> Epic 7's AST
```

### Mapping

| Cypher | Lowers to |
|---|---|
| `(n:Table)` | `?n dsc:type dsc:Table` — labels *are* Epic 7c's label set |
| `(a)-[:FEEDS]->(b)` | reified: `?r dsc:fromEntity ?a . ?r dsc:relType "feeds" . ?r dsc:toEntity ?b` |
| `(a)-[:FEEDS*1..3]->(b)` | property path `?a (dsc:feeds){1,3} ?b` via Epic 7a |
| `(a)-[r:FEEDS]->(b)` with `r.confidence` | `?r dsc:confidence ?conf` — reification makes edge properties natural |
| `OPTIONAL MATCH` | `OPTIONAL` |
| `WHERE` | `FILTER` |
| `WITH` | a plan pipeline boundary |
| `UNWIND` | list expansion in the planner |
| `count(...)`, `collect(...)` | aggregate operators (Slice F) |

Epic 7c owns this mapping table; this epic consumes it rather than restating it. A second copy of the mapping is a divergence waiting to happen.

**Reified relationships make Cypher's edge properties a natural fit** — `[r:FEEDS]` binding to a relationship node with its own properties is exactly what Epic 4 decision 4 already models. A flat-predicate graph would make this mapping awkward.

### Semantic mismatch to handle explicitly

Cypher's `MATCH` uses **relationship-isomorphism** semantics: within one `MATCH`, the same relationship cannot bind twice. SPARQL BGP is **homomorphic** (Epic 7 decision 3). Lowering must inject a distinctness constraint over relationship variables, or a Cypher user gets results Cypher would not produce. This is the single subtlest thing in the epic.

## Acceptance criteria

- [ ] The documented subset parses, lowers, and executes via Epic 7's engine.
- [ ] No second execution engine, planner, or authorization path exists.
- [ ] Relationship-isomorphism semantics are honoured, not silently homomorphic.
- [ ] Variable-length patterns use Epic 7a's traversal.
- [ ] Write clauses are rejected with a message pointing at the catalog API.
- [ ] Authorization applies identically to SPARQL — same compiled predicate.
- [ ] `POST /cypher` returns the same result envelope as `/sparql`.
- [ ] Unsupported constructs return a specific "unsupported" error naming the construct.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Parse the subset — **adopt a parser, or generate one only if none survives**

**Re-scoped twice. Read this before writing any parser code.**

The slice was originally written as "build a Cypher parser". On 4 August 2026 it
became "vendor the EBNF and generate a `pest` parser". A second pass over the
ecosystem the same day found three Rust Cypher crates the first had missed — the
first search returned the *hyphenated* `open-cypher` (abandoned 2022) and was
never repeated for the unhyphenated `opencypher`. So:

> **Adopt an existing parser if one passes a controlled spike. Generate from the
> official grammar only if every candidate fails. Hand-writing one is the last
> resort.**

Full analysis and the licence table are in `00l-build-vs-adopt.md`. What binds
this slice:

- **The ~10k-line figure quoted from `07c` is not this slice's size.** It is a
  *complete* front end for the whole language. This subset is eleven clause
  forms.
- **The spike ran on 4 August 2026 and settled it: adopt `decypher`.** It is
  the only candidate that parses the whole declared subset — 22/22, including
  relationship properties and float literals — with a typed AST and positioned
  errors. `cypher-parser` is **disqualified**: it cannot parse
  `[r:FEEDS {confidence: 0.9}]` and cannot parse `0.8` anywhere. Results, corpus
  and harness are in `00l-build-vs-adopt.md` and
  `spikes/cypher-parsers-2026-08/`.
- **Auditability is a gate, not a score.** `opencypher` is the best API fit on
  paper — a typed, span-annotated AST — and cannot be adopted while its source
  cannot be read, its licence claim cannot be checked against the code, and
  there is nowhere to file a bug. This is the same objection `00l` raised
  against `ocg`; applying it inconsistently would make it worthless.
- **A CST is not an AST, and error-tolerance is not a virtue here.**
  `tree-sitter` recovers from malformed input and returns a partial tree. That
  is ideal for Epic 41's workbench and a hazard for this slice, because this
  parser is a **gate** deciding whether a query is in the supported subset — a
  parser that recovers rather than refusing makes that decision by omission. If
  tree-sitter wins for the editor and something typed wins for the engine, two
  parsers is the right answer and the TCK is what keeps them agreeing.

**The spike is done.** What remains for this slice is the *adapter*: `decypher`
AST → our `CypherAst`, plus the subset gate. Two consequences of the spike bind
the work:

- **The subset gate is ours, not the parser's.** `decypher` parses `CREATE`,
  `CALL` and `FOREACH` correctly — they are valid Cypher — so the adapter
  rejects them, which is where it belongs anyway: only we can say "use
  `POST /assets` instead", and Slice A's RED requires that wording.
- **The adapter is the only file that touches `decypher`'s AST.** Its README
  says that AST is unstable until 0.2.0, so confining it to one boundary with
  its own tests is what makes an alpha dependency acceptable rather than
  reckless. If it is abandoned before 0.2.0, that one file is the blast radius.

**Acceptance criteria**: node and relationship patterns with labels, types, direction, and properties; `MATCH`, `OPTIONAL MATCH`, `WHERE`, `RETURN`, `WITH`, `UNWIND`, `ORDER BY`, `SKIP`, `LIMIT`, `DISTINCT`; variable-length `*1..3`, `*`, `*..5`; a write clause → a specific rejection naming it and pointing at the catalog API; `CALL`/`FOREACH` → "unsupported"; syntax errors report line and column; **an out-of-subset construct is refused explicitly rather than partially parsed** — whichever parser is adopted, a silent partial parse on the engine path is disqualifying.
**RED**: A query corpus with expected ASTs, plus a malformed corpus with expected positions. Write-clause tests asserting the rejection *names the API to use instead* — a bare "unsupported" leaves the user stuck. **A partial-parse test**: a query that is half in-subset must be refused whole, not lowered as far as it got. Mutator watch: accepting `CREATE` must fail; generic errors must fail the naming assertions; treating a partial parse as success must fail the refusal test.
**Done when**: criteria met, the spike's result is recorded in `00l`, mutation report reviewed, commit approved.

### Slice A2: The TCK as a conformance oracle

**Adopted regardless of which parsing route Slice A takes.** The openCypher
**Technology Compatibility Kit** is Apache-2.0 Cucumber features defining Cypher
behaviour. Running it tells us **empirically** what the subset supports instead
of us asserting it — the same role `00k-standards-conformance.md` gives
specification conformance everywhere else.

**Acceptance criteria**: the TCK runs against this engine; every scenario is
recorded as pass, fail, or **out-of-subset**, and the third is a first-class
outcome rather than a failure; the out-of-subset list is generated from the run
rather than hand-maintained, so it cannot drift from what the parser does; a
scenario moving from out-of-subset to fail is a regression the suite catches.
**RED**: A scenario known to be outside the subset must be reported
out-of-subset and **not** as a pass — a harness that counted skips as successes
would report conformance we do not have, which is the exact claim `00a` refuses
to make.
**Done when**: criteria met, the generated subset report is committed, commit approved.

### Slice B: Lower to the SPARQL AST (pure) — **built** (4 August 2026)

**Acceptance criteria**: each mapping-table row lowers correctly, asserted against expected `QueryAst`; a reified relationship pattern lowers to three patterns, not one; edge properties lower to patterns on the relationship variable; `WITH` becomes a pipeline boundary preserving projection; lowering is deterministic; an unlowerable construct fails at lowering, not at execution.
**RED**: Golden `QueryAst` per mapping row. The edge-property case is the one that proves reification pays off. Mutator watch: lowering an edge to a single predicate must fail the three-pattern assertion, and would lose edge properties entirely.
**Done when**: criteria met, mutation report reviewed, commit approved.
**Built as** `crates/graph-owl-query/src/cypher/lower.rs`. Two gaps found while writing, both fixed and covered: a property referenced only in `WHERE`/`RETURN`/`WITH` was never bound by a triple pattern, silently returning zero rows (`collect_properties` + `with_property_bindings` fix it); `decypher` nests chained comparisons (`1 < n.x < 10`) left-recursively, so a naive fold produced `Less(Less(1, ?n_x), 10)` — a boolean compared to an integer — fixed with `lower_comparison` returning the conjunction and the rightmost operand explicitly. A `QueryBody::Regular` (`UNION`) arm was written, found unreachable by mutation testing (the subset gate already refuses `UNION`), and removed rather than left as dead code that reads as "handled".

### Slice C: Relationship-isomorphism semantics — **built** (4 August 2026)

**Value**: The correctness trap. Without it, Cypher users get wrong answers that look right.
**Acceptance criteria**: `MATCH (a)-[r1]->(b)-[r2]->(c)` does **not** bind `r1` and `r2` to the same relationship; across separate `MATCH` clauses, reuse **is** permitted (Cypher's actual rule); node variables may still coincide; the injected distinctness is visible in the lowered AST, not hidden in execution.
**RED**: A self-loop fixture where homomorphic semantics would return a row and Cypher would not — the exact inverse of Epic 7 Slice C's homomorphism test. Both tests must pass simultaneously, which proves the two front ends have genuinely different semantics over one engine. Mutator watch: omitting the distinctness constraint must fail; applying it across `MATCH` clauses must fail the cross-clause test.
**Done when**: criteria met, mutation report reviewed, commit approved.
**Built as** `isomorphism_constraints` in `lower.rs`: pairwise `Not(SameTerm(...))` over relationship variables within one `MATCH`, injected into the algebra as `Filter` nodes so the constraint is visible in the plan. Confirmed pairwise (not just adjacent) with a three-relationship fixture asserting all three pairs are present.

### Slice D: Variable-length patterns via traversal

**Acceptance criteria**: `*1..3` uses Epic 7a's traversal engine, not repeated joins; `*` is bounded by the configured maximum, not unbounded; a cycle terminates; truncation is reported in the result envelope; `[r*]` binding the relationship list returns the path edges.
**RED**: A test asserting the traversal engine is invoked (call counter), not a join expansion — the O(n²) failure Epic 7a exists to prevent. Cycle test with a timeout. Mutator watch: join-based expansion must fail the counter test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Endpoint and shared authorization — **built** (4 August 2026)

**Acceptance criteria**: `POST /cypher` with the same envelope, pagination, and error format as `/sparql`; authorization uses the identical compiled predicate — asserted by a test running the same logical question in both languages under a restricted principal and comparing results; timeouts and truncation behave identically; a `403`-worthy query returns empty, not an error.
**RED**: The cross-language equivalence test under a restricted principal is the important one: if Cypher and SPARQL disagree on what a principal may see, one of them is a leak. Mutator watch: a separate authorization path must fail the equivalence test.
**Done when**: criteria met, mutation report reviewed, commit approved.
**Built as** `Catalog::sparql` and `Catalog::cypher` in `graph-owl-api`, both thin front ends over a shared private `execute_algebra(principal, &spargebra::Query, as_of, budget)` — `cypher` parses+lowers to a `spargebra::Query::Select` and hands it to the identical function `sparql` calls after its own parse. This is not two implementations asserted to agree; it is one function two callers share, so the equivalence is structural rather than tested-into-existence. `POST /cypher` in `graph-owl-server` mirrors `/sparql`'s handler and shares a `query_outcome_json` renderer, registered in `admission.rs`'s `Class::Query` and in `openapi.rs`'s route table. Equivalence asserted at both the `Catalog` unit level (`InMemoryStorage` + a real `Policy`, no container) and the HTTP level (`crates/graph-owl-server/tests/authorization.rs`, mirroring the existing SPARQL fixture against a real Postgres-backed estate).

**A note on the equivalence query's shape**: the cross-language test compares `t.type = "table"` (a property filter) rather than a Cypher label match (`:table`), because Epic 7c deliberately treats a label as an ontology-class assertion (`dsc:type` valued as a `Ref`) and *not* the asset's own string-valued kind field (`a_literal_valued_type_does_not_become_a_label`). The two front ends still answer the identical logical question; the query text differs because Cypher's label syntax and this catalog's `type` field are not the same concept, and pretending otherwise would have tested the wrong equivalence.

### Slice F: Aggregates (shared with SPARQL) — **built** (4 August 2026)

**Acceptance criteria**: `count`, `count(DISTINCT ...)`, `collect`, `sum`, `avg`, `min`, `max`; implemented as planner operators available to **both** front ends; grouping is implicit by non-aggregated `RETURN` terms, per Cypher; `count(*)` and `count(expr)` differ correctly on nulls; aggregates compose with `ORDER BY` and `LIMIT`; Epic 7's SPARQL front end gains `GROUP BY` from the same operators.
**RED**: The null-handling difference between `count(*)` and `count(expr)` is the classic bug. An implicit-grouping test. A test asserting the same operator serves a SPARQL aggregate query. Mutator watch: treating `count(expr)` as `count(*)` must fail the null test; a Cypher-only aggregate implementation must fail the SPARQL reuse test.
**REFACTOR**: this slice retires Epic 7's aggregate deferral. Update that plan's deferred section rather than leaving two statements of the same scope.
**Done when**: criteria met, mutation report reviewed, commit approved.
**Built as** `group_by_items` in `lower.rs`, lowering onto `spargebra::algebra::AggregateExpression`/`GraphPattern::Group` — the same operator `spareval` already evaluates for SPARQL's own `GROUP BY`, so nothing was added to the SPARQL path at all; it already had this via the adopted evaluator. **`collect(...)` is refused, not approximated**: SPARQL's nearest operator, `GROUP_CONCAT`, folds values into one delimited string, and Cypher's `collect()` returns a genuine list — silently substituting one for the other is exactly the class of approximation this module refuses everywhere else. `count(*)` and `count(expr)` lower to different `AggregateExpression` variants (`CountSolutions` vs `FunctionCall`), verified by a real evaluation over flakes with a property missing from one row, not just an algebra-shape assertion — the null-handling difference the plan calls out as the classic bug.

**Three bugs found while writing this slice, all fixed and covered by tests:**

- **`decypher` drops a bare-variable function argument from its typed AST** — `count(n)` arrives with `arguments: []`, confirmed for six different function names, so it is a general AST-lowering gap and not specific to any one aggregate (a third `decypher` defect, alongside the already-documented `CALL … YIELD` drop and the earlier CST-vs-AST admission gap). A property-lookup argument (`sum(n.amount)`) is unaffected. Recovered from the lossless CST — the same tree `subset.rs`'s gate already trusts — by matching the AST node's span to the CST's `FUNCTION_INVOCATION` node and reading its `VARIABLE` child. `lower()` now takes the source text alongside the parsed query solely for this recovery.
- **An explicit alias on a projection item was never actually bound to anything** — `RETURN n.name AS label` named the projected column `label` but the pattern only ever bound `?n_name`; the existing Slice B test (`an_alias_names_the_projected_variable`) passed regardless, because it asserted the rendered plan *contained the string* `"label"`, which is true whether or not anything binds it. Every Slice B/C test before this slice asserted the lowered plan's shape and never executed it — this slice's aggregate work needed real evaluation to check `count(*)` vs `count(expr)`, and running the *existing* alias case the same way found it silently returning rows with the aliased column always absent. Fixed with `bind_aliases`, the non-aggregate counterpart of the `Extend`-based renaming `group_by_items` already needed for a renamed grouping key.
- **A completely unconstrained node pattern binds nothing at all** (`MATCH (n) RETURN n`) — found while fixing the alias bug above, **not fixed**, and deliberately so. `lower_node` emits a triple pattern only for a label or an inline property; with neither, the pattern is an empty BGP, which is SPARQL's one-row identity, and `?n` is never bound to anything the query can return. Fixing it needs an actual design decision — what "any node" means over a triple store with no universal `rdf:type` catch-all — not a rushed patch alongside Slice E/F. Pinned by `an_entirely_unconstrained_node_binds_nothing`, a characterisation test documenting today's behaviour rather than changing it. **This is a pre-existing Slice B gap, not something Slice E or F introduced** — it was simply never caught, because no test before this slice ran a lowered query end-to-end. Needs its own slice: likely a wildcard triple pattern (`?n ?p ?o`, `DISTINCT`-projected on `?n`) or a documented refusal, whichever a dedicated RED test decides.

## Explicitly deferred (with destination)

- **Cypher write clauses** → never (decision 3). Writes go through the catalog API.
- **`CALL` and procedures** → would be an extension surface with its own security model.
- **openCypher / GQL conformance** → a subset is documented; formal conformance is not a goal (`00a-product-position.md`).
- **Cypher-specific shortest-path functions** → Epic 7a exposes these directly as API operations, which is a better fit than a query-language function.
- **Gremlin / TinkerPop** → no. Bolt (Epic 7d) reaches the larger tool population for one implementation; a second wire protocol and traversal language needs its own demand signal. See `00e-crate-architecture.md`.
- **Full GQL conformance** → the subset targets GQL's shape without claiming conformance (`00a-product-position.md`).
- **GraphQL** → a different thing that sounds similar: an API query language, not a graph query language. Out of scope, and worth saying so because the names collide.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. 2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. Parse and lowering stages have **zero I/O dependencies** — asserted.
5. **Cross-language authorization equivalence verified** (Slice E) — a divergence here is a data leak.
6. Epic 7's homomorphism test and this epic's isomorphism test both pass (Slice C).
