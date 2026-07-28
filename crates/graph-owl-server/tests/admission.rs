//! Epic 10: admission control, at the HTTP surface.
//!
//! The semaphore's own behaviour is unit-tested next to its definition. What
//! only a request can show is that it is *reached*, that it is reached on the
//! right routes, and that a rejection is shaped like every other error this API
//! produces — a correct limit that no route consults sheds nothing.
//!
//! Every test here holds its permit directly, through the same `Arc` the router
//! was built with. That is deliberate: driving saturation with a second
//! in-flight request would need a slow handler and a sleep, and a concurrency
//! test with a sleep in it is a flake with a schedule.

mod common;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app_with_admission};
use graph_owl_server::admission::{Admission, Class};
use tower::ServiceExt;

/// One permit per class, so a single held permit saturates.
fn one_each() -> Arc<Admission> {
    Arc::new(Admission::with_limits(
        &[(Class::Query, 1), (Class::Ingestion, 1)],
        1,
    ))
}

async fn post(app: &axum::Router, uri: &str, body: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled")
}

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled")
}

async fn text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

/// **The slice's reason for existing.** A saturated path refuses, and refuses
/// in the shape `00d-api-conventions.md` requires of every error: a problem
/// document with a stable type, plus the `Retry-After` that makes a `503`
/// actionable rather than an invitation to hammer.
#[tokio::test]
async fn a_saturated_query_path_refuses_with_a_problem_document_and_a_retry_after() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _held = admission
        .try_admit(Class::Query)
        .expect("the first permit is available");

    let response = post(
        &app,
        "/sparql",
        r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#,
    )
    .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/problem+json")
    );
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
        Some("1"),
        "a 503 without Retry-After leaves every client to invent its own backoff"
    );

    let body = json_body(response).await;
    assert_eq!(body["type"], "https://graph-owl.dev/errors/overloaded");
    assert_eq!(body["status"], 503);
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("query")),
        "the detail must name the class: {body}"
    );
}

/// The positive half, and the one that stops "reject everything" from passing:
/// with a permit free, the same request is admitted and reaches its handler.
#[tokio::test]
async fn the_same_request_is_admitted_when_a_permit_is_free() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let response = post(
        &app,
        "/sparql",
        r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#,
    )
    .await;

    assert_ne!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// **The test that keeps an overload diagnosable.** A server shedding load is
/// exactly when an operator scrapes it and when the load balancer probes it.
/// Controlling these paths turns a slowdown into an outage and blinds the
/// monitoring that would have explained it.
#[tokio::test]
async fn a_shed_server_still_answers_its_probes_and_its_scrape() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _query = admission.try_admit(Class::Query).expect("permit");
    let _ingest = admission.try_admit(Class::Ingestion).expect("permit");

    for uncontrolled in ["/health", "/ready", "/metrics"] {
        let status = get(&app, uncontrolled).await.status();
        assert_ne!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{uncontrolled} must not be shed"
        );
    }
}

/// And ordinary reads keep working. A limit that shed CRUD would make the
/// catalog unusable to protect the two paths that are expensive.
#[tokio::test]
async fn ordinary_reads_are_unaffected_by_a_saturated_expensive_path() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _query = admission.try_admit(Class::Query).expect("permit");

    assert_eq!(get(&app, "/assets").await.status(), StatusCode::OK);
}

/// **Isolation, end to end.** A connector storm must not shed reads. One shared
/// semaphore passes every other test in this file and fails this one.
#[tokio::test]
async fn a_saturated_ingestion_path_does_not_shed_queries() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _ingest = admission.try_admit(Class::Ingestion).expect("permit");

    let response = post(
        &app,
        "/sparql",
        r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#,
    )
    .await;

    assert_ne!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// A permit is held for the whole request, not just its arrival. Releasing on
/// entry would make the limit count arrivals rather than concurrency, which is
/// no limit at all — and the way to see that from outside is that the path
/// recovers once the request finishes.
#[tokio::test]
async fn the_path_recovers_when_the_permit_is_released() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let held = admission.try_admit(Class::Query).expect("permit");
    let refused = post(
        &app,
        "/sparql",
        r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#,
    )
    .await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);

    drop(held);

    let admitted = post(
        &app,
        "/sparql",
        r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#,
    )
    .await;
    assert_ne!(admitted.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// The rejection is visible to an operator, under the names the observability
/// contract's `graph_owl_http_*` prefix requires. A shed request that no metric
/// records is an overload they hear about from a customer.
#[tokio::test]
async fn a_rejection_is_counted_and_the_permits_are_gauged() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _held = admission.try_admit(Class::Query).expect("permit");
    let _ = post(
        &app,
        "/sparql",
        r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#,
    )
    .await;

    let scrape = text(get(&app, "/metrics").await).await;

    assert!(
        scrape.contains("graph_owl_http_admission_rejections_total"),
        "no rejection counter in the scrape:\n{scrape}"
    );
    assert!(
        scrape.contains("graph_owl_http_admission_permits_available"),
        "no availability gauge in the scrape:\n{scrape}"
    );
    assert!(
        scrape.contains("graph_owl_http_admission_permits_held"),
        "no occupancy gauge in the scrape:\n{scrape}"
    );
    assert!(
        scrape.contains(r#"class="query""#),
        "the class label is what makes the counter actionable:\n{scrape}"
    );
}

/// Admission runs **inside** the observability layer, so a shed request is
/// still logged and still gets its id echoed. The id is the only thing
/// correlating a client's report of a `503` with the server's own record of it.
#[tokio::test]
async fn a_shed_request_still_carries_its_request_id() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _held = admission.try_admit(Class::Query).expect("permit");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sparql")
                .header("content-type", "application/json")
                .header("x-request-id", "shed-me-42")
                .body(Body::from(r#"{"query":"SELECT * WHERE { ?s ?p ?o }"}"#))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("shed-me-42")
    );
}

/// The refusal happens before the body is read. A server that parses a request
/// it is about to reject is doing the expensive part of the work it just
/// decided it had no capacity for.
#[tokio::test]
async fn a_refused_request_is_not_parsed_first() {
    let admission = one_each();
    let (app, _container, _) = test_app_with_admission(&admission).await;

    let _held = admission.try_admit(Class::Query).expect("permit");

    // Missing `query` entirely — a `400` from validation would prove the body
    // was read before the limit was consulted.
    let response = post(&app, "/sparql", r#"{"nonsense":true}"#).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
