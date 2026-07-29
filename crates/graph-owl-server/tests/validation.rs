//! Epic 5 Slices C, D and E, at the HTTP surface.
//!
//! The validator and the shape reader are exhaustively unit-tested without a
//! database. What only an end-to-end pass can show is the part that is about
//! *the estate*: that a shape stated in the graph reaches the real facts, that
//! the queue is stored rather than recomputed, and that a pass writes nothing
//! back into the graph it validated.

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

fn a(id: &str) -> Sid {
    Sid::dsc(id)
}
fn sh(term: &str) -> Sid {
    Sid::new(namespace::SHACL, term)
}
fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}

async fn graph(connection_string: &str) -> graph_owl_engine_postgres::PostgresTripleStore {
    graph_owl_engine_postgres::PostgresTripleStore::connect(connection_string)
        .await
        .expect("graph engine")
}

/// **Demo 4's shape**, stated as triples: every regulatory table must have an
/// owner and a retention tag.
async fn seed_shape(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    let shapes_graph = Sid::dsc("graph:shapes");
    let in_shapes = |s: Sid, p: Sid, o: FlakeValue| Flake {
        s,
        p,
        o,
        cx: Some(shapes_graph.clone()),
        t,
        op: true,
    };

    let facts = vec![
        in_shapes(
            a("RegulatoryShape"),
            rdf_type(),
            FlakeValue::Ref(sh("NodeShape")),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("targetClass"),
            FlakeValue::Ref(a("RegulatoryTable")),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("message"),
            FlakeValue::String("a regulatory table needs an owner and a retention tag".into()),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("property"),
            FlakeValue::Ref(a("RegulatoryShape/owner")),
        ),
        in_shapes(
            a("RegulatoryShape/owner"),
            sh("path"),
            FlakeValue::Ref(a("owner")),
        ),
        in_shapes(
            a("RegulatoryShape/owner"),
            sh("minCount"),
            FlakeValue::Int(1),
        ),
        in_shapes(
            a("RegulatoryShape"),
            sh("property"),
            FlakeValue::Ref(a("RegulatoryShape/retention")),
        ),
        in_shapes(
            a("RegulatoryShape/retention"),
            sh("path"),
            FlakeValue::Ref(a("tag")),
        ),
        in_shapes(
            a("RegulatoryShape/retention"),
            sh("minCount"),
            FlakeValue::Int(1),
        ),
    ];
    store.assert_flakes(&facts).await.expect("seed the shape");
}

/// One regulatory table with neither an owner nor a retention tag.
async fn seed_offender(store: &graph_owl_engine_postgres::PostgresTripleStore) {
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[Flake::assert(
            a("payments"),
            rdf_type(),
            FlakeValue::Ref(a("RegulatoryTable")),
            t,
        )])
        .await
        .expect("seed the table");
}

async fn run_validation(app: &axum::Router) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/validation/runs")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn report(app: &axum::Router, query: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/validation/report{query}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// **The demo, end to end.** A shape stated as triples finds the table that
/// breaks it, and the queue fills with something a steward can act on.
#[tokio::test]
async fn a_shape_stated_in_the_graph_fills_the_queue() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 1, "the shape must have been read: {run}");
    assert_eq!(run["refusedShapes"], 0, "{run}");
    assert_eq!(run["conforms"], false, "{run}");
    assert_eq!(run["violations"], 2, "an owner and a tag: {run}");

    let queue = report(&app, "").await;
    let rows = queue["data"].as_array().expect("data");
    assert_eq!(rows.len(), 2, "{queue}");
    assert!(
        rows.iter().all(|r| r["focusNode"] == "1:payments"),
        "{queue}"
    );
    assert!(
        rows.iter()
            .all(|r| r["message"] == "a regulatory table needs an owner and a retention tag"),
        "the shape's own message must reach the queue: {queue}"
    );
    // A `MinCount` failure suggests asserting the missing value — the queue is
    // only actionable if it says what to do.
    assert_eq!(rows[0]["suggestion"]["action"], "assertMissing", "{queue}");
}

/// And the negative: an estate that satisfies the shape empties the queue. A
/// pass that only ever adds rows is a queue nobody can finish.
#[tokio::test]
async fn fixing_the_data_clears_the_queue_on_the_next_pass() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;
    assert_eq!(report(&app, "").await["total"], 2);

    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[
            Flake::assert(a("payments"), a("owner"), FlakeValue::Ref(a("finance")), t),
            Flake::assert(
                a("payments"),
                a("tag"),
                FlakeValue::String("retain-7y".into()),
                t,
            ),
        ])
        .await
        .expect("fix the data");

    let run = run_validation(&app).await;

    assert_eq!(run["conforms"], true, "{run}");
    let queue = report(&app, "").await;
    assert_eq!(queue["total"], 0, "{queue}");
    // But the report still says *when* it ran. An empty queue that cannot
    // prove it is current is indistinguishable from one that never ran.
    assert!(
        queue["computedAtT"].as_i64().expect("computedAtT") > 0,
        "{queue}"
    );
}

