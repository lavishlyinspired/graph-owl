//! The portable archive format — Epic 37b.
//!
//! Pure types, no I/O: `graph-owl-api` streams them into and out of a
//! `.tar.zst` file, `graph-owl-cli` is the terminal surface. Kept separate
//! from both so the format itself — what a manifest says, what one archived
//! entity carries — is a single, testable definition rather than something
//! implied by a serializer.
//!
//! Lossless by default (decision 2), unlike Epic 20's declarative export:
//! an [`ArchivedEntity`] carries the asset's full version history, not just
//! its current declarable fields.

use chrono::{DateTime, Utc};

use crate::{Asset, AssetKind, AssetVersion, Relationship};

/// The archive format's own version — decision 1: a contract, not an
/// implementation detail. `(major, minor, patch)` rather than a parsed
/// semver string, so a version comparison cannot disagree with how the
/// manifest was actually written.
pub const FORMAT_VERSION: (u16, u16, u16) = (1, 0, 0);

/// Everything a restore needs to know before it reads a single entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveManifest {
    /// `(major, minor, patch)` of the format this archive was written in.
    pub format_version: (u16, u16, u16),
    /// An opaque label for where this came from — not parsed, only shown, so
    /// a restore's error message can say what produced the archive it just
    /// refused.
    pub source_instance: String,
    /// When the export ran.
    pub created_at: DateTime<Utc>,
    /// How many entities the archive carries.
    pub entity_count: u64,
    /// How many relationships the archive carries.
    pub relationship_count: u64,
    /// `None` means the whole catalog — decision 5's default. `Some(&[])`
    /// cannot occur: an empty scope is refused before export runs (an empty
    /// archive that *looks* deliberate is worse than one that fails loudly).
    pub scope: Option<Vec<ScopeSelector>>,
    /// Field names redacted at export time — Slice E. Recorded here so a
    /// restore (and a human reading the manifest) can tell a genuinely empty
    /// field from one that was scrubbed.
    pub redacted_fields: Vec<String>,
    /// SHA-256 of each section file's bytes, hex-encoded, keyed by filename
    /// (`entities.ndjson`, `relationships.ndjson`). A restore verifies each
    /// section independently before writing anything, which is stronger
    /// than trusting the archive's own compressed-stream integrity check —
    /// that catches a corrupted *container*, not a section rewritten with a
    /// still-valid one.
    pub section_checksums: std::collections::BTreeMap<String, String>,
}

impl ArchiveManifest {
    /// Whether a binary carrying [`FORMAT_VERSION`] can read this archive.
    ///
    /// Same major, and not a newer minor — a newer minor may have added a
    /// field this binary does not know to look for, and restoring while
    /// silently ignoring it is exactly the "confidently wrong" failure mode
    /// this project refuses everywhere else. An older minor is always
    /// readable: it is a subset of what this binary understands.
    #[must_use]
    pub fn readable_by_this_binary(&self) -> bool {
        self.format_version.0 == FORMAT_VERSION.0 && self.format_version.1 <= FORMAT_VERSION.1
    }
}

/// What an export was narrowed to — decision 5. `FqnPrefix` covers domain
/// and service scoping (`--scope domain:payments`, `--scope
/// service:snowflake_prod`); `Kind` covers entity-type scoping
/// (`--scope entity-type:table`). Two variants rather than one FQN-only
/// selector because a kind is not a prefix of anything — `table` does not
/// sit at a fixed position in every FQN the way a domain or service does.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum ScopeSelector {
    /// Everything at or beneath this fully-qualified name — domain and
    /// service scoping.
    FqnPrefix(String),
    /// Every asset of this kind — entity-type scoping.
    Kind(AssetKind),
}

impl ScopeSelector {
    /// Whether `asset` falls inside this one selector. [`matches_any`] is
    /// what a caller actually wants — combining scopes is a union (decision
    /// 5) — this is the per-selector primitive it is built from.
    ///
    /// [`matches_any`]: ScopeSelector::matches_any
    #[must_use]
    pub fn matches(&self, asset: &Asset) -> bool {
        match self {
            Self::FqnPrefix(prefix) => {
                asset.fully_qualified_name == *prefix
                    || asset
                        .fully_qualified_name
                        .starts_with(&format!("{prefix}."))
            }
            Self::Kind(kind) => asset.kind == *kind,
        }
    }

    /// A union over every selector — `None`/empty scope always matches,
    /// which is what makes "no scope" mean "the whole catalog" rather than
    /// "nothing in scope".
    #[must_use]
    pub fn matches_any(selectors: &[Self], asset: &Asset) -> bool {
        selectors.is_empty() || selectors.iter().any(|selector| selector.matches(asset))
    }
}

/// What a restore does when an incoming entity collides with a live one —
/// decision 4. No `Default` impl: a silent default is exactly what decision
/// 4 refuses, so every call site must name its policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    /// Any conflict aborts the restore before anything is written.
    Fail,
    /// Existing entities are left untouched; the archive's copy is dropped.
    Skip,
    /// Existing entities are replaced, bumping their version, history kept.
    Overwrite,
}

impl std::fmt::Display for ConflictPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
        })
    }
}

