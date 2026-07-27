mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
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

async fn create_relationship(app: &axum::Router, from_id: &str, to_id: &str) -> String {
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
                        "relationshipType": "derivedFrom"
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

#[tokio::test]
async fn delete_relationship_removes_it_and_it_no_longer_appears_in_listings() {
    let (app, _container) = test_app().await;
    let from_id = create_table(&app, "warehouse.public.orders").await;
    let to_id = create_table(&app, "warehouse.public.customers").await;
    let relationship_id = create_relationship(&app, &from_id, &to_id).await;

    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/relationships/{relationship_id}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let list_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/tables/{from_id}/relationships"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let body = json_body(list_response).await;
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn delete_relationship_for_nonexistent_id_returns_404() {
    let (app, _container) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/relationships/{}", Uuid::new_v4()))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
