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

    // **Decision 5 made usable, widened for Epic 42 Slice D.** A reviewer
    // shown a bare triple is being asked to trust the extractor, which is
    // the thing under review — and the whole sentence, not just the matched
    // phrase, is what lets them judge it in context. `SOURCE` is one
    // sentence, so the passage is the whole thing.
    assert_eq!(claims[0]["passage"], SOURCE, "{queued}");
    let span = claims[0]["span"].as_array().expect("a [start, end] span");
    let start = usize::try_from(span[0].as_u64().expect("start")).expect("fits");
    let end = usize::try_from(span[1].as_u64().expect("end")).expect("fits");
    assert_eq!(
        &claims[0]["passage"].as_str().expect("passage")[start..end],
        "orders service",
        "the span must point at the extracted phrase within the passage: {queued}"
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

// ── a mention is resolved, not string-matched ──────────────────────────────

/// A `service` → `database` → `schema` → `table` chain, returning the table's
/// fully-qualified name and the service's name. A root-kind asset cannot be
/// resolved by mention at all — it has no ancestors, so context can contribute
/// nothing and the score tops out at exactly the threshold, which does not
/// clear it. Which is correct, and means this test needs depth.
async fn nested_table(app: &axum::Router) -> String {
    let mut parent: Option<String> = None;
    let mut fqn = String::new();
    for (kind, name) in [
        ("service", "warehouse"),
        ("database", "sales"),
        ("schema", "public"),
        ("table", "orders"),
    ] {
        let mut body = json!({ "kind": kind, "name": name });
        if let Some(parent_id) = &parent {
            body["parentId"] = json!(parent_id);
        }
        let (status, created) = send(app, "POST", "/assets", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        parent = Some(created["id"].as_str().expect("an id").to_string());
        fqn = created["fullyQualifiedName"]
            .as_str()
            .expect("an fqn")
            .to_string();
    }
    fqn
}

/// The same submission, with the evidence span the caller chooses — the span is
/// what the mention is scored against, so a test about resolution has to control
/// it rather than inherit a fixed one.
fn submission_spanning(subject: &str, confidence: f64, text: &str) -> Value {
    let mut payload = submission(subject, confidence, text);
    payload["result"]["claims"][0]["provenance"]["evidence"] =
        json!({ "start": 0, "end": text.len() });
    payload
}

/// **The identity path extraction actually needs.** A worker reads "the orders
/// table" in prose; the catalog stores facts against an entity id. Matching the
/// subject to a fully-qualified name by string equality answers that question
/// only when the document happens to spell the FQN — and answers it *wrongly*
/// the rest of the time by concluding the entity is unknown, which is how the
/// same real table ends up described twice.
#[tokio::test]
async fn a_mention_that_is_not_a_fully_qualified_name_still_resolves() {
    let (app, _db, url) = test_app().await;
    let fqn = nested_table(&app).await;
    assert!(fqn.contains('.'), "the table should be nested: {fqn}");

    let text = "The orders table in warehouse is append-only.";
    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission_spanning("orders", 0.9, text)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["asserted"], 1,
        "`orders` is not the table's FQN, and it is unambiguously the table: {body}"
    );

    // And the resolution is **recorded**, so "why is this fact attached to this
    // table" has an answer that is not "the extractor said so".
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let recorded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mention_resolutions")
        .fetch_one(&pool)
        .await
        .expect("the resolutions should be readable");
    assert_eq!(
        recorded, 1,
        "a scored resolution is provenance, not a guess"
    );
}

/// **The negative, which is what makes the widening above safe.** Resolution is
/// a threshold, not a nearest-neighbour: a mention that names nothing in the
/// catalog must still be discarded, or extraction would attach every sentence
/// to whichever entity happened to score least badly.
#[tokio::test]
async fn a_mention_that_names_nothing_is_still_discarded() {
    let (app, _db, url) = test_app().await;
    nested_table(&app).await;

    let text = "The zzqxw table in warehouse is append-only.";
    let (status, body) = send(
        &app,
        "POST",
        "/extraction/runs",
        Some(submission_spanning("zzqxw", 0.95, text)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["asserted"], 0,
        "0.95 confidence in a name nobody has is still a claim about nothing: {body}"
    );
    assert_eq!(body["discarded"], 1, "{body}");

    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    let recorded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mention_resolutions")
        .fetch_one(&pool)
        .await
        .expect("the resolutions should be readable");
    assert_eq!(recorded, 0, "nothing resolved, so nothing is recorded");
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
        Some(json!({ "outcome": "reject", "reason": "not a real assertion" })),
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
        Some(json!({ "outcome": "accept" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, queue) = send(&app, "GET", "/extraction/queue", None).await;
    assert!(queue.as_array().expect("an array").is_empty(), "{queue}");
}

/// **Edit is a third outcome, not confirm-with-extra-steps.** The reviewer's
/// corrected subject/predicate/object are what reach the graph — the
/// extractor's original claim is not projected alongside it.
#[tokio::test]
async fn editing_a_claim_projects_the_correction_not_the_original() {
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
    assert_eq!(queued[0]["object"], "append-only", "{queued}");

    let (status, body) = send(
        &app,
        "POST",
        &format!("/extraction/claims/{claim_id}/decision"),
        Some(json!({
            "outcome": "edit",
            "subject": fqn,
            "predicate": "description",
            "object": "append-only, verified by a human",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (_, queue) = send(&app, "GET", "/extraction/queue", None).await;
    assert!(
        queue.as_array().expect("an array").is_empty(),
        "an edited claim is decided, not left pending: {queue}"
    );
}

/// A rejection with no reason teaches the extractor nothing and is
/// unauditable later — Epic 42 decision 3, the same rule the merge
/// adjudication queue enforces.
#[tokio::test]
async fn rejecting_without_a_reason_is_refused() {
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
        Some(json!({ "outcome": "reject" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = send(
        &app,
        "POST",
        &format!("/extraction/claims/{claim_id}/decision"),
        Some(json!({ "outcome": "reject", "reason": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (_, queue) = send(&app, "GET", "/extraction/queue", None).await;
    assert_eq!(
        queue.as_array().expect("an array").len(),
        1,
        "a refused decision must not have decided the claim: {queue}"
    );
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
        Some(json!({ "outcome": "accept" })),
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

// ── the reasoning boundary, in both directions ──────────────────────────────
//
// **This is the property decision 2 is actually about**, and until this test
// existed the epic could not tell whether it held. An asserted claim used to
// stop at `extraction_claims`: reasoning could not see it, which looked exactly
// like containment working and was really the facts never arriving. The rule
// only means something if the *other* direction is also demonstrable — that a
// deployment which opts in gets the confirmed facts and still does not get the
// unconfirmed ones.

use graph_owl_api::{Catalog, UpsertAsset};
use graph_owl_core::extraction::{
    Claim, ExtractionResult, ParsedDocument, Provenance, ReviewDecision, TextSpan,
};
use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_core::{AssetKind, Principal, PrincipalKind};
use graph_owl_engine::TripleStore;

fn principal() -> Principal {
    Principal {
        id: "reviewer".to_string(),
        name: "Reviewer".to_string(),
        kind: PrincipalKind::User,
        roles: Vec::new(),
        is_admin: true,
    }
}

async fn service(catalog: &Catalog, name: &str) -> String {
    catalog
        .upsert_asset(
            &principal(),
            UpsertAsset {
                kind: AssetKind::Service,
                name: name.to_string(),
                parent_id: None,
                description: None,
                properties: None,
                extension: None,
            },
        )
        .await
        .expect("a service should be creatable")
        .fully_qualified_name
}

/// A document stating a dependency chain, with each claim's evidence pointing
/// at the sentence it came from.
fn chain(
    text: &str,
    links: &[(&str, &str)],
    confidence: f64,
) -> (ParsedDocument, ExtractionResult) {
    let document = ParsedDocument {
        source_id: "runbook.md".to_string(),
        media_type: "markdown".to_string(),
        text: text.to_string(),
        sections: Vec::new(),
    };
    let claims = links
        .iter()
        .map(|(from, to)| Claim {
            subject: (*from).to_string(),
            predicate: "dependsOn".to_string(),
            object: (*to).to_string(),
            confidence,
            provenance: Provenance {
                source_id: "runbook.md".to_string(),
                extractor: "runbook-rules".to_string(),
                extractor_version: "1".to_string(),
                extracted_at: chrono::Utc::now(),
                evidence: TextSpan::new(0, text.len()),
            },
        })
        .collect();
    (
        document,
        ExtractionResult {
            claims,
            discarded: Vec::new(),
        },
    )
}

/// `dsc:dependsOn` is transitive — stated in the **default** graph, where an
/// axiom belongs. The chain it closes over is the thing under test.
async fn state_transitivity(url: &str) {
    let graph = graph_owl_engine_postgres::PostgresTripleStore::connect(url)
        .await
        .expect("the graph engine should connect");
    let t = graph.next_time().await.expect("a transaction time");
    graph
        .assert_flakes(&[Flake {
            s: Sid::dsc("dependsOn"),
            p: Sid::new(namespace::RDF, "type"),
            o: FlakeValue::Ref(Sid::new(namespace::OWL, "TransitiveProperty")),
            cx: None,
            t,
            op: true,
        }])
        .await
        .expect("the axiom should be assertable");
}

fn opted_in() -> graph_owl_reasoning::Budget {
    graph_owl_reasoning::Budget {
        include_graphs: vec![Sid::dsc(graph_owl_core::extraction_run::EXTRACTION_GRAPH)],
        ..graph_owl_reasoning::Budget::default()
    }
}

/// **Direction one: an unconfirmed claim cannot affect reasoning.** Not because
/// the reasoner declines to use it, but because it is not in the graph at all —
/// and the opt-in budget is passed here deliberately, so the test proves the
/// claim is absent rather than merely filtered.
#[tokio::test]
async fn a_surfaced_claim_reaches_no_conclusion_even_for_a_deployment_that_opted_in() {
    let (catalog, _db, url) = common::test_catalog().await;
    state_transitivity(&url).await;

    let alpha = service(&catalog, "alpha").await;
    let beta = service(&catalog, "beta").await;
    let gamma = service(&catalog, "gamma").await;

    let text = "alpha depends on beta, and beta depends on gamma.";
    let (document, result) = chain(
        text,
        &[(&alpha, &beta), (&beta, &gamma)],
        // The surface band: evidence, not proof, so a human is asked.
        0.6,
    );
    let outcome = catalog
        .submit_extraction(&document, result, "runbook-rules", "1")
        .await
        .expect("the submission should be accepted");
    assert!(
        matches!(
            outcome,
            graph_owl_api::extraction::SubmissionOutcome::Recorded {
                surfaced: 2,
                asserted: 0,
                ..
            }
        ),
        "{outcome:?}"
    );

    let report = catalog
        .run_reasoning(&opted_in())
        .await
        .expect("reasoning should run");

    assert_eq!(
        report.derived, 0,
        "an unconfirmed machine claim must reach no conclusion — a guess laundered \
         into a derived fact carries provenance that looks like catalog truth"
    );
}

/// **Direction two: an asserted claim does affect reasoning, once a deployment
/// asks for it.** The same chain at a confidence the bands assert on, and the
/// same two runs — the only difference between the assertions below is the
/// budget, which is what makes this a test of the opt-in rather than of the
/// reasoner.
#[tokio::test]
async fn an_asserted_claim_is_reasoned_over_only_by_a_deployment_that_opted_in() {
    let (catalog, _db, url) = common::test_catalog().await;
    state_transitivity(&url).await;

    let alpha = service(&catalog, "alpha").await;
    let beta = service(&catalog, "beta").await;
    let gamma = service(&catalog, "gamma").await;

    let text = "alpha depends on beta, and beta depends on gamma.";
    let (document, result) = chain(text, &[(&alpha, &beta), (&beta, &gamma)], 0.9);
    catalog
        .submit_extraction(&document, result, "runbook-rules", "1")
        .await
        .expect("the submission should be accepted");

    // The default deployment does not reason over extraction.
    let closed = catalog
        .run_reasoning(&graph_owl_reasoning::Budget::default())
        .await
        .expect("reasoning should run");
    assert_eq!(
        closed.derived, 0,
        "extraction must not feed inference unless a deployment says it may"
    );

    // The same facts, the same rules, one different budget.
    let open = catalog
        .run_reasoning(&opted_in())
        .await
        .expect("reasoning should run");
    assert!(
        open.derived > 0,
        "an opted-in deployment must actually see the confirmed facts — before \
         this, `graph:extraction` was never loaded, so the rule above held for \
         the wrong reason and this direction was impossible"
    );

    // And the conclusion is the one the axiom implies, not merely *a* conclusion.
    let alpha_id = catalog
        .get_asset_by_fqn(&alpha)
        .await
        .expect("read")
        .expect("alpha exists")
        .id;
    let gamma_id = catalog
        .get_asset_by_fqn(&gamma)
        .await
        .expect("read")
        .expect("gamma exists")
        .id;
    let derived = catalog
        .derived_about(&graph_owl_core::projection::entity_sid(alpha_id))
        .await
        .expect("the overlay should be readable");
    assert!(
        derived.iter().any(|f| f.p == Sid::dsc("dependsOn")
            && f.o == FlakeValue::Ref(graph_owl_core::projection::entity_sid(gamma_id))),
        "alpha depends on gamma transitively: {derived:#?}"
    );
}

/// **The confirmation path closes the loop.** A claim the bands surfaced is
/// invisible; the same claim after a human says yes is not. This is the
/// transition decision 2 describes and nothing previously performed.
#[tokio::test]
async fn confirming_a_surfaced_claim_makes_it_reason_able() {
    let (catalog, _db, url) = common::test_catalog().await;
    state_transitivity(&url).await;

    let alpha = service(&catalog, "alpha").await;
    let beta = service(&catalog, "beta").await;
    let gamma = service(&catalog, "gamma").await;

    let text = "alpha depends on beta, and beta depends on gamma.";
    let (document, result) = chain(text, &[(&alpha, &beta), (&beta, &gamma)], 0.6);
    catalog
        .submit_extraction(&document, result, "runbook-rules", "1")
        .await
        .expect("the submission should be accepted");

    let queued = catalog.extraction_queue().await.expect("the queue reads");
    assert_eq!(queued.len(), 2, "{queued:#?}");

    for claim in &queued {
        catalog
            .decide_extraction_claim(claim.id, ReviewDecision::Accept, "reviewer")
            .await
            .expect("a reviewer should be able to confirm");
    }

    let report = catalog
        .run_reasoning(&opted_in())
        .await
        .expect("reasoning should run");

    assert!(
        report.derived > 0,
        "a human confirmed both links, so the chain must close: {report:?}"
    );
}

/// **Rejecting is not confirming.** The negative a mutation would otherwise
/// survive: a projection that ran on every decision rather than on a
/// confirmation would put rejected machine output into the graph, which is
/// precisely the claim a reviewer said was wrong.
#[tokio::test]
async fn rejecting_a_surfaced_claim_puts_nothing_in_the_graph() {
    let (catalog, _db, url) = common::test_catalog().await;
    state_transitivity(&url).await;

    let alpha = service(&catalog, "alpha").await;
    let beta = service(&catalog, "beta").await;
    let gamma = service(&catalog, "gamma").await;

    let text = "alpha depends on beta, and beta depends on gamma.";
    let (document, result) = chain(text, &[(&alpha, &beta), (&beta, &gamma)], 0.6);
    catalog
        .submit_extraction(&document, result, "runbook-rules", "1")
        .await
        .expect("the submission should be accepted");

    for claim in catalog.extraction_queue().await.expect("the queue reads") {
        catalog
            .decide_extraction_claim(
                claim.id,
                ReviewDecision::Reject {
                    reason: "not a real dependency".to_string(),
                },
                "reviewer",
            )
            .await
            .expect("a reviewer should be able to reject");
    }

    let report = catalog
        .run_reasoning(&opted_in())
        .await
        .expect("reasoning should run");

    assert_eq!(
        report.derived, 0,
        "a rejected claim must not be in the graph for anything to reason over"
    );
}

// ── the vocabulary and the registry are one thing ───────────────────────────

/// **A claim can now be accepted by the policy and refused by the engine**, and
/// nothing in the type system connects the two lists. `CATALOG_PREDICATES` is
/// what a worker's claims are checked against; the `predicates` table is what
/// `assert_flakes` checks against. A predicate in the first and not the second
/// passes review, projects, and fails at the write — where the only symptom is
/// a logged line and a fact that is not there.
#[tokio::test]
async fn every_predicate_a_worker_may_use_is_registered_in_the_engine() {
    let (_catalog, _db, url) = common::test_catalog().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");

    let registered: Vec<String> =
        sqlx::query_scalar("SELECT name FROM predicates WHERE namespace = 1")
            .fetch_all(&pool)
            .await
            .expect("the registry should be readable");

    for predicate in graph_owl_api::extraction::CATALOG_PREDICATES {
        assert!(
            registered.iter().any(|name| name == predicate),
            "`dsc:{predicate}` is in the extraction vocabulary but not in the \
             predicate registry, so an asserted claim using it would be refused \
             by `assert_flakes` after passing every check above it"
        );
    }
}
