//! Epic 98, at the HTTP surface — **Phase 1.3 of
//! `plans/EPIC-COMPLETION-PLAN.md`**.
//!
//! `Catalog::classify_ontology`/`explain_subsumption` were correct and
//! tested since this epic shipped, but no route ever called either one —
//! EL classification was unreachable in a running deployment. This proves
//! the routes exist and dispatch to the real `Catalog` methods.
//!
//! **No `whelk` binary is assumed to be installed** — this environment
//! does not have one (it is a separate external process, never linked, per
//! `98-owl-el-reasoning.md`'s own licensing note), so `test_app()`'s
//! `Catalog` has no sidecar configured, matching `main.rs`'s own default
//! (`GRAPH_OWL_EL_SIDECAR` unset). That is itself a real path worth
//! proving: the named `Validation` error, not a generic failure, is what a
//! deployment that has not configured a sidecar actually gets back.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use tower::ServiceExt;

#[tokio::test]
async fn classify_without_a_configured_sidecar_names_the_missing_configuration() {
    let (app, _database, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/reasoning/el/classify")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "not a 404 — the route exists"
    );
    let body = json_body(response).await;
    assert_eq!(body["errors"][0]["field"], "sidecar", "{body}");
    assert!(
        body["errors"][0]["detail"]
            .as_str()
            .is_some_and(|d| d.contains("sidecar")),
        "the error must name what is missing, not just fail generically: {body}"
    );
}

#[tokio::test]
async fn explain_with_a_malformed_sid_is_a_named_validation_error() {
    let (app, _database, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/reasoning/el/explain?subclass=not-a-sid&superclass=1:Thing")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert!(
        body["errors"][0]["field"] == "subclass",
        "the offending field must be named: {body}"
    );
}

#[tokio::test]
async fn explain_for_a_subsumption_that_does_not_hold_is_not_found() {
    let (app, _database, _connection_string) = test_app().await;

    // No `TBox` seeded at all — `subclass_of` is empty, so no path can
    // exist between these two synthetic classes. No sidecar is needed for
    // this path: `explain_subsumption` re-derives locally over asserted
    // edges, it never calls `whelk`.
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/reasoning/el/explain?subclass=1:Dog&superclass=1:Animal")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
