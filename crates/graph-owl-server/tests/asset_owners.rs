//! Epic 11 Slice C at the wire.
//!
//! The repository tests prove the schema and the facade tests prove the mapping.
//! What is claimed here is the part only an HTTP test can see: the status codes,
//! the field path in the error body, and the shape of `owners` on the wire.

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

async fn asset(app: &axum::Router, name: &str) -> String {
    let (status, body) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().expect("an id").to_string()
}

/// The one user that exists without somebody signing in.
///
/// **There is no endpoint that creates a user.** Users are auto-provisioned on
/// first authentication (Epic 12 Slice A) and `PUT /users/{id}/roles` refuses an
/// unknown id, so a person who has never signed in cannot be named as an owner
/// through the API at all. Found while writing this test.
///
/// That is not a gap to paper over here: it is precisely why Epic 41 still lists
/// **principal and group management** as outstanding, and it is now recorded there
/// with this as the concrete consequence. `system` is seeded by `V15` and is the
/// only user available, so it stands in for the human-owner case.
const SEEDED_USER: &str = "system";

async fn team(app: &axum::Router, id: &str, name: &str) {
    let (status, body) = send(
        app,
        "POST",
        "/teams",
        Some(json!({ "id": id, "displayName": name, "members": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn owners_are_set_and_returned_with_names() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    team(&app, "platform", "Platform Team").await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [
            { "id": SEEDED_USER, "kind": "user" },
            { "id": "platform", "kind": "team" },
        ]})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let owners = body["owners"].as_array().expect("a list");
    assert_eq!(owners.len(), 2, "{body}");
    // Denormalized on the wire: a console rendering an owner list should not need
    // N follow-up requests to turn ids into names.
    assert_eq!(owners[0]["displayName"], SEEDED_USER);
    assert_eq!(owners[1]["displayName"], "Platform Team");
    assert_eq!(owners[1]["kind"], "team");
}

// "Owner referencing a nonexistent principal → `400` naming the index."
#[tokio::test]
async fn an_unknown_principal_is_a_400_naming_which_owner() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [
            { "id": SEEDED_USER, "kind": "user" },
            { "id": "nobody", "kind": "user" },
        ]})),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("owners[1].id"),
        "the error must name which owner: {body}"
    );
}

// "Removing all owners is allowed — an unowned asset is a real, reportable state."
#[tokio::test]
async fn an_empty_owner_list_is_accepted() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": SEEDED_USER, "kind": "user" }] })),
    )
    .await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["owners"], json!([]));
}

// Owners ride along with the asset, which is what a console list needs and what
// the aggregated subquery exists for.
#[tokio::test]
async fn owners_appear_on_the_asset_itself() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    team(&app, "platform", "Platform Team").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": "platform", "kind": "team" }] })),
    )
    .await;

    let (status, body) = send(&app, "GET", &format!("/assets/{orders}"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["owners"][0]["displayName"], "Platform Team", "{body}");
}

// An unowned asset carries `owners: []`, never an absent field. The distinction
// is not cosmetic: the version classifier treats a *removed* field as breaking,
// so a field that vanished when the last owner was dropped would make a
// governance event read as a schema break.
#[tokio::test]
async fn an_unowned_asset_still_carries_an_owners_field() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;

    let (_, body) = send(&app, "GET", &format!("/assets/{orders}"), None).await;

    assert_eq!(body["owners"], json!([]), "{body}");
}

#[tokio::test]
async fn the_owners_of_a_missing_asset_cannot_be_set() {
    let (app, _db, _) = test_app().await;

    let (status, _) = send(
        &app,
        "PUT",
        &format!("/assets/{}/owners", uuid::Uuid::new_v4()),
        Some(json!({ "owners": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// A read of the dedicated endpoint agrees with the asset's own field — two views
// of one fact that could drift apart.
#[tokio::test]
async fn the_owners_endpoint_agrees_with_the_asset() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": SEEDED_USER, "kind": "user" }] })),
    )
    .await;

    let (_, dedicated) = send(&app, "GET", &format!("/assets/{orders}/owners"), None).await;
    let (_, whole) = send(&app, "GET", &format!("/assets/{orders}"), None).await;

    assert_eq!(dedicated["owners"], whole["owners"]);
}

// The kind is required rather than inferred, because `users.id` and `teams.id` are
// both free text and can collide — guessing would silently assign the wrong
// principal to the field where a silent wrong answer is worst.
#[tokio::test]
async fn an_owner_without_a_kind_is_refused() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;

    let (status, _) = send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": SEEDED_USER }] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---- Epic 11 Slice D: ownership inherits ----

/// `warehouse` → `warehouse.retail` → `warehouse.retail.public` →
/// `warehouse.retail.public.orders`, returned outermost-first.
async fn estate(app: &axum::Router) -> [String; 4] {
    let service = asset(app, "warehouse").await;
    let mut ids = vec![service];
    for (kind, name) in [
        ("database", "retail"),
        ("schema", "public"),
        ("table", "orders"),
    ] {
        let parent = ids.last().expect("a parent").clone();
        let (status, body) = send(
            app,
            "POST",
            "/assets",
            Some(json!({ "kind": kind, "name": name, "parentId": parent })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        ids.push(body["id"].as_str().expect("an id").to_string());
    }
    ids.try_into().expect("four levels")
}

// The wire shape of the flag, asserted against the bytes. A round trip agrees
// with itself whatever the field is called — the lesson `Authorship` taught by
// shipping `agent_id` — and this field exists to be read by a console.
#[tokio::test]
async fn an_inherited_owner_is_flagged_on_the_wire() {
    let (app, _db, _) = test_app().await;
    let [_service, _database, schema, table] = estate(&app).await;
    team(&app, "platform", "Platform Team").await;
    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{schema}/owners"),
        Some(json!({ "owners": [{ "id": "platform", "kind": "team" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(&app, "GET", &format!("/assets/{table}"), None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["owners"][0]["displayName"], "Platform Team", "{body}");
    assert_eq!(body["owners"][0]["inherited"], json!(true), "{body}");
}

// The flag is always present, never omitted when false — a console reading its
// absence cannot tell "owned here" from "an older server that did not know about
// inheritance", and those need different UI.
#[tokio::test]
async fn a_direct_owner_carries_the_flag_set_to_false() {
    let (app, _db, _) = test_app().await;
    let orders = asset(&app, "orders").await;
    send(
        &app,
        "PUT",
        &format!("/assets/{orders}/owners"),
        Some(json!({ "owners": [{ "id": SEEDED_USER, "kind": "user" }] })),
    )
    .await;

    let (_, body) = send(&app, "GET", &format!("/assets/{orders}"), None).await;

    assert_eq!(body["owners"][0]["inherited"], json!(false), "{body}");
}

// The write echo reports what was *recorded*, not what would now be read. A PUT
// that answered with `inherited: true` would tell a client its write had not
// landed where it asked.
#[tokio::test]
async fn setting_owners_echoes_them_as_direct() {
    let (app, _db, _) = test_app().await;
    let [_service, database, _schema, _table] = estate(&app).await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/assets/{database}/owners"),
        Some(json!({ "owners": [{ "id": SEEDED_USER, "kind": "user" }] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["owners"][0]["inherited"], json!(false), "{body}");
}
