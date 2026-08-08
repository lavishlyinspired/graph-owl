use std::sync::Arc;
use std::time::Duration;

use graph_owl_api::Catalog;
use graph_owl_engine_postgres::PostgresTripleStore;
use graph_owl_storage_postgres::PostgresStorage;

#[tokio::main]
async fn main() {
    // Loaded before anything reads the environment, and **never overriding a
    // variable that is already set** — which is `dotenvy`'s behaviour and the
    // property that matters. A `.env` committed to a developer's machine must
    // not be able to quietly beat what an orchestrator injected, or a
    // production deployment inherits a laptop's database URL from a file
    // somebody forgot was there.
    //
    // A missing file is not an error: production supplies real environment
    // variables and has no `.env` at all.
    let _ = dotenvy::dotenv();

    // Before anything that can fail, so a startup failure is itself structured
    // and correlatable rather than a bare panic on stderr.
    let _ = graph_owl_server::observability::install_logging();
    graph_owl_server::observability::metrics_handle();

    // Before the database connection, deliberately. A configuration fault
    // should be reported as a configuration fault, not discovered after a
    // connection attempt that may itself fail for unrelated reasons and hide
    // it.
    let budget = match graph_owl_server::budget::Budget::from_env(|name| std::env::var(name).ok()) {
        Ok(budget) => budget,
        Err(error) => {
            tracing::error!("{error}");
            std::process::exit(78); // EX_CONFIG
        }
    };
    let limit =
        graph_owl_server::budget::container_limit(|path| std::fs::read_to_string(path).ok());
    if let Err(refusal) = graph_owl_server::budget::report(&budget, limit) {
        tracing::error!("{refusal}");
        std::process::exit(78);
    }

    // Read here for the same reason the budget is: a configuration fault is
    // reported as a configuration fault, at startup, naming the variable —
    // rather than surfacing later as a path that refuses every request for a
    // reason nothing on the wire explains.
    let admission =
        match graph_owl_server::admission::Admission::from_env(|name| std::env::var(name).ok()) {
            Ok(admission) => Arc::new(admission),
            Err(error) => {
                tracing::error!("{error}");
                std::process::exit(78); // EX_CONFIG
            }
        };
    for class in graph_owl_server::admission::Class::ALL {
        tracing::info!(
            class = class.label(),
            permits = admission.limit(*class),
            variable = class.env(),
            "admission control: requests beyond this limit are refused, not queued"
        );
    }

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    // Redacted at every mention. `DATABASE_URL` is the one value that is both
    // routinely logged — it names what a startup failure is about — and
    // routinely carries a credential.
    let where_from = graph_owl_server::observability::redact(&database_url);
    let storage = PostgresStorage::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect to postgres at {where_from}: {e}"));
    // The graph view of the same database. Its own migrations, its own tables.
    //
    // A failure here is fatal at *startup* — refusing to boot with a broken
    // graph is a different thing from failing a write when the graph goes down
    // later, which decision 6 forbids. Booting into a silently graph-less
    // catalog would make time-travel quietly return nothing, which is worse
    // than not starting.
    let graph = PostgresTripleStore::connect(&database_url)
        .await
        .unwrap_or_else(|e| panic!("failed to connect the graph engine at {where_from}: {e}"));
    // One backend, seen through both of its capabilities.
    let graph = Arc::new(graph);
    // Epic 101: comma-separated allow-list, off by default like every other
    // deployment-level capability here. No error case to report on a bad
    // value — see `endpoints_from_env`'s own doc comment — so unlike the
    // budget and admission config above, a malformed list cannot fail startup.
    let federation_endpoints = graph_owl_api::federation::endpoints_from_env(
        std::env::var("GRAPH_OWL_FEDERATION_ENDPOINTS")
            .ok()
            .as_deref(),
    );
    let federation_timeout = match std::env::var("GRAPH_OWL_FEDERATION_TIMEOUT_MS") {
        Ok(raw) => Duration::from_millis(raw.trim().parse().unwrap_or_else(|_| {
            panic!(
                "GRAPH_OWL_FEDERATION_TIMEOUT_MS must be a whole number of milliseconds, got {raw:?}"
            )
        })),
        Err(_) => graph_owl_api::federation::DEFAULT_TIMEOUT,
    };

    let mut catalog = Catalog::new(Arc::new(storage))
        .with_graph(graph.clone())
        .with_traversal(graph)
        .with_federation_endpoints(federation_endpoints)
        .with_federation_timeout(federation_timeout);
    // Epic 98: off by default, like every other deployment-level capability
    // here. `Catalog::classify_ontology`/`explain_subsumption` existed and
    // were tested since this epic shipped, but nothing ever called
    // `with_el_sidecar` outside a test — found wiring
    // `plans/EPIC-COMPLETION-PLAN.md` Phase 1.3. A bare name (`"whelk"`,
    // resolved via `PATH`) or an absolute path both work, matching
    // `SidecarConfig::binary`'s own doc comment.
    if let Ok(binary) = std::env::var("GRAPH_OWL_EL_SIDECAR") {
        catalog = catalog.with_el_sidecar(graph_owl_reasoning_el::SidecarConfig {
            binary: std::path::PathBuf::from(binary),
            budget: graph_owl_reasoning_el::ElBudget::default(),
        });
    }

    // Resumes every enabled subscription from its last committed offset —
    // Epic 19 decision 1 ("a durable subscription, not a push endpoint")
    // means a restart must not go silently quiet until someone re-registers
    // it. A failure here is logged, not fatal: a broker being unreachable at
    // boot must not stop the catalog itself from serving the rest of its
    // traffic.
    if let Err(error) = graph_owl_server::streaming::spawn_enabled_subscriptions(&catalog).await {
        tracing::error!("failed to resume streaming subscriptions at startup: {error:?}");
    }

    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind to {bind}: {e}"));

    // Epic 7d, decision 3: a second listening port, present only when this
    // binary was built with `--features bolt` *and* `BOLT_BIND_ADDR` is set —
    // compiled out entirely otherwise, not merely inert.
    #[cfg(feature = "bolt")]
    let bolt_listener = match std::env::var("BOLT_BIND_ADDR") {
        Ok(bolt_bind) => {
            let listener = tokio::net::TcpListener::bind(&bolt_bind)
                .await
                .unwrap_or_else(|e| panic!("failed to bind Bolt listener to {bolt_bind}: {e}"));
            tracing::info!(bind = %bolt_bind, "Bolt protocol server listening");
            Some((listener, bolt_bind))
        }
        Err(_) => None,
    };

    // A server running without authentication is a legitimate local posture and
    // an alarming production one. It says which it is at startup, because an
    // accidentally-open server must not look identical to a secured one.
    //
    // Resolved by the same function the request path uses, not by a second
    // chain of `if`s. Two independent readings of the same environment are two
    // things that can disagree — and the way they disagree is that the log says
    // "oidc" while requests are verified against a shared secret.
    let has_secret = std::env::var("GRAPH_OWL_JWT_SECRET").is_ok_and(|s| !s.is_empty());
    let has_oidc = std::env::var("OIDC_ISSUER").is_ok_and(|s| !s.is_empty());

    if graph_owl_server::is_ambiguous_auth_config(has_secret, has_oidc) {
        tracing::warn!(
            "both GRAPH_OWL_JWT_SECRET and OIDC_ISSUER are set. OIDC is in use and the \
             shared secret is ignored — but it is still a live credential anyone who \
             holds it believes works. Remove GRAPH_OWL_JWT_SECRET."
        );
    }

    match graph_owl_server::auth_mode(has_secret, has_oidc) {
        graph_owl_server::AuthMode::Oidc => tracing::info!(
            %bind, database = %where_from, authentication = "oidc",
            issuer = %std::env::var("OIDC_ISSUER").unwrap_or_default(),
            "graph-owl listening"
        ),
        graph_owl_server::AuthMode::SharedSecret => tracing::info!(
            %bind, database = %where_from, authentication = "shared-secret",
            "graph-owl listening"
        ),
        graph_owl_server::AuthMode::Open => tracing::warn!(
            %bind, database = %where_from, authentication = "disabled",
            "graph-owl listening with authentication DISABLED — every request runs as \
             the system principal. Set OIDC_ISSUER (or GRAPH_OWL_JWT_SECRET) to secure it."
        ),
    }

    #[cfg(feature = "bolt")]
    if let Some((listener, _bind)) = bolt_listener {
        let limits = graph_owl_server::bolt::bolt_limits_from_env(|name| std::env::var(name).ok())
            .unwrap_or_else(|error| {
                tracing::error!("{error}");
                std::process::exit(78); // EX_CONFIG
            });
        let server = graph_owl_server::bolt::build_server(
            catalog.clone(),
            limits,
            graph_owl_api::SparqlBudget::default(),
        );
        // Makes this server visible to `GET /admin/bolt/status` — only
        // reached when a listener was actually bound, so a build with the
        // feature compiled in but `BOLT_BIND_ADDR` unset reports `enabled:
        // false` rather than a listener that does not exist.
        graph_owl_server::bolt::register(server.clone());
        tokio::spawn(async move {
            server
                .serve(listener, async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await;
        });
    }

    axum::serve(
        listener,
        graph_owl_server::app_with_admission(catalog, admission),
    )
    // Drains in-flight requests rather than cutting them mid-write.
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutdown signal received; draining in-flight requests");
    })
    .await
    .expect("server error");
}
