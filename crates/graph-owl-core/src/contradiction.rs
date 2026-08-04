//! Surfacing conflicting institutional knowledge — Epic 31 Slice E.
//!
//! "Conflicting institutional knowledge is visible rather than silently
//! inconsistent." Two things this deliberately does **not** do, both of which
//! decision 6 of the plan forbids:
//!
//! - **Nothing is auto-resolved.** There is no winner field and no ordering
//!   preference. Software adjudicating institutional disagreement is worse than
//!   the disagreement, because it ends the argument without settling it.
//! - **Neither memory is hidden.** A flagged pair is *added* to a review queue;
//!   nothing is removed from anything.
//!
//! ## Why there is no time window
//!
//! The plan's criterion reads "two `Decision` memories about the same asset with
//! overlapping `as_of`". `as_of` is an instant, so "overlapping" needs an
//! interpretation, and the obvious one — "within N days" — would be a magic
//! number with no derivation available, which `00i` rule 4 forbids outright.
//!
//! Supersession already expresses it exactly and needs no number: a decision
//! that has been superseded is **history**, not a competing claim, and a
//! decision that supersedes another is a **correction**, which is the opposite
//! of a contradiction — telling the two apart is most of Slice B's point. So two
//! decisions "overlap" precisely when both are current and neither corrects the
//! other.

use crate::memory::{LinkRelation, Memory, MemoryKind};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

/// Why a pair was flagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContradictionKind {
    /// A reviewer looked at a candidate and agreed. **The strongest statement
    /// available**, and kept distinct from `Declared` because the provenance
    /// differs in a way somebody will want to query: a confirmed candidate is
    /// evidence the heuristic works, a declared one says nothing about it.
    Confirmed,
    /// A person said so, with a `Contradicts` link. Never filtered by kind or
    /// subject: a human assertion of disagreement outranks any rule here.
    Declared,
    /// Two current `Decision` memories anchored to the same asset. A
    /// **candidate** — the word matters, because this is a heuristic and a human
    /// confirms or dismisses it.
    Candidate,
}

/// A flagged pair. **Unordered**: `a` is simply the smaller id, so the same two
/// memories always produce the same pair however they arrived, which is what
/// makes dismissal and deduplication work at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contradiction {
    /// The smaller of the two memory ids.
    pub a: Uuid,
    /// The larger of the two memory ids.
    pub b: Uuid,
    /// The asset both are anchored to, for a `Candidate`. `None` for a
    /// `Declared` one, which needs no shared subject to be real.
    pub subject: Option<Uuid>,
    /// Why the pair was flagged.
    pub kind: ContradictionKind,
}

/// What a reviewer decided about a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Yes, these disagree. **Stays in the queue**, flagged as confirmed —
    /// confirming a contradiction is not resolving it, and this epic never
    /// resolves one.
    Confirmed,
    /// No, they do not. Recorded so the pair is not re-flagged.
    Dismissed,
}

/// A pair a human has looked at, and what they decided.
///
/// **One record with a verdict, not a dismissals table beside a confirmations
/// table.** Two tables would make "confirmed *and* dismissed" representable, and
/// changing one's mind would be a delete from one plus an insert into the other
/// with nothing making that atomic. One row per pair makes the contradictory
/// state unrepresentable and a change of mind an `UPDATE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Review {
    /// One memory in the pair.
    pub a: Uuid,
    /// The other memory in the pair.
    pub b: Uuid,
    /// What the reviewer decided.
    pub verdict: Verdict,
}

