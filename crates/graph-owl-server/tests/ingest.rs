//! Epic 16 Slice A at the wire: synchronous push with partial success.
//!
//! Two criteria carry this slice, and both are about *not* making the pusher do
//! the catalog's work: one bad item must not cost the other 999, and a batch must
//! not have to be submitted in dependency order.

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
    let response = app.clone().oneshot(request).await.expect("handled");
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

fn item(kind: &str, name: &str, parent: Option<&str>) -> Value {
    let mut v = json!({ "kind": kind, "name": name });
    if let Some(p) = parent {
        v["parentFqn"] = json!(p);
    }
    v
}

// **The criterion that makes a push usable.** A pusher walking a source emits what
// it finds when it finds it, so a child arriving before its parent is the normal
// case — requiring dependency order would push the catalog's model onto every
// adapter author.
#[tokio::test]
async fn a_batch_submitted_child_first_still_lands() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("table", "orders", Some("svc.db.public")),
            item("schema", "public", Some("svc.db")),
            item("database", "db", Some("svc")),
            item("service", "svc", None),
        ]})),
    )
    .await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    assert_eq!(body["accepted"], json!(4), "{body}");
    assert_eq!(body["rejected"], json!(0));
    // The table exists with its full path, which is only possible if the parents
    // were applied first.
    let (found, asset) = send(&app, "GET", "/assets?limit=100", None).await;
    assert_eq!(found, StatusCode::OK);
    let fqns: Vec<&str> = asset["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["fullyQualifiedName"].as_str().expect("fqn"))
        .collect();
    assert!(fqns.contains(&"svc.db.public.orders"), "{fqns:?}");
}

// **Partial success.** An all-or-nothing batch makes a pusher re-send everything
// to fix one typo, and at scale somebody stops retrying.
#[tokio::test]
async fn one_bad_item_does_not_cost_the_others() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("service", "good-one", None),
            // A table with no parent: `02-entity-hierarchy` requires one, so this
            // is a per-item failure rather than a malformed request.
            item("table", "orphan", None),
            item("service", "good-two", None),
        ]})),
    )
    .await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    assert_eq!(body["accepted"], json!(2), "{body}");
    assert_eq!(body["rejected"], json!(1), "{body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(results[1]["index"], json!(1), "reported by submitted index");
    assert_eq!(results[1]["status"], json!(400));
    assert!(results[1]["problem"].is_string());
}

// Results come back in **submitted** order, whatever order they were applied in —
// a client matches them against the batch it sent.
#[tokio::test]
async fn results_are_returned_in_submitted_order() {
    let (app, _db, _) = test_app().await;

    let (_, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("schema", "public", Some("svc.db")),
            item("database", "db", Some("svc")),
            item("service", "svc", None),
        ]})),
    )
    .await;

    let indexes: Vec<u64> = body["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|r| r["index"].as_u64().expect("index"))
        .collect();
    assert_eq!(indexes, vec![0, 1, 2]);
}

// A parent neither in the batch nor in the catalog is that item's problem, not the
// batch's — the rest still lands.
#[tokio::test]
async fn an_unresolvable_parent_fails_only_its_own_item() {
    let (app, _db, _) = test_app().await;

    let (_, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("service", "svc", None),
            item("table", "stray", Some("nowhere.at.all")),
        ]})),
    )
    .await;

    assert_eq!(body["accepted"], json!(1), "{body}");
    let results = body["results"].as_array().expect("results");
    assert!(
        results[1]["problem"]
            .as_str()
            .expect("problem")
            .contains("nowhere.at.all"),
        "the problem should name the parent: {body}"
    );
}

