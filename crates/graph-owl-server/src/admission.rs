//! Epic 10: admission control — the question idempotency does not answer.
//!
//! `16-ingestion-apis.md`'s idempotency key answers *"have I already done
//! this?"*. It does not answer *"should I accept this at all, right now?"*, and
//! under a thundering-herd retry storm those are different questions with
//! different answers.
//!
//! A bounded semaphore fronts the expensive paths and a request that cannot
//! acquire a permit is **rejected immediately**, not queued.
//!
//! **Rejecting fast is the whole point.** An unbounded queue converts an
//! overload into a latency collapse: every client waits, every client times
//! out, every client retries, and the queue grows. A fast `503` lets a
//! well-behaved client back off and keeps the requests already in flight
//! completing at normal speed.
//!
//! The failure mode this module must not have is the one a semaphore makes
//! easy: `acquire` instead of `try_acquire` compiles, passes any test that only
//! checks the happy path, and reintroduces the unbounded queue in the one place
//! built to prevent it. Every rejection test here exists to kill that mutation.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A kind of expensive work, limited independently of the others.
///
/// **Independently is the point.** One shared semaphore would let an ingestion
/// storm shed every read query, which is the opposite of what an operator
/// wants: the catalog stays answerable while a connector run is refused.
///
/// `10-operability.md` names three classes — ingestion, query and reasoning.
/// Reasoning has no route until Epic 6, and a class with nothing mapped to it
/// would export two gauges that are constant forever, so it is added when the
/// route is. The extension point is [`class_of`], which is one match arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// A graph read that can walk an unbounded amount of data before its own
    /// budget stops it.
    Query,
    /// A write path that holds a connection for the length of a source scan.
    Ingestion,
}

impl Class {
    /// Every class, in a fixed order so the startup report and the metric
    /// exposition agree run to run.
    pub const ALL: &'static [Class] = &[Class::Query, Class::Ingestion];

    /// The metric label. A bounded set by construction — these two strings are
    /// the only values it ever takes, which is what the observability
    /// contract's label discipline requires.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Class::Query => "query",
            Class::Ingestion => "ingestion",
        }
    }

    /// The environment variable that sets this class's permit count.
    #[must_use]
    pub const fn env(self) -> &'static str {
        match self {
            Class::Query => "GRAPH_OWL_ADMISSION_QUERY_PERMITS",
            Class::Ingestion => "GRAPH_OWL_ADMISSION_INGESTION_PERMITS",
        }
    }
}

/// Which class a **route template** belongs to, if any.
///
/// The template, never the concrete path — same reason as the metric label in
/// [`crate::observability::route_label`]. An unmatched request has no template
/// and is not controlled: it is about to become a `404`, which costs nothing.
///
/// Everything absent from this list is deliberately uncontrolled. `/health`,
/// `/ready` and `/metrics` are the ones that matter: a server shedding load is
/// exactly when an operator needs to scrape it, and a probe that 503s because
/// the service is busy is how a load balancer turns a slowdown into an outage.
#[must_use]
pub fn class_of(route: &str) -> Option<Class> {
    match route {
        // A whole SPARQL query, and a traversal walk. Both can read a large
        // fraction of the graph before their own fact/hop budget stops them.
        "/sparql" | "/assets/{id}/graph" => Some(Class::Query),
        // A source scan and a full projection rebuild. Both hold a connection
        // for their whole duration and neither is latency-sensitive, so both
        // are better refused than queued.
        //
        // `/ingest/batch` joins them for the same reason and a second one: it
        // spools the whole upload to disk before answering, so a herd of them
        // costs disk as well as connections. The *polling* routes are
        // deliberately absent — a client that cannot ask how its job is doing
        // has to guess, and a `503` on a poll would make an overloaded server
        // look like a lost job.
        // A webhook delivery, joining them for the same reason: it holds a
        // connection for `create_inbound_event`'s synchronous write, and an
        // external sender retrying into a `503` is the same back-pressure
        // signal `/ingest/batch` already relies on — Epic 18 Slice E's own
        // criterion, "a burst does not exhaust the pool", is this mechanism
        // rather than a new one.
        "/connectors/postgres/runs"
        | "/graph/reconcile"
        | "/ingest/batch"
        | "/webhooks/receive/{path}" => Some(Class::Ingestion),
        _ => None,
    }
}

