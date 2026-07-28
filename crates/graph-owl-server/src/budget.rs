//! Epic 10: the memory footprint, itemized and defended.
//!
//! `00a-product-position.md` states a footprint — "runs in 2 GB" — rather than a
//! ratio, and `10-operability.md` makes the reason explicit: "graph-owl runs in
//! 2 GB" and "graph-owl takes 35% of whatever you give it" cannot both be true,
//! and the second is the one that surprises an operator. So the budget is
//! **absolute and itemized**, and an operator sizing a container adds the
//! numbers instead of reverse-engineering a percentage.
//!
//! Every line is a configuration variable with a stated default, the total is
//! reported at startup and exported as a metric, and the container limit is read
//! **only as a guard** — never as a sizing input, which would reintroduce the
//! percentage model through the back door.

use std::collections::BTreeMap;

/// One line of the budget.
///
/// `built` is the field that keeps the report honest. Five of these six caches
/// do not exist yet, and a startup line claiming 1168 MB when 32 MB is actually
/// allocated tells an operator to size a container for memory nothing will use.
/// The report therefore carries two totals — see [`Budget`].
pub struct Line {
    /// Metric label. A bounded set by construction, per the observability
    /// contract's label discipline: these are the only values it ever takes.
    pub cache: &'static str,
    pub env: &'static str,
    pub default_bytes: u64,
    /// Whether the component this budgets for exists in this build.
    pub built: bool,
    pub owner: &'static str,
}

const MB: u64 = 1024 * 1024;

/// Bytes as the megabytes a human-facing line reports.
///
/// A function rather than an inline `/ MB` at each call site, so the conversion
/// is observable. Every number in the startup report and the refusal message
/// goes through it, and a wrong one is invisible — "1168 MB" and "1224736768 MB"
/// are both plausible-looking until somebody sizes a container from one.
///
/// Truncating, deliberately: a budget reported as larger than it is invites an
/// operator to over-provision, which is the harmless direction, but a *rounded*
/// number that disagrees with the exported byte gauge is a discrepancy somebody
/// has to investigate. Truncation makes the two consistent by construction.
#[must_use]
pub fn megabytes(bytes: u64) -> u64 {
    bytes / MB
}

/// The budget, from `10-operability.md`.
///
/// Ordered as that table orders it, so the two read the same way side by side.
pub static LINES: &[Line] = &[
    Line {
        cache: "compiled_shapes",
        env: "GRAPH_OWL_BUDGET_SHAPES_MB",
        default_bytes: 32 * MB,
        built: false,
        owner: "Epic 5",
    },
    Line {
        cache: "vector_index",
        env: "GRAPH_OWL_BUDGET_VECTOR_MB",
        default_bytes: 512 * MB,
        built: false,
        owner: "Epic 8 — not a cache but a required resident structure, sized by corpus",
    },
    Line {
        cache: "query_plans",
        env: "GRAPH_OWL_BUDGET_QUERY_PLANS_MB",
        default_bytes: 16 * MB,
        built: false,
        owner: "Epic 7",
    },
    Line {
        cache: "authorization_decisions",
        env: "GRAPH_OWL_BUDGET_AUTHZ_MB",
        default_bytes: 32 * MB,
        built: true,
        owner: "Epic 13",
    },
    Line {
        cache: "analytics_results",
        env: "GRAPH_OWL_BUDGET_ANALYTICS_MB",
        default_bytes: 64 * MB,
        built: false,
        owner: "Epic 38",
    },
    Line {
        cache: "reasoning_working_set",
        env: "GRAPH_OWL_BUDGET_REASONING_MB",
        default_bytes: 512 * MB,
        built: false,
        owner: "Epic 6 — per-run, released on completion",
    },
];

