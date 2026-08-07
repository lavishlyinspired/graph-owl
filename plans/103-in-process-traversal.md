# Plan: In-Process Traversal (Epic 103)

**Status**: **Built, tested, and measured, 8 August 2026 — still not justified for production routing, but the adapter, its differential suite, and a real Postgres bug fix it surfaced are shipped and stay.** See "Built and measured, 8 August 2026" below; the original 30 July 2026 entry-condition measurement is kept as history immediately after it, since it asked a different, now-superseded question.
**Depends on**: Epic 7a (the `TraversalEngine` port), Epic 37a (the trigger)
**Crates**: `graph-owl-traversal-memory` (the second adapter), `graph-owl-engine-postgres` (one bug fix in the first adapter's SQL, found while measuring this epic)

## Goal

Make deep walks fast by extracting a bounded subgraph into memory and walking
it there, instead of asking Postgres to recurse further than a recursive CTE
does well.

## What the measurement found (30 July 2026) — the gate said no

The hypothesis below is that `NOT dst = ANY(path)` dominates, and that the win
therefore **grows with depth**. It was tested directly, and it does not.

A synthetic tree — branching factor 3, 60,000 edges, so the walk considers
roughly `3^d` rows at depth *d* and each pays a scan proportional to its own
path length. A tree deliberately, so the guard **never fires**: removing it
changes the cost and not a single row, which makes the comparison controlled.

| depth | with path tracking | without | ratio |
|---|---|---|---|
| 5 | 0.78ms | 0.66ms | 1.2× |
| 7 | 5.72ms | 2.51ms | 2.3× |
| 9 | 32.1ms | 18.8ms | 1.7× |
| 11 | 86.7ms | 61.0ms | 1.4× |

**The guard costs a ~1.5× constant factor, and the ratio does not grow with
depth.** That is the finding, and it refutes the argument this epic rests on.

Why the reasoning was wrong: the array is *short* at these depths, and `= ANY`
over a handful of text elements is cheap next to the join and the tuple
materialisation. What actually dominates is **row count** — `3^d` — and an
in-memory walk pays that too, because it still has to visit every node. Moving
the visit into a process does not make there be fewer of them.

So petgraph would trade a 1.5× factor for maybe 1.2×, and buy in exchange: a
memory budget for the extracted subgraph, an extraction step on every query, and
the `as_of`/authorization hazard flagged below. Not worth it.

**What would change the verdict**, stated so the next person does not re-argue
it from first principles:

- **Paths much longer than 11.** The scan is linear in path length, so the
  constant grows with it. Catalog containment is 5 levels and lineage chains in
  practice are single digits; a workload with depth-30 walks is a different
  question.
- **A genuinely cyclic graph**, where the guard *fires* rather than merely being
  evaluated. Here it never fired, so this measures its evaluation cost and not
  the work it saves. Lineage is asserted acyclic (Epic 29), which is why a tree
  was the honest fixture.
- **Epic 37a at 10M+ flakes**, if row counts at that scale shift the balance
  between the join and the guard.

The published case studies (one reports 103s → 600ms) are consistent with the
first two conditions holding for *those* graphs. They were never targets, and
the measurement is why that caution was right.

## Built and measured, 8 August 2026

The adapter was built in full and measured honestly, per the standing
instruction for this epic: implement completely, report whatever the
measurement actually shows, including a reconfirmation of the verdict above.
It reconfirmed it — but not for the reason the 30 July measurement gave, and
along the way it found and fixed a real, independent bug in the CTE adapter
that had nothing to do with Epic 103 and everything to do with production
readiness.

**The adapter.** `graph-owl-traversal-memory::InMemoryTraversalEngine` — one
`TripleStore::query_pattern` fetch per call, a `petgraph::DiGraph` built from
it (decision 2's `DiGraph`, never `StableDiGraph`), then a BFS/DFS matching
each of `neighbours`/`subgraph`/`shortest_path`/`all_paths`/`detect_cycles`'s
Postgres CTE semantics exactly: `neighbours`' global-visited-set BFS is the
standard shortest-path argument for the SQL's per-path-guard-then-shortest-
per-node result, not a shortcut; `all_paths`/`detect_cycles` stay exhaustive,
path-guarded DFS because those two genuinely need every route, not just the
nearest.

**The differential suite.** The Postgres adapter's own 26-test suite
(`crates/graph-owl-engine-postgres/tests/traversal.rs`), ported verbatim to
`crates/graph-owl-traversal-memory/tests/traversal.rs` against the same
fixtures and assertions — only `store()` differs, an in-memory `TripleStore`
double instead of a Postgres container. All 26 pass unchanged. Ten more were
added for gaps mutation testing found that the ported suite's own fixtures
happen not to exercise: the `derived` flag (no ported test reads it),
`>` vs `>=` at every node/path/hop boundary (no ported test lands exactly on
one), and `all_paths(x, x, ...)` — a real, found-not-invented gap: the
adapter's first implementation had no notion of a trivial zero-length path
when `from == to`, while the Postgres frontier's own depth-0 base-case row
already covers it. Fixed to match. `cargo mutants` on the adapter: 0 missed
(70 mutants, 57 caught, 13 unviable) after two fix passes.

**`as_of` and the access predicate.** Authorization is not the traversal
engine's job for *either* adapter — `graph-owl-api::Catalog::walk_hop`
post-filters `TraversalResult::reached` against the caller's own visibility
set, one layer above whichever `TraversalEngine` is configured, precisely so
neither adapter has to know who is asking. A new test,
`the_real_in_memory_adapter_still_respects_the_access_predicate_per_principal`
in `graph-owl-api`, plugs the *real* `InMemoryTraversalEngine` (not the
existing suite's hand-scripted `FakeTraversal`) into that seam, walks a real
reified relationship, and runs the same Cypher query as two principals — one
whose policy covers the target, one whose does not. Passed first try, which
is the expected result of a check that lives above the adapter rather than
inside it: it had to hold, and does.

**A real bug, found while measuring, not while implementing.** The honest
performance comparison (`crates/graph-owl-engine-postgres/tests/
traversal_vs_memory.rs`, `#[ignore]`d — a timing comparison is not a
correctness gate and would cost every `cargo test` container-bound wall time
for no behavioural coverage; run by hand with `--ignored --nocapture`) seeds
one ternary tree (branching factor 3, matching the 30 July measurement's own
fixture shape) and times `neighbours` through both adapters on identical
data. The first run reported the in-memory adapter **99×–405× faster** —
not a plausible number for one full-table fetch plus an in-memory BFS
against a bounded-depth recursive CTE, and worth distrusting before
believing, per this project's own standing rule about numbers that disagree
with the rest of the evidence by an order of magnitude.

`EXPLAIN (ANALYZE, BUFFERS)` against the reconstructed SQL (on a database
holding the actual 9,840-edge tree) found it: `push_logical_edges`'s reified-
relationship half joined `live` to itself three times — once each for
`fromEntity`, `toEntity`, `relType` — and the planner, unable to estimate a
CTE scan's row count once a filter narrows it (`rows=25` estimated against
`rows=9,840` actual, a CTE-scan blind spot regardless of `MATERIALIZED`,
which was tried and did not help), chose a nested loop for the `relType`
lookup. At this scale that looped **19,680 times** over the full 39,360-row
`live` result — 96.8 million filtered row comparisons for a lookup that
should cost one pass. **31.8 seconds for a two-hop walk over fewer than
10,000 edges is a production incident waiting to happen**, not a rare edge
case; any deployment with a lineage graph at this scale would have hit it on
an ordinary query.

**The fix**: one aggregate pass over `live`, grouped by the relationship's
own subject, using `FILTER` clauses instead of a second and third self-join —
`MAX(value_ref_id) FILTER (WHERE sid_p = 'fromEntity')` and so on, with a
`HAVING` clause preserving the original inner join's "both endpoints must
exist" requirement and a `bool_or` preserving the original's `derived`
semantics exactly (verified by hand against three-valued-logic NULL cases,
then by two new regression tests — `a_reified_relationship_asserted_in_the_
reasoning_context_is_marked_derived` and `a_relationship_with_no_rel_type_
defaults_to_related` — since neither `derived` nor the `'related'` default
had *any* existing Postgres-side test before this). Verified equivalent by
`EXPLAIN`: identical row counts before and after (13 rows, both plans).
Verified non-regressing: all 26 pre-existing Postgres traversal tests plus
the crate's other three integration test files (retraction, registry,
flake round-trip) pass unchanged. **31.8s → 49.6ms — a 641× fix**, and it
belongs to Epic 7a's crate, not this one; recorded here because this is the
measurement that found it.

**The honest comparison, with the bug fixed.** Two scales, both ternary
trees, `neighbours` from the root at increasing `max_hops`, both adapters
against identical data through the identical connection pool:

| tree | edges | max_hops | Postgres CTE | in-memory | ratio |
|---|---|---|---|---|---|
| depth 8 | 9,840 | 1 | 39ms | 247ms | 0.16× |
| depth 8 | 9,840 | 8 (full tree) | 76ms | 247ms | 0.31× |
| depth 10 | 88,572 | 1 | 349ms | 2.08s | 0.17× |
| depth 10 | 88,572 | 10 (full tree) | 1.13s | 2.07s | 0.55× |

**The Postgres CTE is faster at every point tested, including an exhaustive
full-tree walk on 88,572 edges.** The in-memory adapter's per-call cost is
close to constant (~250ms at the smaller scale, ~2.07s at the larger) because
`fetch_edges` always pulls the *whole* live relation regardless of
`max_hops` — decision 2 refuses to keep a warm graph across calls, so this
extraction is paid every time, and at both scales it dwarfs the walk that
follows it. The CTE's cost grows with depth and dataset size, and the ratio
is closing (0.16→0.31 at the smaller scale, 0.17→0.55 at the larger) — but
closing is not crossing, and no crossover appeared within either tested
range, up to and including walking the entire tree.

**So the verdict is unchanged, and the reasoning is now precise rather than
superseded.** The 30 July measurement's hypothesis — that the `NOT dst =
ANY(path)` cycle guard dominates and the win grows with depth — was already
refuted by its own numbers. This measurement replaces that reasoning with
the real one: the CTE, once its incidental join bug is fixed, is fast enough
that the in-memory adapter's own fixed extraction cost never gets amortized
away within any range tested here. **What would change this verdict**,
restated for what was actually measured rather than guessed at: a workload
where one extraction serves many walks (refused by decision 2, on purpose),
or a dataset meaningfully past 88,572 edges — genuinely untested, not
inferred from this table.

## Why this is an adapter and not a rewrite

`graph-owl-traversal` already defines `TraversalEngine` — `neighbours`,
`subgraph`, `shortest_path`, `all_paths`, `detect_cycles` — and
`graph-owl-engine-postgres` already implements all five as recursive CTEs. This
epic adds a **second implementation of the same trait**. Nothing above it
changes: the explorer, the API and any future consumer keep calling the port.

That the port existed before the need is the whole reason this is cheap. It was
not designed for this; it just happens to be the right seam.

## The real reason a CTE degrades, verified against our own SQL

The frontier's cycle guard is:

```sql
AND NOT d.dst = ANY(f.path)
```

`= ANY(array)` is a **linear scan of the path array, per candidate row**. At
depth *d* with branching factor *b*, the walk considers roughly `b^d` rows and
each pays a scan proportional to its own path length. The cost is not the
recursion; it is the membership test inside it.

In memory that same test is a hash-set lookup — O(1) instead of O(path length).
That is the entire performance argument, and it is why the win grows with depth
rather than with graph size.

Published case studies report large factors (one replacing a path-tracking CTE
reports 103s → 600ms). **Those are other people's graphs on other people's
hardware and are not targets here.** They establish that the effect is real,
not how large it is for this workload — Epic 37a decides that.

## Resolved decisions

1. **petgraph is the algorithm engine, never the model.** Licence verified
   2026-07-28 from the crate's own `Cargo.toml` (petgraph 0.8.3):
   `MIT OR Apache-2.0`, which `00i`'s allowlist accepts. Not currently a
   dependency of this workspace — see the gate note below. It understands `node —edge— node` and nothing
   else: `owl:Restriction`, `owl:unionOf`, `owl:someValuesFrom`, cardinality and
   property chains have no representation in it. Those stay in the semantic
   layer — the flake store, the ontology model, the reasoner — and only a
   *projection* of them becomes a petgraph.

   The distinction matters because the tempting mistake is to make petgraph the
   knowledge graph. It cannot be: it would silently discard every axiom that is
   not an edge, and the loss would be invisible.

2. **`DiGraph`, not `StableDiGraph`.** Stable indices exist to survive
   insertions and removals in a **long-lived** graph. This project must not have
   one: an in-memory graph held across queries is a single graph at a single
   instant for every caller, which breaks `as_of` (whose instant?) and
   authorization (whose view?) simultaneously.

   The graph is therefore **built per query, from an already-filtered fact set,
   and discarded**. With no mutation after construction, index stability buys
   nothing and costs the indirection `StableDiGraph` carries. A long-lived
   editing session — the case stable indices are for — is not this.

3. **Extraction is filtered before it is built**, exactly as the SPARQL dataset
   is (`00l`). `as_of` and the access predicate apply to the scan that feeds the
   graph, so petgraph never holds a fact the caller may not see. Filtering the
   *walk* instead would mean the excluded nodes were already in memory and
   already influencing path lengths.

4. **The trade-off nobody names is extraction cost.** Pulling a subgraph into
   Rust costs a scan the CTE never pays. In-process wins only when the walk is
   deep *relative to* that scan — at one or two hops the CTE has answered before
   petgraph finishes loading. So the routing rule is not "deep queries go
   in-process"; it is "queries whose walk cost exceeds their extraction cost do".

5. **Both adapters stay.** Shallow walks keep using Postgres. This is a routing
   decision per query, not a replacement, and a deployment with no deep queries
   should never pay the extraction.

## A gate that is documented but not running

`00i` says dependencies are gated by `cargo deny` with a permissive-only
allowlist. **There is no `deny.toml` in this repository and no CI step invoking
it** (checked 2026-07-28). The gate is a document, not a control.

Every dependency taken so far happens to be permissive, so nothing is wrong
today — but that is the outcome of care, not of enforcement, and this epic is
the first to propose a *genuinely optional* new dependency. Adding petgraph
without the gate would mean the first crate anyone adds under time pressure gets
whatever licence it has.

**So the gate lands before the dependency does.** `deny.toml` with the `00i`
allowlist, wired into CI, is a prerequisite of this epic's first commit — a
small piece of work that has been owed since `00i` was written.

### Shipped, 8 August 2026 — and it was overdue, not a formality

**`petgraph` had already been added by then, for Epic 38, before this
prerequisite was satisfied.** Wiring the gate up immediately after found
four real findings on already-shipped code, none of which had ever been
audited: `option-ext` (MPL-2.0, via `rust-embed`), `webpki-roots`
(CDLA-Permissive-2.0, via `reqwest`), and two live security advisories
(RUSTSEC-2026-0194/0195, `quick-xml`, reachable via SPARQL federation's
`SERVICE` clause) plus two lower-severity dev-only ones. Full accounting
in `00i-licensing.md` §7 and `deny.toml` itself — not repeated here.

**Proven to actually reject something**, per this epic's own acceptance
criterion: `rust-igraph` (GPL-2.0-or-later, already named in
`00l-build-vs-adopt.md` as the clearest rejected case) added to an
isolated scratch project sharing this repo's own `deny.toml`, confirmed
`cargo deny check licenses` fails with `error[rejected]: failed to
satisfy license requirements`. Not tested by editing this repository's
own `Cargo.toml`, which would have meant committing (or carefully
reverting) a deliberately-broken dependency tree.

CI job: `.github/workflows/ci.yml`'s `deny` job, `EmbarkStudios/cargo-deny-action@v2`.

## Acceptance criteria

- [x] `deny.toml` exists, encodes `00i`'s allowlist, runs in CI, and **fails**
      on a deliberately-introduced copyleft crate — a gate nobody has seen
      reject anything is not known to work.
- [x] The in-process adapter passes **the same test suite** as the Postgres one
      — 26 tests, unchanged, run against both, plus 10 more closing gaps
      mutation testing found in the ported suite's own coverage. `cargo
      mutants`: 0 missed.
- [x] Reported distances count logical edges, identically to the CTE —
      `a_chain_of_five_logical_edges_reports_distance_five`, ported and
      passing against both adapters.
- [x] `as_of` and the access predicate apply to the extraction, asserted by
      building a graph for two principals and comparing —
      `the_real_in_memory_adapter_still_respects_the_access_predicate_per_principal`
      in `graph-owl-api`, the real adapter (not the existing suite's
      `FakeTraversal`) plugged into `Catalog::walk_hop`'s own authorization
      seam.
- [x] Truncation is reported when the extraction hits its bound — a subgraph
      silently smaller than asked for is a wrong answer about connectivity.
      Ported plus two new exact-boundary tests (`>` vs `>=` is invisible
      unless something lands exactly on the budget).
- [x] A cyclic graph terminates — `a_cycle_terminates`, `--timeout 20s`,
      ported and passing.
- [x] **Measured** faster than the CTE on the workload that triggered this
      epic, and the routing threshold is derived from that measurement rather
      than chosen. **Measured slower at every point tested, including a
      full-tree walk on 88,572 edges** — see "Built and measured, 8 August
      2026" above. No crossover was found, so no routing threshold can be
      derived; the honest answer this criterion asked for is that the
      measurement does not support routing any query to the in-process
      adapter within the range actually tested. Both adapters ship anyway,
      per decision 5 and because a real Postgres bug was found and fixed
      getting to this number.

## Explicitly deferred

- **Keeping a warm graph between queries** → decision 2. Revisit only if a
  read-only, single-principal, single-instant deployment appears, which is a
  different product.
- **Analytics over the extracted graph** (PageRank, centrality) → Epic 38 owns
  those, and it can consume the same extraction. Deliberately not bundled here:
  traversal answers "what is connected", analytics answers "what is
  structurally significant", and `CLAUDE.md` keeps those apart on purpose.
- **An external graph database** → `00k`. The trigger for this epic is depth,
  and depth is solved in-process without a second server.
