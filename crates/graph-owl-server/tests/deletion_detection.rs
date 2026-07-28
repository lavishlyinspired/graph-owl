//! Epic 15: a re-run tombstones what the source no longer reports.
//!
//! This is the one connector behaviour that destroys information, so the
//! tests are as much about what it *refuses* to do as about what it does.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn execute(connection_string: &str, statements: &[&str]) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("source connection");
    for statement in statements {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{statement}: {e}"));
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
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
    let status = response.status();
    (status, json_body(response).await)
}

async fn run(app: &axum::Router, connection_string: &str, body: Value) -> Value {
    let mut payload = json!({
        "connectionString": connection_string,
        "serviceName": "hdfc-core",
        "includeSchemas": ["payments"]
    });
    for (key, value) in body.as_object().expect("object") {
        payload[key] = value.clone();
    }
    let (status, report) = send(app, "POST", "/connectors/postgres/runs", Some(payload)).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    report
}

async fn live_count(app: &axum::Router) -> usize {
    let (_, listed) = send(app, "GET", "/assets?limit=1000", None).await;
    listed["data"].as_array().expect("data").len()
}

async fn fixture() -> (axum::Router, common::TestDb, String) {
    let (app, container, connection_string) = test_app().await;
    execute(
        &connection_string,
        &[
            "CREATE SCHEMA IF NOT EXISTS payments",
            "CREATE TABLE payments.upi_transactions (txn_id TEXT PRIMARY KEY, amount NUMERIC)",
            "CREATE TABLE payments.neft_transactions (utr TEXT PRIMARY KEY, amount NUMERIC)",
            "CREATE TABLE payments.imps_transactions (rrn TEXT PRIMARY KEY, amount NUMERIC)",
            "CREATE TABLE payments.rtgs_transactions (utr TEXT PRIMARY KEY, amount NUMERIC)",
            "CREATE TABLE payments.settlements (id BIGINT PRIMARY KEY, netted NUMERIC)",
        ],
    )
    .await;
    run(&app, &connection_string, json!({})).await;
    (app, container, connection_string)
}

/// Off by default. A run that deletes is a different kind of operation from
/// one that only adds, and defaulting to the destructive reading of "sync" is
/// how a routine re-run becomes an incident.
#[tokio::test]
async fn a_plain_rerun_does_not_delete_anything() {
    let (app, _container, connection_string) = fixture().await;
    let before = live_count(&app).await;

    execute(&connection_string, &["DROP TABLE payments.settlements"]).await;
    let report = run(&app, &connection_string, json!({})).await;

    assert!(report["deletions"].is_null(), "{report}");
    assert_eq!(
        live_count(&app).await,
        before,
        "a run that was not asked to delete must not delete"
    );
}

/// **The behaviour.** A dropped table is tombstoned, and its columns go with
/// it because soft delete cascades.
#[tokio::test]
async fn a_dropped_table_is_tombstoned_when_deletion_detection_is_on() {
    let (app, _container, connection_string) = fixture().await;
    let before = live_count(&app).await;

    execute(&connection_string, &["DROP TABLE payments.settlements"]).await;
    let report = run(&app, &connection_string, json!({ "detectDeletions": true })).await;

    assert!(
        report["deletions"]["refused"].is_null(),
        "one table of five is ordinary churn: {report}"
    );
    let after = live_count(&app).await;
    assert!(after < before, "{before} -> {after}");

    let (_, found) = send(&app, "GET", "/assets/search?q=settlements", None).await;
    assert!(
        found["data"].as_array().expect("data").is_empty(),
        "the tombstoned table must leave search"
    );
}

/// The tombstone is a soft delete, so the metadata survives — which is what
/// makes the asset recoverable and its history readable at an earlier instant.
#[tokio::test]
async fn a_tombstoned_asset_is_still_readable_and_marked_deleted() {
    let (app, _container, connection_string) = fixture().await;
    let (_, found) = send(&app, "GET", "/assets/search?q=settlements&kind=table", None).await;
    let id = found["data"][0]["id"].as_str().expect("id").to_string();

    execute(&connection_string, &["DROP TABLE payments.settlements"]).await;
    run(&app, &connection_string, json!({ "detectDeletions": true })).await;

    let (status, asset) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "a tombstone is not a 404");
    assert_eq!(asset["deleted"], true);
    assert!(asset["deletedAt"].is_string());
}

