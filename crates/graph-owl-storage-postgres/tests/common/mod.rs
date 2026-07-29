//! One container per test binary, one database per test.
//!
//! A Postgres container costs about three seconds to start; creating a database
//! on an already-running server costs milliseconds, and gives the same
//! isolation — separate migrations, separate rows, no cross-talk.
//!
//! The container lives in a `OnceCell` that is never dropped, so it outlives
//! every test in the binary and testcontainers' reaper removes it when the
//! process exits.

use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};

/// Pinned, not defaulted: `Postgres::default()` is `postgres:11-alpine`, which
/// predates generated columns and every planner behaviour this project's design
/// notes assume. See `plans/00g-operations.md`.
const POSTGRES_IMAGE_TAG: &str = "18-alpine";

/// One name for the whole project, so every test binary and every run attaches
/// to the same server. Remove it by hand when you want a genuinely fresh one:
///
///     docker rm -f graph-owl-tests
const SHARED_CONTAINER: &str = "graph-owl-tests";

struct Shared {
    _container: ContainerAsync<Postgres>,
    admin_url: String,
}

static SHARED: tokio::sync::OnceCell<Shared> = tokio::sync::OnceCell::const_new();

/// A test's own database. Held by the caller for the test's duration.
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

/// A fresh database on the shared server, and its connection string.
#[allow(dead_code)]
pub async fn fresh_database() -> (TestDb, String) {
    let shared = SHARED
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
                .expect("postgres should start");
            let host = container.get_host().await.expect("host");
            let port = container.get_host_port_ipv4(5432).await.expect("port");
            Shared {
                _container: container,
                admin_url: format!("postgres://postgres:postgres@{host}:{port}/postgres"),
            }
        })
        .await;

    let name = format!("t{}", uuid::Uuid::new_v4().simple());
    let admin = sqlx::PgPool::connect(&shared.admin_url)
        .await
        .expect("connect to the shared server");

    sweep_stale_databases(&admin).await;
    // Not parameterised because an identifier cannot be bound — and the name is
    // a UUID this function generated, never anything a test supplied.
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create the test database");
    admin.close().await;

    // The *last* path segment. `replace("/postgres", …)` would rewrite the
    // scheme's own slashes in `postgres://` first.
    let (prefix, _) = shared
        .admin_url
        .rsplit_once('/')
        .expect("the admin URL names a database");
    (TestDb { name: name.clone() }, format!("{prefix}/{name}"))
}
