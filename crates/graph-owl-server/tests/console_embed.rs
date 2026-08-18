//! Plan 122a A11: `graphowl-app` is the sole embedded console, serving `/`
//! directly through the real, composed `app()` — replacing this crate's
//! `next_embed.rs`, which tested the A0–A10 dual embed (`ui/dist` at `/`,
//! `graphowl-app/dist` under `/next`) now that it no longer exists.
//!
//! `graph_owl_ui`'s own `tests/embed.rs` proves the router is correct in
//! isolation. This exercises the actual composed `app()` — API routes, the
//! Swagger UI router and the console fallback merged in one chain — the
//! thing that ships, not a stand-in for it.

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
async fn root_serves_the_graphowl_app_bundle_in_the_real_composed_app() {
    let files = dist_files(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../graphowl-app/dist/static"
    ));
    let asset = files.iter().find(|f| has_js_extension(f)).expect(
        "graphowl-app/dist/static has at least one .js file — run `npm run build` in graphowl-app/ first",
    );

    let (app, _db, _url) = test_app().await;

    let response =
        <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
            app,
            axum::http::Request::builder()
                .method("GET")
                .uri(format!("/static/{asset}"))
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "/static/{asset} should be served by the graphowl-app embed"
    );
}

#[tokio::test]
async fn bare_root_with_trailing_slash_serves_the_console_in_the_real_composed_app() {
    let (app, _db, _url) = test_app().await;

    let response =
        <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
            app,
            axum::http::Request::builder()
                .method("GET")
                .uri("/")
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .expect("content-type header should be present")
        .to_str()
        .expect("content-type should be valid utf-8");
    assert!(content_type.starts_with("text/html"));
}

#[tokio::test]
async fn an_unknown_client_side_route_falls_back_to_the_console_in_the_real_composed_app() {
    let (app, _db, _url) = test_app().await;

    let response =
        <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
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
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .expect("content-type header should be present")
        .to_str()
        .expect("content-type should be valid utf-8");
    assert!(content_type.starts_with("text/html"));
}

/// **RED requirement, Plan 122a A11**: "all 24 routes reachable at `/`."
/// Reads `graphowl-app/src/lib/routes.ts` directly (the single source of
/// truth `router.tsx` and the route-budget test both already read from)
/// rather than hardcoding a duplicate list here that could drift from it.
#[tokio::test]
async fn every_route_in_graphowl_apps_own_route_list_is_reachable_at_root() {
    let routes_ts = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../graphowl-app/src/lib/routes.ts"
    ))
    .expect("graphowl-app/src/lib/routes.ts should exist");

    let list_start = routes_ts
        .find("export const ROUTES")
        .expect("ROUTES export should exist");
    let array_start = routes_ts[list_start..]
        .find('[')
        .expect("ROUTES should be an array literal");
    let array_end = routes_ts[list_start + array_start..]
        .find(']')
        .expect("ROUTES array should close");
    let array_body = &routes_ts[list_start + array_start + 1..list_start + array_start + array_end];

    let route_names: Vec<&str> = array_body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_end_matches(',');
            let unquoted = trimmed.strip_prefix('"')?.strip_suffix('"')?;
            Some(unquoted)
        })
        .collect();

    assert_eq!(
        route_names.len(),
        24,
        "expected 24 routes in graphowl-app's ROUTES, found {}: {route_names:?}",
        route_names.len()
    );

    let (app, _db, _url) = test_app().await;
    for route in route_names {
        let response =
            <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
                app.clone(),
                axum::http::Request::builder()
                    .method("GET")
                    .uri(format!("/{route}"))
                    .body(axum::body::Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "/{route} should be reachable at root"
        );
    }
}
