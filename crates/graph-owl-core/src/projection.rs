//! Projection of entities into flakes.
//!
//! Pure — a function from an entity to a `Vec<Flake>`, with no I/O. That is
//! deliberate and load-bearing: this is the highest-traffic transformation in
//! the engine, it runs on every write, and being pure is what makes it
//! exhaustively testable without a database.
//!
//! Relational is the source of truth and flakes are the graph *view* of it
//! (`plans/04-engine-triples.md` decision 1). Nothing here ever reads back.

use crate::flake::{Flake, FlakeValue, Sid, namespace};
use crate::{Asset, AssetKind};

/// The predicate naming an asset's container, by the *parent's* kind:
/// `dsc:parentSchema` on a table, `dsc:parentTable` on a column.
///
/// Typed rather than a single `dsc:parent` because "every table in this
/// schema" and "every column in this table" would otherwise be the same query,
/// and a traversal could not tell one level of the hierarchy from another.
#[must_use]
pub fn parent_predicate(kind: AssetKind) -> Option<Sid> {
    kind.parent_kind().map(|parent| {
        let name = parent.as_str();
        let mut capitalized = name.chars();
        let head = capitalized.next().map(|c| c.to_ascii_uppercase());
        Sid::dsc(format!(
            "parent{}{}",
            head.unwrap_or_default(),
            capitalized.as_str()
        ))
    })
}

/// The node an asset occupies in the graph.
#[must_use]
pub fn asset_sid(asset: &Asset) -> Sid {
    Sid::new(namespace::DSC, asset.id.to_string())
}

/// Every field of an asset, as flakes sharing one transaction time.
///
/// The pairs are built once and reused by [`asset_update_flakes`], so a field
/// added here is diffed there automatically — the alternative, two lists that
/// must be kept in step, drifts the first time someone adds a field in a hurry.
fn fields(asset: &Asset) -> Vec<(Sid, FlakeValue)> {
    let mut out = vec![
        (
            Sid::dsc("type"),
            FlakeValue::String(asset.kind.as_str().to_string()),
        ),
        (Sid::dsc("name"), FlakeValue::String(asset.name.clone())),
        (
            Sid::dsc("fqn"),
            FlakeValue::String(asset.fully_qualified_name.clone()),
        ),
        (
            // An ordered pair rendered as text, so `0.10` and `0.1` are
            // different values. A float would make them equal.
            Sid::dsc("version"),
            FlakeValue::String(format!("{}.{}", asset.version.major, asset.version.minor)),
        ),
        (
            Sid::dsc("updatedBy"),
            FlakeValue::String(asset.updated_by.clone()),
        ),
        // Always asserted, never omitted: absence of `deleted` would be
        // ambiguous between "live" and "not yet projected".
        (Sid::dsc("deleted"), FlakeValue::Boolean(asset.deleted)),
        (Sid::dsc("createdAt"), FlakeValue::Instant(asset.created_at)),
        (Sid::dsc("updatedAt"), FlakeValue::Instant(asset.updated_at)),
    ];

    // Optional fields emit nothing when absent. A null assertion would make
    // "nobody wrote one" indistinguishable from "someone cleared it".
    if let Some(description) = &asset.description {
        out.push((
            Sid::dsc("description"),
            FlakeValue::String(description.clone()),
        ));
    }
    if let Some(properties) = &asset.properties {
        out.push((
            Sid::dsc("properties"),
            FlakeValue::Json(properties.to_string()),
        ));
    }
    if let Some(deleted_at) = asset.deleted_at {
        out.push((Sid::dsc("deletedAt"), FlakeValue::Instant(deleted_at)));
    }
    if let (Some(parent), Some(predicate)) = (asset.parent_id, parent_predicate(asset.kind)) {
        // A Ref, not a string: OPST is reference-only, so a text endpoint
        // would be unreachable by reverse traversal.
        out.push((
            predicate,
            FlakeValue::Ref(Sid::new(namespace::DSC, parent.to_string())),
        ));
    }
    out
}

/// Project an asset into the graph.
#[must_use]
pub fn asset_to_flakes(asset: &Asset, t: i64) -> Vec<Flake> {
    let subject = asset_sid(asset);
    fields(asset)
        .into_iter()
        .map(|(predicate, value)| Flake::assert(subject.clone(), predicate, value, t))
        .collect()
}

