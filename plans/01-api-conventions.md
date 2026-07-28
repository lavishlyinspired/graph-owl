# Plan: API Conventions & Contract (Epic 1)
**Branch**: feat/api-conventions
**Status**: **Shipped** — slices A–J. The contract is generated from a single
route table, served at `/openapi.json`, committed, drift-guarded and validated
as OpenAPI 3.1. Slice K is partial: responses are checked against the promised
schemas, but no client is generated in another language. Demo 1
**Depends on**: nothing
**Unblocks**: everything
**Crates**: `graph-owl-core` (Principal, Page, RelationshipType) · `graph-owl-api` (CatalogError) · `graph-owl-server` (problem+json, OpenAPI, extractors)

## Goal

Establish the wire contract every later endpoint inherits, publish it as a machine-readable spec, and retrofit the six endpoints that already exist — while the API has no external consumers and the retrofit is measured in hours.

## Why first

Six endpoints exist today. Forty will exist by Epic 5 (`05-engine-constraints.md`). Changing the error body, pagination shape, and field-naming convention costs a day now and a fortnight later. Nothing else in the roadmap has that cost curve.

## Resolved decisions

1. **Error format**: RFC 9457 `application/problem+json`, stable `type` URIs under `https://graph-owl.dev/errors/`. Clients branch on `type`, never on prose.
2. **Pagination**: cursor-based, opaque base64 encoding `(sort_key, tiebreaker_id)`. Not offset — offset drifts under concurrent inserts.
3. **Wire naming**: `camelCase` via `#[serde(rename_all = "camelCase")]`.
4. **Error taxonomy**: one `CatalogError` enum in `graph-owl-api`, replacing today's split between raw `StorageError` and the bespoke `CreateRelationshipError`.
5. **Relationship types**: closed `RelationshipType` enum, with a static table of legal `(from_type, type, to_type)` triples. Illegal triples are `400`.
6. **Principal seam**: a `Principal` extractor lands now returning a hardcoded system principal. Handlers take it from day one so Epic 12 swaps an implementation instead of forty signatures.
7. **Unknown query parameters are rejected** `400`. A typo'd filter that silently returns everything is a data-leak-shaped bug.
8. **The OpenAPI spec is generated from the code, never hand-maintained.** A hand-written spec drifts from the implementation within weeks, and a spec that lies is worse than none. It is published as a CI artifact and diffed on every PR, so a contract change is visible in review.
9. **`RelationshipType` is append-only and persisted by name, not by ordinal.** The enum is closed (decision 5), which means it is also *stored*. Two rules follow and both are permanent: a variant is **never removed and never renamed** — deprecated at most; and the wire and storage representation is the **string name**, never the discriminant. Persisting a Rust enum by ordinal means reordering the variants silently rewrites the meaning of every stored row, and `#[derive]` makes reordering look like a formatting change. Retiring a type is an entity migration, not an enum edit.
10. **One generic entity resource, parameterized by type — not one handler set per entity.** Four entity types exist today and 25 are planned. CRUD, pagination, field selection, soft delete, version history, and the envelope are identical across all of them; only validation and the type-specific body differ. Writing them 25 times produces 25 places for a pagination bug to hide and guarantees the twenty-fifth entity behaves subtly unlike the first.

## Implementation reference

```rust
// graph-owl-api — the shared shape every entity type inherits
pub trait CatalogEntity: Serialize + DeserializeOwned + Send + Sync + 'static {
    const TYPE_NAME: &'static str;              // "table", "dashboard", …
    type Create: DeserializeOwned + Validate;
    type Update: DeserializeOwned + Validate;   // never carries id or FQN — see the PATCH rule
    fn envelope(&self) -> &EntityEnvelope;
    fn derive_fqn(&self, parent: Option<&str>) -> Result<String, ValidationError>;
}

// graph-owl-server — one router factory, mounted once per entity type
pub fn entity_routes<E: CatalogEntity>() -> Router<AppState>;
```

`entity_routes::<Table>()` and `entity_routes::<Dashboard>()` produce the same eight endpoints with the same pagination, the same problem+json shapes, and the same `Location` header. **A new entity type contributes a `CatalogEntity` impl and a validation function — not a handler module.**

This is the server-side twin of two decisions made elsewhere for the same reason: `39-ui-foundation.md`'s one composable entity page, and `15-connectors.md`'s schema-generated connector configuration. All three exist because the per-instance cost, multiplied by the planned instance count, is the dominant cost in that area.

*Boundary*: genuinely type-specific endpoints — `POST /tables/{id}/relationships`, `GET /tables/{id}/lineage` — are written normally. The generic factory covers the eight endpoints that are identical, not every endpoint an entity has.

