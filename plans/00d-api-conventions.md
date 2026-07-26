# graph-owl — API Conventions
**Crate scope**: `graph-owl-server` (wire contract) · `graph-owl-api` (error taxonomy).

Rules every endpoint follows. A new entity type should require zero API design decisions: apply this document and the shape falls out.

Conventions marked **(built)** are live today. Conventions marked **(change)** differ from what is currently implemented and are landed by Epic 3 — cheap to do now because the API has no external consumers.

## URL shape

```
/{collection}                      collection of entities
/{collection}/{id}                 one entity, by UUID
/{collection}/name/{fqn}           one entity, by fully-qualified name
/{collection}/{id}/{sub}           sub-collection owned by the entity
/{collection}/{id}/versions        version list
/{collection}/{id}/versions/{v}    one historical version
```

- Collections are plural, lower-kebab-case: `/tables`, `/database-schemas`, `/glossary-terms`.
- `{id}` is always a UUID. A non-UUID in that position is a `400`, not a `404`.
- Lookup by name is a **separate path segment** (`/tables/name/{fqn}`), never an overloaded `{id}`. Overloading forces the server to guess whether `foo` is a malformed UUID or a name, and guesses wrong on FQNs that happen to look like UUIDs.
- Relationships are addressable independently (`/relationships/{id}`) because an edge has its own lifecycle. **(built)**

## Methods and status codes

| Method | Success | Notes |
|---|---|---|
| `POST /{collection}` | `201` + body + `Location` header | |
| `GET /{collection}` | `200` + paginated envelope | |
| `GET /{collection}/{id}` | `200` + body | `200` with `deleted: true` for soft-deleted, never `404` |
| `PATCH /{collection}/{id}` | `200` + updated body | Partial. No-op change returns `200` and does not bump the version |
| `PUT /{collection}/{id}` | `200` (updated) or `201` (created) | Upsert by FQN. The connector write path |
| `DELETE /{collection}/{id}` | `200` + tombstoned body | Soft delete. `?hardDelete=true` → `204` |
| `PUT /{collection}/{id}/restore` | `200` + restored body | |

`PUT` as FQN-keyed upsert is what makes connectors idempotent: a connector re-run should converge, not accumulate duplicates or `409`s. It is not a general-purpose full replace.

### Error status codes

| Code | Meaning |
|---|---|
| `400` | Malformed body, bad UUID, invalid field value, illegal relationship triple |
| `401` | Missing or invalid credentials (Epic 10) |
| `403` | Authenticated but policy denies (Epic 11) |
| `404` | Entity does not exist (distinct from soft-deleted, which is `200`) |
| `409` | Uniqueness conflict — duplicate FQN, duplicate relationship edge |
| `412` | `If-Match` version precondition failed |
| `422` | Semantically invalid but well-formed — e.g. a cycle in a containment hierarchy |

**(built)** — the project already returns `400` rather than axum's default `422` for a malformed body, via the custom `AppJson<T>` extractor. That stays.

## Error body **(change)**

RFC 9457 `application/problem+json`:

```json
{
  "type": "https://graph-owl.dev/errors/fqn-conflict",
  "title": "Fully qualified name already exists",
  "status": 409,
  "detail": "A table with fullyQualifiedName 'warehouse.public.customers' already exists.",
  "instance": "/tables",
  "conflictingId": "b2c3d4e5-..."
}
```

`type` is a stable, machine-matchable URI — clients branch on it, never on `detail` prose. Extension members (`conflictingId`) carry the machine-actionable specifics.

Validation failures list every problem at once rather than failing on the first:

```json
{
  "type": "https://graph-owl.dev/errors/validation",
  "title": "Request validation failed",
  "status": 400,
  "errors": [
    { "field": "name", "code": "required", "detail": "name must not be empty" },
    { "field": "owners[0].id", "code": "not_found", "detail": "no user or team with that id" }
  ]
}
```

This replaces the current `{"error": "message"}` shape.

## Pagination **(change)**

Cursor-based. Offset pagination drifts when rows are inserted mid-scan and degrades on large catalogs.

Request: `?limit=25&after={cursor}` (or `before={cursor}`). `limit` defaults to 25, caps at 1000.

Response envelope:

```json
{
  "data": [ ... ],
  "paging": {
    "after": "eyJpZCI6...",
    "before": null,
    "total": 1432
  }
}
```

- `after: null` means the last page.
- `total` is a best-effort count and may be an estimate on large collections; it is for display, never for pagination logic.
- Cursors are opaque base64 — clients must not parse or construct them. They encode the sort key plus a tiebreaker id, so the sort order is part of the cursor. Changing `sort` mid-pagination is a `400`.

Default sort is `fullyQualifiedName` ascending; `?sort=` accepts a small allowlist per collection.

**(built)** — `GET /tables` currently returns a bare JSON array. It becomes the envelope above.

## Filtering

Filters are explicit named query parameters, not a generic query language:

```
GET /tables?databaseSchema={fqn}&owner={id}&tags=PII.Sensitive&include=deleted
```

- Repeated params are AND (`?tags=a&tags=b` = has both).
- Unknown query parameters are a `400`, never silently ignored — a typo'd filter that silently returns everything is a data-leak-shaped bug.
- Rich, free-form querying is search's job (Epic 7), not the list endpoints'.

### `include`

| Value | Returns |
|---|---|
| `non-deleted` (default) | Tombstoned entities excluded |
| `deleted` | Only tombstoned |
| `all` | Both |

## Field selection

`?fields=owners,tags,columns` opts into expensive related data. The default response is the entity's own columns with no joins — a list of 1000 tables must not trigger 1000 owner lookups.

Requesting an unknown field name is a `400`.

## Concurrency control

Mutating requests may carry `If-Match: "0.2"` (the entity version). On mismatch the server returns `412` with the current version in the body. Absent the header, last-write-wins.

Connectors should always send `If-Match` — two concurrent connector runs against the same source otherwise interleave silently.

## Idempotency

- `PUT` and `DELETE` are naturally idempotent.
- `POST /{collection}` accepts an optional `Idempotency-Key` header; a repeat within 24h returns the original `201` response rather than a `409`.

## Bulk operations

Ingestion needs batching. `POST /{collection}/bulk` takes up to 1000 entities and returns per-item results with `207 Multi-Status`:

```json
{
  "succeeded": 998,
  "failed": 2,
  "results": [
    { "index": 0, "status": 201, "id": "..." },
    { "index": 7, "status": 409, "error": { "type": "...", "title": "..." } }
  ]
}
```

Partial success is the correct semantic: one bad table in a 1000-table schema scan must not discard the other 999.

## API versioning

The URL is unversioned. Breaking changes are avoided by adding fields rather than changing them; additive changes are not breaking, and clients must ignore unknown fields.

If a genuinely breaking change becomes unavoidable, it arrives as a `/v2` prefix with the previous version supported for two release cycles. There is no `/v1` prefix today precisely so that adding one later carries a clear signal.

## Content types

- Requests and responses: `application/json`.
- Errors: `application/problem+json`.
- Bulk import/export: `text/csv` on dedicated `/export` and `/import` sub-resources.
- Unsupported `Content-Type` → `415`.

## Naming

`camelCase` on the wire, `snake_case` in Rust, mapped with `#[serde(rename_all = "camelCase")]`. Timestamps are RFC 3339 UTC strings (`2026-07-26T10:30:00Z`); durations are ISO 8601 (`P30D`).

**(change)** — the wire format is currently `snake_case`. Switching to `camelCase` is a one-line serde attribute per struct and aligns with prevailing JSON API convention.
