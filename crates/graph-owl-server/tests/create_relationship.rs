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

#[tokio::test]
async fn post_relationships_with_valid_body_returns_201_with_created_relationship() {
    let (app, _container) = test_app().await;
    let from_id = create_table(&app, "warehouse.public.orders").await;
    let to_id = create_table(&app, "warehouse.public.customers").await;

    let response = app
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
    assert!(body["id"].is_string());
    assert_eq!(body["fromEntityType"], "table");
    assert_eq!(body["fromEntityId"], from_id);
    assert_eq!(body["toEntityType"], "table");
    assert_eq!(body["toEntityId"], to_id);
    assert_eq!(body["relationshipType"], "derivedFrom");
    assert!(body["createdAt"].is_string());
}

#[tokio::test]
async fn post_relationships_for_a_nonexistent_source_table_returns_404() {
    let (app, _container) = test_app().await;
    let to_id = create_table(&app, "warehouse.public.customers").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{}/relationships", Uuid::new_v4()))
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

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_relationships_targeting_a_nonexistent_table_returns_404() {
    let (app, _container) = test_app().await;
    let from_id = create_table(&app, "warehouse.public.orders").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{from_id}/relationships"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "toTableId": Uuid::new_v4(),
                        "relationshipType": "derivedFrom"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_relationships_with_empty_relationship_type_returns_400() {
    let (app, _container) = test_app().await;
    let from_id = create_table(&app, "warehouse.public.orders").await;
    let to_id = create_table(&app, "warehouse.public.customers").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{from_id}/relationships"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "toTableId": to_id,
                        "relationshipType": ""
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_relationships_with_duplicate_tuple_returns_409() {
    let (app, _container) = test_app().await;
    let from_id = create_table(&app, "warehouse.public.orders").await;
    let to_id = create_table(&app, "warehouse.public.customers").await;
    let body = json!({
        "toTableId": to_id,
        "relationshipType": "derivedFrom"
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{from_id}/relationships"))
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
                .uri(format!("/tables/{from_id}/relationships"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(second.status(), StatusCode::CONFLICT);
}
