mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

async fn create_table(app: &axum::Router, fully_qualified_name: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "customers",
                        "fullyQualifiedName": fully_qualified_name
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    body["id"]
        .as_str()
        .expect("id should be a string")
        .to_string()
}

async fn create_relationship(app: &axum::Router, from_id: &str, to_id: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{from_id}/relationships"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "toTableId": to_id,
                        "relationshipType": "derived_from"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn get_relationships_for_a_table_with_none_returns_an_empty_array() {
    let (app, _container) = test_app().await;
    let id = create_table(&app, "warehouse.public.customers").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/tables/{id}/relationships"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn get_relationships_for_a_table_returns_relationships_from_either_side() {
    let (app, _container) = test_app().await;
    let orders_id = create_table(&app, "warehouse.public.orders").await;
    let customers_id = create_table(&app, "warehouse.public.customers").await;
    let archive_id = create_table(&app, "warehouse.public.orders_archive").await;
    create_relationship(&app, &orders_id, &customers_id).await;
    create_relationship(&app, &archive_id, &orders_id).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/tables/{orders_id}/relationships"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let relationships = body.as_array().expect("body should be an array");
    assert_eq!(relationships.len(), 2);
}

#[tokio::test]
async fn get_relationships_for_a_nonexistent_table_returns_404() {
    let (app, _container) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/tables/{}/relationships", Uuid::new_v4()))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
