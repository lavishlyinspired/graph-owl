//! Epic 22 at the wire.
//!
//! The domain tests prove the type rules exhaustively and without I/O; this
//! proves the three things they cannot — that uniqueness is scoped to the
//! entity type by a real index, that a value cannot reach storage unvalidated,
//! and that a definition holding values refuses to be deleted.

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

fn definition(name: &str, entity_type: &str, property_type: &str) -> Value {
    json!({
        "name": name,
        "entityType": entity_type,
        "propertyType": property_type,
    })
}

async fn define(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    send(app, "POST", "/custom-properties", Some(body)).await
}

// ── definitions ────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_property_can_be_defined_and_listed_for_its_type() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("costCenter", "service", "string")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body["id"].is_string(), "{body}");

    let (status, listed) = send(&app, "GET", "/custom-properties?entityType=service", None).await;
    assert_eq!(status, StatusCode::OK);
    let properties = listed.as_array().expect("an array");
    assert_eq!(properties.len(), 1, "{listed}");
    assert_eq!(properties[0]["name"], "costCenter");
}

/// **Decision 2 as a database constraint.** The same name on two entity types
/// is two different properties; a globally-scoped unique index would silently
/// forbid that, and nothing below this level would notice.
#[tokio::test]
async fn a_name_is_unique_per_entity_type_not_globally() {
    let (app, _db, _url) = test_app().await;

    let (first, _) = define(&app, definition("costCenter", "service", "string")).await;
    assert_eq!(first, StatusCode::CREATED);

    let (same_type, body) = define(&app, definition("costCenter", "service", "string")).await;
    assert_eq!(same_type, StatusCode::CONFLICT, "{body}");

    let (other_type, body) = define(&app, definition("costCenter", "table", "string")).await;
    assert_eq!(
        other_type,
        StatusCode::CREATED,
        "the same name on another type is a different property: {body}"
    );
}

/// A custom `description` would shadow the real field, and every reader would
/// then get one of two values depending on which layer answered.
#[tokio::test]
async fn a_name_colliding_with_a_built_in_field_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("description", "service", "string")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **The supported set is listed**, because decision 4 makes it closed on
/// purpose — a client told only "unsupported" has to go and find the docs.
#[tokio::test]
async fn an_unsupported_type_is_refused_and_the_supported_ones_are_named() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("where", "service", "geolocation")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(rendered.contains("string"), "{rendered}");
    assert!(rendered.contains("entityReference"), "{rendered}");
}

#[tokio::test]
async fn an_enum_without_values_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("tier", "service", "enum")).await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "no value could ever satisfy it: {body}"
    );
}

#[tokio::test]
async fn an_unknown_entity_type_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = define(&app, definition("costCenter", "spaceship", "string")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ── values ─────────────────────────────────────────────────────────────────

async fn service_with(app: &axum::Router, name: &str, extension: Value) -> (StatusCode, Value) {
    send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name, "extension": extension })),
    )
    .await
}

#[tokio::test]
async fn a_defined_property_with_a_correct_value_round_trips() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;

    let (status, body) = service_with(&app, "orders", json!({ "costCenter": "CC-1234" })).await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["extension"]["costCenter"], "CC-1234", "{body}");
}

