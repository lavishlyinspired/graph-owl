//! Confidence bands — Epic 17 Slice D's decision rule.
//!
//! Three bands, not a single threshold (decision 3): auto-merging at a
//! moderate confidence conflates distinct entities, and a catalog that
//! silently merged two real tables into one would be worse than one that
//! left duplicates for a human to resolve. This module is only the mapping
//! from a score to a decision — writing the `MergeRecord`, queueing the
//! review row, or inserting a new entity is Slice B/D/E's storage-backed
//! wiring, not this pure function's job.

/// The two boundaries. `auto_merge_at` and `review_at` are themselves
/// configurable per Slice D's acceptance criteria — an operator with source
/// systems that score more separably can tighten or loosen either band
/// without a code change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfidenceBands {
    pub auto_merge_at: f64,
    pub review_at: f64,
}

impl Default for ConfidenceBands {
    /// The plan's own bands: `≥0.9` auto-merges, `0.6–0.9` queues for
    /// review, below `0.6` is treated as a new entity.
    fn default() -> Self {
        Self {
            auto_merge_at: 0.9,
            review_at: 0.6,
        }
    }
}

/// What a score, alone, decides. Both boundaries are **inclusive** on their
/// upper side — a score of exactly `0.9` auto-merges, and exactly `0.6`
/// queues for review, matching Slice D's acceptance criteria stated at
/// those exact values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// `>= auto_merge_at`. The caller writes a `MergeRecord`.
    AutoMerge,
    /// `>= review_at` and `< auto_merge_at`. The caller queues the pair and
    /// creates **nothing** — neither a merge nor a new entity.
    Review,
    /// `< review_at`. The caller proceeds as if the draft were new.
    New,
}

#[must_use]
pub fn decide(score: f64, bands: &ConfidenceBands) -> Decision {
    if score >= bands.auto_merge_at {
        Decision::AutoMerge
    } else if score >= bands.review_at {
        Decision::Review
    } else {
        Decision::New
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_high_score_auto_merges() {
        assert_eq!(
            decide(0.95, &ConfidenceBands::default()),
            Decision::AutoMerge
        );
    }

    #[test]
    fn a_mid_score_queues_for_review() {
        assert_eq!(decide(0.75, &ConfidenceBands::default()), Decision::Review);
    }

    #[test]
    fn a_low_score_is_treated_as_new() {
        assert_eq!(decide(0.3, &ConfidenceBands::default()), Decision::New);
    }

    // ---- boundaries, inclusive, tested at the exact documented values ----

    #[test]
    fn exactly_zero_point_nine_auto_merges() {
        assert_eq!(
            decide(0.9, &ConfidenceBands::default()),
            Decision::AutoMerge
        );
    }

    // Immediately below the boundary must *not* auto-merge, or the boundary
    // is not where it is documented to be.
    #[test]
    fn just_below_zero_point_nine_does_not_auto_merge() {
        assert_ne!(
            decide(0.899_999, &ConfidenceBands::default()),
            Decision::AutoMerge
        );
    }

    #[test]
    fn exactly_zero_point_six_queues_for_review() {
        assert_eq!(decide(0.6, &ConfidenceBands::default()), Decision::Review);
    }

    #[test]
    fn just_below_zero_point_six_is_new_not_review() {
        assert_eq!(
            decide(0.599_999, &ConfidenceBands::default()),
            Decision::New
        );
    }

    // **The criterion that matters most**: a score inside the review band
    // must resolve to `Review`, never silently to `AutoMerge` or `New` — a
    // mutation that widened either boundary would create a duplicate or a
    // false merge without anyone choosing it.
    #[test]
    fn zero_point_eight_five_is_squarely_in_the_review_band() {
        assert_eq!(decide(0.85, &ConfidenceBands::default()), Decision::Review);
    }

    #[test]
    fn bands_are_configurable() {
        let strict = ConfidenceBands {
            auto_merge_at: 0.99,
            review_at: 0.95,
        };
        assert_eq!(decide(0.96, &strict), Decision::Review);
        assert_eq!(
            decide(0.96, &ConfidenceBands::default()),
            Decision::AutoMerge
        );
    }
}