impl std::str::FromStr for ConflictPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fail" => Ok(Self::Fail),
            "skip" => Ok(Self::Skip),
            "overwrite" => Ok(Self::Overwrite),
            other => Err(format!(
                "`{other}` is not a conflict policy; expected one of: fail, skip, overwrite"
            )),
        }
    }
}

/// One entity, with its full version history — the unit `entities.ndjson`
/// carries, one per line. History travels with its entity rather than in a
/// separate section keyed by id, so a restore never has to hold a whole
/// section in memory to reattach the other.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedEntity {
    /// The entity's current state.
    pub asset: Asset,
    /// Every past state, oldest first.
    pub versions: Vec<AssetVersion>,
}

/// A relationship, exactly as `relationships.ndjson` carries it — one per
/// line. A thin wrapper rather than the bare type so the section's line
/// shape can gain a field later without disturbing [`Relationship`] itself.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedRelationship {
    /// The relationship itself.
    pub relationship: Relationship,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(fqn: &str, kind: AssetKind) -> Asset {
        let now = Utc::now();
        Asset {
            id: uuid::Uuid::new_v4(),
            kind,
            name: fqn.rsplit('.').next().unwrap_or(fqn).to_string(),
            fully_qualified_name: fqn.to_string(),
            parent_id: None,
            description: None,
            properties: None,
            owners: Vec::new(),
            version: crate::envelope::EntityVersion::initial(),
            updated_by: "test".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            extension: None,
            lifecycle: crate::lifecycle::LifecycleState::default(),
            deprecation: None,
        }
    }

    #[test]
    fn a_binary_reads_its_own_version_and_any_older_minor() {
        let same = ArchiveManifest {
            format_version: FORMAT_VERSION,
            source_instance: "test".into(),
            created_at: Utc::now(),
            entity_count: 0,
            relationship_count: 0,
            scope: None,
            redacted_fields: vec![],
            section_checksums: std::collections::BTreeMap::new(),
        };
        assert!(same.readable_by_this_binary());

        let older_minor = ArchiveManifest {
            format_version: (FORMAT_VERSION.0, 0, 999),
            ..same.clone()
        };
        assert!(older_minor.readable_by_this_binary());
    }

    /// **The slice's own RED test.** A format version newer than this binary
    /// understands must be refused, never silently accepted — a binary that
    /// accepted anything would restore an archive whose fields it cannot
    /// read and call the result complete.
    #[test]
    fn a_newer_major_or_minor_is_not_readable() {
        let newer_major = ArchiveManifest {
            format_version: (FORMAT_VERSION.0 + 1, 0, 0),
            source_instance: "test".into(),
            created_at: Utc::now(),
            entity_count: 0,
            relationship_count: 0,
            scope: None,
            redacted_fields: vec![],
            section_checksums: std::collections::BTreeMap::new(),
        };
        assert!(!newer_major.readable_by_this_binary());

        let newer_minor = ArchiveManifest {
            format_version: (FORMAT_VERSION.0, FORMAT_VERSION.1 + 1, 0),
            ..newer_major
        };
        assert!(!newer_minor.readable_by_this_binary());
    }

    #[test]
    fn an_fqn_prefix_selector_matches_the_prefix_itself_and_its_descendants() {
        let selector = ScopeSelector::FqnPrefix("payments".to_string());
        assert!(selector.matches(&asset("payments", AssetKind::Service)));
        assert!(selector.matches(&asset("payments.orders", AssetKind::Table)));
        // And the negative: a sibling that merely shares a prefix textually
        // must not match — "payments2" is not "payments".
        assert!(!selector.matches(&asset("payments2.orders", AssetKind::Table)));
    }

    #[test]
    fn a_kind_selector_matches_only_that_kind() {
        let selector = ScopeSelector::Kind(AssetKind::Table);
        assert!(selector.matches(&asset("a.b", AssetKind::Table)));
        assert!(!selector.matches(&asset("a.b", AssetKind::Column)));
    }

    #[test]
    fn combining_scopes_is_a_union() {
        let selectors = vec![
            ScopeSelector::FqnPrefix("payments".to_string()),
            ScopeSelector::Kind(AssetKind::Topic),
        ];
        // In neither selector.
        assert!(!ScopeSelector::matches_any(
            &selectors,
            &asset("billing.invoices", AssetKind::Table)
        ));
        // In the prefix only.
        assert!(ScopeSelector::matches_any(
            &selectors,
            &asset("payments.orders", AssetKind::Table)
        ));
        // In the kind only.
        assert!(ScopeSelector::matches_any(
            &selectors,
            &asset("billing.events", AssetKind::Topic)
        ));
    }

    #[test]
    fn no_scope_matches_everything() {
        assert!(ScopeSelector::matches_any(
            &[],
            &asset("anything.at.all", AssetKind::Table)
        ));
    }

    #[test]
    fn conflict_policy_round_trips_through_its_string_form() {
        for policy in [
            ConflictPolicy::Fail,
            ConflictPolicy::Skip,
            ConflictPolicy::Overwrite,
        ] {
            let parsed: ConflictPolicy = policy.to_string().parse().expect("parses");
            assert_eq!(parsed, policy);
        }
    }

    #[test]
    fn an_unknown_conflict_policy_string_is_refused() {
        assert!("merge".parse::<ConflictPolicy>().is_err());
    }
}
