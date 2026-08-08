//! Epic 30 at the wire — quality signals.
//!
//! **The two assertions that carry this epic** are both refusals. An asset with
//! no tests reports `Unknown` and never `Healthy`, because silence is not a
//! pass and reporting health for something nobody checked asserts trust nobody
//! earned. And an old pass is not a pass: a result older than its cadence is
//! `Stale`, or a pipeline that stopped running keeps looking green for months.
//!
//! The health arithmetic is proved exhaustively in `graph_owl_core::quality`,
//! without a database. These tests prove the wiring.

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

/// A `service` → `database` → `schema` → `table` chain, returning the table's id
/// and FQN. Lineage runs table-to-table (Epic 29 Slice A), so a lineage fixture
/// needs real depth rather than a root-kind asset.
async fn table(app: &axum::Router, service_name: &str) -> (String, String) {
    let mut parent: Option<String> = None;
    let mut created = Value::Null;
    for (kind, name) in [
        ("service", service_name),
        ("database", "sales"),
        ("schema", "public"),
        ("table", "orders"),
    ] {
        let mut body = json!({ "kind": kind, "name": name });
        if let Some(parent_id) = &parent {
            body["parentId"] = json!(parent_id);
        }
        let (status, made) = send(app, "POST", "/assets", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        parent = Some(made["id"].as_str().expect("an id").to_string());
        created = made;
    }
    (
        created["id"].as_str().expect("an id").to_string(),
        created["fullyQualifiedName"]
            .as_str()
            .expect("an fqn")
            .to_string(),
    )
}

async fn test_case(app: &axum::Router, target: &str, name: &str, cadence: Option<&str>) -> String {
    let mut body = json!({
        "name": name,
        "targetFqn": target,
        "testType": "not_null",
    });
    if let Some(cadence) = cadence {
        body["expectedCadence"] = json!(cadence);
    }
    let (status, created) = send(app, "POST", "/test-cases", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

async fn post_result(
    app: &axum::Router,
    case_id: &str,
    status_name: &str,
    hours_ago: i64,
) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/test-cases/{case_id}/results"),
        Some(json!({
            "results": [{
                "status": status_name,
                "observedAt": chrono::Utc::now() - chrono::Duration::hours(hours_ago),
            }],
        })),
    )
    .await
}

async fn health(app: &axum::Router, fqn: &str) -> Value {
    let (status, body) = send(app, "GET", &format!("/health/{fqn}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["health"].clone()
}

// ── Slice A: cases and definitions ──────────────────────────────────────────

#[tokio::test]
async fn a_test_case_is_registered_against_an_asset() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let id = test_case(&app, &fqn, "not_null", Some("P1D")).await;

    let (status, listed) = send(&app, "GET", &format!("/test-cases?targetFqn={fqn}"), None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let cases = listed.as_array().expect("an array");
    assert_eq!(cases.len(), 1, "{listed}");
    assert_eq!(cases[0]["id"], id.as_str());
    assert_eq!(cases[0]["expectedCadence"], "P1D");
}

#[tokio::test]
async fn a_duplicate_case_name_on_one_target_is_a_conflict() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    test_case(&app, &fqn, "not_null", None).await;

    let (status, body) = send(
        &app,
        "POST",
        "/test-cases",
        Some(json!({ "name": "not_null", "targetFqn": fqn, "testType": "not_null" })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// The same check name on two different targets is two cases — a globally
/// unique name would forbid the second, and `not_null` is the commonest check
/// there is.
#[tokio::test]
async fn the_same_case_name_on_two_targets_is_two_cases() {
    let (app, _db, _url) = test_app().await;
    let first = service(&app, "orders-svc").await;
    let second = service(&app, "payments-svc").await;

    test_case(&app, &first, "not_null", None).await;
    test_case(&app, &second, "not_null", None).await;
}

#[tokio::test]
async fn a_case_on_an_asset_that_does_not_exist_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/test-cases",
        Some(json!({
            "name": "not_null",
            "targetFqn": "nothing-like-this",
            "testType": "not_null",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// **A year is not a fixed length of time**, and "did this run within its
/// cadence" has to be answerable by subtracting two instants.
#[tokio::test]
async fn a_cadence_in_years_is_refused_with_a_usable_alternative() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        "/test-cases",
        Some(json!({
            "name": "annual",
            "targetFqn": fqn,
            "testType": "not_null",
            "expectedCadence": "P1Y",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("P30D"), "{body}");
}

/// **The whole point of the definition/case split** (decision 3a). One edit,
/// and every case that inherited the cadence follows — without it, changing a
/// threshold means editing a thousand rows.
#[tokio::test]
async fn editing_a_definitions_cadence_reaches_every_case_that_inherited_it() {
    let (app, _db, _url) = test_app().await;
    let first = service(&app, "orders-svc").await;
    let second = service(&app, "payments-svc").await;

    let (status, definition) = send(
        &app,
        "POST",
        "/test-definitions",
        Some(json!({
            "name": "freshness",
            "testType": "freshness",
            "expectedCadence": "P1D",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{definition}");
    let definition_id = definition["id"].as_str().expect("an id").to_string();

    for target in [&first, &second] {
        let (status, created) = send(
            &app,
            "POST",
            "/test-cases",
            Some(json!({
                "name": "freshness",
                "targetFqn": target,
                "testType": "freshness",
                "definitionId": definition_id,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(
            created["expectedCadence"], "P1D",
            "the case inherits the definition's cadence: {created}"
        );
    }

    let (status, changed) = send(
        &app,
        "POST",
        &format!("/test-definitions/{definition_id}/cadence"),
        Some(json!({ "expectedCadence": "PT12H" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        changed["affectedCases"], 2,
        "one edit, both cases: {changed}"
    );

    let (_, listed) = send(&app, "GET", &format!("/test-cases?targetFqn={first}"), None).await;
    assert_eq!(
        listed[0]["expectedCadence"], "PT12H",
        "the case now resolves to the new cadence: {listed}"
    );
}

/// **A case that overrode the cadence does not follow**, which is what makes
/// the override an override rather than a default nobody can escape.
#[tokio::test]
async fn a_case_with_its_own_cadence_is_not_moved_by_the_definition() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let (_, definition) = send(
        &app,
        "POST",
        "/test-definitions",
        Some(json!({ "name": "freshness", "testType": "freshness", "expectedCadence": "P1D" })),
    )
    .await;
    let definition_id = definition["id"].as_str().expect("an id").to_string();

    let (_, created) = send(
        &app,
        "POST",
        "/test-cases",
        Some(json!({
            "name": "freshness",
            "targetFqn": fqn,
            "testType": "freshness",
            "definitionId": definition_id,
            "expectedCadence": "PT1H",
        })),
    )
    .await;
    assert_eq!(created["expectedCadence"], "PT1H", "{created}");

    let (_, changed) = send(
        &app,
        "POST",
        &format!("/test-definitions/{definition_id}/cadence"),
        Some(json!({ "expectedCadence": "P7D" })),
    )
    .await;
    assert_eq!(
        changed["affectedCases"], 0,
        "it said something different on purpose: {changed}"
    );

    let (_, listed) = send(&app, "GET", &format!("/test-cases?targetFqn={fqn}"), None).await;
    assert_eq!(listed[0]["expectedCadence"], "PT1H", "{listed}");
}

// ── Slice B: results are history ────────────────────────────────────────────

#[tokio::test]
async fn results_are_retained_in_order() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;

    post_result(&app, &case, "success", 3).await;
    post_result(&app, &case, "failed", 1).await;

    let (status, results) = send(&app, "GET", &format!("/test-cases/{case}/results"), None).await;
    assert_eq!(status, StatusCode::OK, "{results}");
    let rows = results.as_array().expect("an array");
    assert_eq!(rows.len(), 2, "{results}");
    assert_eq!(rows[0]["status"], "failed", "newest first: {results}");
}

/// **Decision 2**: a nightly suite across ten thousand tables must not fill
/// every history with observations. The version tracks *descriptive* change.
#[tokio::test]
async fn ingesting_results_does_not_bump_the_assets_version() {
    let (app, _db, _url) = test_app().await;
    let (_, created) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "orders-svc" })),
    )
    .await;
    let id = created["id"].as_str().expect("an id").to_string();
    let fqn = created["fullyQualifiedName"].as_str().expect("an fqn");
    let before = created["version"].clone();
    let case = test_case(&app, fqn, "not_null", Some("P1D")).await;

    for hours in 1..10 {
        post_result(&app, &case, "success", hours).await;
    }

    let (_, after) = send(&app, "GET", &format!("/assets/{id}"), None).await;
    assert_eq!(
        after["version"], before,
        "a test running is not a metadata change: {after}"
    );
}

/// A retried push must not double-count.
#[tokio::test]
async fn a_duplicate_timestamp_is_ignored() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;
    let at = chrono::Utc::now() - chrono::Duration::hours(1);
    let body = json!({ "results": [{ "status": "success", "observedAt": at }] });

    send(
        &app,
        "POST",
        &format!("/test-cases/{case}/results"),
        Some(body.clone()),
    )
    .await;
    let (_, second) = send(
        &app,
        "POST",
        &format!("/test-cases/{case}/results"),
        Some(body),
    )
    .await;

    assert_eq!(second["accepted"], 0, "{second}");
    assert_eq!(second["duplicates"], 1, "{second}");
}

#[tokio::test]
async fn a_result_in_the_future_is_rejected() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;

    let (_, body) = post_result(&app, &case, "success", -5).await;

    assert_eq!(body["rejected"], 1, "{body}");
    assert_eq!(body["accepted"], 0, "{body}");
}

#[tokio::test]
async fn an_unrecognised_status_is_refused_listing_the_real_ones() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;

    let (status, body) = post_result(&app, &case, "flaky", 1).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("aborted"), "{body}");
}

/// Deleting a case takes its results — an observation about a check nobody
/// declared is unattributable.
#[tokio::test]
async fn deleting_a_case_removes_its_results() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;
    post_result(&app, &case, "success", 1).await;

    let (status, _) = send(&app, "DELETE", &format!("/test-cases/{case}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, results) = send(&app, "GET", &format!("/test-cases/{case}/results"), None).await;
    assert!(
        results.as_array().expect("an array").is_empty(),
        "{results}"
    );
}

// ── Slice C: health, and the two refusals ───────────────────────────────────

/// **The most dangerous possible bug in this epic.** An asset nobody tests
/// reported as healthy asserts trust nobody earned, and does it silently.
#[tokio::test]
async fn an_asset_with_no_tests_is_unknown_and_never_healthy() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let summary = health(&app, &fqn).await;

    assert_eq!(summary["state"], "unknown", "silence is not a pass");
    assert_ne!(summary["state"], "healthy");
}

#[tokio::test]
async fn every_case_fresh_and_passing_is_healthy() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;
    post_result(&app, &case, "success", 1).await;

    let summary = health(&app, &fqn).await;

    assert_eq!(summary["state"], "healthy", "{summary}");
    assert_eq!(summary["passing"], 1);
}

#[tokio::test]
async fn a_failing_case_makes_the_asset_unhealthy_and_names_it() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "row_count", Some("P1D")).await;
    post_result(&app, &case, "failed", 1).await;

    let summary = health(&app, &fqn).await;

    assert_eq!(summary["state"], "unhealthy", "{summary}");
    assert_eq!(summary["failingCases"][0], "row_count", "{summary}");
}

/// **An old pass is not a pass** (decision 4). Carrying the last known status
/// forward is how a pipeline that stopped running keeps looking green.
#[tokio::test]
async fn a_result_older_than_its_cadence_is_stale_not_passing() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "freshness", Some("P1D")).await;
    post_result(&app, &case, "success", 100).await;

    let summary = health(&app, &fqn).await;

    assert_eq!(summary["state"], "stale", "{summary}");
    assert_eq!(
        summary["passing"], 0,
        "a four-day-old success is not a current pass: {summary}"
    );
}

/// **The mixed case, which averaging would hide.** A fresh pass beside a check
/// that stopped running is not simply healthy.
#[tokio::test]
async fn a_fresh_pass_beside_a_stale_case_reports_stale() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let fresh = test_case(&app, &fqn, "not_null", Some("P1D")).await;
    let stopped = test_case(&app, &fqn, "freshness", Some("P1D")).await;
    post_result(&app, &fresh, "success", 1).await;
    post_result(&app, &stopped, "success", 100).await;

    let summary = health(&app, &fqn).await;

    assert_eq!(summary["state"], "stale", "{summary}");
    assert_eq!(summary["passing"], 1, "{summary}");
    assert_eq!(summary["stale"], 1, "reported distinctly: {summary}");
}

/// A registered case that has never run is stale, not absent — somebody
/// declared the check and it has produced nothing.
#[tokio::test]
async fn a_case_that_has_never_run_is_stale() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    test_case(&app, &fqn, "not_null", Some("P1D")).await;

    let summary = health(&app, &fqn).await;

    assert_eq!(summary["state"], "stale", "{summary}");
    assert_eq!(summary["stale"], 1, "{summary}");
}

// ── Slice E: retention ──────────────────────────────────────────────────────

/// **The latest result survives pruning**, whatever its age. Pruning it would
/// blank the health signal pruning exists to support, and would do it worst for
/// exactly the infrequently-tested assets whose signal is scarcest.
#[tokio::test]
async fn pruning_keeps_the_most_recent_result_per_case() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;

    post_result(&app, &case, "success", 24 * 200).await;
    post_result(&app, &case, "failed", 24 * 150).await;

    let (status, pruned) = send(&app, "POST", "/test-results/prune", None).await;
    assert_eq!(status, StatusCode::OK, "{pruned}");
    assert_eq!(pruned["pruned"], 1, "one of two, not both: {pruned}");

    let (_, results) = send(&app, "GET", &format!("/test-cases/{case}/results"), None).await;
    let rows = results.as_array().expect("an array");
    assert_eq!(rows.len(), 1, "{results}");
    assert_eq!(
        rows[0]["status"], "failed",
        "and it is the newest one that survived: {results}"
    );
}

#[tokio::test]
async fn pruning_leaves_recent_results_alone() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;
    post_result(&app, &case, "success", 1).await;
    post_result(&app, &case, "success", 2).await;

    let (_, pruned) = send(&app, "POST", "/test-results/prune", None).await;

    assert_eq!(pruned["pruned"], 0, "{pruned}");
}

// ── Slice F: trust propagates along lineage ─────────────────────────────────

/// **The asset's own health and its upstream's are reported separately.**
/// Conflating them makes the signal unactionable: a steward cannot tell whether
/// to fix this table or go upstream.
#[tokio::test]
async fn an_unhealthy_upstream_is_reported_beside_healthy_own_health() {
    let (app, _db, _url) = test_app().await;
    // **Tables, not services.** Epic 29 Slice A refuses lineage across levels —
    // a `service` cannot feed a `service`, because lineage runs table-to-table
    // or column-to-column. The rule is right and the fixture has to respect it.
    let (upstream_id, upstream_fqn) = table(&app, "raw-svc").await;
    let (downstream_id, downstream_fqn) = table(&app, "mart-svc").await;
    let (upstream_id, upstream_fqn) = (upstream_id.as_str(), upstream_fqn.as_str());
    let (downstream_id, downstream_fqn) = (downstream_id.as_str(), downstream_fqn.as_str());

    let (status, edge) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({
            "fromAssetId": upstream_id,
            "toAssetId": downstream_id,
            "relationship": "feeds",
            "source": "manual",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{edge}");

    // The downstream passes; the upstream fails.
    let own = test_case(&app, downstream_fqn, "not_null", Some("P1D")).await;
    post_result(&app, &own, "success", 1).await;
    let theirs = test_case(&app, upstream_fqn, "row_count", Some("P1D")).await;
    post_result(&app, &theirs, "failed", 1).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/health/{downstream_fqn}?includeUpstream=true"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["health"]["state"], "healthy",
        "its own tests pass, and that stays true: {body}"
    );
    assert_eq!(
        body["upstream"]["state"], "unhealthy",
        "reported separately, never merged: {body}"
    );
    assert_eq!(body["upstream"]["assetFqn"], upstream_fqn, "{body}");
    assert_eq!(body["upstream"]["hops"], 1, "{body}");
}

/// Not computed unless asked, given the traversal cost.
#[tokio::test]
async fn upstream_health_is_absent_unless_requested() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (_, body) = send(&app, "GET", &format!("/health/{fqn}"), None).await;

    assert!(body.get("upstream").is_none(), "{body}");
}

/// Absent lineage is not an error — an asset with no upstream reports its own
/// health and nothing else.
#[tokio::test]
async fn an_asset_with_no_lineage_reports_only_its_own_health() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/health/{fqn}?includeUpstream=true"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["upstream"].is_null(), "{body}");
}

