//! Tags and classifications — Epic 25.
//!
//! **Two vocabularies, different jobs** (decision 1). `Classification → Tag` is
//! flat and operational: `PII.Sensitive`, `Tier.Gold` — how do I *handle* this.
//! `Glossary → GlossaryTerm` (Epic 24) is hierarchical and semantic, with
//! synonyms and a review workflow — what does this *mean*. Collapsing them into
//! one tree conflates the two questions, and the answer to one is never the
//! answer to the other.
//!
//! Everything here is pure. Whether a tag exists, and what is already applied
//! to an entity, are questions for storage; what a label *means* and which
//! labels may coexist are decided here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::envelope::{ChangeDescription, EntityVersion};

/// Where a label came from.
///
/// **Carried from day one** (decision 2), before any scanner exists. An
/// automated classifier must be able to *suggest* `PII.Sensitive` without a
/// human having confirmed it, and a model that cannot express provenance forces
/// a rewrite the moment automation arrives — by which time there is labelled
/// data to migrate.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LabelType {
    /// A human applied it directly.
    Manual,
    /// It was pushed down from a parent by an explicit propagate action.
    Propagated,
    /// A scanner proposed it.
    Automated,
    /// Reasoning concluded it.
    Derived,
}

impl LabelType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LabelType::Manual => "manual",
            LabelType::Propagated => "propagated",
            LabelType::Automated => "automated",
            LabelType::Derived => "derived",
        }
    }

    #[must_use]
    pub const fn all() -> &'static [LabelType] {
        &[
            LabelType::Manual,
            LabelType::Propagated,
            LabelType::Automated,
            LabelType::Derived,
        ]
    }

    /// # Errors
    ///
    /// The unrecognised name, so the caller can name it back.
    pub fn parse(raw: &str) -> Result<Self, String> {
        LabelType::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == raw)
            .ok_or_else(|| raw.to_string())
    }
}

/// Whether a human has vouched for a label.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LabelState {
    /// Proposed, awaiting triage. Counts for **nothing** governance-wise until
    /// confirmed — a suggestion treated as a fact is how a scanner's false
    /// positive becomes a compliance claim.
    Suggested,
    Confirmed,
}

impl LabelState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LabelState::Suggested => "suggested",
            LabelState::Confirmed => "confirmed",
        }
    }

    /// # Errors
    ///
    /// The unrecognised name.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "suggested" => Ok(LabelState::Suggested),
            "confirmed" => Ok(LabelState::Confirmed),
            other => Err(other.to_string()),
        }
    }
}

/// An operational vocabulary: `PII`, `Tier`, `Retention`.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// **Decision 4.** A `Tier` classification marked exclusive refuses a second
    /// `Tier.*` on the same entity — otherwise "Tier" means nothing, because an
    /// entity could be Gold and Bronze at once and no consumer could act on
    /// either.
    pub mutually_exclusive: bool,
    pub version: EntityVersion,
    pub updated_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One label within a classification.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    pub classification_id: Uuid,
    /// `{classification}.{tag}` — derived, never client-set, for the same
    /// reason every other FQN in this catalog is.
    pub fully_qualified_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: EntityVersion,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A tag applied to something, with where it came from.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagLabel {
    pub tag_fqn: String,
    /// What it is on. An **FQN rather than an id**, because a column is
    /// addressed by FQN — the same choice Epic 24's `term_attachments` made,
    /// and for the same reason.
    pub target_fqn: String,
    pub label_type: LabelType,
    pub state: LabelState,
    pub applied_by: String,
    pub applied_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_by: Option<String>,
}

/// The tag FQN for a name under a classification.
#[must_use]
pub fn tag_fqn(classification: &str, tag: &str) -> String {
    format!("{classification}.{tag}")
}

/// The classification half of a tag FQN.
///
/// `None` when the FQN has no dot, which is not a tag FQN at all. Splitting on
/// the **first** dot: a tag name may not contain one (see [`validate_tag_name`]),
/// so everything after the first dot is the tag.
#[must_use]
pub fn classification_of(tag_fqn: &str) -> Option<&str> {
    tag_fqn
        .split_once('.')
        .map(|(classification, _)| classification)
}

