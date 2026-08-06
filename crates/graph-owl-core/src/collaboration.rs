//! Collaboration: threads, proposals, announcements, reactions, and the
//! activity feed that merges them with Epic 3's change events — Epic 35.
//!
//! **Threads are not proposals** (decision 2). A thread is an open-ended
//! conversation; a proposal carries a concrete diff and an accept/reject
//! outcome. Conflating them would make "what actually changed" unanswerable
//! from a comment wall.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---- Slice A: Threads and replies ----

/// A conversation anchored to an entity, optionally to one field on it.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// The thread's own id.
    pub id: Uuid,
    /// The entity this thread is about.
    pub about: Uuid,
    /// A field or column FQN this thread is specifically anchored to.
    /// `None` means the thread is about the entity as a whole.
    pub field: Option<String>,
    /// Always the authenticated principal that started it, never
    /// client-supplied.
    pub created_by: String,
    /// When the thread was started.
    pub created_at: DateTime<Utc>,
    /// Whether the question this thread raised has been answered.
    pub resolved: bool,
    /// Who resolved it, if it has been.
    pub resolved_by: Option<String>,
    /// When it was resolved, if it has been.
    pub resolved_at: Option<DateTime<Utc>>,
}

/// One message in a thread. Author is always the authenticated principal —
/// never client-supplied (Slice A's own trust boundary).
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    /// The post's own id.
    pub id: Uuid,
    /// Which thread this post belongs to.
    pub thread_id: Uuid,
    /// Who wrote it — always the authenticated principal.
    pub author: String,
    /// The post's own text.
    pub message: String,
    /// When it was posted.
    pub created_at: DateTime<Utc>,
    /// Set the moment the author edits it — `None` means still as written.
    pub edited_at: Option<DateTime<Utc>>,
    /// A tombstone, not a row removal — deleting must preserve thread
    /// structure (Slice A's own acceptance criteria) rather than leaving a
    /// hole a reply's `thread_id` still points into.
    pub deleted: bool,
}

/// How long after posting an author may still edit their own post. Chosen
/// to be long enough to fix a typo noticed on re-reading and short enough
/// that a much later edit reads as a new post pretending to be the
/// original — no external spec to derive it from, so it is named here
/// rather than left as a bare literal at each call site.
pub const POST_EDIT_WINDOW_MINUTES: i64 = 15;

/// May `actor` edit this post right now?
///
/// **Author only, and only inside the window** — a single `bool` rather
/// than a `Result`, matching decision-shaped predicates elsewhere in this
/// codebase (`glossary::is_attachable`): there is exactly one way to be
/// refused and no detail worth carrying, since the caller already knows
/// who is asking and when the post was made.
#[must_use]
pub fn can_edit_post(
    actor: &str,
    author: &str,
    posted_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    actor == author && now - posted_at <= chrono::Duration::minutes(POST_EDIT_WINDOW_MINUTES)
}

// ---- Slice B: Threads resolve ----

/// May `actor` resolve or reopen this thread?
///
/// **The thread's own author, or an entity owner** (Slice B's own
/// acceptance criteria) — an unrelated reader closing someone else's open
/// question would make "resolved" mean nothing.
#[must_use]
pub fn can_resolve_thread(actor: &str, thread_author: &str, entity_owners: &[String]) -> bool {
    actor == thread_author || entity_owners.iter().any(|owner| owner == actor)
}

// ---- Slice C: Change proposals ----

/// Where a proposal stands.
///
/// Registered in the `OpenAPI` schema as `ChangeProposalStatus` — `graph_owl_authz::agent`
/// already has its own `Proposal`/`ProposalStatus` (an agent's pending action, an
/// unrelated concept that happens to share the English word) and utoipa registers
/// schemas by bare type name, so the default name would silently collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ChangeProposalStatus)]
#[serde(rename_all = "camelCase")]
pub enum ProposalStatus {
    /// Submitted, awaiting a decision.
    Pending,
    /// Decided, and applied — attributed to the proposer (decision 3).
    Accepted,
    /// Decided, and refused — carries a reason.
    Rejected,
}

impl ProposalStatus {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Accepted => "accepted",
            ProposalStatus::Rejected => "rejected",
        }
    }
}

