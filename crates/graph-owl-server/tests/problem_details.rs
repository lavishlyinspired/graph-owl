mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;

const PROBLEM_JSON: &str = "application/problem+json";

fn content_type(response: &axum::response::Response) -> &str {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .expect("error responses must carry a content-type")
}

async fn create_table(app: &axum::Router, name: &str, fqn: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "name": name, "fully_qualified_name": fqn }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

#[tokio::test]
async fn duplicate_fqn_returns_409_problem_json_naming_the_conflicting_entity() {
    let (app, _container) = test_app().await;
    let existing = create_table(&app, "customers", "warehouse.public.customers").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "customers",
                        "fully_qualified_name": "warehouse.public.customers"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(content_type(&response), PROBLEM_JSON);

    let body = json_body(response).await;
    assert_eq!(body["type"], "https://graph-owl.dev/errors/fqn-conflict");
    assert_eq!(body["status"], 409);
    assert!(
        body["title"].as_str().is_some_and(|t| !t.is_empty()),
        "problem must carry a human-readable title"
    );
    // The extension member is what makes the conflict actionable: a client can
    // fetch or update the entity it collided with instead of guessing.
    assert_eq!(body["conflictingId"], existing["id"]);
}

#[tokio::test]
async fn malformed_body_returns_400_problem_json() {
    let (app, _container) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from("{ this is not json"))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(content_type(&response), PROBLEM_JSON);

    let body = json_body(response).await;
    assert_eq!(body["type"], "https://graph-owl.dev/errors/malformed-body");
    assert_eq!(body["status"], 400);
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()));
}

#[tokio::test]
async fn missing_table_returns_404_problem_json() {
    let (app, _container) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/tables/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(content_type(&response), PROBLEM_JSON);

    let body = json_body(response).await;
    assert_eq!(body["type"], "https://graph-owl.dev/errors/not-found");
    assert_eq!(body["status"], 404);
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()));
}

#[tokio::test]
async fn invalid_relationship_type_returns_400_problem_json() {
    let (app, _container) = test_app().await;
    let from = create_table(&app, "orders", "warehouse.public.orders").await;
    let to = create_table(&app, "customers", "warehouse.public.customers").await;

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
                    json!({
                        "to_table_id": to["id"],
                        "relationship_type": ""
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(content_type(&response), PROBLEM_JSON);

    let body = json_body(response).await;
    assert_eq!(
        body["type"],
        "https://graph-owl.dev/errors/validation-failed"
    );
    assert_eq!(body["status"], 400);
}

/// The mutator this slice exists to kill: a `problem_type()` returning one
/// constant passes every single-variant assertion above. Only comparing the
/// variants against each other catches it.
#[tokio::test]
async fn each_error_variant_carries_a_distinct_type_uri() {
    let (app, _container) = test_app().await;
    create_table(&app, "customers", "warehouse.public.customers").await;

    let conflict = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "customers",
                        "fully_qualified_name": "warehouse.public.customers"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    let malformed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from("{ nope"))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    let not_found = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/tables/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    let types: Vec<String> = {
        let mut collected = Vec::new();
        for response in [conflict, malformed, not_found] {
            let body = json_body(response).await;
            collected.push(
                body["type"]
                    .as_str()
                    .expect("every problem carries a type")
                    .to_string(),
            );
        }
        collected
    };

    let unique: std::collections::HashSet<&String> = types.iter().collect();
    assert_eq!(
        unique.len(),
        types.len(),
        "each error variant must have its own type URI, got {types:?}"
    );
}