/// Whether a classification or tag name can exist.
///
/// A dot would make `{classification}.{tag}` ambiguous — a tag literally named
/// `Sensitive.Extra` under `PII` produces `PII.Sensitive.Extra`, which is
/// indistinguishable from a tag `Extra` under a classification `PII.Sensitive`.
///
/// # Errors
///
/// A sentence naming what is wrong, ready to be a `400` detail.
pub fn validate_tag_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("a name is required".to_string());
    }
    if name.contains('.') {
        return Err(format!(
            "`{name}` cannot contain a dot: a tag is addressed as \
             `{{classification}}.{{tag}}`, so a dotted name would be \
             indistinguishable from a different classification"
        ));
    }
    Ok(())
}

/// The label that blocks applying `candidate`, if any.
///
/// **Only within one classification, and only when it is exclusive.** Checking
/// across classifications would refuse `Tier.Gold` beside `PII.Sensitive`,
/// which is the normal case and the whole point of having more than one
/// vocabulary. Checking nothing makes decision 4 a comment.
///
/// A label for the *same* tag is not a conflict — that is idempotent
/// re-application, handled by the caller as a no-op rather than a `409`.
#[must_use]
pub fn conflicting_label<'a>(
    existing: &'a [TagLabel],
    candidate_fqn: &str,
    classification_is_exclusive: bool,
) -> Option<&'a TagLabel> {
    if !classification_is_exclusive {
        return None;
    }
    let candidate_classification = classification_of(candidate_fqn)?;
    existing.iter().find(|label| {
        label.tag_fqn != candidate_fqn
            && classification_of(&label.tag_fqn) == Some(candidate_classification)
    })
}

