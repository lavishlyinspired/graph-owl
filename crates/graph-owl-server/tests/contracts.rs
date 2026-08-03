//! Epic 27 at the wire — data contracts.
//!
//! **The one assertion that carries this epic** is that a breach does not block
//! the change. graph-owl observes metadata and cannot stop a warehouse
//! `ALTER TABLE`, so a system that refused here would be making a promise it has
//! no way to keep — and the producer would route around it.
//!
//! The compatibility matrix itself is proved exhaustively in
//! `graph_owl_core::contract`, without a database. These tests prove the wiring:
//! that the right contracts are selected, that breaches are recorded and
//! accumulate, and that clearing is explicit.

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

/// `POST /teams` with the id in the body — teams are upserted as a whole
/// (Epic 11 Slice C), because a partial update cannot express "remove
/// everybody" and removal is the operation that has to work.
async fn team(app: &axum::Router, id: &str) {
    let (status, body) = send(
        app,
        "POST",
        "/teams",
        Some(json!({ "id": id, "displayName": id, "members": [] })),
    )
    .await;
    assert!(
        status.is_success(),
        "a team should be creatable: {status} {body}"
    );
}

async fn service(app: &axum::Router, name: &str) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string()
}

/// A contract guaranteeing `id` and `amount`, active, with the given mode.
async fn contract(
    app: &axum::Router,
    name: &str,
    asset_fqn: &str,
    mode: &str,
    allow_additional: bool,
) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/contracts",
        Some(json!({
            "name": name,
            "assetFqn": asset_fqn,
            "producer": "platform",
            "consumers": ["analytics"],
            "compatibility": mode,
            "status": "active",
            "schemaGuarantee": {
                "requiredColumns": [
                    { "name": "id", "dataType": "int", "nullable": false },
                    { "name": "amount", "dataType": "int", "nullable": true },
                ],
                "allowAdditional": allow_additional,
            },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

async fn change(app: &axum::Router, fqn: &str, body: Value) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/assets/{fqn}/schema-change"),
        Some(json!({ "change": body, "assetVersion": "0.2" })),
    )
    .await
}

async fn parties(app: &axum::Router) {
    team(app, "platform").await;
    team(app, "analytics").await;
}

// ── Slice A: contracts exist ────────────────────────────────────────────────

#[tokio::test]
async fn a_contract_is_created_with_its_parties_and_guarantee() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;

    let id = contract(&app, "orders-v1", &fqn, "backward", true).await;

    let (status, body) = send(&app, "GET", &format!("/contracts/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["contract"]["producer"], "platform");
    assert_eq!(body["contract"]["consumers"][0], "analytics");
    assert_eq!(body["contract"]["compatibility"], "backward");
    assert_eq!(
        body["contract"]["schemaGuarantee"]["requiredColumns"]
            .as_array()
            .expect("columns")
            .len(),
        2
    );
}

/// **Several contracts per asset is the realistic case**, not the exception —
/// different consumers agree to different things, and a single-contract model
/// breaks on the first organization that has two.
#[tokio::test]
async fn two_contracts_with_different_modes_coexist_on_one_asset() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;

    contract(&app, "strict", &fqn, "full", true).await;
    contract(&app, "lenient", &fqn, "none", true).await;

    let (status, listed) = send(&app, "GET", &format!("/contracts?assetFqn={fqn}"), None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed.as_array().expect("an array").len(), 2, "{listed}");
}

#[tokio::test]
async fn a_contract_on_an_asset_that_does_not_exist_is_refused() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;

    let (status, body) = send(
        &app,
        "POST",
        "/contracts",
        Some(json!({
            "name": "orphan",
            "assetFqn": "nothing-like-this",
            "producer": "platform",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// A contract whose producer does not exist is a promise nobody is accountable
/// for — which is exactly what decision 1 makes it an entity to avoid.
#[tokio::test]
async fn a_contract_with_an_unknown_producer_is_refused() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/contracts",
        Some(json!({ "name": "orphan", "assetFqn": fqn, "producer": "nobody" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

// ── Slice C: breach detection ───────────────────────────────────────────────

/// **Decision 3, and the assertion the whole epic rests on.** The change is
/// reported, not refused — graph-owl cannot stop a warehouse `ALTER TABLE`, and
/// a `409` here would be a promise it has no way to keep.
#[tokio::test]
async fn a_breaching_change_is_reported_and_not_blocked() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let id = contract(&app, "orders-v1", &fqn, "full", true).await;

    let (status, body) = change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "the change is observed, never refused: {body}"
    );
    let breaches = body["breaches"].as_array().expect("breaches");
    assert_eq!(breaches.len(), 1, "{body}");
    assert_eq!(breaches[0]["contractId"], id.as_str());
    assert_eq!(breaches[0]["column"], "amount");
    // The parties are named, so they hear about it from the catalog rather than
    // from a broken dashboard.
    assert_eq!(breaches[0]["producer"], "platform");
    assert_eq!(breaches[0]["consumers"][0], "analytics");
}

#[tokio::test]
async fn a_breach_marks_the_contract_violated_and_is_recorded() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let id = contract(&app, "orders-v1", &fqn, "full", true).await;

    change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;

    let (_, body) = send(&app, "GET", &format!("/contracts/{id}"), None).await;
    assert_eq!(body["contract"]["status"], "violated", "{body}");
    let recorded = body["breaches"].as_array().expect("breaches");
    assert_eq!(recorded.len(), 1, "{body}");
    assert_eq!(recorded[0]["assetVersion"], "0.2");
}

/// **A compatible change does not clear an earlier breach.** Silent clearing
/// would let a producer break something on Monday and look clean on Tuesday,
/// which is the incident this epic exists to surface.
#[tokio::test]
async fn a_later_compatible_change_leaves_the_violation_standing() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let id = contract(&app, "orders-v1", &fqn, "full", true).await;
    change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;

    let (_, clean) = change(
        &app,
        &fqn,
        json!({ "change": "addNullableColumn", "column": "note" }),
    )
    .await;
    assert!(
        clean["breaches"].as_array().expect("breaches").is_empty(),
        "a nullable addition is compatible under `full`: {clean}"
    );

    let (_, body) = send(&app, "GET", &format!("/contracts/{id}"), None).await;
    assert_eq!(
        body["contract"]["status"], "violated",
        "the incident happened and is not undone by a later good change: {body}"
    );
}

/// **Breaches accumulate.** A second incident on an already-broken contract is
/// still an incident, and stopping at the first would hide everything after it.
#[tokio::test]
async fn a_second_breach_accumulates_rather_than_overwriting() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let id = contract(&app, "orders-v1", &fqn, "full", true).await;

    change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;
    change(
        &app,
        &fqn,
        json!({ "change": "narrowType", "column": "id", "to": "smallint" }),
    )
    .await;

    let (_, body) = send(&app, "GET", &format!("/contracts/{id}"), None).await;
    assert_eq!(
        body["breaches"].as_array().expect("breaches").len(),
        2,
        "{body}"
    );
}

/// A `Draft` contract is a proposal nobody has agreed to, so breaching it is
/// not a fact about the world.
#[tokio::test]
async fn a_draft_contract_is_not_evaluated() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let (_, created) = send(
        &app,
        "POST",
        "/contracts",
        Some(json!({
            "name": "proposal",
            "assetFqn": fqn,
            "producer": "platform",
            "compatibility": "full",
            "schemaGuarantee": {
                "requiredColumns": [{ "name": "amount", "dataType": "int", "nullable": true }],
                "allowAdditional": true,
            },
        })),
    )
    .await;
    assert_eq!(created["status"], "draft", "{created}");

    let (_, body) = change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;

    assert!(
        body["breaches"].as_array().expect("breaches").is_empty(),
        "{body}"
    );
}

/// The right contract is selected: a lenient one beside a strict one is not
/// dragged into the breach.
#[tokio::test]
async fn only_the_contracts_that_forbid_the_change_are_breached() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let strict = contract(&app, "strict", &fqn, "full", true).await;
    contract(&app, "lenient", &fqn, "none", true).await;

    let (_, body) = change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;

    let breaches = body["breaches"].as_array().expect("breaches");
    assert_eq!(breaches.len(), 1, "{body}");
    assert_eq!(breaches[0]["contractId"], strict.as_str());
}

/// `allow_additional: false` overrides even the most lenient mode, at the wire
/// as well as in the checker.
#[tokio::test]
async fn forbidding_additions_breaches_under_the_none_mode() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    contract(&app, "no-additions", &fqn, "none", false).await;

    let (_, body) = change(
        &app,
        &fqn,
        json!({ "change": "addNullableColumn", "column": "note" }),
    )
    .await;

    assert_eq!(
        body["breaches"].as_array().expect("breaches").len(),
        1,
        "an explicit refusal beats a lenient mode: {body}"
    );
}

