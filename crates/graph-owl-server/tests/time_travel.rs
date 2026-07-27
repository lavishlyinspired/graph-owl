//! Epic 4 Slice F: `?asOf=` returns the state at a past instant.
//!
//! The differentiator, tested through HTTP. Every assertion here is about a
//! state the catalog *used to be in* being recoverable — reconstructed from
//! flakes, not read out of a snapshot table that could have drifted from the
//! facts it claims to summarise.

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
            txn_id TEXT PRIMARY KEY, amount NUMERIC(18,2) NOT NULL)",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("seed statement");
    }
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
    let response = app
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
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

    let (status, _) = send(
        &app,
        "POST",
        "/connectors/postgres/runs",
        Some(json!({
            "connectionString": connection_string,
            "serviceName": "hdfc-core",
            "includeSchemas": ["payments"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "catalogue must succeed");

    let (_, found) = send(
        &app,
        "GET",
        "/assets/search?q=upi_transactions&kind=table",
        None,
    )
    .await;
    let id = found["data"][0]["id"]
        .as_str()
        .expect("the table should be catalogued")
        .to_string();

    (app, container, id)
}

/// A timestamp far enough ahead that every write so far precedes it, without
/// depending on how long the test took.
fn now_plus_a_moment() -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339()
}

#[tokio::test]
async fn as_of_now_returns_the_current_state() {
    let (app, _container, id) = fixture().await;

    let (status, historical) = send(
        &app,
        "GET",
        &format!("/assets/{id}?asOf={}", urlencode(&now_plus_a_moment())),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let (_, current) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(historical["name"], current["name"]);
    assert_eq!(
        historical["fullyQualifiedName"],
        current["fullyQualifiedName"]
    );
}

/// **The demo moment.** Edit a description, then ask what it said before.
#[tokio::test]
async fn as_of_before_an_edit_returns_what_the_field_used_to_say() {
    let (app, _container, id) = fixture().await;

    let (_, original) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "the original description" })),
    )
    .await;
    assert_eq!(original["description"], "the original description");

    let between = now_plus_a_moment();
    // A second of separation, so the two edits land at distinguishable
    // instants rather than racing inside one clock tick.
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;

    let (_, edited) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "the corrected description" })),
    )
    .await;
    assert_eq!(edited["description"], "the corrected description");

    let (status, historical) = send(
        &app,
        "GET",
        &format!("/assets/{id}?asOf={}", urlencode(&between)),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        historical["description"], "the original description",
        "history must be recoverable — this is the whole claim of the flake model"
    );

    // And the present is unchanged by having asked about the past.
    let (_, current) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(current["description"], "the corrected description");
}

/// Before anything existed, the answer is "not found" — not an empty asset and
/// not the current one. A time-travel query that silently fell back to the
/// present would be the most dangerous possible failure of this feature.
#[tokio::test]
async fn as_of_before_the_catalog_existed_is_not_found() {
    let (app, _container, id) = fixture().await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{id}?asOf={}", urlencode("2001-01-01T00:00:00Z")),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], "https://graph-owl.dev/errors/not-found");
}

#[tokio::test]
async fn a_malformed_as_of_is_rejected_by_name() {
    let (app, _container, id) = fixture().await;

    let (status, body) = send(&app, "GET", &format!("/assets/{id}?asOf=yesterday"), None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["errors"][0]["field"], "asOf");
}

/// The version at a past instant is the version that was current then, not the
/// one that is current now. This is what makes the time slider honest about
/// *which* revision it is showing.
#[tokio::test]
async fn as_of_reports_the_version_that_was_current_then() {
    let (app, _container, id) = fixture().await;

    let (_, first) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "first" })),
    )
    .await;
    let early_version = first["version"].clone();

    let between = now_plus_a_moment();
    tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;

    send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "second" })),
    )
    .await;

    let (_, historical) = send(
        &app,
        "GET",
        &format!("/assets/{id}?asOf={}", urlencode(&between)),
        None,
    )
    .await;

    assert_eq!(
        historical["version"], early_version,
        "the version must be the one in force at that instant"
    );
}

fn urlencode(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            ':' => "%3A".to_string(),
            '+' => "%2B".to_string(),
            _ => c.to_string(),
        })
        .collect()
}