/// **The failure this epic exists to prevent.** A bag accepted untyped is the
/// description field again, with extra steps — unsearchable, unvalidatable, and
/// impossible to report on.
#[tokio::test]
async fn an_undefined_property_name_is_refused_rather_than_stored() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = service_with(&app, "orders", json!({ "notDefined": "value" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_value_of_the_wrong_type_is_refused_and_both_types_are_named() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("retentionDays", "service", "integer")).await;

    let (status, body) = service_with(&app, "orders", json!({ "retentionDays": "ninety" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(rendered.contains("integer"), "{rendered}");
    assert!(rendered.contains("string"), "{rendered}");
}

/// **Definitions are per entity type**, so a property defined on `table` is
/// undefined on a service — and accepting it there would make the scoping
/// decorative.
#[tokio::test]
async fn a_property_defined_on_another_type_is_undefined_here() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "table", "string")).await;

    let (status, body) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// A constraint violation is a `value` error, not a `type` error: the fix is to
/// send a different *one*, not a different *kind*. A client that retried a
/// range violation by casting would loop.
#[tokio::test]
async fn a_constraint_violation_is_reported_as_a_value_error() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({
            "name": "tier",
            "entityType": "service",
            "propertyType": "enum",
            "constraints": { "values": ["gold", "silver"] },
        }),
    )
    .await;

    let (status, body) = service_with(&app, "orders", json!({ "tier": "bronze" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(
        rendered.contains("gold"),
        "the options are listed: {rendered}"
    );
    assert!(rendered.contains("\"value\""), "{rendered}");
}

/// **Every failure at once.** One fix per round trip is the cost this
/// codebase's accumulating validators exist to avoid.
#[tokio::test]
async fn every_bad_value_in_one_write_is_reported_together() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    define(&app, definition("retentionDays", "service", "integer")).await;

    let (status, body) = service_with(
        &app,
        "orders",
        json!({ "costCenter": 7, "retentionDays": "ninety", "unknown": true }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let errors = body["errors"].as_array().expect("field errors");
    assert_eq!(errors.len(), 3, "{body}");
}

/// An asset carrying no organization-defined values is the normal case, and it
/// must not have to say so.
#[tokio::test]
async fn an_asset_without_an_extension_is_created_normally() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "orders" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(
        body.get("extension").is_none(),
        "an empty bag is absent, not `{{}}` on every asset: {body}"
    );
}

// ── deleting a definition ──────────────────────────────────────────────────

#[tokio::test]
async fn an_unused_definition_can_be_deleted() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id");

    let (status, _) = send(&app, "DELETE", &format!("/custom-properties/{id}"), None).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// **Decision 5: removing a definition does not silently delete data**, and the
/// `409` reports the count — "values exist" tells an operator nothing about
/// whether this is a five-minute cleanup or a quarter's work.
#[tokio::test]
async fn a_definition_holding_values_refuses_to_be_deleted_and_reports_the_count() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    service_with(&app, "payments", json!({ "costCenter": "CC-2" })).await;

    let (status, body) = send(&app, "DELETE", &format!("/custom-properties/{id}"), None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains('2'),
        "the count is the actionable detail: {body}"
    );
}

#[tokio::test]
async fn deleting_a_definition_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/custom-properties/{}", uuid::Uuid::new_v4()),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── values change through PATCH, and the history says so ───────────────────

/// **The criterion Slice B left open.** A value that can only be set at
/// creation is a value nobody can correct, and correcting metadata is most of
/// what a catalog is for.
#[tokio::test]
async fn a_value_can_be_changed_by_patch_and_bumps_the_version() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    let (_, created) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    let id = created["id"].as_str().expect("an id").to_string();
    let before = created["version"].clone();

    let (status, patched) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "extension": { "costCenter": "CC-2" } })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["extension"]["costCenter"], "CC-2");
    assert_ne!(
        patched["version"], before,
        "a value change is a change: {patched}"
    );

    // And it is in the history, not merely in the current state — "when did
    // this field change" is the question a version log exists to answer.
    let (_, versions) = send(&app, "GET", &format!("/assets/{id}/versions"), None).await;
    assert!(
        versions.to_string().contains("costCenter"),
        "the change description must name the field: {versions}"
    );
}

/// **The merge, at the wire.** A patch naming one property must not clear the
/// others — a client forced to send the whole bag is racing every other client
/// doing the same, and the loser's value vanishes with nothing failing.
#[tokio::test]
async fn patching_one_property_leaves_the_others_alone() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    define(
        &app,
        json!({ "name": "retentionDays", "entityType": "service", "propertyType": "integer" }),
    )
    .await;
    let (_, created) = service_with(
        &app,
        "orders",
        json!({ "costCenter": "CC-1", "retentionDays": 30 }),
    )
    .await;
    let id = created["id"].as_str().expect("an id").to_string();

    let (status, patched) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "extension": { "costCenter": "CC-2" } })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["extension"]["retentionDays"], 30, "{patched}");
}

