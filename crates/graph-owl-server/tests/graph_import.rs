//! `POST /graph/import/rdf` — the platform's P0, and the route every domain
//! pack is blocked on.
//!
//! `Catalog::import_rdf` has been built since Epic 9 Slice E — parsing, SHACL
//! validation before anything is written, per-subject transactionality, dedup
//! by subject in the source's own import graph, and a dry run. **It had no
//! callers at all**: the only import path that reached HTTP was the admin
//! `/ontology-editor/save`, which is for editing this catalog's own ontology,
//! not for landing a pack's data. So this is a routing slice over a finished
//! capability, which is exactly why it is small and exactly why nothing could
//! ship without it.
//!
//! What these prove is the HTTP plumbing and the decisions that only exist at
//! the route: which formats are accepted, that a bad `source` cannot forge a
//! graph name, that `dryRun` writes nothing, and that the route is
//! admin-gated — the facade method takes no principal, so if the route did
//! not check, nothing would.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{authorization_fixture, test_app, token};
use tower::ServiceExt;

const TURTLE: &str = r#"
@prefix dsc: <https://graph-owl.dev/ns/catalog#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .

dsc:thing-one rdf:type dsc:Asset ;
    dsc:name "Thing One" .
"#;

async fn import_as(
    app: &axum::Router,
    uri: &str,
    subject: &str,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", format!("Bearer {}", token(subject)))
                .header("content-type", "text/turtle")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn a_turtle_document_lands_and_reports_what_it_landed() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = import_as(
        &app,
        "/graph/import/rdf?source=pack-test&format=turtle",
        "system",
        TURTLE,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["landed"].as_array().map(Vec::len),
        Some(1),
        "one subject in the document, one landed: {body}"
    );
    assert!(
        body["landed"][0]
            .as_str()
            .expect("a subject")
            .contains("thing-one"),
        "the outcome names the subject rather than counting it: {body}"
    );
}

