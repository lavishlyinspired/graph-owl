//! `POST /ontology-editor/{preview,dry-run,save}` — Epic 42 Slice G.
//!
//! Parse/validate/save logic itself is already proven exhaustively at the
//! `graph-owl-api` level (`ontology_editor_tests`, no Docker needed). What
//! is left to prove here is the HTTP plumbing: each route is admin-gated
//! the same way `/policies/dry-run` and `/ontology-packs` already are, the
//! request/response wire shapes round-trip, and a save is really durable —
//! a second dry run against the same document must not report it as
//! `skipped`.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{authorization_fixture as fixture, json_body, token};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post(app: &axum::Router, uri: &str, subject: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token(subject)))
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

const VALID_TURTLE: &str = "@prefix ex: <https://graph-owl.dev/ns/catalog#> .\n\
     ex:Widget ex:name \"A widget\" .\n";

const MALFORMED_TURTLE: &str = "@prefix ex: <https://graph-owl.dev/ns/catalog#> .\n\
     ex:Widget ex:name \"unterminated\n";

#[tokio::test]
async fn preview_parses_a_valid_document_and_names_its_declared_subject() {
    let (app, _container, _catalog) = fixture().await;

    let (status, body) = post(
        &app,
        "/ontology-editor/preview",
        "root",
        json!({ "format": "turtle", "document": VALID_TURTLE }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "preview", "{body}");
    assert!(
        body["declared"].as_array().is_some_and(|d| d
            .iter()
            .any(|v| v.as_str() == Some("https://graph-owl.dev/ns/catalog#Widget"))),
        "{body}"
    );
}

#[tokio::test]
async fn preview_reports_a_syntax_error_with_its_own_line() {
    let (app, _container, _catalog) = fixture().await;

    let (status, body) = post(
        &app,
        "/ontology-editor/preview",
        "root",
        json!({ "format": "turtle", "document": MALFORMED_TURTLE }),
    )
    .await;

    // Always 200 — a bad document is a normal outcome, not a system
    // failure, matching `RdfEditDryRun`/`RdfEditSave`'s own reasoning.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "syntaxError", "{body}");
    assert_eq!(body["line"], 2, "{body}");
}

#[tokio::test]
async fn dry_run_checks_a_document_without_writing_it() {
    let (app, _container, _catalog) = fixture().await;

    let (status, body) = post(
        &app,
        "/ontology-editor/dry-run",
        "root",
        json!({ "format": "turtle", "document": VALID_TURTLE }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "checked", "{body}");
    assert!(
        body["accepted"].as_array().is_some_and(|a| !a.is_empty()),
        "{body}"
    );
}

#[tokio::test]
async fn save_writes_the_document_and_a_later_dry_run_still_reports_accepted() {
    let (app, _container, _catalog) = fixture().await;

    let (save_status, save_body) = post(
        &app,
        "/ontology-editor/save",
        "root",
        json!({ "format": "turtle", "document": VALID_TURTLE }),
    )
    .await;
    assert_eq!(save_status, StatusCode::OK, "{save_body}");
    assert_eq!(save_body["kind"], "saved", "{save_body}");
    assert!(
        save_body["landed"]
            .as_array()
            .is_some_and(|l| !l.is_empty()),
        "{save_body}"
    );

    // The RED test's real point: `import_rdf`'s own dedup would report
    // this as `skipped` on a second pass. A dry run of the same document
    // right after saving it must still say it would be accepted.
    let (status, body) = post(
        &app,
        "/ontology-editor/dry-run",
        "root",
        json!({ "format": "turtle", "document": VALID_TURTLE }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["accepted"].as_array().is_some_and(|a| !a.is_empty()),
        "a second check of the same saved document must still say accepted: {body}"
    );
}

#[tokio::test]
async fn all_three_routes_are_refused_to_a_non_admin() {
    let (app, _container, _catalog) = fixture().await;
    let payload = json!({ "format": "turtle", "document": VALID_TURTLE });

    for route in [
        "/ontology-editor/preview",
        "/ontology-editor/dry-run",
        "/ontology-editor/save",
    ] {
        let (status, body) = post(&app, route, "asha", payload.clone()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{route}: {body}");
    }
}

#[tokio::test]
async fn an_unrecognised_format_is_a_named_bad_request() {
    let (app, _container, _catalog) = fixture().await;

    let (status, body) = post(
        &app,
        "/ontology-editor/preview",
        "root",
        json!({ "format": "rdf-xml-typo", "document": VALID_TURTLE }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["errors"][0]["field"], "format", "{body}");
}
