# Plan: Graph Analytics (Epic 38)

**Branch**: feat/analytics
**Status**: **Slices A–D shipped, 8 August 2026** — projection/budget, degree
centrality, connected components, `PageRank` with honest convergence.
Slice E (the `PageRank`-vs-usage-signals bake-off) and Slice F (scheduling,
caching, HTTP surfacing) are **not attempted this pass** — a stated scope
cut, not a silent one; see the write-up below for why. Narrow scope, and a
deliberate reversal, both still true of the whole epic.
**Depends on**: Epic 7a (traversal), Epic 4 (flakes), Epic 28 (usage signals, for comparison)
**Crates**: **`graph-owl-analytics`** (new — pure algorithms over a caller-supplied projection)

## The reversal, stated plainly

`ROADMAP.md` has listed graph analytics under "not doing" since the roadmap was written, with the reasoning: *Epic 28's usage signals cover ranking at a fraction of the cost.* That reasoning still holds for **ranking**, and this epic does not overturn it.

What the property-graph capability review changed is the recognition that three of the four algorithms below **are not ranking**. They answer structural questions about the metadata graph that no usage signal can answer, because they are about the graph's shape rather than about traffic:

- Which assets are connected to nothing? (orphans — a governance question)
- Which subgraphs are entirely disconnected from the rest? (silos — an integration question)
- Which assets, if they broke, would break the most? (blast radius — an operational question)

Those were never really ranking questions, and lumping them in with PageRank is what got them rejected. PageRank itself is included, narrowly, with an explicit bake-off against usage signals in Slice E — if it loses, it is deleted, and this plan says so up front.

**Scope discipline**: four algorithms, chosen because each answers a named question above. Not a graph data-science library. Not centrality families, not embeddings, not community-detection variants. The `00e-crate-architecture.md` rejection of a general analytics crate stands; this is its narrow exception, and the crate is named for what it does.

## Resolved decisions

1. **Four algorithms, each tied to a named question.** Degree centrality (blast radius), weakly-connected components (orphans and silos), PageRank (asset importance, on probation), and cycle detection reuse from Epic 7a (dependency loops). Adding a fifth requires a question it answers that these do not.
2. **Analytics are computed, cached, and stamped — never live per request.** A PageRank over the whole graph on every page load is absurd. Results are materialized with the transaction time they were computed at, and served stale-with-a-timestamp rather than recomputed.
3. **Pure algorithms over an in-memory projection the caller supplies.** The crate does no I/O. This keeps it exhaustively mutation-testable and makes the memory bound the caller's explicit decision rather than a hidden one.
4. **Bounded by construction: if the graph does not fit the configured budget, the run refuses.** It does not silently sample, and it does not swap. A metadata graph at this project's target scale fits in memory comfortably; the guard exists so the failure at 100× is an error message rather than an outage.
5. **Analytics are advisory and labelled as such.** An orphan flag is a prompt for a human, never an automatic action. Nothing in the system deletes, deprecates, or downranks on an analytics result alone.
6. **PageRank is on probation and its exit criterion is written down** (Slice E). This is what makes decision 1 honest rather than scope creep with a nicer vocabulary.

## Implementation reference

```rust
// graph-owl-analytics — pure, no I/O
pub struct GraphProjection {        // caller-supplied, built from TripleStore or LPG
    pub nodes: Vec<NodeId>,
    pub adjacency: CsrGraph,        // compressed sparse row: cache-friendly, compact
    pub weights: Option<Vec<f32>>,  // edge confidence, when weighted
}

pub struct AnalyticsBudget { pub max_nodes: usize, pub max_edges: usize, pub max_iterations: usize }

pub fn degree_centrality(g: &GraphProjection, dir: Direction) -> Vec<Degree>;
pub fn connected_components(g: &GraphProjection) -> Components;      // union-find
pub fn pagerank(g: &GraphProjection, cfg: &PageRankConfig) -> Result<Vec<Rank>, BudgetExceeded>;

pub struct PageRankConfig {
    pub damping: f32,               // 0.85
    pub tolerance: f32,             // convergence threshold
    pub max_iterations: usize,      // hard cap; report if hit without converging
}

pub struct AnalyticsResult<T> {
    pub values: Vec<T>,
    pub computed_at_t: i64,         // decision 2: every result carries its transaction time
    pub converged: bool,
    pub node_count: usize,
}
```

