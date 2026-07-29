//! Organizational memory — Epic 31, the headline differentiator.
//!
//! The knowledge that currently evaporates into chats, tickets and notebooks:
//! *why* a metric changed, *why* a pipeline failed, *why* a dashboard was
//! deprecated. Pure domain types and the decisions that make them trustworthy;
//! storage and retrieval live above.
//!
//! Three properties are structural rather than validated, because each is the
//! kind of rule a later contributor breaks by accident:
//!
//! 1. **Authorship cannot be changed** — [`MemoryUpdate`] has no field for it.
//! 2. **A memory cannot exist unanchored** — [`Memory::new`] refuses without an
//!    `About` link.
//! 3. **Staleness is never stored** — there is no field, only [`staleness`].

use crate::envelope::EntityVersion;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What kind of knowledge this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryKind {
    /// Why something is the way it is.
    Rationale,
    /// What went wrong and what was done.
    Incident,
    /// A decision and the alternatives rejected.
    Decision,
    /// How to interpret something correctly.
    Caveat,
}

/// Who wrote this, and therefore how much it is worth.
///
/// **Immutable once set.** An agent-authored memory relabelled as human-authored
/// destroys the trust model: a reader weighing a claim needs to know whether a
/// person stood behind it, and a field somebody can edit is a field that will be
/// edited by a migration script. [`MemoryUpdate`] has no authorship field, so
/// there is nothing to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Authorship {
    Human {
        user_id: String,
    },
    /// An agent, named. Not "a machine" — which agent matters when its
    /// conclusions turn out to be wrong and somebody has to find the rest.
    Agent {
        agent_id: String,
        model: String,
    },
}

impl Authorship {
    /// The confidence a memory gets when nobody stated one.
    ///
    /// **A person who writes something down means it.** `1.0` for a human is not
    /// flattery; it is that a human memory with no stated confidence is an
    /// assertion, and defaulting it lower would silently rank every unlabelled
    /// human note below every agent guess that happened to claim `0.9`.
    ///
    /// An agent must state its own, because an agent that does not know how sure
    /// it is has told you something important.
    #[must_use]
    pub fn default_confidence(&self) -> Option<f64> {
        match self {
            Self::Human { .. } => Some(1.0),
            Self::Agent { .. } => None,
        }
    }
}

/// How a memory relates to what it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkRelation {
    /// The anchor. **At least one is required** — see [`Memory::new`].
    About,
    /// Something this memory caused or explains the state of.
    Affects,
    /// Evidence: a run, a query, a dashboard.
    Evidence,
    /// Another memory this one builds on.
    Follows,
    /// Named in passing. **Deliberately weaker than `About`** and not an
    /// anchor: a memory that mentions a table is not a memory about it, and
    /// letting it anchor would make every incident report the primary answer
    /// for every asset it happened to name.
    Mentions,
}

/// One edge from a memory to something in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLink {
    pub relation: LinkRelation,
    /// The asset, run, or other memory this points at.
    pub target: Uuid,
}

/// Why a memory could not be created.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MemoryError {
    /// **An unanchored memory is unretrievable.** Retrieval is "what do we know
    /// about *this*", so a memory linked to nothing can never be an answer — it
    /// is written, stored, and invisible, which is worse than being refused.
    #[error("a memory needs at least one `about` link; without one it can never be retrieved")]
    NoAnchor,
    #[error("confidence must be between 0 and 1, got {0}")]
    ConfidenceOutOfRange(f64),
    #[error("a memory needs content")]
    NoContent,
    #[error("an agent-authored memory has to state its own confidence")]
    AgentWithoutConfidence,
}

