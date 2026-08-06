# Session summary — graph-owl

Standing context: this is a multi-epic push to finish Epic 42 (Semantic Browse,
Review Queues & Agent Activity) by first filling in the backend capabilities
several earlier epics were missing, then building Epic 42's own frontend
slices. Work commits directly to `main` per `CLAUDE.md`'s standing rule — no
branch, no PR, no approval needed per commit; push is still a separate ask.

## Gating cadence changed this session

Per direct instruction, the full workspace gate (`scripts/gate.sh --full`,
28+ crates) is **no longer run automatically per epic**. The new loop:

- **Per epic, always**: `CARGO_TARGET_DIR=/tmp/check cargo check -p <crate> --all-targets`
  (no workspace lock) + targeted `cargo nextest run -p <crate> <test>` for the
  crates actually touched.
- **Full `scripts/gate.sh --full`**: only when the user explicitly asks, or once
  several epics have accumulated.

`CLAUDE.md` and the auto-memory files were updated to record this.

## Done and committed this session

1. **Epic 20 — Drift as an HTTP-queryable review queue** (commit `40980db`,
   bundled with the gating-cadence doc update). Postgres-backed drift reports,
   apply/ignore decisions, HTTP surface, OpenAPI entries.

2. **Epic 33 — Domain ontology packs** (commit `1ba79f2`). SKOS/Turtle import
   via a dedicated `graph_owl_rdf_io::skos` module (the existing `Flake`/`Sid`
   RDF parser can't resolve arbitrary external vocabulary IRIs — a real,
   pre-existing limitation, not something built around blindly), licence
   gating, extend-without-fork overrides, upgrade diffing against the pack's
   own stored source Turtle, cross-pack-reference-aware removal guard. Full
   Postgres + HTTP integration tests, 0 missed mutants on the new pure logic.

3. **Epic 35 — Collaboration** (commit `0b1de8f`). The big one this session:
   threads/replies with a structurally-unspoofable author, resolve/reopen with
   authorization, change proposals (the load-bearing piece — stale-value 412,
   owner/admin-only accept/reject, attribution to the **proposer** on accept,
   not the accepter), announcements with an inclusive/exclusive validity
   window and container-to-descendant inheritance, reaction toggle semantics,
   and an activity feed merging Epic 3's version history with five
   collaboration tables in one query per entity.

   - **Real bug found and fixed, not just written around**: Epic 32 already
     owns `Proposal`/`ProposalStatus`/`/proposals` for an unrelated concept
     (an agent's pending action). The collision surfaced three times — a
     Rust compile error at the `Storage` trait, then again as an **axum
     runtime panic at router-build time** (not a compile error!) only
     discovered by actually starting the server, then a third time as a
     silent `OpenAPI` schema collision (utoipa registers by bare type name).
     Fixed by prefixing everything `change_proposal`/`change-proposals` and
     using `#[schema(as = ChangeProposal)]`.
   - 49 new tests (19 real-Postgres, 30 real-HTTP), 0 missed mutants on the
     pure decision logic, `openapi.json` regenerated and passing the
     committed-contract drift guard.
   - **Three scope cuts recorded honestly** in `plans/35-collaboration.md`
     rather than silently dropped: no `AccessPredicate` filtering on the
     activity feed (a real correctness gap, flagged as such); no user-scoped
     `/users/{id}/activity` ("follow" doesn't exist anywhere in this
     codebase); unresolved-thread-count / active-announcements / reaction-
     counts are each their own endpoint rather than riding along on an
     entity read via field selection (Epic 2's field-selection mechanism is a
     fixed column whitelist, not built for a computed cross-table value).

4. **Demo server confirmed running.** `./scripts/demo.sh` was re-run after the
   Epic 32/35 route-collision fix; the server now starts cleanly on `:8080`
   (Postgres via Docker, console built, `/health` returns 200). Still up as of
   this summary.

## Done and committed this session (continued)

4. **Epic 7c — `MappingReport` over HTTP/Bolt** (commit `76dea0db`).
   Threaded `MappingReport` through `cypher`/`cypher_stream`, surfaced the
   accumulated lossy mappings on `POST /cypher` as `lossyMappings`, and
   propagated the same report through Bolt's `RUN`/`PULL` as `notifications`
   metadata (decision 2: reported, never dropped silently).

## In progress — Epic 7d: Bolt status/sessions over HTTP

Implementation is complete and uncommitted. Adds:

- `graph_owl_bolt::BoltSession` and session registry inside `BoltServer`,
  cleaned up via `Drop`-based `SessionGuard` on every disconnect path.
- `graph_owl_server::bolt::{build_server, register}` plus a `static OnceLock`
  so `GET /admin/bolt/status` can see the live listener.
- `GET /admin/bolt/status` (admin-only, `404` for non-admins), returning
  `enabled`, `maxConnections`, `activeConnections`, and the session list.
- Bolt listener startup in `main.rs` when `--features bolt` and
  `BOLT_BIND_ADDR` are both set.
- Integration test `crates/graph-owl-server/tests/bolt_status.rs` covering
  live session visibility, non-admin 404, and disabled-when-not-registered.

**Blocker discovered while resuming**: the workspace disk was 100% full
(`target/` had grown to 510 GB). `cargo clean` freed 769 GB. After that,
`cargo check`, `cargo fmt`, and `cargo clippy` are clean on the touched
`graph-owl-bolt` and `graph-owl-server` crates; `graph-owl-bolt` unit tests
pass. The `bolt_status` integration tests cannot run because Docker Desktop
is not currently running in this environment (`/var/run/docker.sock` is
broken and the sandbox cannot launch Docker.app). Tests and commit are
pending Docker availability.

## Task list — what's left

| # | Task | Status |
|---|---|---|
| 44 | Epic 7c: `MappingReport` over HTTP | **committed** (`76dea0db`) |
| 45 | Epic 7d: Bolt status/sessions over HTTP | **implemented; tests blocked on Docker** |
| 46 | Epic 9/9a: export HTTP routes + authorization | pending |
| 47 | Epic 42 Slice A: vocabulary browser (glossary) | pending |
| 48 | Epic 42 Slice B: 3 more vocabularies, config only | pending |
| 49 | Epic 42 Slice C: review queue (merge adjudication) | pending |
| 50 | Epic 42 Slice D: 3 more queues, config only | pending |
| 51 | Epic 42 Slice E: property-graph view toggle + export dialog | pending |
| 52 | Epic 42 Slice G: text-first ontology editor | pending |
| 53 | Epic 42 Slice F: agent activity, Bolt sessions, route budget | pending |
| 24 | Epic 37c Slice E: publish metadata + dry-run | pending (older, unrelated) |

Epics 44–46 are backend gap-filling (same pattern as 20/33/35 this session);
47–53 are Epic 42's own frontend slices, which is the actual deliverable this
whole push has been building toward. Task 24 is older, unrelated leftover
work not part of this push.
