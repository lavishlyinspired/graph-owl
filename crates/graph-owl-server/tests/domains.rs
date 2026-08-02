//! Epic 23 at the wire — domains and data products.
//!
//! **The two axes this epic exists to separate.** A domain says who is
//! accountable; a data product says what is consumable; the containment
//! hierarchy says where data lives. The tests that matter most here are the
//! ones that would pass if any two of the three were conflated — inheritance
//! that stops at the nearest assigned ancestor rather than accumulating, and
//! membership that is many-to-many where assignment is exclusive.

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

async fn domain(app: &axum::Router, name: &str, parent: Option<&str>) -> String {
    let mut body = json!({ "name": name });
    if let Some(parent) = parent {
        body["parentId"] = json!(parent);
    }
    let (status, created) = send(app, "POST", "/domains", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

/// A `service` → `database` → `schema` → `table` chain, returning every id from
/// the root down. Inheritance is only testable with real depth.
async fn hierarchy(app: &axum::Router, service: &str) -> Vec<String> {
    let mut parent: Option<String> = None;
    let mut ids = Vec::new();
    for (kind, name) in [
        ("service", service),
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
        let id = created["id"].as_str().expect("an id").to_string();
        parent = Some(id.clone());
        ids.push(id);
    }
    ids
}

async fn assign(app: &axum::Router, asset: &str, domain_id: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/assets/{asset}/domain"),
        Some(json!({ "domainId": domain_id })),
    )
    .await
}

async fn resolved(app: &axum::Router, asset: &str) -> Value {
    let (status, body) = send(app, "GET", &format!("/assets/{asset}/domain"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

// ── Slice A: domains exist and nest ─────────────────────────────────────────

#[tokio::test]
async fn a_domain_can_be_created_and_read_back() {
    let (app, _db, _url) = test_app().await;

    let (status, created) = send(
        &app,
        "POST",
        "/domains",
        Some(json!({
            "name": "payments",
            "description": "money movement",
            "domainType": "source-aligned",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["name"], "payments");
    assert_eq!(created["fullyQualifiedName"], "payments");
    assert_eq!(created["domainType"], "source-aligned");
    assert_eq!(
        created["version"],
        json!({ "major": 0, "minor": 1 }),
        "{created}"
    );
}

/// The FQN is a **path**, derived from the parent chain. A client-supplied one
/// could disagree with the parent, which is why there is no field for it.
#[tokio::test]
async fn a_nested_domain_derives_its_path_from_its_parent() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;

    let (status, created) = send(
        &app,
        "POST",
        "/domains",
        Some(json!({ "name": "payments", "parentId": retail })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["fullyQualifiedName"], "retail.payments");
}

#[tokio::test]
async fn a_duplicate_path_is_a_conflict() {
    let (app, _db, _url) = test_app().await;
    domain(&app, "payments", None).await;

    let (status, body) = send(
        &app,
        "POST",
        "/domains",
        Some(json!({ "name": "payments" })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// The same name under a *different* parent is a different domain, and must be
/// allowed — otherwise every organization gets one `billing` in total.
#[tokio::test]
async fn the_same_name_under_a_different_parent_is_allowed() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let wholesale = domain(&app, "wholesale", None).await;

    domain(&app, "billing", Some(&retail)).await;
    let (status, created) = send(
        &app,
        "POST",
        "/domains",
        Some(json!({ "name": "billing", "parentId": wholesale })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["fullyQualifiedName"], "wholesale.billing");
}

/// **The depth-1 cycle**, which is the one a careless edit creates.
#[tokio::test]
async fn a_domain_cannot_be_its_own_parent() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;

    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/domains/{payments}"),
        Some(json!({ "parentId": payments })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **The depth-3 cycle, which is the mutator watch the plan names.** A check
/// that looked only at the immediate parent passes this and leaves an ancestor
/// walk that never terminates — a hung request, not an error.
#[tokio::test]
async fn a_cycle_three_levels_deep_is_refused() {
    let (app, _db, _url) = test_app().await;
    let a = domain(&app, "a", None).await;
    let b = domain(&app, "b", Some(&a)).await;
    let c = domain(&app, "c", Some(&b)).await;

    // Making `c` the parent of `a` closes a → b → c → a.
    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/domains/{a}"),
        Some(json!({ "parentId": c })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// And the negative: an ordinary reparent must still work, or the cycle check
/// would be indistinguishable from reparenting being forbidden.
#[tokio::test]
async fn an_ordinary_reparent_moves_the_domain_and_its_subtree() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let wholesale = domain(&app, "wholesale", None).await;
    let payments = domain(&app, "payments", Some(&retail)).await;
    let billing = domain(&app, "billing", Some(&payments)).await;

    let (status, moved) = send(
        &app,
        "PATCH",
        &format!("/domains/{payments}"),
        Some(json!({ "parentId": wholesale })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["fullyQualifiedName"], "wholesale.payments");

    // **The subtree moves with it.** A rename that moved only its own path
    // would leave every descendant claiming to sit under a domain that no
    // longer has that name, and every path lookup below it would miss.
    let (_, child) = send(&app, "GET", &format!("/domains/{billing}"), None).await;
    assert_eq!(
        child["fullyQualifiedName"], "wholesale.payments.billing",
        "{child}"
    );
}

#[tokio::test]
async fn renaming_a_domain_moves_its_descendants_paths_too() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", Some(&retail)).await;

    let (status, renamed) = send(
        &app,
        "PATCH",
        &format!("/domains/{retail}"),
        Some(json!({ "name": "consumer" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");

    let (_, child) = send(&app, "GET", &format!("/domains/{payments}"), None).await;
    assert_eq!(child["fullyQualifiedName"], "consumer.payments", "{child}");
}

/// A dotted name would make the derived path ambiguous — `retail.payments` as a
/// *name* is indistinguishable from a two-level path.
#[tokio::test]
async fn a_dotted_name_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/domains",
        Some(json!({ "name": "retail.payments" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn children_are_listed_and_an_absent_parent_is_a_404() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    domain(&app, "payments", Some(&retail)).await;
    domain(&app, "returns", Some(&retail)).await;

    let (status, children) = send(&app, "GET", &format!("/domains/{retail}/children"), None).await;
    assert_eq!(status, StatusCode::OK, "{children}");
    assert_eq!(
        children.as_array().expect("an array").len(),
        2,
        "{children}"
    );

    // A 404 rather than an empty list: "no children" and "no such domain" are
    // different answers, and a client that cannot tell them apart renders an
    // empty tree for a typo.
    let (status, _) = send(
        &app,
        "GET",
        &format!("/domains/{}/children", uuid::Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Slice B: assets belong to a domain, with inheritance ────────────────────

#[tokio::test]
async fn a_directly_assigned_asset_reports_that_it_was_not_inherited() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    let (status, body) = assign(&app, table, &payments).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let assignment = resolved(&app, table).await;
    assert_eq!(assignment["id"], payments.as_str());
    assert_eq!(
        assignment["inherited"], false,
        "a direct assignment must say so: {assignment}"
    );
}

/// **The criterion adoption depends on.** Assigning a database has to reach its
/// tables, or governing a five-thousand-table estate means five thousand
/// assignments — which is the reason nobody does it.
#[tokio::test]
async fn an_unassigned_asset_inherits_from_its_nearest_assigned_ancestor() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (schema, table) = (&ids[2], &ids[3]);

    assign(&app, schema, &payments).await;

    let assignment = resolved(&app, table).await;
    assert_eq!(assignment["id"], payments.as_str(), "{assignment}");
    assert_eq!(
        assignment["inherited"], true,
        "it was found by walking up, and saying so is the whole point: {assignment}"
    );
}

/// **Multi-hop, which a single-hop resolver would fail.** The plan names this
/// as the mutator watch: assigning the *database* has to reach the table two
/// levels below it.
#[tokio::test]
async fn inheritance_reaches_across_several_hops() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (database, table) = (&ids[1], &ids[3]);

    assign(&app, database, &payments).await;

    let assignment = resolved(&app, table).await;
    assert_eq!(assignment["id"], payments.as_str(), "{assignment}");
    assert_eq!(assignment["inherited"], true, "{assignment}");
}

/// **Stops at the nearest, and this is the test an accumulating resolver
/// fails.** Two assigned ancestors is not two answers — the closer one wins,
/// because "who is accountable" has exactly one answer.
#[tokio::test]
async fn resolution_stops_at_the_nearest_assigned_ancestor() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (service, schema, table) = (&ids[0], &ids[2], &ids[3]);

    assign(&app, service, &retail).await;
    assign(&app, schema, &payments).await;

    let assignment = resolved(&app, table).await;
    assert_eq!(
        assignment["id"],
        payments.as_str(),
        "the schema is nearer than the service: {assignment}"
    );
}

/// A direct assignment is **not supplemented** by an ancestor's — the plan's
/// own sharp test. An accumulating implementation returns both and reads as
/// though the asset is in two domains.
#[tokio::test]
async fn a_direct_assignment_overrides_an_inherited_one() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (database, table) = (&ids[1], &ids[3]);

    assign(&app, database, &retail).await;
    assign(&app, table, &payments).await;

    let assignment = resolved(&app, table).await;
    assert_eq!(assignment["id"], payments.as_str(), "{assignment}");
    assert_eq!(assignment["inherited"], false, "{assignment}");
}

/// **Decision 1 at the wire.** Quietly overwriting would move accountability
/// without anyone choosing to.
#[tokio::test]
async fn a_second_direct_assignment_is_a_conflict_naming_the_current_domain() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    assign(&app, table, &retail).await;
    let (status, body) = assign(&app, table, &payments).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains("retail"),
        "the response must name the domain it is already in: {body}"
    );
}

/// And `?replace=true` is how a caller says they meant it — a guard that could
/// be satisfied by retrying is not a guard, and one with no way through is an
/// obstruction.
#[tokio::test]
async fn replace_moves_an_already_assigned_asset() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    assign(&app, table, &retail).await;
    let (status, body) = send(
        &app,
        "POST",
        &format!("/assets/{table}/domain?replace=true"),
        Some(json!({ "domainId": payments })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(resolved(&app, table).await["id"], payments.as_str());
}

/// **Assigning over an *inherited* domain is not a conflict.** It is the first
/// direct assignment, and refusing it would make it impossible to override an
/// inherited value at all — the guard eating the feature it is guarding.
#[tokio::test]
async fn assigning_an_asset_that_only_inherits_is_not_a_conflict() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (database, table) = (&ids[1], &ids[3]);

    assign(&app, database, &retail).await;
    let (status, body) = assign(&app, table, &payments).await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn an_assignment_bumps_the_assets_version() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    let (_, before) = send(&app, "GET", &format!("/assets/{table}"), None).await;
    let (_, after) = assign(&app, table, &payments).await;

    assert_ne!(
        after["version"], before["version"],
        "accountability moving is exactly the change a history exists to record"
    );
}

/// Clearing an assignment does not make an asset domainless — it makes it
/// inherit again, which is a different and usually better answer.
#[tokio::test]
async fn clearing_a_direct_assignment_falls_back_to_inheritance() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (database, table) = (&ids[1], &ids[3]);

    assign(&app, database, &retail).await;
    assign(&app, table, &payments).await;

    let (status, _) = send(&app, "DELETE", &format!("/assets/{table}/domain"), None).await;
    assert_eq!(status, StatusCode::OK);

    let assignment = resolved(&app, table).await;
    assert_eq!(assignment["id"], retail.as_str(), "{assignment}");
    assert_eq!(assignment["inherited"], true, "{assignment}");
}

/// **`null`, not a 404.** "This asset is in no domain" is a real and reportable
/// state — it is the assignment-gap report — and a 404 would make it
/// indistinguishable from a bad id.
#[tokio::test]
async fn an_asset_in_no_domain_reports_null_rather_than_a_404() {
    let (app, _db, _url) = test_app().await;
    let ids = hierarchy(&app, "orders-svc").await;

    let (status, body) = send(&app, "GET", &format!("/assets/{}/domain", ids[3]), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.is_null(), "{body}");
}

// ── Slice C: reassignment cascades ──────────────────────────────────────────

/// **The cascade, which under derived resolution is one write.** Moving the
/// database moves every descendant that has not been assigned individually,
/// atomically and with no subtree walk — because the answer was never stored on
/// the descendants in the first place.
#[tokio::test]
async fn moving_a_container_moves_every_descendant_that_has_no_assignment() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let wholesale = domain(&app, "wholesale", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (database, schema, table) = (&ids[1], &ids[2], &ids[3]);

    assign(&app, database, &retail).await;
    assert_eq!(resolved(&app, table).await["id"], retail.as_str());

    let (status, body) = send(
        &app,
        "POST",
        &format!("/assets/{database}/domain?replace=true"),
        Some(json!({ "domainId": wholesale })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    for descendant in [schema, table] {
        assert_eq!(
            resolved(&app, descendant).await["id"],
            wholesale.as_str(),
            "descendant {descendant} should have moved"
        );
    }
}

/// **The sharp one the plan names.** A descendant with its own assignment keeps
/// it when its container moves — a blanket cascade fails this, and it is the
/// difference between "assignment is a default" and "assignment is a lie".
#[tokio::test]
async fn a_descendant_with_its_own_assignment_is_not_moved() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let wholesale = domain(&app, "wholesale", None).await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    let (database, table) = (&ids[1], &ids[3]);

    assign(&app, database, &retail).await;
    assign(&app, table, &payments).await;

    send(
        &app,
        "POST",
        &format!("/assets/{database}/domain?replace=true"),
        Some(json!({ "domainId": wholesale })),
    )
    .await;

    assert_eq!(
        resolved(&app, table).await["id"],
        payments.as_str(),
        "an explicit assignment survives its container moving"
    );
}

/// The count is what tells an operator whether a reorganization did what they
/// meant — and it counts inherited assets, because those are the ones that
/// moved.
#[tokio::test]
async fn the_asset_count_for_a_domain_includes_inherited_assignments() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let ids = hierarchy(&app, "orders-svc").await;

    assign(&app, &ids[0], &retail).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/domains/{retail}/assets/count"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["total"], 4,
        "the service and its three descendants: {body}"
    );
}

// ── Slice D: data products bundle assets ────────────────────────────────────

async fn product(app: &axum::Router, name: &str) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/data-products",
        Some(json!({ "name": name, "purpose": "reporting" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

#[tokio::test]
async fn a_data_product_bundles_assets_from_several_services() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;
    let first = hierarchy(&app, "orders-svc").await;
    let second = hierarchy(&app, "payments-svc").await;

    for table in [&first[3], &second[3]] {
        let (status, body) = send(
            &app,
            "PUT",
            &format!("/data-products/{bundle}/assets/{table}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    let (status, listed) = send(
        &app,
        "GET",
        &format!("/data-products/{bundle}/assets"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["data"].as_array().expect("a page").len(),
        2,
        "{listed}"
    );
}

/// **The inverse of the domain rule, and easy to copy-paste wrong.** An asset
/// belongs to any number of products: the same orders table in "Customer 360"
/// and "Finance Reporting" is not a governance failure, it is two consumable
/// views of one thing.
#[tokio::test]
async fn an_asset_belongs_to_several_data_products() {
    let (app, _db, _url) = test_app().await;
    let first = product(&app, "customer-360").await;
    let second = product(&app, "finance-reporting").await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    for bundle in [&first, &second] {
        let (status, body) = send(
            &app,
            "PUT",
            &format!("/data-products/{bundle}/assets/{table}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    let (status, products) =
        send(&app, "GET", &format!("/assets/{table}/data-products"), None).await;
    assert_eq!(status, StatusCode::OK, "{products}");
    assert_eq!(
        products.as_array().expect("an array").len(),
        2,
        "{products}"
    );
}

/// Adding an asset twice is one edge — the state the caller asked for rather
/// than an error.
#[tokio::test]
async fn adding_an_asset_twice_is_idempotent() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    for _ in 0..2 {
        let (status, _) = send(
            &app,
            "PUT",
            &format!("/data-products/{bundle}/assets/{table}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    let (_, listed) = send(
        &app,
        "GET",
        &format!("/data-products/{bundle}/assets"),
        None,
    )
    .await;
    assert_eq!(
        listed["data"].as_array().expect("a page").len(),
        1,
        "{listed}"
    );
}

#[tokio::test]
async fn adding_an_asset_that_does_not_exist_is_a_400() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/data-products/{bundle}/assets/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **A tombstoned asset is a different refusal from an absent one.** The caller
/// has the right id and the wrong expectation, and "does not exist" would send
/// them looking for a typo that is not there.
#[tokio::test]
async fn adding_a_tombstoned_asset_is_refused_and_says_why() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];

    send(&app, "DELETE", &format!("/assets/{table}"), None).await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/data-products/{bundle}/assets/{table}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("deleted"),
        "the reason has to be the real one: {body}"
    );
}

/// **Removing a member must never delete the asset.** A product is a view of
/// things that exist independently of it, and getting this wrong is
/// catastrophic and irreversible.
#[tokio::test]
async fn removing_an_asset_from_a_product_leaves_the_asset_alone() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];
    send(
        &app,
        "PUT",
        &format!("/data-products/{bundle}/assets/{table}"),
        None,
    )
    .await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/data-products/{bundle}/assets/{table}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, asset) = send(&app, "GET", &format!("/assets/{table}"), None).await;
    assert_eq!(status, StatusCode::OK, "{asset}");
    assert_eq!(asset["deleted"], false, "{asset}");
}

/// Deleting the product takes its memberships and nothing else, for the same
/// reason.
#[tokio::test]
async fn deleting_a_product_leaves_its_assets_alone() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;
    let ids = hierarchy(&app, "orders-svc").await;
    let table = &ids[3];
    send(
        &app,
        "PUT",
        &format!("/data-products/{bundle}/assets/{table}"),
        None,
    )
    .await;

    let (status, _) = send(&app, "DELETE", &format!("/data-products/{bundle}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, asset) = send(&app, "GET", &format!("/assets/{table}"), None).await;
    assert_eq!(status, StatusCode::OK, "{asset}");
}

#[tokio::test]
async fn a_data_product_belongs_to_one_domain() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;

    let (status, created) = send(
        &app,
        "POST",
        "/data-products",
        Some(json!({ "name": "customer-360", "domainId": payments })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["domainId"], payments.as_str());
}

// ── Slice E: both axes are filterable ───────────────────────────────────────

async fn names(app: &axum::Router, uri: &str) -> Vec<String> {
    let (status, body) = send(app, "GET", uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut found: Vec<String> = body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["name"].as_str().expect("a name").to_string())
        .collect();
    found.sort();
    found
}

/// **The query the epic exists for**, and it must include inherited assignment.
/// Matching only direct assignment would report a governed estate as almost
/// empty — the more dangerous direction to be wrong in.
#[tokio::test]
async fn the_domain_filter_returns_inherited_assets_too() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;

    // Only the database is assigned; the schema and table inherit.
    assign(&app, &ids[1], &payments).await;

    let matched = names(&app, &format!("/assets?domain={payments}")).await;

    assert_eq!(
        matched,
        vec!["orders", "public", "sales"],
        "the two inheriting descendants must be in the answer: {matched:?}"
    );
}

/// And the negative: an asset outside the domain must not appear, or the filter
/// would be indistinguishable from no filter at all.
#[tokio::test]
async fn the_domain_filter_excludes_assets_in_other_domains() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let retail = domain(&app, "retail", None).await;
    let first = hierarchy(&app, "orders-svc").await;
    let second = hierarchy(&app, "returns-svc").await;

    assign(&app, &first[0], &payments).await;
    assign(&app, &second[0], &retail).await;

    let matched = names(&app, &format!("/assets?domain={payments}&kind=service")).await;

    assert_eq!(matched, vec!["orders-svc"], "{matched:?}");
}

#[tokio::test]
async fn the_data_product_filter_matches_membership() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;
    let ids = hierarchy(&app, "orders-svc").await;
    send(
        &app,
        "PUT",
        &format!("/data-products/{bundle}/assets/{}", ids[3]),
        None,
    )
    .await;

    let matched = names(&app, &format!("/assets?dataProduct={bundle}")).await;

    assert_eq!(matched, vec!["orders"], "{matched:?}");
}

/// **The wire is camelCase, and this is the assertion that keeps it so.**
/// `AssetListQuery` had `deny_unknown_fields` but no `rename_all`, and every
/// field on it was a single lowercase word — so the wire was camelCase by
/// accident, and the first two-word filter shipped `data_product` beside a
/// surface that is camelCase everywhere else. The existing convention test
/// checks *responses*; nothing checked query parameters, which is why this went
/// out. The snake_case spelling must be refused, not quietly accepted as a
/// second name for the same thing.
#[tokio::test]
async fn a_snake_case_filter_name_is_refused() {
    let (app, _db, _url) = test_app().await;
    let bundle = product(&app, "customer-360").await;

    let (status, body) = send(&app, "GET", &format!("/assets?data_product={bundle}"), None).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a snake_case parameter must not be a second spelling of a camelCase one: {body}"
    );
}

/// Filters compose, and `total` respects them — a count computed before the
/// filter would tell a client there are more pages than there are.
#[tokio::test]
async fn the_domain_filter_composes_with_kind_and_pagination() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    assign(&app, &ids[0], &payments).await;

    let (status, page) = send(
        &app,
        "GET",
        &format!("/assets?domain={payments}&kind=table&limit=10"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{page}");
    let data = page["data"].as_array().expect("a page");
    assert_eq!(data.len(), 1, "only the table, of four assets: {page}");
    assert_eq!(data[0]["name"], "orders");
}

#[tokio::test]
async fn search_takes_the_same_two_filters() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let retail = domain(&app, "retail", None).await;
    let first = hierarchy(&app, "orders-svc").await;
    let second = hierarchy(&app, "returns-svc").await;
    assign(&app, &first[0], &payments).await;
    assign(&app, &second[0], &retail).await;

    let (status, body) = send(
        &app,
        "GET",
        &format!("/assets/search?q=orders&domain={payments}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let found: Vec<&str> = body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["name"].as_str().expect("a name"))
        .collect();
    assert!(found.contains(&"orders"), "{body}");
}

// ── Slice F: deleting a domain does not orphan ──────────────────────────────

#[tokio::test]
async fn an_empty_domain_can_be_deleted() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;

    let (status, body) = send(&app, "DELETE", &format!("/domains/{payments}"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reassignedAssets"], 0);
}

/// **The `409` reports counts**, because "it holds things" tells an operator
/// nothing about whether this is a five-minute cleanup or a quarter's work.
#[tokio::test]
async fn deleting_a_domain_with_assets_is_refused_with_counts() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    assign(&app, &ids[0], &payments).await;

    let (status, body) = send(&app, "DELETE", &format!("/domains/{payments}"), None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains('1'),
        "the count is the actionable part: {body}"
    );
}

/// **Reassignment is transactional and reports what moved.** A delete that
/// silently moved five thousand assets and returned 204 would leave an operator
/// unable to tell whether it did what they meant.
#[tokio::test]
async fn reassign_to_moves_everything_then_deletes() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let retail = domain(&app, "retail", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    assign(&app, &ids[0], &payments).await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/domains/{payments}?reassignTo={retail}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reassignedAssets"], 1, "{body}");

    // The estate moved rather than being orphaned, and the whole subtree came
    // with it because it was inheriting.
    for asset in &ids {
        assert_eq!(
            resolved(&app, asset).await["id"],
            retail.as_str(),
            "asset {asset} should have moved to the target"
        );
    }
    let (status, _) = send(&app, "GET", &format!("/domains/{payments}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reassigning_to_a_domain_that_does_not_exist_is_a_400() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;
    let ids = hierarchy(&app, "orders-svc").await;
    assign(&app, &ids[0], &payments).await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/domains/{payments}?reassignTo={}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **Child domains are never reassigned implicitly.** Where the *assets* go
/// says nothing about where the sub-domains should go, and reparenting them to
/// the target would restructure the accountability tree as a side effect of a
/// delete.
#[tokio::test]
async fn deleting_a_domain_with_children_is_refused_even_with_a_reassign_target() {
    let (app, _db, _url) = test_app().await;
    let retail = domain(&app, "retail", None).await;
    let elsewhere = domain(&app, "elsewhere", None).await;
    domain(&app, "payments", Some(&retail)).await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/domains/{retail}?reassignTo={elsewhere}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains("sub-domain"),
        "the refusal has to say which problem it is: {body}"
    );
}

#[tokio::test]
async fn deleting_a_domain_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/domains/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A typo'd flag must not silently become an unguarded delete.
#[tokio::test]
async fn an_unknown_query_parameter_on_a_domain_delete_is_a_400() {
    let (app, _db, _url) = test_app().await;
    let payments = domain(&app, "payments", None).await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/domains/{payments}?reassign={payments}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
