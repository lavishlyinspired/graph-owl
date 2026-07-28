# Plan: In-Process Traversal (Epic 103)

**Status**: Not started — **entry condition is a measurement (Epic 37a)**
**Depends on**: Epic 7a (the `TraversalEngine` port), Epic 37a (the trigger)
**Crates**: `graph-owl-traversal` (a second adapter)

## Goal

Make deep walks fast by extracting a bounded subgraph into memory and walking
it there, instead of asking Postgres to recurse further than a recursive CTE
does well.

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

## Acceptance criteria

- [ ] `deny.toml` exists, encodes `00i`'s allowlist, runs in CI, and **fails**
      on a deliberately-introduced copyleft crate — a gate nobody has seen
      reject anything is not known to work.
- [ ] The in-process adapter passes **the same test suite** as the Postgres one
      — 26 tests, unchanged, run against both. Two implementations of one trait
      that are not differentially tested are two behaviours.
- [ ] Reported distances count logical edges, identically to the CTE.
- [ ] `as_of` and the access predicate apply to the extraction, asserted by
      building a graph for two principals and comparing.
- [ ] Truncation is reported when the extraction hits its bound — a subgraph
      silently smaller than asked for is a wrong answer about connectivity.
- [ ] A cyclic graph terminates.
- [ ] **Measured** faster than the CTE on the workload that triggered this
      epic, and the routing threshold is derived from that measurement rather
      than chosen.

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
