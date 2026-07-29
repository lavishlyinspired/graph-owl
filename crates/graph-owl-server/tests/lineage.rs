//! Epic 29 Slices A and B: what feeds what, and how far.
//!
//! The two highest-stakes questions in data engineering — *what breaks if I
//! change this* and *where did this number come from* — are the same graph read
//! in opposite directions, so every test here checks both.

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
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A service → database → schema, then `count` tables under it.
async fn tables(app: &axum::Router, count: usize) -> Vec<String> {
    let (_, service) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": "hdfc-core" })),
    )
    .await;
    let (_, database) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "database", "name": "retail", "parentId": service["id"] })),
    )
    .await;
    let (_, schema) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "schema", "name": "payments", "parentId": database["id"] })),
    )
    .await;

    let mut ids = Vec::new();
    for n in 0..count {
        let (status, table) = send(
            app,
            "POST",
            "/assets",
            Some(json!({ "kind": "table", "name": format!("t{n}"), "parentId": schema["id"] })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{table}");
        ids.push(table["id"].as_str().expect("an id").to_string());
    }
    ids
}

async fn feeds(app: &axum::Router, from: &str, to: &str) -> Value {
    let (status, edge) = send(
        app,
        "POST",
        "/lineage",
        Some(json!({ "fromAssetId": from, "toAssetId": to })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{edge}");
    edge
}

#[tokio::test]
async fn a_human_can_record_that_one_table_feeds_another() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 2).await;

    let edge = feeds(&app, &ids[0], &ids[1]).await;

    assert_eq!(edge["fromAssetId"], ids[0]);
    assert_eq!(edge["toAssetId"], ids[1]);
    assert_eq!(edge["relationship"], "feeds");
    assert_eq!(edge["source"], "manual", "a person asserted it");
}

/// **The identity decision.** Automation is often wrong about lineage a human
/// knows, and a human is often out of date about lineage automation observes.
/// Both facts must coexist, or one silently overwrites the other and which one
/// wins depends on run order.
#[tokio::test]
async fn the_same_pair_from_two_sources_are_two_edges() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 2).await;

    let manual = feeds(&app, &ids[0], &ids[1]).await;
    let (status, automated) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({ "fromAssetId": ids[0], "toAssetId": ids[1], "source": "connector" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{automated}");
    assert_ne!(manual["id"], automated["id"]);
}

/// And the negative: the *same* source asserting it twice is one fact stated
/// twice, which is a conflict.
#[tokio::test]
async fn the_same_source_cannot_assert_the_same_edge_twice() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 2).await;
    feeds(&app, &ids[0], &ids[1]).await;

    let (status, body) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({ "fromAssetId": ids[0], "toAssetId": ids[1] })),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn an_asset_cannot_feed_itself() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 1).await;

    let (status, body) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({ "fromAssetId": ids[0], "toAssetId": ids[0] })),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a self-edge is a cycle of length one: {body}"
    );
}

#[tokio::test]
async fn lineage_to_a_nonexistent_asset_is_not_found() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 1).await;

    let (status, _) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({ "fromAssetId": ids[0], "toAssetId": "99999999-9999-4999-8999-999999999999" })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Lineage runs table-to-table or column-to-column. Mixing levels makes "what is
