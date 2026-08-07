//! `GET /assets/{id}/lpg-node` — Epic 42 Slice E's Knowledge tab toggle.
//! Authorization scoping itself is already proven exhaustively at the
//! `graph-owl-api` level (`export_authorization_tests::lpg_node_for_tests`,
//! no Docker needed). What is left to prove here is the HTTP plumbing: the
//! route exists, is wired to `Catalog::lpg_node_for`, and denial reads as
//! `404` the same way every other per-asset read in this server does.

mod common;

use axum::http::StatusCode;
use common::{authorization_fixture, call};

#[tokio::test]
async fn returns_the_authorized_asset_as_a_node_with_its_mapping_report() {
    let (app, _container, _catalog) = authorization_fixture().await;

    let (_, found) = call(&app, "/assets/search?q=upi&limit=10", Some("root")).await;
    let id = found["data"][0]["id"].as_str().expect("id").to_string();

    let (status, body) = call(&app, &format!("/assets/{id}/lpg-node"), Some("asha")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["node"]["labels"]
            .as_array()
            .is_some_and(|l| !l.is_empty()),
        "{body}"
    );
    // A cataloged table's own parent reference is exactly the kind of loss
    // Slice E's UI exists to name on screen, not a fixture artifact.
    assert!(
        body["report"]["lossy"]
            .as_array()
            .is_some_and(|l| !l.is_empty()),
        "a real, hierarchical asset should report at least one lossy mapping: {body}"
    );
}

#[tokio::test]
async fn a_denied_asset_reads_as_not_found_not_forbidden() {
    let (app, _container, _catalog) = authorization_fixture().await;

    let (_, found) = call(&app, "/assets/search?q=customers&limit=10", Some("root")).await;
    let hidden_id = found["data"][0]["id"].as_str().expect("id").to_string();

    let (status, body) = call(&app, &format!("/assets/{hidden_id}/lpg-node"), Some("asha")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["type"], "https://graph-owl.dev/errors/not-found");

    let (admin_status, _) =
        call(&app, &format!("/assets/{hidden_id}/lpg-node"), Some("root")).await;
    assert_eq!(
        admin_status,
        StatusCode::OK,
        "the same id is readable by someone who may see it — proving it exists"
    );
}

#[tokio::test]
async fn a_nonexistent_asset_is_404() {
    let (app, _container, _catalog) = authorization_fixture().await;

    let (status, _) = call(
        &app,
        &format!("/assets/{}/lpg-node", uuid::Uuid::new_v4()),
        Some("root"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