## Acceptance criteria (feature level)

- [ ] Every error response is `application/problem+json` with a stable `type` URI.
- [ ] `GET /tables` returns `{data, paging}` and pages correctly across a boundary with concurrent inserts.
- [ ] All request and response bodies are `camelCase`.
- [ ] A relationship with an illegal `(from, type, to)` triple is rejected `400` before touching storage.
- [ ] An unknown query parameter is rejected `400`.
- [ ] Every handler receives a `Principal`.
- [ ] `POST /tables` returns a `Location` header.
- [ ] An OpenAPI 3.1 spec is generated in CI and diffed on every PR.
- [ ] A generated client round-trips against the running service.
- [ ] `RelationshipType` round-trips by **name**; a test pins every variant's wire string so reordering the enum cannot change it.
- [ ] Two entity types mounted through the generic factory expose byte-identical pagination, error, and header behaviour — asserted by running the same test suite against both.

## Slices

Every slice runs RED → GREEN → MUTATE (`cargo mutants`) → KILL MUTANTS → REFACTOR, and loads `tdd`, `testing`, `mutation-testing`, `refactoring` before any code. Each ends with the work presented and commit approval requested. Not repeated per slice below.

### Slice A: Errors arrive as problem+json

**Value**: A client can branch on a stable machine-readable error identity instead of pattern-matching English.
**Path**: `AppError` gains a `problem_type()` and `title()`; `IntoResponse` emits the RFC 9457 body with `Content-Type: application/problem+json`.
**Acceptance criteria**:
- Duplicate FQN → `409`, `type: ".../fqn-conflict"`, extension member `conflictingId`.
- Malformed JSON → `400`, `type: ".../malformed-body"`.
- Missing table → `404`, `type: ".../not-found"`.
- Response `Content-Type` is `application/problem+json` on every error path.
**RED**: HTTP integration tests asserting the body shape and content type for each of the four existing error paths. Mutator watch: `problem_type()` returning a constant for all variants must be caught — assert distinct `type` values per variant, not merely presence.
**GREEN**: `ProblemDetails` struct in `graph-owl-server`; extend `IntoResponse for AppError`.
**Done when**: all four error paths verified, mutation report reviewed, commit approved.

### Slice B: Validation failures report every problem at once

**Value**: A client fixing a bad request sees all four mistakes in one round trip, not one per retry.
**Path**: `AppJson<T>` collects field errors rather than short-circuiting; `CatalogError::Validation(Vec<FieldError>)`.
**Acceptance criteria**:
- A body missing `name` *and* carrying an empty `fullyQualifiedName` returns both entries in `errors[]`.
- Each entry has `field`, `code`, `detail`.
- `field` uses dotted/indexed paths for nested values (`owners[0].id`).
**RED**: Test posting a body with two independent violations, asserting both appear. Mutator watch: returning only the first error must fail — assert `errors.len() == 2`.
**GREEN**: field-error accumulation in the extractor.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice C: Lists are cursor-paginated

**Value**: A consumer can page a 10,000-table catalog without drift or degradation.
**Path**: `GET /tables?limit&after` → `Catalog::list_tables(page: PageRequest)` → keyset `WHERE (fqn, id) > ($1, $2) ORDER BY fqn, id LIMIT n+1`.
**Acceptance criteria**:
- Default `limit` 25, max 1000; `limit=1001` → `400`.
- Response is `{data, paging: {after, before, total}}`.
- `after: null` on the last page.
- Inserting a row mid-pagination neither skips nor duplicates an unrelated row.
- A cursor from a different `sort` value → `400`.
- Opaque cursor: a hand-crafted or truncated cursor → `400`, never a panic or a 500.
**RED**: Repository test paging 30 rows in pages of 10 with an insert between pages 1 and 2, asserting the page-2 contents are unaffected. HTTP test for the envelope shape and the malformed-cursor path. Mutator watch: `LIMIT n+1` vs `LIMIT n` determines `after`-nullness — assert the last page has `after: null` *and* the page before it does not.
**GREEN**: `PageRequest`/`Page<T>` in `graph-owl-core`; keyset SQL; base64 cursor codec.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice D: The wire speaks camelCase