/// Permits per class when nothing is configured.
///
/// **Not a tuning number.** Every controlled path holds a Postgres connection
/// for its whole duration, so the concurrency beyond which more requests add no
/// throughput is the pool's size — 10, which is what `PgPool::connect` uses in
/// `graph-owl-storage-postgres`. Permits above that number do not admit more
/// work; they move the queue out of the semaphore, where it is bounded, visible
/// and sheds fast, and into the pool's acquire, where it is unbounded,
/// unmeasured and waits. That is precisely the collapse this module exists to
/// prevent, so the default is the pool size and not a multiple of it.
///
/// Each class gets its own, so the classes sum to more than the pool. That is
/// intentional: the semaphore's job is to stop a herd of hundreds, not to keep
/// the pool from ever waiting, and isolation between classes is worth more than
/// a total that happens to match.
pub const DEFAULT_PERMITS: usize = 10;

/// Seconds a rejected client is told to wait.
///
/// One, because `Retry-After` in delta-seconds has one-second granularity
/// (RFC 9110 §10.2.3) and one is therefore the smallest value it can express.
/// A rejection means "not right now", and a larger default keeps a recovered
/// server idle while clients that could be served sit out the interval.
/// Deployments whose overload episodes outlast a permit hold raise it.
pub const DEFAULT_RETRY_AFTER_SECONDS: u64 = 1;

/// The variable that sets [`DEFAULT_RETRY_AFTER_SECONDS`].
pub const RETRY_AFTER_ENV: &str = "GRAPH_OWL_ADMISSION_RETRY_AFTER_SECONDS";

/// A configuration value that cannot be honoured.
///
/// Carries the variable's name for the same reason [`crate::budget::BudgetError`]
/// does: `10-operability.md`'s first acceptance criterion is that the service
/// refuses to start on invalid config **naming the offending variable**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionError {
    pub variable: &'static str,
    pub value: String,
    pub reason: &'static str,
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}={:?} is not usable: {}",
            self.variable, self.value, self.reason
        )
    }
}

/// `graph_owl_http_*`, per the observability contract's prefix table: admission
/// control is part of the HTTP surface, which Epic 10 owns. A new
/// `graph_owl_admission_*` prefix would be a subsystem the contract does not
/// list, and the contract says adding one is a review-blocking finding.
const PERMITS_AVAILABLE: &str = "graph_owl_http_admission_permits_available";
const PERMITS_HELD: &str = "graph_owl_http_admission_permits_held";
const REJECTIONS: &str = "graph_owl_http_admission_rejections_total";

/// The permit pools, one per class.
///
/// `Debug` because a test asserting a configuration is *refused* writes
/// `expect_err`, which has to render the `Ok` value it did not expect. Deriving
/// it is cheaper than making every such test unwrap by hand — and a permit pool
/// has nothing secret in it.
#[derive(Debug)]
pub struct Admission {
    limits: BTreeMap<Class, usize>,
    semaphores: BTreeMap<Class, Arc<Semaphore>>,
    retry_after_seconds: u64,
}

impl Admission {
    /// Build from explicit limits. Every class not named takes
    /// [`DEFAULT_PERMITS`].
    #[must_use]
    pub fn with_limits(limits: &[(Class, usize)], retry_after_seconds: u64) -> Self {
        let mut resolved = BTreeMap::new();
        for class in Class::ALL {
            let permits = limits
                .iter()
                .find(|(named, _)| named == class)
                .map_or(DEFAULT_PERMITS, |(_, permits)| *permits);
            resolved.insert(*class, permits);
        }
        Self::build(resolved, retry_after_seconds)
    }

    fn build(limits: BTreeMap<Class, usize>, retry_after_seconds: u64) -> Self {
        let semaphores = limits
            .iter()
            .map(|(class, permits)| (*class, Arc::new(Semaphore::new(*permits))))
            .collect();
        let admission = Self {
            limits,
            semaphores,
            retry_after_seconds,
        };
        // Published at construction, so a scrape taken before the first
        // controlled request already shows the ceiling. A gauge that only
        // appears under load is missing exactly when an operator goes looking
        // for it — during the incident, before the first rejection.
        for class in Class::ALL {
            admission.publish(*class);
        }
        admission
    }

