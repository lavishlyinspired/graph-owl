# Plan: `explain()` — P10's third MCP intelligence tool

**Branch**: main (committed directly, per `CLAUDE.md`)
**Status**: Shipped 11 August 2026, continuing "keep going tool by tool and
complete P10."
**Crates**: `graph-owl-mcp` (the tool), `graph-owl-server` (a real-Postgres
proof, reusing `reasoning.rs`'s own ontology fixture).

## What was built

- `ContextSource::explain(principal, subject, predicate, object)` — wraps
  `Catalog::explain_fact`, the same capability the pre-existing
  `GET /reasoning/explain` route already serves.
- `FactExplanation` — a bare `serde_json::Value` inside a typed envelope
  (`explanation`, `truncated`, `truncationReason`), not a typed recursive
  tree. `graph_owl_reasoning::Explanation`/`graph_owl_core::flake::Flake`
  deliberately have no `Serialize` impl (the same convention `Sid`/`EdgeRef`
  already follow throughout this crate), so `explanation_json`/`flake_json`
  are hand-written free functions rendering the *identical* shape
  `GET /reasoning/explain`'s own `explanation_body`/`flake_body` already
  produce — kept as their own copy in `graph-owl-mcp` rather than shared,
  because this crate does not depend on `graph-owl-server` and the source
  types cannot derive `Serialize` to share via `serde_json::to_value`
  either.
- `EXPLAIN` tool declaration taking `subject`/`predicate`/`object` as
  **IRIs**, resolved via `Sid::from_iri` at the dispatch layer
  (`required_sid`) before `explain` is ever called — a deliberate departure
  from the HTTP route's own `namespace:name` compact form, chosen because
  an agent's most natural source for these values is a prior `traverse`/
  `find_evidence` result, which already returns IRIs.
- `impl budget::Fits for FactExplanation` — **no lever shrinks this
  payload**, by design: an explanation's chains and premises are the
  fact's whole justification, and dropping one would not present a smaller
  true answer, it would present a wrong one (a derivation missing a step).
  `graph_owl_reasoning::Budget` already bounds how deep/wide the search
  goes before this type exists; an oversized result is returned accurate
  and over budget rather than shrunk and misleading.
- `CatalogContext::explain` — the real production adapter, a direct port
  of the HTTP handler's own assembly (`explain_fact` → `explanation_json`).

## The authorization decision — the same rule as `find_evidence`, not re-derived

`Catalog::explain_fact` takes no principal. The HTTP route this wraps
already discards the one its own `Auth` extractor requires
(`Auth(_principal)`), so this tool inherits that route's existing posture
rather than inventing a new one — the general rule `105l` already stated:
a capability that already ships over HTTP inherits that route's posture; a
capability with no prior HTTP exposure (`traverse` vs. `105j`'s
`graph_context`) needs its own fresh authorization analysis.

## Mutation report

**`lib.rs`'s dispatch, argument parsing, `Fits` impl, and rendering
functions** — `--in-diff`, `--lib` scoped: **8 of 8 (of the mutants that
compile) caught, 3 unviable, 0 missed**, after two rounds. First round
found 3 MISSED, all involving code the `Fixture` test double bypasses:
`explanation_json`/`flake_json` are never called by the unit-level
dispatcher tests at all (`Fixture::explain` hands back an already-built
`serde_json::Value` rather than routing a real
`graph_owl_reasoning::Explanation` through the rendering functions), and
`FactExplanation::render()`'s only dispatcher-reachable caller
(`budget::fit`'s internal size estimate) never affects what is actually
returned to the client — `jsonrpc.rs` serializes the struct directly via
`Serialize`, and every dispatcher test here stays under the default
budget regardless of what the estimate said. Closed with three direct
tests: `render()` asserted against its own output, and
`explanation_json` exercised against all four `Explanation` variants
(`Asserted`, `Circular`, `Unknown`, `Derived` with a nested premise),
checked against the exact JSON shape `GET /reasoning/explain`'s own
handler already produces.

**`catalog.rs`'s production adapter** — scoped to a new real-Postgres test
reusing `reasoning.rs`'s own three-level ontology fixture
(`seed_ontology`: `payments` → `PiiTable` → `SensitiveTable` →
`GovernedTable`), proving `CatalogContext::explain` recurses to the same
depth and shape the HTTP route's own existing test
(`a_derived_fact_explains_all_the_way_down_to_assertions`) already proves
for `/reasoning/explain` directly. **Clean result — zero open gaps**:
`cargo mutants` found exactly two candidates for the method, matching the
same shape `105j`'s `graph_context` report found for a different reason.
`Ok(Some(Default::default()))` is unviable (`FactExplanation` derives no
`Default` — deliberately, the same reasoning `TraversalContext`/
`EvidenceContext` already established), and the sole remaining candidate
(replacing the whole function body with `Ok(None)`) is caught by the new
real-Postgres test's own `.expect("the fact is derived")`. Unlike
`traverse`/`find_evidence`, this method has no hop-count or bounds
parameter and no branching beyond the two `match` arms `explain_fact`'s
own `Result` already forces — there is structurally less surface for a
fixture-shaped gap to hide in.

## What this deliberately does not do

- **No change to `graph-owl-server`'s existing `explanation_body`/
  `flake_body`.** Both are duplicated rather than shared, matching the
  established, deliberate convention that flake-rendering code lives at
  each boundary rather than behind a shared `Serialize` impl. A future
  refactor extracting one shared renderer is legitimate but out of scope
  for adding a tool.
- **No IRI round-trip test against a runtime-registered pack namespace.**
  The fixture uses the built-in `dsc:` namespace
  (`https://graph-owl.dev/ns/catalog#`), which `Sid::from_iri` resolves
  without any runtime registration — proving the parsing path works, not
  proving it works for a GST-pack-shaped IRI specifically. The dispatch
  logic (`required_sid` → `Sid::from_iri`) is identical either way, and
  `Sid::from_iri`'s own chaining of runtime namespaces is `traverse`
  /`find_evidence`'s inherited, already-relied-upon behavior, not new
  code this slice adds.
