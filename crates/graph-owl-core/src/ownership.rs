//! Who owns a thing — Epic 11 Slice C.
//!
//! `00c-domain-model.md`: "`owners` is a list of `EntityReference` pointing at
//! `User` or `Team`. **Single-owner models fail immediately** — every real asset
//! has a producing team and an accountable individual." So this is a list, and it
//! is a list of *both* kinds.

use serde::{Deserialize, Serialize};

/// What kind of principal owns something.
///
/// Deliberately **not** [`crate::PrincipalKind`], which answers a different
/// question — who is *calling* (User/Service/System). An owner is a person or a
/// team; a service cannot be accountable for an asset, because accountability
/// means somebody can be asked.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnerKind {
    User,
    Team,
}

/// A denormalized pointer to an owning principal.
///
/// **Denormalized on purpose.** `00c` requires responses to carry "name, FQN,
/// type, not bare ids": a console rendering an owner list should not have to make
/// N follow-up requests to turn ids into names, and an agent given bare ids
/// reports ids to a human.
///
/// There is no separate `fullyQualifiedName`. For a principal the id **is** the
/// qualified name — `users.id` and `teams.id` are globally unique, human-chosen
/// text — and a second field that always equals the first is a field that will
/// eventually disagree with it.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityReference {
    pub id: String,
    pub kind: OwnerKind,
    /// What a reader sees. Resolved at read time from the principal's record, so
    /// a renamed team shows its new name everywhere rather than whatever it was
    /// called when ownership was assigned.
    pub display_name: String,
    /// True when this owner was not recorded on the entity itself but found by
    /// walking up the containment hierarchy — Epic 11 Slice D.
    ///
    /// **The flag is the whole point of inheriting.** Without it, a 5,000-table
    /// catalog reads as fully owned when in fact nobody has ever named an owner
    /// below the database, and the ownership-gap report has nothing to report.
    /// With it, "deliberately owned here" and "owned by whoever owns the thing
    /// above" are different answers, which is what a steward needs to know.
    ///
    /// Always serialized, never omitted when false: a console reading its absence
    /// cannot tell "direct" from "an older server that did not know about
    /// inheritance".
    ///
    /// Per *entry* rather than per list. Today the list is homogeneous — the walk
    /// stops at the first owned ancestor and takes all of its owners — but the
    /// flag describes a fact about one owner, and putting it beside the owner it
    /// describes means no caller has to correlate two places.
    ///
    /// **`default` is load-bearing, not defensive.** Every version snapshot in
    /// `asset_versions` written before this field existed is an `Asset` JSON
    /// without it, and `asset_versions` deserializes with `.ok()?` inside a
    /// `filter_map` — so a snapshot that failed to parse would not error, it
    /// would *silently vanish from an asset's history*. `false` is also the
    /// correct reading of those snapshots rather than merely a safe one:
    /// inheritance did not exist when they were written, so every owner they
    /// record was recorded directly.
    #[serde(default)]
    pub inherited: bool,
}

impl EntityReference {
    #[must_use]
    pub fn user(id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: OwnerKind::User,
            display_name: display_name.into(),
            inherited: false,
        }
    }

    #[must_use]
    pub fn team(id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: OwnerKind::Team,
            display_name: display_name.into(),
            inherited: false,
        }
    }

    /// Mark this owner as reached by walking up the hierarchy.
    #[must_use]
    pub fn inherited(self) -> Self {
        Self {
            inherited: true,
            ..self
        }
    }
}

