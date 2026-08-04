//! Epic 37c Slice C: the catalog embedded with no server.
//!
//! `cargo run -p graph-owl-api --example embedded`. Set `DATABASE_URL` to
//! run the same code against Postgres instead — the backend construction
//! below is the only line that differs; everything after it is `Arc<dyn
//! Storage>` either way.

use std::sync::Arc;

use graph_owl_api::{Catalog, UpsertAsset};
use graph_owl_core::{AssetKind, Principal};
use graph_owl_storage::Storage;
use graph_owl_storage_memory::InMemoryStorage;
use graph_owl_storage_postgres::PostgresStorage;

fn asset(name: &str, description: &str) -> UpsertAsset {
    UpsertAsset {
        kind: AssetKind::Service,
        name: name.to_string(),
        parent_id: None,
        description: Some(description.to_string()),
        properties: None,
        extension: None,
    }
}

#[tokio::main]
async fn main() {
    let storage: Arc<dyn Storage> = match std::env::var("DATABASE_URL") {
        Ok(url) => Arc::new(PostgresStorage::connect(&url).await.expect("Postgres")),
        Err(_) => Arc::new(InMemoryStorage::default()),
    };
    let catalog = Catalog::new(storage);
    let system = Principal::system();

    let created = catalog
        .upsert_asset(&system, asset("orders-api", "Order placement"))
        .await
        .expect("a root-kind asset needs no parent");
    println!("created {} ({})", created.fully_qualified_name, created.id);

    let found = catalog
        .get_asset_by_fqn(&created.fully_qualified_name)
        .await
        .expect("lookup should not error");
    assert_eq!(found.map(|a| a.id), Some(created.id));

    let updated = catalog
        .upsert_asset(&system, asset("orders-api", "Order placement, returns"))
        .await
        .expect("the same FQN updates rather than duplicates");
    assert_eq!(updated.id, created.id, "same FQN, no duplicate");
    println!("updated description: {:?}", updated.description);

    let missing = catalog.get_asset(uuid::Uuid::new_v4()).await;
    assert!(
        matches!(missing, Ok(None)),
        "a random id resolves to nothing"
    );
}
