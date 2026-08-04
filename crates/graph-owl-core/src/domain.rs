//! Domains and data products — Epic 23.
//!
//! **A second grouping axis, orthogonal to containment.** The technical
//! hierarchy says where data *lives*; a [`Domain`] says who is *accountable*
//! for it, and a [`DataProduct`] says what is *consumable*. The three answer
//! different questions and a single tree cannot answer all of them: a domain
//! spans several services, a product bundles assets from several schemas.
//!
//! Everything here is pure. Resolving which domain an asset falls under needs
//! the containment hierarchy and therefore lives in the adapter; deciding
//! *what a resolution means* does not, and is here.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::envelope::{ChangeDescription, EntityVersion};
use chrono::{DateTime, Utc};

/// An accountability boundary.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Domain {
    /// The stable identifier.
    pub id: Uuid,
    /// The domain's own name.
    pub name: String,
    /// Derived from the parent chain, never client-set — the same rule the
    /// asset hierarchy follows, for the same reason: a path a client supplies
    /// is a path that can disagree with the parent.
    pub fully_qualified_name: String,
    /// The containing domain, if this one is nested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    /// A human-readable description, if one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Open text rather than an enum. "Source-aligned", "consumer-aligned" and
    /// "aggregate" are one framework's vocabulary, and an organization using
    /// another should not need a release to say so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_type: Option<String>,
    /// Named people to ask — **not** the owners.
    ///
    /// An owner is accountable; an expert is knowledgeable. Conflating them
    /// means either the accountable person is presumed to know the data or the
    /// knowledgeable one is presumed answerable for it, and both are wrong
    /// often enough to matter.
    #[serde(default)]
    pub experts: Vec<String>,
    /// The envelope's version, bumped on every change.
    pub version: EntityVersion,
    /// Who or what made the most recent change.
    pub updated_by: String,
    /// A human-readable note on the most recent change, if one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    /// Whether the domain is tombstoned.
    pub deleted: bool,
    /// When the domain was tombstoned, if it has been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// When the domain was first created.
    pub created_at: DateTime<Utc>,
    /// When the domain was most recently changed.
    pub updated_at: DateTime<Utc>,
}

/// A consumable bundle of assets, spanning technical boundaries.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataProduct {
    /// The stable identifier.
    pub id: Uuid,
    /// The product's own name.
    pub name: String,
    /// The full dotted path from the root.
    pub fully_qualified_name: String,
    /// A human-readable description, if one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What it is *for*, separate from what it *is*.
    ///
    /// A product with no stated purpose is the failure this entity exists to
    /// prevent: a bundle of tables somebody assembled and nobody can explain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// The domain accountable for this product, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_id: Option<Uuid>,
    /// The envelope's version, bumped on every change.
    pub version: EntityVersion,
    /// Who or what made the most recent change.
    pub updated_by: String,
    /// A human-readable note on the most recent change, if one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    /// Whether the product is tombstoned.
    pub deleted: bool,
    /// When the product was tombstoned, if it has been.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// When the product was first created.
    pub created_at: DateTime<Utc>,
    /// When the product was most recently changed.
    pub updated_at: DateTime<Utc>,
}

/// Which domain an asset falls under, and whether it was told or inherited.
///
/// **The flag is the whole point.** Without it, a catalog where one database
/// was assigned reads as fully governed when in fact nobody has ever named a
/// domain below it — and the assignment-gap report has nothing to report. With
/// it, "deliberately placed here" and "wherever the thing above it sits" are
/// different answers, which is what a steward needs.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainAssignment {
    /// The domain's stable identifier.
    pub id: Uuid,
    /// The domain's own name.
    pub name: String,
    /// The domain's full dotted path from the root.
    pub fully_qualified_name: String,
    /// True when the domain was found by walking up the containment hierarchy
    /// rather than recorded on the asset itself.
    ///
    /// Always serialized, never omitted when false: a console reading its
    /// absence cannot tell "direct" from "an older server that did not know
    /// about inheritance".
    pub inherited: bool,
}

/// The fully-qualified name a domain gets under `parent`.
///
/// Dot-separated, the same convention the asset hierarchy uses — one FQN
/// grammar for the whole catalog, so a reader who has learned it once can read
/// any of them.
#[must_use]
pub fn domain_fqn(parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(parent) if !parent.is_empty() => format!("{parent}.{name}"),
        _ => name.to_string(),
    }
}

/// Whether a domain name can exist at all.
///
/// A dot would make the derived FQN ambiguous — `payments.billing` as a *name*
/// under `retail` produces `retail.payments.billing`, which is indistinguishable
/// from a real three-level path and would make the hierarchy unreadable from
/// its own identifiers.
///
/// # Errors
///
/// A sentence naming what is wrong, ready to be a `400` detail.
pub fn validate_domain_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("a domain needs a name".to_string());
    }
    if name.contains('.') {
        return Err(format!(
            "`{name}` cannot contain a dot: a fully-qualified name is \
             dot-separated, so a dotted name would be indistinguishable from a \
             deeper path"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_domain_is_its_own_path() {
        assert_eq!(domain_fqn(None, "payments"), "payments");
        assert_eq!(domain_fqn(Some(""), "payments"), "payments");
    }

    #[test]
    fn a_nested_domain_is_dotted_under_its_parent() {
        assert_eq!(domain_fqn(Some("retail"), "payments"), "retail.payments");
        assert_eq!(
            domain_fqn(Some("retail.payments"), "billing"),
            "retail.payments.billing"
        );
    }

    /// **The name rule exists to keep the path readable from itself.** A dotted
    /// name produces an FQN indistinguishable from a deeper hierarchy, and
    /// nothing downstream can then tell `retail.payments` the two-level path
    /// from `retail` containing a domain literally called `payments`.
    #[test]
    fn a_dotted_name_is_refused_and_says_why() {
        let error = validate_domain_name("retail.payments").expect_err("a dot is refused");

        assert!(error.contains("dot"), "{error}");
        assert!(error.contains("retail.payments"), "{error}");
    }

    #[test]
    fn a_blank_name_is_refused() {
        assert!(validate_domain_name("").is_err());
        assert!(validate_domain_name("   ").is_err());
    }

    /// And the negative, or the rule above would be indistinguishable from
    /// every name being refused.
    #[test]
    fn an_ordinary_name_is_accepted() {
        assert!(validate_domain_name("payments").is_ok());
        assert!(validate_domain_name("customer-360").is_ok());
    }

    #[test]
    fn an_assignment_round_trips_through_json_carrying_its_inherited_flag() {
        let assignment = DomainAssignment {
            id: Uuid::nil(),
            name: "payments".to_string(),
            fully_qualified_name: "retail.payments".to_string(),
            inherited: true,
        };

        let json = serde_json::to_value(&assignment).expect("serialize");
        assert_eq!(json["inherited"], true);
        assert!(
            json.get("fullyQualifiedName").is_some(),
            "the wire is camelCase: {json}"
        );

        let parsed: DomainAssignment = serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, assignment);
    }

    /// **`inherited: false` must be on the wire, not omitted.** A console
    /// reading its absence cannot tell a direct assignment from a server that
    /// predates inheritance, and it would then render every direct assignment
    /// as inherited or vice versa.
    #[test]
    fn a_direct_assignment_still_states_that_it_is_not_inherited() {
        let json = serde_json::to_value(DomainAssignment {
            id: Uuid::nil(),
            name: "payments".to_string(),
            fully_qualified_name: "payments".to_string(),
            inherited: false,
        })
        .expect("serialize");

        assert_eq!(json["inherited"], false, "{json}");
    }
}