/// A concrete, reviewable diff against one field — decision 2's
/// counterpart to [`Thread`]: not a conversation, a proposed value with an
/// outcome.
///
/// Registered in the `OpenAPI` schema as `ChangeProposal` — see [`ProposalStatus`]'s
/// doc comment for why.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[schema(as = ChangeProposal)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    /// The proposal's own id.
    pub id: Uuid,
    /// The entity this proposal is about.
    pub about: Uuid,
    /// Which field it proposes a new value for.
    pub field: String,
    /// The value at the moment the proposal was made — the staleness
    /// check compares this against the field's actual current value.
    pub current_value: Option<String>,
    /// What the proposer wants it changed to.
    pub proposed_value: Option<String>,
    /// Why — the proposer's own explanation.
    pub rationale: String,
    /// Where this proposal stands.
    pub status: ProposalStatus,
    /// **Never the accepter.** Decision 3: accepting attributes the change
    /// to whoever proposed it, so contribution history is right and a
    /// second contribution from the same person is not discouraged by
    /// having the first one credited to a reviewer instead.
    pub proposed_by: String,
    /// Who decided it, once decided.
    pub decided_by: Option<String>,
    /// When it was decided, once decided.
    pub decided_at: Option<DateTime<Utc>>,
    /// Set on rejection — the same "a decision with no reason teaches
    /// nothing" rule this project already applies to merge review (Epic
    /// 17) and drift (Epic 20).
    pub decision_reason: Option<String>,
    /// When the proposal was made.
    pub created_at: DateTime<Utc>,
}

/// May this proposal still be decided?
///
/// A one-line predicate rather than a transition matrix, unlike
/// [`crate::glossary::can_transition`]'s — a proposal has exactly two
/// terminal states and no path between them, so there is no matrix to get
/// wrong, only a single "has anyone decided this yet".
#[must_use]
pub fn is_decidable(status: ProposalStatus) -> bool {
    matches!(status, ProposalStatus::Pending)
}

// ---- Slice D: Announcements ----

/// A time-boxed notice on an entity — disappears when its window closes
/// rather than being manually retired, because a permanent banner is
/// ignored within a week (decision 5).
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    /// The announcement's own id.
    pub id: Uuid,
    /// The entity it is posted against — visible on descendants too, if
    /// `about` is a container (Slice D's inheritance rule).
    pub about: Uuid,
    /// The notice itself.
    pub message: String,
    /// When it starts showing — inclusive.
    pub starts_at: DateTime<Utc>,
    /// When it stops — exclusive.
    pub ends_at: DateTime<Utc>,
    /// Who created it.
    pub created_by: String,
    /// When it was created — distinct from `starts_at`, which may be in
    /// the future.
    pub created_at: DateTime<Utc>,
}

/// Is this announcement live at `now`?
///
/// **Inclusive start, exclusive end.** An announcement scheduled for
/// "right now" must already be visible — a reviewer should not have to
/// wait a tick after creating one — but one whose window ends *now* must
/// not still be shown, or both edges of the boundary would display it at
/// once.
#[must_use]
pub fn is_active(now: DateTime<Utc>, starts_at: DateTime<Utc>, ends_at: DateTime<Utc>) -> bool {
    now >= starts_at && now < ends_at
}

// ---- Slice E: Reactions ----

/// A fixed, closed set — this project's standing answer to an open-ended
/// vocabulary nobody would ever finish arguing about. Not derived from any
/// external spec; named here once rather than as a bare string at every
/// call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReactionKind {
    /// This post answered the question.
    Helpful,
    /// Agreement, without adding a reply.
    Agree,
    /// Disagreement, without adding a reply.
    Disagree,
    /// This needs a closer look.
    Question,
}

impl ReactionKind {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ReactionKind::Helpful => "helpful",
            ReactionKind::Agree => "agree",
            ReactionKind::Disagree => "disagree",
            ReactionKind::Question => "question",
        }
    }
}

/// What a toggle should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReactionAction {
    /// The reaction did not exist; add it.
    Add,
    /// The reaction already existed; remove it.
    Remove,
}

/// Toggling is the entire reaction model (Slice E's own acceptance
/// criteria): reacting a second time with the same kind removes it rather
/// than duplicating, so "did I already react" needs no count to answer.
///
/// A pure decision over already-known state — whether the row exists is
/// the caller's to check; this only says what should happen next.
#[must_use]
pub fn toggle_reaction(already_reacted: bool) -> ReactionAction {
    if already_reacted {
        ReactionAction::Remove
    } else {
        ReactionAction::Add
    }
}

// ---- Slice F: The activity feed ----

/// One entry the feed can show — Epic 3's own change events, merged with
/// every collaboration event this module adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// An Epic 3 field-level change.
    Change,
    /// A new thread was started.
    ThreadStarted,
    /// A thread was resolved.
    ThreadResolved,
    /// A reply landed on a thread.
    PostAdded,
    /// A change proposal was made.
    ProposalCreated,
    /// A change proposal was accepted or rejected.
    ProposalDecided,
    /// A new announcement started.
    AnnouncementCreated,
}

