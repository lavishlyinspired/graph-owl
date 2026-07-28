# graph-owl — Operations

**Crate scope**: cross-cutting. Owned by no epic; binding on all of them.
**Companion to** `00a`–`00f`. Describes the **target** state.

## Why this document exists

A cross-reference of every plan file against a pair of mature reference implementations found ten concerns that appear in **no plan at all** — not deferred, not rejected, simply absent. Absence is the dangerous category: a deferred decision is a decision, and an omitted one is discovered during an incident.

The common thread is that each belongs to no single epic. Data retention is not Epic 3's job or Epic 4's job; it is a property of the system that both must respect. Filing them under whichever epic happens to touch them first is how they get built three incompatible ways, so they live here and every epic conforms.

## 0. Supported PostgreSQL versions

*Added 28 July 2026, after a migration failure revealed the integration suite had
been running against PostgreSQL 11 the whole time.*

| | Version | Why |
|---|---|---|
| **Currently pinned** | **16-alpine** | What the suite has actually been run green against. |
| **Target** | **18** (major pinned, minor floats) | Testing on an older major than a deployment would run means the tested plan is not the plan that runs. The minor floats so a security release arrives without a code change; the major stays a deliberate decision. |
| **Minimum supported** | **14** | Set by what the design actually relies on, not by what is newest. |
| **Not used** | 19 beta | A beta in CI turns an unrelated failure into upstream triage, and its on-disk format may change between betas. It is worth running *ahead* of a major release, on a scheduled job that is allowed to fail, never as the gate on a commit. |

**Why the pin is 16 and not the target.** The bump to 18 is a one-constant
change and was written, then backed out unverified: `docker pull` could not
reach the registry from the environment the change was made in — a
`hello-world` pull hung identically — so the suite could not be run against 18
even once. Shipping an unverified pin to shared test infrastructure would break
every integration test for the next person with a failure that looks like their
own. The move is: pull `postgres:18-alpine`, change
`POSTGRES_IMAGE_TAG` in the nine test fixtures, run
`cargo test --workspace -- --test-threads=2`, and update this row.

**What sets the floor of 14**, in the order the constraint binds:

- **12** — `GENERATED … STORED` columns. Epic 8's search vector is one, which is
  what makes the search index transactionally consistent with the row rather
  than a derived store that can drift.
- **13** — deduplication of B-tree index entries, which the four flake index
  orderings depend on to stay proportionate: SPOT, PSOT, POST and OPST each
  repeat the same subject or predicate across millions of rows.
- **14** — bottom-up index deletion. `04-engine-triples.md`'s indexing review
  reasons about it explicitly. Running the suite on 11 meant that reasoning was
  an untested claim for as long as it stood.

**How this was found, and the general lesson.** `testcontainers-modules`'
`Postgres::default()` is `postgres:11-alpine`. Nothing in the suite had needed a
post-11 feature until a generated column did, so eleven epics' worth of
integration tests validated against a version three majors behind anything this
project would deploy on — and, worse, three majors behind the version its own
design notes cite. **A test dependency's default is a decision somebody else
made about your product.** The image tag is now a named constant with a stated
reason in every fixture, so the next person to read one is told what it is for
rather than left to inherit it.

## 1. Schema migration and rollback

Every epic from 2 onward ships a migration. No plan says what happens when one fails halfway, or when a release is rolled back after a migration has run.

**Standing rules:**

1. **Migrations are forward-only and additive within a release.** No `DROP COLUMN`, no `ALTER TYPE`, no `NOT NULL` on an existing column in the same release that starts writing it. A destructive change is split across two releases with a deploy between them.
2. **The expand/contract sequence is mandatory for any column that changes shape**: release N adds the new column and dual-writes; release N+1 backfills and reads the new one; release N+2 drops the old. Three releases is the cost of being able to roll back release N+1 without data loss, and it is always cheaper than the alternative.
3. **Rolling back the binary must never require rolling back the schema.** Version N-1 of the binary must run against version N of the schema. This is what makes a rollback a thirty-second operation instead of a restore-from-backup.
4. **A migration is tested against a populated database, not an empty one.** A migration that passes on a fresh schema and locks a 50-million-row table in production has been tested exactly nowhere useful.
5. **Long-running migrations run outside the deploy.** A backfill is a job, not a startup step; a service that will not accept traffic until 50 million rows are rewritten is an outage with a progress bar.