/// The flakes that turn `before` into `after`.
///
/// A changed field produces a retraction of the old value *and* an assertion
/// of the new one, at the same `t`. Asserting without retracting would leave
/// two current values for a single-valued predicate; retracting without
/// asserting would blank the field. An unchanged field produces nothing at
/// all, which is what makes a nightly connector re-run safe.
#[must_use]
pub fn asset_update_flakes(before: &Asset, after: &Asset, t: i64) -> Vec<Flake> {
    let subject = asset_sid(after);
    let old = fields(before);
    let new = fields(after);

    let mut out = Vec::new();

    for (predicate, was) in &old {
        match new.iter().find(|(p, _)| p == predicate) {
            // Unchanged: nothing to say.
            Some((_, is)) if is == was => {}
            // Changed or cleared: withdraw what was stored. The retraction
            // carries the *old* value — one naming the new value would match
            // no stored fact and withdraw nothing.
            _ => out.push(Flake {
                op: false,
                ..Flake::assert(subject.clone(), predicate.clone(), was.clone(), t)
            }),
        }
    }

    for (predicate, is) in &new {
        let unchanged = old.iter().any(|(p, was)| p == predicate && was == is);
        if !unchanged {
            out.push(Flake::assert(
                subject.clone(),
                predicate.clone(),
                is.clone(),
                t,
            ));
        }
    }

    out
}

/// Reassemble an asset from the flakes visible at some transaction time.
///
/// The inverse of [`asset_to_flakes`], and the payoff of the whole flake
/// model: given the flakes current *as of* a past `t`, this returns the entity
/// as it stood then — reconstructed, not looked up in a snapshot table that
/// could have drifted from the facts.
///
/// Returns `None` when the flakes do not describe an asset at all. That is the
/// honest answer for a subject that did not exist yet: an asset synthesised
/// from partial facts would be a state the catalog was never in.
///
/// # Errors
///
/// None — a malformed set yields `None` rather than a panic, because these
/// flakes come from storage and a corrupt row must not take down a read.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn asset_from_flakes(id: uuid::Uuid, flakes: &[Flake]) -> Option<Asset> {
    let subject = id.to_string();
    let mine: Vec<&Flake> = flakes.iter().filter(|f| f.s.id == subject).collect();
    if mine.is_empty() {
        return None;
    }

    let find = |predicate: &str| -> Option<&FlakeValue> {
        mine.iter().find(|f| f.p.id == predicate).map(|f| &f.o)
    };
    let text = |predicate: &str| -> Option<String> {
        match find(predicate) {
            Some(FlakeValue::String(s)) => Some(s.clone()),
            _ => None,
        }
    };
    let instant = |predicate: &str| -> Option<chrono::DateTime<chrono::Utc>> {
        match find(predicate) {
            Some(FlakeValue::Instant(dt)) => Some(*dt),
            _ => None,
        }
    };

    // Identity, name and kind are what make this an asset rather than an
    // arbitrary bag of facts. Without all three there is nothing to return.
    let kind = AssetKind::parse(&text("type")?).ok()?;
    let name = text("name")?;
    let fully_qualified_name = text("fqn")?;

    let version = text("version")
        .and_then(|raw| {
            let (major, minor) = raw.split_once('.')?;
            Some(crate::envelope::EntityVersion {
                major: major.parse().ok()?,
                minor: minor.parse().ok()?,
            })
        })
        .unwrap_or_else(crate::envelope::EntityVersion::initial);

    // The parent predicate is typed by the parent's kind, so the lookup has to
    // ask for the one this kind would have written.
    let parent_id = parent_predicate(kind).and_then(|predicate| match find(&predicate.id) {
        Some(FlakeValue::Ref(reference)) => reference.id.parse().ok(),
        _ => None,
    });

    Some(Asset {
        id,
        kind,
        name,
        fully_qualified_name,
        parent_id,
        description: text("description"),
        // Not projected: the graph view holds catalog facts, and an
        // organization's own fields are not among them until a slice decides
        // which predicate each one lowers to.
        extension: None,
        properties: match find("properties") {
            Some(FlakeValue::Json(raw)) => serde_json::from_str(raw).ok(),
            _ => None,
        },
        // **Empty on a historical read, and that is the honest answer.** Owners
        // live in a relational join table, not in the triple projection, so a
        // reconstruction from flakes has nothing to read. Filling them from the
        // *current* owners would attribute today's ownership to a past version,
        // which is exactly the misattribution `change_description: None` below
        // refuses for the same reason.
        owners: Vec::new(),
        version,
        updated_by: text("updatedBy").unwrap_or_else(|| "system".to_string()),
        // A historical read reconstructs *state*, not the diff that produced
        // it. The change description belongs to the version row that recorded
        // it, and inventing one here would attribute a change to the wrong
        // transaction.
        change_description: None,
        deleted: matches!(find("deleted"), Some(FlakeValue::Boolean(true))),
        deleted_at: instant("deletedAt"),
        created_at: instant("createdAt").unwrap_or_else(chrono::Utc::now),
        updated_at: instant("updatedAt").unwrap_or_else(chrono::Utc::now),
    })
}

