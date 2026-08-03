//! Epic 28 at the wire — usage and popularity.
//!
//! **Two assertions carry this epic.** Query text must be *absent from storage*
//! when the deployment has not opted in — dropped at the boundary, not filtered
//! on read, because only one of those survives a database dump landing somewhere
//! it should not. And pruning must never erase `last_accessed`, which is the
//! single most useful signal there is.
//!
//! The trend arithmetic is proved exhaustively in `graph_owl_core::usage`,
//! without a database. These tests prove the wiring.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request should build");

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should be handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, parsed)
}

async fn service(app: &axum::Router, name: &str) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string()
}

fn observation(fqn: &str, consumer: &str, days_ago: i64, query_id: &str) -> Value {
    json!({
        "assetFqn": fqn,
        "consumer": consumer,
        "operation": "read",
        "occurredAt": chrono::Utc::now() - chrono::Duration::days(days_ago),
        "queryId": query_id,
    })
}

async fn push(app: &axum::Router, observations: Vec<Value>) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/usage",
        Some(json!({ "observations": observations })),
    )
    .await
}

// ── Slice A: ingest ─────────────────────────────────────────────────────────

#[tokio::test]
async fn observations_are_ingested_in_a_batch() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = push(
        &app,
        vec![
            observation(&fqn, "asha", 1, "q1"),
            observation(&fqn, "etl_bot", 2, "q2"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["accepted"], 2, "{body}");
    assert_eq!(body["unmatched"], 0);
}

/// **An observation about a table nobody has catalogued yet is kept**, not
/// rejected — the connector may simply not have run, and discarding it would
/// throw away exactly the usage that tells you something is missing from the
/// catalog.
#[tokio::test]
async fn usage_for_an_uncatalogued_asset_is_retained_and_reported() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = push(
        &app,
        vec![observation("not-catalogued-yet", "asha", 1, "q1")],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["accepted"], 1, "kept, not dropped: {body}");
    assert_eq!(
        body["unmatched"], 1,
        "and reported, because that is a connector gap: {body}"
    );
}

/// A duplicate `(asset, query_id)` is ignored, so re-ingesting a log file is
/// free rather than doubling every count.
#[tokio::test]
async fn a_repeated_query_id_is_ignored() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    push(&app, vec![observation(&fqn, "asha", 1, "q1")]).await;
    let (_, second) = push(&app, vec![observation(&fqn, "asha", 1, "q1")]).await;

    assert_eq!(second["accepted"], 0, "{second}");
    assert_eq!(second["duplicates"], 1, "{second}");
}

/// An observation dated in the future is a clock problem, and storing it would
/// make every window computation wrong until it passed.
#[tokio::test]
async fn an_observation_in_the_future_is_rejected() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (_, body) = push(&app, vec![observation(&fqn, "asha", -5, "q1")]).await;

    assert_eq!(body["rejected"], 1, "{body}");
    assert_eq!(body["accepted"], 0, "{body}");
}

/// **Usage is not a metadata change.** A thousand observations must not produce
/// a thousand versions — the asset did not change, somebody read it.
#[tokio::test]
async fn ingesting_usage_does_not_bump_the_assets_version() {
    let (app, _db, _url) = test_app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "orders-svc" })),
    )
    .await;
    let id = created["id"].as_str().expect("an id").to_string();
    let fqn = created["fullyQualifiedName"].as_str().expect("an fqn");
    let before = created["version"].clone();

    let batch: Vec<Value> = (0..20)
        .map(|n| observation(fqn, "asha", 1, &format!("q{n}")))
        .collect();
    push(&app, batch).await;

    let (_, after) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(
        after["version"], before,
        "reading a table is not editing it: {after}"
    );
}

