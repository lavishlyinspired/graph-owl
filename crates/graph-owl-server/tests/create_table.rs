mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn post_tables_with_valid_body_returns_201_with_created_table() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "customers",
                        "fullyQualifiedName": "warehouse.public.customers"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    assert_eq!(body["name"], "customers");
    assert_eq!(body["fullyQualifiedName"], "warehouse.public.customers");
    assert!(body["id"].is_string());
    assert!(body["createdAt"].is_string());
    assert!(body["updatedAt"].is_string());
}

#[tokio::test]
async fn post_tables_missing_name_returns_400() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "fullyQualifiedName": "warehouse.public.customers" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_tables_with_duplicate_fully_qualified_name_returns_409() {
    let (app, _container, _connection_string) = test_app().await;
    let body = json!({
        "name": "customers",
        "fullyQualifiedName": "warehouse.public.customers"
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(second.status(), StatusCode::CONFLICT);
}
