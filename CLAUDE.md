# graph-owl

A knowledge graph engine that stores, queries, reasons over, and validates enterprise metadata as a connected graph. Rust workspace, 28 crates — 5 built, 23 placeholders created by the epic that needs them.

**Built** (the walking skeleton: HTTP → facade → port → Postgres):

```
graph-owl-core               pure domain types, no I/O
graph-owl-storage            Storage trait + StorageError (the port)
graph-owl-storage-postgres   sqlx-backed impl of Storage (the adapter)
graph-owl-api                Catalog facade, wraps Arc<dyn Storage>
graph-owl-server             axum HTTP layer, composition root
```

**Placeholders**, grouped by what they are:

| Group | Crates |
|---|---|
| Engine | `engine` (port) · `engine-postgres` · `ontology` · `constraint` · `reasoning` · `query` · `traversal` |
| Property graph | `lpg` · `bolt` · `lpg-io` |
| Search | `search` (port) · `search-hnsw` · `search-opensearch` |
| Interop & activation | `rdf-io` · `events` · `mcp` · `connectors` · `cli` |
| Other | `authz` · `resolution` · `analytics` · `ui` · `storage-memory` |

Every placeholder's `lib.rs` names the epic that implements it. `plans/00e-crate-architecture.md` is the authority on which crates exist, which were rejected, and the growth trigger for adding one — **read it before creating a crate**.

Edition 2024, Rust workspace with `[workspace.lints.clippy] all = "warn", pedantic = "warn"`. Frontend sources live in `ui/`, outside `crates/`; `graph-owl-ui` only embeds and serves the build output.

## Process

TDD is non-negotiable: RED (failing test first) → GREEN (minimum code) → MUTATE (`cargo mutants`) → KILL MUTANTS → REFACTOR (only if it adds value).

**Commit directly to `main` without asking.** This supersedes the previous rule requiring approval at every commit point, which made autonomous runs stall. A slice is committable when its acceptance criteria are met, the workspace test suite is green, `clippy` and `fmt` are clean on the touched crates, and the mutation run has been started and reviewed. Do not branch, do not open a PR, do not pause — just commit.

Three things that did **not** change with it:

- **The TDD cycle itself.** No production code without a failing test. Committing without approval is not permission to skip RED.
- **Honest commit messages.** State what was done, what was traded, and what is left — including known-loose design a later slice must fix. A commit message that hides a compromise is worse than the compromise.
- **Push is still a separate decision.** Commit freely; ask before `git push`.

Never name the third-party systems whose clones sit under `.claude/docs/referenceRepo/` — not in code, comments, commit messages, plan docs, or any other committed file. Those clones may stay on disk for architecture research, but must never be committed or cited by name. This project's git history was deliberately squashed once already to scrub such references; don't reintroduce them. When a design decision was informed by that research, write down the pattern and the reasoning behind it, never the source.

## Licensing — binding during implementation

**Neither reference under `.claude/docs/referenceRepo/` is permissively licensed throughout, and one is not open source at all.** graph-owl contains no code from either, and that is the entire basis on which their non-compete terms do not bind this project. It is a property to actively maintain while writing code, not a claim made once.

Full rules in **`plans/00i-licensing.md`** — read it before implementing anything in Phase 1. Named specifics (which licence, which directories, incident log) are in `.claude/docs/licensing-detail.md`, gitignored.

The four that matter most while coding:

1. **Do not open reference source while writing the corresponding graph-owl code.** Study and implementation happen in separate sessions. This is the only mechanically checkable rule and the most effective one.
2. **Specifications are the source; implementations are not.** W3C for RDF/SPARQL/OWL/SHACL/SKOS/JSON-LD, ISO/IEC 39075 and openCypher for Cypher, the published Bolt/PackStream spec, RFC 9457 for errors. If a capability has a spec, the spec is the *only* permitted reference — including when the spec is unclear.
3. **Never copy anything**: source (including translated or "adapted"), constant tables, thresholds, tuning numbers, size classes, timeouts, error strings, metric names, config keys, test fixtures, golden files, or comments. **Every magic number in graph-owl must be derivable from a stated reason in a plan** — "the reference used this" is not a reason, and a number without one was never justified for this system anyway.
4. **When stuck**: the spec first, then a permissively licensed implementation (licence checked *before* reading), then ask a human. Never open the source-available or community-licensed reference to unblock a task — that is exactly the moment the rule exists for.

