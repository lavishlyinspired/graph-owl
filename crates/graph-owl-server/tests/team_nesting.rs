//! Epic 11 Slices B, F and G at the wire, plus user creation.
//!
//! These are the claims only an HTTP test can make: status codes, the `409` body
//! carrying counts, and idempotency being a `200` rather than a `409`.

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

async fn team(app: &axum::Router, id: &str, parent: Option<&str>) -> (StatusCode, Value) {
    let mut body = json!({ "id": id, "displayName": format!("The {id} team"), "members": [] });
    if let Some(p) = parent {
        body["parentTeamId"] = json!(p);
    }
    send(app, "POST", "/teams", Some(body)).await
}

async fn user(app: &axum::Router, id: &str) -> (StatusCode, Value) {
    send(
        app,
        "PUT",
        &format!("/users/{id}"),
        Some(json!({ "displayName": format!("{id} the person") })),
    )
    .await
}

async fn asset(app: &axum::Router, name: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("an id").to_string()
}

// ---- User creation: Slice A's missing half ----

// **A person who has never signed in could not previously be named as an owner.**
// Users existed only by auto-provisioning on authentication, which is exactly
// backwards for onboarding.
#[tokio::test]
async fn a_user_can_be_created_before_they_ever_sign_in() {
    let (app, _db, _) = test_app().await;

    let (status, body) = user(&app, "priya").await;

    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(body["id"], "priya");
    assert_eq!(body["displayName"], "priya the person");
    // Creating a user grants nothing — the right default for onboarding, and the
    // reason roles are not settable here.
    assert_eq!(body["roles"], json!([]));
}

// `PUT` on the id is idempotent: a retry is a rename, not a second user.
#[tokio::test]
async fn creating_the_same_user_twice_is_a_rename_not_a_duplicate() {
    let (app, _db, _) = test_app().await;
    user(&app, "priya").await;

    let (status, body) = send(
        &app,
        "PUT",
        "/users/priya",
        Some(json!({ "displayName": "Priya Sharma" })),
    )
    .await;

    assert!(status.is_success(), "{body}");
    assert_eq!(body["displayName"], "Priya Sharma");
}

#[tokio::test]
async fn a_user_needs_a_name_somebody_can_recognise() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "PUT",
        "/users/priya",
        Some(json!({ "displayName": "   " })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("displayName"), "{body}");
}

// **A newly created user can immediately be named as an owner** — which is the
// whole point of this endpoint existing, and the gap Slice C's tests had to work
// around by owning assets with the seeded `system` user.
#[tokio::test]
async fn a_created_user_can_be_named_as_an_owner() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    user(&app, "priya").await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": "priya", "kind": "user" }] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["owners"][0]["displayName"], "priya the person");
}

// ---- Slice B: nesting at the wire ----

#[tokio::test]
async fn a_team_can_report_into_another_and_the_children_are_listed() {
    let (app, _db, _) = test_app().await;
    team(&app, "platform", None).await;
    let (status, body) = team(&app, "data-eng", Some("platform")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["parentTeamId"], "platform");

    let (status, children) = send(&app, "GET", "/teams/platform/children", None).await;

    assert_eq!(status, StatusCode::OK, "{children}");
    let listed = children.as_array().expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], "data-eng");
}

// `parentTeamId` is always present, `null` for a root — a console reading its
// absence cannot tell "top of the hierarchy" from "a server without nesting".
#[tokio::test]
async fn a_root_team_reports_a_null_parent_rather_than_omitting_it() {
    let (app, _db, _) = test_app().await;

    let (_, body) = team(&app, "platform", None).await;

    assert_eq!(body["parentTeamId"], Value::Null);
    assert!(
        body.get("parentTeamId").is_some(),
        "the field must be present"
    );
}

// Cycles at each depth Slice B names. Depths 1 and 2 are caught by a naive check
// too; **depth 3 is the one that distinguishes an ancestor walk from a parent
// comparison**, and it is asserted separately for that reason.
#[tokio::test]
async fn a_self_parenting_team_is_refused() {
    let (app, _db, _) = test_app().await;
    team(&app, "platform", None).await;

    let (status, body) = team(&app, "platform", Some("platform")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("parentTeamId"), "{body}");
}

#[tokio::test]
async fn a_two_team_cycle_is_refused() {
    let (app, _db, _) = test_app().await;
    team(&app, "platform", None).await;
    team(&app, "data-eng", Some("platform")).await;

    let (status, body) = team(&app, "platform", Some("data-eng")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("cycle"), "{body}");
}

#[tokio::test]
async fn a_three_team_cycle_is_refused() {
    let (app, _db, _) = test_app().await;
    team(&app, "exec", None).await;
    team(&app, "platform", Some("exec")).await;
    team(&app, "data-eng", Some("platform")).await;

    let (status, body) = team(&app, "exec", Some("data-eng")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("cycle"), "{body}");
}