// ── `?health=` filter — Phase 4 decision 4.5 of plans/EPIC-COMPLETION-PLAN.md ──
//
// `health_expr` (graph-owl-storage-postgres) is a second, SQL-side
// implementation of the exact precedence `graph_owl_core::quality::health_of`
// already runs in Rust — the same "computation pushed into SQL rather than a
// stored copy" shape `?certification=` already established. The tests below
// exist specifically to catch the two implementations disagreeing, not just
// to prove a route exists.

fn names(page: &Value) -> Vec<String> {
    page["data"]
        .as_array()
        .expect("a data array")
        .iter()
        .map(|a| a["name"].as_str().expect("a name").to_string())
        .collect()
}

#[tokio::test]
async fn an_unrecognised_health_filter_is_refused_naming_the_real_states() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(&app, "GET", "/assets?health=broken", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "health", "{body}");
}

#[tokio::test]
async fn the_health_filter_returns_only_assets_with_no_tests_for_unknown() {
    let (app, _db, _url) = test_app().await;
    service(&app, "untested-svc").await;
    let tested = service(&app, "tested-svc").await;
    let case = test_case(&app, &tested, "not_null", Some("P1D")).await;
    post_result(&app, &case, "success", 1).await;

    let (status, page) = send(&app, "GET", "/assets?health=unknown", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["untested-svc"], "{page}");
}

