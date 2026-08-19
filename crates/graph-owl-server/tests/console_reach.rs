//! Plan 123 Slice I — the console can reach the data the console holds.
//!
//! Two defects, both found live against real reconciled data and both of the
//! same kind: a surface that reported something plausible and wrong rather
//! than reporting nothing.
//!
//! 1. `GET /search?q=INV-MAR-011` returned `[]` while `/graph/context` on that
//!    same subject returned its whole neighbourhood. Search covered assets,
//!    glossary terms and business metrics; a pack's imported flakes have no
//!    asset representation, so every one was invisible. **Explore is the
//!    console's main screen and its only entry point could not see the graph.**
//!
//! 2. Overview reported `GRAPH FACTS 724, GRAPH NODES 0`. The node count
//!    matched `dsc:type` flakes, which only projected catalog assets carry.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, token};
use tower::ServiceExt;

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    content_type: &str,
    body: String,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {}", token("system")))
                .header("content-type", content_type)
                .body(Body::from(body))
                .expect("builds"),
        )
        .await
        .expect("handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    call(app, method, uri, "application/json", body.to_string()).await
}

/// Imported RDF, exactly as a pack lands it — typed with its own vocabulary's
/// `rdf:type`, never `dsc:type`. That is the whole point: a fixture using
/// `dsc:type` would pass against the broken code.
async fn seed_pack_data(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for name in ["invoiceNumber", "supplierGstin"] {
        let (status, _) = json(
            app,
            "POST",
            "/predicates",
            serde_json::json!({"namespace": 1024, "name": name, "valueType": 1, "many": false}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let turtle = r#"
        @prefix gst: <https://graph-owl.dev/packs/gst#> .
        @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

        gst:books-INV-MAR-011 rdf:type gst:PurchaseInvoice ;
            gst:invoiceNumber "INV-MAR-011" .
        gst:supplier-1 rdf:type gst:Supplier ;
            gst:supplierGstin "27AABCS1429B1Z8" .
    "#;
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=console-reach&format=turtle",
        "text/turtle",
        turtle.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["rejected"].as_array().map(Vec::len).unwrap_or(0),
        0,
        "{body}"
    );
}

#[tokio::test]
async fn search_finds_a_graph_subject_by_the_value_it_carries() {
    let (app, _db, _url) = test_app().await;
    seed_pack_data(&app).await;

    let (status, results) = call(
        &app,
        "GET",
        "/search?q=INV-MAR-011",
        "application/json",
        String::new(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{results}");
    let hits = results.as_array().expect("array");
    let subject = hits
        .iter()
        .find(|h| h["kind"] == "graph-subject")
        .unwrap_or_else(|| panic!("no graph subject in {results}"));
    assert!(
        subject["id"]
            .as_str()
            .unwrap_or_default()
            .contains("INV-MAR-011"),
        "{subject}"
    );
    // The hit says *why* it is a hit. A bare identifier leaves a reader
    // guessing which of a subject's fields matched.
    assert!(
        subject["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("invoiceNumber"),
        "{subject}"
    );
}

#[tokio::test]
async fn a_value_no_subject_carries_returns_no_graph_hits() {
    // The negative. Without it, a search returning *every* subject would pass
    // the test above and make the console's main entry point useless in a
    // different way.
    let (app, _db, _url) = test_app().await;
    seed_pack_data(&app).await;

    let (_, results) = call(
        &app,
        "GET",
        "/search?q=INV-NOT-A-REAL-ONE",
        "application/json",
        String::new(),
    )
    .await;

    assert!(
        !results
            .as_array()
            .expect("array")
            .iter()
            .any(|h| h["kind"] == "graph-subject"),
        "{results}"
    );
}

#[tokio::test]
async fn overview_counts_imported_subjects_as_graph_nodes() {
    let (app, _db, _url) = test_app().await;
    seed_pack_data(&app).await;

    let (status, overview) =
        call(&app, "GET", "/overview", "application/json", String::new()).await;

    assert_eq!(status, StatusCode::OK, "{overview}");
    let nodes = overview["graph"]["nodes"].as_u64().unwrap_or(0);
    let flakes = overview["graph"]["flakes"].as_u64().unwrap_or(0);

    // Two imported subjects, neither carrying `dsc:type`. The old count read
    // 0 here while `flakes` read 4 — a tile labelled "graph nodes" reporting
    // that a store full of facts has no nodes.
    assert_eq!(nodes, 2, "{overview}");
    assert!(
        flakes >= nodes,
        "facts cannot be fewer than subjects: {overview}"
    );

    // Two — the two `rdf:type` assertions. A type assertion *is* a reference
    // to the class node, so it counts, and excluding it would mean naming one
    // predicate as special: exactly the mistake this slice fixes. Stated
    // rather than tuned away, because a reader of the tile should know that
    // "edges" includes typing.
    assert_eq!(overview["graph"]["edges"].as_u64(), Some(2), "{overview}");
}

#[tokio::test]
async fn an_empty_graph_reports_zero_nodes_rather_than_failing() {
    let (app, _db, _url) = test_app().await;

    let (status, overview) =
        call(&app, "GET", "/overview", "application/json", String::new()).await;

    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(
        overview["graph"]["nodes"].as_u64().unwrap_or(999),
        0,
        "{overview}"
    );
}