/// A configuration value that cannot be honoured.
///
/// Carries the variable's name because `10-operability.md`'s first acceptance
/// criterion is that the service "refuses to start on invalid config, **naming
/// the offending variable**". An operator reading "invalid configuration" has to
/// bisect their environment; one reading the name does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetError {
    pub variable: &'static str,
    pub value: String,
    pub reason: &'static str,
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}={:?} is not usable: {}",
            self.variable, self.value, self.reason
        )
    }
}

/// The configured budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    /// Per-cache bytes, keyed by the metric label. `BTreeMap` so the startup
    /// report and the metric exposition are ordered the same way every run —
    /// a report whose lines move between restarts is one nobody can diff.
    pub lines: BTreeMap<&'static str, u64>,
}

impl Budget {
    /// Read the budget from the environment.
    ///
    /// Takes a reader rather than calling `std::env::var`, so the whole of this
    /// is testable without a process-global mutation that other tests would
    /// race against.
    ///
    /// # Errors
    ///
    /// [`BudgetError`] naming the variable when a value is not a non-negative
    /// integer number of megabytes.
    pub fn from_env(read: impl Fn(&str) -> Option<String>) -> Result<Self, BudgetError> {
        let mut lines = BTreeMap::new();
        for line in LINES {
            let bytes = match read(line.env) {
                None => line.default_bytes,
                Some(raw) => {
                    let trimmed = raw.trim();
                    let megabytes: u64 = trimmed.parse().map_err(|_| BudgetError {
                        variable: line.env,
                        value: raw.clone(),
                        reason: "expected a whole number of megabytes",
                    })?;
                    // Multiplication before any comparison, so a value large
                    // enough to overflow is refused rather than wrapping into a
                    // small budget that silently passes the container check.
                    megabytes.checked_mul(MB).ok_or(BudgetError {
                        variable: line.env,
                        value: raw.clone(),
                        reason: "too large to express in bytes",
                    })?
                }
            };
            lines.insert(line.cache, bytes);
        }
        Ok(Self { lines })
    }

    /// What this build can actually allocate today.
    ///
    /// Only the lines whose component exists. This is the number the container
    /// guard checks, because refusing to start over memory that no code can
    /// reserve would be refusing over a plan.
    #[must_use]
    pub fn allocated_bytes(&self) -> u64 {
        LINES
            .iter()
            .filter(|line| line.built)
            .filter_map(|line| self.lines.get(line.cache))
            .sum()
    }

    /// What the budget becomes when every line's component lands.
    ///
    /// Reported so an operator sizing a container for the finished product has
    /// the number, and warned against rather than refused on — see
    /// [`Verdict`].
    #[must_use]
    pub fn planned_bytes(&self) -> u64 {
        self.lines.values().sum()
    }
}

/// What the container limit says about this budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No cgroup limit, so nothing to check against. Not an error: running
    /// outside a container is normal.
    Unconstrained,
    /// Everything fits, now and when the remaining caches land.
    Fits,
    /// Today's allocation fits, but the finished product will not. A warning
    /// rather than a refusal — the memory is not reserved yet, and refusing to
    /// start over a future line item would block a deployment that works.
    PlannedExceedsLimit { planned: u64, limit: u64 },
    /// What this build allocates does not fit. **Refuse to start.** Failing at
    /// startup with a number is strictly better than being OOM-killed under
    /// load, which arrives later, with no explanation, in production.
    AllocatedExceedsLimit { allocated: u64, limit: u64 },
}

/// Check a budget against a container limit.
///
/// The limit is a *guard*, never a sizing input. Deriving the budget from it
/// would reintroduce the percentage model the positioning rejects.
#[must_use]
pub fn check(budget: &Budget, limit: Option<u64>) -> Verdict {
    let Some(limit) = limit else {
        return Verdict::Unconstrained;
    };
    let allocated = budget.allocated_bytes();
    if allocated > limit {
        return Verdict::AllocatedExceedsLimit { allocated, limit };
    }
    let planned = budget.planned_bytes();
    if planned > limit {
        return Verdict::PlannedExceedsLimit { planned, limit };
    }
    Verdict::Fits
}

