# Plan: SPARQL Federation — `SERVICE` (Epic 101)

**Status**: Not started — **scheduled**
**Depends on**: Epic 7 (algebra and executor), Epic 13 (authorization)
**Crates**: `graph-owl-query`

## Goal

Join graph-owl's graph against an external SPARQL endpoint in one query.

## Why it is cheaper than it looks, and more dangerous

**Cheaper still than first written**: `spargebra` parses `SERVICE`, and
`spareval` takes a **`ServiceHandler`** — so there is no executor to write
either. What this epic supplies is one trait implementation, and everything
below is *what that implementation must enforce*. SPARQL 1.2 Federated Query is
at Candidate Recommendation (7 April 2026), the most stable part of the 1.2
suite.

**Dangerous**, in three ways that are the actual content of this epic:

1. **It is an outbound network call from a query.** A slow endpoint makes
   graph-owl slow; a hanging one makes it hang. Every other operation here is
   bounded by a budget this process controls, and this one is not.
2. **It sends data outward.** A join ships graph-owl's bindings to the remote
   endpoint as filter values. Those bindings may be metadata the caller is
   permitted to see and the *remote operator* is not. This is a data-exfiltration
   path wearing a query's clothes.
3. **The remote answer has no provenance the caller can assess.** Results merge
   with local ones and look identical.

## Resolved decisions

1. **Endpoints are allow-listed by configuration, never by the query.** A
   `SERVICE <https://anywhere>` naming an arbitrary URL is an outbound request
   composed by whoever wrote the query. The allow-list is administrative
   configuration, and an unlisted endpoint is refused by name.
2. **Authorization applies before bindings leave.** Epic 13's predicate filters
   what a caller can see, and only what survives it may be sent outward. Filtering
   the *result* instead would mean the denied values were already transmitted.
3. **Every federated call is budgeted and its own timeout.** `SILENT` is honoured
   per spec — a failed `SERVICE` yields empty rather than failing the query — but
   the result is **marked** as having a silenced failure. Silent-and-invisible
   turns a network problem into a wrong answer.
4. **Federated results are tagged with their source** in the result metadata, so
   a caller can tell which endpoint contributed a row.
5. **No `SERVICE` inside a constraint or a reasoning rule.** Epic 96 could
   otherwise make validation depend on a third party's uptime, and Epic 6 could
   derive facts from one. Derived facts must be reproducible from local state.

## Acceptance criteria

- [ ] A `SERVICE` against an allow-listed endpoint joins correctly.
- [ ] An unlisted endpoint is refused, naming it and the allow-list.
- [ ] A timeout is bounded and reported, not inherited from the client's
      patience.
- [ ] `SILENT` yields empty **and** the result records the silenced failure.
- [ ] Bindings denied by policy are never transmitted — asserted by capturing
      the outbound request in a test double and inspecting it. **The important
      test in this epic**: it is the only way to prove a leak did not happen,
      since a result-side assertion passes even when the data already left.
- [ ] Results name their contributing endpoint.
- [ ] **Console: the workbench result grid attributes every remote row to its
      endpoint, and a silenced `SILENT` failure is visible in the result, not
      only in a log.** This epic names unattributable remote results as one of
      its three dangers; a grid that renders a remote row identically to a local
      one *is* that danger, just rendered. A `SILENT` failure is worse on screen
      than in an API response — an empty region of a result grid reads as "no
      such data" rather than "we could not ask", which is the same
      absence-versus-omission confusion `40-ui-graph-explorer.md` treats as its
      most damaging bug.
- [ ] **Console: the endpoint allow-list is an admin surface** with the same
      dry-run treatment as Epic 13's policy editor — adding a federation
      endpoint is granting the query engine permission to make outbound calls
      with the caller's bindings, which is a policy decision wearing a
      configuration costume.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR.