    /// Read the limits from the environment.
    ///
    /// Takes a reader rather than calling `std::env::var`, so the whole of this
    /// is testable without a process-global mutation other tests would race
    /// against.
    ///
    /// # Errors
    ///
    /// [`AdmissionError`] naming the variable when a value is not a positive
    /// integer.
    pub fn from_env(read: impl Fn(&str) -> Option<String>) -> Result<Self, AdmissionError> {
        let mut limits = BTreeMap::new();
        for class in Class::ALL {
            let permits = match read(class.env()) {
                None => DEFAULT_PERMITS,
                Some(raw) => {
                    let parsed: usize = raw.trim().parse().map_err(|_| AdmissionError {
                        variable: class.env(),
                        value: raw.clone(),
                        reason: "expected a whole number of permits",
                    })?;
                    // Zero is refused rather than honoured. A limit of zero
                    // rejects every request on the path forever, which is
                    // indistinguishable at the client from the service being
                    // permanently broken — and it is far more often a typo
                    // than an intent. Refusing to start says so once, at the
                    // moment somebody can still fix it.
                    if parsed == 0 {
                        return Err(AdmissionError {
                            variable: class.env(),
                            value: raw.clone(),
                            reason: "at least one permit is required; zero would refuse every \
                                     request on this path",
                        });
                    }
                    parsed
                }
            };
            limits.insert(*class, permits);
        }

        let retry_after_seconds = match read(RETRY_AFTER_ENV) {
            None => DEFAULT_RETRY_AFTER_SECONDS,
            Some(raw) => raw.trim().parse().map_err(|_| AdmissionError {
                variable: RETRY_AFTER_ENV,
                value: raw.clone(),
                reason: "expected a whole number of seconds",
            })?,
        };

        Ok(Self::build(limits, retry_after_seconds))
    }

    /// This class's ceiling.
    #[must_use]
    pub fn limit(&self, class: Class) -> usize {
        self.limits.get(&class).copied().unwrap_or(DEFAULT_PERMITS)
    }

    #[must_use]
    pub fn retry_after_seconds(&self) -> u64 {
        self.retry_after_seconds
    }

    /// Take a permit, or refuse **without waiting**.
    ///
    /// `try_acquire_owned`, never `acquire_owned`. The waiting version compiles,
    /// passes every happy-path test, and turns the bounded semaphore back into
    /// the unbounded queue — silently, and only under the load nobody tests
    /// with.
    #[must_use]
    pub fn try_admit(&self, class: Class) -> Option<Permit> {
        let semaphore = self.semaphores.get(&class)?;
        match Arc::clone(semaphore).try_acquire_owned() {
            Ok(permit) => {
                self.publish(class);
                Some(Permit {
                    _permit: permit,
                    semaphore: Arc::clone(semaphore),
                    limit: self.limit(class),
                    class,
                })
            }
            Err(_) => {
                metrics::counter!(REJECTIONS, "class" => class.label()).increment(1);
                self.publish(class);
                None
            }
        }
    }

    /// Export this class's occupancy as it stands.
    fn publish(&self, class: Class) {
        let available = self
            .semaphores
            .get(&class)
            .map_or(0, |semaphore| semaphore.available_permits());
        publish_occupancy(class, available, self.limit(class));
    }
}

/// What the two gauges should read.
///
/// `held` is derived rather than counted separately: two independently
/// maintained numbers drift, and the pair that must always sum to the limit is
/// exactly the pair a reader will use to conclude the limit is wrong.
///
/// A value rather than two `gauge!` calls, so the arithmetic is testable. The
/// numbers here are only ever *reported*, never acted on, which makes a wrong
/// one invisible at runtime — an operator sees a plausible saturation figure
/// and has no way to tell it is off by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occupancy {
    pub available: usize,
    pub held: usize,
}

