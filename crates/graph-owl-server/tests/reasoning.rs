//! Epic 6 Slices D and E, at the HTTP surface.
//!
//! The reasoner itself is exhaustively unit-tested in `graph-owl-reasoning`
//! without a database. What only an end-to-end run can show is the part that
//! is about *storage*: that conclusions land in their own graph, that a re-run
//! replaces rather than accumulates, and that the asserted base is byte-for-byte
//! what it was before the run — which is the guarantee that makes enabling
//! reasoning a reversible decision.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
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

/// A three-level hierarchy: `payments` is a `PiiTable`, which is a
/// `SensitiveTable`, which is a `GovernedTable`. Depth 3 so the conclusion
/// under test needs two rounds of inference rather than one.
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

async fn run_reasoning(app: &axum::Router) -> Value {
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
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn explain(app: &axum::Router, s: &str, p: &str, o: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/reasoning/explain?s={s}&p={p}&o={o}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// Everything in the default graph, as stored.
async fn default_graph(store: &graph_owl_engine_postgres::PostgresTripleStore) -> Vec<Flake> {
    let mut flakes = store
        .query_pattern(&TriplePattern {
            cx: Some(None),
            ..Default::default()
        })
        .await
        .expect("default graph");
    flakes.sort_by_key(|f| (f.s.to_string(), f.p.to_string(), format!("{:?}", f.o)));
    flakes
}

async fn overlay(store: &graph_owl_engine_postgres::PostgresTripleStore) -> Vec<Flake> {
    store
        .query_pattern(&TriplePattern {
            cx: Some(Some(Sid::dsc("graph:reasoning"))),
            ..Default::default()
        })
        .await
        .expect("reasoning graph")
}

/// **Decision 2's guarantee, and the reason the overlay is a separate graph.**
/// A run must leave the asserted base exactly as it found it. Derivations
/// written beside assertions cannot be taken back, because the next run's
/// wholesale replacement would delete asserted data along with them.
#[tokio::test]
async fn a_run_leaves_the_asserted_base_exactly_as_it_found_it() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let before = default_graph(&store).await;
    let report = run_reasoning(&app).await;
    let after = default_graph(&store).await;

    assert_eq!(before, after, "a run wrote into the default graph");
    assert!(
        report["derived"].as_u64().expect("derived") >= 2,
        "depth 3 implies two types: {report}"
    );
    assert_eq!(
        report["capped"],
        Value::Null,
        "fixpoint, not a cap: {report}"
    );
}

/// And the conclusions are actually *there* — in their own graph, so the
/// assertion above is about separation rather than about a run that did
/// nothing.
#[tokio::test]
async fn conclusions_land_in_the_reasoning_graph() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    run_reasoning(&app).await;

    let derived = overlay(&store).await;
    assert!(
        derived
            .iter()
            .any(|f| f.s == dsc("payments") && f.o == FlakeValue::Ref(dsc("GovernedTable"))),
        "the depth-3 conclusion is missing: {derived:#?}"
    );
    assert!(
        derived
            .iter()
            .all(|f| f.cx == Some(Sid::dsc("graph:reasoning"))),
        "{derived:#?}"
    );
}

/// A scheduled run must converge. Accumulation would grow the overlay without
/// bound and leave conclusions standing after the facts behind them are gone.
#[tokio::test]
async fn a_second_run_replaces_rather_than_accumulating() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let first = run_reasoning(&app).await;
    let after_first = overlay(&store).await.len();
    let second = run_reasoning(&app).await;
    let after_second = overlay(&store).await.len();

    assert_eq!(after_first, after_second, "the overlay grew across runs");
    assert_eq!(first["derived"], second["derived"]);
    assert_eq!(
        second["replaced"], first["derived"],
        "the second run withdrew exactly what the first wrote: {second}"
    );
    assert_eq!(first["replaced"], 0, "nothing to replace yet: {first}");
}

/// **Withdrawing a premise withdraws the conclusion.** This is what makes the
/// overlay derived rather than merely written: a conclusion that outlives its
/// premise is a fact nobody asserted and nothing implies.
#[tokio::test]
async fn retracting_an_axiom_removes_its_conclusions_on_the_next_run() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;
    run_reasoning(&app).await;

    let t = store.next_time().await.expect("a transaction time");
    store
        .retract_flakes(&[Flake::assert(
            dsc("SensitiveTable"),
            sub_class_of(),
            FlakeValue::Ref(dsc("GovernedTable")),
            t,
        )])
        .await
        .expect("retract the axiom");
    run_reasoning(&app).await;

    let derived = overlay(&store).await;
    assert!(
        !derived
            .iter()
            .any(|f| f.o == FlakeValue::Ref(dsc("GovernedTable"))),
        "the conclusion outlived its premise: {derived:#?}"
    );
    // And the negative: the conclusion that still holds is still there, so the
    // assertion above is about the retracted axiom rather than about an empty
    // overlay.
    assert!(
        derived
            .iter()
            .any(|f| f.o == FlakeValue::Ref(dsc("SensitiveTable"))),
        "{derived:#?}"
    );
}

/// The recursive explanation, end to end. One level would name
/// `payments type SensitiveTable` as a premise and stop — and why *that* held
/// is the half a reviewer is actually checking.
#[tokio::test]
async fn a_derived_fact_explains_all_the_way_down_to_assertions() {
    let (app, _database, connection_string) = test_app().await;
    seed_ontology(&graph(&connection_string).await).await;

    let (status, body) = explain(
        &app,
        &dsc("payments").to_string(),
        &rdf_type().to_string(),
        &dsc("GovernedTable").to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "derived", "{body}");
    let chain = &body["chains"][0];
    assert_eq!(chain["rule"], "subClassOf", "{body}");
    let deeper = chain["premises"]
        .as_array()
        .expect("premises")
        .iter()
        .find(|p| p["status"] == "derived")
        .expect("one premise is itself derived");
    assert!(
        deeper["chains"][0]["premises"]
            .as_array()
            .expect("inner premises")
            .iter()
            .all(|p| p["status"] == "asserted"),
        "depth 2 rests on assertions: {body}"
    );
}

#[tokio::test]
async fn an_asserted_fact_explains_as_asserted() {
    let (app, _database, connection_string) = test_app().await;
    seed_ontology(&graph(&connection_string).await).await;

    let (status, body) = explain(
        &app,
        &dsc("payments").to_string(),
        &rdf_type().to_string(),
        &dsc("PiiTable").to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "asserted", "{body}");
}

/// A fact that is neither stated nor implied has no explanation, and saying so
/// with a `404` is the difference between "nothing supports this" and "this is
/// supported by nothing", which read the same and mean opposite things.
#[tokio::test]
async fn a_fact_that_is_neither_asserted_nor_derived_is_not_found() {
    let (app, _database, connection_string) = test_app().await;
    seed_ontology(&graph(&connection_string).await).await;

    let (status, _) = explain(
        &app,
        &dsc("payments").to_string(),
        &rdf_type().to_string(),
        &dsc("PublicTable").to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A malformed identifier is the caller's mistake, not a missing fact — and
/// `400` rather than `404` is what tells them which.
#[tokio::test]
async fn an_unparseable_identifier_is_rejected_rather_than_missing() {
    let (app, _database, _) = test_app().await;

    let (status, _) = explain(&app, "not-a-sid", "1:type", "1:x").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}