**Technical grounding, checked before writing slices** (against `spareval`/`spargebra`'s own published API, not the W3C spec — the executor is adopted, not hand-built): `spareval::DefaultServiceHandler` is a **synchronous** trait (`fn handle(&self, service_name: &NamedNode, pattern: &GraphPattern, base_iri: Option<&Iri<String>>) -> Result<QuerySolutionIter<'static>, Self::Error>`), registered via `QueryEvaluator::with_default_service_handler`. `spargebra::algebra::GraphPattern` implements `Display`, producing valid SPARQL syntax for the pattern body — wrapping it as `SELECT * WHERE { <pattern> }` is a complete federated query. `sparesults::QueryResultsParser` parses a remote endpoint's `application/sparql-results+json` response directly into the `QuerySolution`s `QuerySolutionIter::new` expects. Because `execute_algebra` (`graph-owl-api`) runs `spareval` synchronously inside an `async fn` with no `spawn_blocking` (unlike `cypher_stream`, which already isolates blocking work that way), the HTTP call inside `handle()` needs the same isolation — reusing that established pattern rather than introducing a second way to bridge sync/async.

### Slice A: An unlisted endpoint is refused, by name

**Value**: The single most important safety property in this epic — a `SERVICE` clause cannot make an arbitrary outbound call — lands before any capability that could leak data exists to misuse.
**Path**: A deployment-configured allow-list (a `Catalog` construction-time field, the same shape `SparqlBudget` already uses — "raised deliberately per deployment, not per query, because a caller who could raise their own budget does not have one") → a `FederationServiceHandler` implementing `DefaultServiceHandler`, wired into `execute_algebra`'s evaluator via `.with_default_service_handler(...)` → a query naming an endpoint not on the list fails with a message naming the endpoint and the configured list.
**Acceptance criteria**: a `SERVICE <https://not-allowed>` query returns an error naming `https://not-allowed` and the allow-list; a `SERVICE` against an allow-listed endpoint reaches the handler (real HTTP still returns "not yet supported" — Slice B's job) rather than being refused for the allow-list reason.
**RED**: A query with an unlisted endpoint asserts the specific error message names the endpoint. Mutator watch: an allow-list check inverted (`contains` → `!contains`) must fail a test with an *allowed* endpoint reaching the handler.
**Done when**: criteria met, mutation report reviewed. Met (2026-08-05).

### Slice B: A SERVICE against an allow-listed endpoint joins correctly, bounded by a timeout

**Value**: The actual capability — real federation.
**Path**: `FederationServiceHandler::handle` serializes the pattern (`SELECT * WHERE { pattern }`), issues an HTTP GET/POST via the SPARQL 1.1 Protocol inside `spawn_blocking` (bridging the sync trait method to the async `reqwest` client already in the workspace), parses `application/sparql-results+json` via `sparesults`, and returns a bounded `QuerySolutionIter`. A configurable per-call timeout (deployment-level, same shape as the allow-list) bounds the wait.
**Acceptance criteria**: a query with `SERVICE <mock-endpoint> { ?s ?p ?o }` against a local test HTTP server returns the mock's rows, joined with any local pattern in the same query. A timeout returns a clear error rather than hanging the request.
**RED**: An integration test spinning up a local HTTP server (axum, matching every other test double in this codebase) returning a fixed SPARQL JSON result set; the federated query's rows include it. Second RED: a server that never responds — the query fails at the configured timeout, not the caller's patience. Mutator watch: a timeout duration read but never applied must fail the slow-server test.
**Done when**: criteria met, mutation report reviewed.

### Slice C: SILENT is honoured and marked; results name their endpoint

**Value**: A network failure must never look like "no such data" — the plan's own named danger.
**Path**: `SERVICE SILENT <endpoint>` on a failed call yields an empty result for that pattern (per SPARQL 1.1 spec) rather than failing the whole query, but `SparqlOutcome` gains a field recording which endpoints silently failed. Every row contributed by a `SERVICE` clause is tagged with its source endpoint in the outcome.
**Acceptance criteria**: a `SILENT` service that fails to connect returns an otherwise-successful query with zero rows from that clause **and** a populated "silenced failures" list naming the endpoint. A non-`SILENT` failure fails the whole query. Rows from a federated join carry their endpoint.
**RED**: A silent failure against an unreachable mock server asserts both "query succeeds" and "silencedFailures contains the endpoint" — a test that only checked one half would pass a fix that dropped the other. Mutator watch: `SILENT` flag read but not threaded through must fail a test where a *non*-silent failure is asserted to fail the whole query.
**Done when**: criteria met, mutation report reviewed.

### Slice D: Bindings denied by policy are never transmitted

**Value**: **The important test in this epic**, per the plan's own words — the only way to prove a leak did not happen, since a result-side assertion passes even when the data already left.
**Path**: A test double replacing the real HTTP transport captures every outbound request verbatim. A query joining a policy-filtered local pattern (an asset the caller may not see) against a `SERVICE` clause is run as a principal denied that asset, and the captured outbound request is asserted to never contain the denied value — provable because `execute_algebra`'s existing ordering (`scoped_facts` runs and filters *before* `spareval` ever executes) means the evaluator never holds a denied fact in the first place, so `FederationServiceHandler` has nothing to leak; this slice is the regression guard proving that ordering holds under federation too, not a new filter.
**Acceptance criteria**: the captured outbound request body contains only the `SERVICE` clause's own static pattern text — never a value from a locally-scoped-out fact.
**RED**: Two principals, one denied a specific asset; the query joins that asset's local data against a `SERVICE` clause; the outbound capture is inspected byte-for-byte for the denied value's absence, for both principals (so the test can't pass by never containing *any* local value).
**Done when**: criteria met, mutation report reviewed.

### Slice E: Console — result attribution, SILENT visibility, allow-list admin with dry-run

**Value**: An unattributable remote row and an invisible `SILENT` failure are this epic's own named dangers, rendered on screen exactly as dangerous as they are in the API.
**Path**: The workbench result grid tags federated rows with their endpoint (reusing the five-pattern design's existing badge/tag idiom) and surfaces `silencedFailures` as a visible warning, not a silently-empty region. A new Admin tab lists allow-listed endpoints with add/remove and a dry-run ("what would a query against this endpoint be permitted to reach") mirroring the Policies panel's own dry-run-before-save shape.
**Acceptance criteria**: a federated row is visually distinguishable from a local one with its endpoint named; a `SILENT`-failed clause shows a warning naming the endpoint, not an empty table region read as "no data"; the allow-list is addable/removable from Admin.
**Done when**: criteria met, live-verified against the real stack (agent-browser, matching every other console slice this session).

## Explicitly deferred

- **Federated `UPDATE`** → writing to a remote store from a query. No.
- **Endpoint discovery / service descriptions** → SPARQL 1.2 Service Description
  is a Working Draft, and an allow-list does not need discovery.
