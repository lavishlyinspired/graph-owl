//! Probabilistic scoring — Epic 17 Slice C.
//!
//! A pure function of two entity views. No I/O, so every term is testable in
//! isolation by holding the other three equal — the only way to prove a
//! weight is actually applied rather than coincidentally absent from the
//! total.

use std::collections::HashSet;

use graph_owl_core::resolution::Evidence;

/// What the scorer reads from one side of a candidate pair. Borrowed, not
/// owned — scoring runs against whatever the caller already has (a draft, a
/// stored entity) without a copy on either side.
#[derive(Debug, Clone, Copy)]
pub struct EntityView<'a> {
    pub name: &'a str,
    /// `None` for a root entity. Two roots are **not** treated as sharing a
    /// parent — absence is not a match, the same principle Slice A2 states
    /// for key-based identity, applied here to the probabilistic side too.
    pub parent_fqn: Option<&'a str>,
    pub source_system: Option<&'a str>,
    /// Empty for a non-table kind, or a table observed with no columns yet.
    pub column_names: &'a [String],
}

/// Per-term weights. Configurable per Slice C's acceptance criteria — a
/// deployment that finds structural overlap more reliable than name
/// similarity for its sources can say so without a code change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreWeights {
    pub name_similarity: f64,
    pub structural_overlap: f64,
    pub same_parent: f64,
    pub same_source_system: f64,
}

impl Default for ScoreWeights {
    /// The plan's own weights: `0.5` name, `0.3` structure, `0.1` parent,
    /// `0.1` source — summing to `1.0` so a maximal score is representable.
    fn default() -> Self {
        Self {
            name_similarity: 0.5,
            structural_overlap: 0.3,
            same_parent: 0.1,
            same_source_system: 0.1,
        }
    }
}

/// Jaccard similarity over column-name sets. `0.0` when both sides are
/// empty — a division by zero avoided structurally, not by coincidence of
/// the inputs scoring ever exercises. Two non-empty, disjoint sets already
/// divide safely (a non-zero union), so this guard is the only case that
/// needs one.
///
/// **`&&` vs `||` here is a provably equivalent mutant, not an untested
/// branch.** Jaccard of an empty set against *anything* is `0` regardless —
/// an empty `a` against a non-empty `b` gives `intersection=0` over
/// `union=|b|`, which is `0.0` by the real computation the same as by this
/// guard. The two operators can only disagree on "exactly one side empty",
/// and both paths produce the identical answer there, so no test can
/// distinguish them; `cargo mutants` reports this one as MISSED and it is
/// expected to.
fn column_overlap(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let (intersection, union) = column_overlap_counts(a, b);
    // `union` cannot be zero here: reaching this line means at least one of
    // `a`/`b` was non-empty, and the union of a non-empty set with anything
    // is non-empty.
    #[allow(clippy::cast_precision_loss)]
    let ratio = intersection as f64 / union as f64;
    ratio
}

/// The raw intersection and union sizes behind [`column_overlap`]'s ratio —
/// what [`evidence`] reports as `shared_columns`/`total`, since "0.5 overlap"
/// is not diagnosable but "1 of 2 columns shared" is.
fn column_overlap_counts(a: &[String], b: &[String]) -> (usize, usize) {
    let a: HashSet<&str> = a.iter().map(String::as_str).collect();
    let b: HashSet<&str> = b.iter().map(String::as_str).collect();
    (a.intersection(&b).count(), a.union(&b).count())
}

/// Whether two optional fields agree — and are both present. `None` against
/// `None` is not a match: a table with an unknown source system is not
/// evidence it shares a source with anything else.
fn agrees(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a == b)
}

