//! Plan 122a A11: the single-embed console. `graphowl-app/dist` serves `/`
//! directly — the A0–A10 dual embed (`ui/dist` at `/`, `graphowl-app/dist`
//! under `/next`) is gone; see `_archived/README.md`'s `ui/` entry.
//!
//! What matters here is not the literal bytes (those come from whatever is
//! currently built into `dist/`) but the routing contract: real static
//! assets get the right content type and immutable caching, and an unknown
//! client-side path falls back to `index.html` so the SPA router can
//! resolve it.

use axum::body::Body;
use axum::http::{Request, StatusCode};
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
async fn console_router_serves_a_real_static_asset_with_immutable_caching() {
    let files = dist_files(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../graphowl-app/dist/static"
    ));
    let asset = files.iter().find(|f| has_js_extension(f)).expect(
        "graphowl-app/dist/static has at least one .js file — run `npm run build` in graphowl-app/ first",
    );

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
async fn console_router_falls_back_to_index_html_for_an_unknown_client_side_route() {
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
async fn bare_root_with_trailing_slash_serves_the_console() {
    // The exact shape of the bug this crate's history already found once
    // (`_archived/README.md`'s `ui/` entry, and the CLAUDE.md gotcha on
    // `nest()`'s trailing-slash gap) — asserted directly here now that `/`
    // is the only mount, not inferred from a `/next/` regression test that
    // no longer exists.
    let app = graph_owl_ui::router();
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
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

/// **RED requirement, Plan 122a A11**: "a structural test asserts `build.rs`
/// watches `graphowl-app/dist` and that no path resolves into `ui/`." Reads
/// `build.rs`'s own source rather than trusting intent — the same
/// discipline `ui/`'s `vocabularyStructure.test.ts` used, generalised to
/// this crate's own removal of the dual embed.
#[test]
fn build_rs_watches_graphowl_app_dist_and_never_ui() {
    let build_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("build.rs should exist");

    // Only the executable `cargo:rerun-if-changed` lines matter here — a
    // doc comment is free to say "ui/" while narrating the A0-A11
    // migration (as this file's own does), and a check that flagged prose
    // would be a check nobody could satisfy honestly.
    let watch_lines: Vec<&str> = build_rs
        .lines()
        .filter(|line| {
            line.trim_start()
                .starts_with("println!(\"cargo:rerun-if-changed=")
        })
        .collect();

    assert!(
        watch_lines
            .iter()
            .any(|line| line.contains("graphowl-app/dist")),
        "build.rs must watch graphowl-app/dist so a rebuilt frontend is not silently ignored: {watch_lines:?}"
    );
    assert!(
        !watch_lines
            .iter()
            .any(|line| line.contains("../ui/") || line.contains("/ui/dist")),
        "build.rs must not watch the archived ui/ embed: {watch_lines:?}"
    );
}

/// Companion to the `build.rs` check: `lib.rs` itself must not reference
/// the archived `ui/` tree either — a path that resolved into it would
/// fail to compile today (nothing is there any more), but this is the
/// assertion that keeps it that way on purpose, not by accident.
#[test]
fn lib_rs_never_resolves_into_ui() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("src/lib.rs should exist");

    assert!(
        !lib_rs.contains("/../../ui/") && !lib_rs.contains("\"ui/dist\""),
        "src/lib.rs must not embed anything from the archived ui/ tree: {lib_rs}"
    );
}