/// The stable sort key a feed entry orders by.
///
/// **Timestamp first, id second.** Two events sharing one timestamp (an
/// import batch, a fast test) would otherwise order arbitrarily per query
/// — different every call — which breaks pagination: a row seen on page 1
/// could reappear, or vanish, on page 2. The id is an arbitrary tie-break,
/// but a *stable* one, which is the only property pagination actually
/// needs.
#[must_use]
pub fn activity_sort_key(occurred_at: DateTime<Utc>, id: Uuid) -> (DateTime<Utc>, Uuid) {
    (occurred_at, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    // ---- can_edit_post (Slice A) ----

    #[test]
    fn the_author_may_edit_inside_the_window() {
        assert!(can_edit_post("alice", "alice", at(0), at(60)));
    }

    #[test]
    fn someone_else_may_never_edit_even_inside_the_window() {
        assert!(!can_edit_post("mallory", "alice", at(0), at(60)));
    }

    #[test]
    fn the_author_may_not_edit_past_the_window() {
        let just_past = at(0)
            + chrono::Duration::minutes(POST_EDIT_WINDOW_MINUTES)
            + chrono::Duration::seconds(1);
        assert!(!can_edit_post("alice", "alice", at(0), just_past));
    }

    // The boundary itself is inside the window — otherwise a click landing
    // on the exact minute would be refused for reasons no user could see.
    #[test]
    fn exactly_at_the_window_boundary_is_still_editable() {
        let boundary = at(0) + chrono::Duration::minutes(POST_EDIT_WINDOW_MINUTES);
        assert!(can_edit_post("alice", "alice", at(0), boundary));
    }

    // ---- can_resolve_thread (Slice B) ----

    #[test]
    fn the_thread_author_may_resolve_it() {
        assert!(can_resolve_thread("alice", "alice", &[]));
    }

    #[test]
    fn an_entity_owner_may_resolve_someone_elses_thread() {
        assert!(can_resolve_thread(
            "bob",
            "alice",
            &["bob".to_string(), "carol".to_string()]
        ));
    }

    // The negative that makes the two positives mean something: neither
    // the author nor an owner must default to permitted.
    #[test]
    fn an_unrelated_user_may_not_resolve_it() {
        assert!(!can_resolve_thread(
            "mallory",
            "alice",
            &["bob".to_string()]
        ));
    }

    // ---- is_decidable (Slice C) ----

    #[test]
    fn only_pending_is_decidable() {
        assert!(is_decidable(ProposalStatus::Pending));
        assert!(!is_decidable(ProposalStatus::Accepted));
        assert!(!is_decidable(ProposalStatus::Rejected));
    }

    #[test]
    fn proposal_status_wire_names_are_exact() {
        assert_eq!(ProposalStatus::Pending.as_str(), "pending");
        assert_eq!(ProposalStatus::Accepted.as_str(), "accepted");
        assert_eq!(ProposalStatus::Rejected.as_str(), "rejected");
    }

    // ---- is_active (Slice D) ----

    #[test]
    fn between_start_and_end_is_active() {
        assert!(is_active(at(50), at(0), at(100)));
    }

    #[test]
    fn before_start_is_not_active() {
        assert!(!is_active(at(0), at(50), at(100)));
    }

    // Inclusive start: exactly at the start must already be visible.
    #[test]
    fn exactly_at_start_is_active() {
        assert!(is_active(at(0), at(0), at(100)));
    }

    // Exclusive end: exactly at the end must no longer be visible — the
    // mutant this guards against is flipping `<` to `<=`, which would show
    // the announcement on both sides of a boundary shared with a
    // following one.
    #[test]
    fn exactly_at_end_is_not_active() {
        assert!(!is_active(at(100), at(0), at(100)));
    }

    #[test]
    fn after_end_is_not_active() {
        assert!(!is_active(at(200), at(0), at(100)));
    }

    // ---- toggle_reaction (Slice E) ----

    #[test]
    fn reacting_when_absent_adds_it() {
        assert_eq!(toggle_reaction(false), ReactionAction::Add);
    }

    #[test]
    fn reacting_again_when_present_removes_it() {
        assert_eq!(toggle_reaction(true), ReactionAction::Remove);
    }

    #[test]
    fn reaction_kind_wire_names_are_exact() {
        assert_eq!(ReactionKind::Helpful.as_str(), "helpful");
        assert_eq!(ReactionKind::Agree.as_str(), "agree");
        assert_eq!(ReactionKind::Disagree.as_str(), "disagree");
        assert_eq!(ReactionKind::Question.as_str(), "question");
    }

    // ---- activity_sort_key (Slice F) ----

    #[test]
    fn later_timestamps_sort_after_earlier_ones_regardless_of_id() {
        let low_id = Uuid::from_u128(1);
        let high_id = Uuid::from_u128(2);
        let earlier = activity_sort_key(at(0), high_id);
        let later = activity_sort_key(at(1), low_id);
        assert!(earlier < later, "timestamp must dominate the id tie-break");
    }

    #[test]
    fn equal_timestamps_break_ties_by_id() {
        let low_id = Uuid::from_u128(1);
        let high_id = Uuid::from_u128(2);
        assert!(activity_sort_key(at(0), low_id) < activity_sort_key(at(0), high_id));
    }
}