/// Score a candidate pair. Callers check [`crate::normalize::is_deterministic_match`]
/// first — this function is what runs when that short-circuit does not fire,
/// and it has no notion of the short-circuit itself.
///
/// # Errors
///
/// Never returns an error; the result is always in `[0, 1]` for weights that
/// themselves sum to at most `1.0`.
#[must_use]
pub fn score(a: &EntityView<'_>, b: &EntityView<'_>, weights: &ScoreWeights) -> f64 {
    let name_similarity = strsim::jaro_winkler(&a.name.to_lowercase(), &b.name.to_lowercase());
    let structural_overlap = column_overlap(a.column_names, b.column_names);
    let same_parent = f64::from(agrees(a.parent_fqn, b.parent_fqn));
    let same_source_system = f64::from(agrees(a.source_system, b.source_system));

    weights.name_similarity * name_similarity
        + weights.structural_overlap * structural_overlap
        + weights.same_parent * same_parent
        + weights.same_source_system * same_source_system
}

/// The evidence behind a scored pair, for a `MergeRecord` or a review
/// queue's display — the reason [`score`] itself stays a single `f64` while
/// this exists separately: a merge that only kept the number would not be
/// diagnosable, and a scorer that always built the full breakdown would
/// pay for structure no caller checking "is this above 0.9" ever reads.
///
/// Always includes name similarity (it is always computed); the other three
/// only when they actually hold — an evidence list is what was found, not
/// every term the scorer considered whether or not it fired.
#[must_use]
pub fn evidence(a: &EntityView<'_>, b: &EntityView<'_>) -> Vec<Evidence> {
    let mut found = vec![Evidence::NameSimilarity {
        metric: "jaro_winkler".to_string(),
        value: strsim::jaro_winkler(&a.name.to_lowercase(), &b.name.to_lowercase()),
    }];

    let (shared_columns, total) = column_overlap_counts(a.column_names, b.column_names);
    if shared_columns > 0 {
        found.push(Evidence::StructuralOverlap {
            shared_columns,
            total,
        });
    }
    if agrees(a.parent_fqn, b.parent_fqn) {
        found.push(Evidence::SameParent);
    }
    if agrees(a.source_system, b.source_system) {
        found.push(Evidence::SameSourceSystem);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(
        name: &'a str,
        parent_fqn: Option<&'a str>,
        source_system: Option<&'a str>,
        column_names: &'a [String],
    ) -> EntityView<'a> {
        EntityView {
            name,
            parent_fqn,
            source_system,
            column_names,
        }
    }

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    // ---- one isolating test per term ----

    #[test]
    fn the_name_similarity_term_is_applied() {
        let columns = cols(&["id", "amount"]);
        let same_name = score(
            &view("orders", Some("warehouse.sales"), Some("pg"), &columns),
            &view("orders", Some("warehouse.sales"), Some("pg"), &columns),
            &ScoreWeights::default(),
        );
        let different_name = score(
            &view("orders", Some("warehouse.sales"), Some("pg"), &columns),
            &view("zzqxw", Some("warehouse.sales"), Some("pg"), &columns),
            &ScoreWeights::default(),
        );
        assert!(
            same_name > different_name,
            "same_name={same_name} different_name={different_name}"
        );
    }

    #[test]
    fn the_structural_overlap_term_is_applied() {
        let full_overlap = score(
            &view("orders", Some("p"), Some("pg"), &cols(&["id", "amount"])),
            &view("orders", Some("p"), Some("pg"), &cols(&["id", "amount"])),
            &ScoreWeights::default(),
        );
        let no_overlap = score(
            &view("orders", Some("p"), Some("pg"), &cols(&["id", "amount"])),
            &view("orders", Some("p"), Some("pg"), &cols(&["foo", "bar"])),
            &ScoreWeights::default(),
        );
        assert!(
            full_overlap > no_overlap,
            "full_overlap={full_overlap} no_overlap={no_overlap}"
        );
    }

    #[test]
    fn the_same_parent_term_is_applied() {
        let columns = cols(&["id"]);
        let same_parent = score(
            &view("orders", Some("warehouse.sales"), Some("pg"), &columns),
            &view("orders", Some("warehouse.sales"), Some("pg"), &columns),
            &ScoreWeights::default(),
        );
        let different_parent = score(
            &view("orders", Some("warehouse.sales"), Some("pg"), &columns),
            &view("orders", Some("warehouse.finance"), Some("pg"), &columns),
            &ScoreWeights::default(),
        );
        assert!(
            same_parent > different_parent,
            "same_parent={same_parent} different_parent={different_parent}"
        );
    }

    #[test]
    fn the_same_source_system_term_is_applied() {
        let columns = cols(&["id"]);
        let same_source = score(
            &view("orders", Some("p"), Some("pg-prod"), &columns),
            &view("orders", Some("p"), Some("pg-prod"), &columns),
            &ScoreWeights::default(),
        );
        let different_source = score(
            &view("orders", Some("p"), Some("pg-prod"), &columns),
            &view("orders", Some("p"), Some("mysql-staging"), &columns),
            &ScoreWeights::default(),
        );
        assert!(
            same_source > different_source,
            "same_source={same_source} different_source={different_source}"
        );
    }

    // ---- the plan's own acceptance criterion, verbatim ----

    #[test]
    fn identical_names_in_different_parents_score_below_identical_names_in_the_same_parent() {
        let columns = cols(&["id"]);
        let same_parent = score(
            &view("orders", Some("warehouse.sales"), None, &columns),
            &view("orders", Some("warehouse.sales"), None, &columns),
            &ScoreWeights::default(),
        );
        let different_parent = score(
            &view("orders", Some("warehouse.sales"), None, &columns),
            &view("orders", Some("warehouse.finance"), None, &columns),
            &ScoreWeights::default(),
        );
        assert!(different_parent < same_parent);
    }

    // ---- absence is not a match, for the probabilistic terms too ----

    #[test]
    fn two_entities_with_no_known_parent_are_not_scored_as_sharing_one() {
        let columns = cols(&["id"]);
        let neither_side_has_a_parent = score(
            &view("orders", None, None, &columns),
            &view("orders", None, None, &columns),
            &ScoreWeights::default(),
        );
        let one_side_has_a_parent = score(
            &view("orders", Some("warehouse.sales"), None, &columns),
            &view("orders", Some("warehouse.sales"), None, &columns),
            &ScoreWeights::default(),
        );
        assert!(neither_side_has_a_parent < one_side_has_a_parent);
    }

    // ---- the zero-column edge case ----

    #[test]
    fn no_columns_on_either_side_contributes_zero_rather_than_dividing_by_zero() {
        let empty: Vec<String> = Vec::new();
        assert!(column_overlap(&empty, &empty).abs() < 1e-9);
    }

    #[test]
    fn disjoint_nonempty_column_sets_score_zero_overlap() {
        assert!(column_overlap(&cols(&["a", "b"]), &cols(&["c", "d"])).abs() < 1e-9);
    }

    #[test]
    fn fully_shared_columns_score_full_overlap() {
        assert!((column_overlap(&cols(&["a", "b"]), &cols(&["a", "b"])) - 1.0).abs() < 1e-9);
    }

    // ---- bounds and configurability ----

    #[test]
    fn score_stays_within_zero_and_one() {
        let a_cols = cols(&["a"]);
        let b_cols = cols(&["b"]);
        let cases = [
            (
                view("orders", Some("p"), Some("s"), &a_cols),
                view("orders", Some("p"), Some("s"), &a_cols),
            ),
            (
                view("orders", None, None, &[]),
                view("zzqxw", Some("other"), Some("other2"), &b_cols),
            ),
        ];
        for (a, b) in &cases {
            let s = score(a, b, &ScoreWeights::default());
            assert!((0.0..=1.0).contains(&s), "{s} out of bounds");
        }
    }

    #[test]
    fn weights_are_configurable() {
        let weights = ScoreWeights {
            name_similarity: 0.0,
            structural_overlap: 0.0,
            same_parent: 1.0,
            same_source_system: 0.0,
        };
        let s = score(
            &view("aaa", Some("p"), None, &[]),
            &view("zzz", Some("p"), None, &[]),
            &weights,
        );
        assert!(
            (s - 1.0).abs() < 1e-9,
            "only the same_parent weight should contribute, got {s}"
        );
    }

    // **The terms are summed, not chained by any other operator.** The
    // isolating tests above only assert relative ordering (`a > b`), which a
    // `+` mutated to `*` between two terms can still satisfy by coincidence
    // of the fixture values — found by mutation testing, not anticipated.
    // An exact expected total, with each term at a distinct known value, is
    // the only assertion strong enough to tell a sum from a product.
    #[test]
    fn the_four_terms_are_summed() {
        let same = cols(&["id", "amount"]);
        let disjoint = cols(&["foo", "bar"]);
        // name_similarity = 1.0 (identical), structural_overlap = 0.0
        // (disjoint columns), same_parent = 1.0, same_source_system = 0.0
        // (different sources) — four distinct term values, so a sum and any
        // other combination of them disagree.
        let s = score(
            &view("orders", Some("p"), Some("pg-a"), &same),
            &view("orders", Some("p"), Some("pg-b"), &disjoint),
            &ScoreWeights::default(),
        );
        let expected = 0.5 * 1.0 + 0.3 * 0.0 + 0.1 * 1.0 + 0.1 * 0.0;
        assert!((s - expected).abs() < 1e-9, "s={s} expected={expected}");
    }

    // ---- evidence: what a merge or a review queue actually shows ----

    #[test]
    fn evidence_always_reports_name_similarity() {
        let found = evidence(
            &view("orders", None, None, &[]),
            &view("orders", None, None, &[]),
        );
        assert!(matches!(
            found.first(),
            Some(Evidence::NameSimilarity { metric, value })
                if metric == "jaro_winkler" && (*value - 1.0).abs() < 1e-9
        ));
    }

    #[test]
    fn evidence_reports_structural_overlap_only_when_columns_are_shared() {
        let shared = evidence(
            &view("orders", None, None, &cols(&["id", "amount"])),
            &view("orders", None, None, &cols(&["id", "extra"])),
        );
        assert!(
            shared
                .iter()
                .any(|e| matches!(e, Evidence::StructuralOverlap { shared_columns, total } if *shared_columns == 1 && *total == 3)),
            "expected a 1-of-3 structural overlap, got {shared:?}"
        );

        let disjoint = evidence(
            &view("orders", None, None, &cols(&["id"])),
            &view("orders", None, None, &cols(&["other"])),
        );
        assert!(
            !disjoint
                .iter()
                .any(|e| matches!(e, Evidence::StructuralOverlap { .. })),
            "no shared columns should report no structural-overlap evidence, got {disjoint:?}"
        );
    }

    #[test]
    fn evidence_reports_same_parent_only_when_both_sides_agree() {
        let agree = evidence(
            &view("orders", Some("warehouse.sales"), None, &[]),
            &view("orders", Some("warehouse.sales"), None, &[]),
        );
        assert!(agree.contains(&Evidence::SameParent));

        let disagree = evidence(
            &view("orders", Some("warehouse.sales"), None, &[]),
            &view("orders", Some("warehouse.finance"), None, &[]),
        );
        assert!(!disagree.contains(&Evidence::SameParent));

        let neither_has_one = evidence(
            &view("orders", None, None, &[]),
            &view("orders", None, None, &[]),
        );
        assert!(
            !neither_has_one.contains(&Evidence::SameParent),
            "absence on both sides must not report as agreement"
        );
    }

    #[test]
    fn evidence_reports_same_source_system_only_when_both_sides_agree() {
        let agree = evidence(
            &view("orders", None, Some("pg-prod"), &[]),
            &view("orders", None, Some("pg-prod"), &[]),
        );
        assert!(agree.contains(&Evidence::SameSourceSystem));

        let disagree = evidence(
            &view("orders", None, Some("pg-prod"), &[]),
            &view("orders", None, Some("mysql-staging"), &[]),
        );
        assert!(!disagree.contains(&Evidence::SameSourceSystem));
    }
}
