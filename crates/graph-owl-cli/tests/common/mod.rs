//! One Postgres for the end-to-end test, shared with the rest of the
//! workspace by name — the same reuse the other crates use, so this test
//! adds no container of its own.

use std::sync::Arc;

use graph_owl_api::Catalog;
use testcontainers::{ContainerAsync, ImageExt, ReuseDirective, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

const POSTGRES_IMAGE_TAG: &str = "18-alpine";
/// The same name every other crate's harness uses, so all of them attach to
/// one server rather than starting one each.
const SHARED_CONTAINER: &str = "graph-owl-tests";

struct Shared {
    _container: ContainerAsync<Postgres>,
    admin_url: String,
}

static SHARED: tokio::sync::OnceCell<Shared> = tokio::sync::OnceCell::const_new();

pub struct TestDb {
    #[allow(dead_code)]
    name: String,
}

async fn shared() -> &'static Shared {
    SHARED
        .get_or_init(|| async {
            let container = Postgres::default()
                .with_tag(POSTGRES_IMAGE_TAG)
                .with_container_name(SHARED_CONTAINER)
                .with_reuse(ReuseDirective::Always)
                .start()
                .await
                .expect("postgres should start");
            let host = container.get_host().await.expect("host");
            let port = container.get_host_port_ipv4(5432).await.expect("port");
            Shared {
                _container: container,
                admin_url: format!("postgres://postgres:postgres@{host}:{port}/postgres"),
            }
        })
        .await
}

/// The real router over its own fresh database.
pub async fn test_app() -> (axum::Router, TestDb) {
    let shared = shared().await;
    let name = format!("t{}", uuid::Uuid::new_v4().simple());

    let admin = sqlx::PgPool::connect(&shared.admin_url)
        .await
        .expect("admin connect");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create database");
    admin.close().await;

    let (prefix, _) = shared
        .admin_url
        .rsplit_once('/')
        .expect("the admin URL names a database");
    let url = format!("{prefix}/{name}");

    // No JWT secret: open mode, so every request runs as the system
    // principal. This test is about payload shape, not authentication.
    unsafe { std::env::remove_var("GRAPH_OWL_JWT_SECRET") };

    let storage = graph_owl_storage_postgres::PostgresStorage::connect(&url)
        .await
        .expect("connect and migrate");
    let graph = graph_owl_engine_postgres::PostgresTripleStore::connect(&url)
        .await
        .expect("connect the graph engine");
    let graph = Arc::new(graph);
    let catalog = Catalog::new(Arc::new(storage))
        .with_graph(graph.clone())
        .with_traversal(graph);

    (graph_owl_server::app(catalog), TestDb { name })
}
