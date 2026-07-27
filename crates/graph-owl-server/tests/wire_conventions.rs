mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

/// Walks a JSON document and collects every object key containing an underscore.
/// A single assertion over the whole response beats one per field: a new field
/// added in `snake_case` is caught without anyone remembering to test it.
fn snake_case_keys(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key.contains('_') {
                    found.push(format!("{path}.{key}"));
                }
                snake_case_keys(child, &format!("{path}.{key}"), found);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                snake_case_keys(child, &format!("{path}[{index}]"), found);
            }
        }
        _ => {}
    }
}

fn assert_no_snake_case(body: &Value, what: &str) {
    let mut found = Vec::new();
    snake_case_keys(body, what, &mut found);
    assert!(
        found.is_empty(),
        "{what} leaked snake_case keys onto the wire: {found:?}"
    );
}

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
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await
}

#[tokio::test]
async fn no_response_on_any_surface_carries_a_snake_case_key() {
    let (app, _container, _connection_string) = test_app().await;
    let from = create_table(&app, "orders", "warehouse.public.orders").await;
    let to = create_table(&app, "customers", "warehouse.public.customers").await;
    assert_no_snake_case(&from, "POST /tables");

    let relationship = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/tables/{}/relationships",
                    from["id"].as_str().unwrap()
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "toTableId": to["id"], "relationshipType": "derivedFrom" }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(relationship.status(), StatusCode::CREATED);
    assert_no_snake_case(&json_body(relationship).await, "POST /relationships");

    for uri in [
        "/tables",
        &format!("/tables/{}", from["id"].as_str().unwrap()),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_no_snake_case(&json_body(response).await, uri);
    }
}

#[tokio::test]
async fn error_bodies_are_camel_case_too() {
    let (app, _container, _connection_string) = test_app().await;

    // A validation failure names fields, so it is the response most likely to
    // leak an internal Rust identifier.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "fullyQualifiedName": "" }).to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    let body = json_body(response).await;
    assert_no_snake_case(&body, "validation error");
    let fields: Vec<&str> = body["errors"]
        .as_array()
        .expect("errors")
        .iter()
        .map(|e| e["field"].as_str().unwrap())
        .collect();
    assert!(
        fields.contains(&"fullyQualifiedName"),
        "an error must name the field the client sent, not the Rust one: {fields:?}"
    );
}

/// Slice A shipped every conflict under one type URI, which was flagged there
/// as loose: a duplicate relationship tuple is not a duplicate FQN, and a client
/// resolving one does something different from a client resolving the other.
#[tokio::test]
async fn an_fqn_conflict_and_a_relationship_conflict_are_different_problems() {
    let (app, _container, _connection_string) = test_app().await;
    let from = create_table(&app, "orders", "warehouse.public.orders").await;
    let to = create_table(&app, "customers", "warehouse.public.customers").await;

    let duplicate_fqn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "name": "orders", "fullyQualifiedName": "warehouse.public.orders" })
                        .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    let relationship_body =
        json!({ "toTableId": to["id"], "relationshipType": "derivedFrom" }).to_string();
    let uri = format!("/tables/{}/relationships", from["id"].as_str().expect("id"));
    for _ in 0..1 {
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&uri)
                    .header("content-type", "application/json")
                    .body(Body::from(relationship_body.clone()))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(created.status(), StatusCode::CREATED);
    }
    let duplicate_relationship = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&uri)
                .header("content-type", "application/json")
                .body(Body::from(relationship_body))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(duplicate_fqn.status(), StatusCode::CONFLICT);
    assert_eq!(duplicate_relationship.status(), StatusCode::CONFLICT);

    let fqn_type = json_body(duplicate_fqn).await["type"]
        .as_str()
        .expect("type")
        .to_string();
    let relationship_type = json_body(duplicate_relationship).await["type"]
        .as_str()
        .expect("type")
        .to_string();

    assert_eq!(fqn_type, "https://graph-owl.dev/errors/fqn-conflict");
    assert_eq!(
        relationship_type,
        "https://graph-owl.dev/errors/relationship-conflict"
    );
    assert_ne!(
        fqn_type, relationship_type,
        "two different uniqueness violations must not share one error identity"
    );
}
