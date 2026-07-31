//! Business meaning: terms, their SKOS relations, and the review they pass
//! through before anything may point at them — Epic 24 Slices B, C and D.
//!
//! Everything here is a decision rather than a shape, which is why it is in the
//! leaf crate: the transition matrix, the attachability rule, and the direction
//! of a SKOS relation are all things that can be *wrong*, as opposed to merely
//! stored. A leaf crate also mutates several times faster than the server does.
//!
//! **The vocabulary is SKOS, not an invented one** (decision 2). `broader`,
//! `narrower`, `related`, `exactMatch`, `closeMatch` are W3C's, which is what
//! makes Epic 9's export a mapping rather than a translation and Epic 33's packs
//! importable. The source is the SKOS Reference; no implementation was consulted.

use serde::{Deserialize, Serialize};

/// Where a term is in its life.
///
/// Ordered as it is lived, but **not** `Ord` — "greater than `InReview`" is not
/// a question with an answer, and deriving the trait would invite a comparison
/// somewhere that reads as a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TermStatus {
    Draft,
    InReview,
    Approved,
    Deprecated,
}

impl TermStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TermStatus::Draft => "draft",
            TermStatus::InReview => "inReview",
            TermStatus::Approved => "approved",
            TermStatus::Deprecated => "deprecated",
        }
    }

    /// # Errors
    /// The unrecognised value, so the caller can name it in the response.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "draft" => Ok(TermStatus::Draft),
            "inReview" => Ok(TermStatus::InReview),
            "approved" => Ok(TermStatus::Approved),
            "deprecated" => Ok(TermStatus::Deprecated),
            other => Err(other.to_string()),
        }
    }

    pub const ALL: [TermStatus; 4] = [
        TermStatus::Draft,
        TermStatus::InReview,
        TermStatus::Approved,
        TermStatus::Deprecated,
    ];
}

/// Why a transition was refused, so the response can say which rule was hit
/// rather than a bare `422`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("a term cannot go from {from} to {to}")]
    NotPermitted {
        from: &'static str,
        to: &'static str,
    },
    #[error("approval needs at least one assigned reviewer")]
    NoReviewer,
    #[error("only an assigned reviewer may approve this term")]
    NotAReviewer,
}

/// May a term move from one status to another?
///
/// Separate from [`approve`] because the matrix is about the *shape* of the
/// workflow and approval additionally involves who is asking. Splitting them
/// keeps the matrix testable as a matrix.
#[must_use]
// Kept flat, one permitted transition per line, because each carries its own
// reason. Nesting them as clippy suggests merges `InReview→Approved` with
// `InReview→Draft` — two moves with opposite meanings — into one pattern that no
// comment can then describe.
#[allow(clippy::unnested_or_patterns)]
pub fn can_transition(from: TermStatus, to: TermStatus) -> bool {
    // Written as the permitted set rather than as refusals, so a status added
    // later is refused by default. The opposite shape — listing what is illegal —
    // silently permits every pair nobody thought about.
    matches!(
        (from, to),
        (TermStatus::Draft, TermStatus::InReview)
            | (TermStatus::InReview, TermStatus::Approved)
            // Back to the author for more work. Without it a reviewer's only
            // options are approve or abandon.
            | (TermStatus::InReview, TermStatus::Draft)
            // A definition that turns out wrong needs a path back that is not
            // deprecation.
            | (TermStatus::Approved, TermStatus::InReview)
            | (TermStatus::Approved, TermStatus::Deprecated)
    )
}

/// Attempt a transition, given who is asking and who was assigned.
///
/// `actor` and `reviewers` matter only for approval; every other move is a
/// question of the matrix alone.
///
/// # Errors
///
/// [`TransitionError`] naming which of the three rules refused it.
pub fn transition(
    from: TermStatus,
    to: TermStatus,
    actor: &str,
    reviewers: &[String],
) -> Result<TermStatus, TransitionError> {
    if !can_transition(from, to) {
        return Err(TransitionError::NotPermitted {
            from: from.as_str(),
            to: to.as_str(),
        });
    }
    // The reviewer rules bind approval and nothing else. Requiring an assignee to
    // move a term *into* review would mean a term cannot enter the workflow until
    // somebody is already reviewing it, which inverts the order people work in.
    if to == TermStatus::Approved {
        if reviewers.is_empty() {
            return Err(TransitionError::NoReviewer);
        }
        if !reviewers.iter().any(|r| r == actor) {
            return Err(TransitionError::NotAReviewer);
        }
    }
    Ok(to)
}

