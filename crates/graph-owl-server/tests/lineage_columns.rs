//! Epic 29 Slices D, E and F — column lineage, reconciliation, and survival.
//!
//! **The sharpest assertion here is that a manually curated edge survives a
//! connector run.** Source-blind replacement silently deletes lineage a human
//! knew and automation does not, and it does it every night without an error —
//! which is why the slice exists and why the test is written first.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request should build");

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should be handled");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, parsed)
}

/// A `service` → `database` → `schema` → `table`, plus named columns.
/// Column-level lineage needs real columns, so the fixture builds them.
async fn table_with_columns(
    app: &axum::Router,
    service: &str,
    columns: &[&str],
) -> (String, String, Vec<String>) {
    let mut parent: Option<String> = None;
    let mut table_id = String::new();
    let mut table_fqn = String::new();
    for (kind, name) in [
        ("service", service),
        ("database", "sales"),
        ("schema", "public"),
        ("table", "orders"),
    ] {
        let mut body = json!({ "kind": kind, "name": name });
        if let Some(parent_id) = &parent {
            body["parentId"] = json!(parent_id);
        }
        let (status, created) = send(app, "POST", "/assets", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        table_id = created["id"].as_str().expect("an id").to_string();
        table_fqn = created["fullyQualifiedName"]
            .as_str()
            .expect("an fqn")
            .to_string();
        parent = Some(table_id.clone());
    }

    let mut column_fqns = Vec::new();
    for column in columns {
        let (status, created) = send(
            app,
            "POST",
            "/assets",
            Some(json!({ "kind": "column", "name": column, "parentId": table_id })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        column_fqns.push(
            created["fullyQualifiedName"]
                .as_str()
                .expect("an fqn")
                .to_string(),
        );
    }
    (table_id, table_fqn, column_fqns)
}

async fn edge(app: &axum::Router, from: &str, to: &str, source: &str) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/lineage",
        Some(json!({
            "fromAssetId": from,
            "toAssetId": to,
            "relationship": "feeds",
            "source": source,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().expect("an id").to_string()
}

// ── Slice D: lineage reaches column level ───────────────────────────────────

/// **Many-to-one is the ordinary case, not an edge case.** A one-to-one model
/// breaks on the first concatenation anybody catalogues, and `first_name` +
/// `last_name` → `full_name` is the example every warehouse has.
#[tokio::test]
async fn a_many_to_one_column_mapping_round_trips() {
    let (app, _db, _url) = test_app().await;
    let (source_id, _, source_columns) =
        table_with_columns(&app, "raw-svc", &["first_name", "last_name"]).await;
    let (target_id, _, target_columns) = table_with_columns(&app, "mart-svc", &["full_name"]).await;
    let edge_id = edge(&app, &source_id, &target_id, "manual").await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/lineage/{edge_id}/columns"),
        Some(json!({
            "mappings": [
                {
                    "fromColumnFqn": source_columns[0],
                    "toColumnFqn": target_columns[0],
                    "expression": "concat(first_name, ' ', last_name)",
                },
                { "fromColumnFqn": source_columns[1], "toColumnFqn": target_columns[0] },
            ],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mappings"], 2, "{body}");

    let (_, listed) = send(&app, "GET", &format!("/lineage/{edge_id}/columns"), None).await;
    let mappings = listed.as_array().expect("an array");
    assert_eq!(mappings.len(), 2, "{listed}");
    assert!(
        mappings
            .iter()
            .all(|m| m["toColumnFqn"] == target_columns[0].as_str()),
        "both sources feed the one target: {listed}"
    );
    assert!(
        mappings.iter().any(|m| m["expression"].is_string()),
        "the expression is what makes the mapping checkable: {listed}"
    );
}

/// **Phase 3 item 3.8, sequenced after 3.3.** `lineage_column_mappings`
/// stores plain-TEXT FQNs with no foreign key — renaming the table underneath
/// a mapping used to leave it pointing at an FQN that no longer existed,
/// because 3.3's cascade only ever touched `assets.fully_qualified_name`.
/// This proves the rename reaches the mapping too, the same way it already
/// reaches every descendant asset.
#[tokio::test]
async fn renaming_the_source_table_updates_its_columns_mappings() {
    let (app, _db, _url) = test_app().await;
    let (source_id, _, source_columns) =
        table_with_columns(&app, "raw-svc", &["first_name", "last_name"]).await;
    let (target_id, _, target_columns) = table_with_columns(&app, "mart-svc", &["full_name"]).await;
    let edge_id = edge(&app, &source_id, &target_id, "manual").await;
    let (status, body) = send(
        &app,
        "PUT",
        &format!("/lineage/{edge_id}/columns"),
        Some(json!({
            "mappings": [
                {
                    "fromColumnFqn": source_columns[0],
                    "toColumnFqn": target_columns[0],
                    "expression": "concat(first_name, ' ', last_name)",
                },
                { "fromColumnFqn": source_columns[1], "toColumnFqn": target_columns[0] },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, renamed) = send(
        &app,
        "PATCH",
        &format!("/assets/{source_id}"),
        Some(json!({ "name": "orders_v2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    let new_table_fqn = renamed["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string();

    let (_, listed) = send(&app, "GET", &format!("/lineage/{edge_id}/columns"), None).await;
    let mappings = listed.as_array().expect("an array");
    assert_eq!(mappings.len(), 2, "{listed}");
    assert!(
        mappings.iter().all(|m| m["fromColumnFqn"]
            .as_str()
            .expect("fqn")
            .starts_with(&format!("{new_table_fqn}."))),
        "every source column must move with the rename: {listed}"
    );
    // The target side is untouched — only the renamed subtree's own columns
    // move, and a mapping is not symmetric.
    assert!(
        mappings
            .iter()
            .all(|m| m["toColumnFqn"] == target_columns[0].as_str()),
        "the unrelated target side must not move: {listed}"
    );
}

/// A mapping to a column that does not exist is a lineage claim nothing can
/// render, and it would sit there looking like coverage.
#[tokio::test]
async fn a_mapping_naming_a_column_that_does_not_exist_is_refused() {
    let (app, _db, _url) = test_app().await;
    let (source_id, _, source_columns) = table_with_columns(&app, "raw-svc", &["id"]).await;
    let (target_id, _, _) = table_with_columns(&app, "mart-svc", &["id"]).await;
    let edge_id = edge(&app, &source_id, &target_id, "manual").await;

    let (status, body) = send(
        &app,
        "PUT",
        &format!("/lineage/{edge_id}/columns"),
        Some(json!({
            "mappings": [{
                "fromColumnFqn": source_columns[0],
                "toColumnFqn": "nothing.like.this",
            }],
        })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// **Mappings are keyed by column FQN, not by position.** Adding a column that
/// sorts before the mapped one cannot move the mapping — the failure a
/// position-based model has.
#[tokio::test]
async fn a_mapping_follows_the_column_name_not_its_position() {
    let (app, _db, _url) = test_app().await;
    let (source_id, _, source_columns) = table_with_columns(&app, "raw-svc", &["zzz_last"]).await;
    let (target_id, target_fqn, target_columns) =
        table_with_columns(&app, "mart-svc", &["total"]).await;
    let edge_id = edge(&app, &source_id, &target_id, "manual").await;
    send(
        &app,
        "PUT",
        &format!("/lineage/{edge_id}/columns"),
        Some(json!({
            "mappings": [{
                "fromColumnFqn": source_columns[0],
                "toColumnFqn": target_columns[0],
            }],
        })),
    )
    .await;

    // A new column that sorts first changes nothing about which column the
    // mapping is on.
    send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "column", "name": "aaa_first", "parentId": target_id })),
    )
    .await;

    let (_, listed) = send(&app, "GET", &format!("/lineage/{edge_id}/columns"), None).await;
    assert_eq!(
        listed[0]["toColumnFqn"],
        target_columns[0].as_str(),
        "{listed}"
    );
    assert!(target_fqn.contains("orders"), "fixture sanity");
}

/// A `PUT` replaces wholesale: a refactor that makes a column come from one
/// source instead of two cannot be expressed by adding.
#[tokio::test]
async fn setting_mappings_replaces_what_was_there() {
    let (app, _db, _url) = test_app().await;
    let (source_id, _, source_columns) =
        table_with_columns(&app, "raw-svc", &["first_name", "last_name"]).await;
    let (target_id, _, target_columns) = table_with_columns(&app, "mart-svc", &["full_name"]).await;
    let edge_id = edge(&app, &source_id, &target_id, "manual").await;

    send(
        &app,
        "PUT",
        &format!("/lineage/{edge_id}/columns"),
        Some(json!({
            "mappings": [
                { "fromColumnFqn": source_columns[0], "toColumnFqn": target_columns[0] },
                { "fromColumnFqn": source_columns[1], "toColumnFqn": target_columns[0] },
            ],
        })),
    )
    .await;
    send(
        &app,
        "PUT",
        &format!("/lineage/{edge_id}/columns"),
        Some(json!({
            "mappings": [
                { "fromColumnFqn": source_columns[0], "toColumnFqn": target_columns[0] },
            ],
        })),
    )
    .await;

    let (_, listed) = send(&app, "GET", &format!("/lineage/{edge_id}/columns"), None).await;
    assert_eq!(listed.as_array().expect("an array").len(), 1, "{listed}");
}

// ── Slice E: connector-asserted lineage reconciles ──────────────────────────

/// **The critical test.** A connector run replaces what *that connector*
/// asserted, and nothing else. Source-blind replacement silently deletes
/// lineage a human curated — every night, without an error.
#[tokio::test]
async fn a_manually_curated_edge_survives_a_connector_reconciliation() {
    let (app, _db, _url) = test_app().await;
    let (raw_id, raw_fqn, _) = table_with_columns(&app, "raw-svc", &["id"]).await;
    let (mart_id, _, _) = table_with_columns(&app, "mart-svc", &["id"]).await;
    let (other_id, _, _) = table_with_columns(&app, "other-svc", &["id"]).await;

    // A human knows raw → other. The connector has never seen it.
    edge(&app, &raw_id, &other_id, "manual").await;
    // The connector asserted raw → mart on a previous run.
    edge(&app, &raw_id, &mart_id, "connector").await;

    // This run enumerates the same scope and asserts nothing at all — the
    // shape that deletes everything if the scoping is wrong.
    let (status, report) = send(
        &app,
        "POST",
        "/lineage/reconcile",
        Some(json!({
            "source": "connector",
            "scopePrefix": raw_fqn,
            "edges": [],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(
        report["removed"], 1,
        "the connector's own edge went: {report}"
    );

    let (_, graph) = send(&app, "GET", &format!("/lineage/asset/{raw_id}"), None).await;
    let rendered = graph.to_string();
    assert!(
        rendered.contains(&other_id),
        "the curated edge must survive: {graph}"
    );
    assert!(
        !rendered.contains(&mart_id),
        "and the connector's must not: {graph}"
    );
}

/// A later run asserting a different edge leaves only that one, from that
/// source — the ordinary convergence case.
#[tokio::test]
async fn a_later_run_replaces_the_earlier_runs_edges() {
    let (app, _db, _url) = test_app().await;
    let (raw_id, raw_fqn, _) = table_with_columns(&app, "raw-svc", &["id"]).await;
    let (first_id, _, _) = table_with_columns(&app, "mart-a", &["id"]).await;
    let (second_id, _, _) = table_with_columns(&app, "mart-b", &["id"]).await;

    send(
        &app,
        "POST",
        "/lineage/reconcile",
        Some(json!({
            "source": "connector",
            "scopePrefix": raw_fqn,
            "edges": [{
                "fromAssetId": raw_id, "toAssetId": first_id, "relationship": "feeds",
            }],
        })),
    )
    .await;

    let (_, second) = send(
        &app,
        "POST",
        "/lineage/reconcile",
        Some(json!({
            "source": "connector",
            "scopePrefix": raw_fqn,
            "edges": [{
                "fromAssetId": raw_id, "toAssetId": second_id, "relationship": "feeds",
            }],
        })),
    )
    .await;

    assert_eq!(second["added"], 1, "{second}");
    assert_eq!(second["removed"], 1, "{second}");

    let (_, graph) = send(&app, "GET", &format!("/lineage/asset/{raw_id}"), None).await;
    let rendered = graph.to_string();
    assert!(rendered.contains(&second_id), "{graph}");
    assert!(!rendered.contains(&first_id), "{graph}");
}

/// **Reconciliation is scoped.** A run covering one service must not remove
/// edges in another — the same bug as source-blindness wearing a different hat.
#[tokio::test]
async fn a_run_covering_one_scope_leaves_another_scopes_edges_alone() {
    let (app, _db, _url) = test_app().await;
    let (raw_id, raw_fqn, _) = table_with_columns(&app, "raw-svc", &["id"]).await;
    let (elsewhere_id, _, _) = table_with_columns(&app, "elsewhere-svc", &["id"]).await;
    let (mart_id, _, _) = table_with_columns(&app, "mart-svc", &["id"]).await;

    // The same connector asserted an edge out of a different service.
    edge(&app, &elsewhere_id, &mart_id, "connector").await;
    edge(&app, &raw_id, &mart_id, "connector").await;

    send(
        &app,
        "POST",
        "/lineage/reconcile",
        Some(json!({ "source": "connector", "scopePrefix": raw_fqn, "edges": [] })),
    )
    .await;

    let (_, graph) = send(&app, "GET", &format!("/lineage/asset/{elsewhere_id}"), None).await;
    assert!(
        graph.to_string().contains(&mart_id),
        "an edge outside the enumerated scope must survive: {graph}"
    );
}

/// A reconciliation with no scope would replace every edge the source ever
/// asserted anywhere, so the scope is required rather than defaulted.
#[tokio::test]
async fn a_reconciliation_without_a_scope_is_refused() {
    let (app, _db, _url) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/lineage/reconcile",
        Some(json!({ "source": "connector", "edges": [] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

// ── Slice F: lineage survives entity deletion ───────────────────────────────

/// **Soft delete retains edges.** Restoring a mistakenly deleted table restores
/// its lineage too — an edge deleted on tombstoning could not come back, and
/// the restore would silently be partial.
#[tokio::test]
async fn soft_deleting_an_asset_retains_its_lineage_and_restore_brings_it_back() {
    let (app, _db, _url) = test_app().await;
    let (raw_id, _, _) = table_with_columns(&app, "raw-svc", &["id"]).await;
    let (mart_id, _, _) = table_with_columns(&app, "mart-svc", &["id"]).await;
    edge(&app, &raw_id, &mart_id, "manual").await;

    let (status, _) = send(&app, "DELETE", &format!("/assets/{mart_id}"), None).await;
    assert_eq!(status, StatusCode::OK, "soft delete returns a count");

    let (_, during) = send(&app, "GET", &format!("/lineage/asset/{raw_id}"), None).await;
    assert!(
        during.to_string().contains(&mart_id),
        "the edge is retained while the asset is tombstoned: {during}"
    );

    let (status, _) = send(&app, "POST", &format!("/assets/{mart_id}/restore"), None).await;
    assert_eq!(status, StatusCode::OK);

    let (_, after) = send(&app, "GET", &format!("/lineage/asset/{raw_id}"), None).await;
    assert!(
        after.to_string().contains(&mart_id),
        "and it is a normal node again after restore: {after}"
    );
}