/// Occupancy from a live reading of the semaphore.
#[must_use]
pub fn occupancy(available: usize, limit: usize) -> Occupancy {
    Occupancy {
        available,
        // Saturating: a limit lowered at runtime would otherwise underflow into
        // an enormous `held`, and a panic in a metrics path is worse than a
        // number that is briefly conservative.
        held: limit.saturating_sub(available),
    }
}

/// Occupancy as it will be once a permit currently being dropped is released.
///
/// **The `+ 1` is the whole point.** `Drop` for [`Permit`] runs *before* the
/// inner `OwnedSemaphorePermit`'s own `Drop`, so the semaphore still counts this
/// permit as held. Reporting the raw reading leaves every gauge one permit
/// pessimistic for the rest of the process — a server that looks permanently
/// busier than it is, which is the shape of a number an operator scales on.
#[must_use]
pub fn occupancy_on_release(available_now: usize, limit: usize) -> Occupancy {
    occupancy(available_now.saturating_add(1).min(limit), limit)
}

fn publish_occupancy(class: Class, available: usize, limit: usize) {
    publish(class, occupancy(available, limit));
}

/// Set the two gauges from a computed occupancy.
fn publish(class: Class, occupancy: Occupancy) {
    #[allow(clippy::cast_precision_loss)]
    let available = occupancy.available as f64;
    #[allow(clippy::cast_precision_loss)]
    let held = occupancy.held as f64;
    metrics::gauge!(PERMITS_AVAILABLE, "class" => class.label()).set(available);
    metrics::gauge!(PERMITS_HELD, "class" => class.label()).set(held);
}

/// A held permit. Released on drop — which is at the end of the request,
/// because the middleware holds it across `next.run`.
pub struct Permit {
    _permit: OwnedSemaphorePermit,
    semaphore: Arc<Semaphore>,
    limit: usize,
    class: Class,
}

