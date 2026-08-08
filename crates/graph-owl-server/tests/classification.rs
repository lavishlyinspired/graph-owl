//! Epic 25 at the wire — tags, classifications and labels.
//!
//! **The tests that matter here are the ones about provenance.** Anyone can
//! store a string on an entity; what makes this governance is that a scanner's
//! guess and a steward's decision are distinguishable, that a rejection sticks,
//! and that a label cannot vanish from a thousand columns by accident.

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

async fn classification(app: &axum::Router, name: &str, exclusive: bool) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/classifications",
        Some(json!({ "name": name, "mutuallyExclusive": exclusive })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

async fn tag(app: &axum::Router, classification_id: &str, name: &str) -> String {
    let (status, created) = send(
        app,
        "POST",
        &format!("/classifications/{classification_id}/tags"),
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string()
}

/// A `service` → `database` → `schema` → `table` → two columns, returning every
/// FQN. Column-level labelling is the point of Slice C, so the fixture needs
/// real columns.
async fn hierarchy(app: &axum::Router, service: &str) -> Vec<(String, String)> {
    let mut parent: Option<String> = None;
    let mut fqns = Vec::new();
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
        fqns.push((
            id,
            created["fullyQualifiedName"]
                .as_str()
                .expect("an fqn")
                .to_string(),
        ));
    }
    let table_id = parent.expect("a table");
    for column in ["cust_ssn", "order_total"] {
        let (status, created) = send(
            app,
            "POST",
            "/assets",
            Some(json!({ "kind": "column", "name": column, "parentId": table_id })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        fqns.push((
            created["id"].as_str().expect("an id").to_string(),
            created["fullyQualifiedName"]
                .as_str()
                .expect("an fqn")
                .to_string(),
        ));
    }
    fqns
}

async fn apply(app: &axum::Router, target: &str, tag_fqn: &str) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        &format!("/labels/{target}"),
        Some(json!({ "tagFqn": tag_fqn })),
    )
    .await
}

async fn labels(app: &axum::Router, target: &str) -> Vec<Value> {
    let (status, body) = send(app, "GET", &format!("/labels/{target}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().expect("an array").clone()
}

// ── Slice A: classifications and tags exist ─────────────────────────────────

#[tokio::test]
async fn a_classification_and_its_tags_can_be_defined() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;

    let fqn = tag(&app, &pii, "Sensitive").await;

    assert_eq!(fqn, "PII.Sensitive", "the FQN is derived, not supplied");
    let (status, listed) = send(&app, "GET", "/tags", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed.as_array().expect("an array").len(), 1);
}

/// **Scoped uniqueness, which a global unique index would get wrong.** `Gold`
/// under `Tier` and `Gold` under `SupportPlan` are different tags; refusing the
/// second would make one vocabulary's names block another's.
#[tokio::test]
async fn the_same_tag_name_under_two_classifications_is_two_tags() {
    let (app, _db, _url) = test_app().await;
    let tier = classification(&app, "Tier", false).await;
    let plan = classification(&app, "SupportPlan", false).await;

    assert_eq!(tag(&app, &tier, "Gold").await, "Tier.Gold");
    assert_eq!(tag(&app, &plan, "Gold").await, "SupportPlan.Gold");
}

#[tokio::test]
async fn a_duplicate_tag_under_one_classification_is_a_conflict() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    tag(&app, &pii, "Sensitive").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/classifications/{pii}/tags"),
        Some(json!({ "name": "Sensitive" })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// A dotted name would make `{classification}.{tag}` ambiguous.
#[tokio::test]
async fn a_dotted_tag_name_is_refused() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/classifications/{pii}/tags"),
        Some(json!({ "name": "Sensitive.Extra" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn deleting_a_classification_with_tags_is_refused_unless_recursive() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    tag(&app, &pii, "Sensitive").await;

    let (status, body) = send(&app, "DELETE", &format!("/classifications/{pii}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/classifications/{pii}?recursive=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ── Slice B: tags attach with provenance ────────────────────────────────────

#[tokio::test]
async fn a_manual_label_defaults_to_confirmed_and_says_who_applied_it() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    let (status, body) = apply(&app, &fqns[3].1, &sensitive).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let found = labels(&app, &fqns[3].1).await;
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0]["labelType"], "manual");
    assert_eq!(found[0]["state"], "confirmed");
    assert_eq!(found[0]["tagFqn"], sensitive.as_str());
}

/// **Decision 2 at the wire.** A scanner's proposal must not count as curation,
/// and a caller that forgot to say so must get the *safe* answer rather than
/// the dangerous one.
#[tokio::test]
async fn an_automated_label_defaults_to_suggested() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/labels/{}", fqns[4].1),
        Some(json!({ "tagFqn": sensitive, "labelType": "automated" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let found = labels(&app, &fqns[4].1).await;
    assert_eq!(found[0]["labelType"], "automated");
    assert_eq!(
        found[0]["state"], "suggested",
        "a machine proposal is not curation: {found:#?}"
    );
}

#[tokio::test]
async fn applying_the_same_tag_twice_is_one_label() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    apply(&app, &fqns[3].1, &sensitive).await;
    let (status, _) = apply(&app, &fqns[3].1, &sensitive).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "idempotent, not a conflict");
    assert_eq!(labels(&app, &fqns[3].1).await.len(), 1);
}

/// **Decision 4.** Two tags from one exclusive classification means the
/// classification says nothing.
#[tokio::test]
async fn a_second_tag_from_an_exclusive_classification_is_refused_naming_the_first() {
    let (app, _db, _url) = test_app().await;
    let tier = classification(&app, "Tier", true).await;
    let gold = tag(&app, &tier, "Gold").await;
    let bronze = tag(&app, &tier, "Bronze").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    apply(&app, &fqns[3].1, &gold).await;
    let (status, body) = apply(&app, &fqns[3].1, &bronze).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains("Tier.Gold"),
        "the response must name the conflicting tag: {body}"
    );
}

/// **The negative that makes exclusivity a rule rather than a ban.** A tag from
/// a different classification is the normal case — that is why there is more
/// than one vocabulary.
#[tokio::test]
async fn a_tag_from_another_classification_coexists_with_an_exclusive_one() {
    let (app, _db, _url) = test_app().await;
    let tier = classification(&app, "Tier", true).await;
    let pii = classification(&app, "PII", false).await;
    let gold = tag(&app, &tier, "Gold").await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    apply(&app, &fqns[3].1, &gold).await;
    let (status, body) = apply(&app, &fqns[3].1, &sensitive).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(labels(&app, &fqns[3].1).await.len(), 2);
}

/// And a non-exclusive classification permits several of its own, which is what
/// `PII.Sensitive` beside `PII.Restricted` needs.
#[tokio::test]
async fn a_non_exclusive_classification_permits_several_of_its_tags() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let restricted = tag(&app, &pii, "Restricted").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    apply(&app, &fqns[3].1, &sensitive).await;
    let (status, _) = apply(&app, &fqns[3].1, &restricted).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(labels(&app, &fqns[3].1).await.len(), 2);
}

#[tokio::test]
async fn a_tag_that_does_not_exist_is_refused_naming_it() {
    let (app, _db, _url) = test_app().await;
    let fqns = hierarchy(&app, "orders-svc").await;

    let (status, body) = apply(&app, &fqns[3].1, "PII.Imaginary").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("PII.Imaginary"), "{body}");
}

/// A label on a name nothing resolves to is a governance claim about nothing,
/// and it would sit there looking like coverage.
#[tokio::test]
async fn a_target_that_does_not_exist_is_refused() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;

    let (status, body) = apply(&app, "nothing.like.this", &sensitive).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn applying_a_label_bumps_the_targets_version() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    let (_, before) = send(&app, "GET", &format!("/assets/{}", fqns[3].0), None).await;
    apply(&app, &fqns[3].1, &sensitive).await;
    let (_, after) = send(&app, "GET", &format!("/assets/{}", fqns[3].0), None).await;

    assert_ne!(
        after["version"], before["version"],
        "a governance label appearing is exactly what a consumer watches for"
    );
}

// ── Slice C: columns are taggable ───────────────────────────────────────────

/// **`PII` belongs on the SSN column, not the whole table.** Table-level
/// labelling is too coarse to act on — masking a table is not a thing anybody
/// wants to do.
#[tokio::test]
async fn a_single_column_can_be_labelled_without_its_siblings() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    let (ssn, total) = (&fqns[4].1, &fqns[5].1);

    apply(&app, ssn, &sensitive).await;

    assert_eq!(labels(&app, ssn).await.len(), 1);
    assert!(
        labels(&app, total).await.is_empty(),
        "the sibling column must be untouched"
    );
}

