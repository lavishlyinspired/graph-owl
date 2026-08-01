//! Epic 24 Slices A, B and C at the wire: glossary and term CRUD, scoped
//! uniqueness, synonym search, "delete a glossary with terms → 409 unless
//! recursive" (Slice A); SKOS relations with inverse consistency and cycle
//! rejection (Slice B); and the review workflow — transitions, reviewer
//! assignment, and the `403` a non-reviewer's approval attempt earns
//! (Slice C).

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

async fn glossary(app: &axum::Router, name: &str) -> Value {
    let (status, body) = send(app, "POST", "/glossaries", Some(json!({ "name": name }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

async fn term(app: &axum::Router, glossary_id: &str, name: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/glossaries/{glossary_id}/terms"),
        Some(json!({ "name": name, "definition": format!("the meaning of {name}") })),
    )
    .await
}

// ---- Glossary CRUD ----

#[tokio::test]
async fn a_glossary_can_be_created_and_fetched() {
    let (app, _db, _) = test_app().await;

    let created = glossary(&app, "Finance").await;
    assert_eq!(created["fullyQualifiedName"], "Finance");

    let (status, fetched) = send(
        &app,
        "GET",
        &format!("/glossaries/{}", created["id"].as_str().unwrap()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{fetched}");
    assert_eq!(fetched["name"], "Finance");
}

#[tokio::test]
async fn every_glossary_is_listed() {
    let (app, _db, _) = test_app().await;
    glossary(&app, "Finance").await;
    glossary(&app, "Support").await;

    let (status, body) = send(&app, "GET", "/glossaries", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().expect("a list").len(), 2);
}

#[tokio::test]
async fn a_glossary_needs_a_name() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(&app, "POST", "/glossaries", Some(json!({ "name": "  " }))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn fetching_an_unknown_glossary_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "GET",
        &format!("/glossaries/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Term CRUD and FQN ----

#[tokio::test]
async fn a_term_is_created_under_a_glossary_with_a_derived_fqn() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;

    let (status, body) = term(&app, finance["id"].as_str().unwrap(), "Customer").await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["fullyQualifiedName"], "Finance.Customer");
    assert_eq!(body["status"], "draft");
}

// **The scoped-uniqueness pair the plan names**: the same term name in two
// different glossaries must both succeed, because "Customer" in Finance and
// "Customer" in Support are different terms with different addresses.
#[tokio::test]
async fn the_same_term_name_in_two_glossaries_both_succeed() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let support = glossary(&app, "Support").await;

    let (first, first_body) = term(&app, finance["id"].as_str().unwrap(), "Customer").await;
    let (second, second_body) = term(&app, support["id"].as_str().unwrap(), "Customer").await;

    assert_eq!(first, StatusCode::CREATED, "{first_body}");
    assert_eq!(second, StatusCode::CREATED, "{second_body}");
    assert_eq!(first_body["fullyQualifiedName"], "Finance.Customer");
    assert_eq!(second_body["fullyQualifiedName"], "Support.Customer");
}

// And the negative: the same name **within** one glossary collides.
#[tokio::test]
async fn the_same_term_name_twice_in_one_glossary_is_a_conflict() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    term(&app, finance["id"].as_str().unwrap(), "Customer").await;

    let (status, body) = term(&app, finance["id"].as_str().unwrap(), "Customer").await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn creating_a_term_under_an_unknown_glossary_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = term(&app, &uuid::Uuid::new_v4().to_string(), "Customer").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn every_term_in_a_glossary_is_listed() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let id = finance["id"].as_str().unwrap();
    term(&app, id, "Customer").await;
    term(&app, id, "Revenue").await;

    let (status, body) = send(&app, "GET", &format!("/glossaries/{id}/terms"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().expect("a list").len(), 2);
}

#[tokio::test]
async fn a_term_can_be_updated_and_the_change_is_read_back() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Customer").await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/glossary-terms/{id}"),
        Some(json!({
            "definition": "a party who has purchased or may purchase",
            "synonyms": ["client"],
            "abbreviations": ["cust"],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["definition"],
        "a party who has purchased or may purchase"
    );
    assert_eq!(body["synonyms"], json!(["client"]));
    assert_eq!(body["abbreviations"], json!(["cust"]));
}

#[tokio::test]
async fn deleting_a_term_removes_it() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Customer").await;
    let id = created["id"].as_str().unwrap();

    let (status, _) = send(&app, "DELETE", &format!("/glossary-terms/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(&app, "GET", &format!("/glossary-terms/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Search: synonyms and abbreviations are indexed, per Slice A ----

#[tokio::test]
async fn a_synonym_match_finds_the_term() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Customer").await;
    let id = created["id"].as_str().unwrap();
    send(
        &app,
        "PATCH",
        &format!("/glossary-terms/{id}"),
        Some(json!({ "synonyms": ["client"] })),
    )
    .await;

    let (status, body) = send(&app, "GET", "/glossary-terms/search?q=client", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let hits = body.as_array().expect("a list");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], id);
}

// The negative half: an unrelated word must not match, or the positive above
// would pass against a search that returns everything.
#[tokio::test]
async fn an_unrelated_word_does_not_match() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    term(&app, finance["id"].as_str().unwrap(), "Customer").await;

    let (status, body) = send(&app, "GET", "/glossary-terms/search?q=zzzznomatch", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.as_array().expect("a list").is_empty());
}

// ---- Deleting a glossary: 409 unless recursive ----

#[tokio::test]
async fn deleting_a_glossary_with_terms_is_a_409() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    term(&app, finance["id"].as_str().unwrap(), "Customer").await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/glossaries/{}", finance["id"].as_str().unwrap()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains('1'), "a count: {body}");
}

#[tokio::test]
async fn deleting_a_glossary_recursively_takes_its_terms_with_it() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let id = finance["id"].as_str().unwrap();
    let (_, created) = term(&app, id, "Customer").await;
    let term_id = created["id"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/glossaries/{id}?recursive=true"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(&app, "GET", &format!("/glossary-terms/{term_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the term should be gone too");
}

#[tokio::test]
async fn deleting_an_empty_glossary_needs_no_recursive_flag() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/glossaries/{}", finance["id"].as_str().unwrap()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn deleting_an_unknown_glossary_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/glossaries/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Slice B: SKOS relations at the wire ----

async fn add_relation(
    app: &axum::Router,
    term_id: &str,
    kind: &str,
    target: &str,
) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/glossary-terms/{term_id}/relations"),
        Some(json!({ "kind": kind, "target": target })),
    )
    .await
}

/// `system` is pre-seeded by V15, so open-mode's `Principal::system()` — every
/// unauthenticated request in these tests — is already a known reviewer
/// without provisioning a user first.
async fn set_reviewers(
    app: &axum::Router,
    term_id: &str,
    reviewers: &[&str],
) -> (StatusCode, Value) {
    send(
        app,
        "PUT",
        &format!("/glossary-terms/{term_id}/reviewers"),
        Some(json!({ "reviewers": reviewers })),
    )
    .await
}

async fn transition(app: &axum::Router, term_id: &str, to: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/glossary-terms/{term_id}/transitions"),
        Some(json!({ "to": to })),
    )
    .await
}

