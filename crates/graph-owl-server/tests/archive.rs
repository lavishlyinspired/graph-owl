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