/// May this term be attached to an asset?
///
/// **Only `Approved`** (decision 4). A draft definition attached to a thousand
/// columns becomes the de facto definition whatever its status says, and the
/// status then describes a review nobody can act on.
#[must_use]
pub fn is_attachable(status: TermStatus) -> bool {
    matches!(status, TermStatus::Approved)
}

/// One SKOS relation from a term to something else.
///
/// The internal relations carry a term id; the match relations carry an IRI,
/// because they point *out* of this catalog by design.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "target")]
pub enum SkosRelation {
    Broader(String),
    Narrower(String),
    Related(String),
    /// An IRI in another vocabulary asserting the same concept.
    ///
    /// **Not checked for reachability.** A vocabulary that must be online for a
    /// term to be valid is a vocabulary whose terms fail when somebody else's
    /// server does, and `exactMatch` is a claim about meaning rather than about
    /// availability.
    ExactMatch(String),
    CloseMatch(String),
}

impl SkosRelation {
    #[must_use]
    pub fn predicate(&self) -> &'static str {
        match self {
            SkosRelation::Broader(_) => "skos:broader",
            SkosRelation::Narrower(_) => "skos:narrower",
            SkosRelation::Related(_) => "skos:related",
            SkosRelation::ExactMatch(_) => "skos:exactMatch",
            SkosRelation::CloseMatch(_) => "skos:closeMatch",
        }
    }

    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            SkosRelation::Broader(t)
            | SkosRelation::Narrower(t)
            | SkosRelation::Related(t)
            | SkosRelation::ExactMatch(t)
            | SkosRelation::CloseMatch(t) => t,
        }
    }

    /// Does this relation point at another term in this catalog, rather than out
    /// at an IRI? Only these participate in cycles and inversion.
    #[must_use]
    pub fn is_internal(&self) -> bool {
        matches!(
            self,
            SkosRelation::Broader(_) | SkosRelation::Narrower(_) | SkosRelation::Related(_)
        )
    }
}

/// The relation that must be visible from the other end.
///
/// **Read, not stored** (Slice B). Storing both directions gives two rows that
/// can disagree, and the one that is wrong is whichever nobody looked at. So
/// `broader` A→B is the single stored fact and `narrower` B→A is derived from
/// it on read.
///
/// `None` for the match relations: the far end is an IRI in somebody else's
/// vocabulary, and this catalog cannot add a row to it.
#[must_use]
pub fn inverse_of(relation: &SkosRelation, from_term: &str) -> Option<SkosRelation> {
    match relation {
        SkosRelation::Broader(_) => Some(SkosRelation::Narrower(from_term.to_string())),
        SkosRelation::Narrower(_) => Some(SkosRelation::Broader(from_term.to_string())),
        SkosRelation::Related(_) => Some(SkosRelation::Related(from_term.to_string())),
        // The far end is an IRI in somebody else's vocabulary. Inventing an
        // inverse would assert a fact about a system nobody here controls.
        SkosRelation::ExactMatch(_) | SkosRelation::CloseMatch(_) => None,
    }
}

/// Every relation visible on `term`, stored and derived together.
///
/// `stored` is what each term declared, keyed by term id.
#[must_use]
pub fn visible_relations(term: &str, stored: &[(String, SkosRelation)]) -> Vec<SkosRelation> {
    stored
        .iter()
        .filter_map(|(owner, relation)| {
            if owner == term {
                // What this term declared, as declared.
                Some(relation.clone())
            } else if relation.target() == term {
                // The other end, derived rather than stored — one fact, so there
                // is no second row to disagree with the first.
                inverse_of(relation, owner)
            } else {
                None
            }
        })
        .collect()
}

