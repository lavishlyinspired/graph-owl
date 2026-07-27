mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post_table(app: &axum::Router, name: &str, fqn: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "name": name, "fullyQualifiedName": fqn }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled")
}

fn location(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a create must carry a Location header")
        .to_string()
}

// ---------------------------------------------------------------- Slice I

#[tokio::test]
async fn creating_a_table_returns_a_location_pointing_at_the_created_table() {
    let (app, _container) = test_app().await;

    let response = post_table(&app, "orders", "warehouse.public.orders").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let header = location(&response);
    let body = json_body(response).await;

    // Compared against the returned id, not matched against a pattern: a
    // hardcoded or stale path passes a regex and sends the client to the
    // wrong resource.
    assert_eq!(
        header,
        format!("/tables/{}", body["id"].as_str().expect("id")),
        "Location must address the entity that was actually created"
    );
}

#[tokio::test]
async fn the_location_header_actually_resolves() {
    let (app, _container) = test_app().await;

    let created = post_table(&app, "orders", "warehouse.public.orders").await;
    let header = location(&created);
    let created_body = json_body(created).await;

    let followed = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&header)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(
        followed.status(),
        StatusCode::OK,
        "a Location a client cannot follow is worse than none"
    );
    assert_eq!(json_body(followed).await["id"], created_body["id"]);
}

#[tokio::test]
async fn creating_a_relationship_returns_a_location_too() {
    let (app, _container) = test_app().await;
    let from = json_body(post_table(&app, "orders", "warehouse.public.orders").await).await;
    let to = json_body(post_table(&app, "customers", "warehouse.public.customers").await).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/tables/{}/relationships",
                    from["id"].as_str().unwrap()
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "toTableId": to["id"], "relationshipType": "feeds" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::CREATED);
    let header = location(&response);
    let body = json_body(response).await;
    assert_eq!(
        header,
        format!("/relationships/{}", body["id"].as_str().expect("id"))
    );
}

// ---------------------------------------------------------------- Slice H

/// A typo'd filter that silently returns the unfiltered collection is a
/// data-leak-shaped bug: the client believes it applied a restriction that was
/// never applied, and nothing in the response says otherwise.
#[tokio::test]
async fn an_unknown_query_parameter_is_rejected_and_named() {
    let (app, _container) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/tables?ownr=alice")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = json_body(response).await;
    let rendered = body.to_string();
    assert!(
        rendered.contains("ownr"),
        "the response must name the offending parameter — 'bad request' alone \
         leaves the client re-reading its own code: {rendered}"
    );
}

#[tokio::test]
async fn every_documented_query_parameter_is_still_accepted() {
    let (app, _container) = test_app().await;

    for uri in ["/tables", "/tables?limit=5", "/tables?limit=5&after="] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        // The empty cursor is a client error, not an unknown-parameter error.
        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{uri} must be routable"
        );
        if uri != "/tables?limit=5&after=" {
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
    }
}

// ---------------------------------------------------------------- Slice G

/// The seam's entire value is that Epic 12 changes *one* place. A second
/// construction site anywhere in the server means authentication becomes a
/// find-and-replace across handlers instead of an extractor swap — and the one
/// that gets missed is an unauthenticated write path.
#[test]
fn the_extractor_is_the_only_place_a_principal_is_constructed() {
    let source = include_str!("../src/lib.rs");
    let constructions = source.matches("Principal::system()").count();
    assert_eq!(
        constructions, 1,
        "Principal::system() must appear exactly once — inside the extractor"
    );
    assert!(
        source.contains("impl<S> FromRequestParts<S> for Auth"),
        "the single construction site must be the extractor"
    );
}

/// Every mutating handler takes a principal. A create, update or delete that
/// does not is a write nobody can attribute, and Epic 3's `updated_by` will
/// have nothing to record.
#[test]
fn every_mutating_handler_accepts_a_principal() {
    let source = include_str!("../src/lib.rs");
    for handler in [
        "async fn create_table",
        "async fn update_table",
        "async fn delete_table",
        "async fn create_relationship",
        "async fn delete_relationship",
    ] {
        let start = source
            .find(handler)
            .unwrap_or_else(|| panic!("{handler} should exist"));
        let signature = &source[start..start + 300];
        let body_starts = signature.find(" {").unwrap_or(signature.len());
        assert!(
            signature[..body_starts].contains("Auth(principal): Auth"),
            "{handler} mutates and must carry a principal"
        );
    }
}
