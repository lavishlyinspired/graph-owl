//! The file format — Epic 20 Slice A.
//!
//! **Deliberately its own types, not a re-export of `graph_owl_core::Asset`.**
//! Slice A's refactor note settles this: a file format that people commit to
//! git and review in pull requests has to be able to evolve independently of
//! the internal representation, and coupling them would make every internal
//! rename a breaking change to everybody's checked-in YAML. The cost is one
//! conversion; the alternative is a format nobody can safely refactor.

use serde::{Deserialize, Serialize};

/// The only `apiVersion` this release understands.
///
/// Present on every declaration for the reason every versioned config format
/// has one: the day the shape changes, files written against today's shape
/// must be recognisable as such rather than mis-parsed into it.
pub const API_VERSION: &str = "graph-owl.dev/v1";

/// One declared entity.
///
/// `deny_unknown_fields` is the point rather than a detail: a typo'd key in a
/// declaration would otherwise be silently ignored, and the entity would be
/// created without the field its author believed they had set — the exact
/// class of failure a plan-before-apply workflow exists to make impossible.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Declaration {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
    /// The parent's fully-qualified name. `None` only for a root-kind entity
    /// (a `service`); every other kind is refused without one by the same
    /// containment rule `AssetKind::parent_kind` already enforces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Absent means **"not declared"**, never "set to null" — decision 4.
    /// `Option<Option<String>>` would be needed to express an explicit null,
    /// and that is Slice C's problem (clearing a field), not Slice A's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Declaration {
    /// The address this declaration claims, derived the same way the catalog
    /// derives it — parent chain plus name — so a declared FQN and a live one
    /// are comparable without a second rule.
    #[must_use]
    pub fn fully_qualified_name(&self) -> String {
        match &self.metadata.parent {
            Some(parent) => format!("{parent}.{}", self.metadata.name),
            None => self.metadata.name.clone(),
        }
    }
}
