# Plan 111 — Engine → API → UI: the capabilities that stop at the second arrow

**Status**: complete — all six slices shipped, 14 August 2026. **Branch**: main. **Trigger**: a capability assessment
(`missing core and ui capabilities`) arguing that the product "has not yet
become a deeply connected knowledge graph", and that the larger gap is not
missing engine work but capability that no human can reach.

## The rule this plan is built on

> **If deleting `packs/gst/` and installing `packs/healthcare/` would break a
> feature, that feature is in the wrong layer.**

Plan 110 established the weaker version of this for the console. This plan
applies it to the whole stack, and adds the arrow that keeps getting dropped:

```
ENGINE ──▶ API ──▶ UI
              └──▶ MCP
```

A capability that stops at `API` is reachable by an integrator who reads the
OpenAPI document and by nobody else. Plan 110 called that "a private function
with an HTTP route". **This plan finds the same defect one layer down: a
capability that stops at `ENGINE` is a tested Rust function with no callers.**

## What the assessment got right, measured against the tree

Every claim below was checked rather than accepted. Three were wrong in the
product's favour and are recorded here so they are not re-scheduled.

### Already closed — do not re-propose

| Assessment says | Actual state |
|---|---|
| OWL EL unused, no UI | `ReasoningPanel` — profile detection, EL classify, explain-why. Plan 110 Slice 1 |
| Data-quality tests invisible | `QualityPanel` — definitions, cases, contracts. Read-only, deliberately |
| Metrics invisible | `MetricsPanel`, with the server-computed `gaps` column |
| Recertification/label queues invisible | `GovernanceQueues`, and label suggestions are actionable |
| Graph analytics has no route | `GET /assets/{id}/analytics`, reached by `api.assetAnalytics` |
| Entity resolution has no UI | `/resolution/queue` — list, confirm, reject, bulk — all reached |
| Drift has no UI | `/drift`, `/drift/{id}/apply`, `/drift/{id}/ignore` — all reached |
| Lineage has no UI | `/lineage/asset/{id}` with upstream/downstream depth |
| Ontology alignment unexposed | `/alignments`, `/alignments/review` — reached |
| GST matching is naive | Normalized `invoiceKey`, PAN-level supplier matching, head-wise tax comparison and a ≤₹1 de-minimis floor all shipped (`plans/109`) |

`PageRank`, community detection, graph embeddings, KG completion, federated
SPARQL and OWL QL are **correctly** identified by the assessment as things not
to build for their own sake, and this plan schedules none of them.

### Genuinely open, and verified

| # | Gap | Evidence it is real |
|---|---|---|
| 1 | **Path finding has no caller anywhere** | `TraversalEngine::shortest_path` and `all_paths` are implemented and integration-tested in `graph-owl-engine-postgres/tests/traversal.rs`. `grep` across `crates/` finds **no** `Catalog` method, **no** route, **no** MCP tool, **no** console call. The engine is paid for and unreachable |
| 2 | **Time travel reaches the explorer and not the query surface** | `asOf` is accepted on `GET /assets/{id}`, `GET /assets/{id}/graph`, `POST /sparql` and `POST /cypher`. The console has a global clock (`TimeControl`), a `?asOf=` deep link, and a graph comparison built on `graph/diff.ts` — **that half is done and this plan does not re-propose it.** What is missing is the query surface: `api.sparql()` takes one argument, the query, and cannot ask a question of the past at all |
| 3 | **`POST /cypher` has no console caller** | The workbench is SPARQL-only; `grep cypher ui/src/api.ts` is empty |
| 4 | **Contradictions have no surface** | `GET /assets/{id}/contradictions` and `/contradictions/reviews` exist. `grep -il contradiction ui/src/` matches one unrelated file |
| 5 | **Reconciliation never invokes the pack's blocking strategies** | `[[matching.blocking]]` is declared in both packs and read by resolution; `reconcile_pack` runs SPARQL only. Already recorded as unbuilt in `plans/109` |

## Why path finding is first, and it is not close