// And the negative, so the three above are about cycles rather than about nesting
// being refused outright.
#[tokio::test]
async fn a_legitimate_nesting_is_accepted() {
    let (app, _db, _) = test_app().await;
    team(&app, "exec", None).await;
    team(&app, "platform", Some("exec")).await;
    team(&app, "analytics", None).await;

    let (status, body) = team(&app, "analytics", Some("platform")).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_parent_that_does_not_exist_is_refused_by_name() {
    let (app, _db, _) = test_app().await;

    let (status, body) = team(&app, "data-eng", Some("nobody")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("nobody"), "{body}");
}

// ---- Slice F: following at the wire ----

// "Follow is idempotent (double-follow → `200`, one edge)". A `409` on the second
// call would make a retried request look like a conflict.
#[tokio::test]
async fn following_twice_is_two_successes_and_one_follower() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;

    let (first, one) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/followers/system"),
        None,
    )
    .await;
    let (second, two) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/followers/system"),
        None,
    )
    .await;

    assert_eq!(first, StatusCode::OK, "{one}");
    assert_eq!(second, StatusCode::OK, "{two}");
    assert_eq!(one["created"], json!(true));
    assert_eq!(two["created"], json!(false), "the second created nothing");
    assert_eq!(two["followerCount"], json!(1), "one edge, not two");
}

#[tokio::test]
async fn unfollowing_removes_the_follow() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/followers/system"),
        None,
    )
    .await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/assets/{orders}/followers/system"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, follows) = send(&app, "GET", "/users/system/follows", None).await;
    assert!(follows["data"].as_array().expect("a page").is_empty());
}

#[tokio::test]
async fn what_a_user_follows_is_listed_and_paginated() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    let mart = asset(&app, "mart").await;
    for id in [&orders, &mart] {
        send(&app, "PUT", &format!("/assets/{id}/followers/system"), None).await;
    }

    let (status, body) = send(&app, "GET", "/users/system/follows", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().expect("a page").len(), 2);
    assert!(
        body.get("paging").is_some(),
        "paginated like every asset page"
    );
}

// "Following a soft-deleted entity → `400`." Recording interest in a tombstone is
// a subscription to something nobody can read.
#[tokio::test]
async fn following_a_deleted_asset_is_refused() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    send(&app, "DELETE", &format!("/assets/{orders}"), None).await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/followers/system"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn following_an_asset_that_does_not_exist_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "PUT",
        &format!("/assets/{}/followers/system", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Slice G: deletion at the wire ----

#[tokio::test]
async fn a_principal_owning_nothing_is_deleted() {
    let (app, _db, _) = test_app().await;
    user(&app, "priya").await;

    let (status, _) = send(&app, "DELETE", "/users/priya", None).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

// "Deleting an owner of assets → `409` reporting how many assets and of which
// types." The counts are the point: "you own 400 things" is not actionable.
#[tokio::test]
async fn deleting_an_owner_is_a_409_naming_the_counts() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    user(&app, "priya").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": "priya", "kind": "user" }] })),
    )
    .await;

    let (status, body) = send(&app, "DELETE", "/users/priya", None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let detail = body.to_string();
    assert!(detail.contains('1'), "a count: {detail}");
    assert!(detail.contains("service"), "and the kind: {detail}");
}

#[tokio::test]
async fn reassigning_transfers_ownership_then_deletes() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    user(&app, "priya").await;
    user(&app, "ravi").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": "priya", "kind": "user" }] })),
    )
    .await;

    let (status, body) = send(
        &app,
        "DELETE",
        "/users/priya?reassignTo=ravi&reassignToKind=user",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, owners) = send(&app, "GET", &format!("/assets/{orders}/owners"), None).await;
    assert_eq!(owners["owners"][0]["id"], "ravi");
}

// **The kind is required with `reassignTo`.** A user and a team can share an id,
// and guessing would transfer an estate to the wrong principal — a mistake no
// response body would reveal.
#[tokio::test]
async fn reassigning_without_a_kind_is_refused() {
    let (app, _db, _) = test_app().await;
    user(&app, "priya").await;

    let (status, body) = send(&app, "DELETE", "/users/priya?reassignTo=ravi", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("reassignToKind"), "{body}");
}

#[tokio::test]
async fn reassigning_to_an_unknown_principal_is_refused() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    user(&app, "priya").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": "priya", "kind": "user" }] })),
    )
    .await;

    let (status, body) = send(
        &app,
        "DELETE",
        "/users/priya?reassignTo=nobody&reassignToKind=user",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// "Deleting a team with child teams → `409` unless children are reassigned."
#[tokio::test]
async fn deleting_a_team_with_children_is_a_409() {
    let (app, _db, _) = test_app().await;
    team(&app, "platform", None).await;
    team(&app, "data-eng", Some("platform")).await;

    let (status, body) = send(&app, "DELETE", "/teams/platform", None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("data-eng"), "{body}");
}

#[tokio::test]
async fn deleting_a_principal_that_does_not_exist_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(&app, "DELETE", "/users/nobody", None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
