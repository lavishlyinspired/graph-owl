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

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind to 0.0.0.0:8080");
    axum::serve(listener, graph_owl_server::app(catalog))
        .await
        .expect("server error");
}