async fn relations_of(app: &axum::Router, term_id: &str) -> Value {
    let (status, body) = send(
        app,
        "GET",
        &format!("/glossary-terms/{term_id}/relations"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn broader_implies_narrower_without_a_second_stored_edge() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, child) = term(&app, glossary_id, "Checking Account").await;
    let (_, parent) = term(&app, glossary_id, "Account").await;
    let child_id = child["id"].as_str().unwrap();
    let parent_id = parent["id"].as_str().unwrap();

    let (status, body) = add_relation(&app, child_id, "broader", parent_id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let on_parent = relations_of(&app, parent_id).await;
    let listed = on_parent.as_array().expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["kind"], "narrower");
    assert_eq!(listed[0]["target"], child_id);
}

#[tokio::test]
async fn narrower_cannot_be_asserted_directly() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, child) = term(&app, glossary_id, "Checking Account").await;
    let (_, parent) = term(&app, glossary_id, "Account").await;

    let (status, body) = add_relation(
        &app,
        parent["id"].as_str().unwrap(),
        "narrower",
        child["id"].as_str().unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_self_parenting_term_is_refused() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, account) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = account["id"].as_str().unwrap();

    let (status, body) = add_relation(&app, id, "broader", id).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// Depth 3, because a check comparing only the immediate parent passes depth
// 1 and fails here — the same trap Epic 11's team nesting hit.
#[tokio::test]
async fn a_three_term_cycle_is_refused() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, a) = term(&app, glossary_id, "A").await;
    let (_, b) = term(&app, glossary_id, "B").await;
    let (_, c) = term(&app, glossary_id, "C").await;
    let a_id = a["id"].as_str().unwrap();
    let b_id = b["id"].as_str().unwrap();
    let c_id = c["id"].as_str().unwrap();
    add_relation(&app, a_id, "broader", b_id).await;
    add_relation(&app, b_id, "broader", c_id).await;

    let (status, body) = add_relation(&app, c_id, "broader", a_id).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// The negative beside the cycle tests: poly-hierarchy is legitimate SKOS,
