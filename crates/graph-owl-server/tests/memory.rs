//! Epic 31 at the wire.
//!
//! The domain tests prove the decisions and the repository tests prove the
//! schema. This proves the **HTTP surface** — which is a separate claim, because
//! a handler can serialise anything it likes, and three of Slice A's and B's
//! acceptance criteria are about status codes and response bodies rather than
//! about stored rows.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{test_app, test_app_with_secret};
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

/// An asset to hang memories off, and its id.
///
/// `POST /assets`, not `POST /tables`: `tables` and `assets` are different
/// relations, and `memory_links.asset_target` is a foreign key into `assets`.
/// The first version of this fixture used `/tables` and every link was rejected
/// as unresolvable — the constraint telling the truth about which noun a memory
/// can be about.
async fn subject(app: &axum::Router, name: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/assets",
        // A `service`, because it is the one kind that is a root — a `table`
        // requires a parent, and building a four-level hierarchy in every test
        // would be scaffolding that proves nothing about memory. A service is a
        // legitimate thing to hold institutional knowledge about.
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("an id").to_string()
}

/// **Confidence is stated on purpose.**
///
/// Until Epic 12 lands, an unauthenticated request resolves to the `system`
/// principal, and `authorship_of` maps every non-person to *agent* authorship —
/// which must state its own confidence, because an agent that does not know how
/// sure it is has told you something important. So every fixture here exercises
/// the agent path, and the human default is unit tested in
/// `graph-owl-core::memory` where a `User` principal can be constructed.
///
/// The alternative — mapping `system` to human authorship so the fixtures read
/// more nicely — is exactly the relabelling the trust model refuses, and it would
/// have been invisible in this file.
fn memory_body(content: &str, subject: &str) -> Value {
    json!({
        "kind": "decision",
        "content": content,
        "confidence": 0.9,
        "links": [{ "relation": "about", "target": subject }],
    })
}

#[tokio::test]
async fn a_memory_is_created_and_read_back() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;

    let (created, body) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("Refunds are excluded from revenue.", &table)),
    )
    .await;

    assert_eq!(created, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("an id");
    // **Authorship comes from the principal, never the body.** A request that
    // could name its own author could forge one, and the whole trust model rests
    // on it. The body above names no author and could not; the `system`
    // principal this build resolves to is not a person, so it is recorded as an
    // agent — honestly, rather than as a plausible username.
    assert_eq!(body["authorship"]["kind"], "agent", "{body}");
    assert_eq!(body["authorship"]["agentId"], "system", "{body}");

    let (read, one) = send(&app, "GET", &format!("/memories/{id}"), None).await;
    assert_eq!(read, StatusCode::OK);
    assert_eq!(one["content"], "Refunds are excluded from revenue.");
}

// The confidence default rule from the side this build can reach: an agent must
// state its own, and is refused when it does not. "A human defaults to 1.0" is
// the same rule's other half and is unit tested — it needs Epic 12 to exercise
// over HTTP, because no request can currently resolve to a person.
#[tokio::test]
async fn an_agent_authored_memory_must_state_its_own_confidence() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({
            "kind": "decision",
            "content": "Refunds are excluded.",
            "links": [{ "relation": "about", "target": table }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("confidence"), "{body}");
}

// And the stated value is what is stored, rather than being replaced by a
// default — otherwise the test above would pass on a handler that silently
// substituted 1.0.
#[tokio::test]
async fn a_stated_confidence_is_kept_as_given() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;

    let (_, body) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("Refunds are excluded.", &table)),
    )
    .await;

    assert!((body["confidence"].as_f64().expect("a number") - 0.9).abs() < f64::EPSILON);
}

