//! Epic 18 Slice D at the wire.
//!
//! The facade tests (`graph-owl-api`) prove the processing pipeline and
//! replay/purge semantics. What can only be checked here is the HTTP
//! surface: that a delivery is actually processed asynchronously after the
//! response goes back, admin gating, and the dead-letter/replay/purge wire
//! shapes.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
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

async fn register_mapping(app: &axum::Router, name: &str) {
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
}

fn hmac_sign(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    let hex: String = bytes.iter().fold(String::new(), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    });
    format!("sha256={hex}")
}

async fn register_endpoint(app: &axum::Router, path: &str, mapping: &str, secret: &str) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": path,
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": mapping,
            "secret": secret,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

async fn deliver(app: &axum::Router, path: &str, secret: &str, body: &'static [u8]) -> Value {
    let signature = hmac_sign(secret, body);
    let request = Request::builder()
        .method("POST")
        .uri(format!("/webhooks/receive/{path}"))
        .header("X-Signature", signature)
        .body(Body::from(body))
        .expect("request should build");
    let response = app.clone().oneshot(request).await.expect("handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

/// Poll until the event settles out of `Received` — the pipeline runs in a
/// detached task after the response, so a test has to wait on the state,
/// not assume it happened by the time the HTTP call returns.
async fn settled(app: &axum::Router, id: &str) -> Value {
    for _ in 0..200 {
        let (status, event) = send(app, "GET", &format!("/webhooks/events/{id}"), None).await;
        assert_eq!(status, StatusCode::OK, "{event}");
        if matches!(event["state"].as_str(), Some("applied" | "failed")) {
            return event;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("event {id} did not settle out of Received in time");
}

#[tokio::test]
async fn a_delivery_is_processed_asynchronously_after_the_response() {
    let (app, _database, _) = test_app().await;
    register_mapping(&app, "dbt-run-completed").await;
    register_endpoint(&app, "dbt", "dbt-run-completed", "shared-secret").await;

    let recorded = deliver(
        &app,
        "dbt",
        "shared-secret",
        br#"{"kind":"service","tableName":"orders"}"#,
    )
    .await;
    let id = recorded["id"].as_str().expect("id");

    let event = settled(&app, id).await;
    assert_eq!(event["state"], "applied", "{event}");
}

#[tokio::test]
async fn a_bad_payload_is_dead_lettered_and_visible_in_the_queue() {
    let (app, _database, _) = test_app().await;
    register_mapping(&app, "dbt-run-completed").await;
    let endpoint = register_endpoint(&app, "dbt", "dbt-run-completed", "shared-secret").await;
    let endpoint_id = endpoint["id"].as_str().expect("id").to_string();

    let recorded = deliver(&app, "dbt", "shared-secret", br#"{"kind":"table"}"#).await;
    let id = recorded["id"].as_str().expect("id");
    let event = settled(&app, id).await;
    assert_eq!(event["state"], "failed", "{event}");

    let (status, dlq) = send(&app, "GET", "/webhooks/dead-letters", None).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<_> = dlq["data"]
        .as_array()
        .expect("array")
        .iter()
        .map(|e| e["id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(ids.contains(&id.to_string()), "{dlq}");

    let (status, filtered) = send(
        &app,
        "GET",
        &format!("/webhooks/dead-letters?endpoint={endpoint_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let filtered_ids: Vec<_> = filtered["data"]
        .as_array()
        .expect("array")
        .iter()
        .map(|e| e["id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(filtered_ids.contains(&id.to_string()), "{filtered}");
}

#[tokio::test]
async fn replaying_after_a_mapping_fix_applies_the_dead_lettered_event() {
    let (app, _database, _) = test_app().await;
    register_mapping(&app, "dbt-run-completed").await;
    let endpoint = register_endpoint(&app, "dbt", "dbt-run-completed", "shared-secret").await;
    let endpoint_id = endpoint["id"].as_str().expect("id").to_string();

    let recorded = deliver(&app, "dbt", "shared-secret", br#"{"kind":"service"}"#).await;
    let id = recorded["id"].as_str().expect("id").to_string();
    let event = settled(&app, &id).await;
    assert_eq!(event["state"], "failed", "{event}");

    let (status, body) = send(
        &app,
        "POST",
        "/webhooks/mappings",
        Some(json!({
            "name": "dbt-run-completed",
            "kind": path_expr("/kind"),
            "entityName": {"kind": "literal", "value": "orders"},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let since = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let until = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let (status, summary) = send(
        &app,
        "POST",
        "/webhooks/replay",
        Some(json!({ "endpoint": endpoint_id, "since": since, "until": until })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["applied"], 1, "{summary}");

    let (status, event) = send(&app, "GET", &format!("/webhooks/events/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(event["state"], "applied", "{event}");
}

#[tokio::test]
async fn purging_reports_how_many_were_removed() {
    let (app, _database, _) = test_app().await;
    register_mapping(&app, "dbt-run-completed").await;
    register_endpoint(&app, "dbt", "dbt-run-completed", "shared-secret").await;

    let recorded = deliver(&app, "dbt", "shared-secret", br#"{"kind":"table"}"#).await;
    let id = recorded["id"].as_str().expect("id");
    settled(&app, id).await;

    // Nothing older than a year — the just-created failure survives.
    let (status, result) = send(
        &app,
        "DELETE",
        "/webhooks/dead-letters?olderThanDays=365",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["purged"], 0, "{result}");

    // Everything: the failure is old enough to have been "received" before
    // right now, so a 0-day cutoff catches it.
    let (status, result) = send(
        &app,
        "DELETE",
        "/webhooks/dead-letters?olderThanDays=0",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["purged"], 1, "{result}");
}
