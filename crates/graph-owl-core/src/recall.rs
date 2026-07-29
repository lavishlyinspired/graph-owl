//! Ranking recalled memories — Epic 31 Slice C.
//!
//! "The right memory surfaces first, which is the whole capability." A **pure
//! function** of (memory, query, asset state): no I/O, no clock, no embeddings
//! fetched on the fly — everything the score depends on is an argument, so the
//! whole thing is exhaustively testable and the tests are the specification.
//!
//! ## Why the weights are what they are
//!
//! Three tiers, and the tiering is the actual design decision:
//!
//! 1. **Anchoring dominates.** A memory explicitly `About` this asset is
//!    on-topic regardless of how it is worded; every other term is a *guess* at
//!    topicality. Weighted highest so no accumulation of weaker signals can
//!    promote a memory about a different asset above one about this one.
//! 2. **Evidence of topic, and the anti-signal, next** — lexical match,
//!    Epic 8's semantic score, and the staleness penalty. The penalty sits at
//!    the *same* weight as lexical match on purpose: a stale memory that
//!    matches the words perfectly is the precise failure this feature exists to
//!    prevent, so being stale has to be able to cancel a perfect match.
//! 3. **Qualifiers last** — recency, authorship, confidence. These break ties
//!    among memories that are equally on-topic. They must never overturn
//!    topicality, which is what a lower weight means here.
//!
//! Within a tier the numbers are equal because there is no evidence to
//! distinguish them, and inventing a gap would be inventing precision. The
//! isolating tests hold every other term equal for exactly this reason: what is
//! claimed is the *ordering* each term produces, not a calibrated exchange rate
//! between terms.

use crate::memory::{Authorship, LinkRelation, Memory, Staleness};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use uuid::Uuid;

/// How much each term counts. Configurable, because the right trade-off is a
/// property of an estate rather than of this code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub anchor: f64,
    pub lexical: f64,
    pub semantic: f64,
    pub staleness: f64,
    pub recency: f64,
    pub authorship: f64,
    pub confidence: f64,
    /// Days after which a memory's recency term has halved.
    pub recency_half_life_days: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            anchor: 3.0,
            lexical: 2.0,
            semantic: 2.0,
            staleness: 2.0,
            recency: 1.0,
            authorship: 1.0,
            confidence: 1.0,
            // Six months. A memory about a data asset written half a year ago is
            // worth about half one written today, because that is roughly the
            // cadence at which the pipelines, owners and column meanings this
            // catalogs actually turn over. Configurable precisely because an
            // estate that moves faster or slower should say so.
            recency_half_life_days: 180.0,
        }
    }
}

/// One memory offered up for ranking, with the state ranking needs about it.
///
/// The staleness verdict is **passed in** rather than computed here: it depends
/// on the subject's current version, which is I/O, and this function refuses to
/// do I/O.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    pub memory: &'a Memory,
    pub staleness: Staleness,
    /// Epic 8's embedding similarity, `None` until it exists.
    ///
    /// Arithmetically `None` and `Some(0.0)` reach the total identically — a
    /// missing addend and a zero addend are the same addend, and pretending
    /// otherwise would be a comment lying about the code. What `None` buys is
    /// **honesty in the report**: [`Score::semantic`] stays `None`, so a reader
    /// can tell "measured, and not similar" from "never measured". Treating
    /// unmeasured as zero is the conservative direction — it cannot promote a
    /// memory on evidence nobody gathered.
    pub semantic: Option<f64>,
}

/// The score, decomposed. **The decomposition is the explanation** — a ranking
/// nobody can audit is a ranking nobody should act on, and it is also what makes
/// "a config change changes ordering predictably" checkable rather than a hope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    pub anchor: f64,
    pub lexical: f64,
    pub semantic: Option<f64>,
    /// Negative or zero. Kept signed so the total is a plain sum and the sign
    /// says which way the term pushes.
    pub staleness: f64,
    pub recency: f64,
    pub authorship: f64,
    pub confidence: f64,
    pub total: f64,
}

#[derive(Debug, Clone)]
pub struct Ranked<'a> {
    pub memory: &'a Memory,
    pub staleness: Staleness,
    pub score: Score,
}