// "`About` is required (at least one)." An unanchored memory is written, stored
// and permanently unretrievable — worse than being refused at the door.
#[tokio::test]
async fn a_memory_with_no_anchor_is_refused_pointing_at_links() {
    let (app, _database, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({ "kind": "caveat", "content": "Orphan.", "links": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    // The field is named, so "add an about link" is the fix a client can read
    // off the response rather than infer.
    assert!(
        body.to_string().contains("links"),
        "the error should name the field: {body}"
    );
}

// "Confidence outside `[0,1]` → 400."
#[tokio::test]
async fn confidence_outside_the_unit_interval_is_refused() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let mut payload = memory_body("Too sure.", &table);
    payload["confidence"] = json!(1.5);

    let (status, body) = send(&app, "POST", "/memories", Some(payload)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("confidence"), "{body}");
}

// "A link to a nonexistent target → 400 naming the index." The index is the
// requirement: "one of your links is wrong" is not actionable with several.
#[tokio::test]
async fn an_unresolvable_link_is_rejected_naming_its_index() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let ghost = uuid::Uuid::new_v4();

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({
            "kind": "caveat",
            "content": "Read first.",
            "confidence": 0.9,
            "links": [
                { "relation": "about", "target": table },
                { "relation": "evidence", "target": ghost },
            ],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("links[1]"),
        "the error should name which link: {body}"
    );
}

// "Multiple links with different relations."
#[tokio::test]
async fn a_memory_keeps_several_relations_at_once() {
    let (app, _database, _) = test_app().await;
    let orders = subject(&app, "orders").await;
    let mart = subject(&app, "revenue_mart").await;

    let (status, body) = send(
        &app,
        "POST",
        "/memories",
        Some(json!({
            "kind": "incident",
            "content": "The nightly load double-counted refunds.",
            "confidence": 0.9,
            "links": [
                { "relation": "about", "target": orders },
                { "relation": "affects", "target": mart },
                { "relation": "mentions", "target": mart },
            ],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("an id");
    let (_, one) = send(&app, "GET", &format!("/memories/{id}"), None).await;
    assert_eq!(one["links"].as_array().expect("links").len(), 3, "{one}");
}

// Slice C at the wire: ranked, and each item carries its staleness verdict and
// the decomposed score — a ranking nobody can audit is one nobody should act on.
#[tokio::test]
async fn recall_returns_ranked_memories_with_staleness_and_score() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    for content in ["Refunds are excluded.", "The owner is finance."] {
        send(
            &app,
            "POST",
            "/memories",
            Some(memory_body(content, &table)),
        )
        .await;
    }

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/{table}/memories?q=refunds"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body.as_array().expect("an array");
    assert_eq!(items.len(), 2);
    // The better lexical match first, and the terms visible.
    assert_eq!(items[0]["memory"]["content"], "Refunds are excluded.");
    assert!(items[0]["score"]["total"].is_number(), "{body}");
    assert_eq!(items[0]["staleness"]["state"], "fresh", "{body}");
}

// An absent query is a real question — "everything we know about this table" —
// and must not produce `NaN`, which would poison the sort into an arbitrary
// order that still looks like a ranking.
#[tokio::test]
async fn recall_without_a_query_still_returns_an_ordered_answer() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("Refunds are excluded.", &table)),
    )
    .await;

    let (status, body) = send(&app, "GET", &format!("/assets/{table}/memories"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let total = body[0]["score"]["total"].as_f64().expect("a number");
    assert!(total.is_finite(), "score was not finite: {body}");
}

// Slice B at the wire: the correction is created, and the original stays
// readable rather than being overwritten.
#[tokio::test]
async fn a_correction_is_created_and_the_original_stays_readable() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let (_, original) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("Refunds are included.", &table)),
    )
    .await;
    let first = original["id"].as_str().expect("an id").to_string();

    let (status, correction) = send(
        &app,
        "POST",
        &format!("/memories/{first}/supersede"),
        Some(memory_body("Refunds are excluded.", &table)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{correction}");
    assert_eq!(correction["supersedes"], json!(first));

    let (read, before) = send(&app, "GET", &format!("/memories/{first}"), None).await;
    assert_eq!(read, StatusCode::OK, "the original must stay readable");
    assert_eq!(before["content"], "Refunds are included.");
    assert_eq!(before["supersededBy"], correction["id"]);
}

// "Retrieval returns only the current memory by default, superseded ones with
// `?include=superseded`."
#[tokio::test]
async fn superseded_memories_are_hidden_by_default_and_available_on_request() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let (_, original) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("Refunds are included.", &table)),
    )
    .await;
    let first = original["id"].as_str().expect("an id").to_string();
    send(
        &app,
        "POST",
        &format!("/memories/{first}/supersede"),
        Some(memory_body("Refunds are excluded.", &table)),
    )
    .await;

    let (_, current) = send(&app, "GET", &format!("/assets/{table}/memories"), None).await;
    let (_, history) = send(
        &app,
        "GET",
        &format!("/assets/{table}/memories?includeSuperseded=true"),
        None,
    )
    .await;

    assert_eq!(current.as_array().expect("array").len(), 1, "{current}");
    assert_eq!(history.as_array().expect("array").len(), 2, "{history}");
}

