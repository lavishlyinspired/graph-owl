mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn seed_source(connection_string: &str) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("source connection");
    for statement in [
        "CREATE SCHEMA IF NOT EXISTS payments",
        "CREATE TABLE IF NOT EXISTS payments.upi_transactions (
            txn_id TEXT PRIMARY KEY, rrn CHAR(12) NOT NULL, amount NUMERIC(18,2) NOT NULL)",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("seed statement");
    }
}

async fn catalogue(app: &axum::Router, connection_string: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/connectors/postgres/runs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "connectionString": connection_string,
                        "serviceName": "hdfc-core",
                        "includeSchemas": ["payments"]
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

async fn table_id(app: &axum::Router) -> String {
    let (_, found) = send(
        app,
        "GET",
        "/assets/search?q=upi_transactions&kind=table",
        None,
    )
    .await;
    found["data"][0]["id"]
        .as_str()
        .expect("the table should be catalogued")
        .to_string()
}

async fn fixture() -> (
    axum::Router,
    testcontainers_modules::testcontainers::ContainerAsync<
        testcontainers_modules::postgres::Postgres,
    >,
    String,
) {
    let (app, container, connection_string) = test_app().await;
    seed_source(&connection_string).await;
    catalogue(&app, &connection_string).await;
    let id = table_id(&app).await;
    (app, container, id)
}

#[tokio::test]
async fn a_freshly_catalogued_asset_starts_at_zero_one() {
    let (app, _container, id) = fixture().await;

    let (status, asset) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(asset["version"], json!({ "major": 0, "minor": 1 }));
    assert!(
        asset["changeDescription"].is_null(),
        "the initial version has no diff — there was nothing before it, and an \
         empty diff would read as 'nothing changed' rather than 'this is where it began'"
    );
}

#[tokio::test]
async fn editing_a_description_bumps_the_minor_and_records_the_diff() {
    let (app, _container, id) = fixture().await;

    let (status, updated) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "UPI transaction ledger, NPCI-settled." })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["version"], json!({ "major": 0, "minor": 2 }));
    assert_eq!(
        updated["description"],
        "UPI transaction ledger, NPCI-settled."
    );

    let added = updated["changeDescription"]["fieldsAdded"]
        .as_array()
        .expect("fieldsAdded");
    assert_eq!(added.len(), 1, "got {added:?}");
    assert_eq!(added[0]["field"], "description");
    assert_eq!(
        added[0]["after"], "UPI transaction ledger, NPCI-settled.",
        "a diff without the new value cannot answer 'what does it say now'"
    );
}

/// The property that makes a nightly connector safe, and the reason
/// `ChangeKind::None` is a real outcome rather than an absence.
#[tokio::test]
async fn a_patch_that_changes_nothing_does_not_bump_the_version() {
    let (app, _container, id) = fixture().await;

    let (_, first) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "ledger" })),
    )
    .await;
    assert_eq!(first["version"], json!({ "major": 0, "minor": 2 }));

    let (status, second) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "ledger" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "a no-op is success, not an error");
    assert_eq!(
        second["version"],
        json!({ "major": 0, "minor": 2 }),
        "re-sending the same value must not inflate history"
    );

    let (_, versions) = send(&app, "GET", &format!("/assets/{id}/versions"), None).await;
    assert_eq!(
        versions.as_array().expect("versions").len(),
        1,
        "and must not write a history row either"
    );
}

#[tokio::test]
async fn version_history_lists_every_change_newest_first() {
    let (app, _container, id) = fixture().await;

    for description in ["first", "second", "third"] {
        send(
            &app,
            "PATCH",
            &format!("/assets/{id}"),
            Some(json!({ "description": description })),
        )
        .await;
    }

    let (status, versions) = send(&app, "GET", &format!("/assets/{id}/versions"), None).await;
    assert_eq!(status, StatusCode::OK);
    let versions = versions.as_array().expect("versions");
    assert_eq!(versions.len(), 3);

    assert_eq!(versions[0]["version"], json!({ "major": 0, "minor": 4 }));
    assert_eq!(versions[2]["version"], json!({ "major": 0, "minor": 2 }));
    assert_eq!(
        versions[0]["snapshot"]["description"], "third",
        "a snapshot is the state *after* the change, so reading history never \
         requires replaying diffs from the beginning"
    );
    assert_eq!(versions[1]["snapshot"]["description"], "second");
}