One incident already occurred and was reverted during planning (a cache-tier table reproduced near-verbatim, rationale included). Assume the same failure mode will present itself while coding.

**Dependencies**: `cargo deny` with a permissive-only allowlist (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib). Copyleft and source-available crates are rejected by default.

**Crate naming is not a concern.** `core`, `api`, `server`, `query`, `cli`, `storage` are universal Rust convention, not anyone's expression; `graph-owl-bolt` names the protocol it speaks, which is descriptive use of an openly specified protocol. Do not rename crates for licensing reasons — see `plans/00i-licensing.md`.

## Gotchas learned building the Table entity slice

- **axum 0.7 + edition 2024 doesn't mix.** Implementing a custom `FromRequest<S>` extractor against axum 0.7 from an edition-2024 crate fails with `E0195` (lifetime params on `from_request` don't match the trait). axum-core 0.7.x was authored under edition-2021 RPITIT capture rules; edition 2024 changed them. Fix is to upgrade to axum 0.8 (native async-fn-in-trait, edition-2024 compatible) — not to downgrade the workspace edition. Also note: axum 0.8 changed path param syntax from `:id` to `{id}`.

- **testcontainers-rs: keep the container handle alive.** `ContainerAsync<Postgres>` must stay bound for as long as the test needs the database — if a helper function returns only the connection string/pool and drops the container locally, Docker tears it down almost immediately and the next query fails with a pool timeout. Test helpers must return the container alongside the pool, e.g. `(PostgresStorage, ContainerAsync<Postgres>, String)`, and the caller must bind it (even as `_container`) for the test's duration.

- **refinery has no direct sqlx integration.** Migrations need a separate `tokio_postgres::Client` (via `tokio_postgres::connect(..., NoTls)`, with the connection future spawned) alongside the `sqlx::PgPool` used for app queries. Run via `embedded::migrations::runner().run_async(&mut migration_client).await`, with migrations embedded through `refinery::embed_migrations!("migrations")`.

- **Postgres `TIMESTAMPTZ` is microsecond precision; `chrono::Utc::now()` is nanosecond.** Verified non-flaky across repeated test runs in this project, but worth remembering if a future equality assertion on a round-tripped timestamp ever looks suspicious.

- **Partial updates: one atomic `UPDATE ... SET x = COALESCE($n, x) ... RETURNING`,** not read-then-write. Avoids a race between the read and the write, and lets Postgres's own `now()` set `updated_at` rather than passing a Rust-side timestamp.

- **PATCH immutability via DTO shape, not validation.** `TableUpdate` simply has no `id`/`fully_qualified_name` fields, so there's nothing for a client to send that could mutate them — serde silently drops unknown fields. Prefer this structural approach over runtime rejection when a field should never be client-settable on an endpoint.

- **Custom 400 vs axum's default 422.** axum's built-in `Json<T>` extractor returns `422 Unprocessable Entity` for a syntactically valid but semantically invalid body (e.g. a missing required field). This project's acceptance criteria require `400` instead, so `graph-owl-server` wraps it in a custom `AppJson<T>` extractor that remaps the rejection.

