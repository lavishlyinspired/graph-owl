//! Epic 14 Slice H: the stdio transport, proved two ways.
//!
//! `graph_owl_server::stdio::run_session`'s own unit tests already cover
//! framing exhaustively against a fake `ContextSource`. What only exists
//! once there is a real catalog and a real compiled binary behind it:
//!
//! 1. **Parity.** The same call, over both transports, against the *same*
//!    catalog — proving this crate's stdio wiring builds an equivalent
//!    `Server` (same reads, same writes, same budget), not merely that
//!    `jsonrpc::handle` behaves consistently when called directly.
//! 2. **The compiled binary itself.** `DATABASE_URL` and
//!    `GRAPH_OWL_MCP_STDIO_PRINCIPAL` really are read from the process
//!    environment, logging really does stay off stdout, and an unconfigured
//!    process really does refuse every call rather than running open.

mod common;

use std::process::Stdio;

use graph_owl_api::{Catalog, UpsertAsset};
use graph_owl_core::{AssetKind, Principal};
use graph_owl_mcp::ContextSource;
use graph_owl_mcp::catalog::{CatalogContext, CatalogWriter};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

async fn seeded_asset(catalog: &Catalog) -> String {
    catalog
        .upsert_asset(
            &Principal::system(),
            UpsertAsset {
                kind: AssetKind::Service,
                name: "warehouse".to_string(),
                parent_id: None,
                description: None,
                properties: None,
                extension: None,
            },
        )
        .await
        .expect("upsert_asset")
        .fully_qualified_name
}

fn get_asset_context_request(fqn: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": graph_owl_mcp::GET_ASSET_CONTEXT,
            "arguments": { "fullyQualifiedName": fqn }
        }
    })
}

/// **Parity.** `system` is what `test_catalog()`'s open-mode principal
/// resolves to over HTTP (`tests/mcp.rs`'s own `AGENT` constant), so the
/// stdio side is built with the identical principal for a fair comparison.
#[tokio::test]
async fn the_same_call_over_both_transports_gets_the_same_answer() {
    let (catalog, _container, _url) = common::test_catalog().await;
    let fqn = seeded_asset(&catalog).await;

    let http_app = graph_owl_server::app(catalog.clone());
    let http_response = {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(get_asset_context_request(&fqn).to_string()))
            .expect("request");
        let response = http_app.oneshot(request).await.expect("handled");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice::<Value>(&bytes).expect("json")
    };

    let principal = Principal::system();
    let reads = CatalogContext::new(catalog.clone(), principal.clone());
    let writes = CatalogWriter::new(catalog, principal.clone());
    let server = graph_owl_mcp::jsonrpc::Server {
        reads: &reads,
        writes: Some(&writes),
        budget: graph_owl_mcp::budget::TokenBudget::default(),
    };
    let mut stdio_out = Vec::new();
    let input = format!("{}\n", get_asset_context_request(&fqn));
    graph_owl_server::stdio::run_session(
        &server,
        Some(principal.id.as_str()),
        std::io::Cursor::new(input.into_bytes()),
        &mut stdio_out,
    )
    .await
    .expect("session completes");
    let stdio_response: Value =
        serde_json::from_slice(stdio_out.strip_suffix(b"\n").unwrap_or(&stdio_out)).expect("json");

    // Not a vacuous match on two equally-failed calls — both sides must
    // have genuinely found the asset, or a bug that broke *both* transports
    // identically would still pass this assertion.
    assert_eq!(http_response["result"]["isError"], false, "{http_response}");
    assert_eq!(
        http_response["result"], stdio_response["result"],
        "http: {http_response}\nstdio: {stdio_response}"
    );
}