**Flake-specific**: the flake table is append-only and its `FlakeValue` discriminants are pinned (`04-engine-triples.md`). A migration that renumbers a discriminant, reassigns a namespace code, or rewrites `t` is not a migration — it is a rewrite of history, and time-travel makes it permanently visible. These are prohibited outright rather than governed.

## 2. Backup, restore, and the numbers

`37b-portability.md` covers logical export and restore — a portability feature. That is a different thing from disaster recovery, and conflating them leaves the actual DR question unanswered.

| Target | Value | Reasoning |
|---|---|---|
| **RPO** (data loss tolerated) | 15 minutes | Continuous WAL archiving. A metadata catalog can lose fifteen minutes of ingestion — the connectors re-run and converge (Epic 15). It cannot lose fifteen minutes of *human* curation, which is why this is minutes and not hours |
| **RTO** (time to serve) | 1 hour | Restore Postgres, start one binary. The single-service deployment model is what makes this achievable; it is a direct dividend of `00a-product-position.md`'s operational-simplicity budget |
| **Backup verification** | Weekly automated restore | An unverified backup is a belief, not a backup |
| **Retention** | 30 daily, 12 monthly | |

**Postgres is the backup boundary.** Everything reconstructible — search indexes (Epic 8), the reasoning overlay (Epic 6), analytics results (Epic 38) — is explicitly **not** backed up, because backing up derived data doubles the restore time to recover something a rebuild produces correctly. The restore runbook rebuilds them; the RTO above includes that rebuild.

## 3. Data retention

Three stores grow without bound and none has a stated policy.

| Store | Growth driver | Policy |
|---|---|---|
| Entity version history (Epic 3) | Every update | Keep all versions for 2 years, then **collapse** — retain first, last, and every Major, discard intermediate Minors. History becomes coarser with age, never absent |
| Flakes (Epic 4) | Every projection | **No time-based deletion.** Retraction (`op = false`) is the model; deleting old flakes deletes the differentiator. Growth is managed by partitioning (`37a-scale.md`), not expiry |
| Change events (Epic 3), usage (Epic 28), quality results (Epic 30) | Continuous | 90 days at full fidelity, then aggregate. These are observations, not assertions; nobody needs the individual query event from last year |

**Personal-data erasure.** A `User` entity carries a name and an email, and an erasure request is a legal instrument, not a soft delete. The mechanism is **crypto-shredding at the identity boundary**: personal fields are stored under a per-subject key, and erasure destroys the key. The graph keeps its structure — attribution edges, ownership history, the shape of what happened — while the personal data becomes unrecoverable.

This is the only honest answer for an append-only store. Physically deleting flakes would break the time-travel invariant that the whole engine rests on; leaving the data in place would not satisfy the request. Destroying the key satisfies it without lying about immutability.

*Scope note*: this describes personal data on `User` entities. Personal data that arrives inside a free-text description is an ingestion-side concern (Epic 25 classification), not something retention policy can retrofit.

## 4. Runbooks

Epic 10 makes the system observable. Observability without a runbook means every alert is a fresh investigation by whoever is awake.

Each of these ships **with the epic that creates the failure mode**, not afterwards, and each states: what the alert means, what to check first, what to do, and what *not* to do.