/// A piece of organizational knowledge.
///
/// **No staleness field.** Whether a memory still describes its subject changes
/// when the *subject* changes, not when the memory does — so a stored flag is
/// wrong the moment somebody edits the table it is about, and wrong silently.
/// See [`staleness`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: Uuid,
    pub kind: MemoryKind,
    pub content: String,
    /// A one-line form for a list or an agent's context budget. Optional: a
    /// forced summary is a truncated first sentence, which is worse than none.
    pub summary: Option<String>,
    pub authorship: Authorship,
    pub confidence: f64,
    pub links: Vec<MemoryLink>,
    /// The instant this was true of its subject. Compared against the subject's
    /// version to compute staleness.
    pub as_of: DateTime<Utc>,
    /// The memory this one corrects, if any.
    pub supersedes: Option<Uuid>,
    /// The memory that corrected this one. **Set rather than overwriting**, so
    /// what people believed survives being corrected — which is most of the
    /// value of keeping a record at all.
    pub superseded_by: Option<Uuid>,
}

/// What a client may change.
///
/// **No `authorship`, no `supersedes`, no `superseded_by`.** Authorship is
/// immutable; the supersession fields are set by the supersede operation, which
/// has to write both sides at once. Leaving them out means serde drops anything
/// a client sends, so the rule cannot be broken by a request — the same
/// structural approach `TableUpdate` uses for `id`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdate {
    pub content: Option<String>,
    /// `null` clears it, absent leaves it alone. Collapsing the two makes
    /// "remove this summary" unexpressible, and a forced summary is a truncated
    /// first sentence — worse than none.
    // `Option<Option<T>>` is what clippy warns about and exactly what is
    // wanted: the outer layer is "did the client mention this field", the inner
    // one is "what did they set it to". Flattening either away loses a case.
    #[allow(
        clippy::option_option,
        reason = "absent and null are different requests"
    )]
    #[serde(default, deserialize_with = "explicit_null")]
    pub summary: Option<Option<String>>,
    pub confidence: Option<f64>,
    pub links: Option<Vec<MemoryLink>>,
}

/// Tell `null` apart from absent.
///
/// serde's default collapses both to `None`, which is right for a field that
/// cannot be cleared and wrong for one that can. Written here rather than taken
/// from a dependency: it is four lines, and the workspace does not otherwise
/// need `serde_with`.
#[allow(
    clippy::option_option,
    reason = "absent and null are different requests"
)]
fn explicit_null<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

impl Memory {
    /// A new memory, or the reason it cannot exist.
    ///
    /// # Errors
    ///
    /// [`MemoryError::NoAnchor`] without an `About` link;
    /// [`MemoryError::NoContent`] for blank content;
    /// [`MemoryError::ConfidenceOutOfRange`] outside `[0, 1]`;
    /// [`MemoryError::AgentWithoutConfidence`] when an agent states none.
    pub fn new(
        kind: MemoryKind,
        content: String,
        authorship: Authorship,
        confidence: Option<f64>,
        links: Vec<MemoryLink>,
        as_of: DateTime<Utc>,
    ) -> Result<Self, MemoryError> {
        if content.trim().is_empty() {
            return Err(MemoryError::NoContent);
        }
        if !links.iter().any(|l| l.relation == LinkRelation::About) {
            return Err(MemoryError::NoAnchor);
        }

        let Some(confidence) = confidence.or_else(|| authorship.default_confidence()) else {
            return Err(MemoryError::AgentWithoutConfidence);
        };
        // Checked after defaulting, so a supplied value is validated and a
        // defaulted one cannot be out of range by construction.
        if !(0.0..=1.0).contains(&confidence) {
            return Err(MemoryError::ConfidenceOutOfRange(confidence));
        }

        Ok(Self {
            id: Uuid::new_v4(),
            kind,
            content,
            summary: None,
            authorship,
            confidence,
            links,
            as_of,
            supersedes: None,
            superseded_by: None,
        })
    }

    /// Whether this memory has been corrected.
    #[must_use]
    pub fn is_superseded(&self) -> bool {
        self.superseded_by.is_some()
    }

    /// Everything this memory is anchored to.
    #[must_use]
    pub fn anchors(&self) -> Vec<Uuid> {
        self.links
            .iter()
            .filter(|l| l.relation == LinkRelation::About)
            .map(|l| l.target)
            .collect()
    }
}