**Value**: Aligns with prevailing JSON convention so clients need no field-name translation layer.
**Path**: `#[serde(rename_all = "camelCase")]` on every DTO and domain type crossing the wire.
**Acceptance criteria**: `fullyQualifiedName`, `createdAt`, `updatedAt`, `relationshipType`, `fromEntityId` on the wire; Rust stays `snake_case`.
**RED**: Update existing HTTP tests to assert camelCase keys — they fail against the current snake_case output.
**GREEN**: serde attributes.
**REFACTOR**: assess a shared `#[serde(rename_all)]`-carrying derive or a workspace lint to prevent regression on new types.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice E: One error taxonomy across the facade

**Value**: The facade expresses domain-meaningful failures; handlers map rather than decide.
**Path**: `CatalogError` in `graph-owl-api` (`NotFound`, `Conflict`, `Validation`, `IllegalRelationship`, `Storage`); `From<CatalogError> for AppError` in the server; delete `CreateRelationshipError`.
**Acceptance criteria**:
- Every `Catalog` method returns `Result<_, CatalogError>`.
- Storage-level `Conflict` becomes a domain-level conflict naming the field that collided.
- No behavior change visible at the HTTP layer — existing status codes hold.
**RED**: Facade tests asserting each `CatalogError` variant for its triggering condition; existing HTTP tests must stay green throughout.
**GREEN**: introduce the enum, migrate call sites.
**REFACTOR**: this is the slice where the `AppError` ↔ `CatalogError` mapping either collapses cleanly or reveals a missing variant — assess deliberately.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice F: Relationship types are a closed, validated vocabulary

**Value**: `Table contains Database` is rejected at the boundary instead of silently corrupting the graph.
**Path**: `RelationshipType` enum in `graph-owl-core`; static `LEGAL_TRIPLES` table; validation in `Catalog::create_relationship` before storage.
**Acceptance criteria**:
- The twelve types from `plans/00c-domain-model.md` deserialize; an unknown string → `400`.
- `Table upstream Table` accepted; `Table contains Database` → `400` with `type: ".../illegal-relationship"`.
- Validation precedes the existence checks, so an illegal triple between two nonexistent tables reports the triple problem, not `404`.
**RED**: Table-driven test over legal and illegal triples. Mutator watch: a validator returning `true` unconditionally must fail — the illegal-triple cases cover this. Also assert ordering: illegal triple + nonexistent tables → `400` not `404`.
**GREEN**: enum, lookup table, validation call.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice G: Every handler carries a Principal

