//! Domain ontology packs — Epic 33.
//!
//! **Packs supply vocabulary, not schema** (the plan's own framing). A pack
//! is a versioned, imported artifact — never vendored into this repo
//! (`plans/33-ontology-packs.md` decision 1) — that lands as `Approved`
//! terms in its own glossary (decision 4), extendable without a fork
//! (decision 2) via [`PackOverride`], which is stored apart from the pack's
//! own content so an upgrade cannot lose it.
//!
//! Every function here is pure: given the already-parsed
//! [`graph_owl_rdf_io::skos::SkosConcept`]s and whatever storage already
//! knows, it decides what should happen. The actual reading and writing is
//! the facade's job, in `graph-owl-api`.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use graph_owl_core::glossary::SkosRelation;
use graph_owl_rdf_io::skos::SkosConcept;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One imported vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OntologyPack {
    pub id: Uuid,
    /// A short slug ("fibo", "icd10") — not unique alone, since a pack has
    /// several versions over time; `(pack_id, version)` is the real key.
    pub pack_id: String,
    pub version: String,
    pub licence: Licence,
    pub source_url: String,
    /// The pack-owned glossary its terms landed in.
    pub glossary_id: Uuid,
    pub term_count: usize,
    pub imported_at: DateTime<Utc>,
}

/// How a pack may be used — tracked per pack and surfaced (decision 3).
///
/// **No `Default` and no "unknown" variant.** A pack whose manifest omits
/// this must refuse import rather than be guessed permissive — see
/// [`crate`]'s module doc and Slice B's own acceptance criterion. Making
/// this required at the type level, not merely validated, is what makes
/// that guarantee structural rather than a check someone could forget to
/// call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum Licence {
    /// Freely redistributable and usable.
    Permissive { name: String },
    /// Usable, but the notice must be surfaced wherever a term from this
    /// pack is displayed.
    AttributionRequired { name: String, notice: String },
    /// Import must be explicitly acknowledged — see
    /// [`import_requires_acknowledgement`].
    LicenceRequired { name: String, contact: String },
}

/// Does this licence require an explicit "I have the rights to this"
/// acknowledgement before import may proceed?
#[must_use]
pub fn import_requires_acknowledgement(licence: &Licence) -> bool {
    matches!(licence, Licence::LicenceRequired { .. })
}

/// The attribution notice to surface wherever this pack's terms are shown,
/// if its licence requires one.
#[must_use]
pub fn attribution_notice(licence: &Licence) -> Option<&str> {
    match licence {
        Licence::AttributionRequired { notice, .. } => Some(notice),
        Licence::Permissive { .. } | Licence::LicenceRequired { .. } => None,
    }
}

/// What kind of change a [`PackOverride`] makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum OverrideKind {
    /// Replaces the term's definition. `payload: {"definition": "..."}`.
    Redefine,
    /// Hides the term from normal listing without deleting pack content.
    Hide,
    /// Adds one synonym. `payload: {"synonym": "..."}`.
    AddSynonym,
    /// Adds one relation the pack itself did not assert.
    /// `payload: {"kind": "broader"|"narrower"|"related", "target": "..."}`.
    AddRelation,
}

/// An organization's local customization of one pack term — decision 2,
/// "extend without fork". Stored keyed by `term_path`, the term's own
/// **source concept IRI**, not its local database id: the id a re-import or
/// upgrade assigns is not guaranteed stable across storage adapters or
/// migrations, but the IRI is the one thing the publisher itself promises
/// not to change for the same concept.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PackOverride {
    pub id: Uuid,
    pub pack_id: Uuid,
    pub term_path: String,
    pub kind: OverrideKind,
    pub payload: serde_json::Value,
}

/// A pack term as a reader actually sees it — pack content plus whatever
/// overrides apply, merged transparently (Slice C).
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveTerm {
    pub definition: String,
    pub synonyms: Vec<String>,
    pub hidden: bool,
    /// **Never inferred from a diff.** Set the instant any override applies,
    /// so a reader always knows this is not the pack's own, unmodified word
    /// — the whole reason Slice C's acceptance criteria call for it visible.
    pub overridden: bool,
}

