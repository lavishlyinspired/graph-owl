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
                // Docker defaults `/dev/shm` to 64 MB. Postgres sizes parallel
                // query and hash-join segments against it, and a shared server
                // holding many databases exhausts it — surfacing as "could not
                // resize shared memory segment... No space left on device" in
                // whichever test happened to be running, which reads like a
                // disk problem and is not one.
                .with_shm_size(256 * 1024 * 1024)
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

/// Drop databases left by earlier runs, once per process.
///
/// A reused container keeps its databases forever, and each test makes one.
/// After a few hundred runs that is thousands of databases, and Postgres
/// exhausts `/dev/shm` long before it exhausts disk — the failure arrives as
/// "could not resize shared memory segment" in an unrelated test.
///
/// Only databases with **no active connections** are dropped, so a binary
/// running concurrently cannot have its own database pulled out from under it.
/// Failures are ignored: this is housekeeping, and a test suite that refused to
/// run because it could not tidy up would be worse than an untidy server.
async fn sweep_stale_databases(admin: &sqlx::PgPool) {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    let mut should = false;
    SWEPT.call_once(|| should = true);
    if !should {
        return;
    }

    let Ok(stale) = sqlx::query_scalar::<_, String>(
        "SELECT datname FROM pg_database d
          WHERE datname LIKE 't%'
            AND NOT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname = d.datname)",
    )
    .fetch_all(admin)
    .await
    else {
        return;
    };

    for database in stale {
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {database}"))
            .execute(admin)
            .await;
    }
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

    // **This call went missing once.** The sweep was defined here and never
    // invoked, so 197 databases accumulated before anyone noticed — and the
    // symptom was a slow suite, which points anywhere but at a dead function.
    sweep_stale_databases(&admin).await;

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

/// The raw `Catalog`, for a test that needs to drive something below the
/// HTTP surface directly — Epic 19 Slice B's kill-and-restart test needs
/// precise control over how many messages get applied before it simulates a
/// crash, which the router's fire-and-forget background spawn does not
/// offer from outside the crate.
#[allow(dead_code)]
pub async fn test_catalog() -> (Catalog, TestDb, String) {
    let (container, connection_string, catalog) = build_catalog(None).await;
    (catalog, container, connection_string)
}

/// Same, with JWT verification enabled — for a test that needs to drive
/// something below the HTTP surface (Epic 7d's Bolt tests: `HELLO` carries a
/// bearer token directly, with no HTTP request to extract it from) while
/// still authenticating against the identical `GRAPH_OWL_JWT_SECRET` an HTTP
/// test in the same binary would.
#[allow(dead_code)]
pub async fn test_catalog_with_secret(secret: &str) -> (Catalog, TestDb, String) {
    let (container, connection_string, catalog) = build_catalog(Some(secret)).await;
    (catalog, container, connection_string)
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
        .with_traversal(graph.clone())
        .with_namespaces(graph);

    (database, connection_string, catalog)
}

#[allow(dead_code)]
pub async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read body");
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}

/// The secret [`authorization_fixture`] signs tokens with — exported so a
/// caller past the HTTP surface (Epic 7d's Bolt tests, which authenticate a
/// raw `HELLO` rather than an HTTP header) can mint a compatible token via
/// [`token`].
#[allow(dead_code)]
pub const AUTHZ_FIXTURE_SECRET: &str = "demo-signing-secret-not-for-production";

#[allow(dead_code)]
pub fn token(subject: &str) -> String {
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        sub: &'a str,
        name: &'a str,
        exp: usize,
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &Claims {
            sub: subject,
            name: subject,
            exp: 4_102_444_800, // year 2100
        },
        &jsonwebtoken::EncodingKey::from_secret(AUTHZ_FIXTURE_SECRET.as_bytes()),
    )
    .expect("token should encode")
}

#[allow(dead_code)]
pub async fn call(
    app: &axum::Router,
    uri: &str,
    subject: Option<&str>,
) -> (axum::http::StatusCode, Value) {
    let mut builder = axum::http::Request::builder().method("GET").uri(uri);
    if let Some(subject) = subject {
        builder = builder.header("authorization", format!("Bearer {}", token(subject)));
    }
    let response =
        <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
            app.clone(),
            builder
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    let status = response.status();
    (status, json_body(response).await)
}