/// Would adding `broader` from `child` to `parent` close a loop?
///
/// Walks the existing `broader` edges. **Poly-hierarchy is legitimate in SKOS**
/// — a term may have several `broader` parents — so this cannot assume one
/// parent per term, and a walk that follows only the first would miss a cycle
/// through the second.
#[must_use]
pub fn would_cycle(child: &str, parent: &str, broader: &[(String, String)]) -> bool {
    // Walk up from the proposed parent. Reaching `child` means the new edge would
    // close the loop.
    let mut seen = std::collections::HashSet::new();
    let mut frontier = vec![parent.to_string()];
    while let Some(current) = frontier.pop() {
        if current == child {
            return true;
        }
        // Guards a store that is *already* cyclic — a migration or a direct write
        // can put it there, and a hang has nothing to report.
        if !seen.insert(current.clone()) {
            continue;
        }
        // **Every** parent, not the first. Poly-hierarchy is legitimate SKOS, so a
        // walk that took one parent would miss a loop through the second and call
        // the graph clean.
        frontier.extend(
            broader
                .iter()
                .filter(|(from, _)| *from == current)
                .map(|(_, to)| to.clone()),
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reviewers(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    // ---- the transition matrix (Slice C) ----

    #[test]
    fn a_term_walks_the_workflow_in_order() {
        assert!(can_transition(TermStatus::Draft, TermStatus::InReview));
        assert!(can_transition(TermStatus::InReview, TermStatus::Approved));
        assert!(can_transition(TermStatus::Approved, TermStatus::Deprecated));
    }

    // **The illegal move that matters.** Draft→Approved skips review entirely,
    // which is the whole mechanism — a workflow that can be jumped is decoration.
    #[test]
    fn a_term_cannot_skip_review() {
        assert!(!can_transition(TermStatus::Draft, TermStatus::Approved));
    }

    // Sending a term back for more work is legitimate and has to be permitted, or
    // a reviewer's only options are approve or abandon.
    #[test]
    fn a_term_in_review_can_be_sent_back() {
        assert!(can_transition(TermStatus::InReview, TermStatus::Draft));
    }

    // A deprecated term is the end of the line. Reviving one would give two
    // periods of validity with a gap in the middle, and every asset attached
    // during the gap would have pointed at a term that was not attachable.
    #[test]
    fn a_deprecated_term_does_not_come_back() {
        for to in TermStatus::ALL {
            assert!(
                !can_transition(TermStatus::Deprecated, to),
                "deprecated should not reach {to:?}"
            );
        }
    }

    // Every status pair is either permitted or refused, and the ones nobody
    // thought about must default to refused. An always-permit check passes each
    // positive test above and fails only here.
    #[test]
    fn no_status_transitions_to_itself_and_the_matrix_is_mostly_closed() {
        for from in TermStatus::ALL {
            assert!(
                !can_transition(from, from),
                "{from:?} should not transition to itself"
            );
        }
        let permitted = TermStatus::ALL
            .iter()
            .flat_map(|from| TermStatus::ALL.iter().map(move |to| (*from, *to)))
            .filter(|(from, to)| can_transition(*from, *to))
            .count();
        // Draft→InReview, InReview→Approved, InReview→Draft, Approved→Deprecated,
        // Approved→InReview. Five, and pinning the count is what catches a
        // permissive matrix that passes every case above.
        assert_eq!(permitted, 5);
    }

    // Re-opening an approved term for review is permitted: a definition that
    // turns out to be wrong needs a path back that is not deprecation.
    #[test]
    fn an_approved_term_can_be_reopened() {
        assert!(can_transition(TermStatus::Approved, TermStatus::InReview));
    }

    // ---- approval's extra rules (Slice C) ----

    #[test]
    fn an_assigned_reviewer_may_approve() {
        let after = transition(
            TermStatus::InReview,
            TermStatus::Approved,
            "alice",
            &reviewers(&["alice", "bob"]),
        )
        .expect("alice was assigned");

        assert_eq!(after, TermStatus::Approved);
    }

    // The negative half. Anyone approving their own term makes the reviewer list
    // decoration, and a list that does not constrain anything is worse than none
    // — it reads as a control that is not there.
    #[test]
    fn somebody_who_was_not_assigned_cannot_approve() {
        let error = transition(
            TermStatus::InReview,
            TermStatus::Approved,
            "mallory",
            &reviewers(&["alice"]),
        )
        .expect_err("mallory was not assigned");

        assert_eq!(error, TransitionError::NotAReviewer);
    }

    #[test]
    fn approval_with_no_reviewer_assigned_is_refused() {
        let error = transition(TermStatus::InReview, TermStatus::Approved, "alice", &[])
            .expect_err("nobody was assigned");

        assert_eq!(error, TransitionError::NoReviewer);
    }

    // The reviewer rules apply *only* to approval. Requiring a reviewer to move a
    // term into review would mean a term cannot enter the workflow until somebody
    // has already been assigned to it, which inverts the order people work in.
    #[test]
    fn moving_a_term_into_review_needs_no_reviewer() {
        assert_eq!(
            transition(TermStatus::Draft, TermStatus::InReview, "alice", &[]),
            Ok(TermStatus::InReview)
        );
    }

    #[test]
    fn an_illegal_transition_names_both_ends() {
        let error = transition(
            TermStatus::Draft,
            TermStatus::Approved,
            "alice",
            &reviewers(&["alice"]),
        )
        .expect_err("draft cannot be approved");

        assert_eq!(
            error,
            TransitionError::NotPermitted {
                from: "draft",
                to: "approved",
            }
        );
    }

    // ---- attachability (Slice D) ----

    // Both halves in one test, because an unconditional check passes whichever
    // half is written alone — which is exactly the mutant the plan names.
    #[test]
    fn only_an_approved_term_attaches() {
        assert!(is_attachable(TermStatus::Approved));
        assert!(!is_attachable(TermStatus::Draft));
        assert!(!is_attachable(TermStatus::InReview));
        assert!(!is_attachable(TermStatus::Deprecated));
    }

    // ---- SKOS relations (Slice B) ----

    #[test]
    fn broader_and_narrower_are_each_others_inverse() {
        assert_eq!(
            inverse_of(&SkosRelation::Broader("parent".into()), "child"),
            Some(SkosRelation::Narrower("child".into()))
        );
        assert_eq!(
            inverse_of(&SkosRelation::Narrower("child".into()), "parent"),
            Some(SkosRelation::Broader("parent".into()))
        );
    }

    // `related` is symmetric, so its inverse is itself pointed the other way.
    #[test]
    fn related_is_symmetric() {
        assert_eq!(
            inverse_of(&SkosRelation::Related("b".into()), "a"),
            Some(SkosRelation::Related("a".into()))
        );
    }

    // **The far end of a match is somebody else's vocabulary.** This catalog
    // cannot add a row to it, so inventing an inverse would assert a fact about a
    // system nobody here controls.
    #[test]
    fn a_match_to_an_external_iri_has_no_inverse_to_write() {
        assert_eq!(
            inverse_of(&SkosRelation::ExactMatch("http://x.example/1".into()), "a"),
            None
        );
        assert_eq!(
            inverse_of(&SkosRelation::CloseMatch("http://x.example/1".into()), "a"),
            None
        );
    }

    // **The single-edge assertion.** `narrower` must be visible from the parent
    // without a second stored row — two rows can disagree, and the wrong one is
    // whichever nobody looked at.
    #[test]
    fn narrower_is_visible_from_the_parent_without_being_stored() {
        let stored = vec![("child".to_string(), SkosRelation::Broader("parent".into()))];

        let on_parent = visible_relations("parent", &stored);

        assert_eq!(on_parent, vec![SkosRelation::Narrower("child".into())]);
    }

    #[test]
    fn a_terms_own_declarations_are_visible_on_it() {
        let stored = vec![("child".to_string(), SkosRelation::Broader("parent".into()))];

        assert_eq!(
            visible_relations("child", &stored),
            vec![SkosRelation::Broader("parent".into())]
        );
    }

    // The negative that makes the two above mean something: an unrelated term
    // must see nothing. A `visible_relations` that returned everything would
    // satisfy both positives.
    #[test]
    fn an_unrelated_term_sees_none_of_it() {
        let stored = vec![("child".to_string(), SkosRelation::Broader("parent".into()))];

        assert!(visible_relations("stranger", &stored).is_empty());
    }

    // A match relation is visible on the term that declared it and nowhere else —
    // there is no other end in this catalog to show it on.
    #[test]
    fn an_external_match_appears_only_on_the_term_that_declared_it() {
        let stored = vec![(
            "a".to_string(),
            SkosRelation::ExactMatch("http://x.example/1".into()),
        )];

        assert_eq!(visible_relations("a", &stored).len(), 1);
        assert!(visible_relations("http://x.example/1", &stored).is_empty());
    }

    // **Decision 2 is only real if these strings are right.** The whole argument
    // for SKOS over an invented vocabulary is that Epic 9's export becomes a
    // mapping rather than a translation and Epic 33's packs stay importable — and
    // both of those rest entirely on emitting the predicate W3C actually defines.
    // A typo here is invisible in this catalog and wrong in every consumer of it.
    #[test]
    fn each_relation_projects_as_the_skos_predicate_it_names() {
        assert_eq!(
            SkosRelation::Broader("x".into()).predicate(),
            "skos:broader"
        );
        assert_eq!(
            SkosRelation::Narrower("x".into()).predicate(),
            "skos:narrower"
        );
        assert_eq!(
            SkosRelation::Related("x".into()).predicate(),
            "skos:related"
        );
        assert_eq!(
            SkosRelation::ExactMatch("x".into()).predicate(),
            "skos:exactMatch"
        );
        assert_eq!(
            SkosRelation::CloseMatch("x".into()).predicate(),
            "skos:closeMatch"
        );
    }

    // Internal relations are the ones with another end in this catalog, so they
    // are the ones that can cycle and the ones that can be inverted. Getting this
    // wrong would send an external IRI into the cycle walk, where it would be
    // treated as a term id that happens to match nothing.
    #[test]
    fn only_the_relations_with_a_local_far_end_are_internal() {
        assert!(SkosRelation::Broader("x".into()).is_internal());
        assert!(SkosRelation::Narrower("x".into()).is_internal());
        assert!(SkosRelation::Related("x".into()).is_internal());
        assert!(!SkosRelation::ExactMatch("http://x.example/1".into()).is_internal());
        assert!(!SkosRelation::CloseMatch("http://x.example/1".into()).is_internal());
    }

    // The two must agree: exactly the relations that have an inverse to write are
    // the internal ones. Left independent, one can drift from the other and the
    // symptom is a derived edge pointing at an IRI.
    #[test]
    fn a_relation_has_an_inverse_exactly_when_it_is_internal() {
        for relation in [
            SkosRelation::Broader("x".into()),
            SkosRelation::Narrower("x".into()),
            SkosRelation::Related("x".into()),
            SkosRelation::ExactMatch("http://x.example/1".into()),
            SkosRelation::CloseMatch("http://x.example/1".into()),
        ] {
            assert_eq!(
                inverse_of(&relation, "a").is_some(),
                relation.is_internal(),
                "{relation:?}"
            );
        }
    }

    // ---- cycles, at depth (Slice B) ----

    #[test]
    fn a_term_cannot_be_its_own_broader() {
        assert!(would_cycle("a", "a", &[]));
    }

    #[test]
    fn a_two_step_loop_is_refused() {
        let broader = vec![("b".to_string(), "a".to_string())];

        assert!(would_cycle("a", "b", &broader));
    }

    // Depth 3, because a check that compares only the immediate parent passes
    // depth 1 and fails here — the same trap Epic 11's nesting hit.
    #[test]
    fn a_three_step_loop_is_refused() {
        let broader = vec![
            ("b".to_string(), "a".to_string()),
            ("c".to_string(), "b".to_string()),
        ];

        assert!(would_cycle("a", "c", &broader));
    }

    // The negative. A checker that refused everything would pass all three above.
    #[test]
    fn an_unrelated_parent_is_permitted() {
        let broader = vec![("b".to_string(), "a".to_string())];

        assert!(!would_cycle("c", "b", &broader));
    }

    // **Poly-hierarchy is normal SKOS data, not an edge case.** A term may have
    // several `broader` parents, and the walk has to follow all of them — one
    // that follows only the first misses a cycle through the second and reports
    // the graph as clean.
    #[test]
    fn a_cycle_through_a_second_parent_is_still_found() {
        let broader = vec![
            // `c` has two parents. The loop runs through the second one.
            ("c".to_string(), "unrelated".to_string()),
            ("c".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ];

        assert!(would_cycle("a", "c", &broader));
    }

    #[test]
    fn several_broader_parents_are_permitted_when_they_close_no_loop() {
        let broader = vec![("a".to_string(), "b".to_string())];

        assert!(!would_cycle("a", "c", &broader), "a may be under b and c");
    }

    // An already-cyclic store must not hang the walk. The data can be cyclic even
    // though this check exists, because a migration or a direct write can put it
    // there — and a hang has nothing to report.
    #[test]
    fn an_already_cyclic_store_terminates() {
        let broader = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ];

        let _ = would_cycle("x", "a", &broader);
    }
}