**CSR adjacency, not a hash map of vectors.** The algorithms are memory-bandwidth-bound; an adjacency list of `HashMap<NodeId, Vec<NodeId>>` costs several times the memory and loses locality on every iteration. This is the one place in the project where a data-structure choice is worth stating in a plan.

### The four questions and their answers

| Question | Algorithm | Surfaced as |
|---|---|---|
| What breaks if this breaks? | Degree centrality, outgoing, over lineage edges | Blast-radius count on the asset page (Epic 40) |
| What is documented but connected to nothing? | Weakly-connected components, size-1 | Governance report; Epic 14 `TrustSummary.gaps` |
| What parts of the estate are isolated from the rest? | Weakly-connected components, size distribution | Integration report — a silo is usually a missing connector |
| Which assets matter structurally? | PageRank over lineage | **On probation** — Slice E |

## Acceptance criteria

- [x] All four algorithms implemented as pure functions over `GraphProjection`.
- [ ] Every result carries `computed_at_t` and is served with it. **Slice F, not attempted** — this is a scheduling/serving concern, not something the pure algorithms carry themselves.
- [x] A graph exceeding the budget → refused (`ProjectionError::TooManyNodes`/`TooManyEdges`), never silent sampling or swapping.
- [x] `PageRank` reports whether it converged; hitting `max_iterations` is not silently a success.
- [ ] Results are materialized on a schedule, not computed per request. **Slice F, not attempted.**
- [ ] Analytics never trigger an automatic action (decision 5) — asserted structurally. **Slice F, not attempted** — nothing calls these functions from any action-taking code path yet, so the structural test has nothing to guard.
- [x] `graph-owl-analytics` performs **zero I/O** — no dependency in its `Cargo.toml` reaches storage, network, or the filesystem.
- [ ] The `PageRank` bake-off (Slice E) is run and its result recorded in this file. **Not attempted** — see the write-up below for why.

## Slices A–D: projection, degree, components, `PageRank` — shipped 8 August 2026

**`project(all_nodes, flakes, edge_types, budget)`** builds a
[`GraphProjection`] as compressed sparse row, deterministic by construction
(nodes numbered in sorted-`Sid` order, each adjacency list sorted by
neighbour). **`all_nodes` is a real addition beyond the plan's own
implementation reference**: without it, a node touched by no matching edge
can never enter the projection at all — it cannot be an edge endpoint with
no edge — which would make orphan detection (Slice C's whole reason to
exist) structurally unable to find the single most important case:
an asset connected to *nothing*. Found by this slice's own RED test, not
anticipated in the plan.

**`degree_centrality`** returns every node's in/out/total degree,
including zero — never silently dropping a disconnected node, which would
make "not scored" indistinguishable from "scored zero". Weighted when the
projection carries confidence weights.

**`connected_components`** is union-find with path compression, weakly
connected (edge direction ignored for connectivity, kept for degree).
`Components::orphans()` is the size-1 set. **The filter-visibility
criterion the plan itself calls out as the subtle one** — "orphan" means
nothing without saying which edges were considered — is carried
structurally: `GraphProjection` now records its own `edge_types`, and
`Components` copies it forward, so a result can never be presented as a
filter-independent fact.

**`pagerank`** is power iteration with damping, `converged: false` reported
honestly on iteration exhaustion rather than ever implied by iteration
count alone, dangling mass explicitly redistributed every pass (not
dropped), weighted when the projection carries weights. The reference
check (a directed cycle converges to the uniform distribution) is derived
from the algorithm's own symmetry, not transcribed from any external
implementation's fixture.

