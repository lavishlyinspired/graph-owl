mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

/// Seeds a warehouse in the *source* database, then catalogs it. Uses the same
/// container as the catalog, which is fine and even useful: it proves the
/// connector reads through `information_schema` rather than peeking at
/// graph-owl's own tables.
async fn seed_source(connection_string: &str) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("source connection");
    for statement in [
        "CREATE SCHEMA IF NOT EXISTS sales",
        "CREATE TABLE IF NOT EXISTS sales.orders (
            id BIGINT PRIMARY KEY,
            customer_id BIGINT NOT NULL,
            total NUMERIC(12,2),
            placed_at TIMESTAMPTZ NOT NULL
         )",
        "CREATE TABLE IF NOT EXISTS sales.customers (
            id BIGINT PRIMARY KEY,
            email TEXT NOT NULL,
            country TEXT
         )",
        "CREATE OR REPLACE VIEW sales.recent_orders AS
            SELECT id, customer_id FROM sales.orders",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("seed statement");
    }
}

async fn run_connector(app: &axum::Router, connection_string: &str) -> Value {
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
                        "serviceName": "warehouse",
                        "includeSchemas": ["sales"]
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn get(app: &axum::Router, uri: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK, "GET {uri}");
    json_body(response).await
}

#[tokio::test]
async fn a_connector_run_populates_the_full_hierarchy() {
    let (app, _container, connection_string) = test_app().await;
    seed_source(&connection_string).await;

    let summary = run_connector(&app, &connection_string).await;
    assert_eq!(summary["failed"], 0, "no record should fail: {summary}");
    assert!(summary["created"].as_u64().expect("created") > 0);

    // One service at the root, addressable by the name the run supplied.
    let roots = get(&app, "/assets/roots").await;
    let roots = roots.as_array().expect("roots array");
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0]["kind"], "service");
    assert_eq!(roots[0]["name"], "warehouse");

    // The FQN is derived from the parent chain, so a table addresses as
    // service.database.schema.table without anyone constructing that string.
    let orders = get(&app, "/assets/search?q=orders&kind=table").await;
    let orders = orders["data"].as_array().expect("data");
    let orders = orders
        .iter()
        .find(|a| a["name"] == "orders")
        .expect("orders table should be catalogued");
    assert!(
        orders["fullyQualifiedName"]
            .as_str()
            .expect("fqn")
            .ends_with(".sales.orders"),
        "got {}",
        orders["fullyQualifiedName"]
    );

    // Columns arrive with the type information a catalog exists to carry.
    let columns = get(
        &app,
        &format!("/assets/{}/children", orders["id"].as_str().unwrap()),
    )
    .await;
    let columns = columns.as_array().expect("children");
    let names: Vec<&str> = columns
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"customer_id"), "got {names:?}");
    assert!(names.contains(&"placed_at"), "got {names:?}");
    let placed_at = columns
        .iter()
        .find(|c| c["name"] == "placed_at")
        .expect("placed_at");
    assert_eq!(
        placed_at["properties"]["dataType"], "timestamp with time zone",
        "a column without its type is a name, not metadata"
    );
    assert_eq!(placed_at["properties"]["nullable"], false);
}

/// A view is a real asset with real lineage. Filtering it out would make the
/// graph wrong rather than smaller — so it is catalogued and *marked*.
#[tokio::test]
async fn views_are_catalogued_and_distinguishable_from_tables() {
    let (app, _container, connection_string) = test_app().await;
    seed_source(&connection_string).await;
    run_connector(&app, &connection_string).await;

    let found = get(&app, "/assets/search?q=recent_orders").await;
    let view = found["data"]
        .as_array()
        .expect("data")
        .iter()
        .find(|a| a["name"] == "recent_orders")
        .expect("the view should be catalogued");
    assert_eq!(view["properties"]["tableType"], "VIEW");
}

/// The property that makes a scheduled connector safe: a second run over an
/// unchanged source must converge, not duplicate. Without it, a nightly job
/// doubles the catalog every night.
#[tokio::test]
async fn a_second_run_converges_instead_of_duplicating() {
    let (app, _container, connection_string) = test_app().await;
    seed_source(&connection_string).await;

    run_connector(&app, &connection_string).await;
    let after_first = get(&app, "/assets/stats").await;

    run_connector(&app, &connection_string).await;
    let after_second = get(&app, "/assets/stats").await;

    assert_eq!(
        after_first, after_second,
        "a re-run over an unchanged source must change nothing"
    );
}

/// Breadcrumbs, and the proof that containment actually holds: walking up from
/// a column reaches the service through exactly the expected kinds.
#[tokio::test]
async fn ancestors_walk_from_a_column_to_the_service_root() {
    let (app, _container, connection_string) = test_app().await;
    seed_source(&connection_string).await;
    run_connector(&app, &connection_string).await;

    let columns = get(&app, "/assets/search?q=customer_id&kind=column").await;
    let column = &columns["data"].as_array().expect("data")[0];

    let ancestors = get(
        &app,
        &format!("/assets/{}/ancestors", column["id"].as_str().unwrap()),
    )
    .await;
    let kinds: Vec<&str> = ancestors
        .as_array()
        .expect("ancestors")
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();

    assert_eq!(
        kinds,
        vec!["service", "database", "schema", "table", "column"],
        "root-first, which is the order a breadcrumb renders in"
    );
}

#[tokio::test]
async fn a_system_schema_is_not_catalogued() {
    let (app, _container, connection_string) = test_app().await;
    seed_source(&connection_string).await;
    run_connector(&app, &connection_string).await;

    let found = get(&app, "/assets/search?q=information_schema").await;
    assert!(
        found["data"].as_array().expect("data").is_empty(),
        "cataloguing system schemas buries real assets under internal ones"
    );
}
