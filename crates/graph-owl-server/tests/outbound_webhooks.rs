//! Epic 14 Slice F (decision 4.2) at the wire.
//!
//! The facade tests (`graph-owl-api`) prove `OutboundWebhookSink::emit`
//! actually enqueues a delivery for a matching subscription. What can only
//! be checked here is the HTTP boundary: that `POST /admin/outbound-webhooks`
//! reaches that registration, that the secret never comes back in a
//! response, and that the route is admin-gated the same way every other
//! credential-holding registration surface in this crate is.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app, test_app_with_secret};
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "outbound-webhook-test-signing-secret";

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
    (status, json_body(response).await)
}

async fn register(app: &axum::Router, url: &str, secret: &str) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/admin/outbound-webhooks",
        Some(json!({
            "url": url,
            "eventTypes": ["created"],
            "secret": secret,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// **The registration secret never appears in any response** — same claim
/// `webhooks.rs` makes for inbound endpoints, at the equivalent outbound
/// surface.
#[tokio::test]
async fn a_registered_webhook_never_exposes_its_secret() {
    let (app, _database, _) = test_app().await;

    let created = register(&app, "https://example.com/hooks/a", "do-not-leak-me").await;
    assert!(created.get("secret").is_none(), "{created}");
    let created_string = created.to_string();
    assert!(
        !created_string.contains("do-not-leak-me"),
        "the secret came back in the response that stored it: {created_string}"
    );

    let (status, listed) = send(&app, "GET", "/admin/outbound-webhooks", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed_string = listed.to_string();
    assert!(
        !listed_string.contains("do-not-leak-me"),
        "the secret came back in a later read: {listed_string}"
    );
}

#[tokio::test]
async fn a_registered_webhook_round_trips_its_url_and_event_types() {
    let (app, _database, _) = test_app().await;
    let created = register(&app, "https://example.com/hooks/a", "secret").await;
    assert_eq!(created["url"], "https://example.com/hooks/a");
    assert_eq!(created["eventTypes"], json!(["created"]));
    assert_eq!(created["enabled"], true, "{created}");
}

#[tokio::test]
async fn enabled_defaults_to_true_when_omitted() {
    let (app, _database, _) = test_app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/admin/outbound-webhooks",
        Some(json!({
            "url": "https://example.com/hooks/a",
            "secret": "secret",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["enabled"], true, "{body}");
}

#[tokio::test]
async fn a_blank_url_is_refused() {
    let (app, _database, _) = test_app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/admin/outbound-webhooks",
        Some(json!({
            "url": "",
            "secret": "secret",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// A first registration has no existing key for `secret: None` to fall back
/// to — see `graph-owl-storage-postgres/tests/outbound_webhooks.rs`'s
/// `registering_a_new_webhook_without_a_secret_is_refused` for the storage
/// layer's own proof of this. This is the same refusal seen through the
/// HTTP surface.
#[tokio::test]
async fn registering_a_new_webhook_without_a_secret_is_refused() {
    let (app, _database, _) = test_app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/admin/outbound-webhooks",
        Some(json!({ "url": "https://example.com/hooks/a" })),
    )
    .await;
    assert!(
        status.is_server_error() || status.is_client_error(),
        "{status}: {body}"
    );
}

/// Admin-only, same reasoning as inbound webhook endpoints: an outbound
/// subscription holds a signing secret and decides where catalog events are
/// delivered.
#[tokio::test]
async fn a_non_admin_cannot_register_or_list_outbound_webhooks() {
    let (app, _database, _) = test_app_with_secret(SECRET).await;

    let register_request = Request::builder()
        .method("POST")
        .uri("/admin/outbound-webhooks")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::from(
            json!({
                "url": "https://example.com/hooks/a",
                "secret": "secret",
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

    let list_request = Request::builder()
        .method("GET")
        .uri("/admin/outbound-webhooks")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::empty())
        .expect("request should build");
    let response = app.clone().oneshot(list_request).await.expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
