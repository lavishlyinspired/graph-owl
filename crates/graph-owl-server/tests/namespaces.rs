//! `POST /namespaces` — how a domain pack brings its own vocabulary.
//!
//! Epic 105 DN-1 built the registry (a table, a port, an adapter) and nothing
//! exposed it, which is the same shape `Catalog::import_rdf` was in before P0:
//! a finished capability nothing could reach. This is the route, and it is
//! what makes the neutrality claim usable rather than merely true.
//!
//! **The load-bearing design decision is that the caller names an IRI and
//! never a code.** A pack manifest carrying a number would make two
//! deployments that installed packs in different orders disagree about what
//! `1024` means — and a `Sid` is stored as a bare `(code, local)` pair, so
//! that disagreement is unfixable after the fact rather than a migration.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{authorization_fixture, test_app, token};
use tower::ServiceExt;

const HOSP: &str = "https://example.org/ns/hospitality#";
const AUTO: &str = "https://example.org/ns/automotive#";

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    subject: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {}", token(subject)))
        .header("content-type", "application/json");
    let request = match body {
        Some(value) => request.body(Body::from(value.to_string())),
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn declaring_a_vocabulary_allocates_a_code_the_caller_never_chose() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/namespaces",
        "system",
        Some(serde_json::json!({ "iri": HOSP, "declaredBy": "pack:hospitality" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["iri"], HOSP);
    assert_eq!(body["declaredBy"], "pack:hospitality");
    assert_eq!(
        body["code"], 1024,
        "the first code handed out is the first the binary does not own: {body}"
    );
}

#[tokio::test]
async fn re_declaring_the_same_vocabulary_returns_the_same_code() {
    // **A pack is reloaded far more often than it is first installed.** If
    // this returned a conflict, every second `demo.sh` run would fail; if it
    // allocated a second code, the pack's own IRIs would resolve to two
    // different `Sid`s depending on when they were written.
    let (app, _db, _url) = test_app().await;
    let payload = serde_json::json!({ "iri": HOSP, "declaredBy": "pack:hospitality" });

    let (_, first) = send(&app, "POST", "/namespaces", "system", Some(payload.clone())).await;
    let (status, second) = send(&app, "POST", "/namespaces", "system", Some(payload)).await;

    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(first["code"], second["code"]);

    let (_, listed) = send(&app, "GET", "/namespaces", "system", None).await;
    assert_eq!(
        listed.as_array().map(Vec::len),
        Some(1),
        "and no second row was created: {listed}"
    );
}

#[tokio::test]
async fn two_vocabularies_get_two_codes() {
    let (app, _db, _url) = test_app().await;

    let (_, hosp) = send(
        &app,
        "POST",
        "/namespaces",
        "system",
        Some(serde_json::json!({ "iri": HOSP })),
    )
    .await;
    let (_, auto) = send(
        &app,
        "POST",
        "/namespaces",
        "system",
        Some(serde_json::json!({ "iri": AUTO })),
    )
    .await;

    assert_ne!(
        hosp["code"], auto["code"],
        "two vocabularies sharing a code would make every flake ambiguous"
    );
}

#[tokio::test]
async fn declaring_without_a_declared_by_attributes_it_to_the_caller() {
    // Provenance is never absent. A namespace outlives the pack that
    // introduced it, so "who asked for this" is the only way an operator
    // later works out what a stray prefix is.
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/namespaces",
        "system",
        Some(serde_json::json!({ "iri": HOSP })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["declaredBy"], "system");
}

#[tokio::test]
async fn an_empty_iri_is_refused() {
    // An empty prefix strips from every IRI, so it would match everything and
    // win longest-prefix against nothing — resolution would stop meaning
    // anything at all.
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/namespaces",
        "system",
        Some(serde_json::json!({ "iri": "   " })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn an_unknown_field_is_refused_rather_than_ignored() {
    // `deny_unknown_fields`: a caller who sent `{"code": 5}` believing it
    // chose the code must be told it did not, rather than silently getting
    // an allocated one and a false belief about what its manifest controls.
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/namespaces",
        "system",
        Some(serde_json::json!({ "iri": HOSP, "code": 5 })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn the_list_is_readable_and_starts_empty() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(&app, "GET", "/namespaces", "system", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body.as_array().map(Vec::len),
        Some(0),
        "a fresh deployment ships no runtime namespaces: {body}"
    );
}

#[tokio::test]
async fn a_non_admin_cannot_declare_but_can_read() {
    // Declaring is permanent — a code is never reissued, because every flake
    // written while it was live still carries it — so an unprivileged caller
    // who could mint them could exhaust the range irreversibly. *Reading* is
    // not privileged: the list is the vocabulary this deployment
    // understands, which anyone writing a query needs.
    let (app, _db, _catalog) = authorization_fixture().await;

    let (declare, _) = send(
        &app,
        "POST",
        "/namespaces",
        "asha",
        Some(serde_json::json!({ "iri": HOSP })),
    )
    .await;
    assert_eq!(declare, StatusCode::NOT_FOUND);

    let (read, _) = send(&app, "GET", "/namespaces", "asha", None).await;
    assert_eq!(
        read,
        StatusCode::OK,
        "reading a prefix list is not privileged"
    );
}
