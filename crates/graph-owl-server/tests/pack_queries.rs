//! `POST /packs/{pack}/queries` and `POST /packs/{pack}/queries/{name}/run`
//! — Epic 105 P106 Slice 4a (`plans/106-agent-trace-hygiene.md`), the
//! named-query counterpart to `/packs/{pack}/finding-rules`
//! (`reconcile.rs`): a pack's `[[queries]]` entry, registered the same way
//! and inlined the same way, but invoked by name with a caller-supplied
//! binding rather than run unconditionally as a detector.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, token};
use tower::ServiceExt;

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    content_type: &str,
    body: String,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {}", token("system")))
                .header("content-type", content_type)
                .body(Body::from(body))
                .expect("request should build"),
        )
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

async fn json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    call(app, method, uri, "application/json", body.to_string()).await
}

/// One subject, one predicate, one object — enough to prove the round
/// trip without depending on any pack's real vocabulary.
async fn seed_one_fact(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/test-pack#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "namespace declare");

    let (status, body) = json(
        app,
        "POST",
        "/predicates",
        serde_json::json!({"namespace": 1024, "name": "status", "valueType": 1, "many": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "predicate: {body}");

    let turtle = r#"
        @prefix tp: <https://graph-owl.dev/packs/test-pack#> .

        tp:subject-one tp:status "open" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=test-pack&format=turtle",
        "text/turtle",
        turtle.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "import: {body}");
}

// `GRAPH ?g { }` is required, not decorative: `/graph/import/rdf?source=`
// lands data in a *named* graph (`graph:import:{source}`), and a pattern
// with no `GRAPH` clause reads only the *default* graph — the same
// semantics `plans/105-domain-neutrality.md` documents and every real GST
// pack query already follows. Found live while writing this test: a plain
// `VALUES ?subject { <{{subject}}> } ?subject <p> ?o` (no `GRAPH` clause)
// scanned the fact (`factsScanned: 1`) but returned zero rows, because the
// scan and the evaluator both correctly stayed inside the default graph —
// where nothing imported via `/graph/import/rdf` ever lands.
const STATUS_OF: &str = "SELECT ?status WHERE { GRAPH ?g { \
                          VALUES ?subject { <{{subject}}> } \
                          ?subject <https://graph-owl.dev/packs/test-pack#status> ?status } }";

async fn register_status_of_query(app: &axum::Router) {
    let (status, body) = json(
        app,
        "POST",
        "/packs/test-pack/queries",
        serde_json::json!({
            "queries": [{"name": "status-of", "query": STATUS_OF}],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register query: {body}");
}

#[tokio::test]
async fn a_registered_query_run_with_a_binding_returns_the_bound_answer() {
    let (app, _db, _url) = test_app().await;
    seed_one_fact(&app).await;
    register_status_of_query(&app).await;

    let (status, outcome) = json(
        &app,
        "POST",
        "/packs/test-pack/queries/status-of/run",
        serde_json::json!({
            "bindings": {"subject": "https://graph-owl.dev/packs/test-pack#subject-one"},
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{outcome}");
    let rows = outcome["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 1, "{outcome}");
    // A raw SELECT row renders a string literal with its own quoting kept
    // (`"open"`, not `open`) — the same convention every other raw
    // `/sparql` row in this project's tests observes; this only ever
    // differs downstream, where `findings_from_rows` strips it.
    assert_eq!(rows[0]["status"], "\"open\"", "{outcome}");
    // Slice 2's diagnostics ride the same `query_outcome_json` envelope
    // `/sparql` uses — a real scan happened, not a stub.
    assert!(outcome["factsScanned"].is_u64(), "{outcome}");
}

/// **The negative that proves substitution, not luck.** A binding for a
/// subject with no `status` fact must come back empty, not the fact for a
/// *different* subject the query happens to have seen.
#[tokio::test]
async fn a_binding_naming_a_different_subject_gets_that_subjects_own_answer() {
    let (app, _db, _url) = test_app().await;
    seed_one_fact(&app).await;
    register_status_of_query(&app).await;

    let (status, outcome) = json(
        &app,
        "POST",
        "/packs/test-pack/queries/status-of/run",
        serde_json::json!({
            "bindings": {"subject": "https://graph-owl.dev/packs/test-pack#subject-nonexistent"},
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{outcome}");
    let rows = outcome["rows"].as_array().expect("rows array");
    assert!(rows.is_empty(), "{outcome}");
}

/// A binding naming a placeholder the query never declared is the
/// caller's mistake, not a 500 — matching Slice 4a's own acceptance
/// criterion.
#[tokio::test]
async fn a_non_existent_binding_is_a_validation_error_not_a_500() {
    let (app, _db, _url) = test_app().await;
    seed_one_fact(&app).await;
    register_status_of_query(&app).await;

    let (status, body) = json(
        &app,
        "POST",
        "/packs/test-pack/queries/status-of/run",
        serde_json::json!({"bindings": {"nonExistentPlaceholder": "urn:x"}}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn running_an_unregistered_query_name_is_not_found() {
    let (app, _db, _url) = test_app().await;
    seed_one_fact(&app).await;

    let (status, _) = json(
        &app,
        "POST",
        "/packs/test-pack/queries/never-registered/run",
        serde_json::json!({"bindings": {}}),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
