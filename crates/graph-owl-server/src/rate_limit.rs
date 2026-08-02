//! Epic 18 Slice E: per-endpoint webhook rate limiting.
//!
//! `01-api-conventions.md` treats rate limiting as an ingress concern,
//! "unless per-principal quotas are ever required." A registered webhook
//! endpoint is exactly that exception: a distinct external sender with its
//! own identity, and the criterion this module exists for is "one sender
//! flooding must not cost every other sender their traffic" — the same
//! isolation reasoning `admission::Class` already applies between query and
//! ingestion, narrowed to one endpoint instead of one route class.
//!
//! **The limit is per-endpoint configuration, never a global default.**
//! `admission::DEFAULT_PERMITS` derives its number from a real fact (the
//! Postgres pool size); no equivalent fact exists for "requests per minute
//! from an external sender" — different integrations legitimately have
//! different expected volumes, and inventing one number for all of them
//! would be exactly the unreasoned magic constant this project's licensing
//! rule already forbids for a different reason. `None` means unlimited, set
//! by whoever registers the endpoint and knows its sender's real traffic.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const WINDOW: Duration = Duration::from_secs(60);

/// One endpoint's current fixed window.
struct Window {
    started_at: Instant,
    count: u32,
}

/// Refused, naming how long to wait.
///
/// Seconds remaining in the current window, never zero: `Retry-After: 0`
/// tells a client to retry immediately, which is indistinguishable from no
/// rate limit at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimited {
    pub retry_after_seconds: u64,
}

/// A fixed one-minute window per endpoint, not a token bucket: the
/// configured limit is itself named "per minute", so the window checked
/// against is the same unit an operator configured it in — a token bucket's
/// smoother admission curve would answer a question ("what is the
/// sustained rate") nobody configuring this endpoint asked.
pub struct RateLimiter {
    windows: Mutex<HashMap<Uuid, Window>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Admits a request against `endpoint`'s own `limit_per_minute`.
    /// `None` never refuses — an endpoint with no configured limit is
    /// unlimited, not defaulted to some number this module would have to
    /// invent.
    ///
    /// # Errors
    ///
    /// [`RateLimited`] when the endpoint's own limit is exceeded within the
    /// current window.
    pub fn try_admit(
        &self,
        endpoint: Uuid,
        limit_per_minute: Option<u32>,
    ) -> Result<(), RateLimited> {
        self.try_admit_at(endpoint, limit_per_minute, Instant::now())
    }

    /// Takes an explicit `now` so tests can move time without a real sleep
    /// — the same reason `Admission::try_admit` is synchronous and
    /// `try_acquire`-shaped: a test that has to wait in real time to prove
    /// a window rolls over is a test nobody runs twice.
    fn try_admit_at(
        &self,
        endpoint: Uuid,
        limit_per_minute: Option<u32>,
        now: Instant,
    ) -> Result<(), RateLimited> {
        let Some(limit) = limit_per_minute else {
            return Ok(());
        };
        let mut windows = self.windows.lock().unwrap();
        let window = windows.entry(endpoint).or_insert(Window {
            started_at: now,
            count: 0,
        });
        if now.duration_since(window.started_at) >= WINDOW {
            window.started_at = now;
            window.count = 0;
        }
        if window.count >= limit {
            let elapsed = now.duration_since(window.started_at);
            let remaining = WINDOW.saturating_sub(elapsed);
            return Err(RateLimited {
                retry_after_seconds: remaining.as_secs().max(1),
            });
        }
        window.count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The public entry point, not just its `_at` internals — proving
    /// `try_admit` actually delegates to the real-clock check rather than
    /// being a stub that always admits.
    #[test]
    fn the_public_entry_point_admits_within_limit_and_refuses_past_it() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();

        assert!(limiter.try_admit(endpoint, Some(1)).is_ok());
        assert!(
            limiter.try_admit(endpoint, Some(1)).is_err(),
            "a second call within the same minute must be refused"
        );
    }

    #[test]
    fn no_configured_limit_never_refuses() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();
        let now = Instant::now();
        for _ in 0..1000 {
            assert!(limiter.try_admit_at(endpoint, None, now).is_ok());
        }
    }

    #[test]
    fn requests_up_to_the_limit_are_admitted() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();
        let now = Instant::now();
        for _ in 0..5 {
            assert!(limiter.try_admit_at(endpoint, Some(5), now).is_ok());
        }
    }

    /// **The rejection test, synchronous on purpose** — same reasoning as
    /// `admission`'s: a waiting implementation cannot return `Err` here at
    /// all, only block, and this has to prove refusal without a real sleep.
    #[test]
    fn the_request_past_the_limit_is_refused() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();
        let now = Instant::now();
        for _ in 0..3 {
            limiter
                .try_admit_at(endpoint, Some(3), now)
                .expect("within limit");
        }
        assert!(
            limiter.try_admit_at(endpoint, Some(3), now).is_err(),
            "the 4th request in the same window must be refused"
        );
    }

    #[test]
    fn the_refusal_names_seconds_remaining_in_the_window() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();
        let now = Instant::now();
        limiter
            .try_admit_at(endpoint, Some(1), now)
            .expect("first admitted");
        let thirty_seconds_later = now + Duration::from_secs(30);
        let refused = limiter
            .try_admit_at(endpoint, Some(1), thirty_seconds_later)
            .expect_err("second refused");
        assert_eq!(refused.retry_after_seconds, 30);
    }

    /// Never zero: a `Retry-After: 0` is indistinguishable from no limit at
    /// all, and a client honoring it would retry immediately into the same
    /// refusal.
    #[test]
    fn retry_after_is_never_reported_as_zero() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();
        let now = Instant::now();
        limiter
            .try_admit_at(endpoint, Some(1), now)
            .expect("first admitted");
        // Right at the edge of the window — real elapsed time would round
        // to zero seconds remaining without the `.max(1)`.
        let almost_expired = now + Duration::from_millis(59_999);
        let refused = limiter
            .try_admit_at(endpoint, Some(1), almost_expired)
            .expect_err("still refused, window not yet rolled over");
        assert!(refused.retry_after_seconds >= 1);
    }

    /// **The window rolling over.** Without this, a limiter that never reset
    /// would refuse an endpoint forever after its first burst — as wrong in
    /// the other direction as a limit that never refuses.
    #[test]
    fn a_new_window_resets_the_count() {
        let limiter = RateLimiter::new();
        let endpoint = Uuid::new_v4();
        let now = Instant::now();
        limiter
            .try_admit_at(endpoint, Some(1), now)
            .expect("first admitted");
        assert!(limiter.try_admit_at(endpoint, Some(1), now).is_err());

        let next_window = now + Duration::from_secs(61);
        assert!(
            limiter.try_admit_at(endpoint, Some(1), next_window).is_ok(),
            "a new window must readmit"
        );
    }

    /// **Isolation between endpoints** — the whole reason this is per-endpoint
    /// rather than one shared counter. One endpoint saturating its own limit
    /// must not cost a different endpoint its traffic.
    #[test]
    fn saturating_one_endpoint_does_not_refuse_another() {
        let limiter = RateLimiter::new();
        let noisy = Uuid::new_v4();
        let quiet = Uuid::new_v4();
        let now = Instant::now();

        limiter
            .try_admit_at(noisy, Some(1), now)
            .expect("first admitted");
        assert!(limiter.try_admit_at(noisy, Some(1), now).is_err());

        assert!(
            limiter.try_admit_at(quiet, Some(1), now).is_ok(),
            "a different endpoint must have its own budget"
        );
    }
}
