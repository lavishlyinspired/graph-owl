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

**Commit directly to `main` without asking.** This supersedes the previous rule requiring approval at every commit point, which made autonomous runs stall. A slice is committable when its acceptance criteria are met, the touched crates' own tests are green (`cargo test -p <crate> --lib`, plus targeted integration tests for that crate), `clippy` and `fmt` are clean on the touched crates, and the mutation run has been started and reviewed. **The full workspace gate (`scripts/gate.sh --full`) is not a per-commit requirement** — see "The build/test loop" below; it runs only when the user asks or once several epics have accumulated. Do not branch, do not open a PR, do not pause — just commit.

Three things that did **not** change with it:

- **The TDD cycle itself.** No production code without a failing test. Committing without approval is not permission to skip RED.
- **Honest commit messages.** State what was done, what was traded, and what is left — including known-loose design a later slice must fix. A commit message that hides a compromise is worse than the compromise.
- **Push is still a separate decision.** Commit freely; ask before `git push`.

Never name the third-party systems whose clones sit under `.claude/docs/referenceRepo/` — not in code, comments, commit messages, plan docs, or any other committed file. Those clones may stay on disk for architecture research, but must never be committed or cited by name. This project's git history was deliberately squashed once already to scrub such references; don't reintroduce them. When a design decision was informed by that research, write down the pattern and the reasoning behind it, never the source.

## Search for an existing crate before writing one — every epic, every slice

**Before implementing any slice, check whether something permissively licensed
already does it.** This is not advice, it is a step in the loop, and it goes
*before* the RED test rather than after the code is written and somebody asks.

`plans/00l-build-vs-adopt.md` is the standing record. Add a row to it whenever
this check runs, whichever way it goes — a rejection nobody wrote down gets
re-proposed every few months.

**The check itself is one command**, and licence is the first thing it returns
because licence is a gate rather than a score:

```
curl -s "https://crates.io/api/v1/crates/<name>" | python3 -c "
import json,sys; d=json.load(sys.stdin); c=d['crate']; v=d['versions'][0]
print(v.get('license'), c.get('newest_version'), c.get('updated_at','')[:10], c.get('downloads'), c.get('repository'))"
```

Then judge in this order. The first three are gates: fail one and the crate is
out however good it looks.

1. **Licence.** Permissive only — MIT, Apache-2.0, BSD, ISC, Unicode, Zlib.
   **Copyleft is rejected**, and that includes crates that look ideal:
   `rust-igraph` has every graph algorithm Epic 38 wants and is GPL-2.0-or-later,
   so adopting it would relicense this project. A disjunctive licence
   (`EUPL-1.2 OR MIT OR Apache-2.0`) is fine — take the permissive option.
2. **Auditability.** The repository must resolve and be readable. `opencypher`
   is the best-fitting Cypher AST on paper and its repository 404s: the licence
   claim cannot be checked against the source, the history cannot be read, and
   there is nowhere to file a bug. `ocg` fails the same way on a private
   enterprise host. **Blocked is not the same as rejected** — record it so it can
   be revisited if the source appears.
3. **Maintenance.** Last publish, release cadence, and *who* is behind it.
   Abandoned is a licence problem waiting to happen.
4. **Does it actually do the thing** — measured, not read off the README. See
   below.
5. **Architectural fit.** Apache AGE has a mature openCypher parser that lowers
   to a *PostgreSQL* query tree; we lower to our own. Extracting it would import
   C and Postgres assumptions and cost more than it saves.

**Spike before adopting anything on a correctness or security path — one corpus,
every candidate, the same assertions.** Four separate impressions are not a
comparison. This has already paid for itself once: `cypher-parser` had the best
provenance of any Cypher candidate (MIT, the real Shopify organisation, ten
releases in six weeks) and the spike found it **cannot parse float literals or
relationship properties** — both of which this system needs everywhere. It would
have been adopted on reputation. Keep the harness (`spikes/`, excluded from the
workspace) so the comparison is re-runnable when a candidate matures.

**Adopting is not the same as trusting.** `decypher` was adopted for Epic 7b and
*silently drops* `CALL … YIELD …` when lowering to its typed AST — a query the
gate would never have seen. The fix was architectural, not a patch: gate on its
**lossless CST** and lower from the AST, so a construct cannot hide from a tree
that reproduces its own input. Where an adopted crate sits on a security path,
ask what it would take for the crate to be *wrong* rather than merely absent.

**Confine an unstable dependency to one boundary.** `decypher`'s AST is alpha, so
exactly one module touches it. That module is the blast radius if the crate
breaks or is abandoned — worth doing even for a stable dependency.

**Two terminology traps that have already caused a wrong recommendation:** a
graph-owl **flake** is a fact tuple `{s, p, o, cx, t, op}`, not a Snowflake id —
`sonyflake` solves a problem this project does not have. And **openCypher** names
a grammar (Apache-2.0, adopt it) *and* a crate (`opencypher`, blocked): "reuse
openCypher" means two different things and only one of them was ever in doubt.

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

