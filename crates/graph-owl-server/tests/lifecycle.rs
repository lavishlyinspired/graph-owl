//! Epic 26 at the wire — lifecycle and certification.
//!
//! **The two tests that carry this epic** are the successor chain and the
//! clock advance. The first is what lets an agent redirect rather than merely
//! warn; the second is the property a stored status cannot have, and the reason
//! the whole thing is computed on read.

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

/// A root-kind asset, because a table needs a parent and building a hierarchy
/// in every test would be scaffolding that proves nothing about lifecycle.
async fn service(app: &axum::Router, name: &str) -> (String, String) {
    let (status, created) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (
        created["id"].as_str().expect("an id").to_string(),
        created["fullyQualifiedName"]
            .as_str()
            .expect("an fqn")
            .to_string(),
    )
}

async fn move_to(app: &axum::Router, id: &str, state: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/assets/{id}/lifecycle"),
        Some(json!({ "lifecycle": state })),
    )
    .await
}

async fn deprecate(
    app: &axum::Router,
    id: &str,
    reason: &str,
    successor: Option<&str>,
) -> (StatusCode, Value) {
    let mut deprecation = json!({ "reason": reason });
    if let Some(successor) = successor {
        deprecation["successorFqn"] = json!(successor);
    }
    send(
        app,
        "POST",
        &format!("/assets/{id}/lifecycle"),
        Some(json!({ "lifecycle": "deprecated", "deprecation": deprecation })),
    )
    .await
}

// ── Slice A: the lifecycle state machine ────────────────────────────────────

/// Everything already in a catalog got there from a connector or a deliberate
/// write, so `Active` is the honest default — marking a whole estate `draft`
/// would make the state meaningless on the day it shipped.
#[tokio::test]
async fn an_asset_starts_active() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;

    let (status, asset) = send(&app, "GET", &format!("/assets/{id}"), None).await;

    assert_eq!(status, StatusCode::OK, "{asset}");
    assert_eq!(asset["lifecycle"], "active", "{asset}");
}

#[tokio::test]
async fn the_legal_moves_are_permitted_and_bump_the_version() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;
    let (_, before) = send(&app, "GET", &format!("/assets/{id}"), None).await;

    let (status, deprecated) = deprecate(&app, &id, "superseded", None).await;
    assert_eq!(status, StatusCode::OK, "{deprecated}");
    assert_eq!(deprecated["lifecycle"], "deprecated");
    assert_ne!(deprecated["version"], before["version"]);

    let (status, retired) = move_to(&app, &id, "retired").await;
    assert_eq!(status, StatusCode::OK, "{retired}");
    assert_eq!(retired["lifecycle"], "retired");
}

/// **`Draft → Retired` is not a shortcut.** An asset that was never active has
/// nothing to retire from, and permitting it would make "retired" mean both "we
/// turned it off" and "we abandoned it before it started".
#[tokio::test]
async fn an_illegal_transition_is_refused_naming_both_ends() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;

    let (status, body) = move_to(&app, &id, "retired").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let reported = body.to_string();
    assert!(reported.contains("active"), "{reported}");
    assert!(reported.contains("retired"), "{reported}");
}

/// **Retired is terminal**, and this is the assertion an always-permit
/// transition fails.
#[tokio::test]
async fn a_retired_asset_cannot_come_back() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;
    deprecate(&app, &id, "superseded", None).await;
    move_to(&app, &id, "retired").await;

    for target in ["active", "draft", "deprecated"] {
        let (status, body) = move_to(&app, &id, target).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "retired must not move to {target}: {body}"
        );
    }
}

/// Un-deprecating is a real correction — forcing a new asset to undo a mistaken
/// deprecation would break every reference to the old one.
#[tokio::test]
async fn a_deprecated_asset_can_be_brought_back() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;
    deprecate(&app, &id, "mistake", None).await;

    let (status, revived) = move_to(&app, &id, "active").await;

    assert_eq!(status, StatusCode::OK, "{revived}");
    assert_eq!(revived["lifecycle"], "active");
}

#[tokio::test]
async fn an_unrecognised_state_is_refused_listing_the_real_ones() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;

    let (status, body) = move_to(&app, &id, "mothballed").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("deprecated"), "{body}");
}

