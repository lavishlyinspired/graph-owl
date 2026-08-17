//! The dual embed (Plan 122a A0) is wired up inside `graph_owl_ui`, whose
//! own `tests/embed.rs` proves each router is correct in isolation —
//! `router_next()` nested under a bare `Router::new()`. That isolation is
//! exactly what a real routing/merge-order bug could hide behind: this
//! crate's `app()` merges API routes, the Swagger UI router, `/next`
//! (nested) and `router()` (merged, with its own fallback) in one long
//! chain, and axum's fallback semantics depend on the *order* those pieces
//! combine. This test exercises the actual composed `app()`, the thing that
//! ships, not a stand-in for it.

mod common;

use common::test_app;

fn dist_files(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {dir}: {e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

fn has_js_extension(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
}

#[tokio::test]
async fn next_serves_the_graphowl_app_bundle_not_the_ui_one_in_the_real_composed_app() {
    let files = dist_files(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../graphowl-app/dist/static"
    ));
    let asset = files.iter().find(|f| has_js_extension(f)).expect(
        "graphowl-app/dist/static has at least one .js file — \
         run `VITE_BASE=/next/ npm run build` in graphowl-app/ first",
    );

    let (app, _db, _url) = test_app().await;

    let response = <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri(format!("/next/static/{asset}"))
            .body(axum::body::Body::empty())
            .expect("request should build"),
    )
    .await
    .expect("request should be handled");

    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "/next/static/{asset} should be served by the graphowl-app embed"
    );
}

/// **Regression test.** `GET /next/` (trailing slash — what a browser
/// actually requests) served `ui/dist`'s `index.html` on the live server
/// even after the routes above passed, because `graph-owl-server` used
/// `.nest("/next", ...)` and axum's `nest()` cannot map any inner path to
/// the outer prefix *with* a trailing slash. Fixed by having
/// `graph_owl_ui::router_next()` own `/next`, `/next/` and `/next/{*path}`
/// as plain routes and `.merge()`-ing it instead of nesting it.
#[tokio::test]
async fn next_root_with_trailing_slash_serves_graphowl_app_in_the_real_composed_app() {
    let (app, _db, _url) = test_app().await;

    let response = <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/next/")
            .body(axum::body::Body::empty())
            .expect("request should build"),
    )
    .await
    .expect("request should be handled");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("/next/static/"),
        "expected graphowl-app's own index.html (base=/next/) at the bare /next/ root, got: {html}"
    );
}

#[tokio::test]
async fn next_falls_back_to_graphowl_apps_own_index_html_in_the_real_composed_app() {
    let (app, _db, _url) = test_app().await;

    let response = <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/next/overview")
            .body(axum::body::Body::empty())
            .expect("request should build"),
    )
    .await
    .expect("request should be handled");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let html = String::from_utf8_lossy(&body);
    // The negative test: a router that shadowed `/next` with the `ui/`
    // fallback would still return 200 text/html, just the wrong bundle. Only
    // graphowl-app's build (base: "/next/") references `/next/static/...`
    // from its own `index.html`.
    assert!(
        html.contains("/next/static/"),
        "expected graphowl-app's own index.html (base=/next/), got: {html}"
    );
}

#[tokio::test]
async fn root_still_serves_the_ui_bundle_unaffected_by_the_next_mount() {
    let (app, _db, _url) = test_app().await;

    let response = <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/explore/some/deep/path")
            .body(axum::body::Body::empty())
            .expect("request should build"),
    )
    .await
    .expect("request should be handled");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let html = String::from_utf8_lossy(&body);
    assert!(
        !html.contains("/next/static/"),
        "root's fallback must be ui/'s own index.html, not graphowl-app's: got {html}"
    );
}
