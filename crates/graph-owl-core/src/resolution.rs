//! Entity resolution's storage-facing types — Epic 17 Slices D and E.
//!
//! Plain data, not logic: the pure decision (`graph_owl_resolution::bands`)
//! and the scorer (`graph_owl_resolution::score`) already live in their own
//! crate with no I/O. What is here is the shape a merge takes once *some*
//! layer decides to write one, so `graph-owl-storage`'s trait and both its
//! adapters can agree on it without a cycle back to `graph-owl-resolution`
//! (which itself depends on `graph-owl-storage`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Why two entities were judged the same. Kept on the record rather than
/// only the final score, so a wrong merge is diagnosable — "these matched on
/// `SameParent` and a `0.61` name similarity" is checkable by a steward;
/// "these scored `0.61`" is not.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Evidence {
    /// The two entities' fully qualified names are identical.
    ExactFqn,
    /// The two entities' fully qualified names are identical once normalized
    /// (case, punctuation).
    NormalizedFqn,
    /// The two entities have the same name within a shared scope.
    ExactName {
        /// The scope the names were compared within.
        scope: String,
    },
    /// The two entities' names scored above a similarity threshold.
    NameSimilarity {
        /// The similarity metric used.
        metric: String,
        /// The score the metric produced.
        value: f64,
    },
    // Explicit per-field `rename` rather than the enum-level
    // `rename_all_fields`: utoipa 5's schema derive does not read that
    // attribute (unlike serde itself, which does — the reason this needs a
    // test asserting the actual serialized bytes, not just a round trip),
    // so the *documented* schema would silently keep the snake_case name
    // while the wire format changed underneath it.
    /// The two entities share enough structure (columns, in practice) to be
    /// judged the same.
    StructuralOverlap {
        /// How many columns the two entities share.
        #[serde(rename = "sharedColumns")]
        shared_columns: usize,
        /// The total column count the overlap was measured against.
        total: usize,
    },
    /// The two entities share a parent.
    SameParent,
    /// The two entities came from the same source system.
    SameSourceSystem,
}

/// Who — or what — decided a merge.
///
/// Deliberately not the shared [`crate::memory::Authorship`] type: that one
/// models a memory's author and has no notion of an automated decision,
/// while a merge's most common decider *is* the auto-merge band itself. A
/// third variant bolted onto `Authorship` for this one caller would make
/// every memory-authoring call site pattern-match a case that can never
/// apply to it.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MergeDecidedBy {
    /// The confidence-band decision itself, unattended.
    Auto,
    /// A person, reviewing the review queue.
    Human {
        /// The deciding user's id.
        #[serde(rename = "userId")]
        user_id: String,
    },
    /// An automated agent.
    Agent {
        /// The deciding agent's id.
        #[serde(rename = "agentId")]
        agent_id: String,
        /// The model the agent ran on.
        model: String,
    },
}

/// The reversibility contract (decision 1, `17-entity-resolution.md`): a
/// merge is a record, not a destructive rewrite. `split_at` turns it from a
/// live merge into a historical one without deleting the evidence that
/// produced it.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRecord {
    /// The stable identifier of this merge record.
    pub id: Uuid,
    /// The entity that survives the merge.
    pub canonical: Uuid,
    /// The entity merged into the canonical one.
    pub merged: Uuid,
    /// Why the two were judged the same.
    pub evidence: Vec<Evidence>,
    /// The scorer's confidence in the merge.
    pub confidence: f64,
    /// Who or what decided it.
    pub decided_by: MergeDecidedBy,
    /// When the merge was decided.
    pub decided_at: DateTime<Utc>,
    /// The engine transaction time the merge itself wrote at — internal
    /// bookkeeping, not part of the plan's illustrative sketch, but what
    /// makes a split able to restore exactly the state `merged_at_t - 1`
    /// (the moment just before the merge's own retraction) held, via
    /// time-travel, rather than reconstructing it from wall-clock time
    /// against a transaction log whose granularity was never meant to
    /// answer that question precisely.
    pub merged_at_t: i64,
    /// When the merge was reversed, if it has been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_at: Option<DateTime<Utc>>,
}

/// One candidate, scored, with the evidence that produced the score — what
/// a review queue displays and what an `Ambiguous` resolution carries so a
/// steward sees why each candidate was proposed, not only that it was.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The candidate entity.
    pub entity: Uuid,
    /// The candidate's fully qualified name.
    pub fqn: String,
    /// The scorer's confidence that this candidate is the same entity.
    pub score: f64,
    /// Why the scorer reached that confidence.
    pub evidence: Vec<Evidence>,
}

