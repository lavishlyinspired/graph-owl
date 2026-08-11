//! Fusing lexical, semantic and graph-connectivity signals into one ranked
//! score — Epic 105 P9's hybrid-search gap (the platform doc's §11):
//! "Ranking exists (Epic 31's embeddings); combining it with lexical and
//! graph signals into one fused ranking is real, separate work."
//!
//! A **pure function**, matching [`crate::recall`]'s own precedent exactly
//! and for the identical reason: everything the score depends on is an
//! argument, so the whole thing is exhaustively testable and the tests are
//! the specification. Generic rather than entity-linking-specific — any
//! caller with a lexical score and optional semantic/graph scores can fuse
//! them the same way.

/// How much each signal counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridWeights {
    /// How strongly lexical/text similarity counts.
    pub lexical: f64,
    /// How strongly embedding similarity counts.
    pub semantic: f64,
    /// How strongly graph connectivity counts.
    pub graph: f64,
}

impl Default for HybridWeights {
    fn default() -> Self {
        // Equal weight — matching `recall::Weights`'s own reasoning for
        // terms within one tier: there is no evidence to distinguish them,
        // and inventing a gap would be inventing precision. All three
        // signals answer the same question (how relevant is this
        // candidate) from different evidence; none is assumed to dominate
        // until a real estate says otherwise.
        Self {
            lexical: 1.0,
            semantic: 1.0,
            graph: 1.0,
        }
    }
}

/// The score, decomposed. **The decomposition is the explanation** — the
/// identical reasoning [`crate::recall::Score`] is built on: a ranking
/// nobody can audit is a ranking nobody should act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridScore {
    /// The weighted lexical term.
    pub lexical: f64,
    /// The weighted semantic term, `None` until measured.
    pub semantic: Option<f64>,
    /// The weighted graph-connectivity term, `None` until measured (no
    /// traversal engine configured, most concretely).
    pub graph: Option<f64>,
    /// The sum of every term above.
    pub total: f64,
}

/// Fuse one candidate's signals into a ranked score.
///
/// `semantic` and `graph` are `None` until measured — the same honesty
/// [`crate::recall::Candidate::semantic`] already established: a missing
/// addend and a zero addend reach the total identically, but `None` in the
/// report lets a reader tell "not similar"/"not connected" from "never
/// measured."
#[must_use]
pub fn fuse(
    lexical: f64,
    semantic: Option<f64>,
    graph: Option<f64>,
    weights: &HybridWeights,
) -> HybridScore {
    let lexical_term = weights.lexical * lexical;
    let semantic_term = semantic.map(|value| weights.semantic * value);
    let graph_term = graph.map(|value| weights.graph * value);
    HybridScore {
        lexical: lexical_term,
        semantic: semantic_term,
        graph: graph_term,
        total: lexical_term + semantic_term.unwrap_or(0.0) + graph_term.unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights() -> HybridWeights {
        HybridWeights {
            lexical: 1.0,
            semantic: 1.0,
            graph: 1.0,
        }
    }

    #[test]
    fn every_term_contributes_to_the_total() {
        let score = fuse(0.5, Some(0.25), Some(0.1), &weights());
        assert!((score.lexical - 0.5).abs() < 1e-9, "{score:?}");
        assert!(
            (score.semantic.expect("measured") - 0.25).abs() < 1e-9,
            "{score:?}"
        );
        assert!(
            (score.graph.expect("measured") - 0.1).abs() < 1e-9,
            "{score:?}"
        );
        assert!((score.total - 0.85).abs() < 1e-9, "{score:?}");
    }

    /// A missing semantic measurement contributes nothing to the total —
    /// arithmetically the same as `Some(0.0)` — but stays `None` in the
    /// report rather than being reported as a measured zero.
    #[test]
    fn an_unmeasured_semantic_term_is_none_not_zero() {
        let score = fuse(0.5, None, Some(0.1), &weights());
        assert_eq!(score.semantic, None);
        assert!((score.total - 0.6).abs() < 1e-9, "{score:?}");
    }

    /// The identical honesty, for the graph term — no traversal engine
    /// configured must read as "not measured," not as "measured, and
    /// disconnected."
    #[test]
    fn an_unmeasured_graph_term_is_none_not_zero() {
        let score = fuse(0.5, Some(0.2), None, &weights());
        assert_eq!(score.graph, None);
        assert!((score.total - 0.7).abs() < 1e-9, "{score:?}");
    }

    /// A term weighted to zero cannot move the total, regardless of its own
    /// value — the mutation-relevant negative half of "a weight scales its
    /// term."
    #[test]
    fn a_zero_weighted_term_never_moves_the_total() {
        let zeroed_graph = HybridWeights {
            graph: 0.0,
            ..weights()
        };
        let with_graph_signal = fuse(0.5, None, Some(0.9), &zeroed_graph);
        let without_graph_signal = fuse(0.5, None, Some(0.0), &zeroed_graph);
        assert!(
            (with_graph_signal.total - without_graph_signal.total).abs() < 1e-9,
            "{with_graph_signal:?} vs {without_graph_signal:?}"
        );
    }

    /// Doubling a weight doubles exactly that term's own contribution, not
    /// the others' — proves the weights are applied per-term, not as one
    /// global multiplier.
    #[test]
    fn a_weight_scales_only_its_own_term() {
        let doubled_lexical = HybridWeights {
            lexical: 2.0,
            ..weights()
        };
        let base = fuse(0.5, Some(0.3), Some(0.2), &weights());
        let scaled = fuse(0.5, Some(0.3), Some(0.2), &doubled_lexical);

        assert!((scaled.lexical - 2.0 * base.lexical).abs() < 1e-9);
        assert!(
            (scaled.semantic.expect("measured") - base.semantic.expect("measured")).abs() < 1e-9
        );
        assert!((scaled.graph.expect("measured") - base.graph.expect("measured")).abs() < 1e-9);
    }

    #[test]
    fn the_total_is_always_the_sum_of_its_own_reported_terms() {
        let score = fuse(0.7, Some(0.4), Some(0.3), &weights());
        let sum = score.lexical + score.semantic.unwrap_or(0.0) + score.graph.unwrap_or(0.0);
        assert!((score.total - sum).abs() < 1e-9, "{score:?}");
    }
}