impl Drop for Permit {
    fn drop(&mut self) {
        // The permit itself is released by its own `Drop`, which runs after
        // this one, so the count read here is still the occupied one — hence
        // `+ 1`. Republishing matters: without it the gauges only ever move
        // upward in occupancy and an operator sees a permanently saturated
        // server that is in fact idle.
        publish(
            self.class,
            occupancy_on_release(self.semaphore.available_permits(), self.limit),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod what_the_gauges_should_read {
        use super::*;

        #[test]
        fn available_and_held_always_sum_to_the_limit() {
            for (available, limit) in [(0, 4), (1, 4), (4, 4), (0, 1)] {
                let occupancy = occupancy(available, limit);
                assert_eq!(occupancy.available, available);
                assert_eq!(
                    occupancy.available + occupancy.held,
                    limit,
                    "the pair a reader uses to conclude the limit is wrong"
                );
            }
        }

        /// A limit lowered below what is outstanding must not underflow into an
        /// enormous `held`. A panic in a metrics path is worse than a number
        /// that is briefly conservative.
        #[test]
        fn more_available_than_the_limit_does_not_underflow() {
            assert_eq!(
                occupancy(9, 4),
                Occupancy {
                    available: 9,
                    held: 0
                }
            );
        }

        /// **The `+ 1` that the raw reading needs.** `Drop` for `Permit` runs
        /// before the inner permit's own `Drop`, so the semaphore still counts
        /// this one as held. Reporting the raw number leaves every gauge one
        /// permit pessimistic for the rest of the process — a server that looks
        /// permanently busier than it is.
        #[test]
        fn releasing_the_last_permit_reports_the_pool_as_free() {
            assert_eq!(
                occupancy_on_release(0, 1),
                Occupancy {
                    available: 1,
                    held: 0
                },
                "a pool of one, its only permit going away, is free"
            );
            assert_eq!(
                occupancy_on_release(2, 4),
                Occupancy {
                    available: 3,
                    held: 1
                }
            );
        }

        /// And it never over-reports past the limit.
        #[test]
        fn a_release_cannot_report_more_available_than_the_pool_holds() {
            assert_eq!(
                occupancy_on_release(4, 4),
                Occupancy {
                    available: 4,
                    held: 0
                }
            );
        }
    }

    /// The gauges are a side effect on a process-global recorder, which makes
    /// them invisible to a test that only inspects return values — and a
    /// mutation replacing the publish with nothing walks straight through.
    /// These read the recorder back.
    mod the_gauges_are_actually_set {
        use super::*;

        fn rendered() -> String {
            crate::observability::metrics_handle().render()
        }

        fn value(render: &str, metric: &str, class: &str) -> Option<f64> {
            render
                .lines()
                .find(|line| line.starts_with(metric) && line.contains(class))
                .and_then(|line| line.rsplit(' ').next())
                .and_then(|n| n.parse().ok())
        }

        #[test]
        fn acquiring_and_releasing_moves_both_gauges() {
            // **Installed before the pool is built**, not after. `Admission`
            // publishes its opening occupancy on construction, and the
            // `metrics` facade drops anything recorded before a recorder
            // exists — the same trap that made the Prometheus recorder's lazy
            // installation lose the whole startup window.
            crate::observability::metrics_handle();

            // A class of its own, so this test cannot race another that shares
            // a pool. `Ingestion` is untouched by the tests above.
            let admission = Admission::with_limits(&[(Class::Ingestion, 2)], 1);

            let permit = admission
                .try_admit(Class::Ingestion)
                .expect("a fresh pool admits");
            let held = rendered();
            assert_eq!(
                value(&held, PERMITS_HELD, "ingestion"),
                Some(1.0),
                "one permit out: {held}"
            );
            assert_eq!(value(&held, PERMITS_AVAILABLE, "ingestion"), Some(1.0));

            drop(permit);
            let freed = rendered();
            assert_eq!(
                value(&freed, PERMITS_HELD, "ingestion"),
                Some(0.0),
                "the pool is idle again: {freed}"
            );
            assert_eq!(
                value(&freed, PERMITS_AVAILABLE, "ingestion"),
                Some(2.0),
                "and fully available — this is the `+ 1` on release"
            );
        }
    }

    mod which_paths_are_controlled {
        use super::*;

        #[test]
        fn a_query_and_a_traversal_are_query_class() {
            assert_eq!(class_of("/sparql"), Some(Class::Query));
            assert_eq!(class_of("/assets/{id}/graph"), Some(Class::Query));
        }

        #[test]
        fn a_connector_run_and_a_reprojection_are_ingestion_class() {
            assert_eq!(
                class_of("/connectors/postgres/runs"),
                Some(Class::Ingestion)
            );
            assert_eq!(class_of("/graph/reconcile"), Some(Class::Ingestion));
        }

        /// Epic 18 Slice E: a webhook delivery holds a connection the same
        /// way `/ingest/batch` does, so a burst of deliveries sheds through
        /// the same mechanism rather than exhausting the pool.
        #[test]
        fn a_webhook_delivery_is_ingestion_class() {
            assert_eq!(class_of("/webhooks/receive/{path}"), Some(Class::Ingestion));
        }

        /// **The negative that matters most.** A server shedding load is
        /// exactly when an operator needs to scrape it and when the load
        /// balancer needs an answer. Controlling these turns a slowdown into an
        /// outage and blinds the monitoring that would have explained it.
        #[test]
        fn a_probe_or_a_scrape_is_never_controlled() {
            for uncontrolled in ["/health", "/ready", "/metrics", "/openapi.json"] {
                assert_eq!(class_of(uncontrolled), None, "{uncontrolled}");
            }
        }

        /// And the negative that stops "control everything": cheap CRUD is not
        /// admission-controlled, or a burst of reads sheds the writes.
        #[test]
        fn ordinary_crud_is_not_controlled() {
            for cheap in [
                "/assets",
                "/assets/{id}",
                "/assets/{id}/children",
                "/tables",
                "/tables/{id}",
                "unmatched",
            ] {
                assert_eq!(class_of(cheap), None, "{cheap}");
            }
        }

        /// The concrete path is not the template, and must not be controlled by
        /// accident — the mapping is keyed on what `MatchedPath` produces.
        #[test]
        fn a_concrete_path_is_not_a_template() {
            assert_eq!(
                class_of("/assets/550e8400-e29b-41d4-a716-446655440000/graph"),
                None
            );
        }

        #[test]
        fn the_two_classes_are_distinct() {
            assert_ne!(Class::Query.label(), Class::Ingestion.label());
            assert_ne!(Class::Query.env(), Class::Ingestion.env());
            assert_eq!(Class::ALL.len(), 2);
        }
    }

    mod a_permit_is_refused_rather_than_queued {
        use super::*;

        #[test]
        fn permits_up_to_the_limit_are_granted() {
            let admission = Admission::with_limits(&[(Class::Query, 2)], 1);

            let first = admission.try_admit(Class::Query);
            let second = admission.try_admit(Class::Query);

            assert!(first.is_some());
            assert!(second.is_some());
        }

        /// **The rejection test, and the one that kills `acquire` for
        /// `try_acquire`.** It is synchronous on purpose: a waiting
        /// implementation cannot return `None` here at all, it can only block,
        /// and blocking a test that holds its own permits is a hang rather
        /// than a pass.
        #[test]
        fn the_request_past_the_limit_is_refused_immediately() {
            let admission = Admission::with_limits(&[(Class::Query, 1)], 1);

            let held = admission.try_admit(Class::Query);
            assert!(held.is_some());

            assert!(
                admission.try_admit(Class::Query).is_none(),
                "a second request must be refused, not queued"
            );
        }

        /// Releasing readmits. Without this, one saturation episode would shed
        /// traffic for the lifetime of the process — a failure that looks
        /// exactly like the overload continuing.
        #[test]
        fn releasing_a_permit_readmits_the_next_request() {
            let admission = Admission::with_limits(&[(Class::Query, 1)], 1);

            let held = admission.try_admit(Class::Query);
            assert!(admission.try_admit(Class::Query).is_none());

            drop(held);

            assert!(
                admission.try_admit(Class::Query).is_some(),
                "a released permit must be reusable"
            );
        }

        /// **Isolation, which is why there is more than one semaphore.** An
        /// ingestion storm must not shed reads. A single shared pool passes
        /// every test above and fails this one.
        #[test]
        fn saturating_one_class_does_not_refuse_the_other() {
            let admission = Admission::with_limits(&[(Class::Ingestion, 1), (Class::Query, 1)], 1);

            let _ingesting = admission.try_admit(Class::Ingestion);
            assert!(admission.try_admit(Class::Ingestion).is_none());

            assert!(
                admission.try_admit(Class::Query).is_some(),
                "a saturated ingestion path must not shed queries"
            );
        }

        #[test]
        fn an_unnamed_class_takes_the_default_limit() {
            let admission = Admission::with_limits(&[(Class::Query, 1)], 1);

            assert_eq!(admission.limit(Class::Ingestion), DEFAULT_PERMITS);
            assert_eq!(admission.limit(Class::Query), 1);
        }
    }

    mod the_limits_come_from_the_environment {
        use super::*;

        fn nothing_set(_: &str) -> Option<String> {
            None
        }

        #[test]
        fn an_unset_environment_gives_the_stated_defaults() {
            let admission = Admission::from_env(nothing_set).expect("defaults are valid");

            assert_eq!(admission.limit(Class::Query), DEFAULT_PERMITS);
            assert_eq!(admission.limit(Class::Ingestion), DEFAULT_PERMITS);
            assert_eq!(admission.retry_after_seconds(), DEFAULT_RETRY_AFTER_SECONDS);
        }

        /// Pinned against literals, not against the constants. Asserting
        /// `DEFAULT_PERMITS == DEFAULT_PERMITS` is true however the constant
        /// drifts, which is how a documented number stops matching the shipped
        /// one without a single test failing.
        #[test]
        fn the_documented_defaults_are_the_shipped_defaults() {
            assert_eq!(DEFAULT_PERMITS, 10);
            assert_eq!(DEFAULT_RETRY_AFTER_SECONDS, 1);
        }

        #[test]
        fn each_class_reads_its_own_variable() {
            let admission = Admission::from_env(|name| match name {
                "GRAPH_OWL_ADMISSION_QUERY_PERMITS" => Some("3".to_string()),
                "GRAPH_OWL_ADMISSION_INGESTION_PERMITS" => Some("7".to_string()),
                _ => None,
            })
            .expect("both values are valid");

            assert_eq!(admission.limit(Class::Query), 3);
            assert_eq!(admission.limit(Class::Ingestion), 7);
        }

        #[test]
        fn whitespace_around_a_value_is_tolerated() {
            let admission = Admission::from_env(|name| {
                (name == Class::Query.env()).then(|| "  4 ".to_string())
            })
            .expect("a padded value is still a number");

            assert_eq!(admission.limit(Class::Query), 4);
        }

        #[test]
        fn the_retry_interval_is_configurable() {
            let admission =
                Admission::from_env(|name| (name == RETRY_AFTER_ENV).then(|| "30".to_string()))
                    .expect("valid");

            assert_eq!(admission.retry_after_seconds(), 30);
        }

        /// The acceptance criterion this epic opens with: a refusal **names the
        /// variable**. An operator reading "invalid configuration" has to
        /// bisect their environment.
        #[test]
        fn a_value_that_is_not_a_number_is_refused_naming_the_variable() {
            let error = Admission::from_env(|name| {
                (name == Class::Ingestion.env()).then(|| "lots".to_string())
            })
            .expect_err("`lots` is not a permit count");

            assert_eq!(error.variable, "GRAPH_OWL_ADMISSION_INGESTION_PERMITS");
            assert_eq!(error.value, "lots");
            assert!(
                error
                    .to_string()
                    .contains("GRAPH_OWL_ADMISSION_INGESTION_PERMITS"),
                "{error}"
            );
        }

        #[test]
        fn a_bad_retry_interval_is_refused_naming_its_own_variable() {
            let error =
                Admission::from_env(|name| (name == RETRY_AFTER_ENV).then(|| "soon".to_string()))
                    .expect_err("`soon` is not a number of seconds");

            assert_eq!(error.variable, RETRY_AFTER_ENV);
        }

        /// Zero is a typo far more often than an intent, and honouring it
        /// produces a path that refuses every request forever — which at the
        /// client is indistinguishable from the service being broken.
        #[test]
        fn zero_permits_is_refused_rather_than_honoured() {
            let error =
                Admission::from_env(|name| (name == Class::Query.env()).then(|| "0".to_string()))
                    .expect_err("zero would refuse everything");

            assert_eq!(error.variable, "GRAPH_OWL_ADMISSION_QUERY_PERMITS");
            assert!(error.reason.contains("zero"), "{}", error.reason);
        }

        /// And the boundary immediately above it: one permit is a legitimate,
        /// if severe, configuration. A check written as `<= 1` would pass every
        /// test above and refuse it.
        #[test]
        fn one_permit_is_allowed() {
            let admission =
                Admission::from_env(|name| (name == Class::Query.env()).then(|| "1".to_string()))
                    .expect("one permit is a valid, if severe, setting");

            assert_eq!(admission.limit(Class::Query), 1);
        }

        #[test]
        fn a_negative_permit_count_is_refused() {
            let error =
                Admission::from_env(|name| (name == Class::Query.env()).then(|| "-1".to_string()))
                    .expect_err("a negative count is not a permit count");

            assert_eq!(error.variable, "GRAPH_OWL_ADMISSION_QUERY_PERMITS");
        }
    }

    mod occupancy_is_visible {
        use super::*;

        /// `held` is derived from `available`, so the pair always sums to the
        /// limit. Two independently maintained counters drift, and the drift is
        /// what makes a reader conclude the limit is wrong.
        #[test]
        fn held_and_available_always_sum_to_the_limit() {
            let admission = Admission::with_limits(&[(Class::Query, 3)], 1);

            let _one = admission.try_admit(Class::Query);
            let _two = admission.try_admit(Class::Query);

            let semaphore = &admission.semaphores[&Class::Query];
            let available = semaphore.available_permits();
            let held = admission.limit(Class::Query) - available;

            assert_eq!(available, 1);
            assert_eq!(held, 2);
            assert_eq!(available + held, admission.limit(Class::Query));
        }

        #[test]
        fn a_released_permit_shows_as_available_again() {
            let admission = Admission::with_limits(&[(Class::Query, 2)], 1);

            let held = admission.try_admit(Class::Query);
            assert_eq!(admission.semaphores[&Class::Query].available_permits(), 1);

            drop(held);

            assert_eq!(admission.semaphores[&Class::Query].available_permits(), 2);
        }
    }
}
