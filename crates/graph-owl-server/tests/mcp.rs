//! The MCP surface at the wire — Epic 14's transport and Epic 32's write half.
//!
//! The protocol's own rules are proved exhaustively in
//! `graph_owl_mcp::jsonrpc`, without a socket. **These tests prove the wiring**,
//! and three properties that only exist once there is a real server behind it:
//!
//! 1. A real agent can complete the thesis question over HTTP.
//! 2. Epic 32's grant actually gates a write that arrives over the wire — an
//!    un-granted agent is refused by the catalog, not merely by a test double.
//! 3. The refusal reaches the agent as a *tool result*, so it can read why.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::test_app;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Who the MCP session runs as in these tests.
///
/// `test_app` configures neither OIDC nor a shared secret, so `Auth` falls
/// through to **open mode**, which resolves every request to
/// `Principal::system()` — id `system`, seeded as a `users` row by `V15` so the
/// foreign keys on "who did this" columns hold. It is `is_admin` and its kind is
/// `System` rather than `Service`, so it may write grants: the self-grant
/// refusal in `set_agent_grant` bars *service* principals specifically, which is
/// what an agent is in a real deployment.
///
/// Hardcoded rather than read from an endpoint because there is no `/me` route
/// — a fixture invented against one would fail for a reason unrelated to what
/// these tests check.
const AGENT: &str = "system";

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

/// One JSON-RPC call.
async fn rpc(app: &axum::Router, method: &str, params: Value) -> Value {
    let (status, body) = send(
        app,
        "POST",
        "/mcp",
        Some(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {body}");
    body
}

/// The decoded payload of a successful tool call.
fn payload(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("expected text content, got {response}"));
    serde_json::from_str(text).unwrap_or_else(|_| panic!("content is not JSON: {text}"))
}

async fn service(app: &axum::Router, name: &str) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/assets",
        Some(json!({ "kind": "service", "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["fullyQualifiedName"]
        .as_str()
        .expect("an fqn")
        .to_string()
}

#[tokio::test]
async fn a_client_can_negotiate_and_discover_the_tools() {
    let (app, _container, _) = test_app().await;

    let initialized = rpc(&app, "initialize", json!({})).await;
    let listed = rpc(&app, "tools/list", json!({})).await;

    assert_eq!(
        initialized["result"]["serverInfo"]["name"], "graph-owl",
        "{initialized}"
    );
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names.len(),
        22,
        // 13 at Epic 14's own count (7 read + 6 write), plus the 8
        // intelligence tools Epic 105 P10 added (traverse, find_evidence,
        // explain, reconcile, analytics, run_rule, resolve_entity,
        // calculate_risk), plus Epic 105 P106 Slice 4b's run_pack_query.
        "{names:?}"
    );
    assert!(names.contains(&"get_asset_context"), "{names:?}");
    assert!(names.contains(&"record_memory"), "{names:?}");
}

/// **The thesis question, over the wire.** An agent that has never seen this
/// catalog reads an asset it was told nothing about.
#[tokio::test]
async fn an_agent_can_read_an_asset_over_the_wire() {
    let (app, _container, _) = test_app().await;
    let fqn = service(&app, "warehouse").await;

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "get_asset_context",
            "arguments": { "fullyQualifiedName": fqn }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], false, "{response}");
    let context = payload(&response);
    assert_eq!(context["fullyQualifiedName"], fqn);
    assert!(
        context["trust"]["gaps"].is_array(),
        "trust rides on every context: {context}"
    );
}

/// **An absent asset and a denied one give the same answer**, and it arrives as
/// a tool result rather than a transport error — the property Slice A rests on,
/// carried unchanged through the wire.
#[tokio::test]
async fn an_unknown_asset_is_a_tool_error_not_a_transport_error() {
    let (app, _container, _) = test_app().await;

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "get_asset_context",
            "arguments": { "fullyQualifiedName": "no.such.asset" }
        }),
    )
    .await;

    assert!(
        response.get("error").is_none(),
        "a denial is not an outage: {response}"
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
}

