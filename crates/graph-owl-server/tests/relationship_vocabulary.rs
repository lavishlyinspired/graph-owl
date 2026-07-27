mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

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

async fn relate(
    app: &axum::Router,
    from: &str,
    to: &str,
    relationship_type: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{from}/relationships"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "toTableId": to, "relationshipType": relationship_type }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled")
}

#[tokio::test]
async fn every_legal_table_to_table_type_is_accepted() {
    let (app, _container, _connection_string) = test_app().await;
    let from = create_table(&app, "orders", "warehouse.public.orders").await;
    let to = create_table(&app, "customers", "warehouse.public.customers").await;
    let (from_id, to_id) = (from["id"].as_str().unwrap(), to["id"].as_str().unwrap());

    for relationship_type in [
        "feeds",
        "derivedFrom",
        "dependsOn",
        "uses",
        "sameAs",
        "relatedTo",
    ] {
        let response = relate(&app, from_id, to_id, relationship_type).await;
        assert_eq!(
            response.status(),
            StatusCode::CREATED,
            "`{relationship_type}` is legal between two tables"
        );
    }
}

#[tokio::test]
async fn an_unknown_type_is_rejected_and_the_response_lists_the_vocabulary() {
    let (app, _container, _connection_string) = test_app().await;
    let from = create_table(&app, "orders", "warehouse.public.orders").await;
    let to = create_table(&app, "customers", "warehouse.public.customers").await;

    let response = relate(
        &app,
        from["id"].as_str().unwrap(),
        to["id"].as_str().unwrap(),
        "derived_from", // the snake_case spelling — a plausible client mistake
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(
        body["type"],
        "https://graph-owl.dev/errors/validation-failed"
    );
    let detail = body["errors"][0]["detail"].as_str().expect("detail");
    assert!(
        detail.contains("derived_from") && detail.contains("derivedFrom"),
        "the error must echo what was sent *and* offer the vocabulary, so the \
         fix is visible without reading docs: {detail:?}"
    );
}

/// `Table contains Table` is well-formed and meaningless. Containment is
/// hierarchy, and no general rule distinguishes it from `Table feeds Table` —
/// which is why the legality table exists.
#[tokio::test]
async fn a_legal_type_in_an_illegal_position_is_rejected_distinctly() {
    let (app, _container, _connection_string) = test_app().await;
    let from = create_table(&app, "orders", "warehouse.public.orders").await;
    let to = create_table(&app, "customers", "warehouse.public.customers").await;

    let response = relate(
        &app,
        from["id"].as_str().unwrap(),
        to["id"].as_str().unwrap(),
        "contains",
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(
        body["type"], "https://graph-owl.dev/errors/illegal-relationship",
        "a known type used wrongly is not the same problem as an unknown type: \
         the client fixes it by choosing a different relationship, not a value"
    );
}

/// The ordering criterion. Validation runs before existence checks, so a client
/// sending an illegal triple is told about the triple — not sent hunting for
/// tables that were never the problem.
#[tokio::test]
async fn an_illegal_triple_between_nonexistent_tables_reports_the_triple_not_a_404() {
    let (app, _container, _connection_string) = test_app().await;

    let response = relate(
        &app,
        &Uuid::new_v4().to_string(),
        &Uuid::new_v4().to_string(),
        "contains",
    )
    .await;

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the triple is checked first"
    );
    let body = json_body(response).await;
    assert_eq!(
        body["type"],
        "https://graph-owl.dev/errors/illegal-relationship"
    );
}

#[tokio::test]
async fn a_legal_triple_between_nonexistent_tables_is_still_a_404() {
    let (app, _container, _connection_string) = test_app().await;

    let response = relate(
        &app,
        &Uuid::new_v4().to_string(),
        &Uuid::new_v4().to_string(),
        "feeds",
    )
    .await;

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "once the triple is fine, missing tables are the real problem"
    );
}
