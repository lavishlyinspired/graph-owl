//! `GET /assets/{id}?fields=...` — `00d-api-conventions.md`'s field
//! selection, documented since that plan but never wired to any handler
//! until Epic 37a Slice B needed to benchmark it and found nothing there.
//!
//! **No new storage or facade code.** `labels_on`, `lineage_graph`, and
//! `list_children` already existed, each backing its own dedicated route;
//! this is purely the HTTP-layer composition `00d` describes — one request
//! instead of the three or four a caller previously had to assemble by
//! hand. `owners` is accepted but always a no-op: `Asset.owners` is already
//! unconditionally serialized (never omitted, by deliberate design — see
//! its own doc comment), so there is nothing to opt into.

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

/// A real `table`, reached through the only legal chain
/// (`AssetKind::parent_kind`): service → database → schema → table. A
/// column's only legal parent is a table, never a service directly.
async fn table(app: &axum::Router, service: &str) -> Value {
    let svc = asset(app, "service", service, None).await;
    let db = asset(app, "database", "db", Some(svc["id"].as_str().unwrap())).await;
    let schema = asset(app, "schema", "public", Some(db["id"].as_str().unwrap())).await;
    asset(app, "table", "orders", Some(schema["id"].as_str().unwrap())).await
}

#[tokio::test]
async fn without_fields_the_response_is_unchanged() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "service", "orders", None).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{}", orders["id"].as_str().unwrap()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.get("tags").is_none(),
        "no fields requested must mean no extra keys: {body}"
    );
    assert!(body.get("lineage").is_none(), "{body}");
    assert!(body.get("columns").is_none(), "{body}");
    // owners is unconditional either way, per its own doc comment.
    assert!(body.get("owners").is_some(), "{body}");
}

#[tokio::test]
async fn fields_tags_composes_the_labels_already_applied() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "service", "orders", None).await;
    let id = orders["id"].as_str().unwrap().to_string();
    let fqn = orders["fullyQualifiedName"].as_str().unwrap().to_string();

    let (status, classification) = send(
        &app,
        "POST",
        "/classifications",
        Some(json!({ "name": "Tier", "mutuallyExclusive": false })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{classification}");
    let classification_id = classification["id"].as_str().unwrap();

    let (status, tag) = send(
        &app,
        "POST",
        &format!("/classifications/{classification_id}/tags"),
        Some(json!({ "name": "Gold" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{tag}");
    let tag_fqn = tag["fullyQualifiedName"].as_str().unwrap();

    let (status, applied) = send(
        &app,
        "POST",
        &format!("/labels/{fqn}"),
        Some(json!({ "tagFqn": tag_fqn })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{applied}");

    let (status, body) = send(&app, "GET", &format!("/assets/{id}?fields=tags"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let tags = body["tags"].as_array().expect("a tags array");
    assert_eq!(tags.len(), 1, "{body}");
    assert_eq!(tags[0]["tagFqn"], tag_fqn, "{body}");
}

#[tokio::test]
async fn fields_lineage_composes_one_hop_each_way() {
    let (app, _db, _) = test_app().await;
    let upstream = table(&app, "raw").await;
    let target = table(&app, "mart").await;

    let (status, edge) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({
            "fromAssetId": upstream["id"],
            "toAssetId": target["id"],
            "relationship": "feeds",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{edge}");

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{}?fields=lineage", target["id"].as_str().unwrap()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let nodes = body["lineage"]["nodes"].as_array().expect("nodes");
    assert!(
        nodes.iter().any(|n| n["id"] == upstream["id"]),
        "the upstream asset should be one hop away: {body}"
    );
}

#[tokio::test]
async fn fields_columns_composes_children() {
    let (app, _db, _) = test_app().await;
    let orders = table(&app, "warehouse").await;
    let table_id = orders["id"].as_str().unwrap();
    let column = asset(&app, "column", "order_id", Some(table_id)).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{table_id}?fields=columns"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let columns = body["columns"].as_array().expect("a columns array");
    assert_eq!(columns.len(), 1, "{body}");
    assert_eq!(columns[0]["id"], column["id"], "{body}");
}

#[tokio::test]
async fn several_fields_compose_in_one_request() {
    let (app, _db, _) = test_app().await;
    let orders = table(&app, "warehouse").await;
    let table_id = orders["id"].as_str().unwrap();
    asset(&app, "column", "order_id", Some(table_id)).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{table_id}?fields=tags,columns"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["tags"].as_array().is_some(), "{body}");
    assert!(body["columns"].as_array().is_some(), "{body}");
}

#[tokio::test]
async fn owners_in_fields_is_a_harmless_no_op() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "service", "orders", None).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{}?fields=owners", orders["id"].as_str().unwrap()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["owners"].as_array().is_some(), "{body}");
}

#[tokio::test]
async fn an_unrecognized_field_is_a_400_naming_it() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "service", "orders", None).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!(
            "/assets/{}?fields=owners,nonsense",
            orders["id"].as_str().unwrap()
        ),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        format!("{body}").contains("nonsense"),
        "the error should name the offending field: {body}"
    );
}