#[tokio::test]
async fn a_re_import_of_the_same_document_skips_rather_than_duplicates() {
    // The acceptance criterion `import_rdf`'s own doc comment names, proven
    // over HTTP rather than at the facade: a pack reloaded is a pack
    // unchanged, so a demo script that runs twice does not double its graph.
    let (app, _db, _url) = test_app().await;
    let uri = "/graph/import/rdf?source=pack-test&format=turtle";

    let (_, first) = import_as(&app, uri, "system", TURTLE).await;
    let (status, second) = import_as(&app, uri, "system", TURTLE).await;

    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(first["landed"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        second["landed"].as_array().map(Vec::len),
        Some(0),
        "nothing lands twice: {second}"
    );
    assert_eq!(
        second["skipped"].as_array().map(Vec::len),
        Some(1),
        "and the skip is reported, not silent: {second}"
    );
}

#[tokio::test]
async fn a_dry_run_reports_what_would_land_and_writes_nothing() {
    let (app, _db, _url) = test_app().await;

    let (status, dry) = import_as(
        &app,
        "/graph/import/rdf?source=pack-test&format=turtle&dryRun=true",
        "system",
        TURTLE,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["landed"].as_array().map(Vec::len), Some(1));

    // The negative that makes the dry run meaningful: a real import
    // afterwards must still land it. If the dry run had written, this would
    // come back skipped.
    let (_, real) = import_as(
        &app,
        "/graph/import/rdf?source=pack-test&format=turtle",
        "system",
        TURTLE,
    )
    .await;
    assert_eq!(
        real["landed"].as_array().map(Vec::len),
        Some(1),
        "the dry run must not have written: {real}"
    );
}

#[tokio::test]
async fn a_document_that_does_not_parse_is_a_400_naming_the_problem() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = import_as(
        &app,
        "/graph/import/rdf?source=pack-test&format=turtle",
        "system",
        "this is not turtle at all {{{",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn an_unsupported_format_is_a_400_listing_what_is_supported() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = import_as(
        &app,
        "/graph/import/rdf?source=pack-test&format=csv",
        "system",
        TURTLE,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let rendered = body.to_string();
    assert!(
        rendered.contains("turtle"),
        "an operator who guessed wrong needs the list, not just a refusal: {body}"
    );
}

#[tokio::test]
async fn every_documented_format_is_accepted() {
    // Each format is a separate `match` arm, and an arm that silently fell
    // through to the error would look identical to a typo in the caller's
    // query string.
    let (app, _db, _url) = test_app().await;

    for (format, document) in [
        ("turtle", TURTLE.to_string()),
        (
            "ntriples",
            "<https://graph-owl.dev/ns/catalog#nt-one> \
             <https://graph-owl.dev/ns/catalog#name> \"NT One\" .\n"
                .to_string(),
        ),
        (
            "nquads",
            "<https://graph-owl.dev/ns/catalog#nq-one> \
             <https://graph-owl.dev/ns/catalog#name> \"NQ One\" .\n"
                .to_string(),
        ),
    ] {
        let (status, body) = import_as(
            &app,
            &format!("/graph/import/rdf?source=fmt-{format}&format={format}"),
            "system",
            &document,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "format {format} refused: {body}");
    }
}

// ── the decisions that exist only at the route ──────────────────────────

#[tokio::test]
async fn a_non_admin_cannot_import() {
    // **`Catalog::import_rdf` takes no principal.** Every other write facade
    // method does, so if this route did not gate, an import would be the one
    // unauthenticated write in the system — and it writes directly to the
    // graph, bypassing the asset-level authorization every other path has.
    //
    // `authorization_fixture` rather than `test_app`, and the difference is
    // load-bearing: **`test_app` runs every caller as an admin**, so a first
    // version of this test asserting `404` for an invented subject got a
    // `200` and looked like a missing gate. `asha` is a real provisioned
    // non-admin, which is the same fixture and the same subject every other
    // admin-gate test in this crate uses (`archive.rs`, `bolt_status.rs`).
    let (app, _db, _catalog) = authorization_fixture().await;

    let (status, _) = import_as(
        &app,
        "/graph/import/rdf?source=pack-test&format=turtle",
        "asha",
        TURTLE,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "refused as not-found rather than forbidden, matching every other \
         admin route — a 403 confirms the route exists to someone probing"
    );
}

#[tokio::test]
async fn a_source_that_could_forge_a_graph_name_is_refused() {
    // The source becomes `graph:import:{source}` verbatim. A source
    // containing the separator could name *another* import's graph — or the
    // shapes graph — and land triples in it, which is a write to a graph the
    // caller did not name and `delete_import` would never clean up.
    let (app, _db, _url) = test_app().await;

    // Percent-encoded, because a raw space or `/` is rejected by the URI
    // builder itself — a first version of this test used them literally and
    // failed at `Request::builder()`, never reaching the server. Encoding
    // them is what actually delivers the dangerous value to the handler,
    // which is the only way to prove the handler refuses it.
    for (bad, encoded) in [
        ("empty", ""),
        ("with:colon", "with%3Acolon"),
        ("with space", "with%20space"),
        ("with/slash", "with%2Fslash"),
        ("graph:shapes", "graph%3Ashapes"),
    ] {
        let (status, body) = import_as(
            &app,
            &format!("/graph/import/rdf?source={encoded}&format=turtle"),
            "system",
            TURTLE,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "source `{bad}` must be refused: {body}"
        );
    }
}

#[tokio::test]
async fn two_sources_land_in_their_own_graphs() {
    // The property that makes `source` worth requiring: the same document
    // imported under two names is two imports, each independently
    // removable. If they shared a graph the second would be reported as
    // skipped.
    let (app, _db, _url) = test_app().await;

    let (_, first) = import_as(
        &app,
        "/graph/import/rdf?source=source-a&format=turtle",
        "system",
        TURTLE,
    )
    .await;
    let (_, second) = import_as(
        &app,
        "/graph/import/rdf?source=source-b&format=turtle",
        "system",
        TURTLE,
    )
    .await;

    assert_eq!(first["landed"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        second["landed"].as_array().map(Vec::len),
        Some(1),
        "a second source is not a re-import of the first: {second}"
    );
}
