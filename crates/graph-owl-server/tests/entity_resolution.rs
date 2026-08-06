//! Epic 17 at the wire.
//!
//! The facade tests (`resolution_decides_before_it_merges`) prove the
//! decisions against a recording graph double. This proves the **HTTP
//! surface** against a real Postgres and a real graph engine — the same
//! shape the composition root wires (`tests/common::build_catalog`) — which
//! is what makes `POST /assets/{id}/resolve` and `POST /merges/{id}/split`
//! genuinely reachable rather than only unit-tested.

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

async fn asset(app: &axum::Router, name: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("an id").to_string()
}

/// No endpoint lists merge records yet — Slice F's review queue is where one
/// lands. Read directly, matching how other suites here inspect state a
/// route does not expose yet (e.g. `authorization.rs`'s direct pool use).
async fn merge_id_for(connection_string: &str, merged: &str) -> uuid::Uuid {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("connect");
    sqlx::query_scalar("SELECT id FROM merge_records WHERE merged_id = $1")
        .bind(uuid::Uuid::parse_str(merged).expect("uuid"))
        .fetch_one(&pool)
        .await
        .expect("a merge record for the merged asset")
}

#[tokio::test]
async fn resolving_a_case_variant_merges_it_into_the_canonical_entity() {
    let (app, _db, _connection_string) = test_app().await;

    let lower = asset(&app, "orders").await;
    let upper = asset(&app, "ORDERS").await;

    let (status, body) = send(&app, "POST", &format!("/assets/{upper}/resolve"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "existing");
    assert_eq!(body["entity"], lower);
    assert_eq!(body["confidence"], json!(1.0));
}

#[tokio::test]
async fn resolving_an_asset_with_no_candidates_reports_new() {
    let (app, _db, _connection_string) = test_app().await;

    let lonely = asset(&app, "zzqxw-only").await;

    let (status, body) = send(&app, "POST", &format!("/assets/{lonely}/resolve"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "new");
}

#[tokio::test]
async fn resolving_an_unknown_asset_is_a_404() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/assets/{}/resolve", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn a_merge_can_be_split_and_a_second_split_is_a_409() {
    let (app, _db, connection_string) = test_app().await;

    let _lower = asset(&app, "orders").await;
    let upper = asset(&app, "ORDERS").await;

    let (status, body) = send(&app, "POST", &format!("/assets/{upper}/resolve"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let merge_id = merge_id_for(&connection_string, &upper).await;

    let (status, body) = send(&app, "POST", &format!("/merges/{merge_id}/split"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["splitAt"].is_string(), "{body}");

    let (status, body) = send(&app, "POST", &format!("/merges/{merge_id}/split"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["type"],
        "https://graph-owl.dev/errors/merge-already-split"
    );
    assert_eq!(body["title"], "This merge has already been split");
    assert_eq!(body["detail"], "this merge has already been split");
}

#[tokio::test]
async fn splitting_an_unknown_merge_is_a_404() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/merges/{}/split", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

async fn asset_with_parent(app: &axum::Router, kind: &str, name: &str, parent_id: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": kind, "name": name, "parentId": parent_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("an id").to_string()
}

/// A `Table`'s parent must be a `Schema`, whose parent must be a `Database`,
/// whose parent must be a `Service` — the full chain, so two tables under it
/// can share the `same_parent` scoring term.
async fn a_schema(app: &axum::Router) -> String {
    let service = asset(app, "svc").await;
    let database = asset_with_parent(app, "database", "db", &service).await;
    asset_with_parent(app, "schema", "sch", &database).await
}

/// Two same-schema tables with matching columns but different names —
/// scores squarely in the review band (0.6-0.9), so resolving `b` queues it
/// rather than merging or discarding it.
async fn an_ambiguous_pair(app: &axum::Router) -> (String, String) {
    let schema = a_schema(app).await;
    let a = asset_with_parent(app, "table", "orders", &schema).await;
    let b = asset_with_parent(app, "table", "orders_v2", &schema).await;
    for (parent, col) in [(&a, "id"), (&a, "amount"), (&b, "id"), (&b, "amount")] {
        asset_with_parent(app, "column", col, parent).await;
    }
    (a, b)
}

#[tokio::test]
async fn the_review_queue_lists_an_ambiguous_pair_and_confirming_it_merges() {
    let (app, _db, _connection_string) = test_app().await;
    let (a, b) = an_ambiguous_pair(&app).await;

    let (status, body) = send(&app, "POST", &format!("/assets/{b}/resolve"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "ambiguous", "{body}");

    let (status, body) = send(&app, "GET", "/resolution/queue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], json!(1), "{body}");
    let entry_id = body["data"][0]["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/resolution/queue/{entry_id}/confirm"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "existing");
    assert_eq!(body["entity"], a);

    // A confirmed pair is no longer pending.
    let (status, body) = send(&app, "GET", "/resolution/queue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], json!(0), "{body}");

    // Confirming twice is a 409, not a second merge.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/resolution/queue/{entry_id}/confirm"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn rejecting_a_review_entry_removes_it_from_the_pending_queue() {
    let (app, _db, _connection_string) = test_app().await;
    let (_a, b) = an_ambiguous_pair(&app).await;

    send(&app, "POST", &format!("/assets/{b}/resolve"), None).await;
    let (_, body) = send(&app, "GET", "/resolution/queue", None).await;
    let entry_id = body["data"][0]["id"].as_str().expect("an id").to_string();

    let (status, _) = send(
        &app,
        "POST",
        &format!("/resolution/queue/{entry_id}/reject"),
        Some(json!({ "reason": "different tables, coincidental column overlap" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = send(&app, "GET", "/resolution/queue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], json!(0), "{body}");

    // Explicitly asking for rejected entries still finds it — the decision
    // was recorded, not deleted. The reason is recorded alongside it: Epic
    // 42 decision 3 requires a rejection be auditable, which an evidence
    // trail with no reason is not.
    let (status, body) = send(&app, "GET", "/resolution/queue?status=rejected", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], json!(1), "{body}");
    assert_eq!(
        body["data"][0]["reason"],
        json!("different tables, coincidental column overlap"),
        "{body}"
    );
}

#[tokio::test]
async fn rejecting_without_a_reason_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;
    let (_a, b) = an_ambiguous_pair(&app).await;

    send(&app, "POST", &format!("/assets/{b}/resolve"), None).await;
    let (_, body) = send(&app, "GET", "/resolution/queue", None).await;
    let entry_id = body["data"][0]["id"].as_str().expect("an id").to_string();

    // No body at all.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/resolution/queue/{entry_id}/reject"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // A body with an empty reason is the same refusal, not a different one —
    // an empty string teaches the matcher nothing, same as no reason at all.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/resolution/queue/{entry_id}/reject"),
        Some(json!({ "reason": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Still pending — a refused reject must not have decided the entry.
    let (status, body) = send(&app, "GET", "/resolution/queue", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], json!(1), "{body}");
}

#[tokio::test]
async fn bulk_decide_reports_each_id_independently() {
    let (app, _db, _connection_string) = test_app().await;
    let (_a, b) = an_ambiguous_pair(&app).await;

    send(&app, "POST", &format!("/assets/{b}/resolve"), None).await;
    let (_, body) = send(&app, "GET", "/resolution/queue", None).await;
    let entry_id = body["data"][0]["id"].as_str().expect("an id").to_string();
    let unknown_id = uuid::Uuid::new_v4().to_string();

    let (status, body) = send(
        &app,
        "POST",
        "/resolution/queue/bulk",
        Some(json!({
            "ids": [entry_id, unknown_id],
            "decision": "reject",
            "reason": "bulk review: below the confidence bar this batch used",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let results = body["data"].as_array().expect("data array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["id"], entry_id);
    assert_eq!(results[0]["ok"], json!(true), "{body}");
    assert_eq!(results[1]["id"], unknown_id);
    assert_eq!(results[1]["ok"], json!(false), "{body}");
}

#[tokio::test]
async fn bulk_reject_without_a_reason_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;
    let (_a, b) = an_ambiguous_pair(&app).await;

    send(&app, "POST", &format!("/assets/{b}/resolve"), None).await;
    let (_, body) = send(&app, "GET", "/resolution/queue", None).await;
    let entry_id = body["data"][0]["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        "/resolution/queue/bulk",
        Some(json!({ "ids": [entry_id], "decision": "reject" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn bulk_decide_with_no_ids_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/resolution/queue/bulk",
        Some(json!({ "ids": [], "decision": "reject" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn resolving_a_mention_finds_the_context_matching_entity_and_never_merges() {
    let (app, _db, _connection_string) = test_app().await;

    // Two same-named tables in different schemas — the plan's own
    // disambiguation scenario.
    let service = asset(&app, "svc").await;
    let database = asset_with_parent(&app, "database", "db", &service).await;
    let staging = asset_with_parent(&app, "schema", "staging", &database).await;
    let prod = asset_with_parent(&app, "schema", "prod", &database).await;
    let in_staging = asset_with_parent(&app, "table", "orders", &staging).await;
    let _in_prod = asset_with_parent(&app, "table", "orders", &prod).await;

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({
            "kind": "rationale",
            "content": "The orders table in staging has a data quality issue.",
            "confidence": 0.9,
            "links": [{ "relation": "about", "target": in_staging }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let memory_id = body["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/memories/{memory_id}/mentions"),
        Some(json!({
            "text": "orders",
            "context": "the orders table in staging"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["resolution"]["entity"], in_staging, "{body}");
    assert!(body["resolution"]["confidence"].as_f64().unwrap() > 0.5);
}

#[tokio::test]
async fn resolving_a_mention_with_no_match_returns_a_null_resolution() {
    let (app, _db, _connection_string) = test_app().await;
    let service = asset(&app, "svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({
            "kind": "rationale",
            "content": "unrelated",
            "confidence": 0.9,
            "links": [{ "relation": "about", "target": service }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let memory_id = body["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/memories/{memory_id}/mentions"),
        Some(json!({ "text": "zzqxw-nothing-matches", "context": "" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["resolution"], Value::Null, "{body}");
}

#[tokio::test]
async fn resolving_a_mention_with_empty_text_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;
    let service = asset(&app, "svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({
            "kind": "rationale",
            "content": "unrelated",
            "confidence": 0.9,
            "links": [{ "relation": "about", "target": service }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let memory_id = body["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/memories/{memory_id}/mentions"),
        Some(json!({ "text": "", "context": "" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
