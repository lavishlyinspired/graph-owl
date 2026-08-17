//! `GET /search` at the wire — Plan 122a A1.
//!
//! `search_merge` (`graph-owl-server`'s own `--lib` unit tests) proves the
//! normalization logic against all three source types with no database.
//! What only an HTTP test can see: that the handler is really wired to the
//! live `Catalog` — real Postgres-backed asset and glossary search, not
//! hand-built Rust values.

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
async fn the_search_finds_a_real_asset_by_name() {
    let (app, _db, _url) = test_app().await;

    let (status, asset) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "search-fixture-warehouse" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{asset}");

    let (status, body) = send(&app, "GET", "/search?q=search-fixture-warehouse", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let results = body.as_array().expect("an array");
    assert!(
        results
            .iter()
            .any(|r| r["kind"] == "asset" && r["label"] == "search-fixture-warehouse"),
        "{body}"
    );
}

#[tokio::test]
async fn a_query_matching_nothing_is_an_empty_list_not_an_error() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "GET",
        "/search?q=nothing-will-ever-match-this-exact-string",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().expect("an array").len(), 0);
}