/// downstream of this table" return a set whose members are not comparable.
#[tokio::test]
async fn lineage_across_levels_is_refused() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 1).await;
    let (_, column) = send(
        &app,
        "POST",
        "/assets",
        Some(json!({ "kind": "column", "name": "amount", "parentId": ids[0] })),
    )
    .await;

    let (status, body) = send(
        &app,
        "POST",
        "/lineage",
        Some(json!({ "fromAssetId": ids[0], "toAssetId": column["id"] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn an_edge_can_be_removed() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 2).await;
    let edge = feeds(&app, &ids[0], &ids[1]).await;

    let (status, _) = send(
        &app,
        "DELETE",
        &format!("/lineage/{}", edge["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, graph) = send(&app, "GET", &format!("/lineage/asset/{}", ids[0]), None).await;
    assert!(graph["edges"].as_array().unwrap().is_empty(), "{graph}");
}

/// **Depth is exact, not approximate.** A five-deep chain asked for three
/// levels returns three — an off-by-one here silently widens every impact
/// analysis somebody makes a change decision on.
#[tokio::test]
async fn a_bounded_walk_returns_exactly_the_depth_asked_for() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 6).await;
    for pair in ids.windows(2) {
        feeds(&app, &pair[0], &pair[1]).await;
    }

    let (status, graph) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?downstream=3&upstream=0", ids[0]),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{graph}");
    // The root plus three hops.
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 4, "{graph}");
    assert_eq!(graph["edges"].as_array().unwrap().len(), 3, "{graph}");
}

/// Both directions, and each spends its own budget. A merged frontier would let
/// an upstream hop spend the downstream allowance, so `upstream=1&downstream=3`
/// would return something that is neither.
#[tokio::test]
async fn upstream_and_downstream_are_bounded_independently() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 5).await;
    for pair in ids.windows(2) {
        feeds(&app, &pair[0], &pair[1]).await;
    }

    // Walk from the middle: two behind, two ahead.
    let (_, graph) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?upstream=1&downstream=2", ids[2]),
        None,
    )
    .await;

    let names: Vec<&str> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();

    assert!(names.contains(&"t1"), "one upstream: {graph}");
    assert!(!names.contains(&"t0"), "and not two: {graph}");
    assert!(
        names.contains(&"t3") && names.contains(&"t4"),
        "two downstream: {graph}"
    );
}

/// A diamond yields the shared node **once**, with both inbound edges. Without
/// a visited set it appears twice, and every count downstream of it doubles.
#[tokio::test]
async fn a_diamond_yields_the_shared_node_once_with_both_edges() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 4).await;
    feeds(&app, &ids[0], &ids[1]).await;
    feeds(&app, &ids[0], &ids[2]).await;
    feeds(&app, &ids[1], &ids[3]).await;
    feeds(&app, &ids[2], &ids[3]).await;

    let (_, graph) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?downstream=3&upstream=0", ids[0]),
        None,
    )
    .await;

    let nodes = graph["nodes"].as_array().unwrap();
    let shared: Vec<_> = nodes.iter().filter(|n| n["name"] == "t3").collect();
    assert_eq!(shared.len(), 1, "the shared node appears once: {graph}");
    assert_eq!(graph["edges"].as_array().unwrap().len(), 4, "{graph}");
}

/// A cycle asserted despite the acyclicity intent must terminate rather than
/// hang. The graph is called a DAG because it should be one, not because
/// anything stops somebody asserting otherwise.
#[tokio::test]
async fn a_cycle_terminates() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 3).await;
    feeds(&app, &ids[0], &ids[1]).await;
    feeds(&app, &ids[1], &ids[2]).await;
    feeds(&app, &ids[2], &ids[0]).await;

    let (status, graph) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?downstream=10&upstream=0", ids[0]),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{graph}");
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 3, "{graph}");
}

#[tokio::test]
async fn a_walk_deeper_than_the_maximum_is_refused() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 1).await;

    let (status, _) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?downstream=99", ids[0]),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// "Nothing downstream" and "the downstream was deleted" are opposite
/// conclusions, so a tombstoned node stays in the graph and is flagged.
#[tokio::test]
async fn a_deleted_node_is_shown_flagged_rather_than_dropped() {
    let (app, _db, _) = test_app().await;
    let ids = tables(&app, 2).await;
    feeds(&app, &ids[0], &ids[1]).await;

    send(&app, "DELETE", &format!("/assets/{}", ids[1]), None).await;

    let (_, graph) = send(
        &app,
        "GET",
        &format!("/lineage/asset/{}?downstream=1", ids[0]),
        None,
    )
    .await;
    let gone = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "t1")
        .expect("the deleted node stays in the picture");

    assert_eq!(gone["deleted"], true, "and is flagged: {graph}");
}
