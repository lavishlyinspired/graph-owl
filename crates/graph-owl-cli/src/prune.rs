//! Scoped, guarded pruning — Epic 20 Slice D.
//!
//! Reuses Epic 15's deletion-detection shape rather than inventing a second
//! one, because the hazard is identical: a misconfigured source (there, a
//! connector enumeration; here, a declaration directory) that reports fewer
//! entities than exist can tombstone a catalog. Two independent guards, both
//! required — scope and a threshold.

use crate::plan::{Change, Plan};

/// What the declared scope covers.
///
/// **Decision 2**: declarations are authoritative only within their declared
/// scope, and a tree scoped to one service never touches anything outside it.
/// Without this, one misconfigured repository can tombstone a whole catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// FQN prefixes this directory claims authority over. Empty means the
    /// scope was never declared — which is refused rather than treated as
    /// "everything", since "I forgot to say" and "I mean the entire catalog"
    /// must not be the same instruction.
    pub prefixes: Vec<String>,
}

impl Scope {
    #[must_use]
    pub fn covers(&self, fully_qualified_name: &str) -> bool {
        self.prefixes.iter().any(|prefix| {
            fully_qualified_name == prefix
                || fully_qualified_name.starts_with(&format!("{prefix}."))
        })
    }
}

/// Why a prune was refused, so the message can say what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No scope declared. Refusing is the whole point: an undeclared scope
    /// treated as "everything" is how a repository that declares three
    /// entities deletes thirty thousand.
    NoScope,
    /// More entities would be pruned than the threshold allows.
    OverThreshold {
        would_prune: usize,
        threshold: usize,
    },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NoScope => write!(
                f,
                "refusing to prune: this directory declares no scope, and an \
                 undeclared scope is not the same instruction as \"the whole catalog\""
            ),
            Refusal::OverThreshold {
                would_prune,
                threshold,
            } => write!(
                f,
                "refusing to prune: {would_prune} entities would be tombstoned, over the \
                 threshold of {threshold}. Raise it deliberately if that is genuinely intended"
            ),
        }
    }
}

/// The number of prunes above which a run refuses rather than proceeds.
///
/// **Not a tuning number, and deliberately small.** The guard exists to catch
/// a directory that is wrong — a bad path, an empty checkout, a scope typo —
/// and those failures are usually total rather than marginal. A large default
/// would let exactly the catastrophic case through while blocking only the
/// harmless ones. An operator who genuinely means to remove more says so.
pub const DEFAULT_PRUNE_THRESHOLD: usize = 10;

/// Decides whether a plan's prunes may proceed.
///
/// # Errors
///
/// [`Refusal`] when no scope is declared, or when the prune count exceeds
/// `threshold`. **Nothing is deleted in either case** — a partial prune up to
/// the limit would be the worst outcome of the three, leaving the catalog in
/// a state neither the files nor the operator described.
pub fn authorize(plan: &Plan, scope: &Scope, threshold: usize) -> Result<Vec<String>, Refusal> {
    if scope.prefixes.is_empty() {
        return Err(Refusal::NoScope);
    }

    let to_prune: Vec<String> = plan
        .entities
        .iter()
        .filter(|entity| entity.change == Change::Prune)
        // Belt and braces: the caller is supposed to have scoped `live`
        // already, but a prune is irreversible enough to check twice.
        .filter(|entity| scope.covers(&entity.fully_qualified_name))
        .map(|entity| entity.fully_qualified_name.clone())
        .collect();

    if to_prune.len() > threshold {
        return Err(Refusal::OverThreshold {
            would_prune: to_prune.len(),
            threshold,
        });
    }

    Ok(to_prune)
}