/// Merges a term's pack content with whatever overrides target it.
///
/// **Purely a projection, not a mutation** — which is what makes "removing
/// an override restores the pack value" true for free: call this again
/// without the removed override and the pack's own value is what comes
/// back, because it was never overwritten in storage to begin with.
#[must_use]
pub fn apply_overrides(
    base_definition: &str,
    base_synonyms: &[String],
    overrides: &[PackOverride],
) -> EffectiveTerm {
    let mut definition = base_definition.to_string();
    let mut synonyms = base_synonyms.to_vec();
    let mut hidden = false;
    let mut overridden = false;

    for entry in overrides {
        overridden = true;
        match entry.kind {
            OverrideKind::Redefine => {
                if let Some(text) = entry.payload.get("definition").and_then(|v| v.as_str()) {
                    definition = text.to_string();
                }
            }
            OverrideKind::Hide => hidden = true,
            OverrideKind::AddSynonym => {
                if let Some(text) = entry.payload.get("synonym").and_then(|v| v.as_str()) {
                    synonyms.push(text.to_string());
                }
            }
            // Relations are not part of this projection — they live beside
            // the term's stored SkosRelations, added by the facade the same
            // way pack-asserted ones are. `overridden` still reflects it.
            OverrideKind::AddRelation => {}
        }
    }

    EffectiveTerm {
        definition,
        synonyms,
        hidden,
        overridden,
    }
}

/// Resolves every concept's SKOS relations to the term id storage assigned
/// it, ready for `Storage::insert_term_relation`.
///
/// **Must run only after every concept in `concepts` has an id in
/// `term_ids`.** `parse_skos_turtle` already refuses a document whose
/// `broader`/`narrower`/`related` names a concept the document itself does
/// not define, so every lookup here is expected to succeed when `term_ids`
/// was built from this same `concepts` list — the `if let` is defensive
/// rather than a case this function expects to hit, and dropping a target
/// silently here is exactly the "flattened hierarchy" failure Slice A's own
/// mutator watch names.
#[must_use]
// No caller needs a non-default hasher; genericizing over one would add a
// type parameter purely for a case nobody has.
#[allow(clippy::implicit_hasher)]
pub fn resolve_relations(
    concepts: &[SkosConcept],
    term_ids: &HashMap<String, Uuid>,
) -> Vec<(Uuid, SkosRelation)> {
    let mut resolved = Vec::new();
    for concept in concepts {
        let Some(&id) = term_ids.get(&concept.iri) else {
            continue;
        };
        for target in &concept.broader {
            if let Some(&target_id) = term_ids.get(target) {
                resolved.push((id, SkosRelation::Broader(target_id.to_string())));
            }
        }
        for target in &concept.narrower {
            if let Some(&target_id) = term_ids.get(target) {
                resolved.push((id, SkosRelation::Narrower(target_id.to_string())));
            }
        }
        for target in &concept.related {
            if let Some(&target_id) = term_ids.get(target) {
                resolved.push((id, SkosRelation::Related(target_id.to_string())));
            }
        }
        // External by definition — never looked up in `term_ids`, which
        // only ever contains ids for concepts *this* document defines.
        for target in &concept.exact_match {
            resolved.push((id, SkosRelation::ExactMatch(target.clone())));
        }
        for target in &concept.close_match {
            resolved.push((id, SkosRelation::CloseMatch(target.clone())));
        }
    }
    resolved
}

/// What upgrading a pack to a new version would do — Slice D.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeReport {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    /// The subset of `removed` that still has assets attached — surfaced as
    /// its own list, never folded into `removed`, so it cannot be silently
    /// deprecated out from under a reader who never sees it called out.
    pub attached_removed: Vec<String>,
    /// Overrides whose `term_path` no longer names a concept in the new
    /// version — orphaned, not silently dropped.
    pub orphaned_overrides: Vec<String>,
}

impl UpgradeReport {
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.changed.is_empty() || !self.removed.is_empty()
    }
}

