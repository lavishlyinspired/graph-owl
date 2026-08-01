//! Epic 24 Slice A at the wire: glossary and term CRUD, scoped uniqueness,
//! synonym search, and "delete a glossary with terms → 409 unless recursive".

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