/// What resolving one entity against its candidates decided.
///
/// Three variants, matching the three confidence bands exactly — `New` and
/// `Ambiguous` both leave the caller with **no** write to make, which is
/// the property Slice D's acceptance criteria are strictest about: an
/// `Ambiguous` that quietly created anything would defeat the review band's
/// entire purpose.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Resolution {
    /// No candidate scored high enough to act on — including no candidates
    /// at all.
    New,
    /// Merged (auto-merge band) or already the same entity. `entity` is the
    /// canonical id after the merge.
    Existing {
        /// The canonical id after the merge.
        entity: Uuid,
        /// The scorer's confidence in the merge.
        confidence: f64,
    },
    /// Review band: nothing merged, nothing created, and here is what a
    /// human would need to decide.
    Ambiguous {
        /// The candidates a human would choose between.
        candidates: Vec<Candidate>,
    },
}

/// A review-queue entry's state — Epic 17 Slice F.
///
/// `Confirmed`/`Rejected` are terminal: once decided, [`crate::resolution`]'s
/// storage contract keeps the entry exactly as decided rather than letting a
/// later re-resolution reset it to `Pending` — that persistence is the whole
/// point of a rejection ("the queue is not the same computation replayed
/// forever").
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewStatus {
    /// Not yet decided.
    Pending,
    /// A human or agent confirmed the merge.
    Confirmed,
    /// A human or agent rejected the merge.
    Rejected,
}

/// One candidate pair sitting in the review queue.
///
/// A separate, persisted record from [`Candidate`] (which is computed fresh
/// every `resolve_asset` call and never stored) — `ReviewQueueEntry` is what
/// makes a rejection outlive the request that produced it.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQueueEntry {
    /// The stable identifier of this queue entry.
    pub id: Uuid,
    /// The entity being resolved.
    pub target: Uuid,
    /// The candidate it might be the same as.
    pub candidate: Uuid,
    /// The scorer's confidence in the match.
    pub score: f64,
    /// Why the scorer reached that confidence.
    pub evidence: Vec<Evidence>,
    /// Where the entry stands.
    pub status: ReviewStatus,
    /// Who or what decided it, once decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<MergeDecidedBy>,
    /// When it was decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<DateTime<Utc>>,
    /// Why a rejection was rejected — required on reject (Epic 42 decision
    /// 3: an accepted merge with no evidence and a rejection with no reason
    /// teach the matcher nothing), absent on confirm and while pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When the entry was queued.
    pub created_at: DateTime<Utc>,
}

/// A textual reference to an entity — "the orders table", found in a memory,
/// a runbook, or a connector's free-text description — Epic 17 Slice G.
///
/// Distinct from a resolution [`Candidate`]: a mention is unresolved text,
/// not yet known to be a specific entity at all.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMention {
    /// The mention's own text, verbatim from its source.
    pub text: String,
    /// The kind of entity the mention is expected to resolve to, if known —
    /// narrows the candidate search when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_type: Option<crate::AssetKind>,
    /// Surrounding text — "in staging", "in prod" — that disambiguates
    /// between same-named entities the bare text alone cannot.
    #[serde(default)]
    pub context: String,
}

/// What resolving a mention decided, and the record of it.
///
/// **Never a merge.** A mention links a piece of text to an entity; it does
/// not assert that two entities are the same one, which is the whole reason
/// this is its own record rather than reusing [`MergeRecord`] — conflating
/// the two would let a passing reference to a table silently merge it with
/// something else.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MentionResolution {
    /// The stable identifier of this resolution.
    pub id: Uuid,
    /// What the mention was found in — a memory, most commonly. Not a
    /// foreign key to any one table: a mention's source is whatever kind of
    /// document it was extracted from, and constraining it to one entity
    /// type would make this reusable only for that type.
    pub source: Uuid,
    /// The mention's own text, verbatim from its source.
    pub text: String,
    /// The entity the mention was resolved to.
    pub entity: Uuid,
    /// The resolver's confidence in the match.
    pub confidence: f64,
    /// When the mention was resolved.
    pub resolved_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#[serde(rename_all)]` on an enum renames the *variants*, not their
    /// fields — `rename_all_fields` is the separate attribute for that, and
    /// a type whose wire shape matters needs an assertion against the actual
    /// serialized bytes, not just a round trip, or a field going out in
    /// snake_case passes silently.
    #[test]
    fn struct_variant_fields_serialize_as_camel_case() {
        let evidence = Evidence::StructuralOverlap {
            shared_columns: 1,
            total: 2,
        };
        let json = serde_json::to_value(&evidence).expect("serialize");
        assert_eq!(json["sharedColumns"], 1);
        assert_eq!(json["total"], 2);
        assert!(json.get("shared_columns").is_none());

        let decided_by = MergeDecidedBy::Human {
            user_id: "alice".to_string(),
        };
        let json = serde_json::to_value(&decided_by).expect("serialize");
        assert_eq!(json["userId"], "alice");
        assert!(json.get("user_id").is_none());
    }
}
