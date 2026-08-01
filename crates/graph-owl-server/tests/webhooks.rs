//! Epic 18 Slice A at the wire.
//!
//! The facade tests (`graph-owl-api`) prove `Catalog::receive_webhook`
//! verifies before recording. What can only be checked here is the boundary
//! those tests assume: that the HTTP layer hands the *raw* bytes and the
//! *configured* header value through unchanged — no JSON parsing, no
//! re-serialization — because a body that never gets deserialized on a bad
//! signature is a claim about this crate's extractors, not about the facade.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app, test_app_with_secret};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use tower::ServiceExt;

const SECRET: &str = "webhook-test-signing-secret";

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

/// Raw bytes with an arbitrary header — the shape an outside sender uses,
/// never JSON on the way in.
async fn deliver(
    app: &axum::Router,
    path: &str,
    header: Option<(&str, &str)>,
    body: &'static [u8],
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/webhooks/receive/{path}"));
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    let request = request
        .body(Body::from(body))
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

async fn register(app: &axum::Router, path: &str, secret: &str) -> Value {
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
            "mapping": "dbt-run-completed",
            "eventFilter": ["run.completed"],
            "secret": secret,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// **The registration secret never appears in any response** — same claim
/// `connector_configs.rs` makes for connector credentials, at the same
/// surface.
#[tokio::test]
async fn a_registered_endpoint_never_exposes_its_secret() {
    let (app, _database, _) = test_app().await;

    let created = register(&app, "dbt", "shared-secret-do-not-leak").await;
    assert_eq!(created["hasSecret"], true, "{created}");
    assert!(created.get("secret").is_none(), "{created}");
    let created_string = created.to_string();
    assert!(
        !created_string.contains("shared-secret-do-not-leak"),
        "the secret came back in the response that stored it: {created_string}"
    );

    let (status, listed) = send(&app, "GET", "/webhooks/endpoints", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed_string = listed.to_string();
    assert!(
        !listed_string.contains("shared-secret-do-not-leak"),
        "the secret came back in a later read: {listed_string}"
    );
    assert!(
        listed_string.contains("\"path\":\"dbt\""),
        "{listed_string}"
    );
}

/// **A correctly signed delivery is recorded.** The important claim: the
/// signature is checked against exactly these bytes, sent with exactly this
/// header — not a re-serialized or re-parsed form of them.
#[tokio::test]
async fn a_correctly_signed_delivery_is_recorded() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", body);

    let (status, recorded) = deliver(&app, "dbt", Some(("X-Signature", &signature)), body).await;
    assert_eq!(status, StatusCode::CREATED, "{recorded}");
    assert_eq!(recorded["state"], "received", "{recorded}");
}

/// **A bad signature is `401`.** Slice A's central claim, at the wire: a
/// payload that would fail to parse as JSON must still 401 rather than 400 —
/// proving verification happens before any attempt to read the body as
/// anything but bytes.
#[tokio::test]
async fn a_bad_signature_is_unauthenticated_even_for_a_body_that_is_not_json() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "the-real-secret").await;

    let body: &'static [u8] = b"this is not json at all {{{";
    let wrong_signature = hmac_sign("a-guessed-secret", body);

    let (status, problem) =
        deliver(&app, "dbt", Some(("X-Signature", &wrong_signature)), body).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a malformed body must still fail verification before any parse is attempted: {problem}"
    );
}

/// A missing signature header is `401`, not a `400` for a missing field —
/// this is an authentication failure, not a validation one.
#[tokio::test]
async fn a_missing_signature_header_is_unauthenticated() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "secret").await;

    let (status, _) = deliver(&app, "dbt", None, br#"{"event":"run.completed"}"#).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A tampered body does not verify, even under the right header value for a
/// different payload — the signature covers these exact bytes.
#[tokio::test]
async fn a_tampered_body_is_unauthenticated() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "shared-secret").await;

    let signed_body: &'static [u8] = br#"{"amount":10}"#;
    let signature = hmac_sign("shared-secret", signed_body);
    let tampered: &'static [u8] = br#"{"amount":99999}"#;

    let (status, _) = deliver(&app, "dbt", Some(("X-Signature", &signature)), tampered).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// An unregistered path is `404` — there is nothing to verify against.
#[tokio::test]
async fn an_unregistered_path_is_not_found() {
    let (app, _database, _) = test_app().await;

    let (status, _) = deliver(
        &app,
        "no-such-endpoint",
        Some(("X-Signature", "sha256=whatever")),
        br#"{"event":"run.completed"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A disabled endpoint is `404`, not `403` — an existence signal is
/// unnecessary, pulled forward from Slice E's own reasoning.
#[tokio::test]
async fn a_disabled_endpoint_is_not_found() {
    let (app, _database, _) = test_app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": "dbt",
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "enabled": false,
            "secret": "shared-secret",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let signed_body: &'static [u8] = br#"{"event":"run.completed"}"#;
    let signature = hmac_sign("shared-secret", signed_body);
    let (status, _) = deliver(&app, "dbt", Some(("X-Signature", &signature)), signed_body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// One endpoint per path — a second registration under the same path would
/// make "which secret does this URL verify against" unanswerable.
#[tokio::test]
async fn registering_a_taken_path_is_a_conflict() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt", "secret-a").await;

    let (status, _) = send(
        &app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": "dbt",
            "source": "another-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "secret": "secret-b",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// An empty `path`/`source`/`mapping` is refused with `400` rather than
/// stored and left to fail later against the database's own `CHECK`
/// constraints.
#[tokio::test]
async fn a_blank_path_is_refused() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/webhooks/endpoints",
        Some(json!({
            "path": "",
            "source": "dbt-bot",
            "signatureScheme": {
                "kind": "hmacSha256",
                "header": "X-Signature",
                "prefix": "sha256=",
            },
            "mapping": "dbt-run-completed",
            "secret": "secret",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Omitting `enabled` registers a usable endpoint — a freshly registered one
/// has no prior state for a client to be preserving, so absent means "on".
#[tokio::test]
async fn enabled_defaults_to_true_when_omitted() {
    let (app, _database, _) = test_app().await;
    let created = register(&app, "dbt", "secret").await;
    assert_eq!(created["enabled"], true, "{created}");
}

/// Admin-only, same reasoning as connector configs: a webhook endpoint holds
/// a secret and decides what an external system may write into the catalog.
#[tokio::test]
async fn a_non_admin_cannot_register_or_list_webhook_endpoints() {
    let (app, _database, _) = test_app_with_secret(SECRET).await;

    let register_request = Request::builder()
        .method("POST")
        .uri("/webhooks/endpoints")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::from(
            json!({
                "path": "dbt",
                "source": "dbt-bot",
                "signatureScheme": {
                    "kind": "hmacSha256",
                    "header": "X-Signature",
                    "prefix": "sha256=",
                },
                "mapping": "dbt-run-completed",
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
        .uri("/webhooks/endpoints")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::empty())
        .expect("request should build");
    let response = app.clone().oneshot(list_request).await.expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