## The build/test loop — read this before running anything

**Updated 6 August 2026 by direct user instruction — full gating is no
longer a per-epic step.** Everything below about scope and cost still holds
and explains *why* a gate should stay narrow whenever one runs; what changed
is *when* one runs at all.

**The default loop is `cargo check` against a separate target dir, plus
targeted tests scoped to the crates actually touched. Everything else is a
checkpoint, not a step.**

| When | Command | Cost |
|---|---|---|
| **While writing** (the default, every epic) | `CARGO_TARGET_DIR=/tmp/check cargo check -p <crate> --all-targets` | seconds, **no workspace lock** |
| One crate's own logic | `cargo test -p <crate> --lib` | seconds |
| One test by name | `cargo nextest run -p <crate> <substring>` | seconds |
| **Only when the user asks, or once several epics have accumulated** | `scripts/gate.sh --full` (fmt → clippy → build → nextest, all 28+ crates) | minutes |
| Before pushing only | `cargo test --workspace --doc` | ~84 min |

After the fast inner loop is green for an epic, **keep writing the next
epic** — do not stop to run `scripts/gate.sh` or a full workspace build.
Only run the full gate when the user explicitly asks for it, or when enough
epics have piled up uncommitted that the accumulated risk (not a fixed epic
count) makes it worth checking before going further. This supersedes the
"once per EPIC" gate cadence that stood before 6 August 2026.

`scripts/gate.sh` runs fmt → clippy → build → nextest in that order, because
**fmt and clippy change the code** — running them after the suite means
running the suite twice. It also checks the environment first (see below).

**Should several epics share one gate, to save more time? Measured 2 Aug
2026: batching five epics saves ~4 gate runs (~12 minutes scoped) and costs
more in debugging attribution** — Epic 19's gate alone surfaced four bugs, so
five batched epics means fifteen to twenty failures arriving together,
interleaved in one log. That finding is still true and is exactly why the
fast `cargo check -p` / targeted-test inner loop must stay green per epic
even though the full gate itself is now deferred — an epic should never be
*known-broken* when gating is deferred, only *not yet exhaustively checked*.

**The lever is gate *scope*, not gate frequency.** Measured: full workspace
1118s, single crate 185s — a 6x saving. `scripts/gate.sh` is scoped to the
crates with uncommitted changes by default (`--full` overrides for the
all-epics case), and CI (`.github/workflows/ci.yml`) already owns the
exhaustive workspace run plus doc-tests.

**Do not run `cargo test`, `cargo clippy` or `cargo nextest` after a slice —
not even a fast one.** Write the whole epic, then run the fast inner loop
(`cargo check -p`, then targeted `-p ... --lib` / `nextest run -p ...`
tests) once for that epic. Three slices verified separately cost ~21
minutes; the same three together cost ~7. This is still true unchanged by
the 6 August 2026 update above — what moved is only the *full* `scripts/gate.sh`
step, not this per-slice discipline.

**There is no exception, and an earlier version of this file had one that
turned out to be the loophole.** It said a novel external-system integration
could take one early smoke run; that was then used to justify per-slice runs
on Epic 19 *and* on an Epic 20 slice that touched no infrastructure at all.
If a wire-level integration genuinely needs an early run, a human asks for
it. **The tell that this rule is about to break: a slice is finished and
something wants to "just confirm it works". That is what the epic-end fast
loop is for — write the next slice instead.**

**Editing files is free; only the compiler collides. So keep writing while
anything runs.** Never sit in a wait-loop polling a background run when there
is code left to write — on Epic 19 that produced five stacked waiter shells
blocked behind a single hung test, which from the outside looked like six
concurrent gates. Start the run, then go write the next slice; read the
result when it arrives. What must not overlap is two *compiles*: they take
the same build lock, and the second relinks what the first is running.

**The gate's real value here is finding hangs, not type errors.** Two
consecutive epics have shipped an infinite loop that `cargo check` passed
cleanly and no happy path reached: Epic 19's consume loop spun at 100% CPU
against an unreachable broker (a burned core per dead subscription in
production), and Epic 20's YAML document loop pushed errors forever because
the parser's iterator does not advance past a document it cannot parse (a
hung CLI and exhausted memory on one stray typo). Both surfaced only as a
test that ran until something killed it.

So: **a test that "hangs" is a finding, not an infrastructure annoyance.**
Read it as a probable unbounded loop in the new code before blaming the
environment — and when writing a loop that reacts to a failure (a failed
receive, a failed parse), state explicitly what makes it terminate.