/// An explicit null clears that one property. The negative of the merge above:
/// without it, "clear this field" would have no expression at all.
#[tokio::test]
async fn patching_a_property_to_null_clears_only_that_one() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    define(
        &app,
        json!({ "name": "retentionDays", "entityType": "service", "propertyType": "integer" }),
    )
    .await;
    let (_, created) = service_with(
        &app,
        "orders",
        json!({ "costCenter": "CC-1", "retentionDays": 30 }),
    )
    .await;
    let id = created["id"].as_str().expect("an id").to_string();

    let (status, patched) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "extension": { "costCenter": null } })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{patched}");
    assert!(
        patched["extension"].get("costCenter").is_none(),
        "{patched}"
    );
    assert_eq!(patched["extension"]["retentionDays"], 30, "{patched}");
}

/// A patch is validated against the definitions exactly as a create is. The
/// write path that skipped validation would be the one every client used.
#[tokio::test]
async fn a_patch_carrying_an_undefined_property_is_refused() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    let (_, created) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    let id = created["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "extension": { "notDefined": "x" } })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// A patch that says nothing about `extension` must leave it alone — otherwise
/// every description edit would wipe the organization's fields.
#[tokio::test]
async fn a_patch_that_does_not_mention_extension_leaves_it_intact() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    let (_, created) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    let id = created["id"].as_str().expect("an id").to_string();

    let (status, patched) = send(
        &app,
        "PATCH",
        &format!("/assets/{id}"),
        Some(json!({ "description": "the orders service" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["extension"]["costCenter"], "CC-1", "{patched}");
}

// ── definitions evolve safely (Slice C) ────────────────────────────────────

async fn patch_definition(app: &axum::Router, id: &str, body: Value) -> (StatusCode, Value) {
    send(
        app,
        "PATCH",
        &format!("/custom-properties/{id}"),
        Some(body),
    )
    .await
}

/// Editing the help text is not a schema change, and it must not have to prove
/// anything about the values.
#[tokio::test]
async fn changing_a_description_is_always_allowed() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;

    let (status, body) = patch_definition(
        &app,
        &id,
        json!({ "description": "the accounting cost centre" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["description"], "the accounting cost centre");
}

/// **The core of Slice C.** Retyping a property under existing values would
/// leave every one of them unreadable by the definition that claims to describe
/// them — and the `409` reports how many, because the count is what tells an
/// operator whether this is a cleanup or a project.
#[tokio::test]
async fn changing_the_type_while_values_exist_is_refused_and_reports_the_count() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    service_with(&app, "payments", json!({ "costCenter": "CC-2" })).await;

    let (status, body) = patch_definition(&app, &id, json!({ "propertyType": "integer" })).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains('2'),
        "the count is the actionable detail: {body}"
    );
}

/// **The negative, and it is what makes the guard a guard rather than a ban.**
/// With no values there is nothing to strand, so the same change goes through.
#[tokio::test]
async fn changing_the_type_with_no_values_is_allowed() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();

    let (status, body) = patch_definition(&app, &id, json!({ "propertyType": "integer" })).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["propertyType"], "integer");
}

/// Widening admits everything it did before, so it is always safe — and the
/// check finds that out by trying, not by classifying the change.
#[tokio::test]
async fn widening_a_constraint_is_allowed() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(
        &app,
        json!({
            "name": "retentionDays", "entityType": "service", "propertyType": "integer",
            "constraints": { "minimum": 1.0, "maximum": 90.0 },
        }),
    )
    .await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "retentionDays": 30 })).await;

    let (status, body) = patch_definition(
        &app,
        &id,
        json!({ "constraints": { "minimum": 1.0, "maximum": 365.0 } }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

/// **Narrowing past a value that exists is refused, reporting how many.** The
/// alternative is a definition that says one thing and data that says another,
/// with no error and no way to find the rows.
#[tokio::test]
async fn narrowing_a_constraint_past_existing_values_is_refused_with_the_count() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(
        &app,
        json!({
            "name": "retentionDays", "entityType": "service", "propertyType": "integer",
            "constraints": { "minimum": 1.0, "maximum": 365.0 },
        }),
    )
    .await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "retentionDays": 30 })).await;
    service_with(&app, "payments", json!({ "retentionDays": 200 })).await;

    let (status, body) = patch_definition(
        &app,
        &id,
        json!({ "constraints": { "minimum": 1.0, "maximum": 90.0 } }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains('1'),
        "one of two values is stranded, and saying which is the point: {body}"
    );
}

