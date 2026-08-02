# Plan: Metadata-as-Code (Epic 20) ★
**Branch**: feat/metadata-as-code
**Status**: Shipped (Slices A–G, plus the HTTP client and the `graph-owl` binary)
**Depends on**: Epic 15 (idempotent upsert and reconciliation machinery)
**Differentiator** — the flagship. See `plans/00a-product-position.md`.
**Crates**: **`graph-owl-cli`** (new — plan/apply/drift/export) · `graph-owl-core` (declaration types) · `graph-owl-api` (reconciliation, reusing Epic 15's scoped machinery)

## Goal

Declare catalog state in version-controlled files and reconcile continuously. Review metadata changes in a pull request; roll back with `git revert`.

## Why here, and why it is the flagship

Catalogs decay because curation lives in a UI, disconnected from the code that produces the data. When the description and the schema live in the same repository and move through the same review, they change together.

Existing catalogs treat the API as primary and files as an import format. Making **declarations primary** — with a plan shown before anything mutates, and drift reported rather than silently corrected — is a different product, not a feature.

It lands right after connectors because Epic 15 already builds FQN-keyed idempotent upsert, converging re-runs, enumeration-scoped reconciliation, and deletion detection with completion gating. This epic points that machinery at a directory instead of a database.

## Resolved decisions

1. **Plan before apply, always.** `apply` shows what it will do and requires confirmation (or `--yes`). A tool that mutates a catalog without showing its plan will not be trusted with a catalog.
2. **Declarations are authoritative only within their declared scope.** A file tree scoped to `service: snowflake_prod` never touches anything outside it. Without scoping, one misconfigured repository can tombstone a catalog — the same hazard as Epic 15's deletion detection, and it reuses that machinery.
3. **Drift is reported, never auto-corrected.** Detection and correction are separate commands. Automatic correction turns every manual fix into a silent revert.
4. **Curated fields are not clobbered by absent declarations.** Omitting `description` means "not declared", not "set to null". Explicit null is how you clear a field. This mirrors Epic 15's merge rule.
5. **Deletion requires an explicit opt-in and passes a threshold guard**, exactly as connectors do.
6. **The CLI is a thin client over the HTTP API.** No direct database access — the same authorization and validation apply, and the tool works against a remote instance.
7. **Export is lossy by design.** It emits declarable state, not history, versions, or system-derived fields. Round-tripping export → apply must be a no-op.
8. **The CLI's scope is bounded now, not after it has 40 subcommands.** See below — this decision exists because the crate already exists with no stated limit.

### The CLI's scope boundary

`graph-owl-cli` exists for this epic, and without a stated boundary it will not stay that way — a reference CLI in this space carries 40+ subcommands across auth, query, import, export, history, indexing, memory, server, and cluster management, each of which arrived one reasonable request at a time.

**The rule: the CLI is for things a *terminal or a CI job* does better than an HTTP call.** That is a small set.

| In | Why |
|---|---|
| `plan` / `apply` / `diff` / `validate` | This epic. A git-driven workflow needs a git-shaped tool |
| `export` / `import` | `37b-portability.md`. Streaming a multi-gigabyte archive is a file operation |
| `login` | Obtaining a token for the above |

| Out | Where instead |
|---|---|
| `query`, `search` | The API, the console workbench (Epic 41), or any Bolt client (Epic 7d) |
| `serve`, cluster and node administration | The binary is the server; a CLI wrapper adds a layer with no capability |
| Entity CRUD subcommands | This is `curl` with extra steps, and it doubles the API surface's maintenance |
| `history`, `memory`, index administration | The API. A CLI verb per capability is how 40 subcommands happen |

**Conventions**, per the global CLI guidance: data to stdout and diagnostics to stderr, so output pipes cleanly; `--format json` on every command that prints structured data; exit codes that mean something (`0` no changes, `1` error, `2` changes pending) so CI can branch without parsing text; no interactive prompt unless stdin is a TTY, so the same command works in a pipeline.

## Acceptance criteria (feature level)

- [x] A directory of YAML applies to an empty catalog, creating the declared hierarchy — proven end to end against a real router and a real Postgres.
- [x] A second apply with unchanged files is a no-op — zero versions, zero events. Structural rather than incidental: an unchanged entity is classified `NoChange` and `in_dependency_order` omits it entirely, so there is nothing to send. Tested from both directions.
- [x] `--dry-run` prints an accurate plan and mutates nothing — the plan is a pure function of declarations and live state, with no write path to reach.
- [x] An entity removed from the files is tombstoned only with `--prune`, and only within scope. Two independent guards, both tested, including the prefix-boundary case (`service_a` must not claim `service_ab`).
- [x] `drift` reports divergence between declared and live state without changing anything — and distinguishes "someone edited live" from "the file changed and was never applied", which a plain diff cannot.
- [x] `export` produces declarations that re-apply as a no-op — the round-trip test asserts exactly that.
- [x] A CI mode fails a pull request whose plan would delete assets, with exit codes that separate "pending changes" from "error" so a real diff is not a broken build.
- [ ] A malformed or schema-invalid file fails before anything is mutated.

## What is built, and what is not

**Built and tested (51 tests, including two against a real catalog):** the declaration format and its local validator (all errors reported, never the first); plan computation with per-field before/after and byte-identical determinism; apply *ordering* and the consent rule; scope and threshold guards for pruning; drift classification; export round-trip; exit codes and credential redaction.

**The end-to-end test found what the doubles could not, immediately.** The server's `UpsertAsset` takes **`parentId` as a UUID**, not `parentFqn` as a string — so against a real catalog every child entity would have been refused, while the recording double went on accepting the wrong shape indefinitely. That is the whole argument for one real test at an epic's end: a double answers "did we make the right decision?", and only something that can say *no* answers "did we speak the right protocol?". The fix was a design change rather than a patch — `upsert` now returns the id the catalog assigned and apply threads a `ParentIds` map through the loop, which works only because apply runs parents-first. The ordering guarantee turned out to be load-bearing for a second, unanticipated reason.

**Now built:** the HTTP client (`http.rs`, blocking `reqwest` — the CLI does one bounded sequence of requests and exits, so an async runtime would buy nothing) and the `graph-owl` binary with five subcommands: `validate`, `plan`, `apply`, `drift`, `export`. The list is closed per decision 8; querying, entity CRUD and server administration are deliberately absent.

**Conventions verified by running the binary, not by reading the code**: `validate` on a good tree exits 0 with its summary on *stderr*; on a bad tree it exits 1 with structured JSON on *stdout* that `python -m json.tool` parses; in text mode stdout is empty (0 bytes) so diagnostics never pollute a pipe. `--token` reads from the environment and clap hides its value in `--help`, so it appears in neither shell history nor `ps`.

**Two details in the HTTP client that only matter when something goes wrong.** The scope read **pages to exhaustion** — stopping at one page would make the catalog look emptier than it is and plan a prune for everything past it, which no downstream guard can catch because the read itself lied. And `tombstone` resolves the FQN to an id immediately before deleting rather than trusting the id from the earlier plan read: one extra round trip, which is the right price for the one irreversible operation. Every module above is a pure function over `Declarations` and a `Vec<LiveEntity>`, which is why all of it is testable with no server — but it also means nothing yet *fetches* live state or *sends* a change. That is deliberate sequencing, not an oversight: decision 6 makes the CLI a thin client over the HTTP API, so the client is one well-defined piece bolted onto a core that is already proven, rather than the thing everything else is entangled with. The acceptance criterion for applying to a live catalog is marked partial above to say so.

**A bug the gate caught that no amount of type-checking would have.** The document loop pushed one error per malformed document — but `serde_norway`'s multi-document iterator does not advance past a document it could not parse, so a single malformed file produced errors forever. It surfaced as a test running 185 seconds until the runner killed it; in a user's hands it would have hung the CLI and exhausted memory on a stray typo. Parsing now stops at the first parse failure *within a file*, which is also the correct semantic — once a parse fails the parser's position is untrustworthy, so further "documents" read out of it are fiction. Accumulation across files is unaffected.

**Two corrections to the plan's own premises**, found while implementing:

- The placeholder `graph-owl-cli` manifest depended on `graph-owl-api`, which contradicts decision 6 — depending on the facade pulls the storage adapter into a binary that is supposed to run against a *remote* instance. Removed; `graph-owl-core` alone supplies the domain vocabulary (`AssetKind`) that validation needs.
- `serde_yaml` was deprecated and archived by its author in March 2024, so the obvious choice for the file format is not available. Adopted `serde_norway` (MIT OR Apache-2.0, maintained, a `serde_yaml` fork that keeps `Error::location()`), per `00l`'s rule that a parser for a standard we did not invent is adopted rather than written. The `location()` API is not incidental — it is what turns "missing field" into "missing field, this file, this line", which Slice A's criteria require.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: Declarations parse and validate

**Value**: Errors surface at authoring time, in CI, before touching a catalog.
**Path**: `graph-owl-cli` crate; YAML declaration schema per entity type; a `validate` command over a directory.
**Acceptance criteria**:
- One file may declare several entities; a directory is walked recursively.
- Each declaration carries `apiVersion`, `kind`, `metadata.name`, and a parent reference.
- Unknown `kind` → error naming the file and line.
- Missing required field → error naming the file, line, and field.
- Two declarations of the same FQN → error naming both files.
- A parent reference to an entity neither declared nor existing → error.
- `validate` exits non-zero on any error and reports **all** errors, not the first.
- Validation is purely local — no catalog connection required.
**RED**: Fixture directories: valid, unknown kind, missing field, duplicate FQN, dangling parent. Assert every error is reported in one run with file and line. Mutator watch: first-error-only reporting must fail the multi-error fixture.
**GREEN**: CLI crate, declaration types, recursive walk, error accumulation with source spans.
**REFACTOR**: assess whether declaration types should be derived from the domain types or defined separately. Separately — the wire/file format must be able to evolve independently of internal representation, and coupling them makes every rename a breaking file-format change.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: A plan is computed and shown

**Value**: The trust-building step. Nothing mutates until a human has seen what would.
**Path**: fetch live state for the declared scope, diff against declarations, render a plan.
**Acceptance criteria**:
- Plan classifies each entity as create / update / no-change / prune.
- Updates show per-field before → after.
- Plan output is stable and deterministic — same inputs, byte-identical output — so it is diffable in CI.
- `--dry-run` prints the plan and exits without mutating.
- Plan against an empty catalog is all creates.
- Plan with unchanged declarations is all no-change.
- Exit code signals whether changes are pending, so CI can gate on it.
**RED**: Determinism test running plan twice and asserting identical bytes. All-creates and all-no-change tests. Mutator watch: non-deterministic ordering (map iteration) must fail the determinism test — this is the likely real bug.
**GREEN**: state fetch, diff, sorted rendering, exit codes.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Apply converges

**Value**: The core loop — declarations become catalog state, idempotently.
**Path**: execute the plan via the HTTP API, parents before children, reusing Epic 3's upsert.
**Acceptance criteria**:
- Apply to an empty catalog creates the full declared hierarchy with correct FQNs.
- Second apply with unchanged files: zero new versions, zero change events.
- Changing one description updates exactly one entity.
- Parents are applied before children.
- A per-entity failure does not abort the run; it is reported and the exit code reflects it.
- Applying against a catalog changed by hand updates only fields the declarations actually declare — a hand-edited undeclared field survives.
- `--yes` skips confirmation; without it and without a TTY, apply refuses rather than assuming consent.
**RED**: Idempotency test asserting zero versions on second apply. The hand-edit survival test — an undeclared field edited in the UI must not be reset. Mutator watch: treating absent-from-declaration as null must fail it, which is decision 4's failure mode.
**GREEN**: ordered execution, merge semantics, confirmation handling.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Pruning is scoped and guarded

**Value**: Declarations can remove assets without a misconfigured repository emptying a catalog.
**Path**: `--prune` within a declared scope, reusing Epic 3's enumeration-scope and threshold machinery.
**Acceptance criteria**:
- Without `--prune`, an entity absent from the files is left alone.
- With `--prune`, it is tombstoned (soft delete, never hard).
- Pruning is scoped: a tree scoped to `service_a` never touches `service_b`.
- A prune exceeding the threshold (default 10% of scope) aborts and reports, mutating nothing.
- `--force-prune` overrides the threshold and requires explicit confirmation.
- A failed or partial run prunes nothing — completion gating, as in Epic 24.
- The plan shows prunes distinctly and prominently.
**RED**: Four tests mirroring Epic 15 Slice E — scoped, threshold, partial-run, and no-prune-by-default. The scope test is critical: declare only `service_a`, assert `service_b` is untouched. Mutator watch: scope ignored must fail it; missing threshold must fail the abort test.
**GREEN**: scoped reconciliation, threshold guard, completion gating.
**REFACTOR**: this is the third consumer of scoped reconciliation (Epics 24, 13, now 12). If the shared pure `reconcile(...)` function was not extracted earlier, extract it now.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Drift is visible

**Value**: Answers "has anyone changed this outside git" — the question that makes declarations trustworthy over time.
**Path**: `graph-owl drift` comparing live state to declarations, reporting without mutating.
**Acceptance criteria**:
- Reports entities whose live state diverges from declarations, field by field.
- Reports live entities in scope that are undeclared.
- Reports declared entities that are missing.
- Exits non-zero when drift exists, so it can run on a schedule and alert.
- Changes nothing, ever — verified by asserting zero versions after a drift run.
- Machine-readable output (`--format json`) alongside human output.
- Distinguishes drift from pending declaration changes not yet applied.
**RED**: Test asserting a drift run produces zero new versions — decision 3's guarantee. Test asserting the distinction between "someone edited live" and "the file changed but was not applied". Mutator watch: any mutation during drift must fail the first.
**GREEN**: comparison, classification, output formats.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: Export round-trips

**Value**: Adopt metadata-as-code on a catalog that already exists, rather than only on a greenfield one.
**Path**: `graph-owl export --scope <fqn> --out <dir>` emitting declarations.
**Acceptance criteria**:
- Export produces valid declarations that `validate` accepts.
- Applying an unmodified export is a **no-op** — the round-trip property.
- System-derived fields (version, timestamps, FQN, `updatedBy`) are omitted.
- History and change descriptions are omitted — deliberately lossy.
- Output layout is one file per entity or grouped by parent, selectable.
- Files are deterministic and diff-friendly: sorted keys, stable ordering.
- Export is scopeable by service, domain, or entity type.
**RED**: The round-trip test is the specification: export → apply → assert zero versions. Determinism test asserting two exports are byte-identical. Mutator watch: emitting a derived field must break the round-trip, since applying it would be rejected or would churn.
**GREEN**: exporter, field filtering, deterministic serialization.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice G: CI gates on the plan

**Value**: The pull-request workflow that makes this a review process rather than a script.
**Path**: `graph-owl plan --format github` emitting a PR comment; `--fail-on delete` gating.
**Acceptance criteria**:
- Plan renders as a PR-comment-friendly summary with counts and detail.
- `--fail-on delete` exits non-zero when the plan contains prunes.
- `--fail-on drift` exits non-zero when live state has drifted.
- A documented, copy-pasteable GitHub Actions workflow.
- The workflow runs plan on pull requests and apply on merge to the main branch.
- Credentials come from the environment; none appear in output or logs.
**RED**: Golden-file test of the rendered output. Exit-code tests per `--fail-on` mode. A redaction test asserting no token appears in plan output. Mutator watch: an exit code of 0 when deletions are present must fail.
**GREEN**: formatter, gating flags, documented workflow.
**Done when**: criteria met, mutation report reviewed, commit approved.

## Explicitly deferred (with destination)

- **A git-webhook server** → run apply from existing CI. graph-owl does not become a CD system, for the same reason it does not become a scheduler.
- **Partial-file ownership** (several repositories declaring one entity's different fields) → directory scoping covers the realistic case; revisit only if genuinely requested.
- **Declaration templating / variables** → let existing tooling (Helm, Kustomize, envsubst) generate the files; inventing a template language is a trap.
- **Applying policies and roles as code** → natural extension once Epic 13 exists; the declaration schema is designed to accommodate new kinds additively.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. End-to-end CLI tests against a real service and Postgres, not a stub.
5. **Prune tests (Slice D) reviewed with particular care** — this is where a bug empties a customer's catalog.
6. Round-trip property (Slice F) verified against a catalog populated by a connector run, not only by a prior apply.