/// A label is keyed by **name**, not position, so a table PATCH that reorders
/// columns cannot move it to the wrong one.
#[tokio::test]
async fn a_column_label_is_keyed_by_name_not_position() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    let ssn = &fqns[4].1;

    apply(&app, ssn, &sensitive).await;

    // Adding another column afterwards changes nothing about which column the
    // existing label is on — the FQN is the key, so a name that sorts before
    // every existing one cannot shift it.
    send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "column", "name": "aaa_first", "parentId": fqns[3].0 })),
    )
    .await;

    let found = labels(&app, ssn).await;
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0]["targetFqn"], ssn.as_str());
}

// ── Slice D: suggestions are triaged ────────────────────────────────────────

async fn suggest(app: &axum::Router, target: &str, tag_fqn: &str) -> StatusCode {
    send(
        app,
        "POST",
        &format!("/labels/{target}"),
        Some(json!({ "tagFqn": tag_fqn, "labelType": "automated" })),
    )
    .await
    .0
}

#[tokio::test]
async fn a_suggestion_appears_in_the_triage_queue_and_a_confirmed_label_does_not() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    suggest(&app, &fqns[4].1, &sensitive).await;
    apply(&app, &fqns[5].1, &sensitive).await;

    let (status, queue) = send(&app, "GET", "/label-suggestions", None).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let waiting = queue.as_array().expect("an array");
    assert_eq!(waiting.len(), 1, "{queue}");
    assert_eq!(waiting[0]["targetFqn"], fqns[4].1.as_str());
}