/// The node a relationship occupies in the graph.
///
/// A relationship is a **node**, not a bare predicate assertion between its
/// endpoints. The flat form cannot carry a payload — confidence, provenance,
/// the SQL that produced a lineage edge — and "every relationship below 0.5
/// confidence" is not expressible over it at all. The cost is two hops to
/// traverse (`plans/04-engine-triples.md` decision 4).
#[must_use]
pub fn relationship_sid(relationship: &crate::Relationship) -> Sid {
    Sid::new(namespace::DSC, relationship.id.to_string())
}

/// Project a relationship into the graph as a reified node.
#[must_use]
pub fn relationship_to_flakes(relationship: &crate::Relationship, t: i64) -> Vec<Flake> {
    let subject = relationship_sid(relationship);
    let entity = |id: uuid::Uuid| FlakeValue::Ref(Sid::new(namespace::DSC, id.to_string()));

    vec![
        // `rdf:type`, not `dsc:type`: this says what kind of *thing* the node
        // is in the standard vocabulary, which is what lets Epic 9 export it
        // without a translation table.
        Flake::assert(
            subject.clone(),
            Sid::new(namespace::RDF, "type"),
            FlakeValue::Ref(Sid::dsc("Relationship")),
            t,
        ),
        // Endpoints are references so OPST reverse traversal reaches them —
        // "what feeds this table" is a lookup by object, and a string endpoint
        // would put it on a sequential scan.
        Flake::assert(
            subject.clone(),
            Sid::dsc("fromEntity"),
            entity(relationship.from_entity_id),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("toEntity"),
            entity(relationship.to_entity_id),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("relType"),
            FlakeValue::String(relationship.relationship_type.clone()),
            t,
        ),
        // The endpoint *kinds* travel with the edge. Without them a traversal
        // has to resolve both endpoints to learn whether an edge is
        // table→table or column→column, which is two extra reads per edge on
        // the hot path.
        Flake::assert(
            subject.clone(),
            Sid::dsc("fromEntityType"),
            FlakeValue::String(relationship.from_entity_type.clone()),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("toEntityType"),
            FlakeValue::String(relationship.to_entity_type.clone()),
            t,
        ),
        Flake::assert(
            subject,
            Sid::dsc("createdAt"),
            FlakeValue::Instant(relationship.created_at),
            t,
        ),
    ]
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::envelope::EntityVersion;
    use crate::flake::{FlakeValue, Sid, namespace};
    use crate::{Asset, AssetKind};
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn table() -> Asset {
        Asset {
            id: Uuid::from_u128(1),
            kind: AssetKind::Table,
            name: "upi_transactions".to_string(),
            fully_qualified_name: "hdfc-core.postgres.payments.upi_transactions".to_string(),
            parent_id: Some(Uuid::from_u128(2)),
            description: Some("UPI transaction ledger".to_string()),
            extension: None,
            properties: None,
            owners: Vec::new(),
            version: EntityVersion { major: 0, minor: 1 },
            updated_by: "asha".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            updated_at: Utc.timestamp_opt(1_700_000_500, 0).unwrap(),
        }
    }

    fn value_of<'a>(flakes: &'a [crate::flake::Flake], predicate: &str) -> Option<&'a FlakeValue> {
        flakes.iter().find(|f| f.p.id == predicate).map(|f| &f.o)
    }

    #[test]
    fn every_populated_envelope_field_becomes_one_flake() {
        let flakes = asset_to_flakes(&table(), 7);

        assert_eq!(
            value_of(&flakes, "name"),
            Some(&FlakeValue::String("upi_transactions".into()))
        );
        assert_eq!(
            value_of(&flakes, "fqn"),
            Some(&FlakeValue::String(
                "hdfc-core.postgres.payments.upi_transactions".into()
            ))
        );
        assert_eq!(
            value_of(&flakes, "description"),
            Some(&FlakeValue::String("UPI transaction ledger".into()))
        );
        assert_eq!(
            value_of(&flakes, "type"),
            Some(&FlakeValue::String("table".into()))
        );
        assert_eq!(
            value_of(&flakes, "updatedBy"),
            Some(&FlakeValue::String("asha".into()))
        );
        assert_eq!(
            value_of(&flakes, "deleted"),
            Some(&FlakeValue::Boolean(false))
        );
        assert_eq!(
            value_of(&flakes, "version"),
            Some(&FlakeValue::String("0.1".into())),
            "a version is an ordered pair, and `0.10` must not equal `0.1`"
        );
    }

    /// Absence is not a null assertion. Emitting `description = null` would
    /// make "nobody has written one" indistinguishable from "someone
    /// deliberately cleared it", and the graph would then answer a question
    /// the catalog cannot actually answer.
    #[test]
    fn an_absent_field_produces_no_flake_at_all() {
        let mut asset = table();
        asset.description = None;
        let flakes = asset_to_flakes(&asset, 7);

        assert!(
            value_of(&flakes, "description").is_none(),
            "a None description must produce no flake, not a null one"
        );
    }

    #[test]
    fn every_flake_shares_the_transaction_time_it_was_given() {
        let flakes = asset_to_flakes(&table(), 42);
        assert!(!flakes.is_empty());
        assert!(
            flakes.iter().all(|f| f.t == 42),
            "one logical change is one t — otherwise 'the state after change N' \
             is not well defined"
        );
    }

    #[test]
    fn every_flake_is_an_assertion_about_the_entity_itself() {
        let asset = table();
        let expected = Sid::new(namespace::DSC, asset.id.to_string());
        let flakes = asset_to_flakes(&asset, 1);

        assert!(flakes.iter().all(|f| f.s == expected), "wrong subject");
        assert!(flakes.iter().all(|f| f.op), "projection asserts");
        assert!(
            flakes.iter().all(|f| f.cx.is_none()),
            "catalog facts belong in the default graph, not a named one"
        );
    }

    /// The parent predicate names the parent's *kind*, so `dsc:parentSchema`
    /// on a table and `dsc:parentTable` on a column. A single `dsc:parent`
    /// would make "every table in this schema" and "every column in this
    /// table" the same query, which they are not.
    #[test]
    fn the_hierarchy_projects_as_a_typed_reference_to_the_parent() {
        let parent = Uuid::from_u128(2);

        let table_flakes = asset_to_flakes(&table(), 1);
        assert_eq!(
            value_of(&table_flakes, "parentSchema"),
            Some(&FlakeValue::Ref(Sid::new(
                namespace::DSC,
                parent.to_string()
            ))),
            "a table's parent is a schema"
        );

        let mut column = table();
        column.kind = AssetKind::Column;
        let column_flakes = asset_to_flakes(&column, 1);
        assert_eq!(
            value_of(&column_flakes, "parentTable"),
            Some(&FlakeValue::Ref(Sid::new(
                namespace::DSC,
                parent.to_string()
            ))),
            "a column's parent is a table"
        );
    }

    /// A `Ref`, never a string. OPST is a reference-only index, so an endpoint
    /// stored as text is unreachable by reverse traversal — the query "what is
    /// in this schema" would degrade to a scan.
    #[test]
    fn the_parent_is_a_reference_not_a_string() {
        let flakes = asset_to_flakes(&table(), 1);
        let parent = value_of(&flakes, "parentSchema").expect("parent flake");
        assert!(
            parent.is_reference(),
            "{parent:?} must be a Ref for OPST reverse traversal to reach it"
        );
    }

    #[test]
    fn a_root_has_no_parent_flake() {
        let mut service = table();
        service.kind = AssetKind::Service;
        service.parent_id = None;
        let flakes = asset_to_flakes(&service, 1);

        assert!(
            !flakes.iter().any(|f| f.p.id.starts_with("parent")),
            "a service is a root and has nothing to point at"
        );
    }

    /// Timestamps project as `Instant`, not as formatted strings — a string
    /// date cannot be range-queried, and "every table updated since Monday" is
    /// the whole reason the field is in the graph.
    #[test]
    fn timestamps_project_as_instants() {
        let flakes = asset_to_flakes(&table(), 1);
        assert_eq!(
            value_of(&flakes, "createdAt"),
            Some(&FlakeValue::Instant(
                Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            ))
        );
        assert_eq!(
            value_of(&flakes, "updatedAt"),
            Some(&FlakeValue::Instant(
                Utc.timestamp_opt(1_700_000_500, 0).unwrap()
            ))
        );
    }

    #[test]
    fn a_tombstoned_asset_projects_its_deletion() {
        let mut asset = table();
        asset.deleted = true;
        asset.deleted_at = Some(Utc.timestamp_opt(1_700_001_000, 0).unwrap());
        let flakes = asset_to_flakes(&asset, 1);

        assert_eq!(
            value_of(&flakes, "deleted"),
            Some(&FlakeValue::Boolean(true))
        );
        assert_eq!(
            value_of(&flakes, "deletedAt"),
            Some(&FlakeValue::Instant(
                Utc.timestamp_opt(1_700_001_000, 0).unwrap()
            )),
            "when it was tombstoned is the fact a retention policy needs"
        );
    }

    #[test]
    fn a_live_asset_asserts_deleted_false_and_no_deleted_at() {
        let flakes = asset_to_flakes(&table(), 1);
        assert_eq!(
            value_of(&flakes, "deleted"),
            Some(&FlakeValue::Boolean(false)),
            "deleted is always asserted — its absence would be ambiguous"
        );
        assert!(value_of(&flakes, "deletedAt").is_none());
    }

    /// Free-form source properties are `Json`, so a warehouse's own vocabulary
    /// survives into the graph without this project having to normalise it.
    #[test]
    fn properties_project_as_json_when_present() {
        let mut asset = table();
        asset.properties = Some(serde_json::json!({ "dataType": "NUMERIC" }));
        let flakes = asset_to_flakes(&asset, 1);

        match value_of(&flakes, "properties") {
            Some(FlakeValue::Json(raw)) => {
                assert!(raw.contains("NUMERIC"), "got {raw}");
            }
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn no_predicate_is_emitted_twice() {
        let flakes = asset_to_flakes(&table(), 1);
        let mut predicates: Vec<&str> = flakes.iter().map(|f| f.p.id.as_str()).collect();
        let total = predicates.len();
        predicates.sort_unstable();
        predicates.dedup();
        assert_eq!(
            predicates.len(),
            total,
            "a duplicated predicate makes cardinality-one meaningless"
        );
    }

    #[test]
    fn every_predicate_is_in_this_projects_vocabulary() {
        let flakes = asset_to_flakes(&table(), 1);
        assert!(
            flakes.iter().all(|f| f.p.namespace_code == namespace::DSC),
            "catalog predicates are dsc:, and namespace 0 would be unqueryable"
        );
    }

    // ---- relationships ----

    fn relationship() -> crate::Relationship {
        crate::Relationship {
            id: Uuid::from_u128(50),
            from_entity_type: "table".to_string(),
            from_entity_id: Uuid::from_u128(1),
            relationship_type: "feeds".to_string(),
            to_entity_type: "table".to_string(),
            to_entity_id: Uuid::from_u128(2),
            created_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        }
    }

    /// Reification: every flake hangs off the *relationship's* own node, not
    /// off either endpoint. That is what gives an edge somewhere to carry a
    /// payload — a flat `(a) feeds (b)` assertion has no subject to attach
    /// confidence or provenance to.
    #[test]
    fn a_relationship_projects_as_a_node_of_its_own() {
        let edge = relationship();
        let flakes = relationship_to_flakes(&edge, 1);

        assert!(flakes.len() >= 4, "got {}", flakes.len());
        let subject = relationship_sid(&edge);
        assert!(
            flakes.iter().all(|f| f.s == subject),
            "every flake must hang off the relationship's own node"
        );
        assert!(
            flakes.iter().all(|f| f.t == 1 && f.op),
            "one change, one t, all assertions"
        );
    }

    /// OPST is reference-only, so an endpoint stored as text is unreachable by
    /// reverse traversal — and "what feeds this table" is exactly a lookup by
    /// object.
    #[test]
    fn both_endpoints_are_references_not_strings() {
        let edge = relationship();
        let flakes = relationship_to_flakes(&edge, 1);

        for predicate in ["fromEntity", "toEntity"] {
            let value =
                value_of(&flakes, predicate).unwrap_or_else(|| panic!("{predicate} is missing"));
            assert!(
                value.is_reference(),
                "{predicate} is {value:?}; OPST cannot reach a literal"
            );
        }
    }

    #[test]
    fn the_endpoints_point_at_the_entities_they_name() {
        let edge = relationship();
        let flakes = relationship_to_flakes(&edge, 1);

        assert_eq!(
            value_of(&flakes, "fromEntity"),
            Some(&FlakeValue::Ref(Sid::new(
                namespace::DSC,
                edge.from_entity_id.to_string()
            )))
        );
        assert_eq!(
            value_of(&flakes, "toEntity"),
            Some(&FlakeValue::Ref(Sid::new(
                namespace::DSC,
                edge.to_entity_id.to_string()
            )))
        );
    }

    /// Direction is the whole meaning of a lineage edge. A projection that
    /// treated the endpoints as interchangeable would make "what feeds this"
    /// and "what this feeds" the same question.
    #[test]
    fn the_two_endpoints_are_not_interchangeable() {
        let flakes = relationship_to_flakes(&relationship(), 1);
        assert_ne!(
            value_of(&flakes, "fromEntity"),
            value_of(&flakes, "toEntity"),
            "a reversed edge is a different fact"
        );
    }

    /// `rdf:type` rather than `dsc:type`: the standard vocabulary is what lets
    /// Epic 9 export this without a translation table.
    #[test]
    fn a_relationship_declares_its_type_in_the_standard_vocabulary() {
        let flakes = relationship_to_flakes(&relationship(), 1);
        let typed = flakes
            .iter()
            .find(|f| f.p.namespace_code == namespace::RDF && f.p.id == "type")
            .expect("rdf:type is missing");
        assert_eq!(typed.o, FlakeValue::Ref(Sid::dsc("Relationship")));
    }

    #[test]
    fn the_relationship_type_and_endpoint_kinds_travel_with_the_edge() {
        let flakes = relationship_to_flakes(&relationship(), 1);
        assert_eq!(
            value_of(&flakes, "relType"),
            Some(&FlakeValue::String("feeds".into()))
        );
        // Without these a traversal must resolve both endpoints just to learn
        // whether an edge is table-to-table — two extra reads per edge.
        assert_eq!(
            value_of(&flakes, "fromEntityType"),
            Some(&FlakeValue::String("table".into()))
        );
        assert_eq!(
            value_of(&flakes, "toEntityType"),
            Some(&FlakeValue::String("table".into()))
        );
    }

    /// Two relationships between the same pair are two nodes. Collapsing them
    /// would lose the second, and "feeds" plus "sameAs" between one pair is a
    /// legitimate thing for a catalog to record.
    #[test]
    fn two_relationships_between_the_same_pair_are_distinct_nodes() {
        let first = relationship();
        let mut second = relationship();
        second.id = Uuid::from_u128(51);
        second.relationship_type = "sameAs".to_string();

        assert_ne!(relationship_sid(&first), relationship_sid(&second));
    }

    // ---- reconstruction ----

    /// The round trip that the whole flake model exists for: project an entity
    /// out, read it back, get the same entity.
    #[test]
    fn an_asset_survives_projection_and_reconstruction() {
        let original = table();
        let flakes = asset_to_flakes(&original, 1);

        let rebuilt = asset_from_flakes(original.id, &flakes).expect("should reconstruct");

        assert_eq!(rebuilt.id, original.id);
        assert_eq!(rebuilt.kind, original.kind);
        assert_eq!(rebuilt.name, original.name);
        assert_eq!(rebuilt.fully_qualified_name, original.fully_qualified_name);
        assert_eq!(rebuilt.parent_id, original.parent_id);
        assert_eq!(rebuilt.description, original.description);
        assert_eq!(rebuilt.version, original.version);
        assert_eq!(rebuilt.updated_by, original.updated_by);
        assert_eq!(rebuilt.deleted, original.deleted);
        assert_eq!(rebuilt.created_at, original.created_at);
        assert_eq!(rebuilt.updated_at, original.updated_at);
    }

    #[test]
    fn every_asset_kind_round_trips() {
        for kind in AssetKind::ALL {
            let mut asset = table();
            asset.kind = kind;
            if kind.parent_kind().is_none() {
                asset.parent_id = None;
            }
            let rebuilt = asset_from_flakes(asset.id, &asset_to_flakes(&asset, 1))
                .unwrap_or_else(|| panic!("{kind} should reconstruct"));
            assert_eq!(rebuilt.kind, kind);
            assert_eq!(
                rebuilt.parent_id, asset.parent_id,
                "{kind} lost its parent — the predicate is typed by parent kind"
            );
        }
    }

    #[test]
    fn a_tombstoned_asset_reconstructs_as_deleted() {
        let mut asset = table();
        asset.deleted = true;
        asset.deleted_at = Some(Utc.timestamp_opt(1_700_001_000, 0).unwrap());

        let rebuilt = asset_from_flakes(asset.id, &asset_to_flakes(&asset, 1)).expect("rebuild");
        assert!(rebuilt.deleted);
        assert_eq!(rebuilt.deleted_at, asset.deleted_at);
    }

    /// A subject with no flakes did not exist at that transaction time. An
    /// asset synthesised from nothing would be a state the catalog was never
    /// in, which is precisely the lie time-travel must not tell.
    #[test]
    fn a_subject_with_no_flakes_reconstructs_to_nothing() {
        assert!(asset_from_flakes(Uuid::from_u128(1), &[]).is_none());
    }

    /// Flakes about *other* subjects must not contribute. Reading the graph
    /// at a past time returns many subjects' flakes at once, and a
    /// reconstruction that ignored the subject would blend them.
    #[test]
    fn flakes_belonging_to_another_subject_are_ignored() {
        let mine = table();
        let mut theirs = table();
        theirs.id = Uuid::from_u128(99);
        theirs.name = "someone_elses_table".to_string();

        let mut mixed = asset_to_flakes(&theirs, 1);
        mixed.extend(asset_to_flakes(&mine, 1));

        let rebuilt = asset_from_flakes(mine.id, &mixed).expect("rebuild");
        assert_eq!(rebuilt.name, "upi_transactions", "blended two subjects");
    }

    /// An incomplete set is not an asset. Half a projection reconstructed into
    /// a plausible-looking entity is worse than an honest absence.
    #[test]
    fn a_set_missing_identity_facts_reconstructs_to_nothing() {
        let asset = table();
        for required in ["type", "name", "fqn"] {
            let partial: Vec<Flake> = asset_to_flakes(&asset, 1)
                .into_iter()
                .filter(|f| f.p.id != required)
                .collect();
            assert!(
                asset_from_flakes(asset.id, &partial).is_none(),
                "reconstructed an asset with no {required}"
            );
        }
    }

    /// Source properties are where a column's data type and nullability live,
    /// so a reconstruction that dropped them would answer "what type was this
    /// column before the migration" with silence — which is the single most
    /// likely question anyone asks the time slider.
    #[test]
    fn properties_survive_reconstruction() {
        let mut asset = table();
        asset.kind = AssetKind::Column;
        asset.properties = Some(serde_json::json!({ "dataType": "NUMERIC", "nullable": false }));

        let rebuilt = asset_from_flakes(asset.id, &asset_to_flakes(&asset, 1)).expect("rebuild");
        let properties = rebuilt.properties.expect("properties must come back");
        assert_eq!(properties["dataType"], "NUMERIC");
        assert_eq!(properties["nullable"], false);
    }

    #[test]
    fn an_asset_without_properties_reconstructs_without_them() {
        let asset = table();
        assert!(asset.properties.is_none(), "fixture assumption");
        let rebuilt = asset_from_flakes(asset.id, &asset_to_flakes(&asset, 1)).expect("rebuild");
        assert!(
            rebuilt.properties.is_none(),
            "absent must stay absent, not become an empty object"
        );
    }

    /// The reconstruction reads state, never the diff that produced it — the
    /// change description belongs to the version row that recorded it, and
    /// inventing one here would attribute a change to the wrong transaction.
    #[test]
    fn reconstruction_carries_no_change_description() {
        let asset = table();
        let rebuilt = asset_from_flakes(asset.id, &asset_to_flakes(&asset, 1)).expect("rebuild");
        assert!(rebuilt.change_description.is_none());
    }

    // ---- updates ----

    /// An update retracts the old value and asserts the new one, both at the
    /// same `t`. Asserting without retracting would leave two current values
    /// for a single-valued predicate; retracting without asserting would blank
    /// the field.
    #[test]
    fn an_update_retracts_the_old_value_and_asserts_the_new_one() {
        let before = table();
        let mut after = before.clone();
        after.description = Some("UPI ledger, NPCI-settled".to_string());

        let changes = asset_update_flakes(&before, &after, 9);

        let retractions: Vec<_> = changes.iter().filter(|f| !f.op).collect();
        let assertions: Vec<_> = changes.iter().filter(|f| f.op).collect();

        assert_eq!(retractions.len(), 1, "got {retractions:?}");
        assert_eq!(
            retractions[0].o,
            FlakeValue::String("UPI transaction ledger".into()),
            "the retraction must name the value being withdrawn"
        );
        assert_eq!(assertions.len(), 1, "got {assertions:?}");
        assert_eq!(
            assertions[0].o,
            FlakeValue::String("UPI ledger, NPCI-settled".into())
        );
        assert!(
            changes.iter().all(|f| f.t == 9),
            "both halves of one change share its t"
        );
    }

    /// The property that makes a nightly connector safe: re-projecting an
    /// unchanged entity must write nothing at all.
    #[test]
    fn an_update_that_changes_nothing_produces_no_flakes() {
        let before = table();
        let after = before.clone();
        assert!(
            asset_update_flakes(&before, &after, 9).is_empty(),
            "an unchanged entity must not inflate history"
        );
    }

    /// Clearing a field is a retraction with no matching assertion — which is
    /// exactly how the graph represents "this no longer has a description".
    #[test]
    fn clearing_a_field_retracts_without_asserting() {
        let before = table();
        let mut after = before.clone();
        after.description = None;

        let changes = asset_update_flakes(&before, &after, 9);
        assert_eq!(changes.len(), 1);
        assert!(!changes[0].op);
        assert_eq!(changes[0].p.id, "description");
    }

    /// And setting a field that was empty is an assertion with nothing to
    /// retract.
    #[test]
    fn setting_a_previously_absent_field_asserts_without_retracting() {
        let mut before = table();
        before.description = None;
        let after = table();

        let changes = asset_update_flakes(&before, &after, 9);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].op);
        assert_eq!(changes[0].p.id, "description");
    }

    /// A rename changes both the name and the FQN, and the FQN change must be
    /// projected too — otherwise a time-travel query would show the new name
    /// under the old path.
    #[test]
    fn a_rename_projects_both_the_name_and_the_fully_qualified_name() {
        let before = table();
        let mut after = before.clone();
        after.name = "upi_txn".to_string();
        after.fully_qualified_name = "hdfc-core.postgres.payments.upi_txn".to_string();

        let changed: Vec<&str> = asset_update_flakes(&before, &after, 9)
            .iter()
            .map(|f| f.p.id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|s| Box::leak(s.into_boxed_str()) as &str)
            .collect();

        assert_eq!(changed, vec!["fqn", "name"]);
    }

    /// Every retraction must be paired against the value that was actually
    /// stored. A retraction naming the *new* value would withdraw nothing,
    /// leaving both values current.
    #[test]
    fn retractions_carry_the_old_value_never_the_new_one() {
        let before = table();
        let mut after = before.clone();
        after.name = "renamed".to_string();

        for flake in asset_update_flakes(&before, &after, 9) {
            if !flake.op && flake.p.id == "name" {
                assert_eq!(flake.o, FlakeValue::String("upi_transactions".into()));
            }
        }
    }
}
