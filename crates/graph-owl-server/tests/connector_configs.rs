//! Epic 41 Slice F: a credential that never comes back.
//!
//! The facade tests prove the type cannot carry a secret. This proves the
//! *HTTP surface* does not either — which is a different claim, because a
//! handler can serialise anything it likes and the plan's first RED is about
//! what reaches a client.

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
) -> (StatusCode, String) {
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
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

const CREDENTIAL: &str = "s3cr3t-p4ssw0rd-do-not-leak";

/// **The secrets round-trip test, at the wire.** A credential must appear in no
/// response body — not the one that stored it, not a later read.
#[tokio::test]
async fn a_credential_never_appears_in_any_response() {
    let (app, _database, _) = test_app().await;

    let (created, body) = send(
        &app,
        "POST",
        "/connectors/configs",
        Some(json!({
            "connector": "postgres",
            "serviceName": "warehouse",
            "settings": { "host": "db.internal", "database": "retail" },
            "secret": CREDENTIAL,
        })),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED, "{body}");
    assert!(
        !body.contains("s3cr3t"),
        "the credential came back in the response that stored it: {body}"
    );

    let (listed, list) = send(&app, "GET", "/connectors/configs", None).await;
    assert_eq!(listed, StatusCode::OK);
    assert!(
        !list.contains("s3cr3t"),
        "the credential came back in a later read: {list}"
    );

    // And the settings *are* returned, so the assertions above are about the
    // credential rather than about a surface that returns nothing.
    assert!(list.contains("db.internal"), "{list}");
    assert!(list.contains("\"hasSecret\":true"), "{list}");
}

/// **An edit that does not resend the credential keeps it.** A form cannot
/// resend what it was never given, and treating absent as "clear it" would break
/// a connector every time somebody changed a port.
#[tokio::test]
async fn editing_without_the_secret_keeps_it() {
    let (app, _database, _) = test_app().await;
    send(
        &app,
        "POST",
        "/connectors/configs",
        Some(json!({
            "connector": "postgres",
            "serviceName": "warehouse",
            "settings": {},
            "secret": CREDENTIAL,
        })),
    )
    .await;

    let (status, body) = send(
        &app,
        "POST",
        "/connectors/configs",
        Some(json!({
            "connector": "postgres",
            "serviceName": "warehouse",
            "settings": { "database": "changed" },
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let saved: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(saved["hasSecret"], true, "the edit cleared the credential");
    assert_eq!(saved["settings"]["database"], "changed");
}

/// A configuration with no credential is a real state — some databases are
/// reachable without one — and reads as such rather than as an error.
#[tokio::test]
async fn a_configuration_with_no_credential_is_allowed_and_says_so() {
    let (app, _database, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/connectors/configs",
        Some(json!({ "connector": "postgres", "serviceName": "warehouse", "settings": {} })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let saved: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(saved["hasSecret"], false);
}

/// A blank secret is refused rather than stored: it would set `hasSecret` and
/// then fail at connection time with a credential error nobody can explain.
#[tokio::test]
async fn a_blank_credential_is_refused() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/connectors/configs",
        Some(json!({
            "connector": "postgres",
            "serviceName": "warehouse",
            "settings": {},
            "secret": "   ",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// One configuration per service per connector — two would make "which
/// credential did last night's run use" unanswerable.
#[tokio::test]
async fn saving_twice_updates_rather_than_duplicating() {
    let (app, _database, _) = test_app().await;
    let save = |secret: &'static str| {
        send(
            &app,
            "POST",
            "/connectors/configs",
            Some(json!({
                "connector": "postgres",
                "serviceName": "warehouse",
                "settings": {},
                "secret": secret,
            })),
        )
    };

    save("first").await;
    save("second").await;

    let (_, list) = send(&app, "GET", "/connectors/configs", None).await;
    let configs: Value = serde_json::from_str(&list).expect("json");
    assert_eq!(configs.as_array().expect("array").len(), 1, "{list}");
}
