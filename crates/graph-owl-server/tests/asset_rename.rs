//! Phase 3 item 3.3 at the wire — cascade-on-rename for `Asset`. Named
//! explicitly by Epic 34's own write-up as unbuilt machinery its dependency
//! line assumed existed. The same problem Epic 23 already solved for
//! domains (`domains.rs`'s own `renaming_a_domain_moves_its_descendants_paths_too`),
//! solved here for the entity hierarchy instead.

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

async fn asset(app: &axum::Router, kind: &str, name: &str, parent_id: Option<&str>) -> Value {
    let mut body = json!({ "kind": kind, "name": name });
    if let Some(parent_id) = parent_id {
        body["parentId"] = json!(parent_id);
    }
    let (status, created) = send(app, "POST", "/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created
}

#[tokio::test]
async fn renaming_a_root_asset_moves_its_own_fqn() {
    let (app, _db, _url) = test_app().await;
    let service = asset(&app, "service", "warehouse", None).await;
    let id = service["id"].as_str().expect("id");

    let (status, renamed) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "name": "lakehouse" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["name"], "lakehouse", "{renamed}");
    assert_eq!(renamed["fullyQualifiedName"], "lakehouse", "{renamed}");
}

/// **The RED case.** A rename that moved only its own path would leave every
/// descendant claiming to sit under a name that no longer exists — the same
/// reasoning `domains.rs`'s own cascade test already proved for domains.
#[tokio::test]
async fn renaming_an_asset_moves_its_descendants_fqns_too() {
    let (app, _db, _url) = test_app().await;
    let service = asset(&app, "service", "warehouse", None).await;
    let service_id = service["id"].as_str().expect("id");
    let database = asset(&app, "database", "sales", Some(service_id)).await;
    let database_id = database["id"].as_str().expect("id");
    let schema = asset(&app, "schema", "public", Some(database_id)).await;
    let schema_id = schema["id"].as_str().expect("id");

    let (status, renamed) = send(
        &app,
        "PATCH",
        &format!("/assets/{service_id}"),
        Some(json!({ "name": "lakehouse" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["fullyQualifiedName"], "lakehouse", "{renamed}");

    let (_, moved_database) = send(&app, "GET", &format!("/assets/{database_id}"), None).await;
    assert_eq!(
        moved_database["fullyQualifiedName"], "lakehouse.sales",
        "{moved_database}"
    );

    let (_, moved_schema) = send(&app, "GET", &format!("/assets/{schema_id}"), None).await;
    assert_eq!(
        moved_schema["fullyQualifiedName"], "lakehouse.sales.public",
        "the whole subtree moves, not only the direct child: {moved_schema}"
    );
}

/// A sibling with an already-taken name is a `409`, the same conflict a
/// create would report — renaming does not get a quieter failure mode than
/// creating does.
#[tokio::test]
async fn renaming_to_a_taken_sibling_name_is_a_409() {
    let (app, _db, _url) = test_app().await;
    asset(&app, "service", "warehouse", None).await;
    let other = asset(&app, "service", "lakehouse", None).await;
    let other_id = other["id"].as_str().expect("id");

    let (status, conflict) = send(
        &app,
        "PATCH",
        &format!("/assets/{other_id}"),
        Some(json!({ "name": "warehouse" })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
}

/// A name containing the FQN separator would make the result ambiguous —
/// the same rule `fqn::derive` enforces at creation, now enforced on rename
/// too rather than only at the one call site that happened to check first.
#[tokio::test]
async fn renaming_to_a_name_containing_the_separator_is_a_400() {
    let (app, _db, _url) = test_app().await;
    let service = asset(&app, "service", "warehouse", None).await;
    let id = service["id"].as_str().expect("id");

    let (status, rejected) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "name": "sales.public" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
}

/// Renaming to the asset's own current name is a no-op version-wise, the
/// same "no version, no history row" rule every other unchanged patch
/// already follows.
#[tokio::test]
async fn renaming_to_the_same_name_does_not_bump_the_version() {
    let (app, _db, _url) = test_app().await;
    let service = asset(&app, "service", "warehouse", None).await;
    let id = service["id"].as_str().expect("id");
    let before_version = service["version"].clone();

    let (status, unchanged) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "name": "warehouse" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged["version"], before_version, "{unchanged}");
}