/// One owner as a client submits it: an id and which table it is in.
///
/// The kind is **required rather than inferred**. `users.id` and `teams.id` are
/// both free text and could collide, so guessing would silently assign the wrong
/// principal — and "who owns this" is exactly the field where a silent wrong
/// answer is worst.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerRef {
    pub id: String,
    pub kind: OwnerKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{ChangeDescription, ChangeKind, EntityVersion, classify};
    use crate::{Asset, AssetKind};
    use chrono::Utc;
    use uuid::Uuid;

    fn asset_owned_by(owners: Vec<EntityReference>) -> Asset {
        let now = Utc::now();
        Asset {
            id: Uuid::nil(),
            kind: AssetKind::Table,
            name: "orders".into(),
            fully_qualified_name: "warehouse.public.orders".into(),
            parent_id: None,
            description: Some("customer orders".into()),
            properties: None,
            owners,
            version: EntityVersion::initial(),
            updated_by: "system".into(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn as_json(owners: Vec<EntityReference>) -> serde_json::Value {
        serde_json::to_value(asset_owned_by(owners)).expect("serializes")
    }

    // **An unowned asset is a real, reportable state** (Slice C), so the field is
    // always present as a list rather than omitted. Omitting it would make "we
    // have not recorded an owner" indistinguishable from "this version of the API
    // does not have owners".
    #[test]
    fn an_unowned_asset_still_reports_an_empty_owner_list() {
        assert_eq!(as_json(vec![])["owners"], serde_json::json!([]));
    }

    // **The reason the choice above is not cosmetic.** `classify` treats any
    // *removed* field as Major, on the grounds that a consumer reading a field
    // that vanished breaks. An asset losing its last owner is a governance event,
    // not a schema break — so `owners` must go from `["priya"]` to `[]`, which is
    // an update, rather than disappearing, which would be a breaking change.
    #[test]
    fn losing_the_last_owner_is_a_minor_change_not_a_breaking_one() {
        let before = as_json(vec![EntityReference::user("priya", "Priya")]);
        let after = as_json(vec![]);

        let diff = ChangeDescription::between(&before, &after);

        assert_eq!(classify(&diff), ChangeKind::Minor);
        assert!(diff.fields_deleted.is_empty(), "{diff:?}");
    }

    // And the negative that makes the test above about `owners` rather than about
    // `classify` having been loosened: a field that genuinely disappears is still
    // breaking.
    #[test]
    fn a_field_that_genuinely_disappears_is_still_breaking() {
        let before = as_json(vec![]);
        let mut after = before.clone();
        after.as_object_mut().expect("object").remove("description");

        let diff = ChangeDescription::between(&before, &after);

        assert_eq!(classify(&diff), ChangeKind::Major);
    }

    #[test]
    fn gaining_an_owner_is_a_minor_change() {
        let before = as_json(vec![]);
        let after = as_json(vec![EntityReference::team("platform", "Platform")]);

        assert_eq!(
            classify(&ChangeDescription::between(&before, &after)),
            ChangeKind::Minor
        );
    }

    #[test]
    fn replacing_one_owner_with_another_is_a_minor_change() {
        let before = as_json(vec![EntityReference::user("priya", "Priya")]);
        let after = as_json(vec![EntityReference::user("sakshi", "Sakshi")]);

        assert_eq!(
            classify(&ChangeDescription::between(&before, &after)),
            ChangeKind::Minor
        );
    }

    // Users and teams side by side, which `00c` says is the normal case: "every
    // real asset has a producing team and an accountable individual".
    #[test]
    fn an_asset_can_be_owned_by_a_person_and_a_team_at_once() {
        let json = as_json(vec![
            EntityReference::user("priya", "Priya"),
            EntityReference::team("platform", "Platform Team"),
        ]);

        let owners = json["owners"].as_array().expect("a list");

        assert_eq!(owners.len(), 2);
        assert_eq!(owners[0]["kind"], "user");
        assert_eq!(owners[1]["kind"], "team");
    }

    // Submitted order is preserved, because the API reports validation failures by
    // index (`owners[1].id`) and a response that reordered them would make the
    // index point at the wrong entry.
    #[test]
    fn owner_order_survives_serialization() {
        let json = as_json(vec![
            EntityReference::team("platform", "Platform"),
            EntityReference::user("priya", "Priya"),
        ]);
        let owners = json["owners"].as_array().expect("a list");

        assert_eq!(owners[0]["id"], "platform");
        assert_eq!(owners[1]["id"], "priya");
    }

    // The wire shape is camelCase like everything else. Asserted against the bytes
    // rather than a round trip, because a round trip agrees with itself whatever
    // the field names are — the lesson `Authorship` taught by shipping `agent_id`.
    #[test]
    fn the_wire_shape_is_camel_case() {
        let json = as_json(vec![EntityReference::user("priya", "Priya")]);

        assert_eq!(json["owners"][0]["displayName"], "Priya");
        assert!(json["owners"][0].get("display_name").is_none());
    }

    // **Slice D.** An owner recorded on the asset itself is not inherited, and the
    // flag is always present rather than omitted when false — the whole point of
    // the flag is to distinguish "deliberately owned here" from "nobody set this
    // and we walked up", and a field that disappears in one of those two cases
    // makes the console read its absence as the other one.
    #[test]
    fn a_directly_recorded_owner_is_not_inherited() {
        let json = as_json(vec![EntityReference::user("priya", "Priya")]);

        assert_eq!(json["owners"][0]["inherited"], serde_json::json!(false));
    }

    // Inheritance is a property of *this* owner entry, not of the read, so the
    // flag travels with the reference wherever it is rendered.
    #[test]
    fn an_inherited_owner_says_so_on_the_wire() {
        let json = as_json(vec![
            EntityReference::team("platform", "Platform").inherited(),
        ]);

        assert_eq!(json["owners"][0]["inherited"], serde_json::json!(true));
        assert_eq!(json["owners"][0]["id"], "platform");
    }

    // **The version-history regression this field could have caused.** Snapshots
    // in `asset_versions` are whole `Asset` JSON documents, and every one written
    // before Slice D lacks `inherited`. That read path deserializes with `.ok()?`
    // inside a `filter_map`, so a snapshot it cannot parse does not raise — it
    // disappears from the asset's history, silently, with no failing test
    // anywhere. `#[serde(default)]` is what prevents that, and this is the test
    // that stops somebody removing it to make the OpenAPI schema tidier.
    #[test]
    fn an_owner_recorded_before_inheritance_existed_still_parses() {
        let old_snapshot = r#"{"id":"priya","kind":"user","displayName":"Priya"}"#;

        let owner: EntityReference = serde_json::from_str(old_snapshot).expect("parses");

        assert_eq!(owner.id, "priya");
        // Correct, not merely safe: inheritance did not exist when this was
        // written, so the owner it names was necessarily recorded directly.
        assert!(!owner.inherited);
    }

    // A client submits `{id, kind}` and nothing else. `displayName` is resolved at
    // read time, so accepting one on the way in would let a client store a label
    // that disagrees with the principal's actual name.
    #[test]
    fn a_submitted_owner_carries_only_an_id_and_a_kind() {
        let body = r#"{"id":"priya","kind":"user","displayName":"Someone Else"}"#;

        let submitted: OwnerRef = serde_json::from_str(body).expect("parses");

        assert_eq!(submitted.id, "priya");
        assert_eq!(submitted.kind, OwnerKind::User);
    }

    // The kind is required, not inferred: `users.id` and `teams.id` are both free
    // text and could collide, and guessing would silently assign the wrong
    // principal.
    #[test]
    fn a_submitted_owner_without_a_kind_is_refused() {
        assert!(serde_json::from_str::<OwnerRef>(r#"{"id":"priya"}"#).is_err());
    }
}
