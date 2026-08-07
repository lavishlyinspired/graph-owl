//! HTTP surface for the portable archive — Epic 37b.
//!
//! The export/restore *logic* is exhaustively covered at the `Catalog`
//! facade level (`graph-owl-api`'s own `archive_round_trip_tests`, no
//! container needed); what only an HTTP-level test can prove is the wiring
//! this crate itself adds — the admin gate, JSON/query-param parsing, and a
//! raw-body response actually carrying real archive bytes over the wire.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{authorization_fixture as fixture, token};
use tower::ServiceExt;

#[tokio::test]
async fn a_non_admin_is_refused_export_as_a_not_found() {
    let (app, _container, _catalog) = fixture().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/export")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token("asha")))
                .body(Body::from("{}"))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "an unlisted admin surface must read as not-found to a non-admin, \
         matching every other admin-only route in this crate"
    );
}

#[tokio::test]
async fn a_non_admin_is_refused_restore_as_a_not_found() {
    let (app, _container, _catalog) = fixture().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/restore")
                .header("authorization", format!("Bearer {}", token("asha")))
                .body(Body::from(vec![0u8; 4]))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// **The end-to-end wire path.** An admin exports the seeded estate over
/// HTTP, gets back real `.tar.zst` bytes, and restoring those same bytes
/// (with `skip`, since the estate is still live) reports every entity as
/// already-present rather than erroring — proving the JSON request body,
/// the raw binary response, the query-string conflict policy, and the raw
/// binary request body all round-trip through real HTTP.
#[tokio::test]
async fn an_admin_exports_and_restores_over_real_http() {
    let (app, _container, _catalog) = fixture().await;

    let export_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/export")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::from("{}"))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(export_response.status(), StatusCode::OK);
    assert_eq!(
        export_response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/zstd")
    );
    let archive_bytes = axum::body::to_bytes(export_response.into_body(), usize::MAX)
        .await
        .expect("read export body");
    assert!(
        !archive_bytes.is_empty(),
        "the seeded estate is non-empty; the archive must not be either"
    );

    let restore_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/restore?conflictPolicy=skip")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::from(archive_bytes))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(restore_response.status(), StatusCode::OK);
    let outcome: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(restore_response.into_body(), usize::MAX)
            .await
            .expect("read restore body"),
    )
    .expect("restore response should be JSON");
    assert_eq!(outcome["entitiesRestored"], 0, "{outcome:?}");
    assert!(
        outcome["entitiesSkipped"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "every already-live entity should be reported skipped: {outcome:?}"
    );
}

/// Epic 37a: found generating a real scale corpus, not designed in
/// advance. axum's default body limit is 2 MiB; a 60,000-table corpus
/// (well short of the plan's 100,000-entity target) compresses to ~10 MiB
/// and was rejected outright with `413` before this route's limit was
/// raised — a backup/restore feature that cannot hold a real backup.
#[tokio::test]
async fn a_restore_body_over_two_megabytes_is_not_rejected_for_size() {
    let (app, _container, _catalog) = fixture().await;

    // Junk, not a real archive: this test is only about the body-size
    // gate, which runs before the archive is ever parsed. 3 MiB clears
    // axum's default 2 MiB limit and stays well under the raised one.
    let oversized_junk = vec![0u8; 3 * 1024 * 1024];

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/restore")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::from(oversized_junk))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_ne!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "a 3 MiB body must not be rejected on size alone; it should fail \
         later, on being an invalid archive"
    );
}
