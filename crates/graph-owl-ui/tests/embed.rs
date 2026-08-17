//! Plan 122a A0: the dual embed (`ui/dist` at `/`, `graphowl-app/dist` under
//! `/next`) is new behaviour with no prior test coverage in this crate —
//! written before it shipped, not after, per the project's TDD rule.
//!
//! What matters here is not the literal bytes (those come from whatever is
//! currently built into each `dist/`) but the *routing contract*: each
//! embed serves its own assets with the right content type and immutable
//! caching, falls back to its own `index.html` for unknown paths so the
//! client-side router can resolve them, and a path outside `/next` must
//! never be answered by `router_next()`.
//!
//! `router_next()` is `.merge()`d here, exactly as `graph-owl-server`
//! merges it — an earlier version of this router used `.route("/", ...)`
//! plus a wildcard under `.nest("/next", ...)`, which every test below
//! passed while the *live* server still served `ui/dist` at `GET /next/`.
//! `nest()`'s path rewriting maps an inner `"/"` route to the bare prefix
//! only (no trailing slash) with no inner path able to express the
//! trailing-slash form, and `{*path}` never matches an empty remainder —
//! so `/next/` matched neither registered route and fell through to the
//! outer fallback. `router_next()` now owns `/next`, `/next/` and
//! `/next/{*path}` as plain, fully-prefixed routes and is merged rather
//! than nested; `bare_next_root_with_trailing_slash_serves_graphowl_app`
//! below is the regression test for exactly that gap.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

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
async fn ui_router_serves_a_real_static_asset_with_immutable_caching() {
    let files = dist_files(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/dist/static"));
    let asset = files
        .iter()
        .find(|f| has_js_extension(f))
        .expect("ui/dist/static has at least one .js file — run `npm run build` in ui/ first");

    let app = graph_owl_ui::router();
    let response = app
        .oneshot(
            Request::get(format!("/static/{asset}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let cache_control = response
        .headers()
        .get(axum::http::header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cache_control.contains("immutable"), "got {cache_control:?}");
}

#[tokio::test]
async fn ui_router_falls_back_to_index_html_for_an_unknown_client_side_route() {
    let app = graph_owl_ui::router();
    let response = app
        .oneshot(
            Request::get("/explore/some/deep/path")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("text/html"),
        "got {content_type:?}"
    );
}

#[tokio::test]
async fn next_router_serves_graphowl_app_assets_when_nested_under_next() {
    let files = dist_files(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../graphowl-app/dist/static"
    ));
    let asset = files.iter().find(|f| has_js_extension(f)).expect(
        "graphowl-app/dist/static has at least one .js file — \
         run `VITE_BASE=/next/ npm run build` in graphowl-app/ first",
    );

    // Merged exactly as `graph-owl-server` merges it. `router_next()`'s
    // routes already carry the `/next` prefix, so there is no automatic
    // prefix-stripping to rely on here — `serve_next` strips it itself,
    // which is the behaviour the embed build's `base: "/next/"` asset URLs
    // depend on.
    let app = axum::Router::new().merge(graph_owl_ui::router_next());

    let response = app
        .oneshot(
            Request::get(format!("/next/static/{asset}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn next_router_falls_back_to_its_own_index_html_not_the_ui_ones() {
    let app = axum::Router::new().merge(graph_owl_ui::router_next());

    let response = app
        .oneshot(Request::get("/next/overview").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    // The temporary embed build sets `base: "/next/"`, so its own
    // `index.html` references `/next/static/...` — the ui/ index.html
    // (built with the default `/` base) never does. Asserting on this is
    // the negative test: a mixed-up embed would still return *some* HTML,
    // which is why "got a 200" alone would not have caught it.
    assert!(
        html.contains("/next/static/"),
        "expected graphowl-app's own index.html (base=/next/), got: {html}"
    );
}

/// **Regression test.** `GET /next/` — trailing slash, the URL a browser
/// actually requests for the console root — is the exact request that
/// served `ui/dist` instead of `graphowl-app/dist` on the live server while
/// this same assertion, aimed at `/next/overview` instead, passed. See this
/// file's module doc comment for the `nest()` mechanics that produced the
/// gap.
#[tokio::test]
async fn bare_next_root_with_trailing_slash_serves_graphowl_app() {
    let app = axum::Router::new().merge(graph_owl_ui::router_next());

    let response = app
        .oneshot(Request::get("/next/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("/next/static/"),
        "expected graphowl-app's own index.html (base=/next/), got: {html}"
    );
}

/// Companion to the trailing-slash case: `/next` with no slash at all must
/// resolve identically, not just the two forms this crate happens to test.
#[tokio::test]
async fn bare_next_root_without_trailing_slash_serves_graphowl_app() {
    let app = axum::Router::new().merge(graph_owl_ui::router_next());

    let response = app
        .oneshot(Request::get("/next").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("/next/static/"),
        "expected graphowl-app's own index.html (base=/next/), got: {html}"
    );
}

#[tokio::test]
async fn a_path_outside_next_is_not_answered_by_the_next_sub_router() {
    let app = axum::Router::new().merge(graph_owl_ui::router_next());

    let response = app
        .oneshot(Request::get("/overview").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
