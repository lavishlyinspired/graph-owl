//! Shared fixture for this crate's integration tests.
//!
//! **One container per test binary, one database per test.** Starting a
//! Postgres container per test cost about three seconds each; at 88 calls in
//! this crate that was the entire runtime of the suite. Creating a database on
//! an already-running server costs milliseconds, and gives the same isolation:
//! separate catalogs, separate migrations, no shared rows.
//!
//! The container is held in a `OnceCell` that is never dropped, so it outlives
//! every test in the binary. Testcontainers' reaper removes it when the process
//! exits — the same guarantee the per-test handles gave, arrived at once
//! instead of 88 times.

use std::sync::Arc;

use graph_owl_api::Catalog;
use graph_owl_storage_postgres::PostgresStorage;
use serde_json::Value;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

/// The one container this test binary uses.
struct Shared {
    /// Held so the container outlives the tests. Never read.
    _container: ContainerAsync<Postgres>,
    /// Connection string for the server's default database, used only to
    /// `CREATE DATABASE` for each test.
    admin_url: String,
}

static SHARED: tokio::sync::OnceCell<Shared> = tokio::sync::OnceCell::const_new();

async fn shared() -> &'static Shared {
    SHARED
        .get_or_init(|| async {
            let container = Postgres::default()
                .with_tag(POSTGRES_IMAGE_TAG)
                // **Reused across binaries and across runs**, not just within
                // one. A fixed name plus `Always` means the second `cargo test`
                // of the day attaches to the container the first one started
                // instead of paying to boot another, and the 30-odd test
                // binaries in this workspace share one server rather than
                // starting one each.
                //
                // It also fixes a leak this fixture introduced: the handle
                // lives in a `OnceCell` that never drops, so the cleanup that
                // used to run per test never ran, and containers accumulated
                // one per binary per run. 146 of them were found running at
                // once, which had quietly tripled the suite's wall time — a
                // reused container cannot accumulate, because there is only
                // ever the one.
                .with_container_name(SHARED_CONTAINER)
                .with_reuse(testcontainers::ReuseDirective::Always)
                .start()
                .await
                .expect("failed to start postgres container");
            let host = container.get_host().await.expect("failed to get host");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("failed to get mapped port");
            Shared {
                _container: container,
                admin_url: format!("postgres://postgres:postgres@{host}:{port}/postgres"),
            }
        })
        .await
}

/// A test's own database, dropped when the test ends.
///
/// Returned in the position the container handle used to occupy, so call sites
/// that bind it as `_container` and never touch it keep working. It is a real
/// guard rather than a unit: holding it is what documents that the database
/// belongs to this test.
pub struct TestDb {
    #[allow(dead_code)]
    name: String,
}

/// A fresh database on the shared server.
///
/// The name is a UUID rather than a counter: test binaries run concurrently
/// against the same container only if a future change shares one between them,
/// and a counter would collide silently the first time that happened.
async fn fresh_database() -> (TestDb, String) {
    let shared = shared().await;
    let name = format!("t{}", uuid::Uuid::new_v4().simple());

    let admin = sqlx::PgPool::connect(&shared.admin_url)
        .await
        .expect("connect to the shared server");
    // Not parameterised because an identifier cannot be bound — and the name is
    // a UUID this function generated, never anything a test supplied.
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create the test database");
    admin.close().await;

    // Swap the *last* path segment. `replace("/postgres", …)` would rewrite the
    // scheme's own slashes in `postgres://` first, which produces a URL that
    // fails to parse in a way that looks like a container problem.
    let (prefix, _) = shared
        .admin_url
        .rsplit_once('/')
        .expect("the admin URL names a database");
    (TestDb { name: name.clone() }, format!("{prefix}/{name}"))
}

/// Pinned, not defaulted: `Postgres::default()` is `postgres:11-alpine`, which
/// predates generated columns and every planner behaviour this project's design
/// notes assume.
///
/// The **major** is pinned and the minor floats, so a security release arrives
/// without a manual bump while a major upgrade stays a deliberate decision.
/// See `plans/00g-operations.md`, "Supported PostgreSQL versions".
const POSTGRES_IMAGE_TAG: &str = "18-alpine";

/// One name for the whole project, so every test binary and every run attaches
/// to the same server. Remove it by hand when you want a genuinely fresh one:
///
///     docker rm -f graph-owl-tests
const SHARED_CONTAINER: &str = "graph-owl-tests";

#[allow(dead_code)]
pub async fn test_app() -> (axum::Router, TestDb, String) {
    build_app(None).await
}

/// Same, with JWT verification enabled. The secret is process-wide because it
/// is read from the environment, which is where a deployment supplies it.
#[allow(dead_code)]
pub async fn test_app_with_secret(secret: &str) -> (axum::Router, TestDb, String) {
    build_app(Some(secret)).await
}

/// Same, with admission limits the test chooses.
///
/// The caller keeps the `Arc`, which is what makes the rejection tests
/// deterministic: a permit held directly by the test needs no second in-flight
/// request and therefore no sleep, no timing window, and no flake.
#[allow(dead_code)]
pub async fn test_app_with_admission(
    admission: &Arc<graph_owl_server::admission::Admission>,
) -> (axum::Router, TestDb, String) {
    let (container, connection_string, catalog) = build_catalog(None).await;
    (
        graph_owl_server::app_with_admission(catalog, Arc::clone(admission)),
        container,
        connection_string,
    )
}

async fn build_app(secret: Option<&str>) -> (axum::Router, TestDb, String) {
    let (container, connection_string, catalog) = build_catalog(secret).await;
    (graph_owl_server::app(catalog), container, connection_string)
}

async fn build_catalog(secret: Option<&str>) -> (TestDb, String, Catalog) {
    match secret {
        Some(secret) => unsafe { std::env::set_var("GRAPH_OWL_JWT_SECRET", secret) },
        None => unsafe { std::env::remove_var("GRAPH_OWL_JWT_SECRET") },
    }
    let (database, connection_string) = fresh_database().await;

    let storage = PostgresStorage::connect(&connection_string)
        .await
        .expect("failed to connect and migrate");
    // The graph engine, wired the same way the composition root wires it —
    // otherwise the tests would exercise a catalog shape that never ships.
    let graph = graph_owl_engine_postgres::PostgresTripleStore::connect(&connection_string)
        .await
        .expect("failed to connect the graph engine");
    let graph = Arc::new(graph);
    let catalog = Catalog::new(Arc::new(storage))
        .with_graph(graph.clone())
        .with_traversal(graph);

    (database, connection_string, catalog)
}

#[allow(dead_code)]
pub async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body");
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}
