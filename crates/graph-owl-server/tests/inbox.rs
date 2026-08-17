//! `GET /inbox` at the wire — Plan 122a A1.
//!
//! `inbox_merge` (`graph-owl-server`'s own `--lib` unit tests) proves the
//! merge logic against all five source types with no database at all. What
//! only an HTTP test can see: that the handler is really wired to the live
//! `Catalog` — real Postgres-backed reads, not five hand-built Rust values —
//! for the three sources cheap enough to seed through the public API
//! (change proposals, findings, extraction claims). Agent proposals and the
//! resolution queue are exercised only by `inbox_merge`: the former has no
//! public HTTP route to create one at all, and the latter requires driving
//! real entity-resolution scoring into the ambiguous band, which
//! `tests/entity_resolution.rs` already covers as its own concern.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use graph_owl_core::finding::{Evidence, Finding};
use graph_owl_storage::FindingStore;
use graph_owl_storage_postgres::PostgresStorage;
use serde_json::{Value, json};
use tower::ServiceExt;

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

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should be handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, parsed)
}

async fn seed_finding(connection_string: &str) {
    let store = PostgresStorage::connect(connection_string)
        .await
        .expect("connect");
    let finding = Finding::new(
        "gst",
        "gst:MissingInGstr2b",
        "1025:inv-1",
        "claimed, never filed",
        "gst:Section16",
        vec![Evidence {
            subject: "1025:inv-1".to_string(),
            predicate: "taxAmount".to_string(),
            value: "45000".to_string(),
            var: None,
        }],
    )
    .expect("a complete finding");
    store.record_finding(&finding).await.expect("record");
}

async fn seed_change_proposal(app: &axum::Router) {
    let (status, asset) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "inbox-fixture-orders" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{asset}");
    let id = asset["id"].as_str().expect("an id");

    let (status, proposal) = send(
        app,
        "POST",
        &format!("/assets/{id}/change-proposals"),
        Some(json!({
            "field": "description",
            "currentValue": Value::Null,
            "proposedValue": "a better description",
            "rationale": "the old one was empty",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{proposal}");
    assert_eq!(proposal["status"], "pending");
}

async fn seed_extraction_claim(app: &axum::Router) {
    let (status, asset) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "inbox-fixture-payments" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{asset}");
    let fqn = asset["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string();

    let submission = json!({
        "document": {
            "sourceId": "runbook.md",
            "mediaType": "markdown",
            "text": "The payments service is append-only and owned by finance.",
        },
        "result": {
            "claims": [{
                "subject": fqn,
                "predicate": "description",
                "object": "append-only",
                // Mid-band: neither auto-asserted nor auto-discarded, so it
                // lands in the pending queue `/extraction/queue` — and
                // therefore `/inbox` — reads from.
                "confidence": 0.6,
                "provenance": {
                    "sourceId": "runbook.md",
                    "extractor": "pdf-worker",
                    "extractorVersion": "1",
                    "extractedAt": "2026-08-02T00:00:00Z",
                    "evidence": { "kind": "text", "location": { "start": 4, "end": 18 } },
                },
            }],
        },
        "extractor": "pdf-worker",
        "extractorVersion": "1",
    });
    let (status, outcome) = send(app, "POST", "/extraction/runs", Some(submission)).await;
    assert_eq!(status, StatusCode::CREATED, "{outcome}");
}

#[tokio::test]
async fn the_inbox_aggregates_real_pending_items_from_three_independently_owned_queues() {
    let (app, _db, url) = test_app().await;

    seed_change_proposal(&app).await;
    seed_finding(&url).await;
    seed_extraction_claim(&app).await;

    let (status, body) = send(&app, "GET", "/inbox", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The per-source counts, not just a total — the same "one queue going
    // to zero is invisible in a bare total" reasoning `inbox_merge`'s own
    // tests assert against a total of zero, verified here against a live
    // aggregation instead of hand-built values.
    assert!(
        body["counts"]["changeProposals"].as_u64().unwrap_or(0) >= 1,
        "{body}"
    );
    assert!(
        body["counts"]["findings"].as_u64().unwrap_or(0) >= 1,
        "{body}"
    );
    assert!(
        body["counts"]["extractionClaims"].as_u64().unwrap_or(0) >= 1,
        "{body}"
    );

    let items = body["items"].as_array().expect("an items array");
    let sources: std::collections::HashSet<&str> = items
        .iter()
        .map(|item| item["source"].as_str().expect("a source"))
        .collect();
    assert!(sources.contains("change-proposal"), "{body}");
    assert!(sources.contains("finding"), "{body}");
    assert!(sources.contains("extraction-claim"), "{body}");

    // Every item carries the shape the console's inbox drawer renders
    // directly, on the real queue, not only in `inbox_merge`'s
    // hand-constructed fixtures.
    for item in items {
        assert!(item["id"].is_string(), "{item}");
        assert!(item["tag"].is_string(), "{item}");
        assert!(item["title"].is_string(), "{item}");
    }
}

#[tokio::test]
async fn an_inbox_with_nothing_pending_is_a_real_empty_response_not_an_error() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(&app, "GET", "/inbox", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"].as_array().expect("an array").len(), 0);
}
