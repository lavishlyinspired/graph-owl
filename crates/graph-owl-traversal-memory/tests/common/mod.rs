//! A minimal, in-memory `TripleStore` — test-only, seeding the same kind of
//! backing store the real Postgres adapter reads from, so `InMemoryTraversalEngine`
//! can be exercised without a database.
//!
//! Resolution matches `graph-owl-api::projection_isolation_tests::RecordingGraph`'s
//! own documented contract: newest row per fact identity wins, and on a tie the
//! retraction wins — the same current-state semantics every real `TripleStore`
//! implementation in this project shares.

use async_trait::async_trait;
use graph_owl_core::flake::{Flake, TriplePattern};
use graph_owl_engine::{EngineError, TripleStore};
use std::sync::Mutex;

pub struct InMemoryTripleStore {
    flakes: Mutex<Vec<Flake>>,
    clock: std::sync::atomic::AtomicI64,
}

impl InMemoryTripleStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            flakes: Mutex::new(Vec::new()),
            clock: std::sync::atomic::AtomicI64::new(0),
        }
    }

    /// Test-only convenience: writes and panics on failure, matching the
    /// `.expect("write")` every caller in the ported differential suite uses.
    pub async fn seed(&self, flakes: &[Flake]) {
        self.assert_flakes(flakes).await.expect("write");
    }

    /// Test-only convenience: retracts and panics on failure, matching
    /// `.expect("retract")` in the ported suite.
    pub async fn retract(&self, flakes: &[Flake]) {
        self.retract_flakes(flakes).await.expect("retract");
    }

    fn resolve(&self, pattern: &TriplePattern) -> Vec<Flake> {
        let flakes = self.flakes.lock().expect("lock");
        let mut latest: std::collections::HashMap<String, &Flake> =
            std::collections::HashMap::new();

        for flake in flakes
            .iter()
            .filter(|f| pattern.as_of.is_none_or(|t| f.t <= t))
            .filter(|f| pattern.s.as_ref().is_none_or(|s| &f.s == s))
            .filter(|f| pattern.p.as_ref().is_none_or(|p| &f.p == p))
            .filter(|f| pattern.o.as_ref().is_none_or(|o| &f.o == o))
            .filter(|f| pattern.cx.as_ref().is_none_or(|cx| &f.cx == cx))
        {
            let identity = format!("{:?}|{:?}|{:?}|{:?}", flake.s, flake.p, flake.o, flake.cx);
            match latest.get(&identity) {
                Some(seen) if seen.t > flake.t => {}
                Some(seen) if seen.t == flake.t && !seen.op => {}
                _ => {
                    latest.insert(identity, flake);
                }
            }
        }

        latest.into_values().filter(|f| f.op).cloned().collect()
    }
}

impl Default for InMemoryTripleStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TripleStore for InMemoryTripleStore {
    async fn assert_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
        self.flakes.lock().expect("lock").extend_from_slice(flakes);
        Ok(())
    }

    async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError> {
        self.flakes
            .lock()
            .expect("lock")
            .extend(flakes.iter().map(|f| Flake {
                op: false,
                ..f.clone()
            }));
        Ok(())
    }

    async fn query_pattern(&self, pattern: &TriplePattern) -> Result<Vec<Flake>, EngineError> {
        Ok(self.resolve(pattern))
    }

    async fn count(&self, pattern: &TriplePattern) -> Result<u64, EngineError> {
        Ok(self.resolve(pattern).len() as u64)
    }

    async fn next_time(&self) -> Result<i64, EngineError> {
        Ok(self.clock.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1)
    }

    async fn time_at(
        &self,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<i64>, EngineError> {
        Ok(None)
    }
}
