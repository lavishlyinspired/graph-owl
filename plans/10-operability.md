# Plan: Operability & Resource Budget (Epic 10) ★
**Branch**: feat/operability
**Status**: Not started
**Depends on**: Epic 1 (a server, an error model, and a contract to instrument)
**Unblocks**: safe operation of every epic from 14 onward (async, multi-service failure modes)
**Differentiator** — the resource budget makes operational simplicity measurable. See `plans/00a-product-position.md`.
**Crates**: `graph-owl-server` (Config, health/ready, tracing, metrics, shutdown) — no new crates; the resource budget is a CI job

## Goal

Make the service configurable, observable, and shutdown-safe before connectors and search introduce failure modes that `println!` cannot diagnose — and put a **budget** on its footprint so "one binary you can actually run" stays true instead of eroding release by release.

## Why here

Everything after this is harder to debug without it. Search and connectors add asynchronous, cross-service failures — a slow index write, a hung source connection, a partial run — where the only tractable diagnosis is a structured log line correlated by request id and a latency histogram showing *which* dependency degraded.

## Resolved decisions

1. **Twelve-factor config**: environment only, no config files in the image, no secrets in code.
2. **Config is read exactly once**, at the composition root, into a typed struct. Nothing downstream reads an environment variable — that keeps configuration testable and its failure mode at startup rather than at first use.
3. **Fail fast and loud** on invalid config. A service that starts with a bad `DATABASE_URL` and fails on the first request is strictly worse than one that refuses to start.
4. **Liveness and readiness are different endpoints.** `/health` answers "is the process alive" (no dependency checks — a dependency outage must not trigger a restart loop). `/ready` answers "can it serve traffic" and does check dependencies.
5. **JSON logs by default**, human-readable when a TTY is detected.
6. **OTLP trace export is wired but disabled** by default — no collector is assumed.
7. **The resource budget is asserted in CI, not documented as an aspiration.** Operational simplicity is a claimed differentiator; a claim nobody measures decays into marketing. Budgets are revised deliberately with the reason recorded — never silently raised to make a build pass.
8. **Postgres is the only required service.** Search, events, and object storage are optional adapters. A deployment that does not need full-text search does not run OpenSearch.
9. **This epic owns the observability *contract*, and every later epic conforms to it.** Metric names, label sets, log fields, and span names are defined once here. Plans from Epic 14 onward each add instrumentation, and without a contract they will each invent a naming scheme — which produces dashboards that cannot be written because no two subsystems agree on what a "request" is called.
10. **Readiness is three-valued, not two.** A service whose search index is unavailable can still serve the catalog; forcing that into "not ready" removes it from the load balancer and turns a degraded feature into an outage.

## The observability contract

Referenced by every epic that emits telemetry. Adding a metric that does not conform is a review-blocking finding.

**Metrics** — `graph_owl_<subsystem>_<noun>_<unit>`, always a base unit (`seconds`, `bytes`, `total`), never a prefixed one (`ms`, `mb`).

| Subsystem | Prefix | Owner |
|---|---|---|
| HTTP surface | `graph_owl_http_*` | Epic 10 |
| Triple store | `graph_owl_engine_*` | Epic 4 |
| Query | `graph_owl_query_*` | Epic 7 — sourced from the `Tracker` in `07-engine-query.md`, not counted twice |
| Reasoning | `graph_owl_reasoning_*` | Epic 6 |
| Search | `graph_owl_search_*` | Epic 8 |
| Ingestion | `graph_owl_ingest_*` | Epics 15–19 |
| Bolt | `graph_owl_bolt_*` | Epic 7d |

**Label discipline, which is the part that actually breaks systems**: labels are bounded sets — route templates, entity types, status classes, connector type names. **Never** an entity id, an FQN, a user id, a query string, or a connector *instance* name. One unbounded label ends a Prometheus server, and it is always added by someone who needed it for one investigation.

**Log fields**: `request_id`, `principal`, `route`, `status`, `duration_ms`, `error_type`. Errors carry the problem `type` URI from `00d-api-conventions.md` so a log line joins to an API error without a lookup table.