/// cgroup v2 writes `max` for "no limit"; v1 writes a sentinel near `u64::MAX`.
///
/// The v1 sentinel is the trap. It is a real number that parses cleanly, and
/// treating it as a limit makes every budget look comfortably within a
/// nine-exabyte container — a check that always passes, which is exactly the
/// failure mode a guard must not have. Anything at or above this is "no limit".
///
/// The value is `u64::MAX` rounded down to a page boundary, which is what the
/// kernel reports; the comparison is `>=` rather than `==` so a different page
/// size cannot slip past it.
const NO_LIMIT_SENTINEL: u64 = 0x7FFF_FFFF_FFFF_F000;

/// Parse one cgroup memory-limit file.
///
/// `None` means "no limit", not "could not read" — both leave the guard with
/// nothing to check, and a caller that distinguished them would have nothing
/// different to do.
#[must_use]
pub fn parse_container_limit(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed == "max" {
        return None;
    }
    let bytes: u64 = trimmed.parse().ok()?;
    if bytes >= NO_LIMIT_SENTINEL || bytes == 0 {
        return None;
    }
    Some(bytes)
}

/// The container's memory limit, if it is running in one.
///
/// cgroup v2 first, then v1. Reading through an injected reader so the decision
/// is testable without a container — and on a developer's machine neither path
/// exists, which is the `Unconstrained` case and is correct rather than an
/// error.
#[must_use]
pub fn container_limit(read: impl Fn(&str) -> Option<String>) -> Option<u64> {
    const V2: &str = "/sys/fs/cgroup/memory.max";
    const V1: &str = "/sys/fs/cgroup/memory/memory.limit_in_bytes";

    read(V2)
        .and_then(|raw| parse_container_limit(&raw))
        .or_else(|| read(V1).and_then(|raw| parse_container_limit(&raw)))
}

