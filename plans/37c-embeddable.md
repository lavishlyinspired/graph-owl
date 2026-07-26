# Plan: Embeddable Library (Epic 37c) ★
**Branch**: feat/embeddable
**Status**: Not started
**Depends on**: Epic 1 (stable contract); benefits from Epic 34 (wide surface to validate against)
**Differentiator** — see `plans/00a-product-position.md`
**Crates**: **`graph-owl-storage-memory`** (new — promoted from the test fake to a published crate) · `graph-owl-core` + `graph-owl-api` (documented, `#![deny(missing_docs)]`, published) · dependency-boundary CI check

## Goal

Make `graph-owl-core` and `graph-owl-api` usable as a library inside someone else's process — no server, no global state, no forced async runtime.

## Why this is mostly already true

`core` has zero I/O and depends on no other graph-owl crate. `api` holds `Arc<dyn Storage>` rather than concrete adapters. Both were done for testability; embeddability is the same property viewed from outside. The in-memory `Storage` fake already *is* an embedded catalog — it runs in-process in every facade test.

This epic therefore does three things: **prove** the property holds, **document** the surface, and **defend** it with CI checks so it does not erode.

## Why late

The property is worth proving against the widest possible surface. Publishing an embedding API before Epic 5 (`05-engine-constraints.md`) would mean committing to a shape that five more entity families might invalidate.

## Resolved decisions

1. **No new abstraction layer.** If embedding requires a facade over the facade, the layering was wrong. The embedding surface is the existing `Catalog` plus a constructor.
2. **`core` must stay I/O-free**, enforced by a CI dependency check rather than discipline.
3. **Do not force an async runtime choice.** The traits are `async`; the embedder brings their own executor. No `#[tokio::main]`, no runtime construction inside the library.
4. **The in-memory backend becomes a supported artifact**, not a test-only fixture — it is the zero-dependency embedding option and the thing that makes "embed the catalog" a one-line proposition.
5. **Semantic versioning applies from the first published crate.** Embedders depend on stability; the API-has-no-consumers argument that justified breaking changes in Epic 1 expires here.
6. **No FFI or WASM.** A Rust-native library is the scope. Bindings are a separate project if ever wanted.

## Acceptance criteria (feature level)

- [ ] An example binary embeds the catalog and performs CRUD without starting a server.
- [ ] The same example works against both the in-memory backend and Postgres, changing one line.
- [ ] `graph-owl-core` has zero I/O dependencies, asserted in CI.
- [ ] No graph-owl crate constructs an async runtime or reads global state.
- [ ] The embedding surface is documented with runnable examples that compile in CI.
- [ ] Crates publish to a registry with correct metadata and semver.
- [ ] Adding an entity family does not break the embedding surface.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: The layering claim is enforced, not asserted

**Value**: The property that makes everything else possible stops depending on nobody making a mistake.
**Path**: CI checks over the dependency graph.
**Acceptance criteria**:
- `graph-owl-core` depends only on serde, chrono, uuid — no I/O, no async runtime, no other graph-owl crate. Asserted by a `cargo metadata` check that fails the build on violation.
- `graph-owl-api` depends on no adapter crate. Asserted.
- No library crate calls `std::env::var`, constructs a runtime, or installs a global logger. Asserted by a lint or grep-based check with an explicit allowlist for `graph-owl-server`.
- Each check fails the build with a message naming the offending crate and dependency.
- Adding a forbidden dependency in a deliberately-broken test branch fails CI.
**RED**: The check itself is the test. Verify by adding a forbidden dependency and asserting CI fails — a check that never fails is not a check. Mutator watch: a check that passes unconditionally must fail this verification.
**GREEN**: dependency assertions, CI wiring.
**REFACTOR**: assess whether existing crates already violate these rules. Any violation found here is a real architectural finding — record it in `plans/00b-architecture.md`'s decision log rather than quietly fixing it.
**Done when**: criteria met, deliberate-violation branch fails CI, commit approved.

### Slice B: The in-memory backend is a real artifact

**Value**: Turns "embed the catalog" into a one-line proposition with zero infrastructure.
**Path**: promote the test-only fake to `graph-owl-storage-memory`, a published crate.
**Acceptance criteria**:
- Implements the full `Storage` trait, not the subset the tests happened to need.
- Passes the entire shared repository test suite — the same suite Postgres passes.
- Documents its semantics honestly: not durable, single-process, no cross-process transactions.
- Concurrency-safe for multi-threaded embedders.
- Optional bounded capacity with a documented eviction or rejection policy, so an embedder cannot leak unboundedly.
- Existing facade tests migrate to it with no behavior change.
**RED**: Run the full shared repository suite against it — the gaps between the fake and a real backend surface here, and each is a genuine bug. Concurrency test with parallel writers. Mutator watch: a non-thread-safe implementation must fail the concurrency test.
**GREEN**: promote, complete the trait, harden concurrency, capacity policy.
**Done when**: criteria met, full suite green, mutation report reviewed, commit approved.