// **Phase 2.7 of plans/EPIC-COMPLETION-PLAN.md.** An unrecognised kind is
// data the batch supplied, the same as an unresolvable parent — not a
// malformed request. The handler's own contract says so: "207, always,
// once anything was attempted... a 400 would say it failed when 999 items
// landed. Neither is true." A bad kind used to break that promise by
// aborting the whole batch before any item was attempted.
#[tokio::test]
async fn an_invalid_kind_string_fails_only_its_own_item() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("service", "svc", None),
            item("not-a-real-kind", "bogus", None),
        ]})),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::MULTI_STATUS,
        "one bad kind must not cost the whole batch: {body}"
    );
    assert_eq!(body["accepted"], json!(1), "{body}");
    assert_eq!(body["rejected"], json!(1), "{body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(results[0]["status"], json!(200), "{body}");
    assert_eq!(results[1]["status"], json!(400), "{body}");
    assert!(
        results[1]["problem"]
            .as_str()
            .expect("problem")
            .contains("not-a-real-kind"),
        "the problem should name the offending kind: {body}"
    );
}

/// A child under a parent whose *own* kind was invalid cannot resolve that
/// parent either — a genuine cascade, not a second bug. Proves the rejected
/// item does not silently vanish from dependency resolution.
#[tokio::test]
async fn a_child_of_an_invalid_kind_item_fails_as_an_unresolved_parent() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("not-a-real-kind", "bogus", None),
            item("database", "child-db", Some("bogus")),
        ]})),
    )
    .await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    let results = body["results"].as_array().expect("results");
    assert_eq!(
        results[0]["status"],
        json!(400),
        "the bad kind itself: {body}"
    );
    assert_eq!(
        results[1]["status"],
        json!(400),
        "its child cannot resolve a parent that was never created: {body}"
    );
}

// "≤1000 items, larger → `400`." A request is not a job.
#[tokio::test]
async fn a_batch_over_the_ceiling_is_refused_whole() {
    let (app, _db, _) = test_app().await;
    let items: Vec<Value> = (0..1001)
        .map(|i| item("service", &format!("svc-{i}"), None))
        .collect();

    let (status, body) = send(&app, "POST", "/ingest", Some(json!({ "items": items }))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("1000"), "{body}");
}

// A duplicate FQN is a property of the *batch*: it states two intents for one
// entity, and applying both would make the result depend on submission order.
#[tokio::test]
async fn a_batch_naming_one_entity_twice_is_refused_whole() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [
            item("service", "svc", None),
            item("service", "svc", None),
        ]})),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("twice"), "{body}");
}

#[tokio::test]
async fn an_empty_batch_is_a_207_that_did_nothing() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(&app, "POST", "/ingest", Some(json!({ "items": [] }))).await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    assert_eq!(body["accepted"], json!(0));
    assert_eq!(body["rejected"], json!(0));
}

// ---- Slice B: idempotency ----

async fn push_with_key(app: &axum::Router, key: &str, items: Vec<Value>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .header("idempotency-key", key)
        .body(Body::from(json!({ "items": items }).to_string()))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

// "A replay within 24h returns the original response body and status, creating
// nothing." Without this, at-least-once transport duplicates every push it retries.
#[tokio::test]
async fn a_replayed_push_creates_nothing_and_returns_the_original_answer() {
    let (app, _db, _) = test_app().await;
    let items = vec![item("service", "svc", None)];

    let (first_status, first) = push_with_key(&app, "key-1", items.clone()).await;
    let (second_status, second) = push_with_key(&app, "key-1", items).await;

    assert_eq!(first_status, StatusCode::MULTI_STATUS, "{first}");
    assert_eq!(second_status, first_status);
    assert_eq!(
        second, first,
        "a replay returns the original answer verbatim"
    );

    // And nothing was created twice.
    let (_, listed) = send(&app, "GET", "/assets?limit=100", None).await;
    let count = listed["data"]
        .as_array()
        .expect("a page")
        .iter()
        .filter(|a| a["name"] == "svc")
        .count();
    assert_eq!(count, 1, "the replay must not have created a second asset");
}

// **"A key identifies a request, not a slot."** Reusing one for different content
// is a client bug — usually a key generated once and reused across a loop — and
// serving the first response would silently drop a push the client believes landed.
#[tokio::test]
async fn the_same_key_with_a_different_body_is_refused() {
    let (app, _db, _) = test_app().await;
    push_with_key(&app, "key-2", vec![item("service", "one", None)]).await;

    let (status, body) =
        push_with_key(&app, "key-2", vec![item("service", "different", None)]).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("different request"), "{body}");
}