#[tokio::test]
async fn confirming_a_suggestion_flips_it_and_records_who() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    suggest(&app, &fqns[4].1, &sensitive).await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/confirm", fqns[4].1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let found = labels(&app, &fqns[4].1).await;
    assert_eq!(found[0]["state"], "confirmed");
    assert!(found[0]["confirmedBy"].is_string(), "{found:#?}");
}

#[tokio::test]
async fn confirming_an_already_confirmed_label_is_a_conflict() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[4].1, &sensitive).await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/confirm", fqns[4].1),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn confirming_a_label_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/confirm", fqns[4].1),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// **The sharp one.** A rejection that merely deleted the label would be
/// re-proposed by the next run of the same scanner, and a steward would answer
/// the same question forever.
#[tokio::test]
async fn a_rejected_suggestion_is_not_re_proposed_by_the_next_scan() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    suggest(&app, &fqns[4].1, &sensitive).await;

    let (status, _) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/reject", fqns[4].1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(labels(&app, &fqns[4].1).await.is_empty());

    // The scanner runs again over the same column.
    suggest(&app, &fqns[4].1, &sensitive).await;

    assert!(
        labels(&app, &fqns[4].1).await.is_empty(),
        "a rejection has to stick, or the steward answers it forever"
    );
}

/// **And a human changing their mind is not the loop the ledger exists to
/// break.** Only automated re-proposals are dropped; a person deliberately
/// applying a once-rejected tag is making a decision.
#[tokio::test]
async fn a_person_may_apply_a_tag_that_was_once_rejected() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    suggest(&app, &fqns[4].1, &sensitive).await;
    send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/reject", fqns[4].1),
        None,
    )
    .await;

    let (status, body) = apply(&app, &fqns[4].1, &sensitive).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert_eq!(labels(&app, &fqns[4].1).await.len(), 1);
}

