//! Epic 10 Slices C and D: a request is traceable and behaviour is measurable.
//!
//! The decisions live here as pure functions and the middleware only applies
//! them. Two of them are the kind that are wrong silently — a redaction that
//! misses a password, and a metric label that is unbounded — and neither
//! announces itself at runtime. A pure function is the only version of them a
//! test can interrogate exhaustively.

use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::Response;
use graph_owl_api::Catalog;

/// The header a client uses to name its own request, and the one echoed back.
pub const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// The request's id: the client's if it supplied a usable one, a fresh UUID
/// otherwise.
///
/// **Propagation is the point.** An operator correlating a client's report with
/// server logs has only the id the *client* holds; generating a new one when the
/// client already named the request severs exactly the link the header exists to
/// make.
///
/// A supplied value is rejected when it is empty, over-long, or carries anything
/// outside the printable ASCII an id can safely occupy. It is echoed into a
/// response header and into every log line, so an unvalidated one is a header
/// injection and a log-forging primitive in the same field. Rejection falls back
/// to a generated id rather than an error: a malformed correlation header is not
/// a reason to refuse the request underneath it.
#[must_use]
pub fn request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .filter(|supplied| is_usable_request_id(supplied))
        .map_or_else(|| uuid::Uuid::new_v4().to_string(), ToString::to_string)
}

/// 128 characters: comfortably longer than a UUID (36) or a W3C trace parent
/// (55), and short enough that a header full of them cannot become the bulk of
/// a log line.
const MAX_REQUEST_ID: usize = 128;

fn is_usable_request_id(supplied: &str) -> bool {
    !supplied.is_empty()
        && supplied.len() <= MAX_REQUEST_ID
        && supplied
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// Remove the password from a connection string before it can be logged.
///
/// `DATABASE_URL` is the one configuration value that is both routinely logged
/// (it names the thing a startup or readiness failure is about) and routinely
/// carries a credential.
///
/// Only the password is removed. The host, port, database and user are what make
/// the line useful for diagnosis, and redacting the whole URL produces a log
/// that is safe and says nothing.
#[must_use]
pub fn redact(url: &str) -> String {
    // `user:password@host` — the password is between the first `:` after the
    // scheme and the last `@` before the host.
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    // Rightmost `@`: a password may legitimately contain one, and splitting on
    // the first would leave the tail of the password in the output — which is
    // the failure this function exists to prevent, delivered quietly.
    let Some((credentials, host)) = rest.rsplit_once('@') else {
        return url.to_string();
    };
    match credentials.split_once(':') {
        Some((user, _)) => format!("{scheme}://{user}:***@{host}"),
        None => url.to_string(),
    }
}

/// Whether a completed request is worth an `error`-level line.
///
/// `5xx` only. A `404` or a `412` is the API behaving correctly — logging it at
/// `error` trains an operator to ignore the level, and the one real fault then
/// arrives indistinguishable from the noise.
#[must_use]
pub fn is_server_fault(status: StatusCode) -> bool {
    status.is_server_error()
}

/// The metric label for a route.
///
/// **The template, never the concrete path.** `/assets/{id}` is one series;
/// `/assets/<uuid>` is one series per asset, which is how a Prometheus server
/// is ended. `MatchedPath` is absent for a request that matched no route, and
/// that case must collapse to a single bucket rather than echo whatever was
/// typed — an attacker choosing the label values is the same cardinality
/// explosion with intent behind it.
#[must_use]
pub fn route_label(matched: Option<&str>) -> String {
    matched.unwrap_or("unmatched").to_string()
}

/// A duration as the milliseconds a log line reports.
///
/// The metric records seconds and the log records milliseconds, deliberately: a
/// dashboard wants base units and a person reading one line does not want to
/// count zeroes. That makes this a **conversion**, and a wrong conversion is
/// invisible — `duration_ms=0.002` for a two-millisecond request looks like a
/// plausible number, not like a bug. It is a function so a test can state what
/// the number means.
#[must_use]
pub fn duration_ms(elapsed: std::time::Duration) -> f64 {
    elapsed.as_secs_f64() * 1_000.0
}

/// Whether logs are rendered as JSON.
///
/// JSON is **not** the default. The most frequent reader of these lines during
/// development is a person, and a default that is unreadable to them is a
/// default that gets switched off and never switched back on.
#[must_use]
pub fn wants_json_logs(format: Option<&str>) -> bool {
    format.is_some_and(|format| format.eq_ignore_ascii_case("json"))
}

/// Requests to `/metrics` are not counted.
///
/// A scrape every 15 seconds is the most frequent request most deployments
/// make, and counting it buries real traffic under monitoring traffic in every
/// rate query an operator writes.
#[must_use]
pub fn is_metered(path: &str) -> bool {
    path != "/metrics"
}

/// `graph_owl_<subsystem>_<noun>_<unit>`, per the observability contract in
/// `10-operability.md`. Base units only — a metric named in milliseconds forces
/// every dashboard to carry a conversion that one of them will get wrong.
const REQUESTS: &str = "graph_owl_http_requests_total";
const DURATION: &str = "graph_owl_http_request_duration_seconds";

/// The process-wide Prometheus recorder.
///
/// A global because the `metrics` facade is one: a counter incremented anywhere
/// in the process reaches whichever recorder was installed, and installing a
/// second would silently split the series. `OnceLock` makes the second attempt
/// a no-op rather than a panic, so a test that builds two apps in one process
/// does not fail on the second.
static PROMETHEUS: std::sync::OnceLock<metrics_exporter_prometheus::PrometheusHandle> =
    std::sync::OnceLock::new();

/// Install the recorder, or return the one already installed.
pub fn metrics_handle() -> &'static metrics_exporter_prometheus::PrometheusHandle {
    PROMETHEUS.get_or_init(|| {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .install_recorder()
            .expect("the Prometheus recorder installs once per process")
    })
}