/// **A pass writes nothing into the graph it validated.** Validation that
/// mutated the estate would make running it a decision rather than a diagnostic.
#[tokio::test]
async fn a_validation_pass_leaves_the_graph_untouched() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;

    let before = store
        .query_pattern(&TriplePattern::default())
        .await
        .expect("everything");
    run_validation(&app).await;
    let after = store
        .query_pattern(&TriplePattern::default())
        .await
        .expect("everything");

    assert_eq!(before.len(), after.len(), "a validation pass wrote a flake");
}

/// The queue is filterable, because a steward works one severity or one asset
/// at a time. A filter that returns everything is a filter that looks like it
/// worked.
#[tokio::test]
async fn the_queue_can_be_narrowed() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;

    let mine = report(&app, "?focusNode=1:payments").await;
    let theirs = report(&app, "?focusNode=1:something-else").await;
    let by_shape = report(&app, "?shape=1:RegulatoryShape").await;
    let other_shape = report(&app, "?shape=1:NoSuchShape").await;
    let violations = report(&app, "?severity=violation").await;
    let warnings = report(&app, "?severity=warning").await;

    assert_eq!(mine["total"], 2, "{mine}");
    assert_eq!(theirs["total"], 0, "{theirs}");
    assert_eq!(by_shape["total"], 2, "{by_shape}");
    assert_eq!(other_shape["total"], 0, "{other_shape}");
    assert_eq!(violations["total"], 2, "{violations}");
    assert_eq!(warnings["total"], 0, "{warnings}");
}

/// Paging, per `00d`. `total` counts the whole queue rather than the page, or
/// a client cannot tell how much work is left.
#[tokio::test]
async fn the_queue_is_paged_and_reports_the_whole_total() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;
    run_validation(&app).await;

    let first = report(&app, "?limit=1").await;
    let second = report(&app, "?limit=1&offset=1").await;

    assert_eq!(first["data"].as_array().expect("data").len(), 1);
    assert_eq!(first["total"], 2, "the total is the queue, not the page");
    assert_ne!(first["data"][0]["id"], second["data"][0]["id"]);
}

/// **A malformed shape does not stop the others.** An estate goes unvalidated
/// the moment one bad shape can veto the pass — and nobody notices, because the
/// report simply looks clean.
#[tokio::test]
async fn a_broken_shape_is_counted_without_stopping_the_pass() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;
    seed_offender(&store).await;

    // A second shape with no target: unreadable, and on its own.
    let t = store.next_time().await.expect("a transaction time");
    store
        .assert_flakes(&[Flake {
            s: a("BrokenShape"),
            p: rdf_type(),
            o: FlakeValue::Ref(sh("NodeShape")),
            cx: Some(Sid::dsc("graph:shapes")),
            t,
            op: true,
        }])
        .await
        .expect("seed the broken shape");

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 1, "the good shape still ran: {run}");
    assert_eq!(run["refusedShapes"], 1, "and the bad one is counted: {run}");
    assert_eq!(run["violations"], 2, "{run}");
}

/// A shape lives in its own graph, so it is not itself an asset the catalog
/// validates — and validating the shapes graph would make `TableShape` a focus
/// node for `EnvelopeShape`.
#[tokio::test]
async fn shapes_are_not_themselves_validated() {
    let (app, _database, connection_string) = test_app().await;
    let store = graph(&connection_string).await;
    seed_shape(&store).await;

    // A shape targeting *everything with a name* would catch the shape nodes
    // too, if they were in the default graph.
    let t = store.next_time().await.expect("a transaction time");
    let shapes_graph = Sid::dsc("graph:shapes");
    store
        .assert_flakes(&[
            Flake {
                s: a("EverythingShape"),
                p: rdf_type(),
                o: FlakeValue::Ref(sh("NodeShape")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape"),
                p: sh("targetSubjectsOf"),
                o: FlakeValue::Ref(sh("path")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape"),
                p: sh("property"),
                o: FlakeValue::Ref(a("EverythingShape/p")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape/p"),
                p: sh("path"),
                o: FlakeValue::Ref(a("owner")),
                cx: Some(shapes_graph.clone()),
                t,
                op: true,
            },
            Flake {
                s: a("EverythingShape/p"),
                p: sh("minCount"),
                o: FlakeValue::Int(1),
                cx: Some(shapes_graph),
                t,
                op: true,
            },
        ])
        .await
        .expect("seed");

    let run = run_validation(&app).await;

    assert_eq!(run["shapes"], 2, "{run}");
    assert_eq!(
        run["violations"], 0,
        "the property shapes in the shapes graph were validated as data: {run}"
    );
}