#[tokio::test]
async fn an_unrecognised_operation_is_refused_listing_the_real_ones() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/usage",
        Some(json!({
            "observations": [{
                "assetFqn": fqn,
                "consumer": "asha",
                "operation": "browsed",
                "occurredAt": chrono::Utc::now(),
            }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("schemaRead"), "{body}");
}

// ── Slice B: rollups ────────────────────────────────────────────────────────

#[tokio::test]
async fn observations_fold_into_daily_rollups() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    push(
        &app,
        vec![
            observation(&fqn, "asha", 1, "q1"),
            observation(&fqn, "asha", 1, "q2"),
            observation(&fqn, "etl_bot", 1, "q3"),
        ],
    )
    .await;

    let (status, rollups) = send(&app, "GET", &format!("/usage/{fqn}/rollups"), None).await;
    assert_eq!(status, StatusCode::OK, "{rollups}");
    let rows = rollups.as_array().expect("an array");
    assert_eq!(rows.len(), 2, "one row per consumer per day: {rollups}");
    let asha = rows
        .iter()
        .find(|r| r["consumerKey"] == "opaque:asha")
        .unwrap_or_else(|| panic!("{rollups}"));
    assert_eq!(asha["count"], 2, "{rollups}");
}

/// **A late arrival lands on its own day**, not today's — the rollup key is
/// `(asset, consumer, day, operation)` precisely so that a log file processed a
/// week late still reports the week it describes.
#[tokio::test]
async fn a_late_arriving_observation_updates_the_day_it_belongs_to() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    push(&app, vec![observation(&fqn, "asha", 0, "today")]).await;
    push(&app, vec![observation(&fqn, "asha", 5, "last-week")]).await;

    let (_, rollups) = send(&app, "GET", &format!("/usage/{fqn}/rollups"), None).await;
    let rows = rollups.as_array().expect("an array");
    assert_eq!(rows.len(), 2, "two days, not one: {rollups}");
    assert!(
        rows.iter().all(|r| r["count"] == 1),
        "neither day absorbed the other: {rollups}"
    );
}

// ── Slice C: popularity and trend ───────────────────────────────────────────

#[tokio::test]
async fn popularity_is_computed_from_the_rollups() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let batch: Vec<Value> = (0..8)
        .map(|n| observation(&fqn, "asha", 2, &format!("q{n}")))
        .chain((0..3).map(|n| observation(&fqn, "etl_bot", 20, &format!("old{n}"))))
        .collect();
    push(&app, batch).await;

    let (status, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;

    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["queriesLast7d"], 8, "{summary}");
    assert_eq!(summary["queriesLast30d"], 11, "{summary}");
    assert_eq!(summary["distinctConsumers30d"], 2, "{summary}");
    assert!(summary["lastAccessed"].is_string(), "{summary}");
}

/// **The test that stops an asset being wrongly retired.** Nothing was ever
/// ingested, so nothing is known — reporting `dormant` would be a false
/// negative somebody acts on.
#[tokio::test]
async fn an_asset_with_no_usage_reports_unknown_not_dormant() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;

    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["queriesLast7d"], 0);
    assert_eq!(
        summary["trend"], "unknown",
        "absence of data is not absence of use: {summary}"
    );
}

/// And a genuinely untouched asset *is* dormant, or the distinction above would
/// make the signal unreachable.
#[tokio::test]
async fn an_asset_untouched_for_a_quarter_is_dormant() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    push(&app, vec![observation(&fqn, "asha", 120, "ancient")]).await;

    let (_, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;
    assert_eq!(summary["trend"], "dormant", "{summary}");
}

/// **The floor**: three queries in a fortnight is not a rising asset, however
/// the ratio looks.
#[tokio::test]
async fn tiny_counts_do_not_report_a_trend() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    push(
        &app,
        vec![
            observation(&fqn, "asha", 1, "a"),
            observation(&fqn, "asha", 2, "b"),
            observation(&fqn, "asha", 9, "c"),
        ],
    )
    .await;

    let (_, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;
    assert_eq!(summary["trend"], "stable", "{summary}");
}

/// A schema read is not use — a BI tool refreshing its catalogue touches every
/// table it can see, and counting that would make the whole estate look busy.
#[tokio::test]
async fn schema_reads_do_not_count_toward_popularity() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    send(
        &app,
        "POST",
        "/usage",
        Some(json!({
            "observations": [{
                "assetFqn": fqn,
                "consumer": "bi_tool",
                "operation": "schemaRead",
                "occurredAt": chrono::Utc::now(),
                "queryId": "catalogue-refresh",
            }],
        })),
    )
    .await;

    let (_, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;
    assert_eq!(summary["queriesLast7d"], 0, "{summary}");
    assert_eq!(summary["distinctConsumers30d"], 0, "{summary}");
}