/// Rank candidates for a query about one asset, best first.
///
/// Stale memories are **ranked down, never dropped**: "we knew this and it may
/// have changed" is information, and hiding it leaves a reader believing nobody
/// ever looked.
#[must_use]
pub fn rank<'a>(
    query: &str,
    subject: Uuid,
    candidates: &[Candidate<'a>],
    now: DateTime<Utc>,
    weights: &Weights,
) -> Vec<Ranked<'a>> {
    let mut ranked: Vec<Ranked<'a>> = candidates
        .iter()
        .map(|candidate| Ranked {
            memory: candidate.memory,
            staleness: candidate.staleness.clone(),
            score: score(query, subject, candidate, now, weights),
        })
        .collect();

    // **Stable**, so candidates that tie keep their input order: the same query
    // over the same data returning different orders reads as the ranking
    // changing its mind. `partial_cmp` cannot fail here — every term is finite
    // by construction, because `lexical_overlap` guards `0 / 0` and
    // `recency_decay` is clamped — and falling back to `Equal` keeps a future
    // NaN from panicking in a read path.
    ranked.sort_by(|a, b| {
        b.score
            .total
            .partial_cmp(&a.score.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

/// One candidate's decomposed score.
///
/// Terms are reported **already weighted**, so the total is a plain sum of what
/// a reader is shown. Reporting raw values beside a weighted total means the
/// numbers on screen do not add up to the number they explain.
fn score(
    query: &str,
    subject: Uuid,
    candidate: &Candidate<'_>,
    now: DateTime<Utc>,
    weights: &Weights,
) -> Score {
    let anchor = weights.anchor * anchor_strength(candidate.memory, subject);
    let lexical = weights.lexical * lexical_overlap(query, candidate.memory);
    let semantic = candidate.semantic.map(|value| weights.semantic * value);
    // Negative: the only term that pushes down. Kept signed rather than
    // subtracted at the end so the sign in the report says which way it pushes.
    let staleness = -weights.staleness * staleness_penalty(&candidate.staleness);
    let recency = weights.recency
        * recency_decay(candidate.memory.as_of, now, weights.recency_half_life_days);
    let authorship = weights.authorship * authorship_credit(&candidate.memory.authorship);
    let confidence = weights.confidence * candidate.memory.confidence;

    Score {
        anchor,
        lexical,
        semantic,
        staleness,
        recency,
        authorship,
        confidence,
        total: anchor
            + lexical
            + semantic.unwrap_or(0.0)
            + staleness
            + recency
            + authorship
            + confidence,
    }
}

/// How much a relation says the memory is *about* the subject, in `[0, 1]`.
///
/// **Ordinal.** What is claimed is the order, not an exchange rate: `About`
/// beats `Affects` beats `Evidence` beats `Follows` beats `Mentions`. The gaps
/// only bite when a term competes with another term, which is exactly why every
/// isolating test holds the other terms equal.
///
/// - `About` is the anchor, so it is the ceiling.
/// - `Affects` is causal — the memory explains why this asset is as it is — so
///   it sits just below being about it.
/// - `Evidence` makes the subject proof *for* the memory rather than its topic.
/// - `Follows` points at another memory, so the subject is rarely an asset
///   somebody is querying.
/// - `Mentions` is named in passing: real, and the weakest thing that counts.
const fn relation_strength(relation: LinkRelation) -> f64 {
    match relation {
        LinkRelation::About => 1.0,
        LinkRelation::Affects => 0.6,
        LinkRelation::Evidence => 0.5,
        LinkRelation::Follows => 0.4,
        LinkRelation::Mentions => 0.2,
    }
}

/// How much a memory's staleness should count against it, in `[0, 1]`.
///
/// `Stale` is the full penalty, and the [`Weights::staleness`] default sets that
/// equal to [`Weights::lexical`] so it can cancel a *perfect* lexical match —
/// the confident, well-worded, wrong answer is the failure this exists to stop.
///
/// **`0.4` for the softer cases is derived from that same principle**, not
/// picked: possibly-stale should be able to cancel a *marginal* match but not a
/// strong one, and "marginal" is fewer than half the query's words — so the
/// multiplier has to sit below `0.5`. `SubjectUnknown` shares it because there
/// is no evidence the memory is wrong, only no way to check; treating it as
/// fully stale would condemn every memory about an asset a connector stopped
/// reporting.
const fn staleness_penalty(staleness: &Staleness) -> f64 {
    match staleness {
        Staleness::Fresh => 0.0,
        Staleness::PossiblyStale { .. } | Staleness::SubjectUnknown => 0.4,
        Staleness::Stale { .. } => 1.0,
    }
}

/// What authorship is worth, in `[0, 1]`.
///
/// An agent's claim is worth **reviewing, not trusting**. Halving is the
/// coarsest honest statement of "counts, but less"; a finer number would be
/// invented precision about how much less, which nothing here measures.
const fn authorship_credit(authorship: &Authorship) -> f64 {
    match authorship {
        Authorship::Human { .. } => 1.0,
        Authorship::Agent { .. } => 0.5,
    }
}

/// Exponential decay on the memory's `as_of`, in `[0, 1]`.
///
/// **Clamped at 1.0 for anything dated now or later.** Host and container clocks
/// disagree, so `as_of` genuinely lands in the future; unclamped, skew would
/// score above full recency and outrank content.
fn recency_decay(as_of: DateTime<Utc>, now: DateTime<Utc>, half_life_days: f64) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "seconds since an era, well inside f64's exact integer range"
    )]
    let age_days = (now - as_of).num_seconds() as f64 / 86_400.0;
    if age_days <= 0.0 {
        return 1.0;
    }
    0.5_f64.powf(age_days / half_life_days)
}