// Different keys are different requests, so both land — otherwise the guard would
// be refusing legitimate work.
#[tokio::test]
async fn different_keys_are_independent() {
    let (app, _db, _) = test_app().await;

    let (first, _) = push_with_key(&app, "key-a", vec![item("service", "a", None)]).await;
    let (second, _) = push_with_key(&app, "key-b", vec![item("service", "b", None)]).await;

    assert_eq!(first, StatusCode::MULTI_STATUS);
    assert_eq!(second, StatusCode::MULTI_STATUS);
    let (_, listed) = send(&app, "GET", "/assets?limit=100", None).await;
    assert_eq!(listed["data"].as_array().expect("page").len(), 2);
}

// A push without a key still works: the header is how a client opts into replay
// protection, not a requirement that would break every existing caller.
#[tokio::test]
async fn a_push_without_a_key_is_unaffected() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": [item("service", "svc", None)] })),
    )
    .await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
}

// Formatting is not content. Two requests differing only in key order are the same
// request, and reporting them as a mismatch would make a client's serializer a 409.
#[tokio::test]
async fn a_replay_is_matched_on_content_not_byte_order() {
    let (app, _db, _) = test_app().await;
    let a = json!({ "kind": "service", "name": "svc" });
    let b = json!({ "name": "svc", "kind": "service" });

    let (_, first) = push_with_key(&app, "key-3", vec![a]).await;
    let (status, second) = push_with_key(&app, "key-3", vec![b]).await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{second}");
    assert_eq!(second, first);
}

// ---- Slice A: relationships and lineage in the same call ----

fn edge(from: &str, to: &str, relationship: &str) -> Value {
    json!({ "fromFqn": from, "toFqn": to, "relationship": relationship })
}