/// Every open contradiction among these memories.
///
/// Reviews do two different things and it is worth being explicit about which:
/// `Dismissed` **removes** a pair from the answer, and `Confirmed` **upgrades**
/// one that is already there. A confirmation cannot resurrect a pair detection no
/// longer finds — if one side has since been superseded, the disagreement was
/// settled by correction, and re-raising it would reopen a question the record
/// already answers.
#[must_use]
pub fn contradictions(memories: &[&Memory], reviews: &[Review]) -> Vec<Contradiction> {
    let present: HashSet<Uuid> = memories.iter().map(|memory| memory.id).collect();
    let suppressed: HashSet<(Uuid, Uuid)> = reviews
        .iter()
        .filter(|review| review.verdict == Verdict::Dismissed)
        .map(|review| unordered(review.a, review.b))
        .collect();
    let affirmed: HashSet<(Uuid, Uuid)> = reviews
        .iter()
        .filter(|review| review.verdict == Verdict::Confirmed)
        .map(|review| unordered(review.a, review.b))
        .collect();

    // Keyed on the unordered pair, so mutual declarations and a declaration that
    // is also a candidate collapse into one disagreement rather than two or
    // three. `BTreeMap` because a review queue that reorders itself between
    // loads is a review queue nobody can work through.
    let mut found: BTreeMap<(Uuid, Uuid), Contradiction> = BTreeMap::new();

    // **Declared first**, then `or_insert` below, so a pair a person declared is
    // never downgraded to "candidate" by the heuristic finding it too. A human
    // assertion of disagreement is the truer description of the same pair.
    for memory in memories {
        for edge in &memory.links {
            if edge.relation != LinkRelation::Contradicts || edge.target == memory.id {
                continue;
            }
            // The other side may be an asset id, a typo, or a memory the caller
            // did not load. Reporting a pair with one half missing gives a
            // reviewer nothing to review.
            if !present.contains(&edge.target) {
                continue;
            }
            let key = unordered(memory.id, edge.target);
            found.insert(
                key,
                Contradiction {
                    a: key.0,
                    b: key.1,
                    subject: None,
                    kind: ContradictionKind::Declared,
                },
            );
        }
    }

    // **Every** pair, not just consecutive ones: three competing decisions is
    // three disagreements, and a reviewer shown two of them closes the review
    // believing it settled.
    //
    // Mutation reports `skip(index + 1)` → `skip(index)` as a survivor, and it
    // is a genuinely **equivalent** mutant rather than a missing test: the only
    // extra pairs it produces are self-pairs, which `candidate` rejects anyway
    // (it has to, because the same memory can appear twice in the input). The
    // `+ 1` is an optimisation and a statement of intent, not a correctness
    // requirement, so no test can distinguish the two.
    for (index, one) in memories.iter().enumerate() {
        for two in memories.iter().skip(index + 1) {
            if let Some(candidate) = candidate(one, two) {
                found.entry(unordered(one.id, two.id)).or_insert(candidate);
            }
        }
    }

    found
        .into_values()
        // Suppression is applied **last**, so a pair that somehow carried both
        // verdicts is still deterministic. The storage primary key makes that
        // state unreachable; determinism here costs nothing and means a future
        // second writer cannot make the queue depend on row order.
        .filter(|conflict| !suppressed.contains(&(conflict.a, conflict.b)))
        .map(|mut conflict| {
            if affirmed.contains(&(conflict.a, conflict.b)) {
                conflict.kind = ContradictionKind::Confirmed;
            }
            conflict
        })
        .collect()
}

/// Whether these two are a candidate contradiction, and about what.
fn candidate(one: &Memory, two: &Memory) -> Option<Contradiction> {
    if one.id == two.id {
        return None;
    }
    // Only `Decision`. Two rationales about one asset are two explanations,
    // which is normal and useful — flagging them buries the real conflicts under
    // every asset anybody documented twice.
    if one.kind != MemoryKind::Decision || two.kind != MemoryKind::Decision {
        return None;
    }
    // A superseded decision is **history**, not a competing claim, and a
    // decision that supersedes another is a **correction** — the opposite of a
    // contradiction. This pair of checks is what stands in for the plan's
    // "overlapping `as_of`", and it needs no window to justify.
    if one.is_superseded() || two.is_superseded() {
        return None;
    }
    if one.supersedes == Some(two.id) || two.supersedes == Some(one.id) {
        return None;
    }

    let subject = shared_anchor(one, two)?;
    let key = unordered(one.id, two.id);
    Some(Contradiction {
        a: key.0,
        b: key.1,
        subject: Some(subject),
        kind: ContradictionKind::Candidate,
    })
}

/// An asset both are anchored to.
///
/// Anchors only — `Mentions` does not count, which is precisely why it exists as
/// a separate relation: two decisions that merely name the same table are not
/// competing about it.
fn shared_anchor(one: &Memory, two: &Memory) -> Option<Uuid> {
    let theirs: HashSet<Uuid> = two.anchors().into_iter().collect();
    one.anchors()
        .into_iter()
        .find(|target| theirs.contains(target))
}