/// How strongly this memory attaches to the subject, in `[0, 1]`.
///
/// **The strongest link wins, not the first found** — a memory both about and
/// mentioning the subject is about it, and reading list order would make the
/// answer depend on insertion order. A memory that never references the subject
/// scores zero rather than borrowing its own anchor, which would make every
/// memory in the corpus look maximally on-topic for every query.
#[must_use]
pub fn anchor_strength(memory: &Memory, subject: Uuid) -> f64 {
    memory
        .links
        .iter()
        .filter(|edge| edge.target == subject)
        .map(|edge| relation_strength(edge.relation))
        .fold(0.0, f64::max)
}

/// The fraction of the query's distinct words the memory's text contains.
///
/// Distinct, because a repeated query word is one requirement and not two —
/// otherwise "revenue revenue refunds" would score full marks against a memory
/// saying only "revenue". The summary is searched alongside the content: it is
/// often where the findable phrasing lives, having been written to be read
/// quickly.
///
/// An empty query scores **zero, not `NaN`**. `0 / 0` would poison the sort and
/// produce an arbitrary order that still looks like a ranking.
#[must_use]
pub fn lexical_overlap(query: &str, memory: &Memory) -> f64 {
    let wanted: HashSet<String> = words(query).collect();
    if wanted.is_empty() {
        return 0.0;
    }

    let mut haystack = memory.content.clone();
    if let Some(summary) = &memory.summary {
        haystack.push(' ');
        haystack.push_str(summary);
    }
    let present: HashSet<String> = words(&haystack).collect();

    #[allow(
        clippy::cast_precision_loss,
        reason = "word counts, far below f64's exact integer range"
    )]
    let fraction = wanted.intersection(&present).count() as f64 / wanted.len() as f64;
    fraction
}