/// **Epic 32's gate is real over the wire.** An agent with no grant is refused
/// by the catalog, and the refusal reaches it as something it can read and act
/// on — not a bare 403.
#[tokio::test]
async fn an_ungranted_agent_is_refused_with_the_reason() {
    let (app, _container, _) = test_app().await;
    let fqn = service(&app, "warehouse").await;

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "record_memory",
            "arguments": {
                "fullyQualifiedName": fqn,
                "content": "the nightly load double-counts refunds",
                "rationale": "the row counts differ by exactly the refund count",
                "confidence": 0.9
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true, "{response}");
    let problem = payload(&response);
    let text = problem["error"].as_str().unwrap_or_default();
    assert!(
        text.contains("recordMemory") || text.contains("capability"),
        "the agent can ask a human for exactly this: {problem}"
    );
}

/// And with a grant it goes through — otherwise the test above would pass
/// against a surface that refuses everything.
#[tokio::test]
async fn a_granted_agent_can_write() {
    let (app, _container, _) = test_app().await;
    let fqn = service(&app, "warehouse").await;

    let (granted, body) = send(
        &app,
        "PUT",
        &format!("/agents/{AGENT}/grant"),
        Some(json!({ "capabilities": ["recordMemory"] })),
    )
    .await;
    assert_eq!(granted, StatusCode::OK, "{body}");

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "record_memory",
            "arguments": {
                "fullyQualifiedName": fqn,
                "content": "the nightly load double-counts refunds",
                "rationale": "the row counts differ by exactly the refund count",
                "confidence": 0.9
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], false, "{response}");
    let receipt = payload(&response);
    assert_eq!(
        receipt["outcome"], "proposed",
        "recordMemory is not a direct-apply capability: {receipt}"
    );
    assert!(receipt["proposalId"].is_string(), "{receipt}");
}

