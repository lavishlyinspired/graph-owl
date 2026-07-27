mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn get_table_by_id_returns_the_previously_created_table() {
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

    assert_eq!(get_response.status(), StatusCode::OK);
    let body = json_body(get_response).await;
    assert_eq!(body, created);
}

#[tokio::test]
async fn get_table_by_id_for_nonexistent_id_returns_404() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/tables/{}", Uuid::new_v4()))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