#[tokio::test]
async fn every_version_records_who_made_it() {
    let (app, _container, id) = fixture().await;

    send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "ledger" })),
    )
    .await;

    let (_, versions) = send(&app, "GET", &format!("/assets/{id}/versions"), None).await;
    // `system` until Epic 12 supplies a real identity through the Principal
    // seam — but the attribution path is wired now, not retrofitted later.
    assert_eq!(versions[0]["updatedBy"], "system");
}

#[tokio::test]
async fn a_soft_delete_tombstones_the_subtree_and_reports_the_count() {
    let (app, _container, id) = fixture().await;

    let (_, before) = send(&app, "GET", &format!("/assets/{id}/children"), None).await;
    let column_count = before.as_array().expect("children").len();
    assert!(column_count > 0, "the fixture table should have columns");

    let (status, result) = send(&app, "DELETE", &format!("/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        result["deleted"],
        json!(column_count + 1),
        "a live column under a tombstoned table is reachable by search and \
         addresses an asset that no longer exists"
    );

    let (_, found) = send(&app, "GET", "/assets/search?q=upi_transactions", None).await;
    assert!(
        found["data"].as_array().expect("data").is_empty(),
        "a tombstoned asset must not appear in search"
    );
}

/// The metadata is still the truth about a table that used to exist. A `404`
/// would make tombstones invisible and restore undiscoverable.
#[tokio::test]
async fn a_tombstoned_asset_is_still_readable_and_marked_deleted() {
    let (app, _container, id) = fixture().await;
    send(&app, "DELETE", &format!("/assets/{id}"), None).await;

    let (status, asset) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(asset["deleted"], true);
    assert!(asset["deletedAt"].is_string());
}

#[tokio::test]
async fn restore_lifts_the_tombstone_from_the_whole_subtree() {
    let (app, _container, id) = fixture().await;

    let (_, deleted) = send(&app, "DELETE", &format!("/assets/{id}"), None).await;
    let (status, restored) = send(&app, "POST", &format!("/assets/{id}/restore"), None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        restored["restored"], deleted["deleted"],
        "restore must return exactly what delete took, or the subtree is left half-alive"
    );

    let (_, found) = send(&app, "GET", "/assets/search?q=upi_transactions", None).await;
    assert!(!found["data"].as_array().expect("data").is_empty());
}

#[tokio::test]
async fn a_connector_rerun_does_not_resurrect_a_tombstoned_asset() {
    let (app, container, id) = fixture().await;
    let connection_string = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        container.get_host().await.expect("host"),
        container.get_host_port_ipv4(5432).await.expect("port")
    );

    send(&app, "DELETE", &format!("/assets/{id}"), None).await;
    catalogue(&app, &connection_string).await;

    let (_, asset) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(
        asset["deleted"], true,
        "deletion is a governance decision; a connector must not silently reverse it"
    );
}

#[tokio::test]
async fn patching_a_nonexistent_asset_is_a_404() {
    let (app, _container, _id) = fixture().await;

    let (status, body) = send(
        &app,
        "PATCH",
        "/assets/00000000-0000-0000-0000-000000000000",
        Some(json!({ "description": "x" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], "https://graph-owl.dev/errors/not-found");
}

#[tokio::test]
async fn a_blank_description_is_rejected_with_a_pointer_to_null() {
    let (app, _container, id) = fixture().await;

    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "   " })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["field"], "description");
    assert!(
        body["errors"][0]["detail"]
            .as_str()
            .expect("detail")
            .contains("null"),
        "the error must say how to actually clear the field"
    );
}
