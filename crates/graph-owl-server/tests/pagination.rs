mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::json;
use tower::ServiceExt;

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled")
}

async fn seed(app: &axum::Router, count: usize) {
    for n in 0..count {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tables")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "name": format!("t{n:03}"),
                            "fullyQualifiedName": format!("warehouse.public.t{n:03}")
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(response.status(), StatusCode::CREATED);
    }
}

#[tokio::test]
async fn a_client_can_walk_every_page_using_only_the_returned_cursor() {
    let (app, _container, _connection_string) = test_app().await;
    seed(&app, 25).await;

    let mut seen = Vec::new();
    let mut uri = "/tables?limit=10".to_string();
    let mut pages = 0;

    loop {
        let body = json_body(get(&app, &uri).await).await;
        pages += 1;
        for table in body["data"].as_array().expect("data array") {
            seen.push(table["fullyQualifiedName"].as_str().unwrap().to_string());
        }
        let Some(after) = body["paging"]["after"].as_str() else {
            break;
        };
        // Percent-encoding matters: a URL-safe base64 token still needs to
        // survive being placed in a query string.
        uri = format!("/tables?limit=10&after={after}");
        assert!(pages < 10, "pagination failed to terminate");
    }

    assert_eq!(pages, 3, "25 rows in pages of 10");
    assert_eq!(seen.len(), 25, "every row seen exactly once");
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 25, "no duplicates: {seen:?}");
}

#[tokio::test]
async fn the_default_limit_applies_when_none_is_given() {
    let (app, _container, _connection_string) = test_app().await;
    seed(&app, 30).await;

    let body = json_body(get(&app, "/tables").await).await;
    assert_eq!(
        body["data"].as_array().expect("data").len(),
        25,
        "the documented default is 25"
    );
    assert!(
        body["paging"]["after"].is_string(),
        "30 rows do not fit the default page"
    );
}

#[tokio::test]
async fn a_limit_above_the_maximum_is_rejected_rather_than_clamped() {
    let (app, _container, _connection_string) = test_app().await;

    let response = get(&app, "/tables?limit=1001").await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(
        body["type"],
        "https://graph-owl.dev/errors/validation-failed"
    );
    // Clamping would hand back 1000 of a requested 1001 and let the client
    // believe it had read everything.
    assert_eq!(body["errors"][0]["field"], "limit");
}

#[tokio::test]
async fn the_maximum_limit_itself_is_accepted() {
    let (app, _container, _connection_string) = test_app().await;

    // The boundary is inclusive; rejecting 1000 as well would be off by one.
    assert_eq!(
        get(&app, "/tables?limit=1000").await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_handcrafted_cursor_is_a_400_never_a_500_or_a_panic() {
    let (app, _container, _connection_string) = test_app().await;
    seed(&app, 3).await;

    for bogus in [
        "not-base64!!!",
        "YWJj",                     // valid base64, wrong shape
        "",                         // empty
        "%00%00",                   // control characters
        "aaaaaaaaaaaaaaaaaaaaaaaa", // right alphabet, wrong content
    ] {
        let response = get(&app, &format!("/tables?after={bogus}")).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "cursor {bogus:?} must be a client error"
        );
        let body = json_body(response).await;
        assert_eq!(body["errors"][0]["field"], "after", "cursor {bogus:?}");
    }
}

#[tokio::test]
async fn a_zero_limit_is_rejected() {
    let (app, _container, _connection_string) = test_app().await;

    let response = get(&app, "/tables?limit=0").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