**Spans** are named `<subsystem>.<operation>` and always carry `request_id`. A span that crosses a port boundary (facade → storage, query → triple store) starts a child span, so a slow request is attributable to a layer rather than to the process.

### Admission control

Idempotency (`16-ingestion-apis.md`) answers *"have I already done this?"*. It does not answer *"should I accept this at all, right now?"* — and under a thundering-herd retry storm those are different questions with different answers.

A bounded semaphore fronts the expensive paths — ingestion, query, reasoning — and a request that cannot acquire a permit is **rejected immediately with `503` and `Retry-After`**, not queued.

**Rejecting fast is the whole point.** An unbounded queue converts an overload into a latency collapse: every client waits, every client times out, every client retries, and the queue grows. A fast `503` lets a well-behaved client back off and keeps the requests already in flight completing at normal speed. Permits available, permits held, and rejections are metrics per the contract above, so an operator can distinguish "overloaded" from "broken" — which look identical from the outside and demand opposite responses.

### Memory budget

`00a-product-position.md` states a footprint rather than a ratio, and the cache model has to match that or the claim is not defensible. **A percentage-of-RAM model would contradict the positioning**: "graph-owl runs in 2 GB" and "graph-owl takes 35% of whatever you give it" cannot both be true, and the second is the one that surprises an operator.

So the budget is **absolute and itemized**, derived from what this system actually caches:

| Cache | Default | Bound |
|---|---|---|
| Compiled shapes (Epic 5) | 32 MB | Count of shapes × compiled size; small and stable |
| Vector index (Epic 8) | 512 MB | **Not a cache** — a required resident structure, sized by corpus. The one line item that scales with data |
| Query plans (Epic 7) | 16 MB | LRU by plan count |
| Authorization decisions (Epic 13) | 32 MB | LRU, invalidated by epoch, never by TTL |
| Analytics results (Epic 38) | 64 MB | Last complete run only |
| Reasoning working set (Epic 6) | 512 MB | Per-run, `max_memory_bytes`; released on completion |

Every line is an explicit configuration value with a stated default, and the sum is reported at startup and exported as a metric. **An operator sizing a container adds the numbers rather than reverse-engineering a percentage.**

**The reasoning and vector lines are the only two that scale with data**, and both refuse rather than grow: reasoning returns `CappedReason::Memory`, and the vector index reports required-versus-configured at startup and refuses to load a corpus that will not fit. Refusing at startup with a number is strictly better than being OOM-killed under load.

**Container-aware as a guard, not as a sizing input.** The cgroup limit is read only to *check* the configured total against it and fail fast at startup if it exceeds. It never derives the budget — that would reintroduce the percentage model through the back door.

## Acceptance criteria (feature level)

- [ ] Service starts from environment alone and refuses to start on invalid config, naming the offending variable.
- [ ] `/health` returns `200` while Postgres is down; `/ready` returns `503`.
- [ ] Every request emits exactly one structured JSON log line with method, path, status, duration, and request id.
- [ ] A client-supplied `X-Request-Id` is propagated; one is generated when absent.
- [ ] `/metrics` exposes Prometheus request rate, latency histogram, status counts by route, and DB pool saturation.
- [ ] `SIGTERM` drains in-flight requests before exit.
- [ ] `docker compose up` yields a working service plus Postgres.
- [ ] The resource budget is asserted in CI and fails the build on regression.
- [ ] The service runs with Postgres as its only required dependency.
- [ ] An optional dependency failing yields `200 degraded`, never `503`.
- [ ] Every metric emitted anywhere in the workspace conforms to the observability contract — asserted by a CI check over the metric registry, not by review.
- [ ] No metric label can take an unbounded value — asserted by the same check.
- [ ] Every cache has an explicit configured bound; the total is reported at startup and exported as a metric.
- [ ] A configured total exceeding the cgroup limit **fails at startup** with both numbers, rather than being OOM-killed later.
- [ ] An over-capacity request is rejected `503` with `Retry-After` **immediately**, never queued — asserted by measuring rejection latency, not just the status code.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with the implementation skills loaded first, ending awaiting commit approval.

