//! Epic 18 Slice C at the wire.
//!
//! The facade tests (`graph-owl-api`) prove the mapping engine and the
//! shape-rejection reuse. What can only be checked here is the HTTP
//! surface: admin gating, the wire shapes of `Expression`/`Mapping`, and
//! that a dry run's body is the sample payload itself, not a wrapper.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, test_app_with_secret};
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "mapping-test-signing-secret";

fn token(subject: &str) -> String {
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        name: &'a str,
        exp: usize,
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: subject,
            name: subject,
            exp: 4_102_444_800, // year 2100
        },
        &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .expect("token should encode")
}

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
    let response = app.clone().oneshot(request).await.expect("handled");
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

fn path_expr(pointer: &str) -> Value {
    json!({"kind": "path", "pointer": pointer})
}

async fn register(app: &axum::Router, name: &str) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/webhooks/mappings",
        Some(json!({
            "name": name,
            "kind": path_expr("/kind"),
            "entityName": path_expr("/tableName"),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

#[tokio::test]
async fn registering_a_mapping_returns_version_one() {
    let (app, _database, _) = test_app().await;

    let created = register(&app, "dbt-run-completed").await;
    assert_eq!(created["version"], 1, "{created}");
    assert_eq!(created["name"], "dbt-run-completed");
}

#[tokio::test]
async fn registering_the_same_name_twice_adds_a_version() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt-run-completed").await;
    let second = register(&app, "dbt-run-completed").await;

    assert_eq!(second["version"], 2, "{second}");

    let (status, latest) = send(&app, "GET", "/webhooks/mappings/dbt-run-completed", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest["version"], 2);

    let (status, history) = send(
        &app,
        "GET",
        "/webhooks/mappings/dbt-run-completed/versions",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let versions = history.as_array().expect("array");
    assert_eq!(versions.len(), 2, "{history}");
}

#[tokio::test]
async fn an_unregistered_mapping_name_is_not_found() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(&app, "GET", "/webhooks/mappings/no-such-mapping", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// **The dry-run body is the sample payload itself**, unwrapped — proving
/// the wire contract matches what the plan's "sample payload can be
/// dry-run" criterion actually means: testing exactly what a sender would
/// transmit, not a request DTO around it.
#[tokio::test]
async fn a_dry_run_reports_the_draft_it_would_produce() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt-run-completed").await;

    let (status, outcome) = send(
        &app,
        "POST",
        "/webhooks/mappings/dbt-run-completed/dry-run",
        Some(json!({"kind": "table", "tableName": "orders"})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["outcome"], "draft");
    assert_eq!(outcome["kind"], "table");
    assert_eq!(outcome["name"], "orders");
}

#[tokio::test]
async fn a_dry_run_names_a_missing_required_field() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt-run-completed").await;

    let (status, outcome) = send(
        &app,
        "POST",
        "/webhooks/mappings/dbt-run-completed/dry-run",
        Some(json!({"kind": "table"})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert_eq!(outcome["outcome"], "missingField");
    assert_eq!(outcome["field"], "name");
}

#[tokio::test]
async fn a_dry_run_against_an_unregistered_mapping_is_not_found() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/webhooks/mappings/no-such-mapping/dry-run",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_blank_mapping_name_is_refused() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/webhooks/mappings",
        Some(json!({
            "name": "",
            "kind": path_expr("/kind"),
            "entityName": path_expr("/tableName"),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Admin-only, same reasoning as webhook endpoints: a mapping decides how
/// external payloads become catalog entities.
#[tokio::test]
async fn a_non_admin_cannot_manage_mappings() {
    let (app, _database, _) = test_app_with_secret(SECRET).await;

    let register_request = Request::builder()
        .method("POST")
        .uri("/webhooks/mappings")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::from(
            json!({
                "name": "dbt-run-completed",
                "kind": path_expr("/kind"),
                "entityName": path_expr("/tableName"),
            })
            .to_string(),
        ))
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(register_request)
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let get_request = Request::builder()
        .method("GET")
        .uri("/webhooks/mappings/dbt-run-completed")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::empty())
        .expect("request should build");
    assert_eq!(
        app.clone()
            .oneshot(get_request)
            .await
            .expect("handled")
            .status(),
        StatusCode::NOT_FOUND
    );

    let versions_request = Request::builder()
        .method("GET")
        .uri("/webhooks/mappings/dbt-run-completed/versions")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::empty())
        .expect("request should build");
    assert_eq!(
        app.clone()
            .oneshot(versions_request)
            .await
            .expect("handled")
            .status(),
        StatusCode::NOT_FOUND
    );

    let dry_run_request = Request::builder()
        .method("POST")
        .uri("/webhooks/mappings/dbt-run-completed/dry-run")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::from(json!({}).to_string()))
        .expect("request should build");
    assert_eq!(
        app.clone()
            .oneshot(dry_run_request)
            .await
            .expect("handled")
            .status(),
        StatusCode::NOT_FOUND
    );
}