#[tokio::test]
async fn the_health_filter_returns_only_healthy_assets() {
    let (app, _db, _url) = test_app().await;
    let healthy = service(&app, "healthy-svc").await;
    let case = test_case(&app, &healthy, "not_null", Some("P1D")).await;
    post_result(&app, &case, "success", 1).await;
    service(&app, "untested-svc").await;

    let (status, page) = send(&app, "GET", "/assets?health=healthy", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["healthy-svc"], "{page}");
}

#[tokio::test]
async fn the_health_filter_returns_only_unhealthy_assets() {
    let (app, _db, _url) = test_app().await;
    let unhealthy = service(&app, "unhealthy-svc").await;
    let case = test_case(&app, &unhealthy, "row_count", Some("P1D")).await;
    post_result(&app, &case, "failed", 1).await;
    let healthy = service(&app, "healthy-svc").await;
    let ok_case = test_case(&app, &healthy, "not_null", Some("P1D")).await;
    post_result(&app, &ok_case, "success", 1).await;

    let (status, page) = send(&app, "GET", "/assets?health=unhealthy", None).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["unhealthy-svc"], "{page}");
}

/// **The precedence case this filter exists to get right.** A case that is
/// *both* stale and failed counts toward staleness, never toward failure —
/// `health_of`'s own per-case loop checks `is_stale` before it checks
/// `status == Failed`. A SQL translation that checked failure first would
/// put this asset under `?health=unhealthy` instead of `?health=stale`,
/// silently disagreeing with what `GET /health/{fqn}` reports for the exact
/// same asset.
#[tokio::test]
async fn a_stale_and_failed_case_files_under_stale_not_unhealthy() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "stale-failure-svc").await;
    let case = test_case(&app, &fqn, "freshness", Some("P1D")).await;
    post_result(&app, &case, "failed", 100).await;

    // Cross-checked against the single-asset read path, which this filter
    // must agree with.
    let summary = health(&app, &fqn).await;
    assert_eq!(summary["state"], "stale", "{summary}");

    let (_, unhealthy) = send(&app, "GET", "/assets?health=unhealthy", None).await;
    assert!(
        !names(&unhealthy).contains(&"stale-failure-svc".to_string()),
        "a stale failure must not count as unhealthy: {unhealthy}"
    );

    let (status, stale) = send(&app, "GET", "/assets?health=stale", None).await;
    assert_eq!(status, StatusCode::OK, "{stale}");
    assert_eq!(names(&stale), vec!["stale-failure-svc"], "{stale}");
}