/// Lowercased alphanumeric runs. Deliberately no stemming or stop-word list:
/// both are language-specific guesses, and Epic 8's embeddings are the right
/// place for meaning rather than a hand-rolled approximation of it here.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryKind, MemoryLink};
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-30T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn link(relation: LinkRelation, target: Uuid) -> MemoryLink {
        MemoryLink { relation, target }
    }

    /// A memory built for one axis at a time. Every isolating test varies one
    /// argument and holds the rest identical — the only way to prove a weight is
    /// actually applied rather than accidentally correlated with another.
    fn memory(
        subject: Uuid,
        relation: LinkRelation,
        text: &str,
        authorship: Authorship,
        confidence: f64,
        age_days: i64,
    ) -> Memory {
        let mut links = vec![link(relation, subject)];
        if relation != LinkRelation::About {
            // Every memory needs an anchor to exist at all, so a `Mentions`
            // candidate is anchored to something *else* — which is exactly the
            // real shape: an incident about one table that names another.
            links.push(link(LinkRelation::About, Uuid::new_v4()));
        }
        Memory::new(
            MemoryKind::Rationale,
            text.to_string(),
            authorship,
            Some(confidence),
            links,
            now() - Duration::days(age_days),
        )
        .unwrap()
    }

    fn human() -> Authorship {
        Authorship::Human {
            user_id: "sakshi".into(),
        }
    }

    fn agent() -> Authorship {
        Authorship::Agent {
            agent_id: "explainer".into(),
            model: "claude-opus-5".into(),
        }
    }

    fn fresh(memory: &Memory) -> Candidate<'_> {
        Candidate {
            memory,
            staleness: Staleness::Fresh,
            semantic: None,
        }
    }

    fn order(ranked: &[Ranked<'_>]) -> Vec<Uuid> {
        ranked.iter().map(|r| r.memory.id).collect()
    }

    // ---- the six acceptance criteria, one isolating test each ----

    // Anchoring dominates: a memory *about* this asset is on-topic regardless of
    // wording. Identical text, so lexical match cannot explain the ordering.
    #[test]
    fn about_outranks_mentions_with_identical_text() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded from the revenue metric.";
        let anchored = memory(subject, LinkRelation::About, text, human(), 1.0, 10);
        let mentioned = memory(subject, LinkRelation::Mentions, text, human(), 1.0, 10);

        let ranked = rank(
            "revenue refunds",
            subject,
            &[fresh(&mentioned), fresh(&anchored)],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![anchored.id, mentioned.id]);
    }

    #[test]
    fn a_recent_memory_outranks_an_older_one_with_identical_relevance() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded from the revenue metric.";
        let recent = memory(subject, LinkRelation::About, text, human(), 1.0, 1);
        let old = memory(subject, LinkRelation::About, text, human(), 1.0, 700);

        let ranked = rank(
            "revenue refunds",
            subject,
            &[fresh(&old), fresh(&recent)],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![recent.id, old.id]);
    }

    #[test]
    fn a_human_memory_outranks_an_agent_one_at_equal_relevance_and_recency() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded from the revenue metric.";
        let person = memory(subject, LinkRelation::About, text, human(), 0.9, 10);
        let bot = memory(subject, LinkRelation::About, text, agent(), 0.9, 10);

        let ranked = rank(
            "revenue refunds",
            subject,
            &[fresh(&bot), fresh(&person)],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![person.id, bot.id]);
    }

    // **The criterion this feature exists for.** A stale memory that matches the
    // words perfectly is somebody reading a confident, well-worded, wrong
    // answer — so being stale has to outweigh a better lexical match.
    #[test]
    fn a_stale_memory_falls_below_a_fresh_one_that_matches_less_well() {
        let subject = Uuid::new_v4();
        let matches_perfectly = memory(
            subject,
            LinkRelation::About,
            "revenue refunds excluded",
            human(),
            1.0,
            10,
        );
        let matches_partly = memory(
            subject,
            LinkRelation::About,
            "revenue was restated",
            human(),
            1.0,
            10,
        );

        let ranked = rank(
            "revenue refunds excluded",
            subject,
            &[
                Candidate {
                    memory: &matches_perfectly,
                    staleness: Staleness::Stale {
                        since: crate::envelope::EntityVersion { major: 2, minor: 0 },
                    },
                    semantic: None,
                },
                fresh(&matches_partly),
            ],
            now(),
            &Weights::default(),
        );

        assert_eq!(
            order(&ranked),
            vec![matches_partly.id, matches_perfectly.id]
        );
    }

    // And the negative that makes the test above about *staleness*: with both
    // fresh, the better lexical match wins. Otherwise the assertion could pass
    // on a function that ignores lexical match entirely.
    #[test]
    fn the_better_lexical_match_wins_when_both_are_fresh() {
        let subject = Uuid::new_v4();
        let better = memory(
            subject,
            LinkRelation::About,
            "revenue refunds excluded",
            human(),
            1.0,
            10,
        );
        let worse = memory(
            subject,
            LinkRelation::About,
            "revenue was restated",
            human(),
            1.0,
            10,
        );

        let ranked = rank(
            "revenue refunds excluded",
            subject,
            &[fresh(&worse), fresh(&better)],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![better.id, worse.id]);
    }

    // "Weights are configurable; a config change changes ordering predictably."
    // Zeroing a term must remove exactly its effect — which for a two-candidate
    // pair differing only on that term means the ordering collapses to the input
    // order, because the sort is stable.
    #[test]
    fn zeroing_the_authorship_weight_removes_the_authorship_ordering() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded.";
        let person = memory(subject, LinkRelation::About, text, human(), 0.9, 10);
        let bot = memory(subject, LinkRelation::About, text, agent(), 0.9, 10);
        let weights = Weights {
            authorship: 0.0,
            ..Weights::default()
        };

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&bot), fresh(&person)],
            now(),
            &weights,
        );

        assert_eq!(order(&ranked), vec![bot.id, person.id]);
    }

    #[test]
    fn zeroing_the_anchor_weight_removes_the_anchor_ordering() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded.";
        let anchored = memory(subject, LinkRelation::About, text, human(), 1.0, 10);
        let mentioned = memory(subject, LinkRelation::Mentions, text, human(), 1.0, 10);
        let weights = Weights {
            anchor: 0.0,
            ..Weights::default()
        };

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&mentioned), fresh(&anchored)],
            now(),
            &weights,
        );

        assert_eq!(order(&ranked), vec![mentioned.id, anchored.id]);
    }

    #[test]
    fn zeroing_the_staleness_weight_stops_penalising_stale_memories() {
        let subject = Uuid::new_v4();
        let stale_text = memory(
            subject,
            LinkRelation::About,
            "revenue refunds excluded",
            human(),
            1.0,
            10,
        );
        let fresh_text = memory(
            subject,
            LinkRelation::About,
            "revenue was restated",
            human(),
            1.0,
            10,
        );
        let weights = Weights {
            staleness: 0.0,
            ..Weights::default()
        };

        let ranked = rank(
            "revenue refunds excluded",
            subject,
            &[
                Candidate {
                    memory: &stale_text,
                    staleness: Staleness::Stale {
                        since: crate::envelope::EntityVersion { major: 2, minor: 0 },
                    },
                    semantic: None,
                },
                fresh(&fresh_text),
            ],
            now(),
            &weights,
        );

        assert_eq!(order(&ranked), vec![stale_text.id, fresh_text.id]);
    }

    // The remaining three weights, each zeroed. Mutation found these missing:
    // `weight * term` and `weight + term` are indistinguishable at any non-zero
    // weight, because adding a constant to every candidate reorders nothing. It
    // is only at zero that multiplication erases the term and addition passes it
    // straight through — so the zeroing test is the *only* test that pins down
    // how a weight is applied, not merely that it is.
    #[test]
    fn zeroing_the_lexical_weight_removes_the_lexical_ordering() {
        let subject = Uuid::new_v4();
        let better = memory(
            subject,
            LinkRelation::About,
            "revenue refunds excluded",
            human(),
            1.0,
            10,
        );
        let worse = memory(
            subject,
            LinkRelation::About,
            "revenue was restated",
            human(),
            1.0,
            10,
        );
        let weights = Weights {
            lexical: 0.0,
            ..Weights::default()
        };

        let ranked = rank(
            "revenue refunds excluded",
            subject,
            &[fresh(&worse), fresh(&better)],
            now(),
            &weights,
        );

        assert_eq!(order(&ranked), vec![worse.id, better.id]);
    }

    #[test]
    fn zeroing_the_semantic_weight_removes_the_semantic_ordering() {
        let subject = Uuid::new_v4();
        let close = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);
        let far = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);
        let weights = Weights {
            semantic: 0.0,
            ..Weights::default()
        };

        let ranked = rank(
            "refunds",
            subject,
            &[
                Candidate {
                    memory: &far,
                    staleness: Staleness::Fresh,
                    semantic: Some(0.1),
                },
                Candidate {
                    memory: &close,
                    staleness: Staleness::Fresh,
                    semantic: Some(0.9),
                },
            ],
            now(),
            &weights,
        );

        assert_eq!(order(&ranked), vec![far.id, close.id]);
        // And the weighted term really is zero, not merely non-deciding.
        assert_eq!(ranked[0].score.semantic, Some(0.0));
    }

    #[test]
    fn zeroing_the_confidence_weight_removes_the_confidence_ordering() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded.";
        let sure = memory(subject, LinkRelation::About, text, agent(), 0.95, 10);
        let unsure = memory(subject, LinkRelation::About, text, agent(), 0.2, 10);
        let weights = Weights {
            confidence: 0.0,
            ..Weights::default()
        };

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&unsure), fresh(&sure)],
            now(),
            &weights,
        );

        assert_eq!(order(&ranked), vec![unsure.id, sure.id]);
    }

    #[test]
    fn a_more_confident_memory_outranks_a_less_confident_one() {
        let subject = Uuid::new_v4();
        let text = "Refunds were excluded.";
        let sure = memory(subject, LinkRelation::About, text, agent(), 0.95, 10);
        let unsure = memory(subject, LinkRelation::About, text, agent(), 0.2, 10);

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&unsure), fresh(&sure)],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![sure.id, unsure.id]);
    }

    // ---- staleness is ranked down, never dropped ----

    #[test]
    fn a_stale_memory_is_still_returned() {
        let subject = Uuid::new_v4();
        let stale_one = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);

        let ranked = rank(
            "refunds",
            subject,
            &[Candidate {
                memory: &stale_one,
                staleness: Staleness::Stale {
                    since: crate::envelope::EntityVersion { major: 9, minor: 0 },
                },
                semantic: None,
            }],
            now(),
            &Weights::default(),
        );

        assert_eq!(ranked.len(), 1);
        assert!(matches!(ranked[0].staleness, Staleness::Stale { .. }));
    }

    // Possibly-stale is a hint, stale is a warning. Collapsing them means an
    // added column reads as damning as a restructured table.
    #[test]
    fn possibly_stale_is_penalised_less_than_stale() {
        let subject = Uuid::new_v4();
        let one = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);
        let two = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);
        let since = crate::envelope::EntityVersion { major: 2, minor: 0 };

        let ranked = rank(
            "refunds",
            subject,
            &[
                Candidate {
                    memory: &one,
                    staleness: Staleness::Stale { since },
                    semantic: None,
                },
                Candidate {
                    memory: &two,
                    staleness: Staleness::PossiblyStale { since },
                    semantic: None,
                },
            ],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![two.id, one.id]);
        assert!(ranked[0].score.staleness > ranked[1].score.staleness);
    }

    #[test]
    fn a_fresh_memory_carries_no_staleness_penalty() {
        let subject = Uuid::new_v4();
        let one = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&one)],
            now(),
            &Weights::default(),
        );

        assert!((ranked[0].score.staleness - 0.0).abs() < f64::EPSILON);
    }

    // A subject nobody can resolve is a hint, not a warning: there is no
    // evidence the memory is wrong, only no way to check. Treating it as fully
    // stale would condemn every memory about an asset a connector stopped
    // reporting.
    #[test]
    fn an_unknown_subject_is_penalised_like_possibly_stale_not_like_stale() {
        let subject = Uuid::new_v4();
        let a = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);
        let b = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);
        let since = crate::envelope::EntityVersion { major: 2, minor: 0 };

        let unknown = rank(
            "refunds",
            subject,
            &[Candidate {
                memory: &a,
                staleness: Staleness::SubjectUnknown,
                semantic: None,
            }],
            now(),
            &Weights::default(),
        );
        let possibly = rank(
            "refunds",
            subject,
            &[Candidate {
                memory: &b,
                staleness: Staleness::PossiblyStale { since },
                semantic: None,
            }],
            now(),
            &Weights::default(),
        );

        assert!((unknown[0].score.staleness - possibly[0].score.staleness).abs() < f64::EPSILON);
        assert!(unknown[0].score.staleness < 0.0);
    }

    // ---- anchor strength ----

    #[test]
    fn anchor_strength_is_highest_for_about() {
        let subject = Uuid::new_v4();
        let about = memory(subject, LinkRelation::About, "x", human(), 1.0, 0);

        assert!((anchor_strength(&about, subject) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn anchor_strength_orders_the_relations() {
        let subject = Uuid::new_v4();
        let of =
            |relation| anchor_strength(&memory(subject, relation, "x", human(), 1.0, 0), subject);

        assert!(of(LinkRelation::About) > of(LinkRelation::Affects));
        assert!(of(LinkRelation::Affects) > of(LinkRelation::Evidence));
        assert!(of(LinkRelation::Evidence) > of(LinkRelation::Follows));
        assert!(of(LinkRelation::Follows) > of(LinkRelation::Mentions));
        assert!(of(LinkRelation::Mentions) > 0.0);
    }

    // A memory that never references the subject anchors at zero rather than
    // borrowing its own `About` link — which would make every memory in the
    // corpus look maximally on-topic for every query.
    #[test]
    fn a_memory_unrelated_to_the_subject_anchors_at_zero() {
        let elsewhere = memory(Uuid::new_v4(), LinkRelation::About, "x", human(), 1.0, 0);

        assert!((anchor_strength(&elsewhere, Uuid::new_v4()) - 0.0).abs() < f64::EPSILON);
    }

    // **The strongest link wins, not the first found.** A memory both about and
    // mentioning the subject is about it; taking the first link in list order
    // would make the answer depend on insertion order.
    #[test]
    fn the_strongest_link_to_the_subject_decides() {
        let subject = Uuid::new_v4();
        let mut both = memory(subject, LinkRelation::Mentions, "x", human(), 1.0, 0);
        both.links.push(link(LinkRelation::About, subject));

        assert!((anchor_strength(&both, subject) - 1.0).abs() < f64::EPSILON);
    }

    // ---- lexical overlap ----

    #[test]
    fn lexical_overlap_is_the_fraction_of_query_words_present() {
        let subject = Uuid::new_v4();
        let one = memory(
            subject,
            LinkRelation::About,
            "revenue excludes refunds",
            human(),
            1.0,
            0,
        );

        assert!((lexical_overlap("revenue refunds", &one) - 1.0).abs() < f64::EPSILON);
        assert!((lexical_overlap("revenue margin", &one) - 0.5).abs() < f64::EPSILON);
        assert!((lexical_overlap("margin churn", &one) - 0.0).abs() < f64::EPSILON);
    }

    // An empty query is `0 / 0`. Returning NaN poisons the sort and produces an
    // arbitrary order that looks like a ranking.
    #[test]
    fn an_empty_query_scores_zero_rather_than_nan() {
        let subject = Uuid::new_v4();
        let one = memory(subject, LinkRelation::About, "revenue", human(), 1.0, 0);

        for empty in ["", "   ", "!!!"] {
            let score = lexical_overlap(empty, &one);
            assert!(!score.is_nan(), "{empty:?} produced NaN");
            assert!((score - 0.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn lexical_overlap_ignores_case_and_punctuation() {
        let subject = Uuid::new_v4();
        let one = memory(
            subject,
            LinkRelation::About,
            "Revenue, excluding REFUNDS.",
            human(),
            1.0,
            0,
        );

        assert!((lexical_overlap("revenue refunds", &one) - 1.0).abs() < f64::EPSILON);
    }

    // The summary is searched too: it is often where the searchable phrasing
    // lives, precisely because it was written to be read quickly.
    #[test]
    fn lexical_overlap_searches_the_summary_as_well_as_the_content() {
        let subject = Uuid::new_v4();
        let mut one = memory(subject, LinkRelation::About, "see below", human(), 1.0, 0);
        one.summary = Some("Refund handling changed".into());

        assert!(lexical_overlap("refund", &one) > 0.0);
    }

    // A repeated query word is one requirement, not two — otherwise "revenue
    // revenue refunds" would score 1.0 on a memory saying only "revenue".
    #[test]
    fn a_repeated_query_word_counts_once() {
        let subject = Uuid::new_v4();
        let one = memory(subject, LinkRelation::About, "revenue", human(), 1.0, 0);

        assert!((lexical_overlap("revenue revenue refunds", &one) - 0.5).abs() < f64::EPSILON);
    }

    // ---- recency ----

    // Pins the half-life to what it claims: a memory exactly one half-life old
    // scores half. A decay that merely decreases would pass a weaker assertion.
    #[test]
    fn a_memory_one_half_life_old_scores_half_on_recency() {
        let subject = Uuid::new_v4();
        let weights = Weights::default();
        let aged = memory(
            subject,
            LinkRelation::About,
            "refunds",
            human(),
            1.0,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a whole number of days by construction"
            )]
            {
                weights.recency_half_life_days as i64
            },
        );

        let ranked = rank("refunds", subject, &[fresh(&aged)], now(), &weights);

        assert!((ranked[0].score.recency - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_memory_written_now_scores_full_recency() {
        let subject = Uuid::new_v4();
        let brand_new = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 0);

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&brand_new)],
            now(),
            &Weights::default(),
        );

        assert!((ranked[0].score.recency - 1.0).abs() < f64::EPSILON);
    }

    // Host and container clocks disagree, so `as_of` can land in the future. An
    // unclamped decay would score it above 1.0 and let clock skew outrank
    // content.
    #[test]
    fn a_memory_dated_in_the_future_does_not_score_above_full_recency() {
        let subject = Uuid::new_v4();
        let skewed = memory(subject, LinkRelation::About, "refunds", human(), 1.0, -400);

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&skewed)],
            now(),
            &Weights::default(),
        );

        assert!((ranked[0].score.recency - 1.0).abs() < f64::EPSILON);
    }

    // ---- the semantic slot Epic 8 fills ----

    #[test]
    fn a_semantically_similar_memory_outranks_a_dissimilar_one() {
        let subject = Uuid::new_v4();
        let text = "refunds";
        let close = memory(subject, LinkRelation::About, text, human(), 1.0, 10);
        let far = memory(subject, LinkRelation::About, text, human(), 1.0, 10);

        let ranked = rank(
            "refunds",
            subject,
            &[
                Candidate {
                    memory: &far,
                    staleness: Staleness::Fresh,
                    semantic: Some(0.1),
                },
                Candidate {
                    memory: &close,
                    staleness: Staleness::Fresh,
                    semantic: Some(0.9),
                },
            ],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![close.id, far.id]);
    }

    // Until Epic 8 exists every candidate has `None`, so the term has to cancel
    // rather than distort. `None` is reported as absent, not as a zero score, so
    // a reader can tell "no similarity" from "not measured".
    #[test]
    fn an_absent_semantic_score_is_reported_as_absent() {
        let subject = Uuid::new_v4();
        let one = memory(subject, LinkRelation::About, "refunds", human(), 1.0, 10);

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&one)],
            now(),
            &Weights::default(),
        );

        assert_eq!(ranked[0].score.semantic, None);
    }

    // ---- determinism and edges ----

    // Equal scores must keep input order. An unstable sort makes the same query
    // return different orders on the same data, which reads as the ranking
    // changing its mind.
    #[test]
    fn candidates_that_tie_keep_their_input_order() {
        let subject = Uuid::new_v4();
        let text = "refunds";
        let first = memory(subject, LinkRelation::About, text, human(), 1.0, 10);
        let second = memory(subject, LinkRelation::About, text, human(), 1.0, 10);

        let ranked = rank(
            "refunds",
            subject,
            &[fresh(&first), fresh(&second)],
            now(),
            &Weights::default(),
        );

        assert_eq!(order(&ranked), vec![first.id, second.id]);
    }

    #[test]
    fn nothing_to_rank_ranks_to_nothing() {
        assert!(rank("refunds", Uuid::new_v4(), &[], now(), &Weights::default()).is_empty());
    }

    // The total is the sum of the terms, so a reader who disagrees with the
    // order can see exactly which term produced it.
    #[test]
    fn the_total_is_the_sum_of_the_reported_terms() {
        let subject = Uuid::new_v4();
        let one = memory(subject, LinkRelation::About, "revenue", agent(), 0.7, 30);
        let weights = Weights::default();

        let ranked = rank(
            "revenue",
            subject,
            &[Candidate {
                memory: &one,
                staleness: Staleness::PossiblyStale {
                    since: crate::envelope::EntityVersion { major: 1, minor: 4 },
                },
                semantic: Some(0.5),
            }],
            now(),
            &weights,
        );
        let s = ranked[0].score;
        let sum = s.anchor
            + s.lexical
            + s.semantic.unwrap_or(0.0)
            + s.staleness
            + s.recency
            + s.authorship
            + s.confidence;

        assert!((s.total - sum).abs() < 1e-9, "{s:?}");
    }
}
