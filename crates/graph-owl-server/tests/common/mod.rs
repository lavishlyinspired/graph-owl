use std::sync::Arc;

use graph_owl_api::Catalog;
use graph_owl_storage_postgres::PostgresStorage;
use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

pub async fn test_app() -> (axum::Router, ContainerAsync<Postgres>, String) {
    build_app(None).await
}

/// Same, with JWT verification enabled. The secret is process-wide because it
/// is read from the environment, which is where a deployment supplies it.
#[allow(dead_code)]
pub async fn test_app_with_secret(
    secret: &str,
) -> (axum::Router, ContainerAsync<Postgres>, String) {
    build_app(Some(secret)).await
}

async fn build_app(secret: Option<&str>) -> (axum::Router, ContainerAsync<Postgres>, String) {
    match secret {
        Some(secret) => unsafe { std::env::set_var("GRAPH_OWL_JWT_SECRET", secret) },
        None => unsafe { std::env::remove_var("GRAPH_OWL_JWT_SECRET") },
    }
    let container = Postgres::default()
        .start()
        .await
        .expect("failed to start postgres container");
    let host = container.get_host().await.expect("failed to get host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get mapped port");
    let connection_string = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    // The graph engine, wired the same way the composition root wires it —
    // otherwise the tests would exercise a catalog shape that never ships.
    let graph = graph_owl_engine_postgres::PostgresTripleStore::connect(&connection_string)
        .await
        .expect("failed to connect the graph engine");
    let catalog = Catalog::new(Arc::new(storage)).with_graph(Arc::new(graph));

    (graph_owl_server::app(catalog), container, connection_string)
}

pub async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body");
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}