/// Diffs an installed pack's concepts against a candidate new version.
///
/// **Comparison is by source IRI, never by position or count** — the same
/// reasoning `graph_owl_core::glossary`'s cycle walk applies to poly-
/// hierarchy: two documents of the same length can differ entirely, and two
/// of different lengths can share everything but one addition.
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn diff_upgrade(
    installed: &[SkosConcept],
    candidate: &[SkosConcept],
    attached_iris: &HashSet<String>,
    override_paths: &HashSet<String>,
) -> UpgradeReport {
    let installed_by_iri: HashMap<&str, &SkosConcept> =
        installed.iter().map(|c| (c.iri.as_str(), c)).collect();
    let candidate_by_iri: HashMap<&str, &SkosConcept> =
        candidate.iter().map(|c| (c.iri.as_str(), c)).collect();

    let added = candidate
        .iter()
        .filter(|c| !installed_by_iri.contains_key(c.iri.as_str()))
        .map(|c| c.iri.clone())
        .collect();

    let changed = candidate
        .iter()
        .filter(|c| {
            installed_by_iri
                .get(c.iri.as_str())
                .is_some_and(|old| *old != *c)
        })
        .map(|c| c.iri.clone())
        .collect();

    let removed: Vec<String> = installed
        .iter()
        .filter(|c| !candidate_by_iri.contains_key(c.iri.as_str()))
        .map(|c| c.iri.clone())
        .collect();

    let attached_removed = removed
        .iter()
        .filter(|iri| attached_iris.contains(iri.as_str()))
        .cloned()
        .collect();

    let orphaned_overrides = override_paths
        .iter()
        .filter(|path| !candidate_by_iri.contains_key(path.as_str()))
        .cloned()
        .collect();

    UpgradeReport {
        added,
        changed,
        removed,
        attached_removed,
        orphaned_overrides,
    }
}

/// Would removing a pack break an `exactMatch` another pack holds onto one
/// of its terms — Slice E's cross-pack reference guard.
///
/// A pure predicate over already-gathered data; the storage layer's job is
/// only to gather `other_packs_exact_match_targets` (every `exactMatch`
/// target recorded by a term belonging to a *different* pack).
#[must_use]
#[allow(clippy::implicit_hasher)]
pub fn removal_blocked_by_cross_pack_reference(
    removed_term_iris: &HashSet<String>,
    other_packs_exact_match_targets: &[String],
) -> bool {
    other_packs_exact_match_targets
        .iter()
        .any(|target| removed_term_iris.contains(target))
}