### Slice A: Configuration is typed and validated at startup

**Value**: An operator learns about a misconfiguration at deploy time with a message naming the variable, not from a 500 an hour later.
**Path**: `Config` struct in `graph-owl-server`; parsed and validated in `main`; passed by value to constructors.
**Acceptance criteria**:
- `DATABASE_URL` (required), `BIND_ADDR` (default `0.0.0.0:8080`), `LOG_LEVEL` (default `info`), `LOG_FORMAT` (`json`|`pretty`), `DB_POOL_MAX` (default 10), `SHUTDOWN_GRACE_SECONDS` (default 30).
- Missing required variable → exit non-zero naming it.
- Unparseable value → exit non-zero naming the variable and the expected form.
- Out-of-range value (pool size 0) → rejected.
- No code outside `main` reads an environment variable.
**RED**: Unit tests over `Config::from_env(map)` for each failure mode, asserting the error names the variable. Mutator watch: a default silently substituted for an invalid value must fail — assert rejection, not fallback.
**GREEN**: struct, parser, validation.
**REFACTOR**: assess whether `from_env` should take an abstract source rather than reading the process environment, so tests need no global mutation. Yes — take a map.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice B: Liveness and readiness are distinguishable

**Value**: An orchestrator restarts a wedged process without restart-looping the whole fleet during a database blip.
**Path**: `/health` returns `200` unconditionally. `/ready` runs `SELECT 1` with a short timeout.
**Acceptance criteria**:
- `/health` → `200` even when Postgres is unreachable.
- `/ready` → `200` with `{"status":"ready","checks":{"database":"ok"}}` when healthy.
- `/ready` → `503` naming the failed check when Postgres is down.
- **`/ready` → `200` with `{"status":"degraded"}`** when an *optional* dependency (search, event sink) is down but Postgres is healthy — the response names which check degraded.
- Each check declares whether it is required or optional; only required failures produce `503`.
- The readiness check has a bounded timeout and cannot hang the endpoint.
**RED**: Integration test that stops the Postgres container and asserts `/health` stays `200` while `/ready` becomes `503`. Second RED: stop the *search* adapter and assert `/ready` is `200 degraded`, not `503` — treating an optional dependency as required removes a healthy instance from the load balancer and converts a degraded feature into an outage, which is the more expensive failure. Mutator watch: a `/health` that checks dependencies must fail the first test; a check that ignores the required/optional flag must fail the second.
**GREEN**: split endpoints, bounded check.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice C: Requests are traceable

**Value**: An operator can follow one request across every log line it produced.
**Path**: `tracing` + `tracing-subscriber` JSON layer; a middleware that extracts or generates `X-Request-Id` and opens a span.
**Acceptance criteria**:
- One log line per request with method, path, status, duration_ms, request_id.
- Client `X-Request-Id` is propagated; absent → UUID generated.
- The id is echoed in the response header.
- Errors log at `error` with the problem `type`; successes at `info`.
- Log level respects `LOG_LEVEL`; format respects `LOG_FORMAT`.
- No secret ever appears in a log line — `DATABASE_URL` is redacted where logged.
**RED**: Test capturing log output and asserting the fields and the propagated id. A redaction test asserting a password-bearing URL never appears verbatim. Mutator watch: a generated-not-propagated id must fail — assert the *supplied* id appears.
**GREEN**: subscriber, middleware, redaction.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice D: Behavior is measurable

**Value**: An operator sees which route degraded and whether the pool is saturated, without reading logs.
**Path**: `metrics` crate + Prometheus exporter on `/metrics`.
**Acceptance criteria**:
- `http_requests_total{method,route,status}` counter.
- `http_request_duration_seconds{method,route}` histogram.
- `db_pool_connections{state}` gauge.
- `catalog_entities_total{entity_type}` gauge.
- Route labels use the **template** (`/tables/{id}`), never the concrete path — per-id labels are a cardinality explosion.
- `/metrics` is excluded from its own metrics.
**RED**: Test making three requests to `/tables/{different-ids}` and asserting exactly one series with the templated route label. Mutator watch: concrete-path labelling must fail this test.
**GREEN**: exporter, middleware, gauges.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice E: Shutdown drains