// ── Slice D: privacy ────────────────────────────────────────────────────────

/// **The data-protection assertion, and it is about storage rather than
/// display.** Query text is dropped at the boundary when the deployment has not
/// opted in — not filtered on read, because only one of those survives a
/// database dump landing somewhere it should not.
#[tokio::test]
async fn query_text_is_never_persisted_when_the_deployment_has_not_opted_in() {
    let (app, _db, url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/usage",
        Some(json!({
            "observations": [{
                "assetFqn": fqn,
                "consumer": "asha",
                "operation": "read",
                "occurredAt": chrono::Utc::now(),
                "queryId": "q1",
                "queryText": "SELECT * FROM orders WHERE customer_id = 'CUST-4471'",
            }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the batch is accepted: {body}");
    assert_eq!(body["accepted"], 1);

    // Read the column directly. Asserting through the API would only prove it
    // is not *shown*, which is the weaker property and the one that fails a
    // dump.
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let stored: Vec<Option<String>> =
        sqlx::query_scalar("SELECT query_text FROM usage_observations")
            .fetch_all(&pool)
            .await
            .expect("the observations should be readable");

    assert_eq!(stored.len(), 1, "the observation itself was kept");
    assert!(
        stored[0].is_none(),
        "the query text must never have reached storage, not merely be hidden: {stored:?}"
    );
}

// ── Slice E: retention ──────────────────────────────────────────────────────

/// **The last observation survives pruning**, whatever its age. Pruning
/// `last_accessed` out of existence would blank the single most useful signal
/// there is — and an asset with no last-access reads as never used.
#[tokio::test]
async fn pruning_keeps_the_most_recent_observation_per_asset() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    push(
        &app,
        vec![
            observation(&fqn, "asha", 200, "ancient"),
            observation(&fqn, "asha", 150, "old"),
        ],
    )
    .await;

    let (status, pruned) = send(&app, "POST", "/usage/prune", None).await;
    assert_eq!(status, StatusCode::OK, "{pruned}");
    assert_eq!(pruned["pruned"], 1, "one of two, not both: {pruned}");

    let (_, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;
    assert!(
        summary["lastAccessed"].is_string(),
        "the signal survives the prune: {summary}"
    );
    assert_eq!(
        summary["trend"], "dormant",
        "and it still reads as dormant rather than unknown: {summary}"
    );
}

/// Recent observations are untouched, or pruning would be deletion.
#[tokio::test]
async fn pruning_leaves_observations_inside_the_window_alone() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    push(
        &app,
        vec![
            observation(&fqn, "asha", 1, "recent"),
            observation(&fqn, "asha", 2, "also-recent"),
        ],
    )
    .await;

    let (_, pruned) = send(&app, "POST", "/usage/prune", None).await;

    assert_eq!(pruned["pruned"], 0, "{pruned}");
    let (_, summary) = send(&app, "GET", &format!("/usage/{fqn}"), None).await;
    assert_eq!(summary["queriesLast7d"], 2, "{summary}");
}

/// **Rollups survive pruning.** They are the whole reason raw rows can be
/// discarded — the aggregate is what every question actually asks.
#[tokio::test]
async fn rollups_survive_the_pruning_of_their_raw_rows() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    push(
        &app,
        vec![
            observation(&fqn, "asha", 200, "ancient"),
            observation(&fqn, "asha", 199, "also-ancient"),
        ],
    )
    .await;

    send(&app, "POST", "/usage/prune", None).await;

    let (_, rollups) = send(&app, "GET", &format!("/usage/{fqn}/rollups"), None).await;
    let rows = rollups.as_array().expect("an array");
    assert_eq!(
        rows.len(),
        2,
        "both days are still counted after the raw rows went: {rollups}"
    );
}
