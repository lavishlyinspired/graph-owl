//! `GET /reasoning/derived` resolves a pack subject by its full IRI —
//! Plan 113 Slice D.
//!
//! **`ReasoningView` hardcoded `1:${assetId}` — a `dsc:` Sid built from a
//! catalog asset's UUID.** A GST invoice has neither: it is a bare pack
//! subject with no relational row, reachable only by its IRI. This is the
//! same generalization `/graph/context` (Slice A) already made for the walk;
//! reasoning's own per-subject view had the identical gap.

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

const NAMESPACE: &str = "https://graph-owl.dev/packs/planetest113reasoning#";

async fn declare_namespace(app: &axum::Router) -> u16 {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/namespaces")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "iri": NAMESPACE }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert!(response.status().is_success(), "declare the namespace");
    u16::try_from(
        json_body(response).await["code"]
            .as_u64()
            .expect("a namespace code"),
    )
    .expect("a u16 code")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}
fn sub_class_of() -> Sid {
    Sid::new(namespace::RDFS, "subClassOf")
}

/// The same three-level hierarchy `reasoning_auto.rs` seeds under `dsc:`,
/// seeded here under a pack-declared namespace instead: `invoice-1` is a
/// `PiiSubject`, which is a `SensitiveSubject`, which is a `GovernedSubject`.
async fn seed_hierarchy(store: &graph_owl_engine_postgres::PostgresTripleStore, code: u16) {
    let subj = |local: &str| Sid::new(code, local);
    let t = store.next_time().await.expect("a transaction time");
    let facts = vec![
        Flake::assert(
            subj("invoice-1"),
            rdf_type(),
            FlakeValue::Ref(subj("PiiSubject")),
            t,
        ),
        Flake::assert(
            subj("PiiSubject"),
            sub_class_of(),
            FlakeValue::Ref(subj("SensitiveSubject")),
            t,
        ),
        Flake::assert(
            subj("SensitiveSubject"),
            sub_class_of(),
            FlakeValue::Ref(subj("GovernedSubject")),
            t,
        ),
    ];
    store
        .assert_flakes(&facts)
        .await
        .expect("seed the hierarchy");
}

async fn run_reasoning(app: &axum::Router) -> (StatusCode, Value) {
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

async fn derived_about(app: &axum::Router, subject: &str) -> (StatusCode, Value) {
    // The only reserved character an IRI-shaped subject can carry here is
    // `#`, which the `http` crate's own URI parser would otherwise read as
    // the start of a fragment and silently drop, not pass through as query.
    let encoded = subject.replace('#', "%23");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/reasoning/derived?subject={encoded}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// **The whole point of this slice.** A subject named only by its IRI — no
/// `namespace:local` shorthand, no catalog asset UUID — still resolves to
/// the derived facts the reasoner concluded about it.
#[tokio::test]
async fn derived_about_resolves_a_pack_subject_by_its_full_iri() {
    let (app, _container, connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    let store = graph(&connection_string).await;
    seed_hierarchy(&store, code).await;

    let (run_status, run_body) = run_reasoning(&app).await;
    assert_eq!(run_status, StatusCode::OK, "{run_body}");

    let iri = format!("{NAMESPACE}invoice-1");
    let (status, derived) = derived_about(&app, &iri).await;
    assert_eq!(status, StatusCode::OK, "{derived}");

    let objects: Vec<&str> = derived
        .as_array()
        .expect("array")
        .iter()
        .map(|f| f["o"].as_str().expect("o"))
        .collect();
    let governed = Sid::new(code, "GovernedSubject").to_string();
    assert!(
        objects.contains(&governed.as_str()),
        "the two-hop conclusion about a pack subject should be found by resolving its IRI: {derived}",
    );
}

/// An IRI in a namespace this deployment never declared is a `400` naming
/// the field — matching `/graph/context`'s own posture for the same case,
/// not a `500` from a Sid that failed to construct.
#[tokio::test]
async fn an_iri_in_an_unregistered_namespace_is_rejected_by_name() {
    let (app, _container, _connection_string) = test_app().await;

    let (status, body) =
        derived_about(&app, "https://graph-owl.dev/packs/never-declared#thing").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .any(|e| e["field"] == "subject"),
        "{body}"
    );
}

/// The existing `namespace:local` shorthand must still work unchanged —
/// this slice extends `derived_about`'s accepted shapes, it does not narrow
/// them.
#[tokio::test]
async fn the_namespace_local_shorthand_still_resolves() {
    let (app, _container, connection_string) = test_app().await;
    let code = declare_namespace(&app).await;
    let store = graph(&connection_string).await;
    seed_hierarchy(&store, code).await;

    let (run_status, run_body) = run_reasoning(&app).await;
    assert_eq!(run_status, StatusCode::OK, "{run_body}");

    let (status, derived) = derived_about(&app, &format!("{code}:invoice-1")).await;
    assert_eq!(status, StatusCode::OK, "{derived}");
    assert!(!derived.as_array().expect("array").is_empty(), "{derived}");
}
