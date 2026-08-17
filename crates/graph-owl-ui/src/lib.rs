//! Embedded web console: serves the built single-page application from the
//! server binary.
//!
//! **Status**: Epic 39 Slice A; Plan 122a A0 added a second, temporary embed.
//! One binary, one process, no CDN, no reverse proxy — `00f-ui-architecture.md`.
//! Frontend sources live in `ui/`; this crate only embeds and serves the
//! build output.
//!
//! **Plan 122a A0–A11**: `graphowl-app/` is the console rebuild that replaces
//! `ui/`. During the migration both are embedded — `ui/dist` still serves `/`
//! (`router()`), `graphowl-app/dist` serves under `/next` (`router_next()`,
//! `.merge()`d — see its doc comment for why not `.nest()`) — so the working
//! console never breaks mid-rebuild. A11 removes `router()` and `Assets`
//! (the `ui/` embed) entirely and promotes `router_next()`'s content to
//! serve `/`.

use axum::{
    Router,
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../ui/dist"]
struct Assets;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../graphowl-app/dist"]
struct NextAssets;

/// Serves the console.
///
/// **Mount this *after* the API routes.** The SPA fallback must not swallow an
/// unknown API path: a fallback registered first turns every mistyped endpoint
/// into a `200 text/html`, the generated client parses HTML as JSON, and the
/// user sees a blank page instead of an error (`39-ui-foundation.md` Slice A).
pub fn router() -> Router {
    Router::new().fallback(get(serve))
}

/// Serves the in-progress `graphowl-app/` rebuild under `/next`
/// (Plan 122a A0). Temporary — removed at A11 once `graphowl-app` replaces
/// `ui/` as the sole embed.
///
/// **`.merge()` this at the call site, never `.nest("/next", ...)`.** Routes
/// are registered with the `/next` prefix already baked in — three of them,
/// deliberately, because `axum::routing::path_router::path_for_nested_route`
/// (traced directly against axum 0.8.9's source while chasing a live bug
/// this shape produces) maps a nested router's own `"/"` route to the outer
/// prefix *without* a trailing slash, and there is no inner path that maps
/// to the prefix *with* one — `path == "/"` is special-cased to `prefix`
/// alone before the general `format!("{prefix}{path}")` case ever runs. A
/// catch-all wildcard doesn't fill the gap either: matchit's `{*path}`
/// requires at least one captured character, so it never matches a bare
/// trailing slash. The result, confirmed live: `GET /next` and
/// `GET /next/overview` served `graphowl-app/dist` correctly, while
/// `GET /next/` — the URL a browser actually requests for the console root
/// — silently fell through to the outer router's fallback and served
/// `ui/dist` instead. `.merge()` sidesteps `nest()`'s path rewriting
/// entirely, so all three of `/next`, `/next/` and `/next/{*path}` are
/// ordinary routes this router owns outright.
pub fn router_next() -> Router {
    Router::new()
        .route("/next", get(serve_next))
        .route("/next/", get(serve_next))
        .route("/next/{*path}", get(serve_next))
}

// `axum::routing::get` requires a `Handler`, which is only implemented for
// functions returning a `Future` — so these stay `async fn` even though
// neither has an `.await` of its own; the work happens synchronously in
// `serve_from`, which is a plain function for exactly that reason.
async fn serve(uri: Uri) -> Response {
    serve_from::<Assets>(uri.path().trim_start_matches('/'), "console")
}

async fn serve_next(uri: Uri) -> Response {
    // `router_next()` is `.merge()`d, not nested, so axum never strips a
    // prefix for us — strip `/next` ourselves before looking the path up in
    // `NextAssets`, whose embedded paths are relative to `graphowl-app/dist`.
    let path = uri.path().strip_prefix("/next").unwrap_or(uri.path());
    serve_from::<NextAssets>(path.trim_start_matches('/'), "graphowl-app")
}

fn serve_from<A: RustEmbed>(path: &str, label: &'static str) -> Response {
    if let Some(asset) = A::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        // Content-hashed filenames, so the bytes at a URL never change.
        return (
            [
                (header::CONTENT_TYPE, mime.as_ref()),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            asset.data.into_owned(),
        )
            .into_response();
    }

    // Client-side route: hand back the shell so the router resolves it.
    match A::get("index.html") {
        Some(index) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            index.data.into_owned(),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Body::from(format!("{label} assets are not compiled into this binary")),
        )
            .into_response(),
    }
}
