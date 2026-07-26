# graph-owl

Rust metadata-catalog service. Layered workspace:

```
graph-owl-core             pure domain types, no I/O
graph-owl-storage           Storage trait + StorageError (the port)
graph-owl-storage-postgres   sqlx-backed impl of Storage (the adapter)
graph-owl-api                Catalog facade, wraps Arc<dyn Storage>
graph-owl-server             axum HTTP layer
```

Edition 2024, Rust workspace with `[workspace.lints.clippy] all = "warn", pedantic = "warn"`.

## Process

TDD is non-negotiable: RED (failing test first) → GREEN (minimum code) → MUTATE (`cargo mutants`) → KILL MUTANTS → REFACTOR (only if it adds value). Never commit without explicit user approval — this holds even during "complete all remaining slices" autonomous runs; still pause at each commit point.

Never commit/write any mention of the third-party reference systems anywhere in this project (code, comments, commit messages, plan docs). The local reference clones under `.claude/docs/referenceRepo/` may stay on disk for architecture research but must never be committed or cited in project files. This project's git history was deliberately squashed once already to scrub prior references — don't reintroduce them.

## Gotchas learned building the Table entity slice

- **axum 0.7 + edition 2024 doesn't mix.** Implementing a custom `FromRequest<S>` extractor against axum 0.7 from an edition-2024 crate fails with `E0195` (lifetime params on `from_request` don't match the trait). axum-core 0.7.x was authored under edition-2021 RPITIT capture rules; edition 2024 changed them. Fix is to upgrade to axum 0.8 (native async-fn-in-trait, edition-2024 compatible) — not to downgrade the workspace edition. Also note: axum 0.8 changed path param syntax from `:id` to `{id}`.

- **testcontainers-rs: keep the container handle alive.** `ContainerAsync<Postgres>` must stay bound for as long as the test needs the database — if a helper function returns only the connection string/pool and drops the container locally, Docker tears it down almost immediately and the next query fails with a pool timeout. Test helpers must return the container alongside the pool, e.g. `(PostgresStorage, ContainerAsync<Postgres>, String)`, and the caller must bind it (even as `_container`) for the test's duration.

- **refinery has no direct sqlx integration.** Migrations need a separate `tokio_postgres::Client` (via `tokio_postgres::connect(..., NoTls)`, with the connection future spawned) alongside the `sqlx::PgPool` used for app queries. Run via `embedded::migrations::runner().run_async(&mut migration_client).await`, with migrations embedded through `refinery::embed_migrations!("migrations")`.

- **Postgres `TIMESTAMPTZ` is microsecond precision; `chrono::Utc::now()` is nanosecond.** Verified non-flaky across repeated test runs in this project, but worth remembering if a future equality assertion on a round-tripped timestamp ever looks suspicious.

- **Partial updates: one atomic `UPDATE ... SET x = COALESCE($n, x) ... RETURNING`,** not read-then-write. Avoids a race between the read and the write, and lets Postgres's own `now()` set `updated_at` rather than passing a Rust-side timestamp.

- **PATCH immutability via DTO shape, not validation.** `TableUpdate` simply has no `id`/`fully_qualified_name` fields, so there's nothing for a client to send that could mutate them — serde silently drops unknown fields. Prefer this structural approach over runtime rejection when a field should never be client-settable on an endpoint.

- **Custom 400 vs axum's default 422.** axum's built-in `Json<T>` extractor returns `422 Unprocessable Entity` for a syntactically valid but semantically invalid body (e.g. a missing required field). This project's acceptance criteria require `400` instead, so `graph-owl-server` wraps it in a custom `AppJson<T>` extractor that remaps the rejection.

- **Test organization:** `tests/common/mod.rs` (a subdirectory containing `mod.rs`) is treated by Cargo as a shared module importable from multiple integration test binaries in the same crate. A top-level `tests/common.rs` file, by contrast, becomes its own separate test target — not what you want for shared helpers.

## Storage backends vs. source connectors — scaling architecture

These are different problems and shouldn't share a pattern:

- **Storage backends** (where the catalog's own data lives — e.g. Postgres, and later MongoDB) are bounded to a handful of options. One crate per backend, each implementing the `Storage` trait, is the right granularity — `graph-owl-storage-postgres`, later `graph-owl-storage-mongodb`. A factory/config switch at startup (in `graph-owl-server`'s `main.rs`) picks one.

- **Source connectors** (external systems the catalog *catalogs* — Snowflake, Kafka, etc., potentially 100+) do not get one crate each. Verified against a mature reference implementation, which implements all of these as modules inside a single ingestion package behind a shared connector interface, not as 100 separate packages. The Rust equivalent: one `graph-owl-connectors` crate with a module per connector implementing a shared `Connector` trait.

MongoDB storage-backend support is explicitly deferred (not yet implemented) — see `plans/graph-owl-table-entity.md`'s "Explicitly deferred" section.

## Plans

`plans/graph-owl-table-entity.md` documents the completed Table-entity walking skeleton (Slices A-E, all done) and lists explicitly deferred follow-on work. Left in place as a historical record — do not delete.
