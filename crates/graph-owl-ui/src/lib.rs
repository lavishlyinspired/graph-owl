//! Embedded web console: serves the built single-page application from the
//! server binary.
//!
//! **Status**: Epic 39 Slice A. One binary, one process, no CDN, no reverse
//! proxy — `00f-ui-architecture.md`. Frontend sources live in `ui/`; this crate
//! only embeds and serves the build output.

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

/// Serves the console.
///
/// **Mount this *after* the API routes.** The SPA fallback must not swallow an
/// unknown API path: a fallback registered first turns every mistyped endpoint
/// into a `200 text/html`, the generated client parses HTML as JSON, and the
/// user sees a blank page instead of an error (`39-ui-foundation.md` Slice A).
pub fn router() -> Router {
    Router::new().fallback(get(serve))
}

async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(asset) = Assets::get(path) {
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
    match Assets::get("index.html") {
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
            Body::from("console assets are not compiled into this binary"),
        )
            .into_response(),
    }
}
