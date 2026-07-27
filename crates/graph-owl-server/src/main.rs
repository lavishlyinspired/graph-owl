use std::sync::Arc;

use graph_owl_api::Catalog;
use graph_owl_storage_postgres::PostgresStorage;

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let storage = PostgresStorage::connect(&database_url)
        .await
        .expect("failed to connect to postgres");
    let catalog = Catalog::new(Arc::new(storage));

    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {bind}: {e}"));

    // A server running without authentication is a legitimate local posture and
    // an alarming production one. It says which it is at startup, because an
    // accidentally-open server must not look identical to a secured one.
    if std::env::var("GRAPH_OWL_JWT_SECRET").is_ok_and(|s| !s.is_empty()) {
        println!("graph-owl listening on {bind} (authentication: enabled)");
    } else {
        println!(
            "graph-owl listening on {bind} (authentication: DISABLED — every \
             request runs as the system principal. Set GRAPH_OWL_JWT_SECRET to secure it.)"
        );
    }

    axum::serve(listener, graph_owl_server::app(catalog))
        // Drains in-flight requests rather than cutting them mid-write.
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            println!("shutdown signal received; draining in-flight requests");
        })
        .await
        .expect("server error");
}