Everything this product currently answers is a *pattern* question: something is
asserted, or something is absent. `shortest_path` answers a different kind of
question — **"how are these two things connected at all?"** — and neither the
asker nor the product needs to know the shape of the answer in advance.

That is the difference between a reconciliation tool and a knowledge graph, and
the assessment is right that it is where the graph earns its keep. It is also
the cheapest item in the list: the engine is done and tested, so the work is a
facade method, a route and a screen.

**Domain-neutral by construction.** Two node ids, a direction, a hop cap, a
path cap, an optional relationship filter, an optional time. GST asks
*invoice → supplier → filing*; healthcare asks *patient → encounter →
medication*; banking asks *account → transfer → counterparty*. Nothing in the
mechanism names any of them.

---

## Slice A — Path finding, engine to screen ★

**RED first**: `Catalog::find_paths` returns the route between two connected
nodes, `None`-equivalent for two unconnected ones, and refuses a pair the
principal cannot see.

1. **`Catalog::find_paths`** (`graph-owl-api`), modelled exactly on
   `asset_subgraph`: authorize **both** endpoints against relational state
   before walking (decision 7 — the projection lags, so a revoked permission
   must not be honoured by a graph read), resolve `as_of` through
   `graph.time_at`, then delegate to `shortest_path` or `all_paths`.
2. **`POST /graph/paths`** — `{from, to, direction, maxHops, maxPaths,
   relationshipTypes?, asOf?}`. `maxPaths` is a hard stop, not a hint: path
   enumeration in a dense graph is exponential and the alternative is a request
   that runs until something else times out. RFC 9457 on every rejection.
3. **Console: "How is this connected?"** under Explore. Two node pickers, the
   controls above, and the answer as an ordered chain per path — the shape the
   assessment draws, and the shape a reviewer can actually read.
4. **Unconnected is an answer, not an error.** The trait already says so
   (`Ok(None)`); the route must return `200` with an empty path set and the UI
   must say "no route within N hops" rather than showing a failure.

**Acceptance**
- Two connected nodes return the route, not just its length.
- Two unconnected nodes return `200` and an empty set.
- A principal who cannot see one endpoint gets the same answer as if it did not
  exist — no existence oracle.
- `maxPaths` truncation is reported, not silent.
- A relationship-type filter changes the answer, and the filter values come
  from the caller, never from a list compiled into Rust.

**Mutants to watch**: the `>` in the hop bound (off-by-one makes a 3-hop route
appear at `maxHops = 2`); the direction flag (`follows_outgoing` inverted still
finds symmetric routes — the negative test needs an asymmetric graph); the
truncation flag inverted (a truncated answer presented as complete is the
dangerous direction).

## Slice B — Ask a question of the past, and ask it in either language

**Corrected after checking the tree rather than the assessment.** The console
*does* time-travel: `TimeControl` is a global clock, `?asOf=` deep-links, the
asset detail and `GraphExplorer` both honour it, and `GraphExplorer` already
compares two instants through `graph/diff.ts`. The assessment's "temporal UI is
the most important missing capability" is half wrong, and the half it gets
right is narrow and specific:

1. **`api.sparql(query, asOf?)`.** The workbench cannot ask a question of the
   past at all, which is the surface where a practitioner would most want to —
   *"run this same query as it stood before the import"* is one parameter away
   and unreachable.
2. **`api.cypher(query, asOf?)`, and a workbench that offers both languages.**
   `POST /cypher` has no console caller whatsoever. A property-graph query
   language is a headline capability of this product reachable only by curl.
3. **The clock's stated scope is updated with it.** `TimeControl`'s own comment
   says "`?asOf=` is currently answered for a single asset read" — honest when
   written, wrong once the workbench honours it, and a stale scope note is
   worse than none because it is read as authoritative.

**Domain-neutral**: transaction time is a property of the store, not of the
domain. GST reads it as "before and after the supplier filed"; healthcare as
"before and after the encounter was coded".

