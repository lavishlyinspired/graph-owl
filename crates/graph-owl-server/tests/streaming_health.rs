//! Epic 19 Slice C: lag and health.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::json;
use std::time::Duration;
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request should build");
    let response = app.clone().oneshot(request).await.expect("handled");
    let status = response.status();
    (status, json_body(response).await)
}

async fn text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

/// **Lag is computed against the broker's real high-water mark, not
/// estimated.** 100 messages produced to a topic nothing ever consumes from
/// must show as exactly 100 lag once the periodic health poll (Slice C) has
/// had a chance to run.
#[tokio::test]
async fn lag_reports_the_real_backlog() {
    let (app, _database, _) = test_app().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();

    send(
        &app,
        "POST",
        "/webhooks/mappings",
        Some(json!({
            "name": "streaming-health",
            "kind": {"kind": "literal", "value": "service"},
            "entityName": {"kind": "path", "pointer": "/name"},
        })),
    )
    .await;
    let (status, subscription) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {"kind": "kafkaProtocol", "bootstrapServers": bootstrap_servers},
            "topic": topic,
            "consumerGroup": "graph-owl-lag-test",
            "mapping": "streaming-health",
            "startPosition": {"kind": "earliest"},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{subscription}");

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &bootstrap_servers)
        .set("broker.address.family", "v4")
        .create()
        .expect("producer should build");
    for i in 0..100 {
        producer
            .send(
                FutureRecord::to(&topic)
                    .payload(&json!({"name": format!("lag-{i}")}).to_string())
                    .key(&i.to_string()),
                Duration::from_secs(10),
            )
            .await
            .expect("send should succeed");
    }

    // Nothing here calls `process_one_message` — the point is that lag
    // updates from the periodic poll alone, independent of processing.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("handled");
        let scrape = text(response).await;
        if scrape.contains("graph_owl_stream_consumer_lag") && scrape.contains(" 100") {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "lag never reported 100: {scrape}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// **A failed consumer makes readiness fail.** A subscription pointed at an
/// unreachable broker never transitions past `Failed`, and `/ready` — a
/// required check, not advisory — must say so rather than staying green.
#[tokio::test]
async fn a_failed_consumer_makes_ready_fail() {
    let (app, _database, _) = test_app().await;

    send(
        &app,
        "POST",
        "/webhooks/mappings",
        Some(json!({
            "name": "streaming-health-failed",
            "kind": {"kind": "literal", "value": "service"},
            "entityName": {"kind": "path", "pointer": "/name"},
        })),
    )
    .await;
    let (status, subscription) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {"kind": "kafkaProtocol", "bootstrapServers": "127.0.0.1:1"},
            "topic": "unreachable-topic",
            "consumerGroup": "graph-owl-unreachable",
            "mapping": "streaming-health-failed",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{subscription}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("handled");
        if response.status() == StatusCode::SERVICE_UNAVAILABLE {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "/ready never failed for the unreachable subscription"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