/// **The guard.** A connection string pointed at a database with none of the
/// expected schemas reports nothing, and must not tombstone the estate.
#[tokio::test]
async fn a_source_reporting_nothing_is_refused_and_deletes_nothing() {
    let (app, _container, connection_string) = fixture().await;
    let before = live_count(&app).await;

    // The scope now names a schema that does not exist — the shape of a
    // misconfiguration, from the catalog's point of view.
    let (status, report) = send(
        &app,
        "POST",
        "/connectors/postgres/runs",
        Some(json!({
            "connectionString": connection_string,
            "serviceName": "hdfc-core",
            "includeSchemas": ["a_schema_that_does_not_exist"],
            "detectDeletions": true
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let refusal = report["deletions"]["refused"]
        .as_str()
        .unwrap_or_else(|| panic!("the run must refuse: {report}"));

    // Note which guard fires. Not the "reported nothing at all" one: the
    // connector re-creates the service and database rows on every run
    // regardless of schema scope, so a misconfigured run reports 2 of 18
    // assets rather than 0 — and it is the *threshold* that catches it.
    //
    // Worth knowing, because it means the 100%-absent special case is close to
    // unreachable through the connector path, and the threshold is doing the
    // real work of protecting the estate.
    assert!(
        refusal.contains("89%") || refusal.contains("connection string"),
        "the refusal must name what it saw: {refusal}"
    );
    assert_eq!(
        live_count(&app).await,
        before,
        "a refused run deletes nothing at all — not even the part under the threshold"
    );
}

/// A refusal is all-or-nothing. Stopping partway would leave the estate in a
/// state neither the source nor the catalog describes, which is worse than
/// either outcome the guard is choosing between.
#[tokio::test]
async fn a_run_over_the_threshold_deletes_nothing_at_all() {
    let (app, _container, connection_string) = fixture().await;
    let before = live_count(&app).await;

    // Four of five tables gone: far past the 20% default.
    execute(
        &connection_string,
        &[
            "DROP TABLE payments.settlements",
            "DROP TABLE payments.rtgs_transactions",
            "DROP TABLE payments.imps_transactions",
            "DROP TABLE payments.neft_transactions",
        ],
    )
    .await;

    let report = run(&app, &connection_string, json!({ "detectDeletions": true })).await;
    assert!(
        report["deletions"]["refused"].is_string(),
        "must refuse: {report}"
    );
    assert_eq!(live_count(&app).await, before, "and delete nothing");
}

/// The guard forces a decision; it does not make deletion impossible.
#[tokio::test]
async fn an_operator_can_raise_the_threshold_deliberately() {
    let (app, _container, connection_string) = fixture().await;
    let before = live_count(&app).await;

    execute(
        &connection_string,
        &[
            "DROP TABLE payments.settlements",
            "DROP TABLE payments.rtgs_transactions",
            "DROP TABLE payments.imps_transactions",
        ],
    )
    .await;

    let report = run(
        &app,
        &connection_string,
        json!({ "detectDeletions": true, "deletionThreshold": 0.9 }),
    )
    .await;

    assert!(
        report["deletions"]["refused"].is_null(),
        "an explicit threshold must be honoured: {report}"
    );
    assert!(live_count(&app).await < before);
}

/// An asset that failed to ingest is a write problem, not an absence. Deciding
/// deletion against the *fetched* records rather than the ingested ones would
/// convert a transient error into data loss.
#[tokio::test]
async fn a_run_that_deletes_nothing_reports_a_plan_with_no_refusal() {
    let (app, _container, connection_string) = fixture().await;
    let before = live_count(&app).await;

    let report = run(&app, &connection_string, json!({ "detectDeletions": true })).await;

    assert_eq!(report["deletions"]["absent"], 0, "{report}");
    assert!(report["deletions"]["refused"].is_null());
    assert_eq!(
        live_count(&app).await,
        before,
        "nothing changed in the source"
    );
}

/// **Demo 3's moment, made real.** A column dropped from the source is gone
/// from the catalog now, and still present at an instant before the run.
#[tokio::test]
async fn a_dropped_column_is_absent_now_and_present_at_an_earlier_instant() {
    let (app, _container, connection_string) = fixture().await;

    let (_, found) = send(
        &app,
        "GET",
        "/assets/search?q=upi_transactions.amount&kind=column",
        None,
    )
    .await;
    let column_id = found["data"][0]["id"]
        .as_str()
        .expect("the column should be catalogued")
        .to_string();

    let before_the_migration = (chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;

    execute(
        &connection_string,
        &["ALTER TABLE payments.upi_transactions DROP COLUMN amount"],
    )
    .await;
    run(&app, &connection_string, json!({ "detectDeletions": true })).await;

    let (status, now) = send(&app, "GET", &format!("/assets/{column_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(now["deleted"], true, "the column is gone now");

    let encoded = before_the_migration.replace(':', "%3A").replace('+', "%2B");
    let (status, earlier) = send(
        &app,
        "GET",
        &format!("/assets/{column_id}?asOf={encoded}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        earlier["deleted"], false,
        "and was live before the migration — reconstructed from the graph"
    );
    assert_eq!(earlier["name"], "amount");
}
