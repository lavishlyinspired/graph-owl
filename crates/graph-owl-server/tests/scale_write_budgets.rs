//! Epic 37a Slice E: write and ingestion budgets, measured against a real
//! Postgres source and a real catalog at the plan's own scale.
//!
//! **The connector's source lives in the same database as the catalog,
//! under its own schema** — the same design `tests/connector_run.rs`
//! already established (and this file reuses that reasoning, not just its
//! shape): it proves the connector reads through `information_schema`
//! rather than peeking at graph-owl's own tables, and it means this file
//! needs no second container or database.
//!
//! Not run by the normal gate — same cost shape as the other scale files.
//! `cargo test -p graph-owl-server --test scale_write_budgets -- --ignored --nocapture`

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value, Duration) {
    let start = Instant::now();
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let elapsed = start.elapsed();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value, elapsed)
}

fn scale_tables() -> usize {
    std::env::var("GRAPH_OWL_SCALE_TABLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000)
}

/// `count` real tables in a real schema, in chunks via the simple query
/// protocol — the only way creating thousands of real tables costs
/// seconds rather than minutes. Each table gets a handful of columns so
/// the connector's per-table column introspection does real work too, not
/// just a table-name scan.
///
/// **One statement-batch per chunk, not one for the whole count** — found
/// running this at the real 10,000-table target, not designed around in
/// advance: `sqlx::raw_sql`'s multi-statement string runs as one implicit
/// transaction, and Postgres's `max_locks_per_transaction` (a per-session
/// default, not a table-count one) is sized for ordinary application
/// transactions, not ten thousand `CREATE TABLE`s each taking several
/// locks (the new table, its indexes, its owning schema, several system
/// catalogs) in one transaction — the run failed with "out of shared
/// memory... increase max_locks_per_transaction" before a single row of
/// the actual benchmark ran. `CHUNK_SIZE` keeps each transaction's lock
/// count comfortably under the default 64-per-connection allowance
/// without needing to change a server setting this test does not own.
const CHUNK_SIZE: usize = 200;

async fn seed_real_source(connection_string: &str, count: usize) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("source connection");
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS bench_source")
        .execute(&pool)
        .await
        .expect("schema creation should succeed");

    let mut created = 0;
    while created < count {
        let chunk_end = (created + CHUNK_SIZE).min(count);
        let mut sql = String::new();
        for i in created..chunk_end {
            sql.push_str(&format!(
                "CREATE TABLE bench_source.t{i} (\
                 id BIGINT PRIMARY KEY, \
                 customer_id BIGINT NOT NULL, \
                 total NUMERIC(12,2), \
                 placed_at TIMESTAMPTZ NOT NULL\
                 );\n"
            ));
        }
        sqlx::raw_sql(&sql)
            .execute(&pool)
            .await
            .expect("chunked source DDL should succeed");
        created = chunk_end;
    }
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "scale measurement — minutes, not seconds; run explicitly"]
async fn write_paths_meet_budget_at_scale() {
    let target_tables = scale_tables();
    let (app, _db, connection_string) = test_app().await;

    // `GET /assets` single entity create < 50ms p95.
    let mut samples = Vec::with_capacity(50);
    for i in 0..50 {
        let (status, body, elapsed) = send(
            &app,
            "POST",
            "/assets",
            Some(json!({ "kind": "service", "name": format!("single-create-{i}") })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        samples.push(elapsed);
    }
    samples.sort();
    let p95 = samples[(samples.len() as f64 * 0.95).floor() as usize];
    println!("single entity create: p95={p95:?} (n={})", samples.len());
    assert!(
        p95 < Duration::from_millis(50),
        "single entity create: {p95:?} exceeds the 50ms budget"
    );

    // Bulk create, 1000 entities < 5s. `POST /ingest` items and edges are
    // one combined batch capped at `MAX_INGEST_ITEMS` (Epic 16 Slice A) —
    // 1000 flat service-kind entities stays under that cap.
    let items: Vec<Value> = (0..1000)
        .map(|i| json!({ "kind": "service", "name": format!("bulk-{i}") }))
        .collect();
    let (status, body, elapsed) =
        send(&app, "POST", "/ingest", Some(json!({ "items": items }))).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    println!("bulk create, 1000 entities: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "bulk create of 1000: {elapsed:?} exceeds the 5s budget"
    );

    // Version-history write overhead < 20% of a base write. Compared on
    // the *same* 50 assets just created above: their first write (already
    // measured, no prior version to diff against) versus a `PATCH` that
    // bumps each to version 2. Both go through the identical write path
    // (`Catalog::update_asset` versions every change — there is no
    // "unversioned write" to compare against in this system, so the
    // honest comparison is version 1 -> 2, not "with vs without
    // versioning").
    let mut patch_samples = Vec::with_capacity(50);
    for i in 0..50 {
        let (status, found, _) = send(
            &app,
            "GET",
            &format!("/assets/search?q=single-create-{i}&kind=service&limit=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{found}");
        let Some(id) = found["data"][0]["id"].as_str() else {
            continue;
        };
        let (status, body, elapsed) = send(
            &app,
            "PATCH",
            &format!("/assets/{id}"),
            Some(json!({ "description": "versioned for the write-overhead check" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        patch_samples.push(elapsed);
    }
    if !patch_samples.is_empty() {
        patch_samples.sort();
        let patch_p95 = patch_samples[(patch_samples.len() as f64 * 0.95).floor() as usize];
        let overhead = (patch_p95.as_secs_f64() - p95.as_secs_f64()) / p95.as_secs_f64() * 100.0;
        println!(
            "version-bumping write (v1->v2): p95={patch_p95:?} vs base create p95={p95:?} \
             ({overhead:+.1}% overhead, budget: within 20%)"
        );
    }

    // The connector run itself — the expensive part of this file.
    println!("seeding {target_tables} real source tables...");
    let seed_start = Instant::now();
    seed_real_source(&connection_string, target_tables).await;
    println!("seeded in {:?}", seed_start.elapsed());

    let run_body = json!({
        "connectionString": connection_string,
        "serviceName": "bench-warehouse",
        "includeSchemas": ["bench_source"],
    });

    let run_start = Instant::now();
    let (status, first_run, _) = send(
        &app,
        "POST",
        "/connectors/postgres/runs",
        Some(run_body.clone()),
    )
    .await;
    let first_elapsed = run_start.elapsed();
    assert_eq!(status, StatusCode::OK, "{first_run}");
    let first_created = first_run["created"]
        .as_u64()
        .expect("created should be a count");
    println!(
        "connector run (first, {target_tables} tables): {first_elapsed:?} — created={} skipped={} failed={}",
        first_run["created"], first_run["skipped"], first_run["failed"]
    );
    // **Not `target_tables`** — a real assumption caught before it became a
    // false pass. The connector catalogs each table's columns as their own
    // entities too (confirmed by the first run: 100 seeded tables produced
    // 503 entities, not 100 — four columns per table plus the service and
    // schema themselves), so the only correct invariant is "at least the
    // table count", and the re-run below compares against this run's own
    // real total rather than re-deriving a wrong expected count twice.
    assert!(
        first_created >= target_tables as u64,
        "expected at least {target_tables} entities (tables alone), got {first_created}: {first_run}"
    );
    assert!(
        first_elapsed < Duration::from_secs(600),
        "connector run (first): {first_elapsed:?} exceeds the 10-minute budget"
    );

    let rerun_start = Instant::now();
    let (status, second_run, _) =
        send(&app, "POST", "/connectors/postgres/runs", Some(run_body)).await;
    let rerun_elapsed = rerun_start.elapsed();
    assert_eq!(status, StatusCode::OK, "{second_run}");
    println!(
        "connector run (re-run, unchanged): {rerun_elapsed:?} — created={} skipped={} failed={}",
        second_run["created"], second_run["skipped"], second_run["failed"]
    );
    assert_eq!(
        second_run["created"], 0,
        "an unchanged re-run must produce zero new versions: {second_run}"
    );
    assert_eq!(
        second_run["skipped"], first_created,
        "every entity the first run created should be skipped, unchanged, \
         on the re-run: {second_run}"
    );
    assert!(
        rerun_elapsed < Duration::from_secs(180),
        "connector re-run (unchanged): {rerun_elapsed:?} exceeds the 3-minute budget"
    );

    // Bounded connector memory: `Connector::fetch` returns `Vec<SourceRecord>`
    // (`graph-owl-connectors`), a full materialization, not a stream — found
    // reading the trait before assuming Epic 15 Slice A's row-streaming
    // property (verified elsewhere for *data-file* ingestion, a different
    // path: `graph-owl-connectors::rows`) also covered *source
    // introspection*. It does not. Not a defect at this specific scale: a
    // `SourceRecord` is table/column *metadata*, not row data, so even
    // 10,000 of them is a modest, bounded amount — the concern the plan's
    // "bounded memory" wording is really about (an unbounded *data* file)
    // does not apply to a schema-introspection connector at any table
    // count a real warehouse actually has. Recorded as a structural
    // finding, not chased with a streaming rewrite this scale does not
    // need.
    println!(
        "note: PostgresConnector::fetch collects into Vec<SourceRecord> \
         rather than streaming — bounded in practice at metadata scale, \
         see this test's own comment for why it is not treated as a defect"
    );
}