**Value**: Authentication becomes an implementation swap in Epic 11 rather than a forty-handler refactor.
**Path**: `Principal { id, name, kind: User|Service|System }` in `graph-owl-core`; `FromRequestParts` extractor returning the hardcoded system principal; threaded into every `Catalog` method that mutates.
**Acceptance criteria**:
- Every mutating handler takes `principal: Principal`.
- Every mutating `Catalog` method takes `&Principal`.
- The extractor is the single place a principal is constructed.
- Behavior is otherwise unchanged.
**RED**: Facade tests asserting mutating methods accept and record a principal (recorded value observable via a test-visible field until Epic 3's `updated_by` exists).
**GREEN**: type, extractor, signature threading.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice H: Unknown query parameters are rejected

**Value**: A typo'd filter fails loudly instead of silently returning the unfiltered collection.
**Path**: Typed query structs with `#[serde(deny_unknown_fields)]` per list endpoint.
**Acceptance criteria**: `GET /tables?ownr=x` → `400` naming the unknown parameter; every documented parameter still accepted.
**RED**: Test asserting `400` and that the message names `ownr`. Mutator watch: assert the *specific* unknown key is named, not just that a 400 occurred.
**GREEN**: query structs.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice I: Creates return a Location header

**Value**: Standard REST affordance; clients follow the header instead of reassembling the URL.
**Path**: `POST` handlers return `(StatusCode::CREATED, [(LOCATION, url)], Json(entity))`.
**Acceptance criteria**: `POST /tables` → `Location: /tables/{id}` matching the body's `id`; same for relationships.
**RED**: HTTP tests asserting header presence and that it matches the body id. Mutator watch: a hardcoded or mismatched path must fail — assert equality against the returned id, not a regex.
**GREEN**: header construction.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice J: The contract is published and diffed

**Value**: The API becomes consumable without reading Rust, and a contract change becomes visible in review rather than discovered by a client.
**Path**: `utoipa` (or equivalent) deriving OpenAPI 3.1 from handler and type definitions; spec emitted as a CI artifact; a check diffing it against the committed copy.
**Acceptance criteria**:
- Spec covers every endpoint with request/response schemas, status codes, and the problem+json error shape.
- Spec is generated from the code — no hand-maintained YAML (decision 8).
- `openapi.json` is committed; a PR changing the API without regenerating it fails CI.
- The CI diff renders as a readable summary of contract changes.
- Spec validates against the OpenAPI 3.1 schema.
- Examples in the spec are real and exercised, not invented prose.
- A `/openapi.json` endpoint serves it at runtime.
**RED**: A test asserting the generated spec matches the committed one — that check *is* the drift guard. A schema-validation test. A test asserting every route in the router appears in the spec, so a new endpoint cannot be added invisibly. Mutator watch: a generator that silently omits a route must fail the route-coverage test.
**GREEN**: derive macros, generation, commit check, serving endpoint.
**Done when**: acceptance criteria met, mutation report reviewed, commit approved.

### Slice K: A generated client works

**Value**: Proves the spec is not merely well-formed but actually describes the service — the only test that catches a spec that is valid and wrong.
**Path**: generate a client from the spec in CI; exercise it against the running service.
**Acceptance criteria**:
- A client is generated from `openapi.json` in CI (language choice is incidental — TypeScript is the cheapest to run).
- An integration test performs create → read → list → patch → delete through the generated client against a real service.
- Error responses deserialize into typed problem+json.
- Pagination works through the client, including following a cursor.
- Generation and the test run on every PR, so a spec that no longer describes the service fails the build.
- The generated client is not committed — it is a CI artifact, regenerated each run.
**RED**: The round-trip test through the generated client. It fails whenever the spec and implementation diverge, which is precisely its purpose. Mutator watch: a spec with a wrong field type must fail deserialization in this test — verify by deliberately corrupting one field's type and confirming the test fails.
**GREEN**: CI generation, integration test.
**Done when**: acceptance criteria met, deliberate-corruption check verified, commit approved.

## Slice J, as built (28 July 2026)

**The route table is the single source.** `openapi::ROUTES` declares every
operation; the document is generated from it and the router is asserted against
it. A route in the spec that the router does not serve fails
`every_documented_route_is_served_by_the_router`; a verb the table does not
declare fails its negative. That is what makes "the spec cannot drift from the
router" a property rather than a promise.

**Not `#[utoipa::path]` on twenty-eight handlers.** Those macros restate the
method, the path and the status codes beside the function, which is a second
place for them to be wrong and a second place to forget. Schemas *are* derives —
`ToSchema` on the domain types — because there the derive removes duplication
instead of creating it: a field added to `Asset` reaches the contract without
anybody remembering.

**`servers` and `securitySchemes` were found by a validator, not by review.**
Both were genuine omissions rather than lint noise: without `servers` a
generator has no base URL, and without a security scheme the document says an
endpoint can `401` while never saying how to avoid one — a generated client
would have had no way to send a token at all. Each authenticated operation now
carries `security: [{bearerAuth: []}]` and each open one carries `security: []`,
which is **not** the same as omitting it: an empty array says *this endpoint
takes no credential*, where omission inherits a document default.

### Mutation report

`cargo mutants --file crates/graph-owl-server/src/openapi.rs`: **12 mutants, 7
caught, 5 unviable, 0 survived.**

The high unviable count is the shape of the module rather than a gap. Most of it
is `serde_json::json!` literals, and a mutation that replaces one with
`Default::default()` does not typecheck — so cargo-mutants discards it rather
than running it. What *is* mutable — the branches deciding whether an operation
documents a `401`, a `404`, a `400`, or a request body — is covered, and those
are the decisions worth testing: each one is a branch a generated client either
grows or does not.

### What "fails CI" means here, honestly

The slice says a PR changing the API without regenerating `openapi.json` fails
CI. **This repository has no CI pipeline** — no workflows at all — so that
sentence is aspirational in this slice and in every other one that uses it. What
exists is `the_committed_contract_matches_the_code`, a test that fails
`cargo test` and names the command to fix it. That is the gate this project
actually enforces (`CLAUDE.md`: a slice is committable when the workspace suite
is green), and it was verified by deliberately corrupting a field's type in the
committed file and confirming the failure.

Adding a pipeline is worth doing and is not this slice's work.

## Explicitly deferred (with destination)

- **Published client libraries** (versioned, on a package registry) → Epic 37, alongside crate publishing. Generated-and-tested in CI is sufficient until someone consumes them externally.
- **Idempotency keys** and **bulk endpoints** → Epic 15, where connectors create the real need. Building them now would be speculative.
- **`If-Match` / `412`** → Epic 3. There is no version to match against until versioning exists.
- **Rate limiting** → an ingress concern, not an application concern, unless per-principal quotas are ever required.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment via the `refactoring` skill.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. `plans/00d-api-conventions.md` markers updated: **(change)** → **(built)**.
