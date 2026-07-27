//! The entity envelope: version, attribution, and the field-level diff.
//!
//! Applied to five asset kinds now; twenty-five later. That ratio is the whole
//! reason it comes early — `ROADMAP.md`'s retrofit-cost argument.

use serde::{Deserialize, Serialize};
use std::fmt;

/// `Major.Minor`, starting at `0.1`.
///
/// Minor for backward-compatible change (description, tags, owners); Major for
/// breaking change (a column dropped, a type changed, a rename). The split
/// matters because a consumer can subscribe to Major alone and get exactly the
/// changes that could break it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityVersion {
    pub major: u32,
    pub minor: u32,
}

impl EntityVersion {
    /// The version every entity is created at.
    #[must_use]
    pub const fn initial() -> Self {
        Self { major: 0, minor: 1 }
    }

    #[must_use]
    pub const fn next_minor(self) -> Self {
        Self {
            major: self.major,
            minor: self.minor + 1,
        }
    }

    /// Major resets minor to zero: `0.7` → `1.0`, not `1.7`. Carrying the minor
    /// across would make version ordering lie about how much changed.
    #[must_use]
    pub const fn next_major(self) -> Self {
        Self {
            major: self.major + 1,
            minor: 0,
        }
    }

    #[must_use]
    pub fn bump(self, kind: ChangeKind) -> Self {
        match kind {
            ChangeKind::None => self,
            ChangeKind::Minor => self.next_minor(),
            ChangeKind::Major => self.next_major(),
        }
    }

    /// # Errors
    ///
    /// Returns `()` for anything that is not `<u32>.<u32>`.
    pub fn parse(value: &str) -> Result<Self, ()> {
        let (major, minor) = value.split_once('.').ok_or(())?;
        Ok(Self {
            major: major.parse().map_err(|_| ())?,
            minor: minor.parse().map_err(|_| ())?,
        })
    }
}

impl fmt::Display for EntityVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// How much a change is worth. `None` is a real outcome, not an absence: a
/// connector re-running against an unchanged source must produce no version and
/// no event, and that is what makes its idempotency observable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    None,
    Minor,
    Major,
}

/// One field's before and after. `None` on either side means the field was
/// added or removed — distinct from changed, because a consumer reacts
/// differently to a dropped column than to a retyped one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

/// Computed server-side by diffing before against after — never supplied by the
/// client. This is why PATCH stays DTO-shaped rather than JSON Patch: a state
/// diff describes *effect*, a patch document describes *intent*, and an audit
/// trail wants effect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDescription {
    pub fields_added: Vec<FieldChange>,
    pub fields_updated: Vec<FieldChange>,
    pub fields_deleted: Vec<FieldChange>,
}

impl ChangeDescription {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields_added.is_empty()
            && self.fields_updated.is_empty()
            && self.fields_deleted.is_empty()
    }

    /// Diffs two JSON objects field by field.
    ///
    /// Only top-level fields, deliberately: a nested diff produces paths nobody
    /// reads and a change count nobody trusts. Nested structures are compared
    /// whole and reported as one updated field.
    #[must_use]
    pub fn between(before: &serde_json::Value, after: &serde_json::Value) -> Self {
        let mut description = Self::default();
        let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
            return description;
        };

        for (field, after_value) in after {
            match before.get(field) {
                None | Some(serde_json::Value::Null) if !after_value.is_null() => {
                    description.fields_added.push(FieldChange {
                        field: field.clone(),
                        before: None,
                        after: Some(after_value.clone()),
                    });
                }
                Some(before_value) if before_value != after_value && !after_value.is_null() => {
                    description.fields_updated.push(FieldChange {
                        field: field.clone(),
                        before: Some(before_value.clone()),
                        after: Some(after_value.clone()),
                    });
                }
                _ => {}
            }
        }

        for (field, before_value) in before {
            let removed = match after.get(field) {
                None => !before_value.is_null(),
                Some(serde_json::Value::Null) => !before_value.is_null(),
                Some(_) => false,
            };
            if removed {
                description.fields_deleted.push(FieldChange {
                    field: field.clone(),
                    before: Some(before_value.clone()),
                    after: None,
                });
            }
        }

        description
    }
}

/// Fields whose change is breaking. Everything else is Minor.
const BREAKING_FIELDS: [&str; 3] = ["name", "fullyQualifiedName", "dataType"];

