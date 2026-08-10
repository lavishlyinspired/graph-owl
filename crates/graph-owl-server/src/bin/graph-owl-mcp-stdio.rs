//! MCP over stdio — Epic 14 Slice H.
//!
//! A **second, independent process**, not a flag on the main server: stdio
//! MCP clients (Claude Desktop, most IDE integrations) spawn one process per
//! session and speak to it purely over stdin/stdout, which is exactly the
//! stream a shared process's own log output would corrupt. `POST /mcp`
//! (`graph-owl-server`, unchanged by this file) remains the transport for
//! everything else.
//!
//! **Minimal by design, not by oversight.** This binary wires only what the
//! declared MCP tools need — storage, the graph, traversal. It does not spawn
//! `OutboundWebhookSender` or resume Epic 19's streaming subscriptions: both
//! are singleton background jobs, and a second copy racing the main server's
//! against the same tables would mean duplicate deliveries or duplicate
//! consumption, not extra capacity. The main server already owns them.
//!
//! **Logging goes to stderr, never stdout.** stdout is the JSON-RPC wire —
//! the one thing [`graph_owl_server::observability::install_logging`]
//! cannot be reused for, since it defaults to stdout for every other binary
//! in this workspace, where that default is correct.

use std::sync::Arc;

use graph_owl_api::Catalog;
use graph_owl_engine_postgres::PostgresTripleStore;
use graph_owl_mcp::catalog::{CatalogContext, CatalogWriter};
use graph_owl_storage::Storage;
use graph_owl_storage_postgres::PostgresStorage;
use tokio::io::BufReader;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    // Same filter convention as the HTTP server's own `install_logging`,
    // deliberately reimplemented here rather than shared — the one thing
    // that must differ is the writer, and duplicating four lines is cheaper
    // than threading a writer parameter through a function every other
    // binary calls with none.
    let filter = EnvFilter::try_from_env("LOG_LEVEL").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let where_from = graph_owl_server::observability::redact(&database_url);
    let storage = PostgresStorage::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect to postgres at {where_from}: {e}"));
    let graph = PostgresTripleStore::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect the graph engine at {where_from}: {e}"));
    let graph = Arc::new(graph);
    let storage: Arc<dyn Storage> = Arc::new(storage);
    let catalog = Catalog::new(storage)
        .with_graph(graph.clone())
        .with_traversal(graph.clone())
        .with_namespaces(graph.clone())
        .with_predicates(graph);

    // **Explicit, resolved once, and closed by default.** Unlike HTTP's
    // per-request `Auth` extractor, stdio has no header to carry a
    // credential on every message — the operator names who this *process*
    // acts as, or the process acts as nobody. `who` ends up `None` in the
    // unconfigured case, and `run_session` (via `jsonrpc::handle`) already
    // refuses every tool call against `None` — proven in `stdio.rs`'s own
    // tests. There is no second "is this allowed" check to add here.
    let principal_id = std::env::var("GRAPH_OWL_MCP_STDIO_PRINCIPAL").ok();
    let principal = match &principal_id {
        Some(id) => {
            let mut resolved = catalog.resolve_principal(id, id).await.unwrap_or_else(|e| {
                panic!("failed to resolve GRAPH_OWL_MCP_STDIO_PRINCIPAL: {e:?}")
            });
            // **Same bootstrap-admin mechanism HTTP already uses**
            // (`graph_owl_server::is_bootstrap_admin`), not a second one
            // invented for this transport. Without it, a freshly-configured
            // stdio principal is auto-provisioned with no roles and
            // authorization denies by default — the exact "successful
            // sign-in, empty catalog" failure that function's own doc
            // comment names for HTTP, reachable here by the identical
            // path. Re-evaluated from the environment on every process
            // start, never written back, for the same reason: removing the
            // variable revokes it.
            if graph_owl_server::is_bootstrap_admin(
                id,
                &std::env::var("GRAPH_OWL_ADMIN_SUBJECTS").unwrap_or_default(),
            ) {
                resolved.is_admin = true;
            }
            resolved
        }
        // Never reached through `who`, which stays `None` below — this
        // exists only because `CatalogContext`/`CatalogWriter` need *some*
        // `Principal` value to hold, and constructing one costs nothing
        // (no I/O, no row written) precisely because it is never used.
        None => graph_owl_core::Principal::system(),
    };
    if principal_id.is_none() {
        tracing::warn!(
            "GRAPH_OWL_MCP_STDIO_PRINCIPAL is not set — every tool call this session \
             receives will be refused as unauthenticated. Set it to the id of a \
             provisioned principal to allow calls through."
        );
    }

    let reads = CatalogContext::new(catalog.clone(), principal.clone());
    let writes = CatalogWriter::new(catalog, principal);
    let server = graph_owl_mcp::jsonrpc::Server {
        reads: &reads,
        writes: Some(&writes),
        budget: graph_owl_mcp::budget::TokenBudget::default(),
    };

    let (stdin, stdout) = rmcp::transport::io::stdio();
    graph_owl_server::stdio::run_session(
        &server,
        principal_id.as_deref(),
        BufReader::new(stdin),
        stdout,
    )
    .await
    .unwrap_or_else(|e| panic!("stdio session ended in error: {e}"));
}
