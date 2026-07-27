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
async fn delete_table_removes_it_and_subsequent_get_returns_404() {
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

    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/tables/{id}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let get_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/tables/{id}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_table_for_nonexistent_id_returns_404() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/tables/{}", Uuid::new_v4()))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
