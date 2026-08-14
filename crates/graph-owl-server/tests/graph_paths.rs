//! Plan 111 Slice A — `POST /graph/paths`, the route for a capability the
//! engine has answered since Epic 7a and no human could ask for.
//!
//! **Domain-neutral by construction.** Every request below is two node ids,
//! a direction, two bounds and an optional list of edge names the *caller*
//! supplies. Nothing in the route, the handler or the facade knows what an
//! invoice, a patient or a counterparty is — which is the whole test
//! `plans/111-capability-surface.md` applies to itself: *would this work if
//! the only installed pack were hospitality?*

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{json_body, test_app};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn create_table(app: &axum::Router, fully_qualified_name: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tables")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "name": "t", "fullyQualifiedName": fully_qualified_name }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["id"]
        .as_str()
        .expect("id")
        .to_string()
}

async fn link(app: &axum::Router, from: &str, to: &str, kind: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/tables/{from}/relationships"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "toTableId": to, "relationshipType": kind }).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(response.status(), StatusCode::CREATED);
}

async fn paths(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graph/paths")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// A three-table chain, returned as the chain — **the middle node is the
/// answer**, and a response carrying only a hop count would be the one
/// answer a reader cannot act on.
#[tokio::test]
async fn a_connected_pair_comes_back_as_the_route_between_them() {
    let (app, _container, _connection_string) = test_app().await;
    let a = create_table(&app, "wh.public.a").await;
    let b = create_table(&app, "wh.public.b").await;
    let c = create_table(&app, "wh.public.c").await;
    link(&app, &a, &b, "derivedFrom").await;
    link(&app, &b, &c, "derivedFrom").await;

    let (status, body) = paths(
        &app,
        json!({ "from": a, "to": c, "direction": "outgoing", "hops": 4 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let routes = body["paths"].as_array().expect("paths array");
    assert_eq!(routes.len(), 1, "{body}");
    assert_eq!(routes[0]["length"], 2);
    let nodes = routes[0]["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 3, "{body}");
    // Nodes travel as rendered `Sid`s; the endpoints must be the ones asked
    // for, and the middle one must be the table that actually joins them.
    assert!(nodes[0].as_str().expect("sid").contains(&a), "{body}");
    assert!(nodes[1].as_str().expect("sid").contains(&b), "{body}");
    assert!(nodes[2].as_str().expect("sid").contains(&c), "{body}");
    assert_eq!(body["truncated"], false);
}

/// **Two unconnected nodes are `200` with nothing in it, not `404`.**
/// "These are not related" is the commonest true answer to the question, and
/// a status code that means "no such thing" would make the normal case
/// indistinguishable from a bad request.
#[tokio::test]
async fn an_unconnected_pair_is_an_empty_answer_not_a_failure() {
    let (app, _container, _connection_string) = test_app().await;
    let a = create_table(&app, "wh.public.a").await;
    let b = create_table(&app, "wh.public.b").await;
    let c = create_table(&app, "wh.public.c").await;
    link(&app, &a, &b, "derivedFrom").await;

    let (status, body) = paths(&app, json!({ "from": a, "to": c })).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["paths"].as_array().expect("paths array").is_empty(),
        "{body}"
    );
    assert_eq!(body["truncated"], false);
}

/// **A capped enumeration says it was capped.** Presenting one of two routes
/// as the only route is the dangerous direction: the reader's conclusion is
/// "there is exactly one way these are connected", which is a stronger claim
/// than the server ever made.
#[tokio::test]
async fn a_capped_enumeration_reports_that_it_stopped_early() {
    let (app, _container, _connection_string) = test_app().await;
    let a = create_table(&app, "wh.public.a").await;
    let b = create_table(&app, "wh.public.b").await;
    let c = create_table(&app, "wh.public.c").await;
    let d = create_table(&app, "wh.public.d").await;
    link(&app, &a, &b, "derivedFrom").await;
    link(&app, &a, &c, "derivedFrom").await;
    link(&app, &b, &d, "derivedFrom").await;
    link(&app, &c, &d, "derivedFrom").await;

    let (status, all) = paths(
        &app,
        json!({ "from": a, "to": d, "direction": "outgoing", "maxPaths": 10 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all["paths"].as_array().expect("paths").len(), 2, "{all}");
    assert_eq!(all["truncated"], false);

    let (status, capped) = paths(
        &app,
        json!({ "from": a, "to": d, "direction": "outgoing", "maxPaths": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{capped}");
    assert_eq!(capped["paths"].as_array().expect("paths").len(), 1);
    assert_eq!(capped["truncated"], true, "{capped}");
}

/// The filter's vocabulary is the caller's. A pack naming its edges
/// `derivedFrom` and one naming them `prescribedIn` make the same request
/// with different strings.
#[tokio::test]
async fn a_relationship_filter_excludes_a_route_that_uses_another_edge() {
    let (app, _container, _connection_string) = test_app().await;
    let a = create_table(&app, "wh.public.a").await;
    let b = create_table(&app, "wh.public.b").await;
    let c = create_table(&app, "wh.public.c").await;
    link(&app, &a, &b, "derivedFrom").await;
    link(&app, &b, &c, "relatedTo").await;

    let (_, unfiltered) = paths(
        &app,
        json!({ "from": a, "to": c, "direction": "outgoing", "hops": 4 }),
    )
    .await;
    assert_eq!(
        unfiltered["paths"].as_array().expect("paths").len(),
        1,
        "{unfiltered}"
    );

    let (status, filtered) = paths(
        &app,
        json!({
            "from": a, "to": c, "direction": "outgoing", "hops": 4,
            "relationshipTypes": ["derivedFrom"]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    assert!(
        filtered["paths"].as_array().expect("paths").is_empty(),
        "the second hop is a `relatedTo` edge the filter excludes: {filtered}"
    );
}

/// An unparseable direction is a `400` naming the field, in RFC 9457 shape —
/// the convention every other route in this server already follows.
#[tokio::test]
async fn an_unknown_direction_is_rejected_by_name() {
    let (app, _container, _connection_string) = test_app().await;
    let a = create_table(&app, "wh.public.a").await;
    let b = create_table(&app, "wh.public.b").await;

    let (status, body) = paths(&app, json!({ "from": a, "to": b, "direction": "sideways" })).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let errors = body["errors"].as_array().expect("errors array");
    assert!(
        errors.iter().any(|e| e["field"] == "direction"),
        "the rejection must name the field that was wrong: {body}"
    );
}

/// Plan 112 Slice A — the explorer's relationship filter, over HTTP.
///
/// **Absent and empty are different requests**, and the difference is the
/// safety property: absent means "no filter", empty means "a filter that
/// excludes everything". Collapsing them would make a control that selects
/// nothing silently show everything — a filter that looks like it works and
/// does not.
///
/// **Real assets, not tables.** `/assets/{id}/graph` authorizes its seed as an
/// asset, and `tables` and `assets` are different relations — a table id is a
/// `404` here, and assets have no relationship-creation route at all. The two
/// edge types below are therefore containment edges, which is what a real
/// asset neighbourhood is mostly made of anyway.
#[tokio::test]
async fn the_subgraph_filters_by_relationship_type() {
    let (app, _container, _connection_string) = test_app().await;

    let asset = async |body: Value| -> String {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/assets")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(response.status(), StatusCode::CREATED);
        json_body(response).await["id"]
            .as_str()
            .expect("id")
            .to_string()
    };

    let service = asset(json!({ "kind": "service", "name": "wh" })).await;
    let database =
        asset(json!({ "kind": "database", "name": "retail", "parentId": service })).await;
    let schema = asset(json!({ "kind": "schema", "name": "public", "parentId": database })).await;
    let table = asset(json!({ "kind": "table", "name": "a", "parentId": schema })).await;
    // A child as well as a parent, so the seed sits between two *different*
    // edge names and a filter has something to tell apart.
    asset(json!({ "kind": "column", "name": "amount", "parentId": table })).await;

    let walk = async |suffix: &str| -> Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/assets/{table}/graph?hops=1{suffix}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should be handled");
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    };

    let kinds = |body: &Value| -> std::collections::BTreeSet<String> {
        body["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .map(|e| {
                e["relationship"]
                    .as_str()
                    .expect("relationship")
                    .to_string()
            })
            .collect()
    };

    let unfiltered = walk("").await;
    let every = kinds(&unfiltered);
    assert!(
        every.len() >= 2,
        "the seed must sit between two edge names for this to prove anything: {unfiltered}"
    );
    let one = every.iter().next().expect("an edge name").clone();

    let filtered = walk(&format!("&relationshipTypes={one}")).await;
    assert_eq!(
        kinds(&filtered),
        [one.clone()].into_iter().collect(),
        "only the named edge survives: {filtered}"
    );

    // Every name in one comma-separated token — a reader pasting the URL sees
    // the whole filter at once, and it must equal the unfiltered answer.
    let all_names = every.iter().cloned().collect::<Vec<_>>().join(",");
    let both = walk(&format!("&relationshipTypes={all_names}")).await;
    assert_eq!(kinds(&both), every, "{both}");

    // **Empty means nothing, not everything.**
    let empty = walk("&relationshipTypes=").await;
    assert!(
        empty["edges"].as_array().expect("edges").is_empty(),
        "an explicitly empty filter excludes every edge: {empty}"
    );

    // A name no edge uses matches nothing, and is not an error: there is no
    // vocabulary to validate against when a pack brings its own edge names.
    let unknown = walk("&relationshipTypes=noSuchEdge").await;
    assert!(
        unknown["edges"].as_array().expect("edges").is_empty(),
        "{unknown}"
    );
}
