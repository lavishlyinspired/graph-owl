//! `GET /packs/available` and `POST /packs/{pack}/install` — installing a
//! domain pack from the console instead of only the `graph-owl-load-pack`
//! CLI. See `graph_owl_server::pack_install`'s own doc comment for why
//! installation shells out to that CLI rather than re-deriving
//! `pack.toml`'s grammar in Rust a second time.
//!
//! **Every test here points `GRAPH_OWL_PACKS_DIR` at this repo's real
//! `packs/` directory** (computed from `CARGO_MANIFEST_DIR`, not a temp
//! fixture) — the two real packs on disk, `gst` and `hospitality`, are
//! exactly the fixture. All tests in this file use the identical value, so
//! setting it is safe under `cargo test`'s default per-file concurrency:
//! two tests racing to set the *same* value never disagree about what it
//! ends up being.
//!
//! `pack_install.rs`'s own unit tests already cover `scan_available_packs`'s
//! parsing/skip-on-error behaviour against synthetic temp directories —
//! this file proves the HTTP surface: admin-gating, path-traversal
//! rejection, and (the one test genuinely dependent on the environment
//! this was developed in) that `run_pack_loader` really can find and
//! execute `connectors/python/.venv`'s installed `graph-owl-load-pack`,
//! the same way `scripts/demo.sh` already does by hand.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{authorization_fixture, test_app, token};
use tower::ServiceExt;

fn real_packs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn set_packs_dir_to_the_real_one() {
    // SAFETY (in the Rust sense the 2024-edition-required `unsafe` block
    // asks for, not a memory-safety claim): every test in this file writes
    // the identical value, so a concurrent write from another test in this
    // same process can only ever race to the same outcome.
    unsafe {
        std::env::set_var("GRAPH_OWL_PACKS_DIR", real_packs_dir());
    }
}

/// `loader_binary()`'s own CWD-relative default
/// (`connectors/python/.venv/bin/graph-owl-load-pack`) is correct for how
/// this server is actually launched in this project (`scripts/demo.sh` and
/// every manual restart run from the repo root) — but `cargo test`'s CWD is
/// this crate's own directory, not the repo root, so the same relative
/// path resolves to nothing here. The override exists for exactly this
/// kind of CWD mismatch; tests use it rather than the production default
/// needing to guess at a repo root it should not know about.
fn set_loader_bin_to_the_real_one() {
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../connectors/python/.venv/bin/graph-owl-load-pack");
    // SAFETY: see `set_packs_dir_to_the_real_one` above — identical value
    // from every caller.
    unsafe {
        std::env::set_var("GRAPH_OWL_LOAD_PACK_BIN", bin);
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    subject: &str,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {}", token(subject)))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn available_packs_lists_both_real_packs_when_neither_is_installed() {
    set_packs_dir_to_the_real_one();
    let (app, _db, _url) = test_app().await;

    let (status, body) = call(&app, "GET", "/packs/available", "system").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: HashSet<String> = body
        .as_array()
        .expect("array")
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains("gst"), "{ids:?}");
    assert!(ids.contains("hospitality"), "{ids:?}");
}

#[tokio::test]
async fn a_pack_disappears_from_available_once_its_namespace_is_declared() {
    set_packs_dir_to_the_real_one();
    let (app, _db, _url) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/namespaces")
                .header("authorization", format!("Bearer {}", token("system")))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"iri": "https://graph-owl.dev/packs/gst#", "declaredBy": "pack:gst"})
                        .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::OK);

    let (status, body) = call(&app, "GET", "/packs/available", "system").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: HashSet<String> = body
        .as_array()
        .expect("array")
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !ids.contains("gst"),
        "gst is now installed, must not be listed as available: {ids:?}"
    );
    assert!(
        ids.contains("hospitality"),
        "hospitality is untouched: {ids:?}"
    );
}

#[tokio::test]
async fn available_packs_is_admin_gated() {
    set_packs_dir_to_the_real_one();
    let (app, _db, _catalog) = authorization_fixture().await;

    let (status, _) = call(&app, "GET", "/packs/available", "asha").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "refused as not-found rather than forbidden, matching every other admin route"
    );
}

#[tokio::test]
async fn install_is_admin_gated() {
    set_packs_dir_to_the_real_one();
    let (app, _db, _catalog) = authorization_fixture().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/gst/install")
                .header("authorization", format!("Bearer {}", token("asha")))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn installing_an_unknown_pack_id_is_not_found() {
    set_packs_dir_to_the_real_one();
    let (app, _db, _url) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/no-such-pack-anywhere/install")
                .header("authorization", format!("Bearer {}", token("system")))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// The one input this route must never trust literally — a pack id that
/// looks like a path traversal attempt must be refused before it is ever
/// joined onto a filesystem path, not merely fail later because no such
/// directory happens to exist.
#[tokio::test]
async fn a_path_traversal_pack_id_is_refused() {
    set_packs_dir_to_the_real_one();
    let (app, _db, _url) = test_app().await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/packs/..%2F..%2F..%2Fetc/install")
                .header("authorization", format!("Bearer {}", token("system")))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// **Depends on the real environment this project develops in**: the
/// `graph-owl-packs` Python package installed into
/// `connectors/python/.venv` (`cd connectors/python && pip install -e
/// ".[dev]"`), the same way every manual pack install in this project has
/// run so far. Proves `run_pack_loader` genuinely finds and executes that
/// binary with the right arguments — not that a full install round-trips,
/// which would need this test harness to bind a real listening port
/// (`test_app`'s `Router` never does; every other test in this crate talks
/// to it in-process via `oneshot`, with no real socket for a subprocess to
/// dial back into). An unreachable port proves the spawn, argument-passing,
/// and output-capture machinery all work; the loader's own HTTP behaviour
/// once it *can* reach a server is `connectors/python/tests/`' concern.
#[tokio::test]
async fn run_pack_loader_spawns_the_real_loader_and_captures_its_output() {
    set_loader_bin_to_the_real_one();
    let pack_dir = real_packs_dir().join("gst");
    let outcome = graph_owl_server::pack_install::run_pack_loader(
        &pack_dir,
        "http://127.0.0.1:1", // refused immediately; no server needs to be listening
        "irrelevant-in-this-test",
    )
    .await
    .expect("the loader binary must be found and started");

    assert!(!outcome.ok, "port 1 refuses every connection: {outcome:?}");
    assert!(
        !outcome.output.is_empty(),
        "the loader's own stdout/stderr must be captured, not discarded"
    );
}
