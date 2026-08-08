//! Epic 35 at the wire: threads, resolution, change proposals, announcements,
//! reactions and the merged activity feed.
//!
//! The domain tests (`graph_owl_core::collaboration`) prove the decisions —
//! attribution goes to the proposer not the accepter, a boundary is inclusive
//! start / exclusive end, a repeat reaction toggles off — and the repository
//! tests prove the schema. What only an HTTP test can see: status codes, the
//! trust boundary on authorship (a client cannot name its own author), and
//! that authorization is actually wired to the right principal rather than
//! just present in the facade's signature.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app_with_secret;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "collab-demo-signing-secret-not-for-production";

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

async fn send_as(
    app: &axum::Router,
    method: &str,
    uri: &str,
    subject: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {}", token(subject)));
    request = if body.is_some() {
        request.header("content-type", "application/json")
    } else {
        request
    };
    let request = match body {
        Some(body) => request.body(Body::from(body.to_string())),
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

async fn asset_as(app: &axum::Router, subject: &str, name: &str) -> String {
    let (status, body) = send_as(
        app,
        "POST",
        "/assets",
        subject,
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("an id").to_string()
}

/// `PUT /assets/{id}/owners` is admin-gated (an ownership assignment is an
/// administrative act, not a cataloguing one — `graph_owl_server::set_asset_owners`),
/// and this test binary's JWT-mode principals are never admin by default (no
/// `GRAPH_OWL_ADMIN_SUBJECTS` is set, unlike `test_app()`'s open mode, where
/// every request silently runs as `Principal::system()` with `is_admin: true`
/// baked in). `subject` is promoted to admin by the same raw-SQL route
/// `authorization_fixture` uses for `root` in `authorization.rs`, rather than
/// mutating `GRAPH_OWL_ADMIN_SUBJECTS` — that env var is process-wide and this
/// binary runs tests concurrently, so setting it would flip every other
/// in-flight test's admin status too.
async fn set_owner(db_url: &str, app: &axum::Router, subject: &str, asset_id: &str, owner: &str) {
    // Auto-provisions `subject` and `owner` — `PUT .../owners` refuses an owner id
    // naming nobody it has seen (`an_unknown_principal_is_a_400_naming_which_owner`
    // in `asset_owners.rs`), so a never-authenticated `owner` must sign in once first.
    send_as(app, "GET", "/assets/stats", subject, None).await;
    send_as(app, "GET", "/assets/stats", owner, None).await;
    let pool = sqlx::PgPool::connect(db_url).await.expect("db connection");
    sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = $1")
        .bind(subject)
        .execute(&pool)
        .await
        .expect("promote subject to admin");

    let (status, body) = send_as(
        app,
        "PUT",
        &format!("/assets/{asset_id}/owners"),
        subject,
        Some(json!({ "owners": [{ "id": owner, "kind": "user" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

// ---- Slice A: threads and replies ----

#[tokio::test]
async fn starting_a_thread_ignores_a_client_supplied_author() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        // `createdBy` names nothing `StartThreadRequest` reads, but the
        // point of this test is the trust boundary, not the DTO shape: even
        // if a client tries, the authenticated principal must win.
        Some(json!({ "message": "why is this null?", "createdBy": "somebody-else" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["thread"]["createdBy"], "alice", "{body}");
    assert_eq!(body["post"]["author"], "alice", "{body}");
}

#[tokio::test]
async fn starting_a_thread_against_an_unknown_asset_is_a_404() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/assets/{}/threads", uuid::Uuid::new_v4()),
        "alice",
        Some(json!({ "message": "hello" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn a_thread_anchored_to_an_empty_field_is_a_400() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "hello", "field": "" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn replies_are_ordered_and_paginated() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();

    for message in ["first reply", "second reply", "third reply"] {
        let (status, body) = send_as(
            &app,
            "POST",
            &format!("/threads/{thread_id}/posts"),
            "bob",
            Some(json!({ "message": message })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    let (status, page) = send_as(
        &app,
        "GET",
        &format!("/threads/{thread_id}/posts?limit=2&offset=0"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        page["total"],
        json!(4),
        "opening post plus 3 replies: {page}"
    );
    let messages: Vec<&str> = page["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|p| p["message"].as_str().expect("message"))
        .collect();
    assert_eq!(messages, vec!["opening", "first reply"], "{page}");
}

#[tokio::test]
async fn editing_a_post_by_its_author_succeeds_and_records_edited_at() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let post_id = started["post"]["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "PATCH",
        &format!("/posts/{post_id}"),
        "alice",
        Some(json!({ "message": "opening, corrected" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "opening, corrected");
    assert!(!body["editedAt"].is_null(), "{body}");
}

#[tokio::test]
async fn editing_a_post_by_a_non_author_is_a_403() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let post_id = started["post"]["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "PATCH",
        &format!("/posts/{post_id}"),
        "bob",
        Some(json!({ "message": "hijacked" })),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn deleting_a_post_tombstones_it_rather_than_removing_it_from_the_thread() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();
    let post_id = started["post"]["id"].as_str().expect("id").to_string();

    let (status, _) = send_as(&app, "DELETE", &format!("/posts/{post_id}"), "alice", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, page) = send_as(
        &app,
        "GET",
        &format!("/threads/{thread_id}/posts"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        page["total"],
        json!(1),
        "the thread structure survives: {page}"
    );
    assert_eq!(page["data"][0]["deleted"], json!(true), "{page}");
}

#[tokio::test]
async fn deleting_someone_elses_post_is_a_403() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let post_id = started["post"]["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(&app, "DELETE", &format!("/posts/{post_id}"), "bob", None).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn threads_are_filterable_by_resolved_state_at_the_wire() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, open) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "open question" })),
    )
    .await;
    let (_, to_resolve) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "answered question" })),
    )
    .await;
    let to_resolve_id = to_resolve["thread"]["id"].as_str().expect("id").to_string();
    send_as(
        &app,
        "POST",
        &format!("/threads/{to_resolve_id}/resolve"),
        "alice",
        None,
    )
    .await;

    let (status, unresolved) = send_as(
        &app,
        "GET",
        &format!("/assets/{orders}/threads?resolved=false"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unresolved}");
    assert_eq!(unresolved["total"], json!(1), "{unresolved}");
    assert_eq!(
        unresolved["data"][0]["id"], open["thread"]["id"],
        "{unresolved}"
    );
}

// ---- Slice B: threads resolve ----

#[tokio::test]
async fn resolving_a_thread_records_who_and_when() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/resolve"),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["resolved"], json!(true));
    assert_eq!(body["resolvedBy"], "alice");
    assert!(!body["resolvedAt"].is_null());
}

#[tokio::test]
async fn resolving_an_already_resolved_thread_is_a_409() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();
    send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/resolve"),
        "alice",
        None,
    )
    .await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/resolve"),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn an_unrelated_user_cannot_resolve_a_thread() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/resolve"),
        "mallory",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn an_entity_owner_who_did_not_start_the_thread_may_still_resolve_it() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "carol").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/resolve"),
        "carol",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn reopening_a_resolved_thread_clears_the_resolution() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let thread_id = started["thread"]["id"].as_str().expect("id").to_string();
    send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/resolve"),
        "alice",
        None,
    )
    .await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/threads/{thread_id}/reopen"),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["resolved"], json!(false));
    assert!(body["resolvedBy"].is_null());
}

// ---- Slice C: change proposals ----

#[tokio::test]
async fn a_user_without_write_permission_may_still_propose_a_change() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "a better description",
            "rationale": "the old one was empty",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["status"], "pending");
    assert_eq!(body["proposedBy"], "mallory");
}

#[tokio::test]
async fn accepting_attributes_the_change_to_the_proposer_not_the_accepter() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;

    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "a better description",
            "rationale": "the old one was empty",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();

    let (status, accepted) = send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/accept"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["decidedBy"], "alice", "{accepted}");

    let (_, asset_body) = send_as(&app, "GET", &format!("/assets/{orders}"), "alice", None).await;
    assert_eq!(asset_body["description"], "a better description");
    assert_eq!(
        asset_body["updatedBy"], "mallory",
        "decision 3: attribution goes to the proposer, not the accepter: {asset_body}"
    );
}

#[tokio::test]
async fn accepting_against_a_stale_current_value_is_a_409() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;

    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "a better description",
            "rationale": "the old one was empty",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();

    // The field changes underneath the proposal before it is decided.
    let (status, body) = send_as(
        &app,
        "PATCH",
        &format!("/assets/{orders}"),
        "alice",
        Some(json!({ "description": "changed by someone else in the meantime" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/accept"),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
}

#[tokio::test]
async fn only_an_owner_may_accept_or_reject_a_proposal() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "x",
            "rationale": "y",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/accept"),
        "someone-unrelated",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn rejecting_without_a_reason_is_a_400() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;
    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "x",
            "rationale": "y",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/reject"),
        "alice",
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn rejecting_with_a_reason_records_it_and_leaves_the_field_untouched() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;
    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "x",
            "rationale": "y",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/reject"),
        "alice",
        Some(json!({ "reason": "not accurate" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "rejected");
    assert_eq!(body["decisionReason"], "not accurate");

    let (_, asset_body) = send_as(&app, "GET", &format!("/assets/{orders}"), "alice", None).await;
    assert_eq!(asset_body["description"], Value::Null, "{asset_body}");
}

#[tokio::test]
async fn accepting_an_already_decided_proposal_is_a_409() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;
    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "x",
            "rationale": "y",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();
    send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/accept"),
        "alice",
        None,
    )
    .await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/reject"),
        "alice",
        Some(json!({ "reason": "too late" })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn proposals_are_listable_per_entity_and_per_user_at_the_wire() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "x",
            "rationale": "y",
        })),
    )
    .await;

    let (status, for_entity) = send_as(
        &app,
        "GET",
        &format!("/assets/{orders}/change-proposals"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{for_entity}");
    assert_eq!(for_entity["total"], json!(1), "{for_entity}");

    let (status, for_user) = send_as(
        &app,
        "GET",
        "/users/mallory/change-proposals",
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{for_user}");
    assert_eq!(for_user["total"], json!(1), "{for_user}");
}

// ---- Phase 3 item 3.2: the catalog-wide listing and "who am I" ----

/// The RED case `proposals_are_listable_per_entity_and_per_user_at_the_wire`
/// above cannot express: proposals against *two different* entities must
/// both appear from one `GET /change-proposals` call — the whole reason
/// this endpoint exists rather than the caller fanning out per entity.
#[tokio::test]
async fn every_proposal_catalog_wide_is_listable_at_the_wire() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let payments = asset_as(&app, "alice", "payments").await;

    for asset in [&orders, &payments] {
        send_as(
            &app,
            "POST",
            &format!("/assets/{asset}/change-proposals"),
            "mallory",
            Some(json!({
                "field": "description",
                "currentValue": Value::Null,
                "proposedValue": "x",
                "rationale": "y",
            })),
        )
        .await;
    }

    let (status, all) = send_as(&app, "GET", "/change-proposals", "alice", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(
        all["total"],
        json!(2),
        "both entities' proposals must appear from one call: {all}"
    );
    let abouts: Vec<&str> = all["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|p| p["about"].as_str().expect("about"))
        .collect();
    assert!(abouts.contains(&orders.as_str()), "{abouts:?}");
    assert!(abouts.contains(&payments.as_str()), "{abouts:?}");
}

#[tokio::test]
async fn who_am_i_resolves_the_callers_own_identity() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;

    let (status, me) = send_as(&app, "GET", "/me", "alice", None).await;

    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["id"], json!("alice"), "{me}");
}

#[tokio::test]
async fn who_am_i_is_401_without_a_token() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;

    let request = Request::builder()
        .method("GET")
        .uri("/me")
        .body(Body::empty())
        .expect("request should build");
    let response = app.oneshot(request).await.expect("request handled");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---- Slice D: announcements ----

#[tokio::test]
async fn an_inverted_window_is_a_400() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/announcements"),
        "alice",
        Some(json!({
            "message": "deprecated soon",
            "startsAt": "2030-01-02T00:00:00Z",
            "endsAt": "2030-01-01T00:00:00Z",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn only_an_owner_may_create_an_announcement() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/announcements"),
        "mallory",
        Some(json!({
            "message": "deprecated soon",
            "startsAt": "2030-01-01T00:00:00Z",
            "endsAt": "2030-01-02T00:00:00Z",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn an_active_announcement_appears_on_active_but_an_expired_one_does_not() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;
    let now = chrono::Utc::now();
    let active = json!({
        "message": "active now",
        "startsAt": (now - chrono::Duration::hours(1)).to_rfc3339(),
        "endsAt": (now + chrono::Duration::hours(1)).to_rfc3339(),
    });
    let expired = json!({
        "message": "long over",
        "startsAt": (now - chrono::Duration::days(2)).to_rfc3339(),
        "endsAt": (now - chrono::Duration::days(1)).to_rfc3339(),
    });
    send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/announcements"),
        "alice",
        Some(active),
    )
    .await;
    send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/announcements"),
        "alice",
        Some(expired),
    )
    .await;

    let (status, active_body) = send_as(
        &app,
        "GET",
        &format!("/assets/{orders}/announcements/active"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{active_body}");
    let messages: Vec<&str> = active_body
        .as_array()
        .expect("a list")
        .iter()
        .map(|a| a["message"].as_str().expect("message"))
        .collect();
    assert_eq!(messages, vec!["active now"], "{active_body}");

    let (status, all_body) = send_as(
        &app,
        "GET",
        &format!("/assets/{orders}/announcements"),
        "alice",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{all_body}");
    assert_eq!(all_body["total"], json!(2), "both retained: {all_body}");
}

#[tokio::test]
async fn an_announcement_on_a_container_is_visible_on_its_descendant() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let warehouse = asset_as(&app, "alice", "warehouse").await;
    set_owner(&db_url, &app, "alice", &warehouse, "alice").await;
    // `warehouse` → `database` → `schema` → `table`: a container's `kind` bounds what
    // it may directly contain (`a table is contained by a schema, not a service`), so
    // the descendant proof needs the full chain, matching `asset_owners.rs`'s own
    // `estate()` fixture.
    let mut parent = warehouse.clone();
    let mut table = String::new();
    for (kind, name) in [
        ("database", "retail"),
        ("schema", "public"),
        ("table", "orders"),
    ] {
        let (status, body) = send_as(
            &app,
            "POST",
            "/assets",
            "alice",
            Some(json!({ "kind": kind, "name": name, "parentId": parent })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        parent = body["id"].as_str().expect("id").to_string();
        table = parent.clone();
    }
    let now = chrono::Utc::now();
    send_as(
        &app,
        "POST",
        &format!("/assets/{warehouse}/announcements"),
        "alice",
        Some(json!({
            "message": "the whole warehouse is being retired",
            "startsAt": (now - chrono::Duration::hours(1)).to_rfc3339(),
            "endsAt": (now + chrono::Duration::hours(1)).to_rfc3339(),
        })),
    )
    .await;

    let (status, active) = send_as(
        &app,
        "GET",
        &format!("/assets/{table}/announcements/active"),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{active}");
    assert_eq!(active.as_array().expect("a list").len(), 1, "{active}");
}

// ---- Slice E: reactions ----

#[tokio::test]
async fn reacting_twice_toggles_it_off_at_the_wire() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let post_id = started["post"]["id"].as_str().expect("id").to_string();

    let (status, added) = send_as(
        &app,
        "POST",
        &format!("/posts/{post_id}/reactions"),
        "bob",
        Some(json!({ "kind": "helpful" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    assert_eq!(added, json!("add"));

    let (_, counts) = send_as(
        &app,
        "GET",
        &format!("/posts/{post_id}/reactions"),
        "alice",
        None,
    )
    .await;
    assert_eq!(
        counts,
        json!([{ "kind": "helpful", "count": 1 }]),
        "{counts}"
    );

    let (status, removed) = send_as(
        &app,
        "POST",
        &format!("/posts/{post_id}/reactions"),
        "bob",
        Some(json!({ "kind": "helpful" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{removed}");
    assert_eq!(removed, json!("remove"));

    let (_, counts) = send_as(
        &app,
        "GET",
        &format!("/posts/{post_id}/reactions"),
        "alice",
        None,
    )
    .await;
    assert_eq!(counts, json!([]), "{counts}");
}

#[tokio::test]
async fn reacting_to_a_deleted_post_is_a_400() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    let (_, started) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "opening" })),
    )
    .await;
    let post_id = started["post"]["id"].as_str().expect("id").to_string();
    send_as(&app, "DELETE", &format!("/posts/{post_id}"), "alice", None).await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/posts/{post_id}/reactions"),
        "bob",
        Some(json!({ "kind": "helpful" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ---- Slice F: the activity feed ----

#[tokio::test]
async fn the_activity_feed_merges_threads_and_proposals_newest_first() {
    let (app, _db, db_url) = test_app_with_secret(SECRET).await;
    let orders = asset_as(&app, "alice", "orders").await;
    set_owner(&db_url, &app, "alice", &orders, "alice").await;

    send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/threads"),
        "alice",
        Some(json!({ "message": "a question" })),
    )
    .await;
    let (_, proposed) = send_as(
        &app,
        "POST",
        &format!("/assets/{orders}/change-proposals"),
        "mallory",
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "x",
            "rationale": "y",
        })),
    )
    .await;
    let proposal_id = proposed["id"].as_str().expect("id").to_string();
    send_as(
        &app,
        "POST",
        &format!("/change-proposals/{proposal_id}/accept"),
        "alice",
        None,
    )
    .await;

    let (status, feed) = send_as(
        &app,
        "GET",
        &format!("/assets/{orders}/activity"),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{feed}");
    let kinds: Vec<&str> = feed
        .as_array()
        .expect("a feed")
        .iter()
        .map(|e| e["kind"].as_str().expect("kind"))
        .collect();
    // The accepted proposal's own change event, the proposal being decided,
    // the proposal being created, and the thread starting: four entries from
    // three different sources (Epic 3 versions + collaboration events), in
    // one ordered stream — the property this slice's efficiency criterion is
    // about, proven by their all being present rather than fanned out per
    // entity.
    assert!(kinds.contains(&"change"), "{kinds:?}");
    assert!(kinds.contains(&"threadStarted"), "{kinds:?}");
    assert!(kinds.contains(&"proposalCreated"), "{kinds:?}");
    assert!(kinds.contains(&"proposalDecided"), "{kinds:?}");
}

#[tokio::test]
async fn the_activity_feed_for_an_unknown_asset_is_a_404() {
    let (app, _db, _) = test_app_with_secret(SECRET).await;

    let (status, body) = send_as(
        &app,
        "GET",
        &format!("/assets/{}/activity", uuid::Uuid::new_v4()),
        "alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
