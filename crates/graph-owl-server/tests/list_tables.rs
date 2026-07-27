mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn create_table(app: &axum::Router, name: &str, fqn: &str) -> Value {
    let response = app
        .clone()
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
        .expect("request should be handled");
    json_body(response).await
}

#[tokio::test]
async fn get_tables_with_no_rows_returns_an_empty_array() {
    let (app, _container, _connection_string) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/tables")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body, json!({ "data": [], "paging": { "after": null } }));
}

#[tokio::test]
async fn get_tables_returns_all_created_tables() {
    let (app, _container, _connection_string) = test_app().await;
    let first = create_table(&app, "customers", "warehouse.public.customers").await;
    let second = create_table(&app, "orders", "warehouse.public.orders").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/tables")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    // Sorted by FQN — `customers` before `orders` — which is the contract now,
    // not insertion order.
    assert_eq!(body["data"], json!([first, second]));
    assert_eq!(
        body["paging"]["after"],
        Value::Null,
        "both rows fit one page"
    );
}
