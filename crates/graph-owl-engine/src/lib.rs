//! The graph engine port: what a triple store must do, independent of where
//! the flakes actually live.
//!
//! Implemented by `graph-owl-engine-postgres`. Kept separate from
//! `graph-owl-storage` because the two answer different questions — storage
//! owns the entity rows that are the source of truth, this owns the graph
//! projection of them (`plans/04-engine-triples.md` decision 1).

use async_trait::async_trait;
use graph_owl_core::flake::{Flake, FlakeValue, Sid, TriplePattern, namespace};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    /// A flake carried namespace code 0, which is reserved for "unset".
    /// Storing it would put an undiagnosable row in the graph, so it is
    /// refused at the boundary rather than written and puzzled over later.
    #[error(
        "namespace code 0 is reserved for unset; the {position} of this flake is uninitialized"
    )]
    UnsetNamespace { position: &'static str },

    #[error("engine backend failed: {0}")]
    Backend(String),
}

/// Storage and retrieval of flakes.
///
/// Deliberately not one method per query shape: [`query_pattern`] takes a
/// pattern with any combination of bound and unbound terms, and the adapter
/// picks the index. A method per shape would push index selection into every
/// caller, which is exactly the knowledge the adapter exists to hold.
///
/// [`query_pattern`]: TripleStore::query_pattern
#[async_trait]
pub trait TripleStore: Send + Sync {
    /// Write flakes. One statement per call regardless of batch size — a
    /// projection of a wide table is hundreds of flakes, and a round trip
    /// each would make projection the slowest part of every write.
    ///
    /// Re-asserting an identical flake at the same `t` is a no-op, so a
    /// retried projection converges rather than duplicating.
    ///
    /// # Errors
    ///
    /// [`EngineError::UnsetNamespace`] if any flake carries namespace 0;
    /// [`EngineError::Backend`] if the write fails.
    async fn assert_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError>;

    /// Withdraw facts, by writing a retraction row for each.
    ///
    /// **Never a delete.** The asserting row stays, which is the entire reason
    /// history here is recoverable by construction rather than reconstructed
    /// from a parallel audit table that can drift.
    ///
    /// `op` is forced to `false` on every flake regardless of what it carries,
    /// so a caller can hand this the original assertion and get that fact's
    /// retraction — which is exactly what a projection update does.
    ///
    /// Retracting a fact that was never asserted is a no-op, not an error: a
    /// reconciler re-projecting an entity cannot know which facts the previous
    /// projection managed to write.
    ///
    /// # Errors
    ///
    /// [`EngineError::UnsetNamespace`] if any flake carries namespace 0 — a
    /// retraction that names no valid fact would withdraw nothing while
    /// reporting success;
    /// [`EngineError::Backend`] if the write fails.
    async fn retract_flakes(&self, flakes: &[Flake]) -> Result<(), EngineError>;

    /// Flakes matching the pattern, in current state unless the pattern names
    /// an `as_of`. Retracted facts are excluded; the rows recording them are
    /// not deleted.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn query_pattern(&self, pattern: &TriplePattern) -> Result<Vec<Flake>, EngineError>;

    /// How many flakes the pattern matches.
    ///
    /// Must agree with `query_pattern(..).len()` for the same pattern — a
    /// count computed by a different path than the rows is a count that can
    /// disagree with them, and the disagreement always surfaces as a paging
    /// bug rather than as a count bug.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the query fails.
    async fn count(&self, pattern: &TriplePattern) -> Result<u64, EngineError>;

    /// Reserve the next transaction time.
    ///
    /// Every flake in one logical change shares the `t` this returns, which is
    /// what makes "the state after change N" a well-defined thing to ask for.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the clock cannot be advanced.
    async fn next_time(&self) -> Result<i64, EngineError>;

    /// The newest transaction time at or before `at`.
    ///
    /// `None` means nothing had happened yet — which is a different answer
    /// from "the entity did not exist yet", and callers must not collapse the
    /// two: the first says the graph is younger than the question.
    ///
    /// # Errors
    ///
    /// [`EngineError::Backend`] if the lookup fails.
    async fn time_at(&self, at: chrono::DateTime<chrono::Utc>) -> Result<Option<i64>, EngineError>;
}

/// A predicate's definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredicateDef {
    pub namespace: u16,
    pub name: String,
    /// Which [`graph_owl_core::flake::FlakeValue`] variant objects must be.
    pub value_type: i16,
    /// `false` = at most one value per subject.
    ///
    /// Cardinality belongs to the predicate, not the writer: `dsc:name` is
    /// single-valued for everyone, and leaving it to each caller means the
    /// first one that forgets gives a table two names with nothing to say
    /// which is current.
    pub many: bool,
    /// Ships with the binary and cannot be redefined at runtime.
    pub core: bool,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("predicate {namespace}:{name} is already defined")]
    Duplicate { namespace: u16, name: String },

    /// Redefining a core predicate would not migrate the flakes already
    /// written against it — it would make every one of them unreadable.
    #[error("{namespace}:{name} is a core predicate and cannot be redefined")]
    CoreImmutable { namespace: u16, name: String },

    #[error("registry backend failed: {0}")]
    Backend(String),
}

