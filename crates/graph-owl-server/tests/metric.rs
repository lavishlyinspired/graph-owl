//! Epic 24 Slice E at the wire: `Metric` CRUD on `/business-metrics` — a
//! deliberately different path from `/metrics`, which already names the
//! Prometheus exposition endpoint (Epic 10).

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

#[tokio::test]
async fn a_metric_can_be_created_and_fetched() {
    let (app, _db, _) = test_app().await;

    let (status, created) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "total recognised revenue" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["fullyQualifiedName"], "metric.revenue");
    assert_eq!(created["calculationType"], "simple");

    let (status, fetched) = send(
        &app,
        "GET",
        &format!("/business-metrics/{}", created["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{fetched}");
    assert_eq!(fetched["name"], "revenue");
}

// `/business-metrics` must not collide with `/metrics`, the Prometheus
// exposition endpoint — this asserts the two are genuinely different routes.
#[tokio::test]
async fn business_metrics_does_not_collide_with_prometheus_metrics() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(&app, "GET", "/metrics", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    // Prometheus exposition is plain text, not JSON — a `Value::String`
    // confirms this hit the scrape endpoint, not the metric list.
    assert!(body.is_string(), "{body:?}");
}

#[tokio::test]
async fn a_metric_needs_a_name() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "  ", "definition": "x" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_metric_needs_a_definition() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// **A source-less metric is permitted, and the gap is reported on the
// response** rather than refused.
#[tokio::test]
async fn a_metric_with_no_sources_is_permitted_and_flags_the_gap() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "total recognised revenue" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["gaps"],
        json!(["noSources", "noDefiningTerm", "noFormula"])
    );
}

#[tokio::test]
async fn a_source_that_is_not_a_known_asset_is_refused() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({
            "name": "revenue",
            "definition": "total recognised revenue",
            "sourceAssets": ["warehouse.public.orders"],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_known_source_asset_is_accepted() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    let (_, orders_asset) = send(&app, "GET", &format!("/assets/{orders}"), None).await;
    let fqn = orders_asset["fullyQualifiedName"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({
            "name": "revenue",
            "definition": "total recognised revenue",
            "sourceAssets": [fqn],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["sourceAssets"], json!([fqn]));
}

#[tokio::test]
async fn every_created_metric_is_listed() {
    let (app, _db, _) = test_app().await;
    send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d" })),
    )
    .await;
    send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "churn", "definition": "d" })),
    )
    .await;

    let (status, body) = send(&app, "GET", "/business-metrics", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().expect("a page").len(), 2);
}

#[tokio::test]
async fn a_metric_can_be_updated() {
    let (app, _db, _) = test_app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d" })),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/business-metrics/{id}"),
        Some(json!({ "definition": "revised", "unit": "INR" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["definition"], "revised");
    assert_eq!(body["unit"], "INR");
}

#[tokio::test]
async fn a_metric_can_be_deleted() {
    let (app, _db, _) = test_app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d" })),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (status, _) = send(&app, "DELETE", &format!("/business-metrics/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&app, "GET", &format!("/business-metrics/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn fetching_an_unknown_metric_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "GET",
        &format!("/business-metrics/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_metric_is_found_by_name_search() {
    let (app, _db, _) = test_app().await;
    send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d" })),
    )
    .await;

    let (status, body) = send(&app, "GET", "/business-metrics/search?q=revenue", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().expect("a list").len(), 1);
}

#[tokio::test]
async fn defined_by_must_reference_an_approved_term() {
    let (app, _db, _) = test_app().await;
    let (_, glossary) = send(
        &app,
        "POST",
        "/glossaries",
        Some(json!({ "name": "Finance" })),
    )
    .await;
    let (_, term) = send(
        &app,
        "POST",
        &format!("/glossaries/{}/terms", glossary["id"].as_str().unwrap()),
        Some(json!({ "name": "Revenue", "definition": "d" })),
    )
    .await;

    let (status, body) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({
            "name": "revenue",
            "definition": "d",
            "definedBy": term["id"],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ---- Slice F: metric lineage reconciliation at the wire ----

#[tokio::test]
async fn declaring_sources_updates_the_metric() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    let (_, orders_asset) = send(&app, "GET", &format!("/assets/{orders}"), None).await;
    let fqn = orders_asset["fullyQualifiedName"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, created) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d" })),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/business-metrics/{id}/sources"),
        Some(json!({ "sourceAssets": [fqn] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sourceAssets"], json!([fqn]));
    assert!(
        !body["gaps"]
            .as_array()
            .unwrap()
            .contains(&json!("noSources"))
    );
}

#[tokio::test]
async fn clearing_sources_removes_them_and_the_gap_returns() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    let (_, orders_asset) = send(&app, "GET", &format!("/assets/{orders}"), None).await;
    let fqn = orders_asset["fullyQualifiedName"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, created) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d", "sourceAssets": [fqn] })),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/business-metrics/{id}/sources"),
        Some(json!({ "sourceAssets": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["sourceAssets"].as_array().unwrap().is_empty());
    assert!(
        body["gaps"]
            .as_array()
            .unwrap()
            .contains(&json!("noSources"))
    );
}

#[tokio::test]
async fn declaring_an_unknown_source_is_refused() {
    let (app, _db, _) = test_app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/business-metrics",
        Some(json!({ "name": "revenue", "definition": "d" })),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/business-metrics/{id}/sources"),
        Some(json!({ "sourceAssets": ["warehouse.public.orders"] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn setting_sources_on_an_unknown_metric_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "PUT",
        &format!("/business-metrics/{}/sources", uuid::Uuid::new_v4()),
        Some(json!({ "sourceAssets": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
