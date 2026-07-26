# Plan: Graph Analytics (Epic 38)

**Branch**: feat/analytics
**Status**: Not started — **narrow scope, and a deliberate reversal**
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

- [ ] All four algorithms implemented as pure functions over `GraphProjection`.
- [ ] Every result carries `computed_at_t` and is served with it.
- [ ] A graph exceeding the budget → `BudgetExceeded`, never silent sampling or swapping.
- [ ] PageRank reports whether it converged; hitting `max_iterations` is not silently a success.
- [ ] Results are materialized on a schedule, not computed per request.
- [ ] Analytics never trigger an automatic action (decision 5) — asserted structurally.
- [ ] `graph-owl-analytics` performs **zero I/O**.
- [ ] The PageRank bake-off (Slice E) is run and its result recorded in this file.

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