// and a checker refusing every second `broader` would pass the cycle tests
// and fail only here.
#[tokio::test]
async fn a_term_may_have_more_than_one_broader_parent() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, child) = term(&app, glossary_id, "Savings Account").await;
    let (_, first) = term(&app, glossary_id, "Account").await;
    let (_, second) = term(&app, glossary_id, "Financial Product").await;
    let child_id = child["id"].as_str().unwrap();

    let (first_status, first_body) =
        add_relation(&app, child_id, "broader", first["id"].as_str().unwrap()).await;
    let (second_status, second_body) =
        add_relation(&app, child_id, "broader", second["id"].as_str().unwrap()).await;

    assert_eq!(first_status, StatusCode::CREATED, "{first_body}");
    assert_eq!(second_status, StatusCode::CREATED, "{second_body}");
    let on_child = relations_of(&app, child_id).await;
    assert_eq!(on_child.as_array().expect("a list").len(), 2);
}

#[tokio::test]
async fn related_is_symmetric_on_read() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, a) = term(&app, glossary_id, "A").await;
    let (_, b) = term(&app, glossary_id, "B").await;
    let a_id = a["id"].as_str().unwrap();
    let b_id = b["id"].as_str().unwrap();

    add_relation(&app, a_id, "related", b_id).await;

    let on_b = relations_of(&app, b_id).await;
    let listed = on_b.as_array().expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["kind"], "related");
    assert_eq!(listed[0]["target"], a_id);
}

// `exactMatch` points at an external IRI and is **not** validated for
// reachability — an IRI that resolves nowhere must still be accepted.
#[tokio::test]
async fn an_exact_match_to_an_external_iri_is_not_checked_for_reachability() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, account) = term(&app, finance["id"].as_str().unwrap(), "Account").await;

    let (status, body) = add_relation(
        &app,
        account["id"].as_str().unwrap(),
        "exactMatch",
        "http://example.invalid/does-not-exist",
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_broader_target_that_is_not_a_known_term_is_refused() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, account) = term(&app, finance["id"].as_str().unwrap(), "Account").await;

    let (status, body) = add_relation(
        &app,
        account["id"].as_str().unwrap(),
        "broader",
        &uuid::Uuid::new_v4().to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn asserting_a_relation_on_an_unknown_term_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = add_relation(
        &app,
        &uuid::Uuid::new_v4().to_string(),
        "exactMatch",
        "http://x.example/1",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn removing_a_relation_the_term_declared_deletes_it() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, child) = term(&app, glossary_id, "Checking Account").await;
    let (_, parent) = term(&app, glossary_id, "Account").await;
    let child_id = child["id"].as_str().unwrap();
    let parent_id = parent["id"].as_str().unwrap();
    add_relation(&app, child_id, "broader", parent_id).await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/glossary-terms/{child_id}/relations?kind=broader&target={parent_id}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let on_child = relations_of(&app, child_id).await;
    assert!(on_child.as_array().expect("a list").is_empty());
}

// The derived half is not a row: removing `narrower` from the parent (which
// never stored it) must be `404`, not a silent success.
#[tokio::test]
async fn removing_a_derived_relation_that_was_never_stored_is_a_404() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, child) = term(&app, glossary_id, "Checking Account").await;
    let (_, parent) = term(&app, glossary_id, "Account").await;
    let child_id = child["id"].as_str().unwrap();
    let parent_id = parent["id"].as_str().unwrap();
    add_relation(&app, child_id, "broader", parent_id).await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/glossary-terms/{parent_id}/relations?kind=narrower&target={child_id}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Slice C: review workflow at the wire ----

#[tokio::test]
async fn a_term_walks_the_workflow_and_the_version_bumps() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();
    let before = created["version"].as_str().unwrap().to_string();
    set_reviewers(&app, id, &["system"]).await;

    let (status, in_review) = transition(&app, id, "inReview").await;
    assert_eq!(status, StatusCode::OK, "{in_review}");
    assert_eq!(in_review["status"], "inReview");
    assert_ne!(in_review["version"], before, "the version should bump");

    let (status, approved) = transition(&app, id, "approved").await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["status"], "approved");
}

