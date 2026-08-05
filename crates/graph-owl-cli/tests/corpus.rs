//! The corpus generator's own end-to-end proof — Epic 37a Slice A.
//!
//! **What this proves that a pure unit test cannot**: the "reuse Epic 37b's
//! `restore` as the loader, invent no second bulk-write path" design
//! decision is only real if the bytes `corpus::write_archive` produces are
//! genuinely restorable by the real server — not merely shaped like an
//! archive to this crate's own eyes.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread")]
async fn a_generated_corpus_restores_into_a_real_catalog() {
    let (app, _db) = common::test_app().await;

    let corpus = graph_owl_cli::corpus::generate(5, 50);
    let expected_entities = corpus.entities.len();
    let expected_relationships = corpus.relationships.len();
    assert!(expected_entities > 0, "the corpus should not be empty");

    let archive_path = std::env::temp_dir().join(format!(
        "graph-owl-corpus-e2e-{}.tar.zst",
        uuid::Uuid::new_v4()
    ));
    graph_owl_cli::corpus::write_archive(&corpus, &archive_path).expect("write archive");
    let bytes = std::fs::read(&archive_path).expect("read archive back");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/restore?conflictPolicy=fail")
                .body(Body::from(bytes))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let outcome: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body"),
    )
    .expect("restore response should be JSON");

    assert_eq!(
        outcome["entitiesRestored"],
        serde_json::json!(expected_entities),
        "{outcome:?}"
    );
    assert_eq!(
        outcome["relationshipsRestored"],
        serde_json::json!(expected_relationships),
        "{outcome:?}"
    );
    assert_eq!(outcome["aborted"], serde_json::json!(false), "{outcome:?}");

    std::fs::remove_file(&archive_path).ok();
}