/// How far a memory has drifted from its subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Staleness {
    /// The subject has not changed since the memory was written.
    Fresh,
    /// A **Minor** bump: something was added or described. The memory is
    /// probably still true, and saying "stale" would train readers to ignore the
    /// flag on the many occasions it does not matter.
    PossiblyStale { since: EntityVersion },
    /// A **Major** bump — a breaking change. What the memory describes may no
    /// longer exist.
    Stale { since: EntityVersion },
    /// The subject is gone, or was never resolvable. Distinct from stale: there
    /// is nothing left to compare against, and reporting `Fresh` would be a
    /// confident answer about a subject nobody can see.
    SubjectUnknown,
}

/// Compare a memory against its subject's current version.
///
/// **Computed, never stored.** Staleness changes when the *subject* changes, not
/// when the memory does — a stored flag is wrong from the moment somebody edits
/// the table, and wrong silently, which is the worst way for a trust signal to
/// fail. There is deliberately no field on [`Memory`] to put this in.
///
/// A stale memory is **returned and flagged, never hidden**: "we knew this and
/// it may have changed" is information, and dropping it leaves a reader
/// believing nobody ever looked.
#[must_use]
pub fn staleness(
    memory_written_at: EntityVersion,
    subject_now: Option<EntityVersion>,
) -> Staleness {
    let Some(now) = subject_now else {
        return Staleness::SubjectUnknown;
    };
    if now.major > memory_written_at.major {
        return Staleness::Stale { since: now };
    }
    // Guarded on **equal** major, not merely "not greater". Comparing minor
    // independently reports a subject at `1.9` as changed since a memory
    // written at `2.0` — an alarm about nothing, on every memory written
    // during a rollback. Found by the test, not by reading the code.
    if now.major == memory_written_at.major && now.minor > memory_written_at.minor {
        return Staleness::PossiblyStale { since: now };
    }
    Staleness::Fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about(target: Uuid) -> MemoryLink {
        MemoryLink {
            relation: LinkRelation::About,
            target,
        }
    }

    fn human() -> Authorship {
        Authorship::Human {
            user_id: "sakshi".into(),
        }
    }

    fn agent() -> Authorship {
        Authorship::Agent {
            agent_id: "lineage-explainer".into(),
            model: "claude-opus-5".into(),
        }
    }

    fn memory_with(
        authorship: Authorship,
        confidence: Option<f64>,
        links: Vec<MemoryLink>,
    ) -> Result<Memory, MemoryError> {
        Memory::new(
            MemoryKind::Rationale,
            "The revenue metric excludes refunds from 2025 onward.".into(),
            authorship,
            confidence,
            links,
            Utc::now(),
        )
    }

    fn version(major: u32, minor: u32) -> EntityVersion {
        EntityVersion { major, minor }
    }

    // **An unanchored memory is unretrievable.** Retrieval answers "what do we
    // know about *this*", so a memory linked to nothing is written, stored, and
    // permanently invisible — which is worse than being refused at the door.
    #[test]
    fn refuses_a_memory_anchored_to_nothing() {
        let err = memory_with(human(), None, vec![]).unwrap_err();

        assert_eq!(err, MemoryError::NoAnchor);
    }

    // And the negative that makes the check about `About` specifically rather
    // than about link count: three links, none of them an anchor, is still
    // unretrievable.
    #[test]
    fn refuses_links_that_are_not_anchors() {
        let links = vec![
            MemoryLink {
                relation: LinkRelation::Affects,
                target: Uuid::new_v4(),
            },
            MemoryLink {
                relation: LinkRelation::Evidence,
                target: Uuid::new_v4(),
            },
            MemoryLink {
                relation: LinkRelation::Follows,
                target: Uuid::new_v4(),
            },
        ];

        assert_eq!(
            memory_with(human(), None, links).unwrap_err(),
            MemoryError::NoAnchor
        );
    }

    #[test]
    fn accepts_one_anchor_among_several_relations() {
        let links = vec![
            MemoryLink {
                relation: LinkRelation::Evidence,
                target: Uuid::new_v4(),
            },
            about(Uuid::new_v4()),
            MemoryLink {
                relation: LinkRelation::Affects,
                target: Uuid::new_v4(),
            },
        ];

        let memory = memory_with(human(), None, links).unwrap();

        assert_eq!(memory.links.len(), 3);
    }

    // A person who writes something down means it. Defaulting an unlabelled
    // human note lower would rank it below every agent guess that claimed 0.9.
    #[test]
    fn a_human_memory_defaults_to_full_confidence() {
        let memory = memory_with(human(), None, vec![about(Uuid::new_v4())]).unwrap();

        assert!((memory.confidence - 1.0).abs() < f64::EPSILON);
    }

    // An agent that does not know how sure it is has told you something
    // important, so it may not borrow the human default.
    #[test]
    fn an_agent_must_state_its_own_confidence() {
        let err = memory_with(agent(), None, vec![about(Uuid::new_v4())]).unwrap_err();

        assert_eq!(err, MemoryError::AgentWithoutConfidence);
    }

    #[test]
    fn an_agent_that_states_confidence_is_accepted() {
        let memory = memory_with(agent(), Some(0.7), vec![about(Uuid::new_v4())]).unwrap();

        assert!((memory.confidence - 0.7).abs() < f64::EPSILON);
    }

    // A stated confidence wins over the default — otherwise a human who
    // deliberately wrote 0.4 would be recorded as certain.
    #[test]
    fn a_stated_confidence_overrides_the_human_default() {
        let memory = memory_with(human(), Some(0.4), vec![about(Uuid::new_v4())]).unwrap();

        assert!((memory.confidence - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn refuses_confidence_outside_the_unit_interval() {
        for out in [-0.1, 1.1, 2.0] {
            assert_eq!(
                memory_with(human(), Some(out), vec![about(Uuid::new_v4())]).unwrap_err(),
                MemoryError::ConfidenceOutOfRange(out),
                "{out} should be refused"
            );
        }
    }

    // Both ends are legal values, not off-by-one errors. `0.0` is "recorded but
    // disbelieved", which is a real thing to want to say.
    #[test]
    fn accepts_both_ends_of_the_interval() {
        for edge in [0.0, 1.0] {
            assert!(
                memory_with(human(), Some(edge), vec![about(Uuid::new_v4())]).is_ok(),
                "{edge} should be accepted"
            );
        }
    }

    #[test]
    fn refuses_content_that_is_only_whitespace() {
        for blank in ["", "   ", "\n\t"] {
            let err = Memory::new(
                MemoryKind::Caveat,
                blank.into(),
                human(),
                None,
                vec![about(Uuid::new_v4())],
                Utc::now(),
            )
            .unwrap_err();

            assert_eq!(err, MemoryError::NoContent);
        }
    }

    #[test]
    fn anchors_are_the_about_targets_only() {
        let anchor = Uuid::new_v4();
        let affected = Uuid::new_v4();
        let links = vec![
            about(anchor),
            MemoryLink {
                relation: LinkRelation::Affects,
                target: affected,
            },
        ];

        let memory = memory_with(human(), None, links).unwrap();

        assert_eq!(memory.anchors(), vec![anchor]);
    }

    #[test]
    fn a_new_memory_is_neither_a_correction_nor_corrected() {
        let memory = memory_with(human(), None, vec![about(Uuid::new_v4())]).unwrap();

        assert_eq!(memory.supersedes, None);
        assert!(!memory.is_superseded());
    }

    #[test]
    fn a_memory_with_a_successor_reads_as_superseded() {
        let mut memory = memory_with(human(), None, vec![about(Uuid::new_v4())]).unwrap();
        memory.superseded_by = Some(Uuid::new_v4());

        assert!(memory.is_superseded());
    }

    // **The immutability test.** An agent memory relabelled as human-authored
    // destroys the trust model, so `MemoryUpdate` has no authorship field and
    // serde drops the key. Structural rather than validated: there is nothing a
    // request could send that a future handler might forget to reject.
    #[test]
    fn a_patch_body_cannot_carry_authorship() {
        let body = r#"{"content":"revised","authorship":{"kind":"human","userId":"attacker"}}"#;

        let update: MemoryUpdate = serde_json::from_str(body).unwrap();

        assert_eq!(update.content.as_deref(), Some("revised"));
    }

    // The supersession fields are set by the supersede operation, which has to
    // write both sides at once — a client setting one half leaves a dangling
    // chain that reads as history but is not.
    #[test]
    fn a_patch_body_cannot_carry_supersession() {
        let body = format!(
            r#"{{"supersedes":"{}","supersededBy":"{}"}}"#,
            Uuid::new_v4(),
            Uuid::new_v4()
        );

        let update: MemoryUpdate = serde_json::from_str(&body).unwrap();

        assert!(update.content.is_none());
        assert!(update.confidence.is_none());
    }

    // A `null` summary clears it; an absent one leaves it alone. Collapsing the
    // two makes "remove this summary" unexpressible.
    #[test]
    fn a_patch_distinguishes_clearing_a_summary_from_leaving_it() {
        let cleared: MemoryUpdate = serde_json::from_str(r#"{"summary":null}"#).unwrap();
        let untouched: MemoryUpdate = serde_json::from_str("{}").unwrap();

        assert_eq!(cleared.summary, Some(None));
        assert_eq!(untouched.summary, None);
    }

    #[test]
    fn no_change_to_the_subject_reads_as_fresh() {
        assert_eq!(
            staleness(version(1, 3), Some(version(1, 3))),
            Staleness::Fresh
        );
    }

    #[test]
    fn a_major_bump_is_stale() {
        assert_eq!(
            staleness(version(1, 3), Some(version(2, 0))),
            Staleness::Stale {
                since: version(2, 0)
            }
        );
    }

    // Minor is *possibly* stale, not stale. Saying "stale" for an added column
    // trains readers to ignore the flag on the many occasions it does not
    // matter — and then they ignore it on the occasion it does.
    #[test]
    fn a_minor_bump_is_only_possibly_stale() {
        assert_eq!(
            staleness(version(1, 3), Some(version(1, 4))),
            Staleness::PossiblyStale {
                since: version(1, 4)
            }
        );
    }

    // **The ordering test.** A release that bumps both is breaking, so Major has
    // to be checked first. Checking Minor first reports a Major+Minor change as
    // merely possibly stale — the most dangerous wrong answer this function has.
    #[test]
    fn a_bump_to_both_is_stale_not_possibly_stale() {
        assert_eq!(
            staleness(version(1, 3), Some(version(2, 7))),
            Staleness::Stale {
                since: version(2, 7)
            }
        );
    }

    // Versions do not go backwards in this system, but a comparison written as
    // `!=` would report a memory *newer* than its subject as stale — which is
    // an alarm about nothing, on every memory written during a rollback.
    #[test]
    fn a_subject_behind_the_memory_is_not_flagged() {
        assert_eq!(
            staleness(version(2, 0), Some(version(1, 9))),
            Staleness::Fresh
        );
    }

    // Distinct from stale: there is nothing left to compare against, and
    // reporting `Fresh` would be a confident answer about a subject nobody can
    // see.
    #[test]
    fn a_missing_subject_is_not_reported_as_fresh() {
        assert_eq!(staleness(version(1, 0), None), Staleness::SubjectUnknown);
    }

    // **Staleness is recomputed, never stored.** The same memory, unchanged,
    // reads fresh and then stale as its subject moves — which a stored flag
    // gets wrong from the moment somebody edits the table, and gets wrong
    // silently. Expressed as a test because the property is invisible in the
    // type: `Memory` simply has no field to check for.
    #[test]
    fn the_same_memory_changes_verdict_as_its_subject_moves() {
        let written_at = version(1, 2);

        assert_eq!(staleness(written_at, Some(version(1, 2))), Staleness::Fresh);
        assert_eq!(
            staleness(written_at, Some(version(1, 5))),
            Staleness::PossiblyStale {
                since: version(1, 5)
            }
        );
        assert_eq!(
            staleness(written_at, Some(version(3, 0))),
            Staleness::Stale {
                since: version(3, 0)
            }
        );
    }

    // The flag names what changed, so a reader can judge whether it matters to
    // them rather than being told only that something did.
    #[test]
    fn the_flag_carries_the_version_that_caused_it() {
        let Staleness::Stale { since } = staleness(version(1, 0), Some(version(4, 2))) else {
            panic!("expected stale");
        };

        assert_eq!(since, version(4, 2));
    }
}