**Value**: A deploy does not sever in-flight requests.
**Path**: `axum::serve(...).with_graceful_shutdown(signal)`; SIGTERM/SIGINT; bounded by `SHUTDOWN_GRACE_SECONDS`; `/ready` flips to `503` immediately on signal so the load balancer stops routing.
**Acceptance criteria**:
- In-flight request completes after SIGTERM.
- New connections refused after signal.
- `/ready` → `503` immediately on signal, before draining completes.
- Exit within the grace period even if a request hangs.
- Pool closed cleanly.
**RED**: Test issuing a slow request, sending SIGTERM mid-flight, asserting the response completes with `200`. Mutator watch: immediate abort must fail; a `/ready` that stays `200` during drain must fail.
**GREEN**: signal handling, drain, readiness flag.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice F: The service ships as a container

**Value**: A developer or CI runner gets a working stack in one command.
**Path**: multi-stage `Dockerfile` (cargo-chef dependency cache → distroless runtime); `docker-compose.yml` with Postgres.
**Acceptance criteria**:
- Image builds reproducibly; dependency layer cached across source-only changes.
- Runs as a non-root user.
- `docker compose up` yields a service answering `/ready` with `200`.
- Image contains no build toolchain.
- Health check configured in compose.
**RED**: A smoke script (CI-runnable) that builds, composes up, polls `/ready`, creates a table, reads it back, and tears down.
**GREEN**: Dockerfile, compose file, CI job.
**Done when**: acceptance criteria met, smoke script green, commit approved.

### Slice G: The footprint is budgeted and defended ★

**Value**: Turns "lightweight" from a claim into a property the build enforces. This is the slice that makes operational simplicity a differentiator rather than a coincidence.
**Path**: CI job measuring startup, memory, binary size and dependency count against stated budgets.
**Acceptance criteria**:

| Metric | Budget | Measured by |
|---|---|---|
| Cold start to first served request | < 1 s | Time from exec to a `200` from `/ready` |
| Idle RSS after startup | < 100 MB | Measured 30 s after start, no traffic |
| Stripped release binary | < 50 MB | `ls` on the built artifact |
| Required external services | Postgres only | Compose stack with search and events absent still serves CRUD |
| Direct dependencies | Recorded; an increase requires review | `cargo tree --depth 1` count committed and diffed |

- Each budget is asserted in CI; exceeding one fails the build with the measured and budgeted values.
- Measurements run on a consistent runner so numbers are comparable across builds.
- Trends are recorded so gradual drift is visible, not just threshold breaches.
- A dependency-count increase surfaces in the PR diff for review rather than failing outright — new dependencies are sometimes right, but always worth seeing.
- **The optional-services test is the important one**: the compose stack must serve CRUD with OpenSearch absent, proving search is genuinely an optional adapter rather than a soft requirement.
**RED**: A CI job asserting each budget, verified by deliberately breaching one (add a large dependency, confirm the build fails). The optional-services test brings up a stack with only Postgres and exercises CRUD end to end. Mutator watch: the budget check must actually fail when breached — verify rather than assume; a check that always passes is the failure mode here.
**GREEN**: measurement harness, CI job, committed dependency manifest, minimal compose stack.
**REFACTOR**: if a budget cannot be met, the honest options are to fix the cause or revise the budget with a recorded reason — not to loosen it quietly. Record either outcome in `plans/00a-product-position.md`.
**Done when**: criteria met, deliberate-breach verified to fail CI, commit approved.

## Explicitly deferred (with destination)

- **Dashboards and alert rules** → deployment-repo concerns, not application concerns.
- **OTLP export enabled** → wired here, switched on when a collector exists.
- **Per-principal rate limiting** → an ingress concern unless quotas become a product requirement.
- **Audit log as a separate store** → Epic 3 change events already carry principal and diff; a distinct audit sink only if compliance demands separation.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Container smoke script green in CI.