/// A term's own local name, for deriving its FQN — the trailing fragment or
/// path segment of its source IRI, never the `prefLabel`.
///
/// **A label is for display, not addressing.** A publisher's `prefLabel` can
/// contain spaces or punctuation (`"U.S. Dollar"`) that would break
/// `graph_owl_core::fqn::derive`'s "no separator in a segment" rule; the
/// IRI's own local part is what the publisher already treats as a stable
/// identifier, which is exactly what an FQN segment needs to be.
#[must_use]
pub fn local_name_from_iri(iri: &str) -> &str {
    if let Some(pos) = iri.rfind('#') {
        &iri[pos + 1..]
    } else if let Some(pos) = iri.rfind('/') {
        &iri[pos + 1..]
    } else {
        iri
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(iri: &str, label: &str) -> SkosConcept {
        SkosConcept {
            iri: iri.to_string(),
            pref_label: label.to_string(),
            ..Default::default()
        }
    }

    // ---- Licence (Slice B) ----

    #[test]
    fn only_licence_required_needs_acknowledgement() {
        assert!(import_requires_acknowledgement(&Licence::LicenceRequired {
            name: "Custom".into(),
            contact: "licensing@example.org".into(),
        }));
        assert!(!import_requires_acknowledgement(&Licence::Permissive {
            name: "MIT".into()
        }));
        assert!(!import_requires_acknowledgement(
            &Licence::AttributionRequired {
                name: "CC-BY".into(),
                notice: "Cite us".into(),
            }
        ));
    }

    #[test]
    fn only_attribution_required_carries_a_notice() {
        assert_eq!(
            attribution_notice(&Licence::AttributionRequired {
                name: "CC-BY".into(),
                notice: "Cite the source".into(),
            }),
            Some("Cite the source")
        );
        assert_eq!(
            attribution_notice(&Licence::Permissive { name: "MIT".into() }),
            None
        );
        assert_eq!(
            attribution_notice(&Licence::LicenceRequired {
                name: "Custom".into(),
                contact: "x@example.org".into(),
            }),
            None
        );
    }

    // ---- Overrides (Slice C) ----

    #[test]
    fn with_no_overrides_the_pack_value_is_unchanged() {
        let effective = apply_overrides("original definition", &["syn1".to_string()], &[]);
        assert_eq!(effective.definition, "original definition");
        assert_eq!(effective.synonyms, vec!["syn1".to_string()]);
        assert!(!effective.hidden);
        assert!(!effective.overridden);
    }

    #[test]
    fn a_redefine_override_replaces_the_definition_and_marks_overridden() {
        let redefine = PackOverride {
            id: Uuid::new_v4(),
            pack_id: Uuid::new_v4(),
            term_path: "http://ex.org/v#Loan".to_string(),
            kind: OverrideKind::Redefine,
            payload: serde_json::json!({ "definition": "our house definition" }),
        };
        let effective = apply_overrides("pack definition", &[], std::slice::from_ref(&redefine));
        assert_eq!(effective.definition, "our house definition");
        assert!(effective.overridden);
    }

    #[test]
    fn a_hide_override_hides_without_touching_the_definition() {
        let hide = PackOverride {
            id: Uuid::new_v4(),
            pack_id: Uuid::new_v4(),
            term_path: "http://ex.org/v#Loan".to_string(),
            kind: OverrideKind::Hide,
            payload: serde_json::Value::Null,
        };
        let effective = apply_overrides("pack definition", &[], std::slice::from_ref(&hide));
        assert!(effective.hidden);
        assert_eq!(effective.definition, "pack definition");
    }

    #[test]
    fn an_add_synonym_override_appends_without_removing_the_packs_own() {
        let add = PackOverride {
            id: Uuid::new_v4(),
            pack_id: Uuid::new_v4(),
            term_path: "http://ex.org/v#Loan".to_string(),
            kind: OverrideKind::AddSynonym,
            payload: serde_json::json!({ "synonym": "house term" }),
        };
        let effective = apply_overrides(
            "def",
            &["pack synonym".to_string()],
            std::slice::from_ref(&add),
        );
        assert_eq!(
            effective.synonyms,
            vec!["pack synonym".to_string(), "house term".to_string()]
        );
    }

    // **Restoration is a property of re-computing, not of an undo step.**
    // Calling the same function again without the override must yield exactly
    // the pack's own value — proving "removing an override restores the pack
    // value" without any special-case restore code to get wrong.
    #[test]
    fn removing_an_override_and_recomputing_restores_the_pack_value() {
        let redefine = PackOverride {
            id: Uuid::new_v4(),
            pack_id: Uuid::new_v4(),
            term_path: "http://ex.org/v#Loan".to_string(),
            kind: OverrideKind::Redefine,
            payload: serde_json::json!({ "definition": "overridden" }),
        };
        let with_override = apply_overrides("original", &[], std::slice::from_ref(&redefine));
        assert_eq!(with_override.definition, "overridden");

        let without_override = apply_overrides("original", &[], &[]);
        assert_eq!(without_override.definition, "original");
        assert!(!without_override.overridden);
    }

    // ---- Hierarchy resolution (Slice A) ----

    #[test]
    fn broader_edges_are_resolved_to_the_assigned_term_ids() {
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        let concepts = vec![
            concept("http://ex.org/v#Asset", "Asset"),
            SkosConcept {
                broader: vec!["http://ex.org/v#Asset".to_string()],
                ..concept("http://ex.org/v#Loan", "Loan")
            },
        ];
        let term_ids = HashMap::from([
            ("http://ex.org/v#Asset".to_string(), parent_id),
            ("http://ex.org/v#Loan".to_string(), child_id),
        ]);

        let resolved = resolve_relations(&concepts, &term_ids);

        assert_eq!(
            resolved,
            vec![(child_id, SkosRelation::Broader(parent_id.to_string()))]
        );
    }

    #[test]
    fn a_poly_hierarchy_concept_resolves_every_parent_not_just_the_first() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let concepts = vec![
            concept("http://ex.org/v#A", "A"),
            concept("http://ex.org/v#B", "B"),
            SkosConcept {
                broader: vec![
                    "http://ex.org/v#A".to_string(),
                    "http://ex.org/v#B".to_string(),
                ],
                ..concept("http://ex.org/v#C", "C")
            },
        ];
        let term_ids = HashMap::from([
            ("http://ex.org/v#A".to_string(), a),
            ("http://ex.org/v#B".to_string(), b),
            ("http://ex.org/v#C".to_string(), c),
        ]);

        let resolved = resolve_relations(&concepts, &term_ids);

        assert_eq!(resolved.len(), 2, "both parents must survive resolution");
        assert!(resolved.contains(&(c, SkosRelation::Broader(a.to_string()))));
        assert!(resolved.contains(&(c, SkosRelation::Broader(b.to_string()))));
    }

    #[test]
    fn exact_match_targets_pass_through_unresolved_as_external_iris() {
        let id = Uuid::new_v4();
        let concepts = vec![SkosConcept {
            exact_match: vec!["http://other.org/fibo#Loan".to_string()],
            ..concept("http://ex.org/v#Loan", "Loan")
        }];
        let term_ids = HashMap::from([("http://ex.org/v#Loan".to_string(), id)]);

        let resolved = resolve_relations(&concepts, &term_ids);

        assert_eq!(
            resolved,
            vec![(
                id,
                SkosRelation::ExactMatch("http://other.org/fibo#Loan".to_string())
            )]
        );
    }

    // ---- Upgrade diffing (Slice D) ----

    #[test]
    fn a_wholly_new_concept_is_added() {
        let installed = vec![concept("http://ex.org/v#Loan", "Loan")];
        let candidate = vec![
            concept("http://ex.org/v#Loan", "Loan"),
            concept("http://ex.org/v#Bond", "Bond"),
        ];

        let report = diff_upgrade(&installed, &candidate, &HashSet::new(), &HashSet::new());

        assert_eq!(report.added, vec!["http://ex.org/v#Bond".to_string()]);
        assert!(report.changed.is_empty());
        assert!(report.removed.is_empty());
        assert!(
            report.has_changes(),
            "an addition alone must count as a change"
        );
    }

    #[test]
    fn a_relabelled_concept_is_changed_not_added_or_removed() {
        let installed = vec![concept("http://ex.org/v#Loan", "Loan")];
        let candidate = vec![concept("http://ex.org/v#Loan", "Loan Agreement")];

        let report = diff_upgrade(&installed, &candidate, &HashSet::new(), &HashSet::new());

        assert_eq!(report.changed, vec!["http://ex.org/v#Loan".to_string()]);
        assert!(report.added.is_empty());
        assert!(report.removed.is_empty());
        assert!(
            report.has_changes(),
            "a relabelling alone must count as a change"
        );
    }

    #[test]
    fn an_unchanged_concept_appears_in_no_list() {
        let installed = vec![concept("http://ex.org/v#Loan", "Loan")];
        let candidate = vec![concept("http://ex.org/v#Loan", "Loan")];

        let report = diff_upgrade(&installed, &candidate, &HashSet::new(), &HashSet::new());

        assert!(!report.has_changes());
    }

    // **The prominence guarantee.** A removed-and-attached term must be
    // named in `attached_removed`, distinct from the plain `removed` list —
    // folding it in would let it pass unnoticed among ordinary removals.
    #[test]
    fn a_removed_term_still_attached_to_assets_is_reported_prominently() {
        let installed = vec![
            concept("http://ex.org/v#Loan", "Loan"),
            concept("http://ex.org/v#Bond", "Bond"),
        ];
        let candidate = vec![concept("http://ex.org/v#Bond", "Bond")];
        let attached = HashSet::from(["http://ex.org/v#Loan".to_string()]);

        let report = diff_upgrade(&installed, &candidate, &attached, &HashSet::new());

        assert_eq!(report.removed, vec!["http://ex.org/v#Loan".to_string()]);
        assert_eq!(
            report.attached_removed,
            vec!["http://ex.org/v#Loan".to_string()]
        );
        assert!(
            report.has_changes(),
            "a removal alone must count as a change"
        );
    }

    // The negative half: a removed term with **no** attachments must not
    // appear in `attached_removed`, or the list would be meaningless noise.
    #[test]
    fn a_removed_term_with_no_attachments_is_not_flagged_prominent() {
        let installed = vec![concept("http://ex.org/v#Loan", "Loan")];
        let candidate: Vec<SkosConcept> = vec![];

        let report = diff_upgrade(&installed, &candidate, &HashSet::new(), &HashSet::new());

        assert_eq!(report.removed, vec!["http://ex.org/v#Loan".to_string()]);
        assert!(report.attached_removed.is_empty());
    }

    #[test]
    fn an_override_targeting_a_removed_concept_is_orphaned() {
        let installed = vec![concept("http://ex.org/v#Loan", "Loan")];
        let candidate: Vec<SkosConcept> = vec![];
        let overrides = HashSet::from(["http://ex.org/v#Loan".to_string()]);

        let report = diff_upgrade(&installed, &candidate, &HashSet::new(), &overrides);

        assert_eq!(
            report.orphaned_overrides,
            vec!["http://ex.org/v#Loan".to_string()]
        );
    }

    // The negative: an override on a concept that survives the upgrade must
    // not be reported as orphaned.
    #[test]
    fn an_override_on_a_surviving_concept_is_not_orphaned() {
        let installed = vec![concept("http://ex.org/v#Loan", "Loan")];
        let candidate = vec![concept("http://ex.org/v#Loan", "Loan")];
        let overrides = HashSet::from(["http://ex.org/v#Loan".to_string()]);

        let report = diff_upgrade(&installed, &candidate, &HashSet::new(), &overrides);

        assert!(report.orphaned_overrides.is_empty());
    }

    // ---- Cross-pack removal guard (Slice E) ----

    #[test]
    fn removal_is_blocked_when_another_pack_exact_matches_a_removed_term() {
        let removed = HashSet::from(["http://ex.org/v#Loan".to_string()]);
        let others = vec!["http://ex.org/v#Loan".to_string()];

        assert!(removal_blocked_by_cross_pack_reference(&removed, &others));
    }

    #[test]
    fn removal_is_permitted_when_no_other_pack_references_it() {
        let removed = HashSet::from(["http://ex.org/v#Loan".to_string()]);
        let others = vec!["http://ex.org/v#SomethingElse".to_string()];

        assert!(!removal_blocked_by_cross_pack_reference(&removed, &others));
    }

    // ---- Local name derivation (Slice A) ----

    #[test]
    fn a_hash_fragment_iri_gives_the_fragment() {
        assert_eq!(
            local_name_from_iri("http://ex.org/v#PrincipalAmount"),
            "PrincipalAmount"
        );
    }

    #[test]
    fn a_slash_delimited_iri_gives_the_last_segment() {
        assert_eq!(
            local_name_from_iri("http://ex.org/vocab/PrincipalAmount"),
            "PrincipalAmount"
        );
    }

    // The hash must win over the slash when both are present — an IRI's
    // fragment is always what a vocabulary actually addresses by, regardless
    // of how many path segments come before it.
    #[test]
    fn a_hash_after_slashes_still_gives_the_fragment() {
        assert_eq!(local_name_from_iri("http://ex.org/v1/v2#Loan"), "Loan");
    }

    #[test]
    fn an_iri_with_neither_delimiter_is_returned_whole() {
        assert_eq!(local_name_from_iri("Loan"), "Loan");
    }
}