/// A fresh failure beside an unrelated stale case on the *same* asset must
/// still report unhealthy overall — `health_of`'s own outer precedence
/// checks `any_failed` before `any_stale`.
#[tokio::test]
async fn a_fresh_failure_beside_a_stale_case_on_the_same_asset_is_unhealthy() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "mixed-svc").await;
    let failing = test_case(&app, &fqn, "row_count", Some("P1D")).await;
    post_result(&app, &failing, "failed", 1).await;
    let stopped = test_case(&app, &fqn, "freshness", Some("P1D")).await;
    post_result(&app, &stopped, "success", 100).await;

    let summary = health(&app, &fqn).await;
    assert_eq!(summary["state"], "unhealthy", "{summary}");

    let (status, unhealthy) = send(&app, "GET", "/assets?health=unhealthy", None).await;
    assert_eq!(status, StatusCode::OK, "{unhealthy}");
    assert_eq!(names(&unhealthy), vec!["mixed-svc"], "{unhealthy}");
}

/// `GET /assets/search` takes the same filter, matching every other
/// computed-status filter's own convention.
#[tokio::test]
async fn the_search_endpoint_also_respects_the_health_filter() {
    let (app, _db, _url) = test_app().await;
    let fqn = service(&app, "orders-search-svc").await;
    let case = test_case(&app, &fqn, "not_null", Some("P1D")).await;
    post_result(&app, &case, "failed", 1).await;
    service(&app, "orders-search-other").await;

    let (status, body) = send(
        &app,
        "GET",
        "/assets/search?q=orders-search&health=unhealthy",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let hits: Vec<&str> = body["data"]
        .as_array()
        .expect("a data array")
        .iter()
        .map(|h| h["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(hits, vec!["orders-search-svc"], "{body}");
}

/// The facet half of decision 4.5, over the *unfiltered* result set — an
/// unhealthy asset among a healthy one reports both buckets, not just the
/// one a caller happened to filter for.
#[tokio::test]
async fn search_reports_a_health_facet_over_the_visible_set() {
    let (app, _db, _url) = test_app().await;
    let unhealthy = service(&app, "facet-search-unhealthy").await;
    let case = test_case(&app, &unhealthy, "row_count", Some("P1D")).await;
    post_result(&app, &case, "failed", 1).await;
    service(&app, "facet-search-untested").await;

    let (status, body) = send(&app, "GET", "/assets/search?q=facet-search", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let buckets: std::collections::BTreeMap<&str, i64> = body["facets"]["health"]
        .as_array()
        .expect("a health facet array")
        .iter()
        .map(|b| {
            (
                b["value"].as_str().expect("value"),
                b["count"].as_i64().expect("count"),
            )
        })
        .collect();
    assert_eq!(buckets.get("unhealthy"), Some(&1), "{body}");
    assert_eq!(buckets.get("unknown"), Some(&1), "{body}");
}
