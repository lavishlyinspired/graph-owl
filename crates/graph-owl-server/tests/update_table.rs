mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn patch_table_updates_description_and_advances_updated_at() {
    let (app, _container, _connection_string) = test_app().await;

    let create_response = app
        .clone()
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
    let created = json_body(create_response).await;
    let id = created["id"].as_str().expect("id should be a string");

    let patch_response = app
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/tables/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "description": "a new description" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(patch_response.status(), StatusCode::OK);
    let body = json_body(patch_response).await;
    assert_eq!(body["name"], created["name"]);
    assert_eq!(body["fullyQualifiedName"], created["fullyQualifiedName"]);
    assert_eq!(body["description"], "a new description");
    assert_eq!(body["createdAt"], created["createdAt"]);
    assert_ne!(body["updatedAt"], created["updatedAt"]);
}

#[tokio::test]
async fn patch_table_for_nonexistent_id_returns_404() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/tables/{}", Uuid::new_v4()))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "description": "new" }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