**Acceptance**: the same query at an `asOf` before an import returns fewer rows
than at now; an invalid `asOf` is a `400` naming the field; a Cypher query
returns rows through the console; the language choice survives a reload.

## Slice C — Contradictions

`/assets/{id}/contradictions` and `/contradictions/reviews` into
`ReviewSection`'s existing generic queue abstraction — it already has five
implementations, so this is a sixth rather than a new pattern.

Two sources asserting incompatible values about one subject is a *graph-native*
finding: it needs named graphs and provenance to even be expressible, which is
exactly the architecture this product has and a row-store does not.

**Acceptance**: both asserted values are shown with the named graph and source
document that carried each; adjudication records who decided and why; an empty
queue and an unreachable one look different.

## Slice D — the pack's blocking strategies get run ✅ shipped

Expressed neutrally: **a rule that finds nothing and a rule that found a
near-match it could not confirm are different answers, and they looked
identical.**

**The gap was worse than `plans/109` recorded.** It was not that reconciliation
declined to call blocking — `graph_owl_core::blocking_strategy`, 963 lines and
38 tests of domain-neutral blocking, had **no callers anywhere in the
workspace**. Both shipped packs have declared `[[matching.blocking]]` since
Epic 105 and nothing read it. Same defect as `shortest_path`, one crate over.

What shipped:

1. **`Strategy` can be deserialized at all.** Its own doc comment promised
   "the configuration *is* the strategy, with no translation step where a
   domain name could sneak in" and it carried no `Deserialize` derive.
2. **`pack_install::read_blocking_strategies`** reads `[[matching.blocking]]`
   at request time, like `read_console_config` reads `[console]`.
3. **`Catalog::blocking_candidates`** keys the subject and every candidate,
   bounded, reporting truncation, and returns *which strategy* agreed.
4. **`POST /packs/{pack}/candidates`**, with prefix resolution in the handler
   so the facade only ever sees `1024:partyName`.

### Three defects this found, each invisible until the code ran

- **`ngram` vs `n_gram`.** `rename_all = "snake_case"` derives `n_gram`; both
  packs write `ngram`. The packs are the contract, so the wire name is pinned
  with an explicit rename. Caught by a round-trip test written against a real
  pack's own shape rather than a convenient one.
- **`NGram::key` cannot find a transposition, and its doc comment said it
  could.** The key joins the whole sorted window set, so *any* changed window
  changes the key — a transposed identifier never blocks with its correction,
  which is the exact case the variant was added for. `Strategy::keys` is the
  fix: index under each window, which is what n-gram blocking means.
- **The test that appeared to check ordering compared a record with itself.**
  `key(x) == key(x)` is true of every function. That tautology is why the
  point above survived from Epic 105 to now.

### Closed by Slice F

`reconcile_pack` consulting candidates was left open here and is now done —
see Slice F below, which puts them where a reviewer actually looks rather than
into the recorded finding.

## Slice E — Packs declare their own upload surfaces ✅ shipped

`[console.reconciliation]` and `[[console.queues]]` proved the pattern; the
*import* surfaces had never moved. `ui/src/features/packs/packSurfaces.ts`
held a `REGISTRY` constant containing GST's file list — its keys, its labels,
the sentence telling a user where to download each file — so a second pack's
uploads needed a React change. That is exactly the test this plan applies to
itself: *delete `packs/gst/`, install `packs/healthcare/`, does it still work
without changing Rust, server logic or React?*

`[[console.imports]]` now carries the key, label, description, `accept` and
where-to-obtain prose. The console prefers a pack's declaration and falls back
to the registry for a pack installed before it declared its files.

**The honest boundary, stated rather than discovered later: a parser is code.**
A pack cannot declare a CSV reader in TOML, so `format` selects a reader the
console implements (`csv`, `gstr1-json`, `gstr2b-json`). **A format this
console has no reader for is named on screen, not silently dropped** — a
surface that quietly disappears looks like a pack that forgot to declare it,
where the truth is usually that the console build is older than the pack it is
serving.

