//! Epic 41 Slice F's missing half at the wire: policies can now be saved,
//! not only dry-run.

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

fn deny_pii_body(roles: &[&str]) -> Value {
    json!({
        "policy": {
            "name": "deny-pii",
            "rules": [{
                "name": "no-pii",
                "effect": "deny",
                "operations": ["viewSensitive"],
                "resources": { "type": "tagged", "value": "pii" },
            }],
        },
        "roles": roles,
    })
}

#[tokio::test]
async fn a_saved_policy_appears_in_the_list_with_its_roles() {
    let (app, _container, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/policies",
        Some(deny_pii_body(&["analyst", "steward"])),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["policy"]["name"], "deny-pii");

    let (status, list) = send(&app, "GET", "/policies", None).await;
    assert_eq!(status, StatusCode::OK);
    let entries = list.as_array().expect("a list");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["policy"]["name"], "deny-pii");
    let mut roles: Vec<&str> = entries[0]["roles"]
        .as_array()
        .expect("roles")
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    roles.sort_unstable();
    assert_eq!(roles, vec!["analyst", "steward"]);
}

/// **Replace, not add.** Saving the same policy again with a smaller role set
/// must actually shrink it — revoking a role's access to a policy is exactly
/// the case a merge-only write would get backwards, and it is only provable
/// through a real second request over the wire.
#[tokio::test]
async fn saving_again_with_fewer_roles_replaces_the_attachment_over_the_wire() {
    let (app, _container, _url) = test_app().await;

    send(
        &app,
        "POST",
        "/policies",
        Some(deny_pii_body(&["analyst", "steward"])),
    )
    .await;
    send(&app, "POST", "/policies", Some(deny_pii_body(&["analyst"]))).await;

    let (_, list) = send(&app, "GET", "/policies", None).await;
    let entries = list.as_array().expect("a list");
    assert_eq!(entries.len(), 1, "one policy, updated in place, not two");
    assert_eq!(
        entries[0]["roles"].as_array().expect("roles").len(),
        1,
        "{entries:?}"
    );
}

#[tokio::test]
async fn a_policy_with_no_rules_is_a_400_not_a_500() {
    let (app, _container, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/policies",
        Some(json!({ "policy": { "name": "empty", "rules": [] }, "roles": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_deleted_policy_stops_appearing_in_the_list() {
    let (app, _container, _url) = test_app().await;
    send(&app, "POST", "/policies", Some(deny_pii_body(&["analyst"]))).await;

    let (status, _) = send(&app, "DELETE", "/policies/deny-pii", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = send(&app, "GET", "/policies", None).await;
    assert!(list.as_array().expect("a list").is_empty());
}

/// Idempotent, matching `delete_team`'s convention: the caller's goal ("this
/// policy no longer applies to anyone") is already true.
#[tokio::test]
async fn deleting_an_unknown_policy_still_succeeds() {
    let (app, _container, _url) = test_app().await;

    let (status, _) = send(&app, "DELETE", "/policies/never-existed", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// The wire shape a client actually receives, asserted against the bytes —
/// the same guard `Authorship` taught this project it needs, not a round
/// trip that would agree with itself regardless of field names.
#[tokio::test]
async fn the_wire_shape_is_camel_case() {
    let (app, _container, _url) = test_app().await;

    let (_, body) = send(&app, "POST", "/policies", Some(deny_pii_body(&["analyst"]))).await;

    assert_eq!(body["policy"]["rules"][0]["resources"]["type"], "tagged");
    assert!(body["policy"]["rules"][0].get("resource_matcher").is_none());
}