/// Adding an enum value strands nothing.
#[tokio::test]
async fn adding_an_enum_value_is_allowed() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(
        &app,
        json!({
            "name": "tier", "entityType": "service", "propertyType": "enum",
            "constraints": { "values": ["gold", "silver"] },
        }),
    )
    .await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "tier": "gold" })).await;

    let (status, body) = patch_definition(
        &app,
        &id,
        json!({ "constraints": { "values": ["gold", "silver", "bronze"] } }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Removing one in use does not — and this case is exactly why the check runs
/// the validator rather than classifying the change: "removed a member" and
/// "removed a member nobody uses" look identical to a rule about shape.
#[tokio::test]
async fn removing_an_enum_value_in_use_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(
        &app,
        json!({
            "name": "tier", "entityType": "service", "propertyType": "enum",
            "constraints": { "values": ["gold", "silver"] },
        }),
    )
    .await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "tier": "gold" })).await;

    let (status, body) = patch_definition(
        &app,
        &id,
        json!({ "constraints": { "values": ["silver"] } }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// Removing one nobody uses is fine, which is the half that proves the check is
/// looking at the data rather than at the diff.
#[tokio::test]
async fn removing_an_unused_enum_value_is_allowed() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(
        &app,
        json!({
            "name": "tier", "entityType": "service", "propertyType": "enum",
            "constraints": { "values": ["gold", "silver", "bronze"] },
        }),
    )
    .await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "tier": "gold" })).await;

    let (status, body) = patch_definition(
        &app,
        &id,
        json!({ "constraints": { "values": ["gold", "silver"] } }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
}

/// **A rename migrates the values with it.** A rename that changed only the
/// definition would leave every entity holding a key nothing describes — the
/// data would still be there and the catalog would report the field as unset,
/// which is worse than losing it outright.
#[tokio::test]
async fn renaming_a_definition_migrates_the_values() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    let (_, created) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    let asset_id = created["id"].as_str().expect("an id").to_string();

    let (status, body) = patch_definition(&app, &id, json!({ "name": "costCentre" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, asset) = send(&app, "GET", &format!("/assets/{asset_id}"), None).await;
    assert_eq!(asset["extension"]["costCentre"], "CC-1", "{asset}");
    assert!(
        asset["extension"].get("costCenter").is_none(),
        "the old key must not survive beside the new one: {asset}"
    );
}

/// Renaming onto a name already taken on that type is the same collision a
/// definition would hit, and gets the same answer.
#[tokio::test]
async fn renaming_onto_a_taken_name_is_a_conflict() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    define(&app, definition("owningTeam", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();

    let (status, body) = patch_definition(&app, &id, json!({ "name": "owningTeam" })).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// **`entityType` is not editable, and the DTO is why.** Moving a definition
/// between types would orphan every value under the old one; refusing the field
/// outright is cheaper than guarding an operation nobody should reach for.
#[tokio::test]
async fn a_patch_naming_an_entity_type_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();

    let (status, body) = patch_definition(&app, &id, json!({ "entityType": "table" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn patching_a_definition_that_does_not_exist_is_a_404() {
    let (app, _db, _url) = test_app().await;

    let (status, _) = patch_definition(
        &app,
        &uuid::Uuid::new_v4().to_string(),
        json!({ "description": "x" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── force delete ───────────────────────────────────────────────────────────

/// **`?force=true` is the operator's consent, and it is transactional.** Both
/// halves are asserted: the values are gone, *and* every affected entity's
/// version advanced. A bulk strip would pass the first assertion and fail the
/// second, leaving a field that vanished with no record of when.
#[tokio::test]
async fn force_deleting_removes_the_values_and_bumps_every_affected_version() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    let (_, orders) = service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    let (_, payments) = service_with(&app, "payments", json!({ "costCenter": "CC-2" })).await;
    let orders_id = orders["id"].as_str().expect("an id").to_string();
    let payments_id = payments["id"].as_str().expect("an id").to_string();
    let orders_version = orders["version"].clone();
    let payments_version = payments["version"].clone();

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/custom-properties/{id}?force=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    for (asset_id, before) in [(orders_id, orders_version), (payments_id, payments_version)] {
        let (_, asset) = send(&app, "GET", &format!("/assets/{asset_id}"), None).await;
        assert!(
            asset["extension"].get("costCenter").is_none(),
            "the value must be gone: {asset}"
        );
        assert_ne!(
            asset["version"], before,
            "a field that vanished is a change, and the history has to say so: {asset}"
        );
    }

    let (_, listed) = send(&app, "GET", "/custom-properties?entityType=service", None).await;
    assert!(listed.as_array().expect("an array").is_empty(), "{listed}");
}

/// **The negative that keeps `force` meaningful.** Without the flag the same
/// request is still a `409` — a guard that could be satisfied by retrying is
/// not a guard.
#[tokio::test]
async fn deleting_without_force_still_refuses_and_says_how_to_proceed() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;

    let (status, body) = send(&app, "DELETE", &format!("/custom-properties/{id}"), None).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.to_string().contains("force"),
        "an operator refused an operation deserves to be told the way through: {body}"
    );
}

/// A typo'd flag is a `400`, not a silently-ignored parameter that turns a
/// guarded delete into a refused one — or worse, the other way round.
#[tokio::test]
async fn an_unknown_query_parameter_on_delete_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (_, defined) = define(&app, definition("costCenter", "service", "string")).await;
    let id = defined["id"].as_str().expect("an id").to_string();

    let (status, body) = send(
        &app,
        "DELETE",
        &format!("/custom-properties/{id}?forced=true"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ── custom properties are queryable (Slice D) ──────────────────────────────
//
// **Without this the feature is write-only**, which the plan itself calls worse
// than none: a field you can set, validate and version but cannot ask a question
// about is a description field with more ceremony.

async fn ids(app: &axum::Router, uri: &str) -> Vec<String> {
    let (status, body) = send(app, "GET", uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|asset| asset["name"].as_str().expect("a name").to_string())
        .collect()
}

#[tokio::test]
async fn assets_can_be_filtered_by_a_string_property() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;
    service_with(&app, "payments", json!({ "costCenter": "CC-2" })).await;

    let matched = ids(&app, "/assets?extension.costCenter=CC-1").await;

    assert_eq!(matched, vec!["orders".to_string()], "{matched:?}");
}

/// Integer and boolean are compared as their declared types, not as text. A
/// filter that stringified everything would match `30` against `"30"` — and
/// then quietly fail to match `30` against the number thirty that is stored.
#[tokio::test]
async fn assets_can_be_filtered_by_integer_and_boolean_properties() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({ "name": "retentionDays", "entityType": "service", "propertyType": "integer" }),
    )
    .await;
    define(
        &app,
        json!({ "name": "regulated", "entityType": "service", "propertyType": "boolean" }),
    )
    .await;
    service_with(
        &app,
        "orders",
        json!({ "retentionDays": 30, "regulated": true }),
    )
    .await;
    service_with(
        &app,
        "payments",
        json!({ "retentionDays": 90, "regulated": false }),
    )
    .await;

    assert_eq!(
        ids(&app, "/assets?extension.retentionDays=30").await,
        vec!["orders".to_string()]
    );
    assert_eq!(
        ids(&app, "/assets?extension.regulated=false").await,
        vec!["payments".to_string()]
    );
}

#[tokio::test]
async fn an_enum_property_filters_by_value() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({
            "name": "tier", "entityType": "service", "propertyType": "enum",
            "constraints": { "values": ["gold", "silver"] },
        }),
    )
    .await;
    service_with(&app, "orders", json!({ "tier": "gold" })).await;
    service_with(&app, "payments", json!({ "tier": "silver" })).await;

    assert_eq!(
        ids(&app, "/assets?extension.tier=gold").await,
        vec!["orders".to_string()]
    );
}

/// **Both bounds, and both bounds together.** A range is two filters on one
/// property, which falls out of the conventions doc's "repeated params are AND"
/// rather than needing a grammar of its own.
#[tokio::test]
async fn numeric_properties_support_range_filters() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({ "name": "retentionDays", "entityType": "service", "propertyType": "integer" }),
    )
    .await;
    service_with(&app, "brief", json!({ "retentionDays": 7 })).await;
    service_with(&app, "medium", json!({ "retentionDays": 30 })).await;
    service_with(&app, "long", json!({ "retentionDays": 400 })).await;

    let mut at_least_thirty = ids(&app, "/assets?extension.retentionDays.gte=30").await;
    at_least_thirty.sort();
    assert_eq!(
        at_least_thirty,
        vec!["long", "medium"],
        "{at_least_thirty:?}"
    );

    let banded = ids(
        &app,
        "/assets?extension.retentionDays.gte=10&extension.retentionDays.lte=90",
    )
    .await;
    assert_eq!(banded, vec!["medium".to_string()], "{banded:?}");
}

/// Dates are stored and compared as ISO-8601 strings, which sort
/// lexicographically in the order they sort chronologically — so one comparison
/// serves numbers and dates both.
#[tokio::test]
async fn date_properties_support_range_filters() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({ "name": "reviewedOn", "entityType": "service", "propertyType": "date" }),
    )
    .await;
    service_with(&app, "stale", json!({ "reviewedOn": "2024-01-01" })).await;
    service_with(&app, "fresh", json!({ "reviewedOn": "2026-07-01" })).await;

    let recent = ids(&app, "/assets?extension.reviewedOn.gte=2026-01-01").await;

    assert_eq!(recent, vec!["fresh".to_string()], "{recent:?}");
}

/// **The failure mode the plan singles out.** A typo'd filter that silently
/// returned an empty page would be read as an answer about the data rather than
/// about the request — a false negative nobody investigates.
#[tokio::test]
async fn filtering_on_an_undefined_property_is_a_400_not_an_empty_page() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;

    let (status, body) = send(&app, "GET", "/assets?extension.costCentre=CC-1", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("costCentre"),
        "the response must name the filter that failed: {body}"
    );
}