**Mutation testing found three real gaps**, all now fixed: two off-by-one
budget boundaries (`>` vs `>=`, invisible because no test checked "exactly
at budget" specifically), which side of an edge `UnionFind::union` visits
first (a `<` vs `==` distinction only observable when the smaller root is
visited *first*, not merely when the two runs of the same input agree with
each other), and `PageRank`'s own normalization step (`/` vs `*`/`%`,
invisible on every earlier fixture because each happened to have
`out_weight_total == 1.0`, where all three operators coincide). One
mutant — `<` vs `<=` in the same union comparison — is **provably
equivalent**, not a missing test: the comparison is only ever reached
after a guard establishing the two roots differ, so `<=` degenerates to
exactly `<` on every call this line can see. Documented in the code rather
than chased. Final: 30 tests, 0 missed mutants (1 documented equivalent).

## Explicitly deferred this pass (not silently dropped)

- **Slice E, the `PageRank` bake-off.** Its own acceptance criteria name a
  *real corpus* and *a human rating a blind sample* — neither is available
  to an unattended implementation pass, and faking either would violate
  this project's own "measured, not assumed" discipline (`CLAUDE.md`'s
  build-loop section, and the identical lesson already recorded for Epic
  98's sidecar timing and Epic 37a's soak test). Decision 6 stays honest
  about `PageRank` remaining on probation until this actually runs.
- **Slice F, scheduling/caching/HTTP surfacing.** Needs Epic 15's
  scheduler, storage for materialized results, and wiring into
  `graph-owl-api`/the HTTP surface — cross-crate work of a different kind
  than Slices A–D's pure algorithms, not attempted in this pass.

## Which library, and which is licence-poison

**Checked 4 August 2026**, per `CLAUDE.md`'s search-before-building rule.