#[tokio::test]
async fn a_term_cannot_skip_review() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;

    let (status, body) = transition(&app, created["id"].as_str().unwrap(), "approved").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn approval_with_no_reviewer_assigned_is_refused() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();
    transition(&app, id, "inReview").await;

    let (status, body) = transition(&app, id, "approved").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn deprecation_carries_a_reason_and_a_successor() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, old) = term(&app, glossary_id, "Account").await;
    let (_, new) = term(&app, glossary_id, "Account V2").await;
    let old_id = old["id"].as_str().unwrap();
    set_reviewers(&app, old_id, &["system"]).await;
    transition(&app, old_id, "inReview").await;
    transition(&app, old_id, "approved").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/glossary-terms/{old_id}/transitions"),
        Some(json!({
            "to": "deprecated",
            "reason": "superseded",
            "successorTermId": new["id"],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "deprecated");
}

#[tokio::test]
async fn a_successor_that_is_not_a_known_term_is_refused() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();
    set_reviewers(&app, id, &["system"]).await;
    transition(&app, id, "inReview").await;
    transition(&app, id, "approved").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/glossary-terms/{id}/transitions"),
        Some(json!({
            "to": "deprecated",
            "reason": "superseded",
            "successorTermId": uuid::Uuid::new_v4().to_string(),
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn transitioning_an_unknown_term_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = transition(&app, &uuid::Uuid::new_v4().to_string(), "inReview").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn assigning_an_unknown_user_as_reviewer_is_refused() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;

    let (status, body) = set_reviewers(&app, created["id"].as_str().unwrap(), &["nobody"]).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn reviewers_can_be_read_back() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();
    set_reviewers(&app, id, &["system"]).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/glossary-terms/{id}/reviewers"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reviewers"], json!(["system"]));
}

// **The `403` that gives the reviewer list meaning.** Needs two genuinely
// distinct identities, which open mode cannot express (every unauthenticated
// request there is `Principal::system()`) — so this one test runs with JWT
// verification enabled instead.
#[tokio::test]
async fn a_non_reviewer_cannot_approve() {
    const SECRET: &str = "demo-signing-secret-not-for-production";
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
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {}", token(subject)));
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
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
        };
        (status, parsed)
    }

    let (app, _db, _) = common::test_app_with_secret(SECRET).await;
    // Any authenticated call auto-provisions the caller (Epic 12) — this one
    // exists only to seed "alice" as a known user before she is assignable.
    send_as(&app, "GET", "/glossaries", "alice", None).await;

    let (_, finance) = send_as(
        &app,
        "POST",
        "/glossaries",
        "alice",
        Some(json!({ "name": "Finance" })),
    )
    .await;
    let glossary_id = finance["id"].as_str().unwrap();
    let (_, created) = send_as(
        &app,
        "POST",
        &format!("/glossaries/{glossary_id}/terms"),
        "alice",
        Some(json!({ "name": "Account", "definition": "" })),
    )
    .await;
    let id = created["id"].as_str().unwrap();
    send_as(
        &app,
        "PUT",
        &format!("/glossary-terms/{id}/reviewers"),
        "alice",
        Some(json!({ "reviewers": ["alice"] })),
    )
    .await;
    send_as(
        &app,
        "POST",
        &format!("/glossary-terms/{id}/transitions"),
        "mallory",
        Some(json!({ "to": "inReview" })),
    )
    .await;

    let (status, body) = send_as(
        &app,
        "POST",
        &format!("/glossary-terms/{id}/transitions"),
        "mallory",
        Some(json!({ "to": "approved" })),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

// ---- Slice D: terms attach to assets and columns at the wire ----

async fn approve(app: &axum::Router, term_id: &str) {
    set_reviewers(app, term_id, &["system"]).await;
    transition(app, term_id, "inReview").await;
    transition(app, term_id, "approved").await;
}

#[tokio::test]
async fn an_approved_term_can_be_attached_and_listed_in_usage() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();
    approve(&app, id).await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/glossary-terms/{id}/usage"),
        Some(json!({ "targetFqn": "warehouse.public.orders" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, page) = send(&app, "GET", &format!("/glossary-terms/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["data"], json!(["warehouse.public.orders"]), "{page}");
}

// **Only `Approved` terms attach** (decision 4) — a draft's status is named
// in the refusal, not just "bad request".
#[tokio::test]
async fn a_draft_term_cannot_be_attached() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/glossary-terms/{id}/usage"),
        Some(json!({ "targetFqn": "warehouse.public.orders" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("draft"), "{body}");
}

#[tokio::test]
async fn attaching_to_an_unknown_term_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/glossary-terms/{}/usage", uuid::Uuid::new_v4()),
        Some(json!({ "targetFqn": "warehouse.public.orders" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn detaching_a_term_removes_it_from_usage() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();
    approve(&app, id).await;
    send(
        &app,
        "POST",
        &format!("/glossary-terms/{id}/usage"),
        Some(json!({ "targetFqn": "warehouse.public.orders" })),
    )
    .await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/glossary-terms/{id}/usage?targetFqn=warehouse.public.orders"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (_, page) = send(&app, "GET", &format!("/glossary-terms/{id}/usage"), None).await;
    assert!(page["data"].as_array().expect("a page").is_empty());
}

#[tokio::test]
async fn detaching_something_never_attached_is_a_404() {
    let (app, _db, _) = test_app().await;
    let finance = glossary(&app, "Finance").await;
    let (_, created) = term(&app, finance["id"].as_str().unwrap(), "Account").await;
    let id = created["id"].as_str().unwrap();

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/glossary-terms/{id}/usage?targetFqn=warehouse.public.orders"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn usage_of_an_unknown_term_is_a_404() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "GET",
        &format!("/glossary-terms/{}/usage", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