/// A value that cannot be the declared type is a `400` too, for the same
/// reason: the query cannot be evaluated, and an empty page would claim it was.
#[tokio::test]
async fn a_filter_value_of_the_wrong_type_is_a_400() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({ "name": "retentionDays", "entityType": "service", "propertyType": "integer" }),
    )
    .await;

    let (status, body) = send(&app, "GET", "/assets?extension.retentionDays=forever", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// An unrecognised comparison is somebody meaning `gte`. Reading it as part of
/// the property name would answer with an empty page and no hint.
#[tokio::test]
async fn an_unknown_comparison_suffix_is_a_400() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({ "name": "retentionDays", "entityType": "service", "propertyType": "integer" }),
    )
    .await;

    let (status, body) = send(&app, "GET", "/assets?extension.retentionDays.gt=30", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// **`deny_unknown_fields` still holds.** The extension filters are peeled off
/// the raw query before the typed extractor runs; a flattened map would have
/// absorbed this typo instead and turned an unknown-parameter `400` back into a
/// filter that matches everything.
#[tokio::test]
async fn an_unknown_ordinary_parameter_is_still_a_400() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;

    let (status, body) = send(
        &app,
        "GET",
        "/assets?extension.costCenter=CC-1&ownr=alice",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// Filters compose with the ordinary ones and with pagination, and `total`
/// respects them — a count computed before the filter would tell a client there
/// are more pages than there are.
#[tokio::test]
async fn extension_filters_compose_with_kind_and_pagination() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    for name in ["a", "b", "c"] {
        service_with(&app, name, json!({ "costCenter": "CC-1" })).await;
    }
    service_with(&app, "d", json!({ "costCenter": "CC-2" })).await;

    let (status, first) = send(
        &app,
        "GET",
        "/assets?kind=service&extension.costCenter=CC-1&limit=2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first["data"].as_array().expect("a page").len(),
        2,
        "{first}"
    );

    let after = first["paging"]["after"]
        .as_str()
        .expect("a cursor")
        .to_string();
    let (_, second) = send(
        &app,
        "GET",
        &format!("/assets?kind=service&extension.costCenter=CC-1&limit=2&after={after}"),
        None,
    )
    .await;
    let names: Vec<&str> = second["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["c"], "{second}");
    assert!(
        !second.to_string().contains("\"name\":\"d\""),
        "`d` is in a different cost centre and must not appear on any page: {second}"
    );
}

/// A value with a space in it survives the trip. The `extension.*` pairs are
/// peeled off before `serde_urlencoded` runs, so they need their own decoding —
/// and without it every multi-word value would silently match nothing.
#[tokio::test]
async fn a_filter_value_containing_a_space_matches() {
    let (app, _db, _url) = test_app().await;
    define(&app, definition("owningTeam", "service", "string")).await;
    service_with(&app, "orders", json!({ "owningTeam": "Data Platform" })).await;

    assert_eq!(
        ids(&app, "/assets?extension.owningTeam=Data%20Platform").await,
        vec!["orders".to_string()]
    );
}

/// Search takes the same filters as the list, or a client has to learn two
/// filtering languages and the one that gets it wrong returns more.
#[tokio::test]
async fn search_narrows_by_custom_property_and_facets_enum_values() {
    let (app, _db, _url) = test_app().await;
    define(
        &app,
        json!({
            "name": "tier", "entityType": "service", "propertyType": "enum",
            "constraints": { "values": ["gold", "silver"] },
        }),
    )
    .await;
    service_with(&app, "orders", json!({ "tier": "gold" })).await;
    service_with(&app, "orderbook", json!({ "tier": "silver" })).await;

    let (status, all) = send(&app, "GET", "/assets/search?q=order", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");

    // The facet is over enum properties only: a facet over free text is one
    // bucket per value, which is a report rather than something to click.
    let facet = &all["facets"]["extension.tier"];
    assert!(facet.is_array(), "{all}");

    let (status, gold) = send(
        &app,
        "GET",
        "/assets/search?q=order&extension.tier=gold",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{gold}");
    let names: Vec<&str> = gold["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|a| a["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["orders"], "{gold}");
}

/// **The plan's performance criterion, asserted rather than asserted-about.**
/// `jsonb_path_ops` — the operator class `assets_extension` uses — supports
/// `@>` and nothing else, so equality written as `extension -> name = value`
/// would be a sequential scan of the whole table on the most common filter
/// there is. The plan is the only thing that can tell the two apart.
///
/// Ranges are deliberately not index-backed: a btree on one property's
/// expression supports one property, so a generic range index means an index per
/// definition — the per-property migration decision 4 refuses. They filter what
/// the indexable predicates already narrowed.
#[tokio::test]
async fn equality_filtering_uses_the_extension_index() {
    let (app, _db, url) = test_app().await;
    define(&app, definition("costCenter", "service", "string")).await;
    service_with(&app, "orders", json!({ "costCenter": "CC-1" })).await;

    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    // The planner only prefers an index when the table is big enough to be
    // worth one, and a handful of test rows never is — so the choice is forced
    // to reveal what the operator *can* use. A `@>` the index cannot serve is
    // still a sequential scan under this setting, which is what makes the
    // assertion meaningful rather than a tautology.
    // **One connection, held.** `SET` is session-scoped, and a pool is free to
    // run the `EXPLAIN` on a different connection than the `SET` — which would
    // silently restore `enable_seqscan` and make the assertion test nothing.
    let mut conn = pool.acquire().await.expect("a connection");
    sqlx::query("SET enable_seqscan = off")
        .execute(&mut *conn)
        .await
        .expect("planner setting");
    let plan: String = sqlx::query_scalar::<_, String>(
        "EXPLAIN (FORMAT TEXT) SELECT id FROM assets
          WHERE extension @> jsonb_build_object('costCenter'::text, '\"CC-1\"'::jsonb)",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("a plan")
    .join("\n");

    assert!(
        plan.contains("assets_extension"),
        "equality must be able to use the GIN index, or every filtered list \
         scans the table: {plan}"
    );
}