/// Predicates definable at runtime, so an organisation can extend the
/// vocabulary without a release.
#[async_trait]
pub trait PredicateRegistry: Send + Sync {
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if `(namespace, name)` already exists;
    /// [`RegistryError::CoreImmutable`] if it names a core predicate.
    async fn define(&self, definition: &PredicateDef) -> Result<(), RegistryError>;

    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the lookup fails.
    async fn lookup(
        &self,
        namespace: u16,
        name: &str,
    ) -> Result<Option<PredicateDef>, RegistryError>;

    /// Every definition, or those in one namespace.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Backend`] if the query fails.
    async fn list(&self, namespace: Option<u16>) -> Result<Vec<PredicateDef>, RegistryError>;
}

/// Rejects a flake whose subject, predicate, graph or reference object carries
/// a namespace that was never set.
///
/// Lives here rather than in the adapter so every backend refuses the same
/// rows: an adapter-local check makes validity a property of which backend you
/// happen to be running.
///
/// # Errors
///
/// [`EngineError::UnsetNamespace`] naming the offending position.
pub fn reject_unset_namespaces(flakes: &[Flake]) -> Result<(), EngineError> {
    for flake in flakes {
        check(&flake.s, "subject")?;
        check(&flake.p, "predicate")?;
        if let Some(cx) = &flake.cx {
            check(cx, "graph")?;
        }
        if let FlakeValue::Ref(o) = &flake.o {
            check(o, "object")?;
        }
    }
    Ok(())
}

fn check(sid: &Sid, position: &'static str) -> Result<(), EngineError> {
    if sid.namespace_code == namespace::UNSET {
        return Err(EngineError::UnsetNamespace { position });
    }
    Ok(())
}

#[cfg(test)]
mod unset_namespace_tests {
    use super::*;

    fn valid() -> Flake {
        Flake::assert(
            Sid::dsc("table-1"),
            Sid::dsc("name"),
            FlakeValue::String("upi_transactions".into()),
            1,
        )
    }

    #[test]
    fn a_fully_initialized_flake_is_accepted() {
        assert!(reject_unset_namespaces(&[valid()]).is_ok());
    }

    /// Each position is checked separately, because a check that only covers
    /// the subject lets an uninitialized predicate through — and an
    /// uninitialized predicate makes the flake unqueryable rather than merely
    /// wrong.
    #[test]
    fn every_sid_position_is_checked() {
        let cases = [
            (
                "subject",
                Flake {
                    s: Sid::new(namespace::UNSET, "x"),
                    ..valid()
                },
            ),
            (
                "predicate",
                Flake {
                    p: Sid::new(namespace::UNSET, "x"),
                    ..valid()
                },
            ),
            (
                "graph",
                Flake {
                    cx: Some(Sid::new(namespace::UNSET, "x")),
                    ..valid()
                },
            ),
            (
                "object",
                Flake {
                    o: FlakeValue::Ref(Sid::new(namespace::UNSET, "x")),
                    ..valid()
                },
            ),
        ];
        for (position, flake) in cases {
            let error =
                reject_unset_namespaces(&[flake]).expect_err("an unset namespace must be refused");
            assert!(
                matches!(&error, EngineError::UnsetNamespace { position: p } if *p == position),
                "expected position {position}, got {error:?}"
            );
        }
    }

    /// A literal object has no namespace to check. Reaching into it anyway
    /// would reject every string-valued flake in the catalog.
    #[test]
    fn a_literal_object_carries_no_namespace_to_reject() {
        let flake = Flake {
            o: FlakeValue::String(String::new()),
            ..valid()
        };
        assert!(reject_unset_namespaces(&[flake]).is_ok());
    }

    /// The default graph is `None`, not namespace 0. Confusing the two would
    /// reject every flake in the default graph, which is nearly all of them.
    #[test]
    fn the_default_graph_is_absence_not_an_unset_namespace() {
        let flake = Flake {
            cx: None,
            ..valid()
        };
        assert!(reject_unset_namespaces(&[flake]).is_ok());
    }

    /// The scan must not stop at the first flake — a batch is written as one
    /// statement, so one bad flake anywhere in it poisons the whole write.
    #[test]
    fn a_bad_flake_later_in_a_batch_is_still_caught() {
        let batch = [
            valid(),
            valid(),
            Flake {
                p: Sid::new(namespace::UNSET, "x"),
                ..valid()
            },
        ];
        assert!(reject_unset_namespaces(&batch).is_err());
    }

    #[test]
    fn an_empty_batch_is_vacuously_valid() {
        assert!(reject_unset_namespaces(&[]).is_ok());
    }

    #[test]
    fn the_error_names_the_position_so_the_bad_field_is_findable() {
        let error = reject_unset_namespaces(&[Flake {
            p: Sid::new(namespace::UNSET, "x"),
            ..valid()
        }])
        .expect_err("must reject");
        assert!(error.to_string().contains("predicate"), "got {error}");
    }
}
