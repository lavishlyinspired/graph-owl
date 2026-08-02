//! Epic 19 Slice A at the wire.
//!
//! Subscription registration is proven first, then the end-to-end path: a
//! real Kafka message, produced by this test, consumed by the same
//! background task the server itself spawns, mapped, applied and resolved
//! into a real entity — reachable through nothing but the HTTP surface, the
//! same order Epic 18 Slice A proved webhook endpoint registration before
//! webhook receipt.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app, test_app_with_secret};
use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

const SECRET: &str = "streaming-test-signing-secret";

fn token(subject: &str) -> String {
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        name: &'a str,
        exp: usize,
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: subject,
            name: subject,
            exp: 4_102_444_800, // year 2100
        },
        &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .expect("token should encode")
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
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

async fn register(app: &axum::Router, topic: &str, consumer_group: &str) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {
                "kind": "kafkaProtocol",
                "bootstrapServers": "localhost:9092",
            },
            "topic": topic,
            "consumerGroup": consumer_group,
            "mapping": "dbt-run-completed",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// **The registration secret never appears in any response** — same claim
/// `webhooks.rs` makes for webhook secrets, at the same surface.
#[tokio::test]
async fn a_registered_subscription_never_exposes_its_secret() {
    let (app, _database, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {
                "kind": "kafkaProtocol",
                "bootstrapServers": "localhost:9092",
            },
            "topic": "dbt.runs",
            "consumerGroup": "graph-owl",
            "mapping": "dbt-run-completed",
            "secret": "sasl-secret-do-not-leak",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["hasSecret"], true, "{body}");
    assert!(body.get("secret").is_none(), "{body}");
    let body_string = body.to_string();
    assert!(
        !body_string.contains("sasl-secret-do-not-leak"),
        "the secret came back in the response that stored it: {body_string}"
    );

    let (status, listed) = send(&app, "GET", "/streaming/subscriptions", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed_string = listed.to_string();
    assert!(
        !listed_string.contains("sasl-secret-do-not-leak"),
        "the secret came back in a later read: {listed_string}"
    );
    assert!(
        listed_string.contains("\"topic\":\"dbt.runs\""),
        "{listed_string}"
    );
}

/// Defaults documented in the request DTO — `startPosition: latest`,
/// `maxInFlight: 100`, `poisonThreshold: 3`, `enabled: true` — actually take
/// effect when a caller omits them, not just compile.
#[tokio::test]
async fn omitted_fields_default_sensibly() {
    let (app, _database, _) = test_app().await;
    let created = register(&app, "dbt.runs", "graph-owl").await;

    assert_eq!(created["startPosition"]["kind"], "latest", "{created}");
    assert_eq!(created["maxInFlight"], 100, "{created}");
    assert_eq!(created["poisonThreshold"], 3, "{created}");
    assert_eq!(created["enabled"], true, "{created}");
}

/// One subscription per `(topic, consumerGroup)` pair — two registrations
/// naming the same pair would leave "which one actually runs" unanswered.
#[tokio::test]
async fn registering_the_same_topic_and_group_twice_is_a_conflict() {
    let (app, _database, _) = test_app().await;
    register(&app, "dbt.runs", "graph-owl").await;

    let (status, body) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {
                "kind": "kafkaProtocol",
                "bootstrapServers": "localhost:9092",
            },
            "topic": "dbt.runs",
            "consumerGroup": "graph-owl",
            "mapping": "a-different-mapping",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// An empty `topic`/`consumerGroup`/`mapping` is refused with `400` rather
/// than stored and left to fail later against the database's own `CHECK`
/// constraints — same rule as webhook endpoint registration.
#[tokio::test]
async fn a_blank_topic_is_refused() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {
                "kind": "kafkaProtocol",
                "bootstrapServers": "localhost:9092",
            },
            "topic": "",
            "consumerGroup": "graph-owl",
            "mapping": "dbt-run-completed",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Admin-only, same reasoning as webhook endpoints and connector configs: a
/// subscription holds a credential and decides what an external system may
/// write into the catalog on an ongoing basis.
#[tokio::test]
async fn a_non_admin_cannot_register_or_list_subscriptions() {
    let (app, _database, _) = test_app_with_secret(SECRET).await;

    let register_request = Request::builder()
        .method("POST")
        .uri("/streaming/subscriptions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::from(
            json!({
                "broker": {
                    "kind": "kafkaProtocol",
                    "bootstrapServers": "localhost:9092",
                },
                "topic": "dbt.runs",
                "consumerGroup": "graph-owl",
                "mapping": "dbt-run-completed",
            })
            .to_string(),
        ))
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(register_request)
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let list_request = Request::builder()
        .method("GET")
        .uri("/streaming/subscriptions")
        .header("authorization", format!("Bearer {}", token("mallory")))
        .body(Body::empty())
        .expect("request should build");
    let response = app.clone().oneshot(list_request).await.expect("handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// A Pulsar broker is accepted by the same endpoint — the registration
/// surface does not special-case Kafka.
#[tokio::test]
async fn a_pulsar_broker_can_be_registered() {
    let (app, _database, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {
                "kind": "pulsar",
                "serviceUrl": "pulsar://localhost:6650",
            },
            "topic": "dbt.runs",
            "consumerGroup": "graph-owl",
            "mapping": "dbt-run-completed",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["broker"]["kind"], "pulsar", "{body}");
}

async fn register_mapping(app: &axum::Router, name: &str) {
    let (status, body) = send(
        app,
        "POST",
        "/webhooks/mappings",
        Some(json!({
            "name": name,
            "kind": {"kind": "literal", "value": "service"},
            "entityName": {"kind": "path", "pointer": "/name"},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

async fn produce(bootstrap_servers: &str, topic: &str, payload: &Value) {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", "v4")
        .create()
        .expect("producer should build");
    producer
        .send(
            FutureRecord::to(topic)
                .payload(&payload.to_string())
                .key("test-key"),
            Duration::from_secs(10),
        )
        .await
        .expect("send should succeed");
}

/// **The criterion this slice exists to prove, end to end.** Nothing in this
/// test calls `apply_streamed_message` or the low-level consumer directly —
/// only the HTTP surface a real deployment has: register a mapping, register
/// a subscription, produce a message to the real broker the subscription
/// names, and poll for the entity it maps to.
#[tokio::test]
async fn a_produced_message_becomes_a_real_entity() {
    let (app, _database, _) = test_app().await;
    let bootstrap_servers = common::kafka_bootstrap_servers().await;
    let topic = common::unique_topic();

    register_mapping(&app, "streaming-e2e").await;
    let (status, subscription) = send(
        &app,
        "POST",
        "/streaming/subscriptions",
        Some(json!({
            "broker": {
                "kind": "kafkaProtocol",
                "bootstrapServers": bootstrap_servers,
            },
            "topic": topic,
            "consumerGroup": "graph-owl-e2e",
            "mapping": "streaming-e2e",
            "startPosition": {"kind": "earliest"},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{subscription}");

    produce(
        &bootstrap_servers,
        &topic,
        &json!({"name": "streamed-orders"}),
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (status, listed) = send(&app, "GET", "/assets?kind=service", None).await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        if listed["data"]
            .as_array()
            .expect("data")
            .iter()
            .any(|asset| asset["name"] == "streamed-orders")
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the streamed entity never appeared: {listed}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
