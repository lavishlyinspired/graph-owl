//! Epic 104 Slice D, at the HTTP surface — **Phase 1.7 of
//! `plans/EPIC-COMPLETION-PLAN.md`**.
//!
//! `Catalog::upsert_alignment`/`pending_alignment_review` were correct and
//! tested since this slice shipped, but had no route at all — nothing
//! could write a curated or human-confirmed alignment, or read the review
//! queue, over the real API.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;

async fn post_alignment(
    app: &axum::Router,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/alignments")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

async fn get_review_queue(app: &axum::Router) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/alignments/review")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

fn curated_match(left: &str, right: &str, confidence: f64) -> serde_json::Value {
    json!({
        "kind": "match",
        "left": left,
        "right": right,
        "predicate": "exactMatch",
        "source": { "kind": "curated", "detail": "UMLS" },
        "confidence": confidence,
    })
}

#[tokio::test]
async fn a_curated_match_above_the_review_band_is_written_and_not_in_the_review_queue() {
    let (app, _database, _connection_string) = test_app().await;

    let (status, body) = post_alignment(&app, curated_match("1:snomed-a", "1:rxnorm-a", 0.9)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "written", "{body}");

    let queue = get_review_queue(&app).await;
    assert!(
        queue
            .as_array()
            .expect("array")
            .iter()
            .all(|e| e["left"] != "1:snomed-a"),
        "a confidence above the review band must not sit in the queue: {queue}"
    );
}

#[tokio::test]
async fn a_computed_match_in_the_review_band_lands_in_the_review_queue_resolved() {
    let (app, _database, _connection_string) = test_app().await;

    let (status, body) = post_alignment(
        &app,
        json!({
            "kind": "match",
            "left": "1:snomed-b",
            "right": "1:rxnorm-b",
            "predicate": "closeMatch",
            "source": { "kind": "computed", "detail": "embedding-cosine" },
            "confidence": 0.62,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "written", "{body}");

    let queue = get_review_queue(&app).await;
    let entries = queue.as_array().expect("array");
    let entry = entries
        .iter()
        .find(|e| e["left"] == "1:snomed-b")
        .unwrap_or_else(|| {
            panic!("the computed alignment must appear in the review queue: {queue}")
        });
    assert_eq!(entry["right"], "1:rxnorm-b", "{entry}");
    assert_eq!(entry["sourceKind"], "computed", "{entry}");
    assert_eq!(entry["sourceDetail"], "embedding-cosine", "{entry}");
    assert_eq!(entry["confidence"], 0.62, "{entry}");
    assert_eq!(entry["lossyReverse"], false, "{entry}");
    // Phase 3 item 3.14: without this, a reviewer confirming this entry
    // (re-POSTing it with source.kind: "human") has no way to know which
    // skos:*Match to name — closeMatch here, not always exactMatch.
    assert_eq!(entry["predicate"], "closeMatch", "{entry}");
}

/// Decision 3, at the request boundary: `AssertableSource` (what
/// `owl:equivalentClass` may carry) has no `Computed` variant at all, so
/// the type system refuses this once construction is reached — this test
/// proves the HTTP layer catches it *before* that point, naming the field,
/// rather than the request failing in some less legible way.
#[tokio::test]
async fn a_computed_source_can_never_assert_equivalent_class() {
    let (app, _database, _connection_string) = test_app().await;

    let (status, body) = post_alignment(
        &app,
        json!({
            "kind": "equivalentClass",
            "left": "1:class-a",
            "right": "1:class-b",
            "source": { "kind": "computed", "detail": "embedding-cosine" },
            "confidence": 0.95,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "source.kind", "{body}");
}

#[tokio::test]
async fn a_curated_equivalent_class_is_accepted() {
    let (app, _database, _connection_string) = test_app().await;

    let (status, body) = post_alignment(
        &app,
        json!({
            "kind": "equivalentClass",
            "left": "1:class-c",
            "right": "1:class-d",
            "source": { "kind": "curated", "detail": "published-crosswalk" },
            "confidence": 0.95,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["outcome"], "written", "{body}");
}

#[tokio::test]
async fn match_without_a_predicate_is_a_named_validation_error() {
    let (app, _database, _connection_string) = test_app().await;
    let (status, body) = post_alignment(
        &app,
        json!({
            "kind": "match",
            "left": "1:snomed-c",
            "right": "1:rxnorm-c",
            "source": { "kind": "curated", "detail": "UMLS" },
            "confidence": 0.9,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "predicate", "{body}");
}