// "Superseding an already-superseded memory → 409 pointing at the current one."
// A client with only "no" cannot retry against the right target.
#[tokio::test]
async fn superseding_a_corrected_memory_conflicts_and_names_the_current_one() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let (_, original) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("First.", &table)),
    )
    .await;
    let first = original["id"].as_str().expect("an id").to_string();
    let (_, correction) = send(
        &app,
        "POST",
        &format!("/memories/{first}/supersede"),
        Some(memory_body("Second.", &table)),
    )
    .await;
    let current = correction["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/memories/{first}/supersede"),
        Some(memory_body("Also second.", &table)),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains(&current),
        "the 409 must name the current memory so a client can retry: {body}"
    );
}

// **The same body earns the same status from both endpoints.** An unresolvable
// link on `POST /memories` is a `400` naming the index; the correction path used
// to return `500` for it, so a client's fix depended on which endpoint they had
// sent it to. Caught by reading the adapter rather than by a failure, which is
// why it gets a test.
#[tokio::test]
async fn an_unresolvable_link_in_a_correction_is_a_400_not_a_500() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let (_, original) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("First.", &table)),
    )
    .await;
    let first = original["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "POST",
        &format!("/memories/{first}/supersede"),
        Some(json!({
            "kind": "decision",
            "content": "Second.",
            "confidence": 0.9,
            "links": [
                { "relation": "about", "target": table },
                { "relation": "evidence", "target": uuid::Uuid::new_v4() },
            ],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("links[1]"), "{body}");

    // And the original was not corrected by the failed attempt — a rejected
    // correction that still marked the original would leave a chain pointing at
    // a memory nobody stored.
    let (_, before) = send(&app, "GET", &format!("/memories/{first}"), None).await;
    assert_eq!(before["supersededBy"], Value::Null, "{before}");
}

#[tokio::test]
async fn superseding_something_absent_is_a_404() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let absent = uuid::Uuid::new_v4();

    let (status, _) = send(
        &app,
        "POST",
        &format!("/memories/{absent}/supersede"),
        Some(memory_body("Second.", &table)),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// Slice E at the wire: two current decisions about one asset surface as a
// candidate, and **neither is hidden**.
#[tokio::test]
async fn two_competing_decisions_surface_without_either_being_hidden() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let mut ids = Vec::new();
    for content in ["Refunds are included.", "Refunds are excluded."] {
        let (_, body) = send(
            &app,
            "POST",
            "/memories",
            Some(memory_body(content, &table)),
        )
        .await;
        ids.push(body["id"].as_str().expect("an id").to_string());
    }

    let (status, found) = send(
        &app,
        "GET",
        &format!("/assets/{table}/contradictions"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found.as_array().expect("array").len(), 1, "{found}");
    assert_eq!(found[0]["kind"], "candidate");

    // Both memories are still readable, and both still come back from recall.
    // A flagged pair is *added* to a queue, never subtracted from the corpus.
    for id in &ids {
        let (read, _) = send(&app, "GET", &format!("/memories/{id}"), None).await;
        assert_eq!(read, StatusCode::OK, "{id} was hidden by being flagged");
    }
    let (_, recalled) = send(&app, "GET", &format!("/assets/{table}/memories"), None).await;
    assert_eq!(recalled.as_array().expect("array").len(), 2, "{recalled}");
}

/// Two competing decisions about one asset, and their ids.
async fn competing(app: &axum::Router, table: &str) -> (String, Vec<String>) {
    let table = subject(app, table).await;
    let mut ids = Vec::new();
    for content in ["Refunds are included.", "Refunds are excluded."] {
        let (_, body) = send(app, "POST", "/memories", Some(memory_body(content, &table))).await;
        ids.push(body["id"].as_str().expect("an id").to_string());
    }
    (table, ids)
}

// "A human can confirm or dismiss a candidate; dismissal is recorded so the pair
// is not re-flagged."
#[tokio::test]
async fn a_dismissed_candidate_does_not_come_back() {
    let (app, _database, _) = test_app().await;
    let (table, ids) = competing(&app, "orders").await;

    let (reviewed, body) = send(
        &app,
        "POST",
        "/contradictions/reviews",
        // Deliberately the reverse of the order detection would report, so this
        // also proves normalisation at the wire: a verdict that only worked in one
        // order would let the queue reopen on its own.
        Some(json!({
            "a": ids[1],
            "b": ids[0],
            "verdict": "dismissed",
            "note": "different quarters",
        })),
    )
    .await;
    assert_eq!(reviewed, StatusCode::NO_CONTENT, "{body}");

    let (_, found) = send(
        &app,
        "GET",
        &format!("/assets/{table}/contradictions"),
        None,
    )
    .await;

    assert!(
        found.as_array().expect("array").is_empty(),
        "the dismissed pair came back: {found}"
    );
}

// **Confirming is not resolving.** The pair stays in the queue marked confirmed —
// a confirmed disagreement that vanished would read as settled, and settling one
// is the thing this epic refuses to do.
#[tokio::test]
async fn a_confirmed_candidate_stays_in_the_queue_marked_confirmed() {
    let (app, _database, _) = test_app().await;
    let (table, ids) = competing(&app, "orders").await;

    let (reviewed, body) = send(
        &app,
        "POST",
        "/contradictions/reviews",
        Some(json!({ "a": ids[0], "b": ids[1], "verdict": "confirmed" })),
    )
    .await;
    assert_eq!(reviewed, StatusCode::NO_CONTENT, "{body}");

    let (_, found) = send(
        &app,
        "GET",
        &format!("/assets/{table}/contradictions"),
        None,
    )
    .await;

    assert_eq!(found.as_array().expect("array").len(), 1, "{found}");
    assert_eq!(found[0]["kind"], "confirmed", "{found}");

    // And neither memory is hidden by having been confirmed.
    for id in &ids {
        let (read, _) = send(&app, "GET", &format!("/memories/{id}"), None).await;
        assert_eq!(read, StatusCode::OK, "{id} was hidden by being confirmed");
    }
}

// A reviewer changing their mind is one pair with a new verdict. A second click
// must not be a duplicate-key `500`.
#[tokio::test]
async fn a_reviewer_can_change_their_verdict_at_the_wire() {
    let (app, _database, _) = test_app().await;
    let (table, ids) = competing(&app, "orders").await;

    for verdict in ["confirmed", "dismissed"] {
        let (status, body) = send(
            &app,
            "POST",
            "/contradictions/reviews",
            Some(json!({ "a": ids[0], "b": ids[1], "verdict": verdict })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{verdict}: {body}");
    }

    let (_, found) = send(
        &app,
        "GET",
        &format!("/assets/{table}/contradictions"),
        None,
    )
    .await;

    assert!(
        found.as_array().expect("array").is_empty(),
        "the later dismissal did not take effect: {found}"
    );
}

// **No default verdict.** A verdict this endpoint had to guess would be a
// judgement about institutional disagreement made by the absence of a field.
#[tokio::test]
async fn a_review_without_a_verdict_is_refused() {
    let (app, _database, _) = test_app().await;
    let (_, ids) = competing(&app, "orders").await;

    let (status, _) = send(
        &app,
        "POST",
        "/contradictions/reviews",
        Some(json!({ "a": ids[0], "b": ids[1] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// A correction is **not** a contradiction — telling the two apart is most of
// Slice B's point, and it is why Slice E needed no time window.
#[tokio::test]
async fn a_correction_is_not_reported_as_a_contradiction() {
    let (app, _database, _) = test_app().await;
    let table = subject(&app, "orders").await;
    let (_, original) = send(
        &app,
        "POST",
        "/memories",
        Some(memory_body("Refunds are included.", &table)),
    )
    .await;
    let first = original["id"].as_str().expect("an id").to_string();
    send(
        &app,
        "POST",
        &format!("/memories/{first}/supersede"),
        Some(memory_body("Refunds are excluded.", &table)),
    )
    .await;

    let (_, found) = send(
        &app,
        "GET",
        &format!("/assets/{table}/contradictions"),
        None,
    )
    .await;

    assert!(
        found.as_array().expect("array").is_empty(),
        "a correction was reported as a conflict: {found}"
    );
}

#[tokio::test]
async fn an_absent_memory_is_a_404() {
    let (app, _database, _) = test_app().await;

    let (status, _) = send(
        &app,
        "GET",
        &format!("/memories/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- Epic 31: a person, not just `system`, authoring a memory ----
//
// Every fixture above runs unauthenticated, which `test_app()` resolves to
// the `system` principal — the only reachable path before Epic 12 shipped a
// real `Auth` extractor. Epic 12 is shipped now (`resolve_principal` maps
// any auto-provisioned, non-bot JWT subject to `PrincipalKind::User`), so a
// real person authoring a memory over HTTP is reachable; it had just never
// been exercised here. The human-confidence-default *rule* was already
// proven in `graph_owl_core::memory`'s unit tests
// (`a_human_memory_defaults_to_full_confidence`); what was missing is proof
// that a real request, carrying a real token, actually reaches that rule
// through `Auth` and `authorship_of` rather than only through a
// hand-constructed `Principal` in a unit test.

const MEMORY_TEST_SECRET: &str = "memory-demo-signing-secret-not-for-production";

fn person_token(subject: &str) -> String {
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
        &jsonwebtoken::EncodingKey::from_secret(MEMORY_TEST_SECRET.as_bytes()),
    )
    .expect("token should encode")
}

async fn send_as(
    app: &axum::Router,
    method: &str,
    uri: &str,
    subject: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {}", person_token(subject)));
    request = if body.is_some() {
        request.header("content-type", "application/json")
    } else {
        request
    };
    let request = match body {
        Some(body) => request.body(Body::from(body.to_string())),
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

/// **The property Epic 31 was missing proof of.** A real, JWT-authenticated
/// person — not `Principal::system()`, not a hand-built `Principal` in a
/// unit test — posts a memory with no stated confidence, and it must be
/// recorded as human-authored, defaulted to full confidence, and attributed
/// to that exact subject.
#[tokio::test]
async fn a_real_person_authors_a_memory_and_it_defaults_to_full_confidence() {
    let (app, _database, _) = test_app_with_secret(MEMORY_TEST_SECRET).await;

    let (status, asset) = send_as(
        &app,
        "POST",
        "/assets",
        "priya",
        Some(json!({ "kind": "service", "name": "orders" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{asset}");
    let table = asset["id"].as_str().expect("an id").to_string();

    let (created, body) = send_as(
        &app,
        "POST",
        "/memories",
        "priya",
        Some(json!({
            "kind": "decision",
            "content": "Refunds are excluded from revenue.",
            "links": [{ "relation": "about", "target": table }],
        })),
    )
    .await;

    assert_eq!(created, StatusCode::CREATED, "{body}");
    assert_eq!(body["authorship"]["kind"], "human", "{body}");
    assert_eq!(body["authorship"]["userId"], "priya", "{body}");
    assert_eq!(
        body["confidence"].as_f64().expect("a confidence"),
        1.0,
        "a person who writes something down means it: {body}"
    );
}

/// The other half of the same rule: a person's *stated* confidence still
/// wins over the default, exercised over HTTP rather than only in
/// `graph_owl_core::memory`'s `a_stated_confidence_overrides_the_human_default`.
#[tokio::test]
async fn a_real_persons_stated_confidence_overrides_the_human_default() {
    let (app, _database, _) = test_app_with_secret(MEMORY_TEST_SECRET).await;

    let (_, asset) = send_as(
        &app,
        "POST",
        "/assets",
        "priya",
        Some(json!({ "kind": "service", "name": "orders" })),
    )
    .await;
    let table = asset["id"].as_str().expect("an id").to_string();

    let (created, body) = send_as(
        &app,
        "POST",
        "/memories",
        "priya",
        Some(json!({
            "kind": "decision",
            "content": "Refunds are excluded from revenue, tentatively.",
            "confidence": 0.4,
            "links": [{ "relation": "about", "target": table }],
        })),
    )
    .await;

    assert_eq!(created, StatusCode::CREATED, "{body}");
    assert_eq!(body["authorship"]["kind"], "human", "{body}");
    assert_eq!(
        body["confidence"].as_f64().expect("a confidence"),
        0.4,
        "{body}"
    );
}