/// Where the `Auth` extractor leaves the identity for the access log.
///
/// **The middleware cannot see the principal on its own.** `next.run(request)`
/// consumes the request, so anything the extractor puts in its extensions goes
/// with it and is unreachable by the time there is a response to log. A shared
/// cell inserted *before* the handler runs is what lets one value travel back
/// out.
///
/// It stays `None` for an unauthenticated request, a rejected one, and every
/// route that takes no `Auth` — `/health`, `/ready`, `/metrics`. That is the
/// honest answer in all three cases: nobody was identified. Substituting
/// `"anonymous"` would make a failed authentication indistinguishable from a
/// route that never asked for one.
#[derive(Clone, Default)]
pub struct RequestPrincipal(std::sync::Arc<std::sync::Mutex<Option<String>>>);

impl RequestPrincipal {
    /// Record who this request turned out to be.
    ///
    /// Last write wins. A request resolves a principal once, and a lock that
    /// refused a second write would turn a duplicated extractor — legal, if
    /// two arguments both ask for `Auth` — into a failure.
    pub fn set(&self, id: &str) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(id.to_string());
        }
    }

    #[must_use]
    pub fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }
}

/// The span every other span in a request hangs from.
///
/// `<subsystem>.<operation>`, per the observability contract, and it carries
/// `request_id` so **every child inherits it** — which is the whole mechanism.
/// Without a parent span the facade and adapter spans are roots, and a slow
/// request is attributable to the process rather than to a layer, which is the
/// thing the contract asks for.
///
/// `route` rather than the concrete path, for the same reason the metric label
/// is: an entity id in a span field is an unbounded value reaching a tracing
/// backend, and those cost money per unique series just as Prometheus does.
#[must_use]
pub fn request_span(request_id: &str, method: &str, route: &str) -> tracing::Span {
    tracing::info_span!(
        "http.request",
        request_id = %request_id,
        method = %method,
        route = %route,
    )
}

