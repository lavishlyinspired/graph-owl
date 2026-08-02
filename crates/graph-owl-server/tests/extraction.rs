//! Epic 21 at the wire — the surface an out-of-process worker submits to.
//!
//! **This is the test that makes the ports real rather than theoretical.** The
//! domain tests prove the bands and the vocabulary; this proves that a worker
//! which has never linked against this codebase can hand over JSON and be told
//! what it bought. Every request here is one a Python worker sends, in the
//! shape it sends it — which is why the bodies are written as literal JSON
//! rather than built from the Rust types. A test that serialised the domain
//! types would pass even if the wire names drifted, and the wire names are the
//! contract.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
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

/// A `service`, because it is a root kind — a `table` needs a parent, and
/// building a hierarchy in every test would be scaffolding that proves nothing
/// about extraction.
async fn known_asset(app: &axum::Router, name: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string()
}

const SOURCE: &str = "The orders service is append-only and owned by payments.";

/// The shape a worker sends. Written as literal JSON on purpose — see the
/// module note.
fn submission(subject: &str, confidence: f64, text: &str) -> Value {
    json!({
        "document": {
            "sourceId": "runbook.md",
            "mediaType": "markdown",
            "text": text,
        },
        "result": {
            "claims": [{
                "subject": subject,
                "predicate": "description",
                "object": "append-only",
                "confidence": confidence,
                "provenance": {
                    "sourceId": "runbook.md",
                    "extractor": "pdf-worker",
                    "extractorVersion": "1",
                    "extractedAt": "2026-08-02T00:00:00Z",
                    "evidence": { "start": 4, "end": 18 },
                },
            }],
        },
        "extractor": "pdf-worker",
        "extractorVersion": "1",
    })
}

// ── the bands decide, not the worker ────────────────────────────────────────

/// **The core of decision 3 at the wire.** A worker claiming high confidence
/// asserts; the same worker claiming middling confidence waits for a human.
/// Both requests are identical apart from one number, which is the point — the
/// worker's *only* influence is that number, and graph-owl decides what it buys.
#[tokio::test]
async fn confidence_decides_whether_a_claim_asserts_or_waits() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.9, SOURCE)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["asserted"], 1, "0.9 is above the assert threshold");
    assert_eq!(body["surfaced"], 0, "{body}");

    // The same claim at 0.6 waits instead, and the queue is where it waits.
    let (_, queued) = send(&app, "GET", "/extraction/queue", None).await;
    assert_eq!(
        queued.as_array().expect("an array").len(),
        0,
        "an asserted claim must not be queued for review — it was never in doubt"
    );
}

#[tokio::test]
async fn a_middling_confidence_claim_queues_with_the_sentence_it_came_from() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["surfaced"], 1, "{body}");

    let (status, queued) = send(&app, "GET", "/extraction/queue", None).await;
    assert_eq!(status, StatusCode::OK);
    let claims = queued.as_array().expect("an array");
    assert_eq!(claims.len(), 1, "{queued}");

    // **Decision 5 made usable.** A reviewer shown a bare triple is being asked
    // to trust the extractor, which is the thing under review.
    assert_eq!(
        claims[0]["evidence"], "orders service",
        "the queue must carry the source text, not just the span: {queued}"
    );
}

/// Below the surface threshold nothing is stored, and the count says so rather
/// than the claim vanishing — a run that silently dropped its output is
/// indistinguishable from one that found nothing.
#[tokio::test]
async fn a_low_confidence_claim_is_discarded_and_counted() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.2, SOURCE)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["discarded"], 1, "{body}");
    assert_eq!(body["asserted"], 0);
    assert_eq!(body["surfaced"], 0);
}

// ── decision 1: the vocabulary is graph-owl's, not the worker's ─────────────

/// **The guarantee that makes this ontology-constrained extraction rather than
/// open information extraction.** A worker inventing a predicate is refused
/// however confident it is — confidence is not the failing, and no amount of it
/// would make the claim storable.
#[tokio::test]
async fn an_off_ontology_predicate_is_discarded_however_confident_the_worker_is() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let mut payload = submission(&fqn, 0.99, SOURCE);
    payload["result"]["claims"][0]["predicate"] = json!("isFriendsWith");

    let (status, body) = send(&app, "POST", "/extraction/runs", Some(payload)).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["discarded"], 1, "{body}");
    assert_eq!(
        body["asserted"], 0,
        "0.99 must not buy a predicate the model does not have"
    );
}

/// A claim about an entity the catalog has never heard of cannot be attached to
/// anything, so it is refused rather than stored pointing at nothing.
#[tokio::test]
async fn a_claim_about_an_unknown_entity_is_discarded() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission("nothing.like.this", 0.95, SOURCE)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["discarded"], 1, "{body}");
    assert_eq!(body["asserted"], 0);
}

// ── idempotent re-ingestion ────────────────────────────────────────────────

/// Re-submitting an unchanged document does nothing and **says** it did
/// nothing. A worker retrying after a timeout needs to tell "already had this"
/// apart from "found nothing in it", and a silent 201 with zero counts would
/// look like the latter.
#[tokio::test]
async fn re_submitting_an_unchanged_document_is_recognised_not_repeated() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let (first_status, first) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;
    assert_eq!(first_status, StatusCode::CREATED, "{first}");

    let (second_status, second) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;

    assert_eq!(second_status, StatusCode::OK, "{second}");
    assert_eq!(second["outcome"], "alreadyExtracted", "{second}");
    // **Asserted present before asserted equal.** Two absent keys compare equal,
    // so the equality alone passed while `runId` was going out as `run_id` —
    // a vacuous pass that hid a real wire-shape bug until a different test
    // tried to *use* the id.
    assert!(
        first["runId"].is_string(),
        "the wire name must be camelCase: {first}"
    );
    assert_eq!(
        second["runId"], first["runId"],
        "the same document must resolve to the run that already handled it"
    );

    // And nothing was queued twice.
    let (_, queued) = send(&app, "GET", "/extraction/queue", None).await;
    assert_eq!(queued.as_array().expect("an array").len(), 1, "{queued}");
}

