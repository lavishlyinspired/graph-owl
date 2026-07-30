# Writing a custom adapter

**Epic 16, Slices E and F.** Everything here is the supported path for pushing
metadata from a source that will never have a shipped connector.

## The one architectural decision you need to know

**Custom adapters run out of process. There are no plugins.**

graph-owl will not load your code into its address space. That is not a gap —
it is decision 1 of `plans/16-ingestion-apis.md`, and it buys four things worth
more than the convenience of an in-process hook:

- **no ABI coupling** — you are not pinned to our Rust version, our allocator,
  or our release cadence
- **no shared crash blast radius** — an adapter that segfaults takes down an
  adapter, not the catalog
- **any language** — the contract is HTTP; two SDKs are provided and neither is
  privileged
- **your schedule** — you ship when your source changes, not when we release

The in-tree `Connector` trait (Epic 15) still exists, and it is the extension
point for sources worth maintaining upstream. If your source is Snowflake, that
is the right home. If your source is the spreadsheet your team keeps its data
contracts in, this is.

## The push contract

Two paths, and the difference is size, not capability.

| You have | Use | You get |
|---|---|---|
| up to 1000 entities and edges | `POST /ingest` | `207` with a per-item verdict |
| a file of any size | `POST /ingest/batch` | `202` and a job handle to poll |

`POST /ingest` is **synchronous and partially successful**: item 42 being wrong
does not discard the other 999. You get back an array of `{index, status, id,
problem}`, indexed against the list you sent.

`POST /ingest/batch` takes JSONL (`application/x-ndjson`) or CSV (`text/csv`)
as a raw body — not `multipart/form-data`, because every pusher here is a
program and multipart is a browser form encoding. Processing streams, so the
file size does not matter; poll `GET /ingest/jobs/{id}` until `state` leaves
`queued`/`running`.

Job states are five, and the fifth is the one people get wrong:

- `queued`, `running` — keep polling
- `succeeded` — every row landed
- **`partial` — some rows landed and some did not.** Not a failure. A client
  that retries the whole file on `partial` re-pushes 400k rows to fix 100k
- `failed` — nothing usable came of it, or it was stopped

**Entities are named by FQN, never by id.** You do not know our UUIDs and you
should not have to. A parent is `parentFqn`; the service derives the child's own
FQN from it.

**You do not have to submit in dependency order.** A batch containing a table
and the schema that contains it works regardless of which came first, and so
does an edge whose endpoints are in the same push. This is deliberate: a script
walking a source emits what it finds when it finds it.

## Idempotency

**Mandatory, not optional** (decision 4). Every push carries an
`Idempotency-Key`; both SDKs generate one for you.

Three rules that matter:

1. **A retry reuses the key.** A key per *attempt* makes the retry a second
   push, which is exactly the duplication the key exists to prevent. Both SDKs
   do this correctly; a hand-written client usually does not.
2. **A key identifies a request, not a slot.** Reusing one for *different*
   content is `409`, not a silent replay of the old answer. Retrying a `409`
   can never succeed — it is a bug in your code, and the SDKs deliberately do
   not loop on it.
3. **Keys expire after 24 hours.** A replay after that is a new request.

## Scoping

A push says what it is *about*, not what it is *not*. graph-owl does not delete
what your push omitted, because "absent from this file" and "deleted at the
source" are different facts and only your adapter can tell them apart.

If your adapter is authoritative for a subtree and wants deletions detected,
that is Epic 15's deletion-detection path with its threshold guard — not
something a push can express by silence.

## Error handling

Errors are [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) problem documents.
What your adapter should branch on:

| Status | Meaning | What to do |
|---|---|---|
| `207` | per-item outcomes | read `results`; some may have failed |
| `400` | the *request* is wrong | fix the code; do not retry |
| `409` | idempotency key reused for different content | fix the key generation |
| `429`, `5xx` | not now | retry with backoff — the SDKs do |

Both SDKs retry `429` and `5xx` with capped exponential backoff and jitter, and
refuse to retry anything else. The jitter is not decoration: a fleet of adapters
retrying on one schedule reconverges into the spike that made them retry.

## Setting up a bot principal

An adapter authenticates as itself, not as a person.

1. Create a service account in your identity provider and give it a client
   credentials grant.
2. Point graph-owl at the issuer (`OIDC_ISSUER`), which it already needs for
   human sign-in.
3. Grant the resulting subject the roles it needs. An ingesting adapter needs
   write on the subtree it owns and nothing else.
4. Pass the token to the SDK: `GraphOwlClient(base_url=..., token=...)`.

In development, running with no `OIDC_ISSUER` and no `GRAPH_OWL_JWT_SECRET`
puts the server in open mode — which it announces at startup, loudly, because a
server that is accidentally open must not look identical to one that is
deliberately open.

## The SDKs

Both are hand-written ergonomics over a generated type layer (decision 5:
"generated-only clients are unpleasant; hand-written ones drift"). They agree on
behaviour by construction — the two test suites assert the same properties,
because two SDKs that disagree about chunking or idempotency are two products
with one name.

### Python — `sdk/python`

Zero runtime dependencies, on purpose: your environment is not ours to add
packages to.

```python
from graph_owl_sdk import GraphOwlClient, IngestBuilder

client = GraphOwlClient(base_url="http://localhost:8080", token=TOKEN)

request = (
    IngestBuilder()
    .entity("service", "payments")
    .entity("database", "core", parent_fqn="payments")
    .entity("schema", "public", parent_fqn="payments.core")
    .entity("table", "orders", parent_fqn="payments.core.public")
    .build()
)
result = client.push(request)          # batches, keys and retries handled
```

A file, when there are more rows than a request can carry:

```python
handle = client.push_file(open("export.jsonl").read(), "jsonl")
job = client.await_job(handle["id"])
print(job["state"], job["accepted"], job["failures"][:5])
```

### TypeScript — `sdk/typescript`

```ts
import { GraphOwlClient, IngestBuilder } from "@graph-owl/sdk";

const client = new GraphOwlClient({ baseUrl: "http://localhost:8080", token });
const result = await client.push(
  new IngestBuilder()
    .entity({ kind: "service", name: "payments" })
    .entity({ kind: "database", name: "core", parentFqn: "payments" })
    .build(),
);
```

### Generating the type layer

Neither generated client is committed — it is derived from `openapi.json`, and a
committed copy is a second thing to keep in step with the first.

```bash
cargo run -p graph-owl-server --bin openapi > openapi.json
cd sdk/typescript && npm install && npm run generate
```

## A runnable example

`sdk/python/examples/csv_adapter.py` is a complete adapter: it reads a CSV of
tables, builds a hierarchy, pushes it, and reports what landed. It uses only the
published SDK surface — a test asserts that, because an example that reaches
into internals teaches people to reach into internals.

```bash
python sdk/python/examples/csv_adapter.py --base-url http://localhost:8080 \
    sdk/python/examples/tables.csv
```

## Verifying against a real service

```bash
scripts/verify-sdks.sh
```

Starts Postgres and a server, regenerates the contract, and round-trips both
SDKs against it. This runs on every PR — a contract change that breaks an SDK
fails the build rather than a customer's next push.