/// **A capability that does not exist is a `400` naming it**, never a grant that
/// silently drops half of what it was given.
#[tokio::test]
async fn a_grant_naming_an_impossible_capability_is_refused() {
    let (app, _container, _) = test_app().await;
    let (status, problem) = send(
        &app,
        "PUT",
        &format!("/agents/{AGENT}/grant"),
        Some(json!({ "capabilities": ["deleteEverything"] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{problem}");
    assert!(
        problem.to_string().contains("deleteEverything"),
        "name what was wrong: {problem}"
    );
}

/// A refused write still appears in the agent's history — an agent repeatedly
/// attempting un-granted writes is a signal worth seeing.
#[tokio::test]
async fn a_refused_write_is_recorded_in_the_agents_activity() {
    let (app, _container, _) = test_app().await;
    let fqn = service(&app, "warehouse").await;
    let _ = rpc(
        &app,
        "tools/call",
        json!({
            "name": "record_memory",
            "arguments": {
                "fullyQualifiedName": fqn,
                "content": "x", "rationale": "y", "confidence": 0.9
            }
        }),
    )
    .await;

    let (status, activity) = send(&app, "GET", &format!("/agents/{AGENT}/activity"), None).await;

    assert_eq!(status, StatusCode::OK, "{activity}");
    let entries = activity["data"].as_array().expect("entries");
    assert!(
        entries.iter().any(|entry| entry["outcome"] == "refused"),
        "the refusal is on the record: {activity}"
    );
}

/// A JSON-RPC notification gets `204` and no body. An empty `200` body is not
/// nothing, and a client parsing it as JSON fails on a request that succeeded.
#[tokio::test]
async fn a_notification_gets_no_content() {
    let (app, _container, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/mcp",
        Some(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null, "no body at all");
}

/// A malformed body is a JSON-RPC parse error carried over HTTP `200` — the
/// transport delivered the message, so HTTP has nothing to complain about.
#[tokio::test]
async fn a_malformed_body_is_a_protocol_error_at_http_200() {
    let (app, _container, _) = test_app().await;

    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(Body::from("{not json"))
        .expect("request");
    let response = app.oneshot(request).await.expect("handled");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a JSON-RPC document");
    assert_eq!(body["error"]["code"], -32700, "{body}");
}

/// **The trace-review finding** (`plans/106-agent-trace-hygiene.md` Slice 2,
/// `plans/105-mcp-tool-visibility-divergence.md`): a live agent asked
/// `SELECT ?s ?p ?o { ?s gst:governedBy ?o }` over `query_graph`, got zero
/// rows, and had no way to tell "the predicate is genuinely never asserted"
/// apart from "the graph that would hold it was never scanned" — both look
/// identical without `factsScanned`/`plan` on the wire. `QueryAnswer`
/// previously carried only `rows`/`truncated`; this proves the same
/// diagnostics `/sparql` already renders (`query_outcome_json`) now reach
/// `query_graph` too.
#[tokio::test]
async fn query_graph_reports_what_it_scanned_and_planned() {
    let (app, _container, _) = test_app().await;

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "query_graph",
            "arguments": { "query": "SELECT ?s ?p ?o WHERE { ?s ?p ?o }" }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], false, "{response}");
    let answer = payload(&response);
    assert!(answer["factsScanned"].is_u64(), "{answer}");
    // No pattern position could be bound, so pushdown falls back to a whole
    // scan — `describe_scan`'s own rendering of an all-unbound
    // `TriplePattern`, not the `?s ?p ?o` shorthand a reader might expect.
    assert_eq!(answer["plan"], json!(["? ? ?"]), "{answer}");
    assert_eq!(answer["variables"], json!(["s", "p", "o"]), "{answer}");
    // Silence is the signal: nothing rewrote this query, no axiom was
    // refused, and no alignment was crossed, so all three are absent from
    // the wire entirely — the overwhelming-majority case every one of
    // `SparqlOutcome`'s own doc comments names, not present-and-empty.
    let object = answer.as_object().unwrap();
    assert!(!object.contains_key("qlRewrite"), "{answer}");
    assert!(!object.contains_key("refusedAxioms"), "{answer}");
    assert!(!object.contains_key("alignmentsUsed"), "{answer}");
}

/// **The full stack, end to end**: `POST /packs/{pack}/queries` registers
/// a named query the same way the Python pack loader does (Epic 105 P106
/// Slice 4a), and `run_pack_query`'s MCP tool (Slice 4b) invokes it by
/// name over the wire with a caller-supplied binding — proving the
/// registry, `Catalog::run_pack_query`'s substitution, and the MCP
/// dispatch all agree on the same query.
#[tokio::test]
async fn run_pack_query_answers_a_registered_query_over_mcp() {
    let (app, _container, _) = test_app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/namespaces",
        Some(json!({ "iri": "https://graph-owl.dev/packs/run-pack-query-test#" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(
        &app,
        "POST",
        "/predicates",
        Some(json!({"namespace": 1024, "name": "status", "valueType": 1, "many": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let turtle = r#"
        @prefix rpq: <https://graph-owl.dev/packs/run-pack-query-test#> .

        rpq:subject-one rpq:status "open" .
    "#;
    let import_request = Request::builder()
        .method("POST")
        .uri("/graph/import/rdf?source=run-pack-query-test&format=turtle")
        .header("content-type", "text/turtle")
        .body(Body::from(turtle))
        .expect("request should build");
    let import_response = app
        .clone()
        .oneshot(import_request)
        .await
        .expect("request should be handled");
    assert_eq!(
        import_response.status(),
        StatusCode::OK,
        "{import_response:?}"
    );

    let (status, body) = send(
        &app,
        "POST",
        "/packs/run-pack-query-test/queries",
        Some(json!({
            "queries": [{
                "name": "status-of",
                "query": "SELECT ?status WHERE { GRAPH ?g { \
                           VALUES ?subject { <{{subject}}> } \
                           ?subject <https://graph-owl.dev/packs/run-pack-query-test#status> ?status } }",
            }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "run_pack_query",
            "arguments": {
                "pack": "run-pack-query-test",
                "query": "status-of",
                "bindings": {
                    "subject": "https://graph-owl.dev/packs/run-pack-query-test#subject-one",
                },
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], false, "{response}");
    let answer = payload(&response);
    let rows = answer["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 1, "{answer}");
    assert_eq!(rows[0]["status"], "\"open\"", "{answer}");
}

/// A binding for a placeholder the query does not declare is the caller's
/// mistake, over MCP too — matching Slice 4a's own acceptance criterion
/// that this is a validation error, not a `500`.
#[tokio::test]
async fn run_pack_query_reports_an_unknown_binding_as_a_tool_error_not_a_transport_error() {
    let (app, _container, _) = test_app().await;

    let (status, _) = send(
        &app,
        "POST",
        "/packs/gst-run-pack-query-negative/queries",
        Some(json!({
            "queries": [{"name": "q", "query": "ASK { <{{a}}> ?p ?o }"}],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let response = rpc(
        &app,
        "tools/call",
        json!({
            "name": "run_pack_query",
            "arguments": {
                "pack": "gst-run-pack-query-negative",
                "query": "q",
                "bindings": {"wrongName": "urn:x"},
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true, "{response}");
}
