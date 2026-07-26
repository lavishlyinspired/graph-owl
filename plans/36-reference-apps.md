# Plan: Reference Applications (Epic 36)

**Branch**: feat/reference-apps
**Status**: Not started
**Depends on**: Epic 14 (MCP), Epic 16 (SDKs), Epic 29 (graph API)
**Crates**: **No graph-owl crate changes.** Examples live in `examples/` and depend only on published crates and generated SDKs — enforced by Slice A. Any change required in a graph-owl crate is a defect logged against its owning epic.

## Goal

Prove the activation stack works end to end, using only published surfaces. Three small applications, each a test of whether the API is actually usable.

## Why this is a proof, not a product

The reference model lists Applications as an activation output. A reference application is also the only honest test that SDKs and MCP are usable — friction discovered here is an **API defect**, not an application problem. This is explicitly not the web UI that remains out of scope: it is small, it is CI-verified, and it exists to find defects.

## Resolved decisions

1. **Published surfaces only.** No internal crate imports, no `pub(crate)` reach-through, no test helpers. Asserted by a dependency check, because the value of the exercise depends entirely on this constraint.
2. **Friction is a defect in graph-owl, not the app.** When something is awkward here, the fix goes in the API. Working around it in the app defeats the purpose.
3. **Three apps, deliberately small.** An agent workflow, an ingestion adapter, and a read-only browse surface. Each under a few hundred lines — big enough to be real, small enough that nobody mistakes them for a product.
4. **They run in CI.** A broken reference app fails the build, which is what keeps them honest as the API evolves.
5. **No new dependencies on graph-owl's side.** If an app needs a capability, that is an epic, not a patch.

## The three applications

### 1 · Agent workflow (`examples/agent-triage/`)

An agent answering "is this table safe to build on?" using MCP alone.

**Exercises**: Epic 14's seven read tools, trust summaries and gaps, policy filtering, token budgets, memory recall.

**Acceptance criteria**: answers correctly for a healthy asset, a deprecated asset, an uncertified asset, and one the principal cannot fully see; the deprecated case surfaces the successor; the partially-visible case states its view is filtered rather than asserting absence; answers in **≤ 3 tool calls per question** — a proxy for whether Epic 14's tools are task-shaped or endpoint-shaped.

### 2 · Ingestion adapter (`examples/adapter-csv/`)

A custom adapter pushing a fixture source through the SDK — the worked example Epic 16's guide references.

**Exercises**: Epic 16's push API, SDK ergonomics, idempotency, batch, scoping, error handling, bot principals.

**Acceptance criteria**: pushes entities, relationships, and lineage; a re-run produces zero new versions; a deliberately-invalid row is reported per-item without aborting; uses the idempotency key correctly; runs twice in CI to prove convergence.

### 3 · Browse surface (`examples/browse/`)

A minimal read-only server rendering an asset with its context — the smallest thing that proves the read API is renderable.

**Exercises**: Epic 1's REST contract, generated client, pagination, field selection, `EntityReference` denormalization, Epic 8 search.

**Acceptance criteria**: search, list with pagination, and asset detail with owners, tags, lineage, and trust context; renders an asset in **one request** via field selection, not N+1; handles empty, error, and filtered states visibly; uses the generated client, never hand-rolled HTTP.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first. Here the RED is usually a failing integration test against a live service.

### Slice A: Surface-purity enforcement

**Value**: The constraint that makes the whole epic meaningful.
**Acceptance criteria**: a CI check asserts each example depends only on published crates and the generated SDK; an example importing an internal path fails the build with a message naming the import; the check is verified by a deliberately-broken branch; examples build against the *published* crate versions, not workspace paths, so a missing `pub` is caught.
**RED**: The deliberate-violation branch must fail CI. A check that never fails is not a check. Mutator watch: an unconditional pass must fail this verification.
**Done when**: criteria met, deliberate violation fails CI, commit approved.

### Slice B: Agent workflow

**Acceptance criteria**: as above; the tool-call-count assertion is enforced, not advisory; the filtered-view case asserts the agent says its view is partial; runs against a seeded graph in CI.
**RED**: The call-count assertion is design feedback: if answering "who owns this" takes five calls, Epic 14's decision 5 was not honoured and the tool surface needs changing. The filtered-view test catches an agent confidently asserting absence.
**REFACTOR**: any question needing more than three calls is an Epic 14 defect. Record it and fix the tool surface.
**Done when**: criteria met, commit approved.

### Slice C: Ingestion adapter

**Acceptance criteria**: as above; convergence proven by running twice and asserting zero new versions on the second; per-item error reporting asserted; the adapter is the artifact Epic 16's guide links to, so the guide and the code cannot drift.
**RED**: Convergence test. A per-item test with one bad row asserting the rest land. Mutator watch: an adapter that aborts on first error must fail the per-item test.
**Done when**: criteria met, commit approved.

### Slice D: Browse surface

**Acceptance criteria**: as above; the one-request assertion is enforced by a request counter — rendering an asset with owners, tags, and lineage must not fan out; empty, error, and filtered states render distinguishably; no hand-rolled HTTP.
**RED**: The request-counter test catching N+1. If field selection cannot deliver an asset page in one request, that is an Epic 1 defect. Mutator watch: per-related-entity fetching must fail the counter test.
**REFACTOR**: an N+1 here is an API defect. Fix field selection, not the app.
**Done when**: criteria met, commit approved.

### Slice E: Defect log

**Value**: The epic's actual output.
**Acceptance criteria**: every friction point found while building the three apps is recorded as a finding with the epic it belongs to and a proposed fix; findings are triaged, not silently absorbed; findings that were fixed name the commit; findings deferred name why; the log lives in this plan so the exercise's value survives it.
**RED**: n/a — this slice is documentation, and its content is whatever the previous four slices surfaced.
**Done when**: log complete, findings triaged, commit approved.

## Explicitly deferred (with destination)

- **A production web UI** → out of scope (`00a-product-position.md`). These are proofs.
- **Additional example languages** → two SDKs are proven in Epic 16; more examples add coverage, not information.
- **Deployment of the examples** → they run in CI; hosting them is not a goal.
- **An example agent with write access** → after Epic 32, and it would need its own trust discussion.

## Pre-PR quality gate

1. Refactoring assessment — friction findings treated as API defects, not app problems.
2. `cargo test/clippy/fmt`; all three examples build and run in CI.
3. Surface-purity check verified against a deliberately-broken branch (Slice A).
4. Tool-call-count and request-count assertions enforced, not advisory.
5. Defect log complete and triaged before merge.
