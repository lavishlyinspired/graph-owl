# Plan: SPARQL Federation — `SERVICE` (Epic 101)

**Status**: Not started — **scheduled**
**Depends on**: Epic 7 (algebra and executor), Epic 13 (authorization)
**Crates**: `graph-owl-query`

## Goal

Join graph-owl's graph against an external SPARQL endpoint in one query.

## Why it is cheaper than it looks, and more dangerous

**Cheap**: `spargebra` already parses `SERVICE` into a standard algebra node
(`07` decision 8), and SPARQL 1.2 Federated Query reached Candidate
Recommendation on 7 April 2026 — the most stable part of the 1.2 suite. There is
no language work. What remains is an executor for one node type: send a
sub-query over HTTP, receive SPARQL Results, feed the bindings into the join.

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

## Explicitly deferred

- **Federated `UPDATE`** → writing to a remote store from a query. No.
- **Endpoint discovery / service descriptions** → SPARQL 1.2 Service Description
  is a Working Draft, and an allow-list does not need discovery.