/// **Epic 105 P10's `traverse()` tool, against the real adapter.** Every
/// other test in this file exercises `CatalogContext` through
/// `GET_ASSET_CONTEXT`; this is the one that proves `traverse` specifically
/// — that `CatalogContext::traverse` really does call the pre-existing,
/// already-authorized `Catalog::asset_subgraph` and really does convert its
/// `Subgraph` into a `TraversalContext`, rather than the unit-level
/// `Fixture` double in `graph-owl-mcp`'s own tests (which never touches the
/// real adapter at all).
///
/// **Deliberately a lone asset with no relationship.** There is no
/// asset-to-asset relationship-creation path in this codebase to seed a
/// multi-hop fixture with: the legacy `POST /tables/{id}/relationships`
/// route operates on the older `Table`/`Relationship` entity pair, a
/// separate id space from `Asset` (`00c`'s own documented gotcha — Epic
/// 31's fixtures hit exactly this), and `LINK_LINEAGE`
/// (`graph-owl-mcp::write`) is a declared write tool whose own test
/// (`linking_lineage_asks_for_the_capability_that_cannot_apply`) proves it
/// is not actually wired to write anything yet. Building that missing
/// capability is out of scope for wiring one read tool; recorded instead of
/// silently worked around. A lone asset still proves the real path: a BFS
/// walk always includes its own seed at depth zero, so `nodes` is not
/// empty — which is precisely what distinguishes a genuine call from the
/// `Ok(None)` and `Ok(Some(Default::default()))` mutants `cargo mutants`
/// found and a `--lib`-only run cannot see (same class of gap as
/// `observe`/`metrics_endpoint`, documented in `CLAUDE.md`).
#[tokio::test]
async fn traverse_reaches_the_real_catalog_through_the_real_adapter() {
    let (catalog, _container, _url) = common::test_catalog().await;
    let fqn = seeded_asset(&catalog).await;

    let principal = Principal::system();
    let reads = CatalogContext::new(catalog, principal.clone());

    let context = reads
        .traverse(&principal.id, &fqn, graph_owl_mcp::Direction::Upstream, 2)
        .await
        .expect("no source error")
        .expect("the seeded asset is visible to the system principal");

    assert_eq!(context.nodes.len(), 1, "{context:?}");
    assert!(!context.truncated, "{context:?}");
}

/// A real local `/embeddings`-style receiver isn't needed here — this is
/// the actual compiled `graph-owl-mcp-stdio` binary, spawned as a real
/// subprocess with real stdin/stdout pipes, matching this project's
/// standing "a real server, not a mock" testing precedent.
fn spawn_stdio(database_url: &str, principal: Option<&str>) -> tokio::process::Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_graph-owl-mcp-stdio"));
    command
        .env("DATABASE_URL", database_url)
        .env_remove("GRAPH_OWL_MCP_STDIO_PRINCIPAL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(id) = principal {
        command.env("GRAPH_OWL_MCP_STDIO_PRINCIPAL", id);
        // A freshly-provisioned principal has no roles and no policies, so
        // authorization denies by default — the same "successful sign-in,
        // empty catalog" trap `is_bootstrap_admin` exists to solve for
        // HTTP. Naming it here is what makes this a test of "a configured
        // principal can complete a real call", not accidentally a second
        // copy of the unconfigured-refuses test.
        command.env("GRAPH_OWL_ADMIN_SUBJECTS", id);
    }
    command.spawn().expect("spawn graph-owl-mcp-stdio")
}

async fn one_exchange(child: &mut tokio::process::Child, request: &Value) -> Value {
    let stdin = child.stdin.as_mut().expect("piped stdin");
    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("write request");
    stdin.flush().await.expect("flush");

    let stdout = child.stdout.as_mut().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("read response");
    serde_json::from_str(line.trim()).unwrap_or_else(|_| panic!("not JSON: {line:?}"))
}

/// **The acceptance criterion's own wording**: "a stdio server started
/// without a configured principal refuses every call rather than running
/// open." Proven against the real binary, not the injectable function
/// `stdio.rs`'s own tests already cover — this is what actually ships.
#[tokio::test]
async fn the_real_binary_refuses_every_call_when_unconfigured() {
    let (_catalog, _container, url) = common::test_catalog().await;
    let mut child = spawn_stdio(&url, None);

    let fqn = "warehouse"; // no asset need exist — the refusal happens before any lookup
    let response = one_exchange(&mut child, &get_asset_context_request(fqn)).await;

    assert_eq!(response["result"]["isError"], true, "{response}");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("authenticated"), "{text}");

    let _ = child.kill().await;
}

/// The positive: a configured binary really does connect to the real
/// database, resolve the principal, and answer — and its stdout's first
/// line is a JSON-RPC response, not a stray log line (proving logging
/// really did go to stderr).
#[tokio::test]
async fn the_real_binary_answers_when_configured() {
    let (catalog, _container, url) = common::test_catalog().await;
    let fqn = seeded_asset(&catalog).await;
    let mut child = spawn_stdio(&url, Some("stdio-test-agent"));

    let response = one_exchange(&mut child, &get_asset_context_request(&fqn)).await;

    assert_eq!(response["result"]["isError"], false, "{response}");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let payload: Value = serde_json::from_str(text).expect("payload is JSON");
    assert_eq!(
        payload["fullyQualifiedName"], fqn,
        "the real asset, not a placeholder: {payload}"
    );

    let _ = child.kill().await;
}