// ── Slice B: deprecation with a successor ───────────────────────────────────

/// "Deprecated" with no reason is a state nobody can act on.
#[tokio::test]
async fn deprecating_without_a_reason_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;

    let (status, body) = move_to(&app, &id, "deprecated").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_deprecation_carries_a_machine_readable_successor() {
    let (app, _db, _url) = test_app().await;
    let (old, _) = service(&app, "orders-v1").await;
    let (_, new_fqn) = service(&app, "orders-v2").await;

    let (status, body) = deprecate(&app, &old, "superseded", Some(&new_fqn)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deprecation"]["successorFqn"], new_fqn.as_str());
}

/// **Pointing users at another dead asset is worse than pointing nowhere** — it
/// looks like an answer.
#[tokio::test]
async fn a_successor_that_is_itself_deprecated_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (dead, dead_fqn) = service(&app, "orders-v1").await;
    let (old, _) = service(&app, "orders-v0").await;
    deprecate(&app, &dead, "also gone", None).await;

    let (status, body) = deprecate(&app, &old, "superseded", Some(&dead_fqn)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("deprecated"),
        "the reason has to be the real one: {body}"
    );
}

#[tokio::test]
async fn a_successor_that_does_not_exist_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-v1").await;

    let (status, body) = deprecate(&app, &id, "superseded", Some("nothing-like-this")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn an_asset_cannot_succeed_itself() {
    let (app, _db, _url) = test_app().await;
    let (id, fqn) = service(&app, "orders-v1").await;

    let (status, body) = deprecate(&app, &id, "superseded", Some(&fqn)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **The chain, which a one-hop resolution fails.** A → B → C has to reach C:
/// an agent redirected to B, which is itself dead, has been given an answer
/// that is confidently wrong.
#[tokio::test]
async fn a_successor_chain_resolves_to_the_live_terminal_asset() {
    let (app, _db, _url) = test_app().await;
    let (a, a_fqn) = service(&app, "orders-v1").await;
    let (b, b_fqn) = service(&app, "orders-v2").await;
    let (_, c_fqn) = service(&app, "orders-v3").await;

    // **A chain can only be built forwards in time, and that is not an
    // accident of this test.** A successor must be live when it is named, so
    // `A → B` is set while B is still good; B is deprecated later, pointing at
    // C. That is exactly how a chain arises in a real estate — nobody ever
    // deliberately points at something already dead, which is why
    // `a_successor_that_is_itself_deprecated_is_refused` and this test are
    // consistent rather than contradictory.
    deprecate(&app, &a, "superseded", Some(&b_fqn)).await;
    deprecate(&app, &b, "superseded", Some(&c_fqn)).await;

    let (status, terminal) = send(&app, "GET", &format!("/assets/{a_fqn}/successor"), None).await;

    assert_eq!(status, StatusCode::OK, "{terminal}");
    assert_eq!(
        terminal["fullyQualifiedName"],
        c_fqn.as_str(),
        "the walk must reach the live end of the chain: {terminal}"
    );
}

/// A dead end reports `null`, not the dead asset. "Deprecated with no
/// replacement" is a real answer and the most useful one short of a successor.
#[tokio::test]
async fn a_deprecation_with_no_successor_resolves_to_nothing() {
    let (app, _db, _url) = test_app().await;
    let (id, fqn) = service(&app, "orders-v1").await;
    deprecate(&app, &id, "just gone", None).await;

    let (status, terminal) = send(&app, "GET", &format!("/assets/{fqn}/successor"), None).await;

    assert_eq!(status, StatusCode::OK, "{terminal}");
    assert!(terminal.is_null(), "{terminal}");
}

/// A live asset is its own terminal successor, or the endpoint would only work
/// on dead things and every caller would have to check first.
#[tokio::test]
async fn a_live_asset_is_its_own_successor() {
    let (app, _db, _url) = test_app().await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (_, terminal) = send(&app, "GET", &format!("/assets/{fqn}/successor"), None).await;

    assert_eq!(terminal["fullyQualifiedName"], fqn.as_str(), "{terminal}");
}

/// A sunset already past would mean the asset is retired, which is a different
/// state — and one this call is not making.
#[tokio::test]
async fn a_sunset_in_the_past_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/assets/{id}/lifecycle"),
        Some(json!({
            "lifecycle": "deprecated",
            "deprecation": { "reason": "gone", "sunsetAt": "2020-01-01T00:00:00Z" },
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ── Slice C: certification types and issuance ───────────────────────────────

async fn certification_type(
    app: &axum::Router,
    name: &str,
    required_evidence: &[&str],
    issuers: &[&str],
) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/certification-types",
        Some(json!({
            "name": name,
            "defaultValidityDays": 90,
            "requiredEvidence": required_evidence,
            "authorizedIssuers": issuers,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

#[tokio::test]
async fn a_certification_type_can_be_defined_and_listed() {
    let (app, _db, _url) = test_app().await;

    let id = certification_type(&app, "Gold", &["qualityTests"], &[]).await;

    let (status, listed) = send(&app, "GET", "/certification-types", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let types = listed.as_array().expect("an array");
    assert_eq!(types.len(), 1, "{listed}");
    assert_eq!(types[0]["id"], id.as_str());
    assert_eq!(types[0]["requiredEvidence"][0], "qualityTests");
}

/// **Decision 1: certification expires.** A type with no validity would be a
/// trust stamp that becomes a lie within a year.
#[tokio::test]
async fn a_certification_type_without_a_validity_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/certification-types",
        Some(json!({ "name": "Gold" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_non_positive_validity_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/certification-types",
        Some(json!({ "name": "Gold", "defaultValidityDays": 0 })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn issuing_a_certification_defaults_its_expiry_from_the_type() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, issued) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{issued}");
    assert!(issued["expiresAt"].is_string(), "{issued}");
    assert_eq!(issued["status"]["status"], "valid", "{issued}");
    assert!(issued["issuer"].is_string(), "and it names who: {issued}");
}

/// **The criterion that makes certification mean something.** Without evidence
/// enforcement it is decoration — a stamp anyone can apply for any reason.
#[tokio::test]
async fn issuing_without_required_evidence_is_refused_naming_what_is_missing() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &["qualityTests", "ownerConfirmed"], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({
            "typeId": type_id,
            "evidence": [{ "kind": "qualityTests", "reference": "suite-4" }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("ownerConfirmed"),
        "a count would tell an issuer nothing they can act on: {body}"
    );
}

/// And the negative: complete evidence goes through, or enforcement would be a
/// ban rather than a guard.
#[tokio::test]
async fn issuing_with_complete_evidence_succeeds() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &["qualityTests"], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, issued) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({
            "typeId": type_id,
            "evidence": [{ "kind": "qualityTests", "reference": "suite-4" }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{issued}");
    assert_eq!(issued["evidence"][0]["reference"], "suite-4");
}

/// **An allowlist naming somebody who does not exist is refused**, and as a
/// `400` rather than a foreign-key violation surfacing as a `500`. Decision 4
/// is that accountability requires a name; a name nothing resolves to is not
/// one, and a `500` would read as our bug rather than the caller's typo.
#[tokio::test]
async fn an_allowlist_naming_an_unknown_principal_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/certification-types",
        Some(json!({
            "name": "Gold",
            "defaultValidityDays": 90,
            "authorizedIssuers": ["nobody-at-all"],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("nobody-at-all"), "{body}");
}

/// **The issuer allowlist**, which an implementation that ignored it would pass
/// every other test in this file.
#[tokio::test]
async fn an_unauthorized_issuer_is_refused() {
    let (app, _db, _url) = test_app().await;
    // A real principal, because decision 4 is that accountability requires a
    // name — an allowlist naming somebody who does not exist is refused, which
    // `an_allowlist_naming_an_unknown_principal_is_refused` asserts below.
    send(
        &app,
        "PUT",
        "/users/somebody-else",
        Some(json!({ "displayName": "Somebody Else" })),
    )
    .await;
    let type_id = certification_type(&app, "Gold", &[], &["somebody-else"]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id })),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

/// An expiry already past vouches for nothing.
#[tokio::test]
async fn an_expiry_in_the_past_is_refused() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id, "expiresAt": "2020-01-01T00:00:00Z" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn certifying_something_that_does_not_exist_is_refused() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;

    let (status, body) = send(
        &app,
        "POST",
        "/certifications/nothing-like-this",
        Some(json!({ "typeId": type_id })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ── Slice D: status is computed ─────────────────────────────────────────────

/// **The clock-advance test, at the wire.** A certification issued to expire
/// inside the warning window reads as `expiringSoon` with **no write in
/// between** — a stored status could only produce this if something rewrote it,
/// and nothing did.
#[tokio::test]
async fn a_certification_inside_the_window_reads_as_expiring_soon_with_days_left() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;
    let soon = chrono::Utc::now() + chrono::Duration::days(5);

    send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id, "expiresAt": soon })),
    )
    .await;

    let (status, listed) = send(&app, "GET", &format!("/certifications/{fqn}"), None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let found = listed.as_array().expect("an array");
    assert_eq!(found[0]["status"]["status"], "expiringSoon", "{listed}");
    assert_eq!(
        found[0]["status"]["daysRemaining"], 5,
        "the number is the actionable part: {listed}"
    );
}

/// A far-off expiry is plainly valid — the negative that stops the window from
/// swallowing everything.
#[tokio::test]
async fn a_far_off_expiry_reads_as_valid() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;
    let distant = chrono::Utc::now() + chrono::Duration::days(200);

    send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id, "expiresAt": distant })),
    )
    .await;

    let (_, listed) = send(&app, "GET", &format!("/certifications/{fqn}"), None).await;
    assert_eq!(listed[0]["status"]["status"], "valid", "{listed}");
}

/// **Decision 5: expiry does not change lifecycle.** An expired certification
/// means "no longer vouched for", not "deprecated" — conflating them would
/// retire assets nobody re-certified.
#[tokio::test]
async fn an_expiring_certification_leaves_the_lifecycle_alone() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (id, fqn) = service(&app, "orders-svc").await;
    let soon = chrono::Utc::now() + chrono::Duration::days(1);

    send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id, "expiresAt": soon })),
    )
    .await;

    let (_, asset) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(asset["lifecycle"], "active", "{asset}");
}

/// An uncertified asset reports nothing rather than an empty certification —
/// "never vouched for" and "vouched for and lapsed" are different answers.
#[tokio::test]
async fn an_uncertified_asset_has_no_certifications() {
    let (app, _db, _url) = test_app().await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, listed) = send(&app, "GET", &format!("/certifications/{fqn}"), None).await;

    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(listed.as_array().expect("an array").is_empty(), "{listed}");
}

// ── Slice E: recertification ────────────────────────────────────────────────

/// A renewal supersedes rather than accumulating, so "when does my Gold expire"
/// has one answer.
#[tokio::test]
async fn renewing_supersedes_rather_than_accumulating() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    for days in [10, 200] {
        let expiry = chrono::Utc::now() + chrono::Duration::days(days);
        let (status, body) = send(
            &app,
            "POST",
            &format!("/certifications/{fqn}"),
            Some(json!({ "typeId": type_id, "expiresAt": expiry })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    let (_, listed) = send(&app, "GET", &format!("/certifications/{fqn}"), None).await;
    let found = listed.as_array().expect("an array");
    assert_eq!(found.len(), 1, "one live answer per type: {listed}");
    assert_eq!(found[0]["status"]["status"], "valid", "{listed}");
}

/// **The re-check, which is what stops certification decaying into theatre.**
/// A renewal is the same path as an issuance, so evidence that has since
/// disappeared fails it — renewing on stale grounds is exactly the failure this
/// slice exists to prevent.
#[tokio::test]
async fn a_renewal_whose_evidence_is_missing_is_refused() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &["qualityTests"], &[]).await;
    let (_, fqn) = service(&app, "orders-svc").await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({
            "typeId": type_id,
            "evidence": [{ "kind": "qualityTests", "reference": "suite-4" }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // The renewal arrives with the evidence gone.
    let (status, body) = send(
        &app,
        "POST",
        &format!("/certifications/{fqn}"),
        Some(json!({ "typeId": type_id })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("qualityTests"), "{body}");
}

/// The recertification queue lists what expires inside the window, and nothing
/// beyond it — a queue that listed everything would be a list, not a queue.
#[tokio::test]
async fn the_recertification_queue_holds_only_what_is_expiring() {
    let (app, _db, _url) = test_app().await;
    let type_id = certification_type(&app, "Gold", &[], &[]).await;
    let (_, soon_fqn) = service(&app, "expiring-svc").await;
    let (_, later_fqn) = service(&app, "healthy-svc").await;

    for (fqn, days) in [(&soon_fqn, 3), (&later_fqn, 200)] {
        let expiry = chrono::Utc::now() + chrono::Duration::days(days);
        send(
            &app,
            "POST",
            &format!("/certifications/{fqn}"),
            Some(json!({ "typeId": type_id, "expiresAt": expiry })),
        )
        .await;
    }

    let (status, queue) = send(&app, "GET", "/recertification-queue", None).await;

    assert_eq!(status, StatusCode::OK, "{queue}");
    let waiting = queue.as_array().expect("an array");
    assert_eq!(waiting.len(), 1, "{queue}");
    assert_eq!(waiting[0]["targetFqn"], soon_fqn.as_str());
}

// ── Slice F: discoverability ────────────────────────────────────────────────

/// **A deprecated asset is returned with its marker, not filtered out and not
/// unmarked.** Filtering hides reality; unmarking misleads. This is the one
/// assertion that fails under either mistake.
#[tokio::test]
async fn a_deprecated_asset_is_still_listed_and_visibly_marked() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;
    deprecate(&app, &id, "superseded", None).await;

    let (status, page) = send(&app, "GET", "/assets?kind=service", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    let listed = page["data"].as_array().expect("a page");
    let found = listed
        .iter()
        .find(|a| a["id"] == id.as_str())
        .unwrap_or_else(|| panic!("a deprecated asset must not vanish from lists: {page}"));
    assert_eq!(
        found["lifecycle"], "deprecated",
        "and it must say so: {found}"
    );
}

// ── `?lifecycle=` filter — Phase 2.2 of plans/EPIC-COMPLETION-PLAN.md ──────
//
// The column and its partial index shipped with Slice A; nothing wired a
// query parameter to it. An exact match against a stored column, not a walk
// like `owner`/`domain` — lifecycle does not inherit down containment.

fn names(page: &Value) -> Vec<String> {
    let mut found: Vec<String> = page["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["name"].as_str().expect("a name").to_string())
        .collect();
    found.sort();
    found
}

#[tokio::test]
async fn the_lifecycle_filter_returns_only_the_matching_state() {
    let (app, _db, _url) = test_app().await;
    service(&app, "orders-svc").await;
    let (going_away, _) = service(&app, "legacy-svc").await;
    deprecate(&app, &going_away, "superseded", None).await;

    let (status, page) = send(&app, "GET", "/assets?lifecycle=deprecated", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["legacy-svc"], "{page}");
}

/// And the negative half of the same claim: `active` must exclude what
/// `deprecated` included, or the filter would be indistinguishable from no
/// filter at all.
#[tokio::test]
async fn the_lifecycle_filter_excludes_other_states() {
    let (app, _db, _url) = test_app().await;
    service(&app, "orders-svc").await;
    let (going_away, _) = service(&app, "legacy-svc").await;
    deprecate(&app, &going_away, "superseded", None).await;

    let (status, page) = send(&app, "GET", "/assets?lifecycle=active", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["orders-svc"], "{page}");
}

#[tokio::test]
async fn the_lifecycle_filter_also_applies_to_search() {
    let (app, _db, _url) = test_app().await;
    let (id, _) = service(&app, "orders-svc").await;
    deprecate(&app, &id, "superseded", None).await;

    let (status, page) = send(
        &app,
        "GET",
        "/assets/search?q=orders&lifecycle=deprecated",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["orders-svc"], "{page}");

    let (status, empty) = send(
        &app,
        "GET",
        "/assets/search?q=orders&lifecycle=retired",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert!(names(&empty).is_empty(), "nothing is retired yet: {empty}");
}

#[tokio::test]
async fn an_unrecognised_lifecycle_filter_is_refused_naming_the_real_states() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(&app, "GET", "/assets?lifecycle=archived", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "lifecycle", "{body}");
}
