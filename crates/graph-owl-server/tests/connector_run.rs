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

/// **Decision 7, end to end.** A re-run against an unchanged source must not
/// write. Decision 3 already made a re-run *converge*; this is what makes it
/// *cheap*, and the skip count is the only way an operator can tell a run that
/// wrote nothing because nothing changed from one that wrote nothing because it
/// was broken.
#[tokio::test]
async fn a_rerun_against_an_unchanged_source_skips_every_record() {
    let (app, _database, connection_string) = test_app().await;
    seed_source(&connection_string).await;

    let first = run_connector(&app, &connection_string).await;
    assert!(first["created"].as_u64().unwrap() > 0, "{first}");
    assert_eq!(first["skipped"], 0, "nothing is fingerprinted yet: {first}");

    let second = run_connector(&app, &connection_string).await;

    assert_eq!(
        second["created"], 0,
        "an unchanged source must produce no writes: {second}"
    );
    assert_eq!(
        second["skipped"], first["created"],
        "every record the first run created must be skipped by the second: {second}"
    );
    assert_eq!(second["failed"], 0, "{second}");
}

/// And the negative that stops "skip everything" from passing: a source that
/// *has* changed is written, not skipped.
#[tokio::test]
async fn a_changed_source_is_written_rather_than_skipped() {
    let (app, _database, connection_string) = test_app().await;
    seed_source(&connection_string).await;
    run_connector(&app, &connection_string).await;

    // A new column is a record the catalog has never seen.
    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("source connection");
    sqlx::query("ALTER TABLE sales.orders ADD COLUMN settled_at TIMESTAMPTZ")
        .execute(&pool)
        .await
        .expect("add a column");

    let second = run_connector(&app, &connection_string).await;

    assert_eq!(
        second["created"], 1,
        "the new column must be catalogued: {second}"
    );
    assert!(
        second["skipped"].as_u64().unwrap() > 0,
        "and everything unchanged must still skip: {second}"
    );
}

/// **A run leaves a record.** Before this, the report went back in the HTTP
/// response and nowhere else, so "did last night's sync work" was unanswerable
/// the moment the caller closed the connection.
#[tokio::test]
async fn a_run_is_recorded_in_history() {
    let (app, _database, connection_string) = test_app().await;
    seed_source(&connection_string).await;

    let report = run_connector(&app, &connection_string).await;
    let history = get(&app, "/connectors/runs").await;

    let runs = history.as_array().expect("an array of runs");
    assert_eq!(runs.len(), 1, "{history}");
    assert_eq!(
        runs[0]["id"], report["runId"],
        "the report names the row it wrote"
    );
    assert_eq!(runs[0]["serviceName"], "warehouse");
    assert_eq!(runs[0]["connector"], "postgres");
    assert_eq!(runs[0]["created"], report["created"]);
    assert_eq!(runs[0]["failed"], 0);
    assert!(
        runs[0]["finishedAt"].is_string(),
        "a completed run has an ending: {history}"
    );
    assert!(runs[0]["triggeredBy"].is_string(), "{history}");
}

/// Newest first, because history is read as a timeline. A second run must not
/// be hidden behind the first, and the skip count must distinguish them.
#[tokio::test]
async fn history_is_newest_first_and_distinguishes_the_runs() {
    let (app, _database, connection_string) = test_app().await;
    seed_source(&connection_string).await;

    let first = run_connector(&app, &connection_string).await;
    let second = run_connector(&app, &connection_string).await;

    let history = get(&app, "/connectors/runs").await;
    let runs = history.as_array().expect("an array");

    assert_eq!(runs.len(), 2, "{history}");
    assert_eq!(runs[0]["id"], second["runId"], "newest first: {history}");
    assert_eq!(runs[1]["id"], first["runId"]);
    // The second run wrote nothing because nothing changed, and the row says
    // so — which is the whole reason `skipped` is stored beside `created`.
    assert_eq!(runs[0]["created"], 0, "{history}");
    assert!(runs[0]["skipped"].as_u64().unwrap() > 0, "{history}");
}

/// And the negative: a service filter must actually filter, or the parameter
/// is a lie that looks like it worked.
#[tokio::test]
async fn history_can_be_narrowed_to_one_service() {
    let (app, _database, connection_string) = test_app().await;
    seed_source(&connection_string).await;
    run_connector(&app, &connection_string).await;

    let mine = get(&app, "/connectors/runs?serviceName=warehouse").await;
    let theirs = get(&app, "/connectors/runs?serviceName=something-else").await;

    assert_eq!(mine.as_array().expect("array").len(), 1, "{mine}");
    assert!(
        theirs.as_array().expect("array").is_empty(),
        "another service's history is not this one's: {theirs}"
    );
}