// ── Slice H: tags in use cannot vanish silently ─────────────────────────────

#[tokio::test]
async fn an_unused_tag_can_be_deleted() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;

    let (status, body) = send(&app, "DELETE", &format!("/tags/{sensitive}"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removedLabels"], 0);
}

/// **Decision 5, with counts by kind.** "It is in use" tells a steward nothing
/// about the shape of the cleanup; "1 table, 2 columns" tells them whether this
/// is a propagation to undo or a curation to redo.
#[tokio::test]
async fn deleting_a_tag_in_use_is_refused_with_counts_by_kind() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[3].1, &sensitive).await;
    apply(&app, &fqns[4].1, &sensitive).await;

    let (status, body) = send(&app, "DELETE", &format!("/tags/{sensitive}"), None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let reported = body.to_string();
    assert!(reported.contains("table"), "{reported}");
    assert!(reported.contains("column"), "{reported}");
}

/// `?force=true` removes the labels **and** advances every affected entity's
/// version, so a label that disappeared from a thousand columns is visible in
/// each of their histories rather than only in this response.
#[tokio::test]
async fn force_deleting_a_tag_removes_every_label() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[3].1, &sensitive).await;
    apply(&app, &fqns[4].1, &sensitive).await;

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/tags/{sensitive}?force=true"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["removedLabels"], 2, "{body}");
    assert!(labels(&app, &fqns[3].1).await.is_empty());
    assert!(labels(&app, &fqns[4].1).await.is_empty());
}

/// A tombstoned entity does not keep a governance label alive — counting it
/// would refuse a delete over data nobody can see.
#[tokio::test]
async fn a_soft_deleted_entity_does_not_count_toward_tag_usage() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[4].1, &sensitive).await;

    send(&app, "DELETE", &format!("/assets/{}", fqns[4].0), None).await;

    let (status, usage) = send(&app, "GET", &format!("/tags/{sensitive}/usage"), None).await;
    assert_eq!(status, StatusCode::OK, "{usage}");
    assert_eq!(usage["total"], 0, "{usage}");
}

// ── Slice I: propagation, on request ────────────────────────────────────────

#[tokio::test]
async fn propagating_a_table_tag_reaches_its_columns_as_propagated() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[3].1, &sensitive).await;

    let (status, body) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/propagate", fqns[3].1),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["affected"], 2, "both columns: {body}");
    for (_, column) in &fqns[4..6] {
        let found = labels(&app, column).await;
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0]["labelType"], "propagated");
    }
}

/// **The sharp one the plan names.** A steward's deliberate label survives, and
/// calling it `Propagated` afterwards would also be a lie about where it came
/// from.
#[tokio::test]
async fn propagation_does_not_downgrade_a_manual_label() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[3].1, &sensitive).await;
    apply(&app, &fqns[4].1, &sensitive).await;

    send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/propagate", fqns[3].1),
        None,
    )
    .await;

    let found = labels(&app, &fqns[4].1).await;
    assert_eq!(
        found[0]["labelType"], "manual",
        "a deliberate choice survives a propagate: {found:#?}"
    );
}