/// Whether a propagated label may overwrite what is already there.
///
/// **A manual label is never downgraded.** A steward who deliberately tagged
/// one column and then propagated the table's tag must not silently lose the
/// deliberate choice — and "propagated" would be a lie about where that label
/// came from. Everything else is replaceable, because it was machinery to begin
/// with.
#[must_use]
pub fn propagation_may_overwrite(existing: Option<&TagLabel>) -> bool {
    match existing {
        None => true,
        Some(label) => label.label_type != LabelType::Manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(tag_fqn: &str, label_type: LabelType) -> TagLabel {
        TagLabel {
            tag_fqn: tag_fqn.to_string(),
            target_fqn: "svc.db.public.orders".to_string(),
            label_type,
            state: LabelState::Confirmed,
            applied_by: "asha".to_string(),
            applied_at: Utc::now(),
            confirmed_by: None,
        }
    }

    #[test]
    fn a_tag_fqn_is_its_classification_and_name() {
        assert_eq!(tag_fqn("PII", "Sensitive"), "PII.Sensitive");
        assert_eq!(classification_of("PII.Sensitive"), Some("PII"));
    }

    /// A string with no dot is not a tag FQN, and reading it as a
    /// classification with an empty tag would make every bare word collide.
    #[test]
    fn a_string_with_no_dot_has_no_classification() {
        assert_eq!(classification_of("PII"), None);
    }

    #[test]
    fn a_dotted_name_is_refused_and_says_why() {
        let error = validate_tag_name("Sensitive.Extra").expect_err("a dot is refused");
        assert!(error.contains("dot"), "{error}");
    }

    #[test]
    fn an_ordinary_name_is_accepted() {
        assert!(validate_tag_name("Sensitive").is_ok());
        assert!(validate_tag_name("Tier-1").is_ok());
        assert!(validate_tag_name("").is_err());
    }

    // ---- exclusivity (decision 4) ----

    /// **The core of decision 4.** Two tags from one exclusive classification
    /// means the classification says nothing: an entity that is Gold and Bronze
    /// at once cannot be acted on by either.
    #[test]
    fn a_second_tag_from_an_exclusive_classification_conflicts() {
        let existing = vec![label("Tier.Gold", LabelType::Manual)];

        let blocker = conflicting_label(&existing, "Tier.Bronze", true)
            .expect("a second Tier tag must conflict");

        assert_eq!(blocker.tag_fqn, "Tier.Gold", "and it names which one");
    }

    /// **The negative that makes exclusivity a rule rather than a ban.** A tag
    /// from a *different* classification is the normal case — `PII.Sensitive`
    /// beside `Tier.Gold` is exactly why there is more than one vocabulary.
    #[test]
    fn a_tag_from_a_different_classification_never_conflicts() {
        let existing = vec![label("Tier.Gold", LabelType::Manual)];

        assert!(conflicting_label(&existing, "PII.Sensitive", true).is_none());
    }

    /// And a non-exclusive classification permits several, which is what
    /// `PII.Sensitive` + `PII.Restricted` needs.
    #[test]
    fn a_non_exclusive_classification_permits_several_of_its_tags() {
        let existing = vec![label("PII.Sensitive", LabelType::Manual)];

        assert!(conflicting_label(&existing, "PII.Restricted", false).is_none());
    }

    /// Re-applying the **same** tag is idempotence, not a conflict — a `409`
    /// here would make every retry fail.
    #[test]
    fn re_applying_the_same_tag_is_not_a_conflict() {
        let existing = vec![label("Tier.Gold", LabelType::Manual)];

        assert!(conflicting_label(&existing, "Tier.Gold", true).is_none());
    }

    #[test]
    fn nothing_conflicts_with_an_entity_that_has_no_labels() {
        assert!(conflicting_label(&[], "Tier.Gold", true).is_none());
    }

    // ---- propagation precedence ----

    /// **The sharp one.** A steward's deliberate label survives a propagate,
    /// and calling it `Propagated` afterwards would also be a lie about where
    /// it came from.
    #[test]
    fn propagation_never_overwrites_a_manual_label() {
        assert!(!propagation_may_overwrite(Some(&label(
            "PII.Sensitive",
            LabelType::Manual
        ))));
    }

    /// And the negative: everything else is machinery and may be replaced, or
    /// propagation could never update anything it had itself created.
    #[test]
    fn propagation_overwrites_its_own_labels_and_absent_ones() {
        assert!(propagation_may_overwrite(None));
        assert!(propagation_may_overwrite(Some(&label(
            "PII.Sensitive",
            LabelType::Propagated
        ))));
        assert!(propagation_may_overwrite(Some(&label(
            "PII.Sensitive",
            LabelType::Automated
        ))));
        assert!(propagation_may_overwrite(Some(&label(
            "PII.Sensitive",
            LabelType::Derived
        ))));
    }

    // ---- wire shapes ----

    /// **The round trip was asymmetric, and mutation testing found it.**
    /// `LabelType` was checked in both directions; `LabelState` only through
    /// `parse`, so nothing asserted what `as_str` actually produces — and
    /// mutating it to `""` survived. That string is not cosmetic: it is written
    /// straight into a column with `CHECK (state IN ('suggested','confirmed'))`,
    /// so a wrong one turns every label write into a constraint violation.
    #[test]
    fn label_types_and_states_round_trip_through_their_wire_names() {
        for kind in LabelType::all() {
            assert_eq!(LabelType::parse(kind.as_str()), Ok(*kind));
        }
        assert!(LabelType::parse("guessed").is_err());

        for state in [LabelState::Suggested, LabelState::Confirmed] {
            assert_eq!(LabelState::parse(state.as_str()), Ok(state));
        }
        // Pinned to the literals, because these are the two values the
        // database's own `CHECK` accepts — a round trip alone would pass with
        // both halves renamed in step.
        assert_eq!(LabelState::Suggested.as_str(), "suggested");
        assert_eq!(LabelState::Confirmed.as_str(), "confirmed");
        assert!(LabelState::parse("maybe").is_err());
    }

    #[test]
    fn a_label_is_camel_case_on_the_wire() {
        let json =
            serde_json::to_value(label("PII.Sensitive", LabelType::Manual)).expect("serialize");

        assert!(json.get("tagFqn").is_some(), "{json}");
        assert!(json.get("targetFqn").is_some(), "{json}");
        assert!(json.get("labelType").is_some(), "{json}");
        assert!(json.get("tag_fqn").is_none(), "{json}");
        assert_eq!(json["labelType"], "manual");
        assert_eq!(json["state"], "confirmed");
    }
}