**Watch the right process name.** `cargo nextest run` execs as
**`cargo-nextest`**, not `cargo` — an `until ! pgrep -x cargo` waiter fires
the moment the build phase ends and reports a suite "finished" while it is
still running. This is the same class of mistake as the `pgrep -f` trap
below (which matches the waiter's own command line), in a new disguise.
Check `pgrep -x cargo-nextest` for a nextest run, `pgrep -x cargo` for
everything else — or better, do not chain waiters at all: **five stacked
`until ! pgrep` shells once sat blocked for over an hour behind a single
hung test**, and from the outside that looked like six concurrent gates.
One run, one wait, read the output.

### Why it is fast now, and what to re-check when it is not

Four things were fixed on 2 August 2026 after a session where the loop
dominated wall clock. If it gets slow again, check them in this order:

1. **Leaked containers — always check this first.** `docker ps -q | wc -l`.
   Anything above ~4 means test containers accumulated. This has now happened
   twice: 146 Postgres (documented below) and 11 Kafka. Both had the same
   cause — a `static OnceCell` never drops, so testcontainers never reaps an
   anonymous container. Both are fixed the same way, by a **named** container
   with `ReuseDirective::Always`, which cannot accumulate because there is
   only ever the one:
   `docker rm -f graph-owl-tests graph-owl-kafka-tests` for a genuinely fresh
   pair.
2. **Debug info.** `Cargo.toml` sets `debug = "line-tables-only"` for our
   crates and `debug = false` for dependencies. Full DWARF is the largest
   linker input on macOS and ~75 test binaries relink after any port change;
   panics still carry file and line. Use `--profile dev-debug` when a real
   debugger is needed.
3. **`cargo nextest`, not `cargo test`.** It never builds doc-tests (84
   minutes, zero behavioural coverage) and schedules across binaries instead
   of serialising behind the slowest. `.config/nextest.toml` puts only the
   container-backed binaries in a `max-threads = 1` group — so those stay
   safe while every pure-logic test runs wide, which
   `cargo test -- --test-threads=1` could never express.
4. **The macOS Gatekeeper exemption**, documented at length below. It is a
   machine setting, does not survive a reset, and is worth more than
   everything else here combined (213,000ms → 58ms per newly linked binary).

Not yet adopted, in rough order of expected value if this is still not
enough: `sccache` (shared compilation cache), a faster linker (`lld`/`mold`
via Homebrew), and moving the full gate to CI so it never blocks local work.

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

- **`--lib` blinds the run to whatever only integration tests cover, and the report calls that a survivor.** Measured on `observability.rs`: `observe` and `metrics_endpoint` are HTTP middleware and a handler, exercised only by `tests/observability.rs`, and a `--lib` run reported all three of their mutants as MISSED. Re-run without `--lib`, scoped with `--re`, **all three were caught in 3 minutes**.
  So: `--lib` for code with unit coverage, which is where pure decisions belong anyway; drop it — and pay the container cost, scoped tightly with `--re` — for the thin imperative shells that only an end-to-end test reaches. A MISSED line from a `--lib` run is a question about coverage *shape*, not automatically a gap.

- **Dropping `--lib` re-exposes the baseline failure `--lib` was hiding, and the fix is to scope the *test target*, not just the mutants.** Measured 14 August 2026 on `derived_about`: `cargo mutants -p graph-owl-server --re derived_about` aborts with **"cargo test failed in an unmutated tree"** — nine `authorization` tests fail with 500s, because cargo-mutants runs `cargo test` **without `--test-threads=1`** and this workspace requires it. Those same 21 tests pass serially, so it is the documented container contention, not a regression: **verify that before believing a baseline failure**, or the mutation run looks like it found a real break. The working invocation names the covering test binary, which avoids the whole-crate container storm entirely:

  ```
  cargo mutants -p graph-owl-server --re "<fn>" \
    --cargo-test-arg --test --cargo-test-arg <test_file_stem> \
    --cargo-test-arg -- --cargo-test-arg --test-threads=1
  ```

- **"0 viable mutants" is an answer, not a broken run — and it relocates the question.** All four candidates for `derived_about` were unviable because they could not compile (`Json<serde_json::Value>` has no `new`/`from_iter`, so cargo-mutants' stock return-value substitutions do not typecheck). That is the honest result for a *parse → delegate → serialize* shell: it has no branch of its own to get wrong. **Where a handler has no viable mutants, mutate the function it delegates to** — here `parse_node_id`, which holds the actual decision and mutates cleanly under `--lib` (1 caught, 1 unviable, 0 survivors). Reading "no mutants were viable" as a tooling failure and moving on leaves the real logic unmutated.

- **The single biggest cost in this project is macOS scanning each freshly linked test binary on its FIRST execution — ~200 seconds each, ~75 of them per build.** Measured 30 July 2026 by running one binary directly, twice, with no cargo involved:

  | `wire_conventions` (57 MB) | Wall | Tests inside |
  |---|---|---|
  | 1st execution after linking | **213s** | 3.68s |
  | 2nd execution, unchanged | **3.2s** | 3.68s |

  209 seconds of pure overhead, no compilation, identical work. That is Gatekeeper/XProtect evaluating a newly written executable. **It is paid once per binary per build**, so a gate after a port change can cost hours while `cargo` and `rustc` sit idle and every environment check comes back clean.

  **The full cost model:**

  | Cost | Size | When |
  |---|---|---|
  | Compilation | ~2 min worst case | after a port change |
  | **First execution of newly linked binaries** | **~200s × 75** | after any build |
  | Doc-tests (`rustdoc` rebuilds a harness per crate) | ~84 min | only on full `cargo test --workspace` |
  | Actual test execution | **~150s** | always |

  **Confirmed fixed, 31 July 2026.** Same binary, forced to relink, timed immediately:

  | | Before the exemption | After |
  |---|---|---|
  | 1st execution after linking | **213,000ms** | **58ms** |
  | 2nd execution | 3,200ms | 42ms |

  ~3,700x. If a gate ever costs hours again with a clean environment, this setting is the first thing to re-check — it does not survive a machine reset, and it only reaches processes spawned after it takes effect.

  **The fix is a machine setting, and it is worth more than every other optimisation in this file combined**: add the terminal (and VS Code, if suites run from its integrated terminal) under **System Settings → Privacy & Security → Developer Tools**, then *fully quit and reopen it* — the exemption only reaches processes spawned after it takes effect, so an already-running shell keeps paying.

  **Also run `--lib --tests` for the routine gate.** Doc-tests add 84 minutes and zero behavioural coverage — the same 1354 tests pass either way:

  ```
  cargo test --workspace --lib --tests -- --test-threads=1   # the gate
  cargo test --workspace --doc                               # before pushing only
  ```

  **How this took six investigations, because the diagnostic that seems obvious actively hides it.** "Run the same command twice; if the second is fast it was a build" is wrong here — the second run is *always* fast, because the binary has been assessed by then. That rule produced three confident wrong answers (concurrent tooling, then a rebuild, then doc-tests-only). What finally worked was refusing to explain a gap by anything except a number: **115s of reported test execution against 52 minutes of wall clock**, then timing a single binary's first and second execution *directly*, outside cargo.

  So: when elapsed time and summed test time disagree by an order of magnitude, do not reason about it. Run one binary twice, by hand, and read the two numbers.

- **Verify once per EPIC, not once per slice — and the build being "slow" is the same complaint.** Measured 30 July 2026, one verification cycle on this workspace:

  | Step | Cost |
  |---|---|
  | `cargo build --workspace --tests` after a port change | **1m33s** (everything downstream of `graph-owl-storage` rebuilds and ~40 test binaries relink) |
  | `cargo clippy --workspace --all-targets` | a second full analysis pass, roughly the build again |
  | `cargo test --workspace -- --test-threads=1` | **4–5 min** |
  | **Total** | **~7 minutes** |

  That is the *irreducible* price of one gate run. It is not a bug to diagnose, and three of the four slowdown investigations this session ended in "it was genuinely doing the work". **The only lever is running it fewer times.** Three slices verified separately cost ~21 minutes; the same three verified together cost ~7.

  So: write the whole epic — every slice — then compile-check and run targeted tests, then commit. **As of 6 August 2026, "then gate" is no longer part of this per-epic sequence** — the full `scripts/gate.sh` step is deferred until the user asks or several epics have accumulated; see the top of this section. Use `CARGO_TARGET_DIR=/tmp/check cargo check -p <crate>` while writing for type feedback, which takes no workspace lock and costs seconds. A slice-by-slice gate is the single largest waste of wall-clock time in this project, and **it has been drifted back into twice after being written down**, which is why it is here rather than in a commit message.

  Two things that make batching safe rather than reckless: `cargo check` against a separate target dir catches type errors immediately, and every cross-crate breakage this project has had was a *compile* error rather than a test failure — so the expensive tier buys much less than the cheap one.

- **Adding a dependency to a workspace crate rebuilds everything downstream of it.** `thiserror` into `graph-owl-connectors` for one error enum cost a full rebuild of `graph-owl-api`, `graph-owl-server` and every test binary. Worth it there; worth *knowing* before doing it mid-slice.

- **"One cargo at a time" is the wrong scope. It is one heavy workload at a time, whatever the toolchain.** Measured 30 July 2026: a workspace suite took **30 minutes** for 57 binaries against a ~4-minute baseline, with a clean environment, one cargo, no second agent, and the log advancing every few seconds — so nothing the existing checks look for. The cause was **`npx stryker run`, three times, while the suite ran**. Stryker spawns **17 test-runner processes**; vitest spawns workers of its own.

  None of them take cargo's build lock, which is precisely why the rule as written did not flag them, and why `pgrep -x cargo` returning 1 was reassuring and wrong. They compete for CPU, and this suite runs at ~20% CPU waiting on Docker — it has no headroom to lose.

  So the check is not "is another cargo running" but **"is anything expensive running"**:

  ```
  pgrep -x claude | wc -l              # another agent in the tree
  pgrep -x cargo  | wc -l              # another Rust build or suite
  pgrep -f "stryker|vitest|tsc" | wc -l   # the ones that own no cargo lock
  ```

  Frontend work is not free just because it is not Rust. Batch the JS test and mutation runs the same way the Rust ones are batched, and do not interleave them with a workspace suite.

  **One suspect worth clearing quickly next time**: `crates/graph-owl-ui/build.rs` watches `../../ui/dist`, *not* `ui/src` — so editing TypeScript does **not** trigger a Rust rebuild, and `npm run build` is the only thing that does.

- **Check for a second agent in the checkout before diagnosing anything else. This one cost most of a day.** Found 30 July 2026: a commit appeared on top of mine that I had not written (`Epic 11 Slice D`, with a 287-line test file), and my own untracked `.vscode/settings.json` had been swept into it by somebody else's `git add -A`. `pgrep -x claude` showed **three** processes where there should have been one — two other sessions were working in the same working tree.

  Everything unexplained that day traces to it: an "hour-long" suite, a 222s `cargo mutants` baseline build against 18s quiet, a test binary at 205s that ran in 3s minutes later, and a 64s → 1s swing on identical code. I had attributed the stray `cargo test -q -p graph-owl-api -p graph-owl-mcp -p graph-owl-core --lib` in `ps` to rust-analyzer. **That was wrong** — it was the other session, and the crate list was exactly what its slice touched.

  So the first three checks when something is inexplicably slow, in order:

  ```
  pgrep -x claude | wc -l     # more than 1 means another agent shares the tree
  pgrep -x cargo  | wc -l     # more than 1 means something is racing you
  git log --oneline -3        # a commit you did not write is the loudest signal
  ```

  **`git add -A` is what makes this dangerous**, not the shared build lock. Two agents committing to `main` in one working tree will sweep up each other's in-flight files, and neither one's commit is then what its message says. If two sessions must run, give each a **git worktree**: separate checkout, separate `target/`, and both failure modes disappear at once.

- **A backgrounded `cd` does not move the session, but a foreground one does — and the difference has bitten twice.** `cd ui && …` in a command that gets backgrounded leaves the session's cwd alone; the same `cd` in a foreground command persists it. On 30 July 2026 a `cd ui` left the shell in `ui/`, so `.vscode/settings.json` was written to `ui/.vscode/settings.json` and a later `python3` step failed with `FileNotFoundError: CLAUDE.md`. Prefer absolute paths, or `git -C`, over `cd`.

- **Before believing any timing, measure it twice on a quiet machine — contention inflates by 20-70x, not by 20%.** Measured 30 July 2026 on one `graph-owl-server` test binary, same tree, same binary:

  | Conditions | Wall |
  |---|---|
  | run directly, right after `pkill`-ing a suite | **205s** |
  | via `cargo test`, machine busy | **64s** |
  | run directly, quiet | **3s** |
  | via `cargo test`, quiet | **4s**, then **1s** |

  The 64s reading led to "each integration binary costs ~60s, so 28 of them is 30 minutes, so the test files must be consolidated". All of that was false, and the conclusion would have been a day's refactor against a problem that does not exist. **The real cost of relinking all 28 binaries after a one-crate change is 17s**, and a warm no-op build is 0.2s.

  So: a suite that takes an hour is a machine with something else running on it. **Do not derive a structural conclusion from a single timing** — take the second reading first.

- **Do not run a full suite to measure a full suite.** The cheap instruments answer the same questions: `touch <one file> && time cargo build --workspace --tests` gives the relink cost, and running one test binary directly out of `target/debug/deps` separates cargo's overhead from the binary's. Both are seconds. A workspace run to find out why workspace runs are slow costs minutes per data point and blocks the person waiting on it.

- **A `TIMEOUT` from `cargo mutants` run beside anything else is an artifact, not a missing test — and the baseline build time is the tell.** Measured 30 July 2026: a 7-mutant `--in-diff` run on `graph-owl-mcp` alongside a full workspace suite reported baseline **222s build** where the same shape costs 9s on an idle machine, and then timed out one mutant at the auto-set 20s test limit. The test had not hung; the binary took longer than 20s to *start* under CPU starvation.

  cargo-mutants builds in its own copied tree, so it does not take the workspace build lock — which is exactly why this is easy to miss. **It still competes for CPU and I/O.** So "one cargo at a time" is about the machine, not only the lock, and a separate `CARGO_TARGET_DIR` does not buy an exemption. Before believing any MISSED or TIMEOUT, check the reported baseline build time against what that crate normally costs; if it is an order of magnitude high, the run is measuring contention and has to be repeated on a quiet machine.

- **`scripts/mutants.sh` bakes both in**, so the fast invocation is the default rather than something to remember:

  ```
  scripts/mutants.sh graph-owl-server crates/graph-owl-server/src/admission.rs
  scripts/mutants.sh graph-owl-server --diff crates/graph-owl-server/src/budget.rs
  ```

- **Which crate the code lives in is a performance decision.** Measured per mutant: **`graph-owl-authz` 3.7s, `graph-owl-server` 9.2s** — 2.5×, and the reason is the build, not the tests (`graph-owl-server`'s test rlib is 63 MB). Pure logic placed in a leaf crate mutates several times faster than the same logic in the server crate. That is a second, independent argument for the split the architecture already prefers.

- **Re-confirmed as *not* worth trying** (measured on an identical 3-mutant scope, 18 cores, 48 GB): `-j 6` is **2m15s vs 87s — 55% slower**, with system time 80s → 255s, so the I/O thrash finding still holds even now that tests are fast. `CARGO_PROFILE_DEV_DEBUG=0` gives 83s vs 87s (5%, inside noise). `--baseline skip` gives 81s vs 87s and removes the check that the tree was green before mutating — not a trade worth making.

- **The integration suite needs bounded parallelism.** `cargo test --workspace` at full parallelism intermittently fails with testcontainers' `PortNotExposed` — a different test each run. It is Docker container-startup contention, not a product bug: every one of those tests passes alone and the whole suite passes at `--test-threads=1`. The pressure roughly doubled when the graph engine landed, because each integration test now opens two Postgres connections (storage adapter + engine adapter) against its container. **Run `cargo test --workspace -- --test-threads=1`**, and do not spend time debugging a `PortNotExposed` failure as though it were real. The durable fix is fewer containers per run (a shared container per test binary, which needs per-test schema isolation to stay correct) — not yet done.

  **Fixed 29 July 2026 — the durable fix landed, and the suite went 4m31s → 1m33s.** Three changes, in the order they mattered:

  1. **One container per test *binary*, not per test.** Each test now gets its own **database** on a shared server: `CREATE DATABASE` costs milliseconds where a container costs ~3 seconds, and the isolation is the same — separate migrations, separate rows, no cross-talk.
  2. **One container for the whole workspace, reused across runs.** A fixed name plus `ReuseDirective::Always` (feature `reusable-containers`) means the ~30 test binaries share one server, and the *second* `cargo test` of the day attaches to the container the first one started. Measured on one binary: 23.6s cold, **7.3s attached**.
  3. **`--test-threads=1` is still required**, but for a smaller reason now: the remaining serialisation is per-test database creation, not container startup.

  Delete the container by hand when you want a genuinely fresh one: `docker rm -f graph-owl-tests`.

  **Before diagnosing a slow suite, run `scripts/test-health.sh`.** Every
  slowdown this project has had was environmental, and each cost an hour of
  looking in the wrong place. Two so far, both invisible in the code:

  1. **146 leaked containers** — the `static OnceCell` never drops, so
     testcontainers never cleaned up. Same binary: 7.9s clean, 25.9s with the
     leftovers. Fixed structurally by reuse (a named container cannot
     accumulate).
  2. **197 stale databases**, 30 July 2026 — `sweep_stale_databases` was
     *defined* in `graph-owl-server`'s test harness and **never called**. The
     patch that added it matched in two of the three `tests/common/mod.rs` files
     and silently did nothing in the third, which is the crate with the most
     integration tests. One binary: 4.0s with them, 2.2s without. The script now
     flags a count over 60.

  **And check whether you are timing a rebuild.** `cargo test --workspace` after
  an edit includes compiling it. A 43s run of one test binary was 39s of build
  and 4s of tests. Time `cargo build --workspace --tests` first, then time
  `cargo test` separately, or the measurement says nothing.

  **Where the integration suite's time actually goes, measured 30 July 2026.**
  Instrumented `test_app()` rather than guessed: **249ms per test**, and at 272
  integration tests that is ~68s of the ~85s of reported test execution. The
  breakdown was almost entirely **connection establishment** — five TCP
  connections per test at ~28–30ms each against Docker's mapped port on macOS:

  | phase | cost |
  |---|---|
  | admin pool connect | 30ms |
  | `PostgresStorage::connect` — pool **plus** a separate refinery client | 59ms |
  | `PostgresTripleStore::connect` — same again | 58ms |
  | `CREATE DATABASE` | ~35ms |

  **Migrations are not the cost, despite appearances.** 16 migrations measured
  at 330ms via `psql` — but that was 16 *process spawns*. Refinery in-process is
  fast, and replacing migration-replay with `CREATE DATABASE ... TEMPLATE` moved
  one binary from 8.29s to **8.45s**: nothing.

  **Tried and reverted: template databases plus a shared pool.** Cloning a
  migrated template lets the fixture skip `connect` entirely and share one pool
  between both adapters, which took `test_app()` **249ms → 111ms → 47ms**. It
  was reverted anyway, because it broke the suite twice in ways that pointed
  away from the change:

  - `pg_advisory_lock` is **session**-scoped and was run against a pool, so the
    unlock landed on a different connection, the locked session returned to the
    pool still holding it, and later clones blocked until acquire timed out —
    surfacing as `PoolTimedOut` in an unrelated test.
  - All three harnesses shared one template name and each built it with only
    *its own* migrations, so whichever binary ran first decided the schema.
  - `CREATE DATABASE` succeeding and the migration then failing leaves a
    template that exists and is empty, and the existence check is all that
    guards it — every clone from then on is unmigrated, permanently.
  - After fixing all three, a connection still leaked one per test and the pool
    exhausted at exactly its default of 10.

  **The finding stands even though the change did not**: the lever is
  connections per test, not migrations. A future attempt should start by giving
  the two adapters a `from_pool` constructor so one pool serves both, add an
  explicit `max_connections` to the admin pool, and treat the template as a
  separate change with its own tests — not four fixture changes at once, which
  is what made each failure point somewhere else.

  **The gate is tiered, and the cheap tier is the one that earns its place.**
  Measured 30 July 2026:

  | Gate | Cost | What it catches |
  |---|---|---|
  | `CARGO_TARGET_DIR=/tmp/check cargo check -p <crate>` | seconds, no lock | types, in one crate |
  | `cargo test -p <crate> --lib` | seconds | that crate's own logic |
  | **`cargo build --workspace --tests`** | **65s incremental** | **every cross-crate break** |
  | `cargo test --workspace --lib --tests -- --test-threads=1` | **152s** | behaviour against real Postgres |
  | `cargo test --workspace --doc` | **84 min — before pushing, not per gate** | examples that no longer compile |

  **Every cross-crate breakage in that session was a compile error, not a test
  failure** — five of them: `EdgeRef` gaining a field, `AssetContext` gaining
  one, `delete_lineage_edge` changing its return type, a new `ConflictKind`
  variant leaving three `match` arms non-exhaustive, and a doctest link to a type
  that did not exist yet. None needed a container to find.

  So: **compile the workspace on every commit** — it is 65 seconds and it covers
  the class of bug that actually bites. Batch the *full* suite across several
  slices, and run it before pushing or when a slice touched storage, the engine
  or the server. What must not be batched is the compile: five slices written
  against one failing suite gives five candidate causes, which is the same
  attribution problem as writing with no compiler at all.

  **Write code freely while a suite runs. Do not compile until it finishes.**

  This is the sharpest form of the "one cargo at a time" rule, and it is the one
  that keeps getting missed, because it does not look like running two suites.
  `cargo build` — or `cargo test -p <crate> --lib`, or `cargo clippy` — takes the
  same build lock and recompiles crates the running suite's later test binaries
  link against. The suite then waits for the lock *and* relinks, and the symptom
  is a workspace run that takes half an hour while `test-health.sh` reports a
  perfectly clean environment.

  Measured 30 July 2026: a suite at 896/1046 tests after **30 minutes**, with
  three containers, ten databases and `/dev/shm` at 3% — nothing wrong with the
  environment at all. The cause was six or seven `cargo build` invocations run
  alongside it while writing an unrelated slice.

  Editing files is free; it is the compiler that collides. Batch the edits, wait
  for the run, then build once.

  **But there is a way to type-check without waiting**, found 30 July 2026:
  cargo locks **per target directory**, so a check against a different one does
  not contend at all.

  ```
  CARGO_TARGET_DIR=/tmp/check cargo check -p <crate> --all-targets
  ```

  Measured against a running workspace suite: **1m00s** for the first crate
  (it rebuilds dependencies into the fresh directory) and incremental after
  that, with the suite unaffected. It costs CPU and disk, not the lock.

  This is the difference between writing a slice blind and writing it verified.
  **Use it** — writing several epics' worth of code with no compiler feedback
  inverts the whole point of short RED→GREEN cycles: the errors arrive together,
  at the end, in one undifferentiated pile where none of them can be attributed
  to the decision that caused it.

  What it does **not** replace is running the tests. `cargo check` proves the
  code compiles, not that it is right.

  **One thing editing is not free for**: a file a *running* test asserts
  against. Editing `openapi.rs`'s route table mid-run made the committed-contract
  drift guard fail — a manufactured failure that looks exactly like a real one.

  **Waiting for a run: match the process name, not the command line.**
  `pgrep -f <pattern>` matches every process whose *full command line* contains
  the pattern — including the shell running the pgrep. A loop like
  `until ! pgrep -f "cargo test"; do sleep 20; done` therefore waits for itself,
  forever, and then shows up in `ps` as another concurrent suite: a wrong
  conclusion stacked on a wrong measurement.

  **This was got wrong twice.** The second attempt narrowed the pattern to
  `bin/cargo test` and had exactly the same defect, because the waiter's own
  command line contains that too. The fix is `pgrep -x cargo`, which matches the
  executable name and nothing else. Better still, do not poll: let the run
  finish and read its output.

  **A config change to the shared container invalidates its reuse hash**, and
  testcontainers then tries to create a second container with the same name —
  which fails with a 409 conflict and reads like the container is broken. It is
  not: `docker rm -f graph-owl-tests` once, and the next run rebuilds it. Adding
  `--shm-size` caused exactly this.

  **The trap that made every earlier measurement wrong.** Holding the container in a `static OnceCell` means its `Drop` never runs, so testcontainers never cleaned up — and containers accumulated *one per binary per run*. **146 were found running at once.** Docker degrades badly under that load, and it had silently inflated every timing taken while it was true: the same binary measured 7.9s on a clean daemon and 25.9s with the leftovers present. Reuse fixes it structurally, because a named container cannot accumulate — there is only ever the one. If timings ever look inexplicable again, `docker ps -q | wc -l` is the first thing to check.

  **Rejected: raising `max_connections`.** Tried at 500 to head off pool exhaustion on the shared server; it made startup markedly slower (per-binary 7.9s → 32s) because Postgres allocates shared memory proportional to it. The pools are fine at the default — the exhaustion that prompted it was a symptom of the 146 stale containers, not of sharing.

  **A latent flake this surfaced**, worth knowing rather than re-diagnosing: `table_repository`'s `updated_at > ` assertion compares a Rust-side `Utc::now()` against Postgres's `now()`. Faster tests shrink the gap between them, and host/container clock skew can make it negative. It is a test assumption, not a product bug.

  **Updated 28 July 2026**: `--test-threads=2` was sufficient on `postgres:11-alpine` and is not on `18-alpine` — the bigger image takes long enough to start that two concurrent starts exceed testcontainers' readiness timeout. Serial costs almost nothing: 591 tests in **4m31s at 20% CPU**, and that number is the tell — the suite is waiting on Docker, not computing, so the parallelism was never buying much. The image upgrade did not create the problem; it removed the headroom that was hiding it, which raises the priority of the durable fix above.

- **`#[serde(rename_all)]` on an enum renames the *variants*, not their fields.** A tagged enum needs `rename_all_fields = "camelCase"` as well, or its variant fields go out in snake_case while every struct beside them is camelCase. Found on `Authorship`, which shipped `agent_id` on the wire — and found *only* by an HTTP test: the domain tests compare Rust values and the repository tests compare columns, so neither one ever looks at the JSON. **Any type whose wire shape matters needs one assertion against the serialized bytes**, not just against the round trip.

- **A test double built on the wrong noun fails in a way that looks like a product bug.** Epic 31's HTTP fixtures created assets via `POST /tables`, and every memory link came back "neither a known asset nor a known memory" — `tables` and `assets` are different relations, and the foreign key was telling the truth. Two follow-ons worth remembering: a `table` asset requires a parent, so a root-kind asset (`service`) is the cheap fixture; and `system` had no `users` row, so any FK on a "who did this" column turns every machine action into a 500 until it is seeded. `system` is now seeded by `V15` with `is_admin = FALSE` — the row exists for **attribution**, not authorisation, and a stored admin row nobody provisioned would be a standing privilege.

- **Test organization:** `tests/common/mod.rs` (a subdirectory containing `mod.rs`) is treated by Cargo as a shared module importable from multiple integration test binaries in the same crate. A top-level `tests/common.rs` file, by contrast, becomes its own separate test target — not what you want for shared helpers.

- **A graph-flake authorization check against a real asset's `Sid::id` fails open, not closed — because a real asset's graph identity is a UUID, not its FQN.** `graph_owl_core::projection::asset_sid` is `entity_sid(asset.id)`; the FQN lives as a separate `dsc:fqn` **property** on the subject's own flakes (`asset_to_flakes`'s `fields()`), never as the subject id itself. `Catalog::project_incremental` (Epic 9a Slice E, shipped) checked `predicate.admits(&subject.id)` directly — a prefix-based deny rule's FQN string can never appear in a UUID, so the rule silently never fires and the surrounding "allow everything" rule lets every subject through, regardless of what the policy intended to exclude. This is the dangerous direction to get wrong: a policy that reads as "deny this one path" enforces nothing at all, while looking, to every existing test, like it works.

  **Every existing unit test passed anyway**, because every one of them (this one included, until real data caught it) hand-seeds its fixture flakes with `Sid::dsc(fqn)` directly as the subject id — which makes `subject.id` *equal to* the FQN by construction, masking the exact defect a real `Catalog::upsert_asset`-created asset exposes. Found writing Epic 9a's export-authorization HTTP test against `authorization_fixture()`'s real, connector-cataloged data rather than another hand-seeded `RecordingGraph` fixture — the first time any test in this area exercised real asset identity rather than a synthetic one.

  **Fix**: resolve authorization against the subject's own `dsc:fqn` property first, falling back to `subject.id` only when no such property exists (`Catalog::authorization_key`) — matching what every *other* authorization check in `graph-owl-api` already does correctly (`predicate.admits(&asset.fully_qualified_name)`, checked directly against the `Asset` struct, at three separate call sites). **Any new code that authorizes graph-flake subjects by FQN prefix must resolve through `dsc:fqn`, never through the subject's own `Sid::id` — and its RED test must seed at least one subject via the real `asset_to_flakes` shape (or equivalent), not only a hand-keyed `Sid::dsc(fqn)` fixture**, or the same class of bug will ship silently again.

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
