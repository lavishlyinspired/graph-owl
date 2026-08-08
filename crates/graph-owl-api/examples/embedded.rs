//! Epic 37c Slice C: the catalog embedded with no server.
//! Epic 37c Slice F: extended to a second entity family (Epic 34 Slice B's
//! messaging family) to prove the embedding surface does not grow with the
//! entity model — see the module doc comment further down for what that
//! proves and what it does not.
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
use uuid::Uuid;

fn asset(kind: AssetKind, name: &str, parent_id: Option<Uuid>, description: &str) -> UpsertAsset {
    UpsertAsset {
        kind,
        name: name.to_string(),
        parent_id,
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
        .upsert_asset(
            &system,
            asset(AssetKind::Service, "orders-api", None, "Order placement"),
        )
        .await
        .expect("a root-kind asset needs no parent");
    println!("created {} ({})", created.fully_qualified_name, created.id);

    let found = catalog
        .get_asset_by_fqn(&created.fully_qualified_name)
        .await
        .expect("lookup should not error");
    assert_eq!(found.map(|a| a.id), Some(created.id));

    let updated = catalog
        .upsert_asset(
            &system,
            asset(
                AssetKind::Service,
                "orders-api",
                None,
                "Order placement, returns",
            ),
        )
        .await
        .expect("the same FQN updates rather than duplicates");
    assert_eq!(updated.id, created.id, "same FQN, no duplicate");
    println!("updated description: {:?}", updated.description);

    let missing = catalog.get_asset(Uuid::new_v4()).await;
    assert!(
        matches!(missing, Ok(None)),
        "a random id resolves to nothing"
    );

    // A second entity family (Epic 34 Slice B's messaging family, added by
    // this crate a full epic after `Service`/Slice C above was written).
    // `MessagingService` is root-kind exactly like `Service`, but `Topic`
    // *requires* a `MessagingService` parent — exercising `parent_id`,
    // which the walkthrough above never does, since `Service` has none.
    //
    // What this proves: adding a whole new entity family needed no new
    // public type — `UpsertAsset`, `AssetKind::MessagingService/Topic` and
    // `Catalog::{upsert_asset,list_children}` are the same surface used
    // above, just parameterized differently.
    //
    // What this does NOT prove, and should not be read as proving: that
    // `Storage`'s growth across every epic never forced an adapter to
    // implement a method it does not use. Both adapters here (`InMemoryStorage`,
    // `PostgresStorage`) already implement the *whole* trait; a family added
    // after this example was last touched cannot by construction demonstrate
    // whether growing the trait was painless for them to keep up with.
    let broker = catalog
        .upsert_asset(
            &system,
            asset(AssetKind::MessagingService, "kafka", None, "Event backbone"),
        )
        .await
        .expect("a root-kind asset needs no parent");
    println!("created {} ({})", broker.fully_qualified_name, broker.id);

    let topic = catalog
        .upsert_asset(
            &system,
            asset(
                AssetKind::Topic,
                "orders-placed",
                Some(broker.id),
                "Order-placed events",
            ),
        )
        .await
        .expect("Topic's declared parent kind is MessagingService");
    println!("created {} ({})", topic.fully_qualified_name, topic.id);

    let found_topic = catalog
        .get_asset_by_fqn(&topic.fully_qualified_name)
        .await
        .expect("lookup should not error");
    assert_eq!(found_topic.map(|a| a.id), Some(topic.id));

    let broker_children = catalog
        .list_children(Some(broker.id))
        .await
        .expect("listing children should not error");
    assert_eq!(
        broker_children.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![topic.id],
        "the topic is the broker's only child"
    );
}