### Slice C: Embedding is demonstrated end to end

**Value**: The proof, and the documentation people will actually copy.
**Path**: `examples/embedded.rs` constructing a `Catalog` and exercising it in-process.
**Acceptance criteria**:
- Creates a hierarchy, adds relationships, reads back, all in-process with no server.
- Backend swap is one line: in-memory → Postgres.
- Compiles and runs in CI — a broken example fails the build.
- Under 50 lines, readable as documentation.
- Uses only public API — no `pub(crate)` reach-through, no test-only helpers.
- Demonstrates error handling with the real `CatalogError`.
**RED**: The example runs as an integration test asserting its outcomes. A test asserting it compiles against only the published public surface. Mutator watch: an example depending on internals must fail the public-surface check.
**GREEN**: example, CI wiring.
**REFACTOR**: whatever is awkward to write here is a real API defect. Assess and fix the API rather than working around it in the example — this slice's main value is the friction it exposes.
**Done when**: criteria met, example green in CI, commit approved.

### Slice D: The surface is documented and stable

**Value**: An integrator can adopt it without reading the source.
**Path**: crate-level docs, `#![deny(missing_docs)]` on published crates, doctests.
**Acceptance criteria**:
- Every public item in `core` and `api` documented.
- Crate-level docs explain the embedding model, threading, and runtime expectations.
- Doctests compile and run in CI.
- A stated public-API surface: what is stable, what is `#[doc(hidden)]`, what may change.
- `cargo doc` builds warning-free.
- A documented policy for what constitutes a breaking change.
**RED**: `#![deny(missing_docs)]` failing the build until docs exist is the RED. Doctests as executable examples. Mutator watch: n/a — this slice is documentation; the compiler is the test.
**GREEN**: docs, lints, policy.
**Done when**: criteria met, `cargo doc` clean, commit approved.

### Slice E: Crates are publishable

**Value**: Someone can actually depend on it.
**Path**: publishing metadata, license, semver, release process.
**Acceptance criteria**:
- `core`, `storage`, `storage-memory`, `api` publish with description, license, repository, keywords.
- Adapter and server crates are marked `publish = false` unless deliberately released.
- Versions are semver from `0.1.0`; a documented policy governs bumps.
- `cargo publish --dry-run` succeeds for each publishable crate.
- Inter-crate dependencies use version requirements, not only paths, so a published crate resolves.
- A changelog is generated or maintained per crate.
- A release checklist exists and is followed.
**RED**: `cargo publish --dry-run` in CI for each publishable crate — it catches missing metadata and path-only dependencies, the two things that break publishing. Mutator watch: n/a.
**GREEN**: metadata, version requirements, release process.
**Done when**: criteria met, dry-run green, commit approved.

### Slice F: The surface survives expansion

**Value**: Proves the embedding API is not accidentally coupled to today's entity set.
**Path**: verify against the entity families from Epic 34.
**Acceptance criteria**:
- The example extends to a second entity family with no change to the embedding surface.
- No new public type was required to embed the expanded catalog.
- The `Storage` trait's growth across Epics 37–18 did not force embedders to implement methods they do not use — or, if it did, that is recorded as a finding with a proposed remedy.
- Documentation updated for the wider surface.
**RED**: Extend the example to a second family and assert the public surface is unchanged (a snapshot test of the exported API, e.g. `cargo public-api`). Mutator watch: a surface change slipping through must fail the snapshot.
**GREEN**: extension, surface snapshot test.
**REFACTOR**: if the `Storage` trait has grown so large that implementing it is unreasonable for an embedder, that is a genuine finding. Assess splitting it into narrower traits (`EntityStore`, `RelationshipStore`, `HistoryStore`) — and record the reasoning either way in `plans/00b-architecture.md`.
**Done when**: criteria met, surface snapshot green, commit approved.

## Explicitly deferred (with destination)

- **FFI / C bindings** → separate project; a Rust-native library is this epic's scope.
- **WASM target** → interesting for browser-side catalog tooling, but needs the async and storage stories rethought entirely.
- **A plugin system** (embedders registering custom entity types at runtime) → a much larger design; custom properties (Epic 22 — custom properties) covers the common need.
- **Splitting `Storage` into narrower traits** → only if Slice F's assessment says the trait has become unreasonable. Doing it speculatively would churn every adapter.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment — Slice C's friction findings taken seriously as API defects.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Dependency checks fail on a deliberately-broken branch — verified, not assumed.
5. `cargo publish --dry-run` green for every publishable crate.
6. Public API snapshot committed and diffed on every subsequent PR.
