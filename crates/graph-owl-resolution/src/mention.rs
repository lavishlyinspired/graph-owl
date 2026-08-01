//! Mention resolution — Epic 17 Slice G.
//!
//! Pure scoring: given a mention's text and surrounding context, and a
//! candidate entity's name and the ancestor path it sits under, scores how
//! likely the candidate is what the mention refers to. No I/O — candidate
//! discovery and the "never auto-merges" contract belong to whatever layer
//! calls this (`graph-owl-api`'s `Catalog`), the same separation Slice C's
//! `score` keeps from the storage-backed orchestration around it.

/// Below this, a mention resolves to `None` rather than guessing — the
/// plan's own line, and this module's only opinion about what counts as
/// "confident enough."
pub const MENTION_THRESHOLD: f64 = 0.5;

/// Name similarity and context agreement, weighted evenly. Context is what
/// disambiguates two identically-named entities ("the orders table in
/// staging" vs "in prod"), so it needs real weight, not a tiebreaker's worth.
fn mention_weights() -> (f64, f64) {
    (0.5, 0.5)
}

/// Scores one candidate against a mention.
///
/// `ancestor_names` is the candidate's containing path (schema, database,
/// service — not including the candidate's own name), lowercased comparison
/// against `context` is the caller's job not this function's: both inputs
/// are compared case-insensitively here directly.
#[must_use]
pub fn score_mention(
    mention_text: &str,
    context: &str,
    candidate_name: &str,
    ancestor_names: &[String],
) -> f64 {
    let (name_weight, context_weight) = mention_weights();
    let name_similarity =
        strsim::jaro_winkler(&mention_text.to_lowercase(), &candidate_name.to_lowercase());

    let context_lower = context.to_lowercase();
    let context_agrees = ancestor_names
        .iter()
        .any(|segment| !segment.is_empty() && context_lower.contains(&segment.to_lowercase()));

    name_weight * name_similarity + context_weight * f64::from(context_agrees)
}

/// Whether a scored candidate clears the bar to resolve at all — strictly
/// above [`MENTION_THRESHOLD`], not at or above it, matching the plan's own
/// wording ("no candidate above 0.5").
#[must_use]
pub fn clears_threshold(score: f64) -> bool {
    score > MENTION_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ancestors(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    #[test]
    fn an_exact_name_match_with_no_context_signal_scores_only_the_name_half() {
        let score = score_mention("orders", "", "orders", &ancestors(&["warehouse"]));
        assert!((score - 0.5).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn matching_context_adds_the_other_half() {
        let score = score_mention(
            "orders",
            "the orders table in staging",
            "orders",
            &ancestors(&["staging"]),
        );
        assert!((score - 1.0).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn context_naming_a_different_ancestor_does_not_match() {
        let score = score_mention(
            "orders",
            "the orders table in staging",
            "orders",
            &ancestors(&["prod"]),
        );
        assert!((score - 0.5).abs() < 1e-9, "score={score}");
    }

    // ---- the plan's own disambiguation criterion ----

    #[test]
    fn context_disambiguates_between_two_same_named_candidates() {
        let context = "the orders table in staging";
        let staging = score_mention("orders", context, "orders", &ancestors(&["staging"]));
        let prod = score_mention("orders", context, "orders", &ancestors(&["prod"]));
        assert!(
            staging > prod,
            "staging={staging} prod={prod}: context should have picked staging"
        );
        assert!(clears_threshold(staging));
        assert!(!clears_threshold(prod));
    }

    #[test]
    fn an_unrelated_name_scores_low() {
        let score = score_mention("orders", "", "zzqxw", &ancestors(&["warehouse"]));
        assert!(score < MENTION_THRESHOLD, "score={score}");
    }

    // ---- the threshold boundary ----

    #[test]
    fn exactly_the_threshold_does_not_clear_it() {
        assert!(!clears_threshold(MENTION_THRESHOLD));
    }

    #[test]
    fn just_above_the_threshold_clears_it() {
        assert!(clears_threshold(MENTION_THRESHOLD + 0.000_001));
    }

    #[test]
    fn just_below_the_threshold_does_not_clear_it() {
        assert!(!clears_threshold(MENTION_THRESHOLD - 0.000_001));
    }
}