// **The stated criterion**: "a relationship whose endpoints are in the same batch
// resolves". A pusher cannot pre-create in dependency order, so an edge naming two
// entities from its own batch has to work.
#[tokio::test]
async fn an_edge_between_two_entities_in_the_same_batch_resolves() {
    let (app, _db, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({
            "items": [
                item("service", "svc", None),
                item("database", "db", Some("svc")),
                item("schema", "public", Some("svc.db")),
                item("table", "orders", Some("svc.db.public")),
                item("table", "mart", Some("svc.db.public")),
            ],
            "edges": [edge("svc.db.public.orders", "svc.db.public.mart", "feeds")],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    assert_eq!(body["accepted"], json!(6), "5 entities and 1 edge: {body}");
}

// An edge submitted **before** the entities it names must still resolve — edges are
// applied after every entity precisely so submission order cannot matter.
#[tokio::test]
async fn an_edge_naming_entities_submitted_after_it_still_resolves() {
    let (app, _db, _) = test_app().await;

    let (_, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({
            "items": [
                item("service", "svc", None),
                item("database", "db", Some("svc")),
                item("schema", "public", Some("svc.db")),
                item("table", "a", Some("svc.db.public")),
                item("table", "b", Some("svc.db.public")),
            ],
            "edges": [edge("svc.db.public.a", "svc.db.public.b", "feeds")],
        })),
    )
    .await;

    assert_eq!(body["rejected"], json!(0), "{body}");
}

// Lineage goes to the lineage graph, and the flag is explicit: the two models have
// overlapping vocabularies, and guessing would file an edge where nothing looks.
#[tokio::test]
async fn a_lineage_edge_is_recorded_as_lineage() {
    let (app, _db, _) = test_app().await;
    let mut lineage = edge("svc.db.public.a", "svc.db.public.b", "feeds");
    lineage["description"] = json!("nightly load");

    let (_, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({
            "items": [
                item("service", "svc", None),
                item("database", "db", Some("svc")),
                item("schema", "public", Some("svc.db")),
                item("table", "a", Some("svc.db.public")),
                item("table", "b", Some("svc.db.public")),
            ],
            "edges": [lineage],
        })),
    )
    .await;
    assert_eq!(body["rejected"], json!(0), "{body}");

    // It is visible through the lineage API, not merely accepted.
    let ids: Vec<&str> = body["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    let table_a = ids.first().expect("an id");
    let (status, graph) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?upstream=2&downstream=2", ids[3]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{graph}");
    assert!(
        !graph["edges"].as_array().expect("edges").is_empty(),
        "the lineage edge should be in the graph: {graph} (a={table_a})"
    );
}

// An endpoint that exists nowhere fails only its own edge — the entities still land.
#[tokio::test]
async fn an_edge_with_an_unresolvable_endpoint_fails_only_itself() {
    let (app, _db, _) = test_app().await;

    let (_, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({
            "items": [item("service", "svc", None)],
            "edges": [edge("svc", "nowhere.at.all", "feeds")],
        })),
    )
    .await;

    assert_eq!(body["accepted"], json!(1), "{body}");
    assert_eq!(body["rejected"], json!(1), "{body}");
    let results = body["results"].as_array().expect("results");
    assert!(
        results[1]["problem"]
            .as_str()
            .expect("problem")
            .contains("nowhere.at.all"),
        "{body}"
    );
}

// Indexes continue past the entity range, so one numbering addresses the whole
// request — a client should not have to know which list an index refers to.
#[tokio::test]
async fn edge_results_are_indexed_after_the_entities() {
    let (app, _db, _) = test_app().await;

    let (_, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({
            "items": [item("service", "a", None), item("service", "b", None)],
            "edges": [edge("a", "b", "feeds")],
        })),
    )
    .await;

    let indexes: Vec<u64> = body["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|r| r["index"].as_u64().expect("index"))
        .collect();
    assert_eq!(indexes, vec![0, 1, 2], "{body}");
}

// The ceiling counts both lists: splitting work across fields must not double the
// cost a request can impose.
#[tokio::test]
async fn the_ceiling_counts_entities_and_edges_together() {
    let (app, _db, _) = test_app().await;
    let items: Vec<Value> = (0..600)
        .map(|i| item("service", &format!("s{i}"), None))
        .collect();
    let edges: Vec<Value> = (0..600)
        .map(|i| edge("s0", &format!("s{i}"), "feeds"))
        .collect();

    let (status, body) = send(
        &app,
        "POST",
        "/ingest",
        Some(json!({ "items": items, "edges": edges })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// Two pushes with the same entities but different edges are different requests —
// hashing only the entities would replay the first answer for the second.
#[tokio::test]
async fn edges_are_part_of_the_idempotency_fingerprint() {
    let (app, _db, _) = test_app().await;
    let base = vec![item("service", "a", None), item("service", "b", None)];

    let first = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .header("idempotency-key", "edge-key")
        .body(Body::from(
            json!({ "items": base.clone(), "edges": [] }).to_string(),
        ))
        .expect("request");
    app.clone().oneshot(first).await.expect("handled");

    let second = Request::builder()
        .method("POST")
        .uri("/ingest")
        .header("content-type", "application/json")
        .header("idempotency-key", "edge-key")
        .body(Body::from(
            json!({ "items": base, "edges": [edge("a", "b", "feeds")] }).to_string(),
        ))
        .expect("request");
    let response = app.clone().oneshot(second).await.expect("handled");

    assert_eq!(response.status(), StatusCode::CONFLICT);
}