/// One log line and two metrics per request, and the id echoed back.
///
/// Applied with `Router::layer` rather than `route_layer` so `MatchedPath` is
/// already in the extensions: the route *template* is the metric label, and
/// reading it before routing would leave only the concrete path — which is the
/// cardinality explosion this exists to avoid.
pub async fn observe(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let id = request_id(request.headers());
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let route = route_label(
        request
            .extensions()
            .get::<axum::extract::MatchedPath>()
            .map(axum::extract::MatchedPath::as_str),
    );

    // Inserted before the handler runs, so the extractor has somewhere to put
    // the identity that this scope can still read afterwards.
    let principal = RequestPrincipal::default();
    let mut request = request;
    request.extensions_mut().insert(principal.clone());

    let started = std::time::Instant::now();
    // Everything the handler does happens inside this span, so the facade and
    // adapter spans below it are children and inherit `request_id`.
    let span = request_span(&id, &method, &route);
    let mut response = {
        use tracing::Instrument as _;
        next.run(request).instrument(span).await
    };
    let elapsed = started.elapsed();
    let status = response.status();

    // Echoed before anything can return early, so a client always gets back the
    // id its log line will be filed under — including on an error path.
    if let Ok(value) = id.parse() {
        response.headers_mut().insert(REQUEST_ID, value);
    }

    if is_metered(&path) {
        metrics::counter!(
            REQUESTS,
            "method" => method.clone(),
            "route" => route.clone(),
            "status" => status.as_u16().to_string()
        )
        .increment(1);
        metrics::histogram!(DURATION, "method" => method.clone(), "route" => route.clone())
            .record(elapsed.as_secs_f64());
    }

    // `duration_ms` in the log and seconds in the metric, deliberately: a human
    // reading one line wants milliseconds, and Prometheus wants base units.
    let duration_ms = duration_ms(elapsed);
    // `principal` is the field `10-operability.md`'s log contract names, and
    // "which identity made this request" is the first question of every
    // authorization incident. Absent rather than `"anonymous"` when nobody was
    // identified — see `RequestPrincipal`.
    let who = principal.get();
    if is_server_fault(status) {
        tracing::error!(
            request_id = %id,
            principal = who.as_deref().unwrap_or("-"),
            method = %method,
            route = %route,
            status = status.as_u16(),
            duration_ms,
            "request failed"
        );
    } else {
        tracing::info!(
            request_id = %id,
            principal = who.as_deref().unwrap_or("-"),
            method = %method,
            route = %route,
            status = status.as_u16(),
            duration_ms,
            "request"
        );
    }

    response
}

/// How many pool connections are handed out.
///
/// Derived rather than counted, for the reason `PoolStats` does not report it:
/// a pool moves connections between idle and in-use constantly, and two
/// separately-sampled numbers publish a pair that does not sum to the total an
/// operator is reading them against.
///
/// Saturating because the two readings are taken a moment apart — a connection
/// returned in between would otherwise underflow into an enormous in-use count,
/// and a metrics path must not panic.
#[must_use]
pub fn connections_in_use(stats: graph_owl_storage::PoolStats) -> u32 {
    stats.connections.saturating_sub(stats.idle)
}

const POOL: &str = "graph_owl_db_pool_connections";
const ENTITIES: &str = "graph_owl_catalog_entities_total";

/// Serves the exporter's rendering. Excluded from its own counters by
/// [`is_metered`].
///
/// **The gauges are sampled here rather than on a timer.** A background task
/// would publish numbers that are up to its interval old and keep running when
/// nobody is scraping; sampling on the scrape means the value Prometheus reads
/// is the value at the moment it asked, and costs nothing when nobody asks.
pub async fn metrics_endpoint(
    axum::extract::State(catalog): axum::extract::State<Catalog>,
) -> String {
    if let Some(stats) = catalog.pool_stats() {
        metrics::gauge!(POOL, "state" => "idle").set(f64::from(stats.idle));
        metrics::gauge!(POOL, "state" => "in_use").set(f64::from(connections_in_use(stats)));
    }

    // As the system principal, deliberately. This is an operational gauge, and
    // one whose value depended on who scraped it would be meaningless — an
    // estate does not change size according to who is looking. It reports
    // aggregate counts, never which assets exist, and `/metrics` is already
    // unauthenticated by design.
    if let Ok(counts) = catalog
        .count_assets_by_kind_for(&graph_owl_core::Principal::system())
        .await
    {
        for (kind, count) in counts {
            #[allow(clippy::cast_precision_loss)]
            metrics::gauge!(ENTITIES, "entity_type" => kind.as_str()).set(count as f64);
        }
    }

    metrics_handle().render()
}

