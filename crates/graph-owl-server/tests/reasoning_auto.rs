//! Epic 97 decision 4.4's server-tracked retraction watermark, at the HTTP
//! surface — Phase 3 items 1.9 (automatic path) and 3.11 (`maintainedTo`
//! visible on the wire).
//!
//! `reasoning_incremental.rs` proves the explicit, caller-supplied
//! `retracted` list still works unchanged. This file proves the *other*
//! path decision 4.4 actually asked for: an empty-body `POST
//! /reasoning/runs` discovers a retraction nobody told it about, by asking
//! the graph itself what changed since the last run's own watermark.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_engine::TripleStore;
use serde_json::Value;
use tower::ServiceExt;

fn dsc(id: &str) -> Sid {
    Sid::dsc(id)
}
fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}
fn sub_class_of() -> Sid {
    Sid::new(namespace::RDFS, "subClassOf")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

/// The same three-level hierarchy `reasoning.rs`/`reasoning_incremental.rs`
/// seed: `payments` is a `PiiTable`, which is a `SensitiveTable`, which is
/// a `GovernedTable`.
async fn seed_ontology(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    let facts = vec![
        Flake::assert(
            dsc("payments"),
            rdf_type(),
            FlakeValue::Ref(dsc("PiiTable")),
            t,
        ),
        Flake::assert(
            dsc("PiiTable"),
            sub_class_of(),
            FlakeValue::Ref(dsc("SensitiveTable")),
            t,
        ),
        Flake::assert(
            dsc("SensitiveTable"),
            sub_class_of(),
            FlakeValue::Ref(dsc("GovernedTable")),
            t,
        ),
    ];
    store
        .assert_flakes(&facts)
        .await
        .expect("seed the ontology");
}

async fn run_reasoning_empty_body(app: &axum::Router) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/reasoning/runs")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

async fn derived_about(app: &axum::Router, subject: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/reasoning/derived?subject={subject}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// **The whole point of decision 4.4.** A retraction happens through the
/// graph engine directly — standing in for whatever else in the system
/// retracted a base fact — with no caller ever telling `/reasoning/runs`
/// about it. An empty-body call must discover it on its own and take the
/// incremental path, and the result must be **correct**: the two-hop
/// conclusion that depended on the retracted link is gone.
#[tokio::test]
async fn an_untold_retraction_is_discovered_and_maintained_incrementally() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let (status, first) = run_reasoning_empty_body(&app).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first["technique"], "full",
        "the first run has nothing cached: {first}"
    );

    let t = store.next_time().await.expect("a transaction time");
    let retracted = Flake::assert(
        dsc("SensitiveTable"),
        sub_class_of(),
        FlakeValue::Ref(dsc("GovernedTable")),
        t,
    )
    .retracted_at(t);
    store
        .retract_flakes(std::slice::from_ref(&retracted))
        .await
        .expect("retract directly against the graph — no HTTP caller involved");

    // No `retracted` field in this body at all — the automatic path must
    // still find it.
    let (status, second) = run_reasoning_empty_body(&app).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        second["technique"], "incremental",
        "an untold retraction must still be discovered: {second}"
    );

    let derived = derived_about(&app, &dsc("payments").to_string()).await;
    let objects: Vec<&str> = derived
        .as_array()
        .expect("array")
        .iter()
        .map(|f| f["o"].as_str().expect("o"))
        .collect();
    let governed_table = dsc("GovernedTable").to_string();
    assert!(
        !objects.contains(&governed_table.as_str()),
        "the two-hop conclusion outlived the retraction it depended on: {derived}"
    );
}

/// Phase 3 item 3.11: overlay staleness is visible on the wire, not only
/// internally.
#[tokio::test]
async fn maintained_to_reaches_the_response() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let (status, body) = run_reasoning_empty_body(&app).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["maintainedTo"].as_i64().is_some_and(|t| t > 0),
        "{body}"
    );
}

/// Nothing retracted since the last run's watermark is the same
/// "nothing to maintain against" case the explicit path already falls
/// back to full for — reached automatically here, not because an empty
/// list was named in the body.
#[tokio::test]
async fn nothing_retracted_since_the_last_run_still_reports_full() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    run_reasoning_empty_body(&app).await;
    let (status, second) = run_reasoning_empty_body(&app).await;

    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["technique"], "full", "{second}");
}