/// A bank estate with one schema that must not be readable by everyone.
#[allow(dead_code)]
async fn seed_banking_estate(connection_string: &str) {
    let pool = sqlx::PgPool::connect(connection_string)
        .await
        .expect("source connection");
    for statement in [
        "CREATE SCHEMA IF NOT EXISTS core_banking",
        "CREATE SCHEMA IF NOT EXISTS payments",
        "CREATE TABLE IF NOT EXISTS core_banking.customers (
             customer_id BIGINT PRIMARY KEY, pan CHAR(10), aadhaar_last4 CHAR(4))",
        "CREATE TABLE IF NOT EXISTS payments.upi_transactions (
             txn_id TEXT PRIMARY KEY, amount NUMERIC(18,2) NOT NULL)",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .expect("seed statement");
    }
}

/// A bank estate, two users (`root` an admin, `asha` a risk analyst denied
/// the PII-bearing customer master), and a live [`Catalog`] alongside the
/// HTTP router built from that identical catalog — moved here from
/// `authorization.rs` (Epic 13) so Epic 7d's Bolt tests can spawn a
/// `BoltServer` against the exact same authorized data the HTTP router
/// answers from, rather than a second fixture that could quietly drift from
/// it.
#[allow(dead_code)]
pub async fn authorization_fixture() -> (axum::Router, TestDb, Catalog) {
    let (catalog, container, connection_string) =
        test_catalog_with_secret(AUTHZ_FIXTURE_SECRET).await;
    seed_banking_estate(&connection_string).await;
    let app = graph_owl_server::app(catalog.clone());

    // Catalogue as the admin.
    let response =
        <axum::Router as tower::ServiceExt<axum::http::Request<axum::body::Body>>>::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/connectors/postgres/runs")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token("root")))
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "connectionString": connection_string,
                        "serviceName": "hdfc-core",
                        "includeSchemas": ["core_banking", "payments"]
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should be handled");
    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "catalogue must succeed"
    );

    let database = connection_string
        .rsplit_once('/')
        .expect("the connection string names a database")
        .1
        .to_string();

    let pool = sqlx::PgPool::connect(&connection_string)
        .await
        .expect("catalog connection");

    sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = 'root'")
        .execute(&pool)
        .await
        .expect("promote root");

    let rules = serde_json::json!([
        {
            "name": "read-catalog",
            "effect": "allow",
            "operations": ["viewBasic", "viewDetails"],
            "resources": { "type": "all" }
        },
        {
            "name": "no-customer-pii",
            "effect": "deny",
            "operations": ["viewBasic", "viewDetails"],
            "resources": {
                "type": "fqnPrefix",
                "value": format!("hdfc-core.{database}.core_banking")
            }
        }
    ]);
    sqlx::query("INSERT INTO roles (name) VALUES ('risk-analyst') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .expect("role insert");
    sqlx::query("INSERT INTO policies (name, rules) VALUES ('analyst-baseline', $1)")
        .bind(&rules)
        .execute(&pool)
        .await
        .expect("policy insert");
    sqlx::query(
        "INSERT INTO role_policies (role, policy) VALUES ('risk-analyst', 'analyst-baseline')
         ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("role-policy link");

    // First request auto-provisions asha; then grant the role.
    call(&app, "/assets/stats", Some("asha")).await;
    sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ('asha', 'risk-analyst') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .expect("grant role");

    (app, container, catalog)
}

/// Epic 19 Slice A's end-to-end test needs a real broker alongside Postgres.
/// One container per test binary, the same reasoning as the Postgres one
/// above — started via the crate-local `testcontainers-kafka` /
/// `testcontainers-modules-kafka` pin (see `Cargo.toml`), independent of the
/// workspace-wide `testcontainers`/`testcontainers-modules` used for
/// Postgres.
static SHARED_KAFKA: tokio::sync::OnceCell<
    testcontainers_kafka::ContainerAsync<testcontainers_modules_kafka::kafka::apache::Kafka>,
> = tokio::sync::OnceCell::const_new();

#[allow(dead_code)]
pub async fn kafka_bootstrap_servers() -> String {
    use testcontainers_kafka::runners::AsyncRunner;
    use testcontainers_kafka::{ImageExt, ReuseDirective};
    let container = SHARED_KAFKA
        .get_or_init(|| async {
            testcontainers_modules_kafka::kafka::apache::Kafka::default()
                // Named + reused, same reasoning as `SHARED_CONTAINER` above:
                // a `OnceCell` never drops, so an anonymous container is
                // never reaped and one accumulates per binary per run.
                .with_container_name("graph-owl-kafka-tests")
                .with_reuse(ReuseDirective::Always)
                .start()
                .await
                .expect("kafka container should start")
        })
        .await;
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(9092)
        .await
        .expect("container port");
    format!("{host}:{port}")
}

#[allow(dead_code)]
pub fn unique_topic() -> String {
    format!("test-{}", uuid::Uuid::new_v4())
}