/// **Clearing is explicit**, which is the other half of the accumulation rule.
#[tokio::test]
async fn clearing_breaches_returns_the_contract_to_active() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let id = contract(&app, "orders-v1", &fqn, "full", true).await;
    change(
        &app,
        &fqn,
        json!({ "change": "removeColumn", "column": "amount" }),
    )
    .await;

    let (status, cleared) = send(&app, "DELETE", &format!("/contracts/{id}/breaches"), None).await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["cleared"], 1);

    let (_, body) = send(&app, "GET", &format!("/contracts/{id}"), None).await;
    assert_eq!(body["contract"]["status"], "active", "{body}");
    assert!(body["breaches"].as_array().expect("breaches").is_empty());
}

// ── Slice D: SLAs, and the answer that is honest today ──────────────────────

/// **`Unknown`, not `Met`.** SLAs are evaluated against Epic 30's signals
/// (decision 5) and Epic 30 is not built, so nothing has been measured —
/// reporting a satisfied SLA nobody measured manufactures confidence out of
/// missing data, which is the precise failure the three-valued result prevents.
#[tokio::test]
async fn an_sla_with_no_signal_reports_unknown_rather_than_met() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;
    let (status, created) = send(
        &app,
        "POST",
        "/contracts",
        Some(json!({
            "name": "orders-v1",
            "assetFqn": fqn,
            "producer": "platform",
            "status": "active",
            "slas": [{ "kind": "freshness", "maxAgeSeconds": 3600 }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("an id");

    let (status, evaluated) = send(&app, "GET", &format!("/contracts/{id}/slas"), None).await;

    assert_eq!(status, StatusCode::OK, "{evaluated}");
    let results = evaluated.as_array().expect("an array");
    assert_eq!(results.len(), 1, "{evaluated}");
    assert_eq!(
        results[0]["evaluation"]["state"], "unknown",
        "an unmeasured SLA is not a satisfied one: {evaluated}"
    );
}

#[tokio::test]
async fn a_contract_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = send(
        &app,
        "GET",
        &format!("/contracts/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// An unrecognised mode is refused listing the real ones, rather than silently
/// becoming the lenient default — which would make a typo'd contract guarantee
/// nothing.
#[tokio::test]
async fn an_unrecognised_compatibility_mode_is_refused() {
    let (app, _db, _url) = test_app().await;
    parties(&app).await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/contracts",
        Some(json!({
            "name": "typo",
            "assetFqn": fqn,
            "producer": "platform",
            "compatibility": "sideways",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("backward"), "{body}");
}
