//! Epic 9a's export-authorization gap, closed and put on the wire:
//! `GET /graph/export/{format}` for `GraphML`, bulk CSV, Cypher script,
//! JSON Lines and JSON Graph.
//!
//! Authorization scoping itself (the query→filter→convert logic behind
//! every format) is already proven exhaustively at the `graph-owl-api`
//! level (`export_authorization_tests`, no Docker needed). What is left to
//! prove here is the HTTP plumbing: each route exists, is wired to the
//! right `Catalog::export_*` method, is not admin-gated, and — for one
//! representative format — that the authorization those unit tests already
//! proved actually reaches a real HTTP response body.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{authorization_fixture, token};
use tower::ServiceExt;

async fn get_as(app: &axum::Router, uri: &str, subject: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
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
        .expect("body")
        .to_vec();
    (status, bytes)
}

/// **The representative format.** `authorization_fixture` seeds a real bank
/// estate cataloged through the Postgres connector — `core_banking` (denied
/// to `asha`) and `payments` (allowed) — the same fixture Bolt's own
/// cross-surface equivalence test uses, so this is the identical
/// authorization decision already proven at the `graph-owl-api` level,
/// checked one more time end to end over real HTTP.
#[tokio::test]
async fn graphml_export_over_http_omits_what_the_principal_may_not_see() {
    let (app, _container, _catalog) = authorization_fixture().await;

    let (status, bytes) = get_as(&app, "/graph/export/graphml", "asha").await;
    assert_eq!(status, StatusCode::OK);
    let body = String::from_utf8(bytes).expect("GraphML is UTF-8 XML");
    assert!(
        body.contains("payments"),
        "an allowed schema must appear: {body}"
    );
    assert!(
        !body.contains("core_banking"),
        "a denied schema must never reach the response body: {body}"
    );

    let (admin_status, admin_bytes) = get_as(&app, "/graph/export/graphml", "root").await;
    assert_eq!(admin_status, StatusCode::OK);
    let admin_body = String::from_utf8(admin_bytes).expect("GraphML is UTF-8 XML");
    assert!(
        admin_body.contains("core_banking"),
        "an admin's own export is not scoped by the same restriction: {admin_body}"
    );
}

#[tokio::test]
async fn graphml_export_is_served_as_xml_with_a_filename() {
    let (app, _container, _catalog) = authorization_fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/graph/export/graphml")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("handled");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/xml")
    );
    assert!(
        response
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("graph.graphml")),
        "{:?}",
        response.headers()
    );
}

#[tokio::test]
async fn cypher_script_export_names_the_authorized_table_only() {
    let (app, _container, _catalog) = authorization_fixture().await;
    let (status, bytes) = get_as(&app, "/graph/export/cypher", "asha").await;
    assert_eq!(status, StatusCode::OK);
    let script = String::from_utf8(bytes).expect("Cypher script is UTF-8 text");
    assert!(script.contains("payments"), "{script}");
    assert!(!script.contains("core_banking"), "{script}");
}

#[tokio::test]
async fn json_lines_export_carries_only_the_authorized_element() {
    let (app, _container, _catalog) = authorization_fixture().await;
    let (status, bytes) = get_as(&app, "/graph/export/jsonl", "asha").await;
    assert_eq!(status, StatusCode::OK);
    let lines = String::from_utf8(bytes).expect("JSON Lines is UTF-8 text");
    assert!(lines.contains("payments"), "{lines}");
    assert!(!lines.contains("core_banking"), "{lines}");
    for line in lines.lines() {
        let _: serde_json::Value = serde_json::from_str(line).expect("each line is valid JSON");
    }
}

#[tokio::test]
async fn json_graph_export_returns_only_the_authorized_node_in_the_epic_40_shape() {
    let (app, _container, _catalog) = authorization_fixture().await;
    let (status, bytes) = get_as(&app, "/graph/export/json-graph", "asha").await;
    assert_eq!(status, StatusCode::OK);
    let view: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");

    let fqns: Vec<String> = view["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .filter_map(|n| n["fullyQualifiedName"].as_str())
        .map(str::to_string)
        .collect();
    assert!(fqns.iter().any(|fqn| fqn.contains("payments")), "{fqns:?}");
    assert!(
        !fqns.iter().any(|fqn| fqn.contains("core_banking")),
        "{fqns:?}"
    );
    assert!(view["truncated"].is_boolean(), "{view}");
}

/// Bulk CSV is bundled as `.tar.zst` for one HTTP response — unpacked and
/// read back through the exact `zstd`/`tar` pairing the server used to
/// build it, proving the bytes on the wire are a real, readable archive
/// rather than only that the handler returned `200`.
#[tokio::test]
async fn bulk_csv_export_is_a_readable_tar_zst_with_only_the_authorized_label() {
    let (app, _container, _catalog) = authorization_fixture().await;
    let (status, bytes) = get_as(&app, "/graph/export/bulk-csv", "asha").await;
    assert_eq!(status, StatusCode::OK);

    let decoder = zstd::Decoder::new(std::io::Cursor::new(bytes)).expect("zstd decoder");
    let mut archive = tar::Archive::new(decoder);
    let mut names = Vec::new();
    let mut all_csv = String::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        names.push(entry.path().expect("path").to_string_lossy().into_owned());
        std::io::Read::read_to_string(&mut entry, &mut all_csv).ok();
    }
    assert!(names.iter().any(|n| n.contains("nodes-")), "{names:?}");
    assert!(all_csv.contains("payments"), "{all_csv}");
    assert!(!all_csv.contains("core_banking"), "{all_csv}");
}

/// None of the five routes are admin-gated — the calling principal's own
/// [`graph_owl_authz::AccessPredicate`] already scopes the result, the same
/// reasoning `/cypher` and `/sparql` rely on. A principal denied everything
/// still gets `200` with an empty export, not a `404`/`403` the way the
/// genuinely admin-only `/admin/*` surfaces respond to a non-admin.
#[tokio::test]
async fn a_principal_with_no_policy_at_all_gets_200_and_an_empty_export_not_a_refusal() {
    let (app, _container, _catalog) = authorization_fixture().await;
    let (status, bytes) = get_as(&app, "/graph/export/json-graph", "nobody-at-all").await;
    assert_eq!(status, StatusCode::OK);
    let view: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
    assert_eq!(view["nodes"].as_array().expect("nodes").len(), 0, "{view}");
}