/// Install the log subscriber. Returns whether *this* call installed it.
///
/// `LOG_FORMAT=json` for a collector, anything else for a person;
/// `LOG_LEVEL` for the filter, defaulting to `info`.
///
/// The return value exists to be asserted. A function whose entire effect is
/// setting a process global is otherwise unobservable from a test, and an
/// unobservable function is one a mutation run replaces with nothing and
/// nobody notices — which is what happened before this signature.
pub fn install_logging() -> bool {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_env("LOG_LEVEL").unwrap_or_else(|_| EnvFilter::new("info"));

    if wants_json_logs(std::env::var("LOG_FORMAT").ok().as_deref()) {
        fmt().json().with_env_filter(filter).try_init().is_ok()
    } else {
        fmt().with_env_filter(filter).try_init().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_request_id(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(value) = value.parse() {
            headers.insert(REQUEST_ID, value);
        }
        headers
    }

    mod a_request_carries_one_id_end_to_end {
        use super::*;

        #[test]
        fn a_supplied_id_is_propagated_rather_than_replaced() {
            // The whole point of the header. An operator correlating a client's
            // report with server logs has only the id the client holds.
            assert_eq!(
                request_id(&with_request_id("abc-123")),
                "abc-123".to_string()
            );
        }

        #[test]
        fn an_absent_header_produces_a_fresh_id() {
            let generated = request_id(&HeaderMap::new());
            assert!(
                uuid::Uuid::parse_str(&generated).is_ok(),
                "{generated} is not a uuid"
            );
        }

        /// And the negative: two requests without the header must not share an
        /// id. A constant would satisfy the test above and correlate nothing.
        #[test]
        fn two_generated_ids_differ() {
            assert_ne!(request_id(&HeaderMap::new()), request_id(&HeaderMap::new()));
        }

        #[test]
        fn an_empty_header_is_not_an_id() {
            let generated = request_id(&with_request_id(""));
            assert!(uuid::Uuid::parse_str(&generated).is_ok(), "{generated}");
        }

        /// The id is echoed into a response header and into every log line, so
        /// an unvalidated one is header injection and log forging in one field.
        #[test]
        fn an_id_carrying_control_characters_or_spaces_is_refused() {
            for hostile in ["a b", "a\tb", "line1\\nline2", "<script>", "a\"b"] {
                let produced = request_id(&with_request_id(hostile));
                assert_ne!(produced, hostile, "{hostile:?} was echoed verbatim");
                assert!(uuid::Uuid::parse_str(&produced).is_ok(), "{produced}");
            }
        }

        #[test]
        fn an_over_long_id_is_refused_but_one_at_the_limit_is_kept() {
            let at_limit = "a".repeat(MAX_REQUEST_ID);
            assert_eq!(request_id(&with_request_id(&at_limit)), at_limit);

            let over = "a".repeat(MAX_REQUEST_ID + 1);
            assert_ne!(request_id(&with_request_id(&over)), over);
        }

        #[test]
        fn a_uuid_and_a_trace_parent_are_both_usable_shapes() {
            for shape in [
                "550e8400-e29b-41d4-a716-446655440000",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                "req_1234.5678",
            ] {
                assert_eq!(request_id(&with_request_id(shape)), shape);
            }
        }
    }

    mod a_credential_never_reaches_a_log {
        use super::*;

        #[test]
        fn the_password_is_removed_and_everything_useful_is_kept() {
            assert_eq!(
                redact("postgres://catalog:hunter2@db.internal:5432/graphowl"),
                "postgres://catalog:***@db.internal:5432/graphowl"
            );
        }

        /// The failure this function exists to prevent, and the one it would
        /// deliver quietly: a password containing `@` split on the *first*
        /// separator leaves its tail in the output.
        #[test]
        fn a_password_containing_an_at_sign_is_still_fully_removed() {
            let redacted = redact("postgres://catalog:p@ssw@rd@db.internal:5432/graphowl");

            assert!(!redacted.contains("ssw"), "{redacted}");
            assert!(
                !redacted.contains("rd@db") || redacted.contains("***@db"),
                "{redacted}"
            );
            assert_eq!(redacted, "postgres://catalog:***@db.internal:5432/graphowl");
        }

        #[test]
        fn a_url_with_no_credentials_is_returned_unchanged() {
            assert_eq!(
                redact("postgres://db.internal:5432/graphowl"),
                "postgres://db.internal:5432/graphowl"
            );
        }

        #[test]
        fn a_user_with_no_password_is_left_alone() {
            assert_eq!(
                redact("postgres://catalog@db.internal/graphowl"),
                "postgres://catalog@db.internal/graphowl"
            );
        }

        #[test]
        fn something_that_is_not_a_url_is_returned_unchanged() {
            assert_eq!(redact("not a url"), "not a url");
        }

        /// The negative that stops the lazy implementation: returning a
        /// constant, or the empty string, would pass every assertion above.
        #[test]
        fn redaction_keeps_the_host_that_makes_the_line_worth_logging() {
            let redacted = redact("postgres://u:p@db.internal:5432/graphowl");

            assert!(redacted.contains("db.internal"), "{redacted}");
            assert!(redacted.contains("graphowl"), "{redacted}");
            assert!(redacted.contains('u'), "{redacted}");
        }
    }

    mod what_is_worth_an_error_line {
        use super::*;

        #[test]
        fn a_server_fault_is_an_error() {
            assert!(is_server_fault(StatusCode::INTERNAL_SERVER_ERROR));
            assert!(is_server_fault(StatusCode::SERVICE_UNAVAILABLE));
        }

        /// The negative, and the one that matters: a `404` is the API working.
        /// Logging it at `error` trains an operator to ignore the level.
        #[test]
        fn a_client_error_is_not() {
            for correct in [
                StatusCode::NOT_FOUND,
                StatusCode::PRECONDITION_FAILED,
                StatusCode::BAD_REQUEST,
                StatusCode::FORBIDDEN,
                StatusCode::OK,
            ] {
                assert!(!is_server_fault(correct), "{correct}");
            }
        }
    }

    mod a_number_in_a_log_line_means_what_it_says {
        use super::*;
        use std::time::Duration;

        /// A wrong unit conversion is invisible: `duration_ms=0.002` for a
        /// two-millisecond request looks like a plausible number rather than a
        /// bug, and nothing downstream can tell.
        #[test]
        fn a_second_is_a_thousand_milliseconds() {
            assert!((duration_ms(Duration::from_secs(1)) - 1_000.0).abs() < f64::EPSILON);
        }

        #[test]
        fn two_milliseconds_reads_as_two_not_as_a_fraction() {
            assert!((duration_ms(Duration::from_millis(2)) - 2.0).abs() < f64::EPSILON);
        }

        #[test]
        fn sub_millisecond_work_keeps_its_precision_rather_than_rounding_to_zero() {
            // A fast request reading `0` is indistinguishable from an
            // unmeasured one, which is how a latency regression hides.
            let quick = duration_ms(Duration::from_micros(250));
            assert!(quick > 0.0, "{quick}");
            assert!((quick - 0.25).abs() < 1e-9, "{quick}");
        }

        #[test]
        fn no_elapsed_time_is_zero() {
            assert!((duration_ms(Duration::ZERO) - 0.0).abs() < f64::EPSILON);
        }
    }

    mod how_logs_are_rendered {
        use super::*;

        #[test]
        fn json_is_requested_explicitly_and_case_does_not_matter() {
            assert!(wants_json_logs(Some("json")));
            assert!(wants_json_logs(Some("JSON")));
        }

        /// The negative, and the deliberate default: a person reading these
        /// lines during development gets a readable format unless a deployment
        /// asks otherwise.
        #[test]
        fn anything_else_including_nothing_renders_for_a_person() {
            for setting in [None, Some(""), Some("text"), Some("pretty"), Some("jsonl")] {
                assert!(!wants_json_logs(setting), "{setting:?}");
            }
        }

        #[test]
        fn logging_installs_once_and_a_second_call_does_not_replace_it() {
            install_logging();
            assert!(
                tracing::dispatcher::has_been_set(),
                "a subscriber must actually be installed"
            );
            assert!(
                !install_logging(),
                "a second call must be a no-op, not a replacement"
            );
        }
    }

    mod who_made_the_request {
        use super::*;

        /// The first question of every authorization incident, and the reason
        /// the slot exists at all: `next.run(request)` consumes the request, so
        /// the extractor's own extensions are unreachable by the time there is
        /// a response to log.
        #[test]
        fn the_slot_carries_the_identity_back_to_the_logger() {
            let slot = RequestPrincipal::default();
            slot.set("auth0|abc");

            assert_eq!(slot.get(), Some("auth0|abc".to_string()));
        }

        /// A clone shares the cell. The middleware keeps one and hands the
        /// other to the request — two independent cells would leave the logger
        /// reading the one nobody wrote to.
        #[test]
        fn a_clone_sees_what_the_original_was_told() {
            let held = RequestPrincipal::default();
            let handed_to_the_request = held.clone();

            handed_to_the_request.set("auth0|abc");

            assert_eq!(held.get(), Some("auth0|abc".to_string()));
        }

        /// Nobody identified stays `None`. Substituting `"anonymous"` would
        /// make a *failed* authentication indistinguishable from a route that
        /// never asked for one — `/health` and a rejected token would log the
        /// same thing.
        #[test]
        fn an_unidentified_request_names_nobody() {
            assert_eq!(RequestPrincipal::default().get(), None);
        }

        /// Last write wins. Two handler arguments may both ask for `Auth`,
        /// which is legal, and a cell that refused the second write would turn
        /// that into a failure.
        #[test]
        fn a_second_resolution_overwrites_rather_than_failing() {
            let slot = RequestPrincipal::default();
            slot.set("first");
            slot.set("second");

            assert_eq!(slot.get(), Some("second".to_string()));
        }
    }

    mod a_slow_request_is_attributable_to_a_layer {
        use super::*;
        use std::sync::{Arc, Mutex};
        use tracing::subscriber::with_default;
        use tracing_subscriber::layer::SubscriberExt as _;

        type SpanLog = Arc<Mutex<Vec<(String, Option<String>)>>>;

        /// Records span names and their parentage, which is the thing the
        /// contract is actually about — a flat list of spans says nothing about
        /// which layer was slow.
        #[derive(Default, Clone)]
        struct Tree(SpanLog);

        impl<S> tracing_subscriber::Layer<S> for Tree
        where
            S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
        {
            fn on_new_span(
                &self,
                _: &tracing::span::Attributes<'_>,
                id: &tracing::Id,
                ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                let span = ctx.span(id).expect("the span was just created");
                let parent = span.parent().map(|p| p.name().to_string());
                self.0
                    .lock()
                    .expect("lock")
                    .push((span.name().to_string(), parent));
            }
        }

        fn recorded(work: impl FnOnce()) -> Vec<(String, Option<String>)> {
            let tree = Tree::default();
            let subscriber = tracing_subscriber::registry().with(tree.clone());
            with_default(subscriber, work);
            tree.0.lock().expect("lock").clone()
        }

        /// The request span carries the correlation id, so every child inherits
        /// it. Without a parent span the facade and adapter spans are roots and
        /// nothing ties them to the request that caused them.
        #[test]
        fn the_request_span_is_named_and_parents_the_work_beneath_it() {
            let seen = recorded(|| {
                let span = request_span("req-42", "GET", "/assets/{id}");
                let _entered = span.enter();
                tracing::info_span!("catalog.get_asset").in_scope(|| {
                    tracing::info_span!("storage.get_asset").in_scope(|| {});
                });
            });

            assert_eq!(
                seen,
                vec![
                    ("http.request".to_string(), None),
                    (
                        "catalog.get_asset".to_string(),
                        Some("http.request".to_string())
                    ),
                    (
                        "storage.get_asset".to_string(),
                        Some("catalog.get_asset".to_string())
                    ),
                ],
                "the port boundaries have to nest, or a slow request is \
                 attributable to the process rather than to a layer"
            );
        }

        /// And the negative: work done *outside* the request span is a root,
        /// which is what the `.instrument()` in the middleware exists to
        /// prevent. Without it the assertion above would pass on a subscriber
        /// that invented parentage.
        #[test]
        fn work_outside_the_request_span_has_no_parent() {
            let seen = recorded(|| {
                tracing::info_span!("catalog.get_asset").in_scope(|| {});
            });

            assert_eq!(seen, vec![("catalog.get_asset".to_string(), None)]);
        }

        /// `<subsystem>.<operation>`, per the contract. A name without the
        /// subsystem cannot be grouped by layer, which is the one thing these
        /// spans are for.
        #[test]
        fn the_request_span_follows_the_contracts_naming() {
            let seen = recorded(|| {
                let span = request_span("req-1", "GET", "/health");
                let _entered = span.enter();
            });

            let (name, _) = &seen[0];
            assert!(name.contains('.'), "{name} is not <subsystem>.<operation>");
            assert!(name.starts_with("http."), "{name}");
        }
    }

    mod pool_occupancy {
        use super::*;
        use graph_owl_storage::PoolStats;

        #[test]
        fn in_use_is_the_pool_minus_what_is_idle() {
            assert_eq!(
                connections_in_use(PoolStats {
                    connections: 10,
                    idle: 3
                }),
                7
            );
        }

        #[test]
        fn a_fully_idle_pool_has_nothing_in_use() {
            assert_eq!(
                connections_in_use(PoolStats {
                    connections: 5,
                    idle: 5
                }),
                0
            );
        }

        #[test]
        fn a_fully_busy_pool_reports_every_connection_in_use() {
            assert_eq!(
                connections_in_use(PoolStats {
                    connections: 5,
                    idle: 0
                }),
                5
            );
        }

        /// The two readings are taken a moment apart, so a connection returned
        /// in between can make `idle` exceed the total that was sampled first.
        /// Underflowing into an enormous count would be worse than briefly
        /// reporting zero, and a metrics path must not panic at all.
        #[test]
        fn more_idle_than_the_pool_holds_does_not_underflow() {
            assert_eq!(
                connections_in_use(PoolStats {
                    connections: 4,
                    idle: 9
                }),
                0
            );
        }
    }

    mod labels_are_bounded_sets {
        use super::*;

        #[test]
        fn the_route_label_is_the_template() {
            assert_eq!(route_label(Some("/assets/{id}")), "/assets/{id}");
        }

        /// An unmatched request must collapse to one bucket. Echoing the path
        /// would let a caller choose the label values, which is the cardinality
        /// explosion with intent behind it.
        #[test]
        fn an_unmatched_request_is_one_bucket_not_one_per_path() {
            assert_eq!(route_label(None), "unmatched");
        }

        #[test]
        fn metrics_are_excluded_from_their_own_counters() {
            assert!(!is_metered("/metrics"));
        }

        /// And the negative: excluding everything would make the whole feature
        /// silently do nothing.
        #[test]
        fn every_other_route_is_counted() {
            for path in ["/assets", "/metrics/other", "/health", "/"] {
                assert!(is_metered(path), "{path}");
            }
        }
    }
}