**Adopt `petgraph`** (MIT/Apache-2.0, 451M downloads) — checked again 11
August 2026 once the crate was shipped and actually adopted, not just
proposed. The arithmetic itself (`PageRank`'s honest convergence
reporting, `Components`' per-node detail and orphan/silo tracking) stayed
hand-rolled: `petgraph::algo::page_rank` takes a fixed iteration count
with no convergence signal, and `petgraph::algo::connected_components`
returns a bare `usize` count — both materially weaker than what this
crate's own governance-reporting use needs, so swapping them in would
have been a regression, not a refactor (`00l-build-vs-adopt.md`'s
petgraph entry has the full comparison). What *was* adopted is the
storage: `CsrGraph` wraps `petgraph::csr::Csr` rather than hand-rolled
`row_offsets`/`col_indices` — its internal layout turned out to be
exactly that representation already, read directly from its source. What
this crate builds is the projection, the caching, the budget, and the
governance-shaped algorithm contracts — not the adjacency storage, which
petgraph now owns.

**Do not adopt `rust-igraph`.** It has every algorithm this epic wants and is
**GPL-2.0-or-later** — copyleft, which `00i` rejects and which would relicense
graph-owl. It has been proposed once already; it is recorded in
`00l-build-vs-adopt.md` so it is not proposed again.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Projection and budget

**Acceptance criteria**: a `TripleStore` pattern result builds a CSR projection; only the requested edge types are included; a graph exceeding `max_nodes` or `max_edges` refuses with the actual and permitted counts; the projection is deterministic — same input, same node ordering, so results are reproducible; an empty graph projects without error.
**RED**: The determinism test — non-deterministic node ordering makes PageRank results wobble between runs on identical data, which reads as a bug in the algorithm and is a bug in the projection. The budget test asserting the error *names both numbers*, so an operator can size the budget. Mutator watch: hash-order iteration must fail determinism; a budget check after allocation must fail the refusal test.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: Degree centrality → blast radius

**Acceptance criteria**: in-, out-, and total degree; computed over a filtered edge set (lineage only, by default); a node with no edges scores 0, not absent; direction is not conflated — a heavily-consumed table and a heavily-consuming one are different findings; weighted-by-confidence variant available; results tie-break deterministically.
**RED**: The direction test on an asymmetric fixture: a table with 50 downstream consumers and 1 upstream source must not score the same as its inverse. Conflating them turns the blast-radius answer into noise. Mutator watch: summing in and out must fail it.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Connected components → orphans and silos

**Acceptance criteria**: weakly-connected components via union-find with path compression; size-1 components are the orphan set; the size distribution is reported; a fully-connected graph yields one component; an empty graph yields none; components are stable across runs (deterministic ids); an entity with only a `hasOwner` edge is **not** an orphan by the lineage-only projection but **is** by a lineage-only *filter* — the filter is explicit and named in the result.
**RED**: The filter-visibility test is the subtle one: "orphan" means nothing without saying which edges were considered, and a report that does not name its filter produces confident wrong conclusions in a governance review. Mutator watch: omitting the filter from the result must fail; union without path compression passes correctness and must be caught by the scale assertion instead.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: PageRank, with honest convergence

**Acceptance criteria**: power iteration with damping 0.85; converges to a published reference result on a standard small graph, within tolerance; dangling nodes handled explicitly, not by silently dropping their mass; `max_iterations` without convergence returns `converged: false` and the partial result; weighted-by-confidence variant; scores sum to 1 within floating-point tolerance.
**RED**: The dangling-node test: a node with no outgoing edges leaks rank mass, and the leak is invisible on small test graphs but skews the whole ranking on a real one. Assert the sum-to-1 invariant on a fixture containing dangling nodes. Mutator watch: dropping dangling mass must fail the invariant; reporting `converged: true` on iteration exhaustion must fail — that is the mutant that turns a wrong answer into a confident one.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: The bake-off — PageRank versus usage signals

**Value**: This slice decides whether decision 6 keeps PageRank or deletes it. It is the reason the reversal is narrow rather than a capitulation.
**Acceptance criteria**: on a real corpus, produce the top-N most important assets by PageRank and by Epic 28 usage signals; measure agreement; have a human rate a blind sample of both for usefulness; **record the outcome in this file**; if PageRank does not beat usage signals on assets where the two disagree, remove `pagerank` from the crate and update `ROADMAP.md`'s not-doing table to say it was tried and lost.
**RED**: The methodology is the deliverable — a reproducible comparison harness with the corpus, both rankings, the disagreement set, and the rating protocol. A bake-off that cannot be re-run when the corpus changes is an anecdote.
**Done when**: the comparison is run, the outcome is written into this file, the code matches the outcome, and the commit is approved.

### Slice F: Scheduling, caching, and surfacing

**Acceptance criteria**: analytics run on a schedule (Epic 15's scheduler), not per request; results are stored with `computed_at_t` and served with an explicit age; a request during a run gets the previous result, never a partial one; a failed run leaves the previous result intact; results appear on the Epic 40 asset page and in Epic 14's `TrustSummary.gaps`; a structural test asserts **no code path takes an action based on an analytics result** (decision 5); an operator can trigger a run manually.
**RED**: The stale-not-partial test — serving a half-computed component set is worse than serving yesterday's, because it looks current. The structural no-action test guards decision 5. Mutator watch: serving partial results must fail; an auto-deprecation path must fail the structural test.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **Community detection (Louvain, Leiden)** → still not planned. Domains (Epic 24) are the human-assigned answer to "which things belong together", and a human-assigned grouping beats an inferred one for governance.
- **Betweenness, closeness, eigenvector centrality** → degree answers the operational question at a fraction of the cost. Revisit only with a question degree cannot answer.
- **Graph embeddings / link prediction** → `00e-crate-architecture.md` rejects a completion crate; the ML dependency and the explainability loss both cut against `00a-product-position.md`.
- **Distributed / GPU analytics** → single-node deployment; decision 4's budget refuses rather than scales.
- **Analytics over the reasoning overlay** → derived edges (Epic 6) would inflate degree scores with inferred facts; needs a decision about whether inference counts as connectivity. Revisit after Epic 6 ships.

## Pre-PR quality gate

1. `cargo mutants` — 0 missed. Pure algorithms with published reference results; no excuse for a survivor.
2. Refactoring assessment. 3. `cargo test/clippy/fmt`.
4. **Zero I/O dependencies** asserted.
5. **PageRank verified against a published reference result** (Slice D).
6. **The bake-off outcome is recorded in this file and the code matches it** (Slice E).
7. Structural assertion that no analytics result drives an automatic action (Slice F).
