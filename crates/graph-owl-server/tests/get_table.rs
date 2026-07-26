use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use graph_owl_api::Catalog;
use graph_owl_storage_postgres::PostgresStorage;
use serde_json::{Value, json};
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use tower::ServiceExt;
use uuid::Uuid;

async fn test_app() -> (axum::Router, ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .start()
        .await
        .expect("failed to start postgres container");
    let host = container.get_host().await.expect("failed to get host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get mapped port");
    let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    let catalog = Catalog::new(Arc::new(storage));

    (graph_owl_server::app(catalog), container)
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body");
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}

#[tokio::test]
async fn get_table_by_id_returns_the_previously_created_table() {
    let (app, _container) = test_app().await;

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
                        "fully_qualified_name": "warehouse.public.customers"
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
    let (app, _container) = test_app().await;

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