/// Propagation is one level unless asked otherwise — a service tag reaching
/// every column by default is exactly the surprising behaviour decision 3
/// refuses.
#[tokio::test]
async fn propagation_is_one_level_unless_recursive() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[0].1, &sensitive).await;

    let (_, shallow) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/propagate", fqns[0].1),
        None,
    )
    .await;
    assert_eq!(shallow["affected"], 1, "only the database: {shallow}");
    assert!(labels(&app, &fqns[4].1).await.is_empty());

    let (_, deep) = send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/propagate?recursive=true", fqns[0].1),
        None,
    )
    .await;
    assert!(deep["affected"].as_i64().unwrap_or(0) >= 3, "{deep}");
    assert_eq!(labels(&app, &fqns[4].1).await.len(), 1);
}

/// A propagated label is removable on its own, and removing the parent's does
/// not take it — once created they are independent.
#[tokio::test]
async fn propagated_labels_are_independent_of_the_parents() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[3].1, &sensitive).await;
    send(
        &app,
        "POST",
        &format!("/labels/{}/{sensitive}/propagate", fqns[3].1),
        None,
    )
    .await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/labels/{}/{sensitive}", fqns[3].1),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        labels(&app, &fqns[4].1).await.len(),
        1,
        "removing the parent tag does not auto-remove what it propagated"
    );
}

// ── `?tags=` filter — Phase 2.1 of plans/EPIC-COMPLETION-PLAN.md ───────────

async fn asset_names(app: &axum::Router, uri: &str) -> Vec<String> {
    let (status, page) = send(app, "GET", uri, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let mut found: Vec<String> = page["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["name"].as_str().expect("a name").to_string())
        .collect();
    found.sort();
    found
}

/// AND across every tag named — matching a table only when it carries **all**
/// of them, not any.
#[tokio::test]
async fn the_tags_filter_returns_only_assets_carrying_every_named_tag() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let tier = classification(&app, "Tier", false).await;
    let gold = tag(&app, &tier, "Gold").await;
    let both = hierarchy(&app, "orders-svc").await;
    let only_one = hierarchy(&app, "returns-svc").await;

    apply(&app, &both[3].1, &sensitive).await;
    apply(&app, &both[3].1, &gold).await;
    apply(&app, &only_one[3].1, &sensitive).await;

    let matched = asset_names(&app, &format!("/assets?kind=table&tags={sensitive},{gold}")).await;

    assert_eq!(matched, vec!["orders"], "{matched:?}");
}

/// **A table-level match counts a confirmed label on one of its own columns
/// too** — a steward asking "what carries PII" is asking about the table.
#[tokio::test]
async fn the_tags_filter_counts_a_confirmed_column_label_as_the_table_carrying_it() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    // The column, not the table itself.
    apply(&app, &fqns[4].1, &sensitive).await;

    let matched = asset_names(&app, &format!("/assets?kind=table&tags={sensitive}")).await;

    assert_eq!(matched, vec!["orders"], "{matched:?}");
}

/// **A suggested-not-confirmed label counts for nothing** — the same rule
/// the triage queue itself already enforces.
#[tokio::test]
async fn the_tags_filter_excludes_a_label_still_awaiting_confirmation() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    suggest(&app, &fqns[3].1, &sensitive).await;

    let matched = asset_names(&app, &format!("/assets?kind=table&tags={sensitive}")).await;

    assert!(
        matched.is_empty(),
        "a suggestion is not yet a fact: {matched:?}"
    );
}

#[tokio::test]
async fn the_tags_filter_also_applies_to_search() {
    let (app, _db, _url) = test_app().await;
    let pii = classification(&app, "PII", false).await;
    let sensitive = tag(&app, &pii, "Sensitive").await;
    let fqns = hierarchy(&app, "orders-svc").await;
    apply(&app, &fqns[3].1, &sensitive).await;

    let matched = asset_names(&app, &format!("/assets/search?q=orders&tags={sensitive}")).await;
    assert_eq!(matched, vec!["orders"], "{matched:?}");

    let empty = asset_names(
        &app,
        &format!("/assets/search?q=orders&tags={sensitive},Tier.Gold"),
    )
    .await;
    assert!(empty.is_empty(), "Tier.Gold was never applied: {empty:?}");
}
