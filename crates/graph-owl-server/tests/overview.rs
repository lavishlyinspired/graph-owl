//! `GET /overview`'s new `health` field at the wire — Plan 122a A2.
//!
//! `health_pct` (`graph-owl-api`'s own `--lib` unit tests) proves the
//! percentage arithmetic with no database. What only an HTTP test can see:
//! that `coverage_pct` and `governance_pct` are wired to real Postgres
//! counts — a described/undescribed asset and an owned/unowned asset,
//! seeded through the real write paths, not hand-built numbers.

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

#[tokio::test]
async fn coverage_and_governance_reflect_a_real_mixed_estate() {
    let (app, _db, _url) = test_app().await;

    // One documented, owned asset.
    let (status, described_owned) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({
            "kind": "service",
            "name": "overview-fixture-documented-owned",
            "description": "a real description",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{described_owned}");
    let owned_id = described_owned["id"].as_str().expect("an id");
    let (status, owner_body) = send(
        &app,
        "PUT",
        &format!("/assets/{owned_id}/owners"),
        Some(json!({ "owners": [{ "id": "system", "kind": "user" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{owner_body}");

    // One undocumented, unowned asset — the gap in both metrics.
    let (status, gap_asset) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "overview-fixture-undocumented-unowned" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{gap_asset}");

    let (status, body) = send(&app, "GET", "/overview", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let coverage = body["health"]["coveragePct"].as_f64().expect("a number");
    let governance = body["health"]["governancePct"].as_f64().expect("a number");

    // Exactly one gap in two assets each — the real fixture, not an
    // invented number. Both percentages must reflect it, not just exist.
    assert!(
        (0.0..100.0).contains(&coverage),
        "coverage should show a real gap, got {coverage}"
    );
    assert!(
        (0.0..100.0).contains(&governance),
        "governance should show a real gap, got {governance}"
    );
}

#[tokio::test]
async fn a_fully_documented_fully_owned_estate_reports_full_health() {
    let (app, _db, _url) = test_app().await;

    let (status, asset) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({
            "kind": "service",
            "name": "overview-fixture-fully-healthy",
            "description": "complete",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{asset}");
    let id = asset["id"].as_str().expect("an id");
    let (status, owner_body) = send(
        &app,
        "PUT",
        &format!("/assets/{id}/owners"),
        Some(json!({ "owners": [{ "id": "system", "kind": "user" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{owner_body}");

    let (status, body) = send(&app, "GET", "/overview", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert!(
        (body["health"]["coveragePct"].as_f64().unwrap() - 100.0).abs() < 1e-9,
        "{body}"
    );
    assert!(
        (body["health"]["governancePct"].as_f64().unwrap() - 100.0).abs() < 1e-9,
        "{body}"
    );
}