**One guard is worth more than the feature.** `read_console_config` camelCases
the whole table, so `how_to_obtain` must arrive as `howToObtain`; a test reads
the *shipped* pack and asserts it does. That exact key-name failure has
happened here once already — `match_key` reached the console under a name
nothing read, and the reconciliation silently fell back to matching on the
printed identity, which looks like a working page.

**One equivalent mutant, recorded rather than chased.** Stryker replaces
`config?.imports ?? []` with `["Stryker was here"]`; a string has no `.format`,
so no reader matches and the result is still `[]`. Contorting a test to kill it
would test the mutation rather than the behaviour.

## Slice F — the candidates reach the reviewer ✅ shipped

Slice D made the pack's blocking strategies runnable and left one thing open:
nothing in the finding pipeline consulted them. Slice F closes it, and the
placement is the decision worth recording.

**Not attached to the recorded finding — computed when the finding is
opened.** Attaching candidates at reconcile time would bake a judgement into
stored evidence, re-run a blocking scan per finding written rather than per
finding read, and go stale the moment the next import lands. `GET
/findings/{id}/evidence-graph` already assembles the picture a reviewer looks
at; the candidates belong in that assembly.

**A separate wire field from `nearMiss`, deliberately.** They are different
claims, and this plan has refused to flatten strengths of evidence at every
step: `nearMiss` means *the rule declared a similarity band and a value
matched exactly*; a candidate means *a blocking key collided*. The first is
close to an assertion, the second an invitation to look. Each candidate
carries `by` — which strategies agreed — because "an n-gram key collided" and
"a normalized key collided" change what a reviewer does next.

What shipped:

1. **`Catalog::finding_subject`** — the subject as a *resolved* `Sid` and the
   pack that raised it. A key computed against an identity the graph cannot
   resolve matches nothing and reads as a clean result, so an unresolvable
   namespace is `None` rather than a fabricated id.
2. **`surviving_candidates`** — a node the walk already drew is not a
   candidate, it is a node; the near miss is excluded because it is already on
   screen carrying a *stronger* claim, and the same record shown twice at two
   strengths teaches a reviewer to trust neither.
3. **Two caps, both stated**: 1,000 subjects scanned, 5 shown. Past a handful
   the list stops being "look at these two" and becomes a second queue.
4. **Console**: a "Might be the same record" section in the findings queue's
   evidence panel, strategy tags first — the tag is what tells a reviewer how
   much weight the row carries, and burying it after the provenance would make
   every candidate read alike.

### The defect this found in the shipped GST pack

**Every strategy the pack declared keyed a *supplier*, and every finding it
raises is about an *invoice*** — so blocking could never fire on a finding, no
matter how well the machinery worked. Found by wiring the candidates into the
panel and getting an empty list for a scenario that plainly has a near-miss in
it. The pack now declares an n-gram over `gst:invoiceNumber`: the
reconciliation's own join already matches identical numbers, and what no exact
join can see is a *mistyped* one.

This is the third time in Plan 111 that running a capability for the first
time found the declaration around it to be wrong — after `ngram`/`n_gram` and
after `NGram::key`'s doc comment. **A capability nobody calls is not merely
unused; the configuration and documentation around it decay unobserved.**

## Not scheduled, and why

- **Whole-graph analytics, PageRank, community detection** — Epic 38's purity
  boundary forbids unbounded whole-graph computation on a synchronous request,
  and the assessment agrees these are low-value here.
- **Federated SPARQL, OWL QL** — real capability, no asked-for use.
- **Incremental reasoning** (the assessment's Tier 2 item 9) — genuinely
  valuable and genuinely large; it is its own plan, not a slice of this one.
- **A "legal knowledge graph"** — `governed_by` already carries the citation on
  every finding. Making provisions first-class nodes is a pack change, not a
  platform one, and belongs in the pack's own plan.

## The test every slice must pass

1. Would this work if the only installed pack were hospitality?
2. Does the pack declare it, or does the console assume it?
3. What does a deployment with **no** pack see? An honest empty state.

`scripts/check-namespace-neutrality.py` fails the build on a violation of (1).
