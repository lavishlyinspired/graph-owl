//! Epic 22 at the wire.
//!
//! The domain tests prove the type rules exhaustively and without I/O; this
//! proves the three things they cannot — that uniqueness is scoped to the
//! entity type by a real index, that a value cannot reach storage unvalidated,
//! and that a definition holding values refuses to be deleted.

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

fn definition(name: &str, entity_type: &str, property_type: &str) -> Value {
    json!({
        "name": name,
        "entityType": entity_type,
        "propertyType": property_type,
    })
}

async fn define(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    send(app, "POST", "/custom-properties", Some(body)).await
}

// ── definitions ────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_property_can_be_defined_and_listed_for_its_type() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("costCenter", "service", "string")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body["id"].is_string(), "{body}");

    let (status, listed) = send(&app, "GET", "/custom-properties?entityType=service", None).await;
    assert_eq!(status, StatusCode::OK);
    let properties = listed.as_array().expect("an array");
    assert_eq!(properties.len(), 1, "{listed}");
    assert_eq!(properties[0]["name"], "costCenter");
}

/// **Decision 2 as a database constraint.** The same name on two entity types
/// is two different properties; a globally-scoped unique index would silently
/// forbid that, and nothing below this level would notice.
#[tokio::test]
async fn a_name_is_unique_per_entity_type_not_globally() {
    let (app, _db, _url) = test_app().await;

    let (first, _) = define(&app, definition("costCenter", "service", "string")).await;
    assert_eq!(first, StatusCode::CREATED);

    let (same_type, body) = define(&app, definition("costCenter", "service", "string")).await;
    assert_eq!(same_type, StatusCode::CONFLICT, "{body}");

    let (other_type, body) = define(&app, definition("costCenter", "table", "string")).await;
    assert_eq!(
        other_type,
        StatusCode::CREATED,
        "the same name on another type is a different property: {body}"
    );
}

/// A custom `description` would shadow the real field, and every reader would
/// then get one of two values depending on which layer answered.
#[tokio::test]
async fn a_name_colliding_with_a_built_in_field_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("description", "service", "string")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **The supported set is listed**, because decision 4 makes it closed on
/// purpose — a client told only "unsupported" has to go and find the docs.
#[tokio::test]
async fn an_unsupported_type_is_refused_and_the_supported_ones_are_named() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("where", "service", "geolocation")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(rendered.contains("string"), "{rendered}");
    assert!(rendered.contains("entityReference"), "{rendered}");
}

#[tokio::test]
async fn an_enum_without_values_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("tier", "service", "enum")).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "no value could ever satisfy it: {body}"
    );
}

#[tokio::test]
async fn an_unknown_entity_type_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("costCenter", "spaceship", "string")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ── values ─────────────────────────────────────────────────────────────────

async fn service_with(app: &axum::Router, name: &str, extension: Value) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name, "extension": extension })),
    )
    .await
}

#[tokio::test]
async fn a_defined_property_with_a_correct_value_round_trips() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;

    let (status, body) = service_with(&app, "orders", json!({ "costCenter": "CC-1234" })).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["extension"]["costCenter"], "CC-1234", "{body}");
}

/// **The failure this epic exists to prevent.** A bag accepted untyped is the
/// description field again, with extra steps — unsearchable, unvalidatable, and
/// impossible to report on.
#[tokio::test]
async fn an_undefined_property_name_is_refused_rather_than_stored() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = service_with(&app, "orders", json!({ "notDefined": "value" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_value_of_the_wrong_type_is_refused_and_both_types_are_named() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("retentionDays", "service", "integer")).await;

    let (status, body) = service_with(&app, "orders", json!({ "retentionDays": "ninety" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(rendered.contains("integer"), "{rendered}");
    assert!(rendered.contains("string"), "{rendered}");
}

/// **Definitions are per entity type**, so a property defined on `table` is
/// undefined on a service — and accepting it there would make the scoping
/// decorative.
#[tokio::test]
async fn a_property_defined_on_another_type_is_undefined_here() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "table", "string")).await;

    let (status, body) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// A constraint violation is a `value` error, not a `type` error: the fix is to
/// send a different *one*, not a different *kind*. A client that retried a
/// range violation by casting would loop.
#[tokio::test]
async fn a_constraint_violation_is_reported_as_a_value_error() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({
            "name": "tier",
            "entityType": "service",
            "propertyType": "enum",
            "constraints": { "values": ["gold", "silver"] },
        }),
    )
    .await;

    let (status, body) = service_with(&app, "orders", json!({ "tier": "bronze" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(
        rendered.contains("gold"),
        "the options are listed: {rendered}"
    );
    assert!(rendered.contains("\"value\""), "{rendered}");
}

/// **Every failure at once.** One fix per round trip is the cost this
/// codebase's accumulating validators exist to avoid.
#[tokio::test]
async fn every_bad_value_in_one_write_is_reported_together() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    define(&app, definition("retentionDays", "service", "integer")).await;

    let (status, body) = service_with(
        &app,
        "orders",
        json!({ "costCenter": 7, "retentionDays": "ninety", "unknown": true }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let errors = body["errors"].as_array().expect("field errors");
    assert_eq!(errors.len(), 3, "{body}");
}

/// An asset carrying no organization-defined values is the normal case, and it
/// must not have to say so.
#[tokio::test]
async fn an_asset_without_an_extension_is_created_normally() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "orders" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(
        body.get("extension").is_none(),
        "an empty bag is absent, not `{{}}` on every asset: {body}"
    );
}

// ── deleting a definition ──────────────────────────────────────────────────

#[tokio::test]
async fn an_unused_definition_can_be_deleted() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id");

    let (status, _) = send(&app, "DELETE", &format!("/custom-properties/{id}"), None).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// **Decision 5: removing a definition does not silently delete data**, and the
/// `409` reports the count — "values exist" tells an operator nothing about
/// whether this is a five-minute cleanup or a quarter's work.
#[tokio::test]
async fn a_definition_holding_values_refuses_to_be_deleted_and_reports_the_count() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    service_with(&app, "payments", json!({ "costCenter": "CC-2" })).await;

    let (status, body) = send(&app, "DELETE", &format!("/custom-properties/{id}"), None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains('2'),
        "the count is the actionable detail: {body}"
    );
}

#[tokio::test]
async fn deleting_a_definition_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/custom-properties/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
