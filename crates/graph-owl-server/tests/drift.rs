//! Epic 20 x Epic 42 Slice D at the wire — drift, made HTTP-queryable.

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

fn report_for(fqn: &str) -> Value {
    json!({
        "items": [{
            "fullyQualifiedName": fqn,
            "field": "description",
            "kind": "unapplied",
            "liveValue": "old description",
            "declaredValue": "new description",
        }]
    })
}

#[tokio::test]
async fn pushing_a_report_creates_a_pending_item_visible_in_the_queue() {
    let (app, _db, _connection_string) = test_app().await;
    asset(&app, "orders").await;

    let (status, body) = send(&app, "POST", "/drift/reports", Some(report_for("orders"))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body[0]["status"], "pending");
    assert_eq!(body[0]["fullyQualifiedName"], "orders");

    let (status, body) = send(&app, "GET", "/drift", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], json!(1), "{body}");
    assert_eq!(body["data"][0]["field"], "description");
}

#[tokio::test]
async fn pushing_against_an_unknown_asset_is_a_404() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/drift/reports",
        Some(report_for("no-such-asset")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn pushing_an_empty_report_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;

    let (status, body) = send(&app, "POST", "/drift/reports", Some(json!({ "items": [] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn applying_a_pending_item_writes_the_declared_value_and_marks_it_applied() {
    let (app, _db, _connection_string) = test_app().await;
    let id = asset(&app, "orders").await;
    let (_, pushed) = send(&app, "POST", "/drift/reports", Some(report_for("orders"))).await;
    let item_id = pushed[0]["id"].as_str().expect("an id").to_string();

    let (status, body) = send(&app, "POST", &format!("/drift/{item_id}/apply"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "applied");

    let (status, asset_body) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{asset_body}");
    assert_eq!(asset_body["description"], "new description");

    let (_, list_body) = send(&app, "GET", "/drift", None).await;
    assert_eq!(
        list_body["total"],
        json!(0),
        "an applied item must not appear in the default pending listing: {list_body}"
    );
}

#[tokio::test]
async fn ignoring_without_a_reason_is_a_400() {
    let (app, _db, _connection_string) = test_app().await;
    asset(&app, "orders").await;
    let (_, pushed) = send(&app, "POST", "/drift/reports", Some(report_for("orders"))).await;
    let item_id = pushed[0]["id"].as_str().expect("an id").to_string();

    let (status, body) = send(&app, "POST", &format!("/drift/{item_id}/ignore"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn ignoring_a_pending_item_records_the_reason() {
    let (app, _db, _connection_string) = test_app().await;
    asset(&app, "orders").await;
    let (_, pushed) = send(&app, "POST", "/drift/reports", Some(report_for("orders"))).await;
    let item_id = pushed[0]["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/drift/{item_id}/ignore"),
        Some(json!({ "reason": "expected seasonal change, not drift" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ignored");
    assert_eq!(body["reason"], "expected seasonal change, not drift");
}

#[tokio::test]
async fn deciding_an_already_decided_item_is_a_409() {
    let (app, _db, _connection_string) = test_app().await;
    asset(&app, "orders").await;
    let (_, pushed) = send(&app, "POST", "/drift/reports", Some(report_for("orders"))).await;
    let item_id = pushed[0]["id"].as_str().expect("an id").to_string();

    send(&app, "POST", &format!("/drift/{item_id}/apply"), None).await;
    let (status, body) = send(
        &app,
        "POST",
        &format!("/drift/{item_id}/ignore"),
        Some(json!({ "reason": "too late, already applied" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
