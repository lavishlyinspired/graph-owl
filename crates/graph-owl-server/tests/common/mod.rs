use std::sync::Arc;

use graph_owl_api::Catalog;
use graph_owl_storage_postgres::PostgresStorage;
use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

pub async fn test_app() -> (axum::Router, ContainerAsync<Postgres>, String) {
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
    let catalog = Catalog::new(Arc::new(storage));

    (graph_owl_server::app(catalog), container, connection_string)
}

pub async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body");
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}