/// Classifies a diff. A removal is always breaking — a consumer reading a field
/// that vanished breaks regardless of which field it was.
#[must_use]
pub fn classify(description: &ChangeDescription) -> ChangeKind {
    if description.is_empty() {
        return ChangeKind::None;
    }
    if !description.fields_deleted.is_empty() {
        return ChangeKind::Major;
    }
    if description
        .fields_updated
        .iter()
        .any(|change| BREAKING_FIELDS.contains(&change.field.as_str()))
    {
        return ChangeKind::Major;
    }
    ChangeKind::Minor
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entities_start_at_zero_one() {
        assert_eq!(EntityVersion::initial().to_string(), "0.1");
    }

    #[test]
    fn a_minor_bump_advances_by_exactly_one() {
        // Every other version test asserted ordering or resets; none asserted
        // that a minor bump actually advances, which left the arithmetic
        // itself untested.
        assert_eq!(EntityVersion::initial().next_minor().to_string(), "0.2");
        assert_eq!(
            EntityVersion::initial()
                .next_minor()
                .next_minor()
                .to_string(),
            "0.3"
        );
    }

    #[test]
    fn a_major_bump_resets_the_minor() {
        let version = EntityVersion { major: 0, minor: 7 };
        assert_eq!(version.next_major().to_string(), "1.0");
    }

    #[test]
    fn versions_order_by_major_then_minor() {
        let mut versions = vec![
            EntityVersion { major: 1, minor: 0 },
            EntityVersion { major: 0, minor: 9 },
            EntityVersion {
                major: 0,
                minor: 10,
            },
        ];
        versions.sort();
        assert_eq!(
            versions.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["0.9", "0.10", "1.0"],
            "0.10 is after 0.9 — numeric, not lexicographic"
        );
    }

    #[test]
    fn a_version_round_trips_through_its_string_form() {
        for version in [
            EntityVersion::initial(),
            EntityVersion {
                major: 12,
                minor: 34,
            },
        ] {
            assert_eq!(EntityVersion::parse(&version.to_string()), Ok(version));
        }
    }

    #[test]
    fn a_malformed_version_is_rejected() {
        for bad in ["1", "1.2.3", "x.1", "1.y", ""] {
            assert!(EntityVersion::parse(bad).is_err(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn a_no_op_change_produces_no_version_bump() {
        // The property that makes connector idempotency observable: a nightly
        // re-run over an unchanged source must not inflate history.
        let version = EntityVersion::initial();
        assert_eq!(version.bump(ChangeKind::None), version);
    }

    #[test]
    fn an_identical_document_diffs_to_nothing() {
        let doc = json!({ "name": "orders", "description": "the orders table" });
        assert!(ChangeDescription::between(&doc, &doc).is_empty());
    }

    #[test]
    fn a_changed_field_is_updated_and_carries_both_sides() {
        let before = json!({ "description": "old" });
        let after = json!({ "description": "new" });
        let diff = ChangeDescription::between(&before, &after);

        assert_eq!(diff.fields_updated.len(), 1);
        assert_eq!(diff.fields_updated[0].field, "description");
        assert_eq!(diff.fields_updated[0].before, Some(json!("old")));
        assert_eq!(
            diff.fields_updated[0].after,
            Some(json!("new")),
            "an audit trail without the previous value cannot answer 'what did it say before'"
        );
    }

    #[test]
    fn a_new_field_is_added_not_updated() {
        let diff = ChangeDescription::between(&json!({}), &json!({ "description": "x" }));
        assert_eq!(diff.fields_added.len(), 1);
        assert!(diff.fields_updated.is_empty());
    }

    #[test]
    fn a_removed_field_is_deleted_not_updated() {
        // Distinct from updated because a consumer reacts differently to a
        // dropped column than to a retyped one.
        let diff = ChangeDescription::between(&json!({ "description": "x" }), &json!({}));
        assert_eq!(diff.fields_deleted.len(), 1);
        assert_eq!(diff.fields_deleted[0].before, Some(json!("x")));
        assert!(diff.fields_updated.is_empty());
    }

    #[test]
    fn a_field_that_was_absent_and_arrives_null_is_not_an_addition() {
        // Nothing was added: the field is still, in effect, absent. Counting it
        // would put a phantom entry in the audit trail and bump a version for
        // a change nobody made.
        let diff = ChangeDescription::between(&json!({}), &json!({ "description": null }));
        assert!(
            diff.is_empty(),
            "absent -> null is not a change, got {diff:?}"
        );
    }

    #[test]
    fn nulling_a_field_counts_as_deleting_it() {
        let diff = ChangeDescription::between(
            &json!({ "description": "x" }),
            &json!({ "description": null }),
        );
        assert_eq!(
            diff.fields_deleted.len(),
            1,
            "explicit null is how a client clears a field; it is a removal, not an update"
        );
    }

    #[test]
    fn several_changes_accumulate_across_categories() {
        let before = json!({ "description": "old", "owner": "alice" });
        let after = json!({ "description": "new", "tier": "gold" });
        let diff = ChangeDescription::between(&before, &after);
        assert_eq!(diff.fields_updated.len(), 1, "description");
        assert_eq!(diff.fields_added.len(), 1, "tier");
        assert_eq!(diff.fields_deleted.len(), 1, "owner");
    }

    #[test]
    fn a_description_edit_is_minor() {
        let diff = ChangeDescription::between(
            &json!({ "description": "old" }),
            &json!({ "description": "new" }),
        );
        assert_eq!(classify(&diff), ChangeKind::Minor);
    }

    #[test]
    fn a_rename_is_major() {
        let diff = ChangeDescription::between(&json!({ "name": "a" }), &json!({ "name": "b" }));
        assert_eq!(
            classify(&diff),
            ChangeKind::Major,
            "a rename changes the address every consumer holds"
        );
    }

    #[test]
    fn any_removal_is_major_whichever_field_it_was() {
        // A consumer reading a field that vanished breaks regardless of which
        // field it was, so removal does not consult the breaking-field list.
        let diff = ChangeDescription::between(&json!({ "trivial": "x" }), &json!({}));
        assert_eq!(classify(&diff), ChangeKind::Major);
    }

    #[test]
    fn an_empty_diff_classifies_as_no_change() {
        assert_eq!(classify(&ChangeDescription::default()), ChangeKind::None);
    }

    #[test]
    fn a_type_change_on_a_column_is_major() {
        let diff = ChangeDescription::between(
            &json!({ "dataType": "int" }),
            &json!({ "dataType": "bigint" }),
        );
        assert_eq!(classify(&diff), ChangeKind::Major);
    }

    #[test]
    fn diffing_non_objects_is_empty_rather_than_a_panic() {
        assert!(ChangeDescription::between(&json!("a"), &json!("b")).is_empty());
        assert!(ChangeDescription::between(&json!(null), &json!([1])).is_empty());
    }
}
