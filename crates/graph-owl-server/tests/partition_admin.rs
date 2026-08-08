//! Epic 102, at the HTTP surface — **Phase 1.5/1.6 of
//! `plans/EPIC-COMPLETION-PLAN.md`**.
//!
//! `PostgresTripleStore::compact`/`partition_health` (via the new
//! `TripleStore` trait methods) were correct and tested at the engine
//! level (`crates/graph-owl-engine-postgres/tests/partition_split.rs`),
//! but had no `Catalog` wrapper and no route — a real deployment had no
//! way to ever fold `flakes_delta` into `flakes_main`, or even see how far
//! behind it had fallen.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use tower::ServiceExt;

fn dsc(id: &str) -> Sid {
    Sid::dsc(id)
}
fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

async fn get_partition_health(app: &axum::Router) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/admin/partition-health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn a_fresh_write_shows_up_in_the_delta_backlog_and_compaction_clears_it() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;

    // Every write lands in `flakes_delta` by design (Epic 102's own
    // architecture) — no special fixture is needed to populate it, an
    // ordinary assert already does.
    let t = graph_owl_engine::TripleStore::next_time(&store)
        .await
        .expect("a transaction time");
    graph_owl_engine::TripleStore::assert_flakes(
        &store,
        &[Flake::assert(
            dsc("orders"),
            rdf_type(),
            FlakeValue::Ref(dsc("Table")),
            t,
        )],
    )
    .await
    .expect("seed a flake");

    let before = get_partition_health(&app).await;
    assert!(
        before["deltaRows"].as_u64().is_some_and(|n| n >= 1),
        "the write above must be visible in the backlog: {before}"
    );
    assert!(!before["oldestDeltaT"].is_null(), "{before}");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/compact")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    let compacted = json_body(response).await;
    assert!(
        compacted["moved"].as_u64().is_some_and(|n| n >= 1),
        "{compacted}"
    );

    let after = get_partition_health(&app).await;
    assert_eq!(
        after["deltaRows"], 0,
        "compaction must actually empty the backlog it reported: {after}"
    );
    assert!(after["oldestDeltaT"].is_null(), "{after}");
}

#[tokio::test]
async fn compacting_an_empty_delta_moves_nothing_and_does_not_error() {
    let (app, _database, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/compact")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["moved"], 0, "{body}");
}