- **A surviving mutant is almost always a missing *negative* test, not a missing positive one.** Measured across every mutation run in this project so far: every survivor has been a case where the positive assertion passed under the mutation because the mutated code still produced the right answer *for that input*. Epic 6 slice A is the clearest example — inverting transitivity's join condition (`==` → `!=`) still derived the far edge of a genuine `a→b→c→d` chain, so the positive test passed; it took asserting that two **disconnected** edges do *not* compose. `sameAs` widening from `&&` to `||` still copied the right property, so it took a **third entity** whose properties must not be copied.

  So when writing the RED test, for every "X derives/returns/produces Y", also write "and Z does **not**". Cheaper than paying for a second mutation run, and it is the same discipline the plans already demand of `domain`/`range` (assert the swap fails) generalised to every rule.

- **Run `fmt` → `clippy` → `test` green *before* `cargo mutants`, never after.** Clippy takes seconds; a mutation run takes minutes. A clippy fix changes the code, which invalidates the mutation run you just paid for — doing them in the wrong order means running the slow thing twice, which has happened here more than once.

- **Do not try to speed up `cargo mutants` with parallelism, `nextest`, or debug-info settings. All three were measured on this workspace and all three are slower or neutral.** On a 20-mutant file: baseline **75s**, `-j 2` 91s, `-j 6` **158s** (system time went 74s → 577s — concurrent cargo builds thrash I/O), `--test-tool nextest` 91s, `--in-place` 80s and it mutates the working tree, minimal debug info no change, `--baseline skip` no change.

  The reason none of them help: `cargo test -p graph-owl-query` costs 5.6s of which **0.85s is running the tests**. The other 85% is cargo's per-invocation overhead across 28 crates — fingerprint checks, resolution, linking — and `user` time is under 1s, so it is not CPU-bound and parallelism has nothing to parallelise. Tuning test threads or the test runner optimises the 15%.

  What actually reduces mutation time: **fewer mutants** (`--file` scoping, which is already the practice) and **not re-running** (the ordering rule above). Background any run over ~30 mutants and keep working.

- **Re-measured 29 July 2026, and the earlier advice was solving the wrong problem.** A 42-mutant run on `budget.rs` took **over an hour**. The per-mutant breakdown said why: **37s build + 59s test**, because the default invocation runs `cargo test -p graph-owl-server`, and that crate's tests are **integration tests that each start a Postgres container**. Every mutant paid for the whole container suite.

  **The fix is `--cargo-test-arg --lib`**, which restricts the run to unit tests. Test time per mutant drops **59s → 0.8s**, and the same 42-mutant file finishes in **9 minutes instead of ~50**. Use it whenever the mutated code is covered by unit tests — which for pure decision logic it always should be.

  Two consequences worth knowing:

  - With `--lib`, the cost becomes **94% build**. Nothing about the tests matters any more; the whole run is `rustc` recompiling and relinking the mutated crate.
  - cargo-mutants runs `cargo test` **without** `--test-threads=1`, so on this workspace its baseline hits the documented `PortNotExposed` contention and the run aborts with "cargo test failed in an unmutated tree". `--lib` avoids that too, by not starting containers at all.

- **`-D/--in-diff <file>` is the largest remaining lever, and it was not being used.** It mutates only lines the diff touches. Measured on `openapi.rs`: **12 mutants for the whole file, 1 for the change that was actually made.** For an incremental edit to an existing file this is an order of magnitude, and it is the right default when re-verifying a small change:

  ```
  git diff HEAD -- path/to/file.rs > /tmp/change.diff
  cargo mutants -p <crate> --in-diff /tmp/change.diff --cargo-test-arg --lib
  ```

  `--file` remains right for a *new* file, where the file and the diff are the same thing.

- **`scripts/mutants.sh` bakes both in**, so the fast invocation is the default rather than something to remember:

  ```
  scripts/mutants.sh graph-owl-server crates/graph-owl-server/src/admission.rs
  scripts/mutants.sh graph-owl-server --diff crates/graph-owl-server/src/budget.rs
  ```