/// The pair as an unordered key.
///
/// **The whole reason dismissal works.** A reviewer's dismissal recorded as
/// `(b, a)` has to suppress `(a, b)`, or the queue reopens on its own the next
/// time the pair is loaded in the other order — which looks like a product bug
/// and is impossible to reproduce on demand.
///
/// `<` vs `<=` is an **equivalent** mutant, not a missing test: they differ only
/// when `x == y`, and both branches then return `(x, x)`.
fn unordered(x: Uuid, y: Uuid) -> (Uuid, Uuid) {
    if x < y { (x, y) } else { (y, x) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Authorship, MemoryLink};
    use chrono::Utc;

    fn human() -> Authorship {
        Authorship::Human {
            user_id: "sakshi".into(),
        }
    }

    fn decision(subject: Uuid) -> Memory {
        of_kind(MemoryKind::Decision, subject)
    }

    fn of_kind(kind: MemoryKind, subject: Uuid) -> Memory {
        Memory::new(
            kind,
            "Refunds are excluded from revenue.".into(),
            human(),
            None,
            vec![MemoryLink {
                relation: LinkRelation::About,
                target: subject,
            }],
            Utc::now(),
        )
        .unwrap()
    }

    fn dismissed(a: Uuid, b: Uuid) -> Review {
        Review {
            a,
            b,
            verdict: Verdict::Dismissed,
        }
    }

    fn confirmed(a: Uuid, b: Uuid) -> Review {
        Review {
            a,
            b,
            verdict: Verdict::Confirmed,
        }
    }

    fn pairs(found: &[Contradiction]) -> Vec<(Uuid, Uuid)> {
        found.iter().map(|c| (c.a, c.b)).collect()
    }

    fn normalised(x: Uuid, y: Uuid) -> (Uuid, Uuid) {
        if x < y { (x, y) } else { (y, x) }
    }

    // ---- declared ----

    // A person said so, so it is real regardless of kind or subject. Requiring a
    // shared asset would silently drop the disagreements somebody cared enough
    // to write down.
    #[test]
    fn an_explicit_contradicts_link_surfaces_the_pair() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        let two = of_kind(MemoryKind::Rationale, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });

        let found = contradictions(&[&one, &two], &[]);

        assert_eq!(pairs(&found), vec![normalised(one.id, two.id)]);
        assert_eq!(found[0].kind, ContradictionKind::Declared);
    }

    // **Neither memory is hidden.** A flagged pair is added to a queue; nothing
    // is removed. Both ids appear, and both are still the caller's memories.
    #[test]
    fn both_memories_remain_visible_after_being_flagged() {
        let mut one = of_kind(MemoryKind::Decision, Uuid::new_v4());
        let two = of_kind(MemoryKind::Decision, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });
        let corpus = [&one, &two];

        let found = contradictions(&corpus, &[]);

        assert_eq!(corpus.len(), 2);
        assert!(found.iter().any(|c| c.a == one.id || c.b == one.id));
        assert!(found.iter().any(|c| c.a == two.id || c.b == two.id));
    }

    // **Never auto-resolved.** The same pair arriving in either order produces
    // the identical record — no first-listed winner, no implied precedence.
    #[test]
    fn the_pair_is_the_same_however_it_arrives() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        let two = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });

        let forwards = contradictions(&[&one, &two], &[]);
        let backwards = contradictions(&[&two, &one], &[]);

        assert_eq!(forwards, backwards);
    }

    // A link to something outside the corpus is not a pair — the other side may
    // be an asset id, a typo, or a memory the caller did not load.
    #[test]
    fn a_contradicts_link_to_an_absent_memory_is_not_flagged() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: Uuid::new_v4(),
        });

        assert!(contradictions(&[&one], &[]).is_empty());
    }

    // Both sides declaring it is one disagreement, not two.
    #[test]
    fn a_mutually_declared_contradiction_is_reported_once() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        let mut two = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });
        two.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: one.id,
        });

        assert_eq!(contradictions(&[&one, &two], &[]).len(), 1);
    }

    // **`Follows` legitimately points at another memory** — it is how a chain is
    // built — so a corpus routinely contains non-`Contradicts` links whose
    // target is present. Only `Contradicts` may flag a pair; a check that let any
    // link through would report every memory chain as a disagreement. Found by
    // mutation: no earlier test had a link of any other kind pointing *inside*
    // the corpus, because anchors point at assets.
    #[test]
    fn a_link_of_another_kind_to_a_present_memory_is_not_a_contradiction() {
        let two = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        for relation in [
            LinkRelation::Follows,
            LinkRelation::Evidence,
            LinkRelation::Mentions,
            LinkRelation::Affects,
        ] {
            let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
            one.links.push(MemoryLink {
                relation,
                target: two.id,
            });

            assert!(
                contradictions(&[&one, &two], &[]).is_empty(),
                "{relation:?} should not flag a contradiction"
            );
        }
    }

    // A memory declaring it disagrees with itself is a data error, not a
    // disagreement — and reporting it puts an unresolvable item in a queue a
    // human cannot clear.
    #[test]
    fn a_memory_declaring_it_contradicts_itself_is_not_flagged() {
        let mut only = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        only.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: only.id,
        });

        assert!(contradictions(&[&only], &[]).is_empty());
    }

    // ---- candidates ----

    #[test]
    fn two_current_decisions_about_one_asset_are_candidates() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);

        let found = contradictions(&[&one, &two], &[]);

        assert_eq!(pairs(&found), vec![normalised(one.id, two.id)]);
        assert_eq!(found[0].kind, ContradictionKind::Candidate);
        assert_eq!(found[0].subject, Some(subject));
    }

    // Only `Decision`. Two rationales about one asset are two explanations, which
    // is normal and useful; flagging them would bury the real conflicts under
    // every asset that anyone documented twice.
    #[test]
    fn two_memories_that_are_not_decisions_are_not_candidates() {
        let subject = Uuid::new_v4();

        for kind in [
            MemoryKind::Rationale,
            MemoryKind::Incident,
            MemoryKind::Caveat,
        ] {
            let one = of_kind(kind, subject);
            let two = of_kind(kind, subject);

            assert!(
                contradictions(&[&one, &two], &[]).is_empty(),
                "{kind:?} should not be a candidate"
            );
        }
    }

    // One of each is not two decisions. A check that only counted memories per
    // asset would flag this.
    #[test]
    fn a_decision_beside_a_rationale_is_not_a_candidate() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = of_kind(MemoryKind::Rationale, subject);

        assert!(contradictions(&[&one, &two], &[]).is_empty());
    }

    #[test]
    fn decisions_about_different_assets_are_not_candidates() {
        let one = decision(Uuid::new_v4());
        let two = decision(Uuid::new_v4());

        assert!(contradictions(&[&one, &two], &[]).is_empty());
    }

    // **A correction is not a contradiction** — telling the two apart is most of
    // Slice B's point. This is also the reason there is no time window: the
    // supersession relation says it exactly, with no number to justify.
    #[test]
    fn a_decision_that_supersedes_the_other_is_a_correction_not_a_conflict() {
        let subject = Uuid::new_v4();
        let mut original = decision(subject);
        let mut correction = decision(subject);
        correction.supersedes = Some(original.id);
        original.superseded_by = Some(correction.id);

        assert!(contradictions(&[&original, &correction], &[]).is_empty());
    }

    // **The successor half alone is enough.** Supersession is two rows, so a
    // partially-written pair is a real state — and the previous test sets *both*
    // halves, which means `is_superseded()` short-circuits and the `supersedes`
    // check is never actually exercised. Mutation found exactly that: the check
    // could have been broken with every test still green. Both directions,
    // because the argument order depends on slice order.
    #[test]
    fn a_dangling_supersedes_is_still_a_correction() {
        let subject = Uuid::new_v4();
        let original = decision(subject);
        let mut correction = decision(subject);
        correction.supersedes = Some(original.id);

        assert!(contradictions(&[&original, &correction], &[]).is_empty());
        assert!(contradictions(&[&correction, &original], &[]).is_empty());
    }

    // A superseded decision is history, not a competing claim — even against a
    // third decision that never corrected it.
    #[test]
    fn a_superseded_decision_does_not_conflict_with_an_unrelated_one() {
        let subject = Uuid::new_v4();
        let mut retired = decision(subject);
        retired.superseded_by = Some(Uuid::new_v4());
        let current = decision(subject);

        assert!(contradictions(&[&retired, &current], &[]).is_empty());
    }

    // And the negative that makes the two above about *supersession*: without
    // it, the same shape is flagged. Otherwise those tests would pass on a
    // function that finds nothing.
    #[test]
    fn the_same_two_decisions_are_flagged_when_neither_is_superseded() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);

        assert_eq!(contradictions(&[&one, &two], &[]).len(), 1);
    }

    // **Every** pair, not just consecutive ones: three competing decisions is
    // three disagreements, and a reviewer who only sees two of them will close
    // the review believing it settled.
    #[test]
    fn three_competing_decisions_produce_all_three_pairs() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let three = decision(subject);

        assert_eq!(contradictions(&[&one, &two, &three], &[]).len(), 3);
    }

    #[test]
    fn a_memory_never_contradicts_itself() {
        let subject = Uuid::new_v4();
        let only = decision(subject);

        assert!(contradictions(&[&only, &only], &[]).is_empty());
    }

    // Two decisions sharing one asset while each also being about another: the
    // shared anchor is the conflict, and it is named so a reviewer knows what
    // they are arbitrating.
    #[test]
    fn the_shared_anchor_is_the_one_reported() {
        let shared = Uuid::new_v4();
        let mut one = decision(shared);
        let mut two = decision(shared);
        one.links.push(MemoryLink {
            relation: LinkRelation::About,
            target: Uuid::new_v4(),
        });
        two.links.push(MemoryLink {
            relation: LinkRelation::About,
            target: Uuid::new_v4(),
        });

        let found = contradictions(&[&one, &two], &[]);

        assert_eq!(found[0].subject, Some(shared));
    }

    // `Mentions` is not an anchor, so two decisions that merely name the same
    // table are not competing about it — that is precisely why `Mentions` exists
    // as a separate relation.
    #[test]
    fn decisions_that_only_mention_the_same_asset_are_not_candidates() {
        let named = Uuid::new_v4();
        let mut one = decision(Uuid::new_v4());
        let mut two = decision(Uuid::new_v4());
        for memory in [&mut one, &mut two] {
            memory.links.push(MemoryLink {
                relation: LinkRelation::Mentions,
                target: named,
            });
        }

        assert!(contradictions(&[&one, &two], &[]).is_empty());
    }

    // ---- dismissal ----

    #[test]
    fn a_dismissed_pair_is_not_flagged_again() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let dismissal = dismissed(one.id, two.id);

        assert!(contradictions(&[&one, &two], &[dismissal]).is_empty());
    }

    // Order-independent, or a reviewer's dismissal silently stops working the
    // next time the pair is loaded in the other order — the worst kind of bug,
    // because it looks like the queue reopening on its own.
    #[test]
    fn a_dismissal_recorded_in_either_order_suppresses_the_pair() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let backwards = dismissed(two.id, one.id);

        assert!(contradictions(&[&one, &two], &[backwards]).is_empty());
    }

    // A dismissal is about **one pair**, not about a memory. Dismissing A-vs-B
    // must not quietly close A-vs-C.
    #[test]
    fn dismissing_one_pair_leaves_the_others_open() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let three = decision(subject);
        let dismissal = dismissed(one.id, two.id);

        let found = contradictions(&[&one, &two, &three], &[dismissal]);

        assert_eq!(found.len(), 2);
        assert!(!pairs(&found).contains(&normalised(one.id, two.id)));
    }

    // A person declaring a disagreement outranks a heuristic, but a person
    // *dismissing* one outranks the declaration too — it is the same person's
    // judgement, later.
    #[test]
    fn a_dismissal_also_suppresses_a_declared_contradiction() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        let two = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });
        let dismissal = dismissed(two.id, one.id);

        assert!(contradictions(&[&one, &two], &[dismissal]).is_empty());
    }

    #[test]
    fn an_unrelated_dismissal_suppresses_nothing() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let elsewhere = dismissed(Uuid::new_v4(), Uuid::new_v4());

        assert_eq!(contradictions(&[&one, &two], &[elsewhere]).len(), 1);
    }

    // A declared contradiction between two decisions about one asset is one
    // disagreement, and the human declaration is the truer description of it.
    #[test]
    fn a_declared_pair_is_not_also_reported_as_a_candidate() {
        let subject = Uuid::new_v4();
        let mut one = decision(subject);
        let two = decision(subject);
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });

        let found = contradictions(&[&one, &two], &[]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ContradictionKind::Declared);
    }

    // ---- confirmation ----

    // **Confirming is not resolving.** The pair stays in the queue, flagged as
    // confirmed — a confirmed disagreement that vanished from the queue would
    // read as settled, and settling it is the one thing this epic refuses to do.
    #[test]
    fn a_confirmed_candidate_stays_visible_and_is_marked_confirmed() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);

        let found = contradictions(&[&one, &two], &[confirmed(one.id, two.id)]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ContradictionKind::Confirmed);
        assert_eq!(found[0].subject, Some(subject));
    }

    // Order-independent, for the same reason dismissal is: a verdict recorded in
    // the other order would silently stop applying, and the queue would quietly
    // downgrade a reviewed pair back to a guess.
    #[test]
    fn a_confirmation_recorded_in_either_order_applies() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);

        let found = contradictions(&[&one, &two], &[confirmed(two.id, one.id)]);

        assert_eq!(found[0].kind, ContradictionKind::Confirmed);
    }

    // A confirmation **cannot resurrect** a pair detection no longer finds. If one
    // side has since been superseded the disagreement was settled by correction,
    // and re-raising it would reopen a question the record already answers.
    #[test]
    fn a_confirmation_does_not_resurrect_a_pair_that_no_longer_conflicts() {
        let subject = Uuid::new_v4();
        let mut retired = decision(subject);
        let current = decision(subject);
        let review = confirmed(retired.id, current.id);
        retired.superseded_by = Some(current.id);

        assert!(contradictions(&[&retired, &current], &[review]).is_empty());
    }

    // Confirmed beats declared: a reviewer who looked at *this pair in the queue*
    // has made the stronger statement, and the two provenances are kept apart
    // because a confirmed candidate is evidence the heuristic works while a
    // declared one says nothing about it.
    #[test]
    fn confirming_a_declared_contradiction_upgrades_it() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        let two = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });

        let found = contradictions(&[&one, &two], &[confirmed(one.id, two.id)]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ContradictionKind::Confirmed);
    }

    // And the negative: without the review it is still `Declared`, so the test
    // above is about the confirmation rather than about a function that always
    // says confirmed.
    #[test]
    fn an_unreviewed_declaration_stays_declared() {
        let mut one = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        let two = of_kind(MemoryKind::Caveat, Uuid::new_v4());
        one.links.push(MemoryLink {
            relation: LinkRelation::Contradicts,
            target: two.id,
        });

        let found = contradictions(&[&one, &two], &[]);

        assert_eq!(found[0].kind, ContradictionKind::Declared);
    }

    // A verdict about one pair says nothing about another. Confirming A-vs-B must
    // not upgrade A-vs-C, which would put a reviewer's name against a judgement
    // they never made.
    #[test]
    fn confirming_one_pair_leaves_the_others_as_candidates() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let three = decision(subject);

        let found = contradictions(&[&one, &two, &three], &[confirmed(one.id, two.id)]);

        assert_eq!(found.len(), 3);
        let upgraded = found
            .iter()
            .filter(|c| c.kind == ContradictionKind::Confirmed)
            .count();
        assert_eq!(upgraded, 1);
    }

    // Unreachable through storage — the primary key is the pair — but pinned so a
    // future second writer cannot make the queue depend on row order. Dismissal
    // wins, because the pair was removed before anything could upgrade it.
    #[test]
    fn a_pair_carrying_both_verdicts_resolves_deterministically() {
        let subject = Uuid::new_v4();
        let one = decision(subject);
        let two = decision(subject);
        let both = [confirmed(one.id, two.id), dismissed(one.id, two.id)];
        let reversed = [dismissed(one.id, two.id), confirmed(one.id, two.id)];

        assert!(contradictions(&[&one, &two], &both).is_empty());
        assert!(contradictions(&[&one, &two], &reversed).is_empty());
    }

    #[test]
    fn nothing_to_compare_finds_nothing() {
        assert!(contradictions(&[], &[]).is_empty());
    }
}