/// Report the budget at startup, and say whether it fits.
///
/// # Errors
///
/// The refusal message when what this build allocates exceeds the container
/// limit. Returned rather than logged-and-continued, so the caller decides to
/// stop — and `main` does.
pub fn report(budget: &Budget, limit: Option<u64>) -> Result<(), String> {
    for line in LINES {
        let bytes = budget.lines.get(line.cache).copied().unwrap_or(0);
        // Exported for every line, built or not: an operator planning capacity
        // needs the whole table, and a gauge that appears only once its epic
        // ships is a gauge whose absence looks like a scrape failure.
        metrics::gauge!("graph_owl_memory_budget_bytes", "cache" => line.cache).set(bytes as f64);
    }
    metrics::gauge!("graph_owl_memory_budget_allocated_bytes").set(budget.allocated_bytes() as f64);
    metrics::gauge!("graph_owl_memory_budget_planned_bytes").set(budget.planned_bytes() as f64);

    let allocated_mb = megabytes(budget.allocated_bytes());
    let planned_mb = megabytes(budget.planned_bytes());

    match check(budget, limit) {
        Verdict::AllocatedExceedsLimit { allocated, limit } => Err(format!(
            "the configured memory budget does not fit this container: {} MB allocated, \
             {} MB available. Lower a GRAPH_OWL_BUDGET_* value or raise the container \
             limit. Refusing at startup with a number is better than an OOM kill under load.",
            megabytes(allocated),
            megabytes(limit)
        )),
        Verdict::PlannedExceedsLimit { planned, limit } => {
            tracing::warn!(
                allocated_mb,
                planned_mb,
                limit_mb = megabytes(limit),
                "memory budget fits today and will not when the remaining caches land \
                 ({} MB planned against a {} MB container). Nothing is reserved yet, so \
                 this is a sizing warning rather than a refusal.",
                megabytes(planned),
                megabytes(limit)
            );
            Ok(())
        }
        Verdict::Fits | Verdict::Unconstrained => {
            tracing::info!(
                allocated_mb,
                planned_mb,
                "memory budget: {allocated_mb} MB allocated by this build, {planned_mb} MB \
                 when every cache lands"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing_configured(_: &str) -> Option<String> {
        None
    }

    fn defaults() -> Budget {
        Budget::from_env(nothing_configured).expect("defaults are valid")
    }

    mod what_the_defaults_say {
        use super::*;

        #[test]
        fn every_line_in_the_plan_has_a_default() {
            let budget = defaults();

            assert_eq!(budget.lines.len(), LINES.len());
            for line in LINES {
                assert_eq!(budget.lines.get(line.cache), Some(&line.default_bytes));
            }
        }

        /// **The distinction that keeps the report honest.** Five of six caches
        /// do not exist; a startup line claiming the full table tells an
        /// operator to size a container for memory nothing will use.
        #[test]
        fn allocated_counts_only_what_this_build_can_reserve() {
            let budget = defaults();

            assert_eq!(
                budget.allocated_bytes(),
                32 * MB,
                "the decision cache alone"
            );
            assert_eq!(budget.planned_bytes(), 1168 * MB, "the whole table");
            assert!(budget.allocated_bytes() < budget.planned_bytes());
        }

        /// And the negative: `built` has to mean something. If every line were
        /// marked built the two totals would coincide and the distinction above
        /// would be decoration.
        #[test]
        fn exactly_the_built_caches_are_counted_as_allocated() {
            let built: u64 = LINES
                .iter()
                .filter(|l| l.built)
                .map(|l| l.default_bytes)
                .sum();

            assert_eq!(defaults().allocated_bytes(), built);
            assert!(LINES.iter().any(|l| !l.built), "or the split is pointless");
        }
    }

    mod bytes_are_bytes {
        use super::*;

        /// **Pinned to literal bytes, not to `MB`.** Every other assertion here
        /// writes `32 * MB` on both sides, so redefining `MB` changes both and
        /// cancels out — the unit could drift to anything and the suite would
        /// stay green. This is the one place that says what a megabyte is.
        #[test]
        fn a_megabyte_is_1048576_bytes_and_the_defaults_are_in_them() {
            assert_eq!(MB, 1_048_576);
            assert_eq!(defaults().lines["query_plans"], 16_777_216);
            assert_eq!(defaults().lines["authorization_decisions"], 33_554_432);
            assert_eq!(defaults().planned_bytes(), 1_224_736_768);
        }

        /// The reported number and the exported gauge have to agree. An
        /// operator reading "32 MB" in the log and `33554432` in Prometheus
        /// must not have to wonder whether they are the same figure.
        #[test]
        fn megabytes_reports_what_the_byte_gauge_would_show() {
            assert_eq!(megabytes(33_554_432), 32);
            assert_eq!(megabytes(1_224_736_768), 1168);
            assert_eq!(megabytes(0), 0);
        }

        /// Truncating, not rounding — so the reported figure never exceeds the
        /// byte figure it summarises.
        #[test]
        fn a_partial_megabyte_truncates_rather_than_rounding_up() {
            assert_eq!(megabytes(MB - 1), 0);
            assert_eq!(megabytes(MB + MB - 1), 1);
        }
    }

    mod configuration_is_honoured_or_refused_by_name {
        use super::*;

        #[test]
        fn a_configured_line_overrides_its_default() {
            let budget = Budget::from_env(|name| {
                (name == "GRAPH_OWL_BUDGET_AUTHZ_MB").then(|| "8".to_string())
            })
            .expect("valid");

            assert_eq!(budget.lines["authorization_decisions"], 8 * MB);
            assert_eq!(budget.allocated_bytes(), 8 * MB);
        }

        #[test]
        fn zero_is_a_legitimate_budget_meaning_do_not_cache() {
            let budget = Budget::from_env(|name| {
                (name == "GRAPH_OWL_BUDGET_AUTHZ_MB").then(|| "0".to_string())
            })
            .expect("valid");

            assert_eq!(budget.allocated_bytes(), 0);
        }

        #[test]
        fn whitespace_around_a_value_is_not_a_configuration_error() {
            let budget = Budget::from_env(|name| {
                (name == "GRAPH_OWL_BUDGET_AUTHZ_MB").then(|| "  16  ".to_string())
            })
            .expect("valid");

            assert_eq!(budget.lines["authorization_decisions"], 16 * MB);
        }

        /// The acceptance criterion is that the refusal **names the offending
        /// variable**. An operator reading "invalid configuration" has to bisect
        /// their environment; one reading the name does not.
        #[test]
        fn an_unparseable_value_is_refused_and_names_its_variable() {
            for bad in ["many", "-1", "16MB", "1.5", ""] {
                let error = Budget::from_env(|name| {
                    (name == "GRAPH_OWL_BUDGET_QUERY_PLANS_MB").then(|| bad.to_string())
                })
                .expect_err("must refuse");

                assert_eq!(error.variable, "GRAPH_OWL_BUDGET_QUERY_PLANS_MB");
                assert!(
                    error
                        .to_string()
                        .contains("GRAPH_OWL_BUDGET_QUERY_PLANS_MB"),
                    "{error}"
                );
            }
        }

        /// A value large enough to overflow must be refused, not wrapped. A
        /// wrap produces a *small* budget that sails through the container
        /// check — the guard passing because the number became nonsense.
        #[test]
        fn a_value_that_would_overflow_is_refused_rather_than_wrapping() {
            let error = Budget::from_env(|name| {
                (name == "GRAPH_OWL_BUDGET_AUTHZ_MB").then(|| u64::MAX.to_string())
            })
            .expect_err("must refuse");

            assert_eq!(error.variable, "GRAPH_OWL_BUDGET_AUTHZ_MB");
            assert!(error.reason.contains("too large"), "{error}");
        }
    }

    mod the_container_limit_is_a_guard {
        use super::*;

        #[test]
        fn no_limit_means_nothing_to_check() {
            assert_eq!(check(&defaults(), None), Verdict::Unconstrained);
        }

        #[test]
        fn a_generous_container_fits_everything() {
            assert_eq!(check(&defaults(), Some(4096 * MB)), Verdict::Fits);
        }

        /// Today's allocation fits; the finished product will not. A warning,
        /// because nothing is reserved yet and refusing over a future line item
        /// would block a deployment that works.
        #[test]
        fn a_container_too_small_for_the_plan_is_a_warning_not_a_refusal() {
            assert_eq!(
                check(&defaults(), Some(512 * MB)),
                Verdict::PlannedExceedsLimit {
                    planned: 1168 * MB,
                    limit: 512 * MB
                }
            );
        }

        /// And the one that stops the process. Failing at startup with a number
        /// beats an OOM kill under load, which arrives later and explains
        /// nothing.
        #[test]
        fn a_container_too_small_for_what_is_allocated_is_a_refusal() {
            assert_eq!(
                check(&defaults(), Some(16 * MB)),
                Verdict::AllocatedExceedsLimit {
                    allocated: 32 * MB,
                    limit: 16 * MB
                }
            );
        }

        /// Exactly the limit fits. A budget equal to the container is fully
        /// spent, not overspent, and refusing it would make the documented
        /// number unusable as a number.
        #[test]
        fn a_budget_equal_to_the_limit_fits() {
            assert_eq!(check(&defaults(), Some(1168 * MB)), Verdict::Fits);
            assert!(matches!(
                check(&defaults(), Some(1168 * MB - 1)),
                Verdict::PlannedExceedsLimit { .. }
            ));
        }

        /// **The boundary the guard turns on.** A budget exactly equal to the
        /// container is fully spent, not overspent. Refusing it would make the
        /// documented number unusable as a number — an operator who sets the
        /// budget to the limit has done the arithmetic right and would be told
        /// they had not.
        #[test]
        fn an_allocation_exactly_equal_to_the_limit_is_not_a_refusal() {
            let budget = Budget::from_env(|name| {
                (name == "GRAPH_OWL_BUDGET_AUTHZ_MB").then(|| "32".to_string())
            })
            .expect("valid");

            assert_eq!(budget.allocated_bytes(), 32 * MB);
            assert!(
                !matches!(
                    check(&budget, Some(32 * MB)),
                    Verdict::AllocatedExceedsLimit { .. }
                ),
                "a budget that exactly fits must start"
            );
            assert!(matches!(
                check(&budget, Some(32 * MB - 1)),
                Verdict::AllocatedExceedsLimit { .. }
            ));
        }

        #[test]
        fn the_refusal_reports_both_numbers_so_an_operator_can_act() {
            let message = report(&defaults(), Some(16 * MB)).expect_err("refused");

            assert!(message.contains("32 MB"), "{message}");
            assert!(message.contains("16 MB"), "{message}");
            assert!(message.contains("GRAPH_OWL_BUDGET"), "{message}");
        }

        #[test]
        fn a_fitting_budget_reports_without_refusing() {
            assert!(report(&defaults(), Some(4096 * MB)).is_ok());
            assert!(report(&defaults(), None).is_ok());
        }
    }

    mod reading_the_limit_from_a_cgroup {
        use super::*;

        #[test]
        fn a_plain_byte_count_is_the_limit() {
            assert_eq!(parse_container_limit("536870912"), Some(536_870_912));
            assert_eq!(parse_container_limit(" 536870912\n"), Some(536_870_912));
        }

        /// cgroup v2's "no limit".
        #[test]
        fn max_is_no_limit() {
            assert_eq!(parse_container_limit("max"), None);
            assert_eq!(parse_container_limit("max\n"), None);
        }

        /// **The trap.** cgroup v1 writes a sentinel near `u64::MAX` rather than
        /// a word, and it parses cleanly. Treating it as a limit makes every
        /// budget fit inside a nine-exabyte container — a guard that always
        /// passes, which is the one failure mode a guard must not have.
        #[test]
        fn the_v1_unlimited_sentinel_is_not_a_limit() {
            assert_eq!(parse_container_limit("9223372036854771712"), None);
            assert_eq!(parse_container_limit(&u64::MAX.to_string()), None);
        }

        #[test]
        fn nonsense_is_no_limit_rather_than_a_zero_sized_container() {
            for raw in ["", "unlimited", "-1", "1.5", "0"] {
                assert_eq!(parse_container_limit(raw), None, "{raw:?}");
            }
        }

        #[test]
        fn cgroup_v2_is_preferred_over_v1() {
            let limit = container_limit(|path| {
                Some(match path {
                    "/sys/fs/cgroup/memory.max" => "268435456".to_string(),
                    _ => "536870912".to_string(),
                })
            });

            assert_eq!(limit, Some(268_435_456), "v2 wins where both exist");
        }

        #[test]
        fn v1_is_read_when_v2_is_absent() {
            let limit = container_limit(|path| {
                (path == "/sys/fs/cgroup/memory/memory.limit_in_bytes")
                    .then(|| "536870912".to_string())
            });

            assert_eq!(limit, Some(536_870_912));
        }

        /// v2 present but unlimited must fall through to v1 rather than
        /// stopping — a `max` in v2 is not an answer, it is the absence of one.
        #[test]
        fn an_unlimited_v2_falls_through_to_v1() {
            let limit = container_limit(|path| {
                Some(match path {
                    "/sys/fs/cgroup/memory.max" => "max".to_string(),
                    _ => "536870912".to_string(),
                })
            });

            assert_eq!(limit, Some(536_870_912));
        }

        /// A developer's machine has neither. That is `Unconstrained`, which is
        /// correct rather than an error.
        #[test]
        fn no_cgroup_at_all_is_no_limit() {
            assert_eq!(container_limit(|_| None), None);
        }
    }
}
