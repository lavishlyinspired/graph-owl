# Plan: Reference Applications (Epic 36)

**Branch**: feat/reference-apps
**Status**: **All five slices shipped, 8 August 2026.** `examples/` holds `agent-triage/` (new, Python, MCP-only), `adapter-csv/` (a pointer to the artifact Epic 16 Slice F already built at `sdk/python/examples/csv_adapter.py` — not duplicated, since Slice C's own AC says "the adapter is the artifact Epic 16's guide links to, so the guide and the code cannot drift"), and `browse/` (new, Python, generated read client). All three verified against a real live service via `scripts/verify-examples.sh`, mirroring `verify-sdks.sh`/`verify-langchain.sh`'s own shape. See each slice's own account below for what building them actually found, and Slice E for the full triaged defect log.
**Depends on**: Epic 14 (MCP), Epic 16 (SDKs), Epic 29 (graph API)
**Crates**: **One graph-owl crate change, exactly the kind decision 2 anticipates.** Examples themselves live in `examples/` and depend only on published crates and generated SDKs — enforced by Slice A. Slice D found the OpenAPI contract generator (`crates/graph-owl-server/src/openapi.rs`) had no mechanism for documenting query parameters at all — a genuine API defect blocking a generated client from using an already-implemented capability, not a new capability request — and fixed it there, per decision 2 ("friction is a defect in graph-owl, not the app... the fix goes in the API"). See Slice D and Slice E's findings table for the full account and what was deliberately left unfixed.

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

### Slice A: Surface-purity enforcement — **shipped, 8 August 2026**

**Value**: The constraint that makes the whole epic meaningful.
**Acceptance criteria**: a CI check asserts each example depends only on published crates and the generated SDK; an example importing an internal path fails the build with a message naming the import; the check is verified by a deliberately-broken branch; examples build against the *published* crate versions, not workspace paths, so a missing `pub` is caught.
**RED**: The deliberate-violation branch must fail CI. A check that never fails is not a check. Mutator watch: an unconditional pass must fail this verification.
**Done when**: criteria met, deliberate violation fails CI, commit approved.
**Shipped as** — `scripts/check-examples-purity.py`, run from `scripts/verify-examples.sh`.

- **The plan's own wording ("published crates", "pub(crate)") is Rust-flavoured, written before `00j-language-boundaries.md` settled reference applications as Python** ("examples should look like what a user would actually write"). Translated: an example may import only the standard library and the public (`__all__`-declared) exports of `graph_owl_sdk` — never a private submodule, never `sys.path` manipulation reaching into the monorepo's own source. Checked with `ast`, not a regex, because a regex cannot tell `from graph_owl_sdk import X` (public) from `from graph_owl_sdk.ingest import X` (the identical symbol, reached through a private path) without parsing the statement's real structure.
- **Verified both directions, not just written**: a deliberately-broken scratch file (a private-submodule import, plus `sys.path.insert`) was run against the check and failed with both violations named; a clean file passed. A real bug was found doing this, not designed around in advance: the first version of the `sys.path.insert(...)` detector compared the wrong slice of the attribute chain (`("sys", "insert")` instead of `("sys.path", "insert")`) and silently missed the call — caught by actually running the deliberate-violation case, not by inspection.
- **Test harnesses (`test_*.py`, `conftest.py`) are exempt** — proving an example works legitimately needs things the app itself must never do (mint a test JWT, call an admin endpoint to seed a scenario), and the check would otherwise flag its own test scaffolding.

### Slice B: Agent workflow — **shipped, 8 August 2026**

**Acceptance criteria**: as above; the tool-call-count assertion is enforced, not advisory; the filtered-view case asserts the agent says its view is partial; runs against a seeded graph in CI.
**RED**: The call-count assertion is design feedback: if answering "who owns this" takes five calls, Epic 14's decision 5 was not honoured and the tool surface needs changing. The filtered-view test catches an agent confidently asserting absence.
**REFACTOR**: any question needing more than three calls is an Epic 14 defect. Record it and fix the tool surface.
**Done when**: criteria met, commit approved.
**Shipped as** — `examples/agent-triage/` (`mcp_client.py`, `triage.py`, `conftest.py`, `test_triage.py`), verified against a real live service via `scripts/verify-examples.sh`.

- **`triage.py` always makes exactly three MCP calls** — `search_assets` (resolve a name to a real asset), `get_asset_context` (the trust signal that answers the question), `recall_memory` (institutional notes) — a fixed, low, testable proxy for whether Epic 14's tools are task-shaped, not endpoint-shaped. All 6 real-service tests pass at this budget: a healthy certified asset (SAFE), a deprecated asset (NOT SAFE, successor named), an uncertified asset (NOT SAFE, gap named), a policy-filtered view (PARTIAL VIEW, not silently upgraded to either SAFE or NOT SAFE), the same asset for an unrestricted principal (no partial flag — the negative control), and a nonexistent asset (NOT FOUND, not asserted absent).
- **A real, load-bearing discovery building this, not a Slice B bug**: this catalog's authorization **denies by default** (`plans/12-13-security.md`'s own words: "a completely successful sign-in renders as an empty estate"). A non-admin principal with no role and no policy sees nothing at all through `search_assets` — confirmed directly against a real server before writing the fix, not assumed. The reference app's own default querying principal (`alice`) needed an explicit baseline "can see everything" policy before any of the safe/deprecated/uncertified scenarios could mean anything — the same baseline viewer policy a real deployment needs before anyone but an admin can use the catalog. Recorded in `conftest.py`'s own comment rather than silently worked around.
- **A second real discovery, checked rather than assumed**: `GRAPH_OWL_ADMIN_SUBJECTS` bootstrap-admin only applies on the OIDC verification path (`graph-owl-server::verify_jwks`) — the shared-secret HS256 path (`authenticate_bearer_token`'s other branch) never calls `is_bootstrap_admin` at all. Confirmed **deliberate, not an oversight**, by reading `plans/12-13-security.md`: `GRAPH_OWL_ADMIN_SUBJECTS` is documented paired with OIDC specifically ("unset GRAPH_OWL_JWT_SECRET"), and shared-secret mode is its own doc comment's "legacy, and a demo affordance". The test harness works within that boundary rather than widening it: `conftest.py`'s `bootstrap_admin()` grants the first admin via direct SQL, the exact mechanism the same plan names as sanctioned ("Granting the first role required direct SQL").
- **A methodology fix, found the same way Slice D of Epic 37a found one**: `search_assets` full-text-searches names, and a dotted FQN does not tokenize the way a bare name does — the first version of every test searched by full FQN and got zero hits. Fixed by searching leaf names, safe within this suite because the container is recreated fresh per run.

### Slice C: Ingestion adapter — **shipped, 8 August 2026**

**Acceptance criteria**: as above; convergence proven by running twice and asserting zero new versions on the second; per-item error reporting asserted; the adapter is the artifact Epic 16's guide links to, so the guide and the code cannot drift.
**RED**: Convergence test. A per-item test with one bad row asserting the rest land. Mutator watch: an adapter that aborts on first error must fail the per-item test.
**Done when**: criteria met, commit approved.
**Shipped as** — `sdk/python/tests/test_example_adapter_live.py`, exercising the *existing* `sdk/python/examples/csv_adapter.py` (Epic 16 Slice F) against a real live service. **Not duplicated into `examples/adapter-csv/`** — the acceptance criterion itself says the adapter is the artifact Epic 16's guide links to, and moving it would break that guide's own link for no benefit; `examples/adapter-csv/` documents the cross-reference instead of holding a second copy.

- **Genuine convergence, verified against a real server**: pushing the identical CSV twice leaves the asset's version unchanged the second time — not merely "accepted", which the ingest response reports identically for both a real create and a real no-op.
- **Two rejection scenarios were tried and found not to produce what the acceptance criterion actually needs, checked directly rather than assumed** — both a duplicate FQN within one batch and an unrecognised `kind` string turned out to be **whole-request `400`s** (every item refused together, no `results` array), not the per-item `207` shape `csv_adapter.py`'s own `report()` function is written to read. The genuine per-item case is a row whose `parentFqn` resolves to nothing already in the catalog and nothing else in the same batch — that failure is local to one item, confirmed directly against a real server, and the rest of the batch lands (`accepted: 1, rejected: 1`).
- **A third test (threading a real per-item rejection through the CSV path specifically) was attempted and dropped**: `csv_adapter.py`'s own hierarchy derivation always builds a self-consistent parent chain from one row, so it structurally cannot reach the server's per-item rejection path on its own. The property is still covered end to end — the real per-item shape is proven above, and `report()`'s own reading of that exact shape is already unit-tested (`test_example_adapter.py`) — just not forced through a single test that would have been testing something other than what it claimed to.

### Slice D: Browse surface — **shipped, 8 August 2026**

**Acceptance criteria**: as above; the one-request assertion is enforced by a request counter — rendering an asset with owners, tags, and lineage must not fan out; empty, error, and filtered states render distinguishably; no hand-rolled HTTP.
**RED**: The request-counter test catching N+1. If field selection cannot deliver an asset page in one request, that is an Epic 1 defect. Mutator watch: per-related-entity fetching must fail the counter test.
**REFACTOR**: an N+1 here is an API defect. Fix field selection, not the app.
**Done when**: criteria met, commit approved.
**Shipped as** — `examples/browse/app.py` (`http.server.BaseHTTPRequestHandler`, stdlib only for the web layer) and `examples/browse/test_browse.py` (9 tests, verified against a real live service). No hand-written Python API client existed for the read surface — `graph_owl_sdk` covers push/ingest only (`00j-language-boundaries.md`'s own Python mandate, same reasoning `sdk/typescript` already applies) — so this slice generated one, the largest single piece of work in Epic 36.

- **A structural defect, not a browse-app bug: the OpenAPI contract had no mechanism for documenting query parameters on *any* endpoint.** `openapi.rs`'s spec-building loop set `operation["parameters"]` exactly once, inside `if route.path.contains("{id}")` — the `{id}` *path* parameter, never a query parameter, on any of the 180+ routes in `ROUTES`. `GET /assets/{id}?fields=...`, documented and implemented since Epic 37a Slice B, had no way for a generated client to know `fields` or `asOf` existed. Found by generating a Python client from the committed contract and reading its own output: `get_assets_id.sync()`'s signature had no `fields`/`as_of` parameters at all.
  **Fixed narrowly**: an additive `QueryParam`/`query_param()`/`QUERY_PARAMS` lookup table keyed by `(method, path)`, wired into the existing parameters-building loop without touching the `Route` struct or any of its 180+ existing call sites. Scoped to exactly the three endpoints this app needs — `GET /assets` (kind, owner, unowned, domain, dataProduct, limit, after), `GET /assets/{id}` (fields, asOf), `GET /assets/search` (q *required*, kind, domain, dataProduct, limit, after) — proven by three new tests in `crates/graph-owl-server/tests/openapi.rs`. **The other ~177 routes' query parameters are still undocumented** — see Slice E, "deferred, not fixed."
- **`openapi-python-client` (MIT) adopted for the read client** — recorded in `plans/00l-build-vs-adopt.md` with the regeneration command. Packaged as its own distribution (`sdk/python/graph_owl_read_client/`, `src/` layout, its own `pyproject.toml` declaring `attrs`/`httpx`/`python-dateutil`/`typing-extensions`) rather than folded into `graph_owl_sdk`'s zero-dependency package — folding them would give every `graph_owl_sdk`-only consumer three dependencies they never asked for, breaking that package's own "no runtime dependencies, on purpose" stdlib-only design. **Not committed** — `sdk/python/graph_owl_read_client/src/` is gitignored and regenerated by `scripts/verify-examples.sh` from `openapi.json`, the identical convention `sdk/typescript/src/generated/` already established for the TypeScript client (`verify-generated-client.sh`'s own reasoning: "a committed copy is a second thing to keep in step with").
- **`sync()`'s real return contract, found running it rather than assumed from the generated type signature**: `get_assets_id.sync()` returns `Asset | Problem | None` — a documented `404` decodes to a typed `Problem`, **not** `None`. `None` is what the generated client returns for a status the route does not document as success, because `raise_on_unexpected_status` defaults to `False` (checked directly in `client.py`, not assumed) — a `500` or any other undocumented status is swallowed to `None` rather than raising `UnexpectedStatus`. The first version of `render_asset_page` assumed `None` meant "not found" and crashed on `Problem.fully_qualified_name` the first time it hit a real 404. Fixed by checking `isinstance(asset, Problem)` for not-found and `asset is None` for "something went wrong" separately — two different failure shapes the generated client's own default configuration actually produces, not one.
- **`?fields=tags,lineage,columns`'s composed keys are not part of the `Asset` schema** — the handler (`crates/graph-owl-server/src/lib.rs`, the `AsOfQuery` fields loop) merges them onto the JSON response *after* serializing `Asset`, so the OpenAPI contract's `Asset` schema never describes them. A strictly-typed generated client therefore has no typed attribute for `tags`/`lineage`/`columns` — they only reach the caller through `attrs`' own `additional_properties` catch-all dict, untyped. `render_asset_detail` reads them from there rather than as typed fields. Not fixed — see Slice E.
- **`Client(timeout=...)` must be a top-level constructor kwarg, not nested inside `httpx_args`** — `get_httpx_client()` passes `timeout=self._timeout` and separately unpacks `**self._httpx_args`, so a `timeout` key placed in `httpx_args` collides with it (`TypeError: got multiple values for keyword argument 'timeout'`). Found writing the unreachable-backend test.
- **The request-counter test needed the search API queried directly, not the rendered HTML page parsed for a link** — the first version picked the first `<a href="/assets/...">` out of `render_search_page`'s own output, and Postgres full-text search stems "orders" to a token that also matches a seeded `order_id` column, so the "first result" was sometimes the wrong asset. Fixed by calling `get_assets_search.sync()` directly in the test and filtering to the exact FQN.
- **A real HTTP round trip was added alongside the direct function-call tests** — every other test in the file calls `render_search_page`/`render_asset_page` directly (the same pattern `agent-triage`'s suite uses for `triage()`), which proves the business logic but leaves `do_GET`'s own routing and status/header wiring untested. Two tests start a real `HTTPServer` on a system-assigned port and issue a genuine `urllib.request` GET against it, catching the class of bug direct calls cannot: a route dispatching to the wrong handler, a status code that never reaches the wire, a missing content-type header.

### Slice E: Defect log — **shipped, 8 August 2026**

**Value**: The epic's actual output.
**Acceptance criteria**: every friction point found while building the three apps is recorded as a finding with the epic it belongs to and a proposed fix; findings are triaged, not silently absorbed; findings that were fixed name the commit; findings deferred name why; the log lives in this plan so the exercise's value survives it.
**RED**: n/a — this slice is documentation, and its content is whatever the previous four slices surfaced.
**Done when**: log complete, findings triaged, commit approved.

#### Findings

| # | Found in | Finding | Status |
|---|---|---|---|
| 1 | Slice B | Authorization denies by default — a principal with no role and no policy sees nothing via `search_assets`. Not a bug: `plans/12-13-security.md`'s own documented behaviour ("a completely successful sign-in renders as an empty estate"). | **Not a defect — a characteristic worth naming** for anyone building against this catalog fresh: a reference app (or a first deployment) needs an explicit baseline viewer policy before it can see anything at all. |
| 2 | Slice B | `GRAPH_OWL_ADMIN_SUBJECTS` bootstrap-admin only applies on the OIDC verification path; the shared-secret HS256 path never calls it. | **Confirmed deliberate**, not fixed — `plans/12-13-security.md` pairs `GRAPH_OWL_ADMIN_SUBJECTS` with OIDC specifically, and shared-secret mode is documented as "legacy, and a demo affordance." Worked around in `conftest.py` via the same direct-SQL admin bootstrap the security plan itself names as sanctioned. |
| 3 | Slice C | A duplicate FQN within one batch, and an unrecognised `kind` string, both produce a whole-request `400` (no `results` array) rather than the per-item `207` the ingest guide's own examples suggest. | **A design characteristic, not a defect** — every batch failure mode was checked directly against a real server rather than assumed; the genuine per-item-rejection shape (an unresolvable `parentFqn`) exists and is exercised. Worth naming for anyone writing a new adapter expecting per-item granularity on every failure class. |
| 4 | Slice D | The OpenAPI contract had no mechanism for documenting query parameters on any of its 180+ routes — only the `{id}` path parameter was ever emitted. | **Fixed**, scoped to the three endpoints Slice D needs (`GET /assets`, `GET /assets/{id}`, `GET /assets/search`) — `crates/graph-owl-server/src/openapi.rs`'s additive `QUERY_PARAMS` table, proven by 3 new tests in `tests/openapi.rs`. **The remaining ~177 routes' query parameters are deliberately not backfilled here** — a blanket fix touching every route is a separate, much larger undertaking (auditing every handler's actual accepted query parameters against what each route's own extractor reads) that this slice's own scope does not call for. Whoever picks it up next should extend `QUERY_PARAMS` route by route, the same additive shape, rather than restructure `Route`/`route()`. |
| 5 | Slice D | `?fields=tags,lineage,columns` composes keys onto the JSON response that the `Asset` OpenAPI schema does not describe, so a generated, strictly-typed client can only reach them via `additional_properties`, untyped. | **Deferred** — the composition happens in the handler after `Asset` is serialized (`crates/graph-owl-server/src/lib.rs`), and describing it properly in the contract means either a second, explicit response schema for the field-selected shape or restructuring `fields=` into typed sub-resources. Both are a real API-design decision, not a mechanical fix, and out of scope for a reference-app slice. Worth an epic of its own if a typed consumer of `tags`/`lineage`/`columns` shows up. |
| 6 | Slice D | `openapi-python-client` emits warnings and silently skips generating six endpoints — `GET /ontology-packs/{id}/overrides/{override_id}`, `DELETE /policies/{name}`, `GET /webhooks/mappings/{name}`, `POST /webhooks/mappings/{name}/dry-run`, `GET /webhooks/mappings/{name}/versions`, `POST /webhooks/receive/{path}` — with "Incorrect path templating," a mismatch between the path template and its declared path parameters. | **Deferred** — none of Epic 36's three apps touch these endpoints, so it did not block this slice, but any future Python client work touching ontology-pack overrides, policy deletion, or webhook mapping management will hit the same silent gap. Worth a standalone fix to `openapi.rs`'s path-parameter naming for those specific routes. |
| 7 | Slice D | The generated client's `Client(timeout=...)` must be its own constructor kwarg — passing `timeout` inside `httpx_args` collides with the client's own `timeout=self._timeout` and raises `TypeError`. | Not a graph-owl defect — this is `openapi-python-client`'s own generated `Client.get_httpx_client()` behaviour, unrelated to this project's contract. Recorded here only because it cost real debugging time; no action needed beyond the comment left in `test_browse.py`. |

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
