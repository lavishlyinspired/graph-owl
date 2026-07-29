//! Epic 10 Slices C and D, at the HTTP surface.
//!
//! The pure decisions are unit-tested next to their definitions. What only an
//! end-to-end request can show is that they are *reached*: a label computed
//! correctly and then not used is the same outage as one computed wrongly.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use tower::ServiceExt;

async fn call(app: &axum::Router, uri: &str, request_id: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(id) = request_id {
        builder = builder.header("x-request-id", id);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).expect("request should build"))
        .await
        .expect("request should be handled");
    let status = response.status();
    let echoed = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    (status, echoed)
}

async fn scrape(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("metrics are utf-8")
}

/// **The cardinality test, and the reason Slice D exists.** Three requests to
/// three different ids must produce *one* series. Labelling with the concrete
/// path produces three, and a real estate produces one per asset — which is how
/// a Prometheus server is ended.
#[tokio::test]
async fn three_requests_to_three_ids_produce_one_series() {
    let (app, _container, _) = test_app().await;

    for id in [
        "11111111-1111-4111-8111-111111111111",
        "22222222-2222-4222-8222-222222222222",
        "33333333-3333-4333-8333-333333333333",
    ] {
        let (status, _) = call(&app, &format!("/assets/{id}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let scraped = scrape(&app).await;
    let series: Vec<&str> = scraped
        .lines()
        .filter(|line| line.starts_with("graph_owl_http_requests_total"))
        .filter(|line| line.contains("/assets/"))
        .collect();

    assert_eq!(
        series.len(),
        1,
        "one template, one series — found {}: {series:#?}",
        series.len()
    );
    assert!(
        series[0].contains("/assets/{id}"),
        "the label must be the template: {}",
        series[0]
    );
    // And the negative: no concrete id may appear anywhere in the exposition.
    assert!(
        !scraped.contains("11111111-1111-4111-8111-111111111111"),
        "an entity id reached a metric label"
    );
}

/// A client that named its request gets that name back. Generating a fresh id
/// severs exactly the link the header exists to make.
#[tokio::test]
async fn a_supplied_request_id_is_echoed_back_unchanged() {
    let (app, _container, _) = test_app().await;

    let (_, echoed) = call(&app, "/health", Some("client-chosen-42")).await;

    assert_eq!(echoed, "client-chosen-42");
}

/// And the negative: with no header supplied, one is still returned — and two
/// requests do not share it.
#[tokio::test]
async fn a_request_without_an_id_still_gets_one_and_they_differ() {
    let (app, _container, _) = test_app().await;

    let (_, first) = call(&app, "/health", None).await;
    let (_, second) = call(&app, "/health", None).await;

    assert!(!first.is_empty(), "every response carries a correlation id");
    assert_ne!(first, second, "a constant id correlates nothing");
}

/// A `4xx` still gets the header. The error path is where an operator most
/// needs the correlation, so a header added only on success is added at exactly
/// the wrong time.
///
/// A missing asset rather than a missing *route*: an unknown path falls through
/// to the SPA and is a `200`, by design — the console's client-side routes have
/// to resolve somehow.
#[tokio::test]
async fn an_error_response_carries_the_request_id_too() {
    let (app, _container, _) = test_app().await;

    let (status, echoed) = call(
        &app,
        "/assets/99999999-9999-4999-8999-999999999999",
        Some("failing-request"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(echoed, "failing-request");
}

/// `/metrics` is excluded from its own counters. A scrape every 15 seconds is
/// the most frequent request most deployments make, and counting it buries real
/// traffic in every rate query an operator writes.
#[tokio::test]
async fn the_metrics_endpoint_does_not_count_itself() {
    let (app, _container, _) = test_app().await;

    scrape(&app).await;
    scrape(&app).await;
    let scraped = scrape(&app).await;

    assert!(
        !scraped.contains("route=\"/metrics\""),
        "the scrape counted itself: {scraped}"
    );
    // And the negative: the exporter is actually producing output, so the
    // assertion above is not passing on an empty body.
    let (status, _) = call(&app, "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        scrape(&app).await.contains("graph_owl_http_requests_total"),
        "the exporter must be emitting something for the exclusion to mean anything"
    );
}

/// Duration is recorded in **seconds**, per the observability contract. A
/// metric named in milliseconds forces every dashboard to carry a conversion,
/// and one of them will get it wrong.
#[tokio::test]
async fn request_duration_is_exported_in_base_units() {
    let (app, _container, _) = test_app().await;

    call(&app, "/health", None).await;
    let scraped = scrape(&app).await;

    assert!(
        scraped.contains("graph_owl_http_request_duration_seconds"),
        "{scraped}"
    );
    assert!(
        !scraped.contains("duration_ms"),
        "a millisecond-named metric is a contract violation: {scraped}"
    );
}

/// The two gauges Slice D names, sampled at scrape time rather than on a timer.
#[tokio::test]
async fn the_pool_and_entity_gauges_are_exported() {
    let (app, _database, _) = test_app().await;

    // One asset, so the entity gauge has something to report that is not zero —
    // an all-zero gauge is indistinguishable from one that is never set.
    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/assets")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind":"service","name":"hdfc-core"}"#))
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(created.status(), StatusCode::CREATED);

    let scraped = scrape(&app).await;

    assert!(
        scraped.contains(r#"graph_owl_db_pool_connections{state="idle"}"#),
        "{scraped}"
    );
    assert!(
        scraped.contains(r#"graph_owl_db_pool_connections{state="in_use"}"#),
        "{scraped}"
    );
    assert!(
        scraped.contains(r#"graph_owl_catalog_entities_total{entity_type="service"} 1"#),
        "the catalogue holds one service and the gauge must say so: {scraped}"
    );
}

/// And the negative: a kind nothing was created for must not appear with a
/// fabricated count. A gauge reporting zero tables where the catalog has never
/// had one is a different statement from silence, and only one of them is true.
#[tokio::test]
async fn a_kind_with_no_assets_is_not_reported_as_zero() {
    let (app, _database, _) = test_app().await;

    let scraped = scrape(&app).await;

    assert!(
        !scraped.contains(r#"entity_type="column""#),
        "nothing has been catalogued: {scraped}"
    );
}