/// **The negative half, and the one a mutation survives without.** An *edited*
/// document must be re-read — idempotence keyed on the source id alone would
/// freeze a document at whatever its first version said, which is silent and
/// permanent.
#[tokio::test]
async fn an_edited_document_is_extracted_again() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;

    let edited = "The orders service is append-only, partitioned by day.";
    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, edited)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["outcome"], "recorded",
        "an edit must re-extract: {body}"
    );
}

// ── the review queue, and what a decision persists ─────────────────────────

/// **Human curation must survive re-processing** — the same rule as Epic 15's
/// hand-edit preservation. A reviewer who said no must not be asked again by
/// the next run of the same extractor over an edited document.
#[tokio::test]
async fn a_rejected_claim_is_not_re_queued_when_the_document_is_re_ingested() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;
    let (_, queued) = send(&app, "GET", "/extraction/queue", None).await;
    let claim_id = queued[0]["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/extraction/claims/{claim_id}/decision"),
        Some(json!({ "confirmed": false })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // The document changes, so this is a genuinely new run — which is exactly
    // the case a rejection keyed on the run id would fail to suppress.
    let edited = "The orders service is append-only. Reviewed by nobody.";
    let (_, second) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, edited)),
    )
    .await;
    assert_eq!(
        second["surfaced"], 0,
        "a reviewer already answered this: {second}"
    );
    assert_eq!(second["discarded"], 1, "{second}");

    let (_, queue) = send(&app, "GET", "/extraction/queue", None).await;
    assert!(
        queue.as_array().expect("an array").is_empty(),
        "the queue must not re-ask a rejected question: {queue}"
    );
}

/// Confirming clears the claim from the queue. The distinction from rejection
/// is what happens on re-ingestion, which the test above pins.
#[tokio::test]
async fn confirming_a_claim_takes_it_out_of_the_queue() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;
    let (_, queued) = send(&app, "GET", "/extraction/queue", None).await;
    let claim_id = queued[0]["id"].as_str().expect("an id").to_string();

    let (status, _) = send(
        &app,
        "POST",
        &format!("/extraction/claims/{claim_id}/decision"),
        Some(json!({ "confirmed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, queue) = send(&app, "GET", "/extraction/queue", None).await;
    assert!(queue.as_array().expect("an array").is_empty(), "{queue}");
}

/// A review decision has no default. Both directions of a default are wrong:
/// true asserts what nobody approved, false rejects what nobody refused.
#[tokio::test]
async fn a_decision_without_a_verdict_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/extraction/claims/{}/decision", uuid::Uuid::new_v4()),
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn deciding_a_claim_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/extraction/claims/{}/decision", uuid::Uuid::new_v4()),
        Some(json!({ "confirmed": true })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

// ── a bad run is deletable wholesale ───────────────────────────────────────

/// **What decision 0 buys.** A mis-prompted model or a broken OCR pass is one
/// delete, not a hunt through the graph for facts nothing can attribute — and
/// the claims must go with the run, or the delete leaves exactly the orphans it
/// exists to prevent.
#[tokio::test]
async fn deleting_a_run_takes_its_claims_with_it() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let (_, submitted) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission(&fqn, 0.6, SOURCE)),
    )
    .await;
    let run_id = submitted["runId"].as_str().expect("a run id").to_string();

    let (_, before) = send(&app, "GET", "/extraction/queue", None).await;
    assert_eq!(before.as_array().expect("an array").len(), 1);

    let (status, _) = send(&app, "DELETE", &format!("/extraction/runs/{run_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, after) = send(&app, "GET", "/extraction/queue", None).await;
    assert!(
        after.as_array().expect("an array").is_empty(),
        "a deleted run must not leave claims nothing can attribute: {after}"
    );
}

#[tokio::test]
async fn deleting_a_run_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/extraction/runs/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── the run's identity must be stated ──────────────────────────────────────

/// A blank extractor name would make every worker look like the same one, so a
/// second worker over the same document would be mistaken for a re-run of the
/// first and silently do nothing.
#[tokio::test]
async fn a_submission_without_an_extractor_identity_is_refused() {
    let (app, _db, _url) = test_app().await;
    let fqn = known_asset(&app, "orders").await;

    let mut payload = submission(&fqn, 0.9, SOURCE);
    payload["extractor"] = json!("  ");

    let (status, body) = send(&app, "POST", "/extraction/runs", Some(payload)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **All the failures at once, not the first.** A worker author fixing one
/// field per round trip is the cost this codebase's accumulating validator
/// exists to avoid.
#[tokio::test]
async fn every_missing_identity_field_is_reported_in_one_response() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(json!({
            "document": { "sourceId": "", "mediaType": "markdown", "text": "x" },
            "result": { "claims": [] },
            "extractor": "",
            "extractorVersion": "",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let reported = body.to_string();
    assert!(reported.contains("extractor"), "{reported}");
    assert!(reported.contains("extractorVersion"), "{reported}");
    assert!(reported.contains("sourceId"), "{reported}");
}
