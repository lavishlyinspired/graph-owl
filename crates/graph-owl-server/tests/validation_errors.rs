mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;

async fn post_tables(app: axum::Router, body: String) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/tables")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .expect("request should build"),
    )
    .await
    .expect("request should be handled")
}

/// The point of the slice: a client fixing a bad request sees every mistake in
/// one round trip, not one per retry.
#[tokio::test]
async fn two_independent_violations_are_both_reported() {
    let (app, _container) = test_app().await;

    let response = post_tables(
        app,
        json!({ "fullyQualifiedName": "" }).to_string(), // name missing AND fqn empty
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(
        body["type"],
        "https://graph-owl.dev/errors/validation-failed"
    );

    let errors = body["errors"]
        .as_array()
        .expect("a validation problem carries an errors array");

    // The mutator this test exists to kill: short-circuiting on the first
    // violation passes every "is it a 400" assertion and still forces the
    // client into one round trip per mistake.
    assert_eq!(
        errors.len(),
        2,
        "both violations must be reported, got {errors:?}"
    );

    let fields: Vec<&str> = errors
        .iter()
        .map(|e| e["field"].as_str().expect("each error names a field"))
        .collect();
    assert!(fields.contains(&"name"), "got {fields:?}");
    assert!(fields.contains(&"fullyQualifiedName"), "got {fields:?}");
}

#[tokio::test]
async fn each_error_entry_carries_field_code_and_detail() {
    let (app, _container) = test_app().await;

    let response = post_tables(app, json!({ "fullyQualifiedName": "" }).to_string()).await;
    let body = json_body(response).await;
    let errors = body["errors"].as_array().expect("errors array");

    for error in errors {
        assert!(
            error["field"].as_str().is_some_and(|s| !s.is_empty()),
            "field must be present and non-empty: {error:?}"
        );
        assert!(
            error["code"].as_str().is_some_and(|s| !s.is_empty()),
            "code must be present and non-empty: {error:?}"
        );
        assert!(
            error["detail"].as_str().is_some_and(|s| !s.is_empty()),
            "detail must be present and non-empty: {error:?}"
        );
    }

    // Codes are machine-readable and must distinguish *why* a field failed —
    // a missing field and a blank one need different fixes.
    let codes: Vec<&str> = errors.iter().map(|e| e["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"required"), "got {codes:?}");
    assert!(codes.contains(&"empty"), "got {codes:?}");
}

#[tokio::test]
async fn a_wrong_typed_field_is_a_validation_error_not_a_parse_failure() {
    let (app, _container) = test_app().await;

    let response = post_tables(
        app,
        json!({ "name": 42, "fullyQualifiedName": "warehouse.public.orders" }).to_string(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(
        body["type"], "https://graph-owl.dev/errors/validation-failed",
        "a well-formed document with a wrong-typed field is a validation \
         failure the client can act on field-by-field, not an opaque parse error"
    );
    let errors = body["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["field"], "name");
    assert_eq!(errors[0]["code"], "type");
}

#[tokio::test]
async fn syntactically_broken_json_stays_a_malformed_body() {
    let (app, _container) = test_app().await;

    let response = post_tables(app, "{ not json".to_string()).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(
        body["type"], "https://graph-owl.dev/errors/malformed-body",
        "a document that is not JSON at all has no fields to report against"
    );
    assert!(
        body["errors"].is_null(),
        "malformed bodies carry no per-field errors"
    );
}

#[tokio::test]
async fn a_valid_body_still_succeeds() {
    let (app, _container) = test_app().await;

    let response = post_tables(
        app,
        json!({
            "name": "orders",
            "fullyQualifiedName": "warehouse.public.orders"
        })
        .to_string(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
}
