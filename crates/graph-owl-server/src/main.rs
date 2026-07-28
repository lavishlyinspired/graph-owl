use std::sync::Arc;

use graph_owl_api::Catalog;
use graph_owl_engine_postgres::PostgresTripleStore;
use graph_owl_storage_postgres::PostgresStorage;

#[tokio::main]
async fn main() {
    // Before anything that can fail, so a startup failure is itself structured
    // and correlatable rather than a bare panic on stderr.
    let _ = graph_owl_server::observability::install_logging();
    graph_owl_server::observability::metrics_handle();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    // Redacted at every mention. `DATABASE_URL` is the one value that is both
    // routinely logged — it names what a startup failure is about — and
    // routinely carries a credential.
    let where_from = graph_owl_server::observability::redact(&database_url);
    let storage = PostgresStorage::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect to postgres at {where_from}: {e}"));
    // The graph view of the same database. Its own migrations, its own tables.
    //
    // A failure here is fatal at *startup* — refusing to boot with a broken
    // graph is a different thing from failing a write when the graph goes down
    // later, which decision 6 forbids. Booting into a silently graph-less
    // catalog would make time-travel quietly return nothing, which is worse
    // than not starting.
    let graph = PostgresTripleStore::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect the graph engine at {where_from}: {e}"));
    // One backend, seen through both of its capabilities.
    let graph = Arc::new(graph);
    let catalog = Catalog::new(Arc::new(storage))
        .with_graph(graph.clone())
        .with_traversal(graph);

    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {bind}: {e}"));

    // A server running without authentication is a legitimate local posture and
    // an alarming production one. It says which it is at startup, because an
    // accidentally-open server must not look identical to a secured one.
    if std::env::var("GRAPH_OWL_JWT_SECRET").is_ok_and(|s| !s.is_empty()) {
        tracing::info!(%bind, database = %where_from, authentication = "enabled",
                       "graph-owl listening");
    } else {
        tracing::warn!(
            %bind, database = %where_from, authentication = "disabled",
            "graph-owl listening with authentication DISABLED — every request runs as \
             the system principal. Set GRAPH_OWL_JWT_SECRET to secure it."
        );
    }

    axum::serve(listener, graph_owl_server::app(catalog))
        // Drains in-flight requests rather than cutting them mid-write.
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutdown signal received; draining in-flight requests");
        })
        .await
        .expect("server error");
}