| Runbook | Epic | The mistake it prevents |
|---|---|---|
| Postgres unreachable | 10 | Restarting the service, which fixes nothing and loses in-flight work |
| Flake projection drift | 4 | Rebuilding the projection when reconciliation would fix it in place |
| Reasoning run capped | 6 | Raising the budget, when the `CappedReason` says the rule set has a cycle |
| Search index stale or corrupt | 8 | Rebuilding during peak load; the catalog serves fine without search |
| Connector run failed or stuck | 15 | Re-running with deletion detection on after a partial failure — the way to tombstone a live catalog |
| Ingestion backlog growing | 19 | Scaling consumers when the bottleneck is downstream |
| Certificate or JWKS rotation failure | 12 | Disabling authentication "temporarily" |
| Disk pressure from flake growth | 4, 37a | Deleting old flakes (see §1 — prohibited) |

**Ownership**: this is a single-team, single-tenant deployment model (`ROADMAP.md`). There is no on-call rotation to define, and inventing one for a product with no operator would be theatre. What is defined is that **every alert has a runbook, and an alert without one is a review-blocking finding** — that rule survives whatever the team shape turns out to be.

## 5. Testing strategy above the unit

Plans specify unit, integration, and mutation testing well. What no plan defines is the level above: what runs against a full system, and what it is allowed to assume.

| Level | Runs against | Scope | Gate |
|---|---|---|---|
| Unit | Nothing external | Pure logic. Exhaustive, mutation-verified | Every PR |
| Integration | Real Postgres via testcontainers | One adapter or one endpoint | Every PR |
| **Contract** | Generated client vs live OpenAPI | Every documented endpoint compiles and round-trips | Every PR |
| **Journey** | Full stack, seeded corpus | The seven journeys below | Every PR |
| **Scale** | Full stack, 100k entities | `37a-scale.md` budgets | Nightly |

**The seven journeys.** Each crosses at least three epics, which is exactly what unit tests cannot cover:

1. Connector run → entity appears → searchable → viewable in the console.
2. Ingest → validate → violation appears → waive with reason → violation still visible on the asset.
3. Assert a fact → reason → derived fact appears with its explanation → retract the premise → derived fact disappears.
4. Write a memory as an agent → retrieve it via MCP → correct it as a human → history shows both.
5. Change an upstream table → impact analysis lists downstream → certification invalidated.
6. Query as principal A and principal B → different result sets, consistent counts, no existence leak.
7. Time travel: view the estate as of a date before a deletion → the deleted entity is present.

**Journey tests own their data.** A journey that depends on a shared seeded database passes locally, fails in CI, and gets marked flaky rather than fixed. Each journey seeds what it needs and cleans up.

## 6. The trait boundary

`Storage` and `TripleStore` are each one trait carrying both reads and writes. Splitting them costs nothing now and buys three things:

```rust
pub trait TripleStoreRead: Send + Sync { /* query_pattern, exists, count, … */ }
pub trait TripleStoreWrite: TripleStoreRead { /* assert, retract */ }
```

1. **A read-only consumer takes a read-only handle.** Epic 7d's Bolt server is read-only by decision; expressing that in the type system is better than expressing it in a code review. Epic 7's query engine, Epic 38's analytics, and Epic 40's console are all read-only.
2. **Test doubles get simpler.** Most tests need a read fake; today they must stub write methods they never call.
3. **A future read-replica adapter is a `TripleStoreRead` impl** and cannot accidentally be handed a write.

The same split applies to `Storage`. **This is a refactor to make when Epic 7d lands**, not before — doing it now against one consumer is a boundary drawn without information, and `00e-crate-architecture.md`'s revisit trigger says split when the dependency set genuinely diverges. Epic 7d is the divergence.

## 7. What this document does not cover

| Not here | Where |
|---|---|
| Resource budgets, health, metrics, shutdown | `10-operability.md` — this document defers to it entirely |
| Logical export and restore | `37b-portability.md` — portability, not disaster recovery |
| Scale targets and partitioning triggers | `37a-scale.md` |
| Authentication and authorization | `12-13-security.md` |
| Rate limiting | `01-api-conventions.md` (contract) and `10-operability.md` (enforcement) |
| Multi-region, HA, failover | Not planned. Single-node is the deployment model (`ROADMAP.md`) |
