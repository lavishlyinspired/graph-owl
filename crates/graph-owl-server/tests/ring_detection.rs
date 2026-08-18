//! Plan 123 Slice G — circular trading, end to end over HTTP.
//!
//! **The plan's own RED, and its own mutator**: a synthetic three-party ring
//! is returned as one finding and an unconnected party of larger value is not
//! swept into it; break the ring with one edge and it must stop being
//! reported.
//!
//! The mutator is the reason this needed a new primitive rather than the
//! `connected_components` the analytics crate already had. That pass is
//! *weakly* connected — it ignores direction — so breaking the ring leaves
//! all three parties in one component and the mutator survives. A test
//! asserting "one component" would have passed against code that cannot tell
//! a ring from a chain.

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

async fn setup(app: &axum::Router) {
    let (status, _) = json(
        app,
        "POST",
        "/namespaces",
        serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json(
        app,
        "POST",
        "/predicates",
        serde_json::json!({"namespace": 1024, "name": "suppliedTo", "valueType": 0, "many": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

async fn import(app: &axum::Router, turtle: &str) {
    let (status, body) = call(
        app,
        "POST",
        "/graph/import/rdf?source=ring-test&format=turtle",
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

async fn cycles_from(app: &axum::Router, seed: &str) -> Vec<Vec<String>> {
    let (status, body) = json(
        app,
        "POST",
        "/graph/context/analytics",
        serde_json::json!({ "seed": seed, "hops": 4, "maxNodes": 50 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["cycles"]
        .as_array()
        .map(|outer| {
            outer
                .iter()
                .map(|inner| {
                    inner
                        .as_array()
                        .map(|c| {
                            c.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Three parties supplying each other in a closed loop, plus one unconnected
/// party trading at larger value — the plan's own scenario.
const RING: &str = r#"
    @prefix gst: <https://graph-owl.dev/packs/gst#> .
    @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

    gst:party-a rdf:type gst:Supplier ; gst:suppliedTo gst:party-b .
    gst:party-b rdf:type gst:Supplier ; gst:suppliedTo gst:party-c .
    gst:party-c rdf:type gst:Supplier ; gst:suppliedTo gst:party-a .

    gst:party-big rdf:type gst:Supplier ; gst:suppliedTo gst:party-customer .
"#;

const BROKEN_RING: &str = r#"
    @prefix gst: <https://graph-owl.dev/packs/gst#> .
    @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

    gst:party-a rdf:type gst:Supplier ; gst:suppliedTo gst:party-b .
    gst:party-b rdf:type gst:Supplier ; gst:suppliedTo gst:party-c .

    gst:party-big rdf:type gst:Supplier ; gst:suppliedTo gst:party-customer .
"#;

#[tokio::test]
async fn a_three_party_ring_is_reported_as_one_cycle() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(&app, RING).await;

    let cycles = cycles_from(&app, "https://graph-owl.dev/packs/gst#party-a").await;

    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycles[0].len(), 3, "{cycles:?}");
}

#[tokio::test]
async fn breaking_the_ring_with_one_edge_stops_it_being_reported() {
    // The plan's own mutator. `connected_components` survives this — all
    // three parties stay weakly connected — so it is the test that proves the
    // finding means what it says.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(&app, BROKEN_RING).await;

    let cycles = cycles_from(&app, "https://graph-owl.dev/packs/gst#party-a").await;

    assert!(cycles.is_empty(), "{cycles:?}");
}

#[tokio::test]
async fn an_unconnected_party_of_larger_value_is_not_swept_into_the_ring() {
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(&app, RING).await;

    let cycles = cycles_from(&app, "https://graph-owl.dev/packs/gst#party-a").await;

    let members: Vec<&str> = cycles[0].iter().map(String::as_str).collect();
    assert!(
        !members.iter().any(|m| m.contains("party-big")),
        "{members:?}"
    );
}

#[tokio::test]
async fn a_seed_outside_the_ring_does_not_report_it() {
    // The walk is bounded and seeded; a party with no path to the ring must
    // not be told about it. Reporting a ring a caller cannot reach would make
    // the finding unattributable.
    let (app, _db, _url) = test_app().await;
    setup(&app).await;
    import(&app, RING).await;

    let cycles = cycles_from(&app, "https://graph-owl.dev/packs/gst#party-big").await;

    assert!(cycles.is_empty(), "{cycles:?}");
}
