//! Epic 97's `DRed` maintenance, at the HTTP surface — **Phase 1.9 of
//! `plans/EPIC-COMPLETION-PLAN.md`**.
//!
//! `Catalog::run_reasoning_incremental` shipped, mutation-tested, with
//! nothing in `graph-owl-server` calling it — `POST /reasoning/runs` always
//! did a full run. Per the user's own decision (Phase 4.4): retractions
//! reach it as an explicit, caller-supplied `retracted` list in the request
//! body, naming exactly what it withdrew from the base graph itself — the
//! endpoint does not perform that withdrawal, only maintains the overlay to
//! match it.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_engine::TripleStore;
use serde_json::{Value, json};
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

/// The same three-level hierarchy `reasoning.rs` seeds: `payments` is a
/// `PiiTable`, which is a `SensitiveTable`, which is a `GovernedTable`.
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

async fn run_reasoning(app: &axum::Router, body: Option<Value>) -> (StatusCode, Value) {
    let request = Request::builder().method("POST").uri("/reasoning/runs");
    let request = match &body {
        Some(_) => request.header("content-type", "application/json"),
        None => request,
    };
    let response = app
        .clone()
        .oneshot(
            request
                .body(match body {
                    Some(v) => Body::from(v.to_string()),
                    None => Body::empty(),
                })
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

/// The real case: a caller retracts a fact through its own write path (here,
/// directly against the graph engine — standing in for whatever produced
/// the retraction), then tells `/reasoning/runs` exactly what it withdrew.
/// The overlay must be maintained via `DRed`, not a wholesale re-derivation,
/// and it must be **correct**: the conclusion that depended on the
/// retracted link is gone, and the one that does not is untouched.
#[tokio::test]
async fn a_retracted_list_in_the_body_takes_the_incremental_path_and_prunes_correctly() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_ontology(&store).await;

    let (status, first) = run_reasoning(&app, None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["technique"], "full", "{first}");

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
        .expect("retract in the graph, so the incremental result agrees with a full re-run");

    let (status, second) = run_reasoning(
        &app,
        Some(json!({
            "retracted": [{
                "s": retracted.s.to_string(),
                "p": retracted.p.to_string(),
                "o": { "type": "ref", "value": dsc("GovernedTable").to_string() },
                "t": t,
            }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["technique"], "incremental", "{second}");

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

/// A malformed `Sid` in the retracted list must fail loudly, naming the
/// offending field — not be silently misparsed or dropped.
#[tokio::test]
async fn a_malformed_sid_in_the_retracted_list_is_a_named_validation_error() {
    let (app, _database, _connection_string) = test_app().await;

    let (status, body) = run_reasoning(
        &app,
        Some(json!({
            "retracted": [{
                "s": "not-a-sid",
                "p": sub_class_of().to_string(),
                "o": { "type": "ref", "value": dsc("GovernedTable").to_string() },
                "t": 1,
            }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "retracted[0].s", "{body}");
}

/// An unsupported value type must be rejected, not silently coerced or
/// dropped — this endpoint only accepts the scalar shapes `DRed` maintenance
/// actually needs.
#[tokio::test]
async fn an_unsupported_value_type_in_the_retracted_list_is_rejected() {
    let (app, _database, _connection_string) = test_app().await;

    let (status, body) = run_reasoning(
        &app,
        Some(json!({
            "retracted": [{
                "s": dsc("payments").to_string(),
                "p": rdf_type().to_string(),
                "o": { "type": "bytes", "value": [1, 2, 3] },
                "t": 1,
            }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