- **Which crate the code lives in is a performance decision.** Measured per mutant: **`graph-owl-authz` 3.7s, `graph-owl-server` 9.2s** — 2.5×, and the reason is the build, not the tests (`graph-owl-server`'s test rlib is 63 MB). Pure logic placed in a leaf crate mutates several times faster than the same logic in the server crate. That is a second, independent argument for the split the architecture already prefers.

- **Re-confirmed as *not* worth trying** (measured on an identical 3-mutant scope, 18 cores, 48 GB): `-j 6` is **2m15s vs 87s — 55% slower**, with system time 80s → 255s, so the I/O thrash finding still holds even now that tests are fast. `CARGO_PROFILE_DEV_DEBUG=0` gives 83s vs 87s (5%, inside noise). `--baseline skip` gives 81s vs 87s and removes the check that the tree was green before mutating — not a trade worth making.

- **The integration suite needs bounded parallelism.** `cargo test --workspace` at full parallelism intermittently fails with testcontainers' `PortNotExposed` — a different test each run. It is Docker container-startup contention, not a product bug: every one of those tests passes alone and the whole suite passes at `--test-threads=1`. The pressure roughly doubled when the graph engine landed, because each integration test now opens two Postgres connections (storage adapter + engine adapter) against its container. **Run `cargo test --workspace -- --test-threads=1`**, and do not spend time debugging a `PortNotExposed` failure as though it were real. The durable fix is fewer containers per run (a shared container per test binary, which needs per-test schema isolation to stay correct) — not yet done.

  **Updated 28 July 2026**: `--test-threads=2` was sufficient on `postgres:11-alpine` and is not on `18-alpine` — the bigger image takes long enough to start that two concurrent starts exceed testcontainers' readiness timeout. Serial costs almost nothing: 591 tests in **4m31s at 20% CPU**, and that number is the tell — the suite is waiting on Docker, not computing, so the parallelism was never buying much. The image upgrade did not create the problem; it removed the headroom that was hiding it, which raises the priority of the durable fix above.

- **Test organization:** `tests/common/mod.rs` (a subdirectory containing `mod.rs`) is treated by Cargo as a shared module importable from multiple integration test binaries in the same crate. A top-level `tests/common.rs` file, by contrast, becomes its own separate test target — not what you want for shared helpers.

## Storage backends vs. source connectors — scaling architecture

These are different problems and shouldn't share a pattern:

- **Storage backends** (where the catalog's own data lives — e.g. Postgres, and later MongoDB) are bounded to a handful of options. One crate per backend, each implementing the `Storage` trait, is the right granularity — `graph-owl-storage-postgres`, later `graph-owl-storage-mongodb`. A factory/config switch at startup (in `graph-owl-server`'s `main.rs`) picks one.

- **Source connectors** (external systems the catalog *catalogs* — Snowflake, Kafka, etc., potentially 100+) do not get one crate each. Verified against a mature reference implementation, which puts every connector in a single ingestion package behind a shared connector interface rather than shipping 100 separate packages. The Rust equivalent: one `graph-owl-connectors` crate with a module per connector implementing a shared `Connector` trait.

MongoDB storage-backend support is explicitly deferred (not yet implemented) — see `plans/90-done-table-entity.md`'s "Explicitly deferred" section.

## Documentation map

Read these before planning or implementing anything non-trivial:

| Document | Answers |
|---|---|
| `plans/00a-product-position.md` | What this competes on, what it refuses to compete on, and the enforced budgets |
| `plans/00b-architecture.md` | Layering, flake model, crate map, error model, testing strategy, decision log |
| `plans/00c-domain-model.md` | Entities, envelope, FQN rules, relationships, versioning, triple projection |
| `plans/00d-api-conventions.md` | URL shape, status codes, error body, pagination, filtering, concurrency |
| `plans/00e-crate-architecture.md` | Which crates exist, which were rejected, and the rule for adding one |
| `plans/00f-ui-architecture.md` | Console stack, the two-renderer rule, non-negotiables, CI budgets, what the console will never do |
| `plans/00g-operations.md` | Migration & rollback, backup/DR (RPO/RTO), data retention & erasure, runbooks, the testing levels above unit |
| `plans/00h-ui-design-system.md` | Design tokens, chrome, the five reusable UI patterns, and the epic → screen inventory |
| `plans/00i-licensing.md` | **Clean-room rules binding on every implementation session** — what may be read, what may never be copied |
| `plans/00j-language-boundaries.md` | Rust vs Python vs TypeScript — the process boundary is the language boundary; what is a component and what is a consumer |
| `plans/00k-standards-conformance.md` | **What this product does and does not implement of each W3C standard, dated.** Read before claiming conformance of any kind |
| `plans/00l-build-vs-adopt.md` | **Which libraries to take and which to write.** Read before implementing any standard-shaped component — the answer is usually "adopt" |
| `plans/ROADMAP.md` | All 43 epics in 9 phases, sequenced, with the plan-file work queue |

### Which `00*` docs bind which work

The `00*` documents are **standing reference, not per-epic reading** — they are the decisions every epic inherits. Not all of them bind every epic, so this is the routing table. **Read the binding rows before starting an epic, not after a review finds a conflict.**

| Working on | Must read first |
|---|---|
| **Anything at all** | `00i` (licensing — before writing a line), `00a` (what this competes on) |
| **Anything claiming a W3C standard** | `00k` — and update its verification date if you check a spec |
| **Writing a parser, reasoner or serializer** | `00l` first — a permissive crate may already do it |
| An engine epic (4–9a) | `00b` (layering, flake model, error model), `00c` (domain model, FQN, triple projection), `00e` (before creating any crate) |
| An API surface (1, 2, 3, 16, 34) | `00d` (URL shape, status codes, error body, pagination, concurrency), `00c` |
| A UI epic (39–42) | `00f` (stack, budgets, non-negotiables), `00h` (tokens, the five patterns, screen inventory), `00d` |
| A collection epic (15–21) | `00c`, `00d`, `00g` §5 (journey tests) |
| Anything touching deploy, migration, or data lifetime | `00g` (rollback, DR, retention, runbooks) |
| Adding a crate | `00e` — it is the authority, and the growth trigger is a gate |

Two standing obligations that apply to **every** epic regardless of the table:

- **When implementation and a `00*` document disagree, the document is right and the code has drifted.** Fix the code, or change the document deliberately and say why in `00b`'s decision log.
- **Every magic number needs a stated reason in its plan** (`00i` rule 4). This is both a licensing control and a design one.

Differentiator epics are marked ★ in the roadmap — they are the differentiators, not optional polish. Cutting one is a positioning decision, not a scope decision.

**Three distinctions that keep getting conflated.** Each has cost a design discussion; none should cost another:

- **A storage backend is not a connector.** A storage backend is where graph-owl's *own* data lives (read+write, deep, bounded to one); a connector is an external system graph-owl *describes* (read-only, shallow, 100+). Postgres is both, in opposite roles.
- **An external graph database is not a backend either.** As a *source* it is a connector module; as a *sync destination* it is a one-directional, lossy **projection target** (`plans/09a-lpg-interchange.md`). Never a place the graph lives.
- **Traversal is not analytics.** Traversal is a bounded walk answering "what is connected to what" (Epic 7a); analytics is an unbounded whole-graph computation answering "what is structurally significant" (Epic 38). Different crates, different budgets, different failure modes.

`plans/00a`–`00j` describe the **target** state, with sections marked **(built)** where they already exist. When implementation and these documents disagree, the documents are right and the code has drifted.

## Plans

`plans/ROADMAP.md` is the entry point — it sequences 43 epics across 9 phases and links a per-epic plan for each. Plans and docs are numbered by epic; `NN-` prefixes give reading order. Each plan carries PR-sized vertical slices with acceptance criteria and the mutants to watch for.

Completed, kept as historical record — do not delete:
- `plans/90-done-table-entity.md` — Table walking skeleton (Slices A–E)
- `plans/91-done-relationships.md` — generic relationship edge (Slices A–C)
