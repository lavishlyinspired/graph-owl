//! Labelled property graph model and the bidirectional flake ↔ LPG mapping —
//! Epic 7c.
//!
//! **A projection, not a second store.** Flakes stay the single source of graph
//! truth; this crate is a pure mapping evaluated on demand. A materialized
//! parallel property graph would be a second thing to keep consistent, which is
//! the failure mode Epic 4 spent its complexity budget avoiding.
//!
//! The mapping is cheap for one specific reason: **Epic 4 already reifies every
//! relationship.** Edge properties are the defining LPG feature and the thing
//! plain RDF cannot express without reification — so the expensive half is
//! already built and paid for. What is left is vocabulary.
//!
//! Two rules govern everything here:
//!
//! 1. **Losses are enumerated, not discovered.** Where the flake model cannot
//!    survive the trip, the code reports it in a [`MappingReport`] rather than
//!    dropping it. A silently lossy round trip is worse than a refused one,
//!    because the caller keeps the result and believes it.
//! 2. **Order is deterministic.** [`PropertyMap`] is a `BTreeMap`, so two
//!    projections of the same input serialize to the same bytes. A `HashMap`
//!    would make every downstream serialization — Bolt frames, `GraphML`,
//!    fixtures — unstable for no gain.

pub mod element_id;

pub use element_id::{ElementId, ElementIdError};

use std::collections::BTreeMap;

use graph_owl_core::flake::{Flake, FlakeValue, Sid};

/// A node label. Derived from `dsc:type`, **never stored twice** — a separate
/// label list would drift from the types on the first schema change.
pub type Label = String;

/// An edge type, from `dsc:relType`.
pub type EdgeType = String;

/// Property names beginning with `_` belong to the projection.
///
/// Reserved rather than merely conventional: `_graph` and `_t` carry Epic 4's
/// named-graph scoping and transaction time through a model that has no place
/// for either, and a user property of the same name would silently replace one.
pub const RESERVED_PREFIX: char = '_';

/// The named graph a fact belongs to — `graph:extraction`, `graph:reasoning`.
pub const GRAPH_KEY: &str = "_graph";

/// Transaction time. Read-only through the projection: writing it would let a
/// caller forge history, which is the one thing an append-only log exists to
/// prevent.
pub const TIME_KEY: &str = "_t";

/// A property value: one variant per [`FlakeValue`], plus the LPG-native ones.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum PropertyValue {
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    DateTime(chrono::DateTime<chrono::Utc>),
    /// Whole seconds, matching `FlakeValue::Duration`.
    Duration(i64),
    /// A repeated predicate.
    ///
    /// **Order is not meaningful.** Flakes are a set, so any order here would be
    /// an artifact of storage rather than of the data — which is why the plan
    /// records list ordering as something that does not round-trip.
    List(Vec<PropertyValue>),
    /// A `Ref` in **property** position rather than as an edge.
    ///
    /// The one genuinely lossy direction: LPG has no property whose value is
    /// another element, so this becomes a handle and a consumer that does not
    /// know to resolve it sees a string. Reported as
    /// [`LossyMapping::RefInProperty`].
    ElementRef(ElementId),
}

impl PropertyValue {
    /// Project one flake object.
    ///
    /// **Exhaustive on purpose.** A `_` arm would mean a new `FlakeValue`
    /// variant projects as something plausible and wrong; without one, adding a
    /// variant is a compile error at exactly the place that has to decide.
    ///
    /// `String` and `Json` produce the same value and are kept as separate arms
    /// for the same reason: they are separate *decisions*. Merging them would
    /// read as "these are the same thing", when in fact one is a string and the
    /// other is JSON that this projection has deliberately chosen not to parse —
    /// and the next person to touch this needs to see that choice, not infer it.
    #[allow(clippy::match_same_arms)]
    #[must_use]
    pub fn from_flake(value: &FlakeValue) -> Self {
        match value {
            FlakeValue::Ref(sid) => PropertyValue::ElementRef(ElementId::encode(sid)),
            FlakeValue::String(text) => PropertyValue::String(text.clone()),
            FlakeValue::Boolean(flag) => PropertyValue::Boolean(*flag),
            FlakeValue::Int(number) => PropertyValue::Integer(*number),
            FlakeValue::Float(number) => PropertyValue::Float(*number),
            FlakeValue::Instant(instant) => PropertyValue::DateTime(*instant),
            // JSON arrives as a string and leaves as one. Parsing it into a map
            // would make the round trip lossy in the *other* direction — key
            // order and number formatting do not survive a reparse, so what came
            // back would differ from what was stored.
            FlakeValue::Json(text) => PropertyValue::String(text.clone()),
            FlakeValue::Bytes(bytes) => PropertyValue::Bytes(bytes.clone()),
            FlakeValue::Uuid(id) => PropertyValue::String(id.to_string()),
            FlakeValue::Duration(seconds) => PropertyValue::Duration(*seconds),
        }
    }
}

/// A node's or edge's properties, in deterministic order.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
#[serde(transparent)]
pub struct PropertyMap(BTreeMap<String, PropertyValue>);

impl PropertyMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a property the *projection* owns.
    pub fn insert_reserved(&mut self, key: &str, value: PropertyValue) {
        self.0.insert(key.to_string(), value);
    }

    /// Insert a property from the data.
    ///
    /// **Refuses reserved names.** A user property called `_graph` would
    /// silently replace the named-graph marker, and the loss would be invisible
    /// because the value is a plausible string either way.
    ///
    /// A repeated key grows into a list rather than replacing, because
    /// last-write-wins would drop values with no report.
    ///
    /// # Errors
    ///
    /// [`MappingError::ReservedPropertyName`] naming the key.
    pub fn insert_user(&mut self, key: &str, value: PropertyValue) -> Result<(), MappingError> {
        if key.starts_with(RESERVED_PREFIX) {
            return Err(MappingError::ReservedPropertyName(key.to_string()));
        }
        match self.0.remove(key) {
            None => {
                self.0.insert(key.to_string(), value);
            }
            Some(PropertyValue::List(mut existing)) => {
                existing.push(value);
                self.0
                    .insert(key.to_string(), PropertyValue::List(existing));
            }
            Some(first) => {
                self.0
                    .insert(key.to_string(), PropertyValue::List(vec![first, value]));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.0.get(key)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Keys in sorted order — the ordering guarantee, made observable.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }
}

/// A node as a property-graph consumer sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LpgNode {
    pub element_id: ElementId,
    /// From `dsc:type`. **Sorted and deduplicated**, so two projections agree.
    pub labels: Vec<Label>,
    pub properties: PropertyMap,
}

/// An edge, projected from a reified relationship.
///
/// **The two-hop reification is an implementation detail of the flake layer.** A
/// consumer that sees `(:Table)-[:FEEDS {confidence: 0.9}]->(:Table)` is seeing
/// the truth of the model; one that sees three nodes is seeing the encoding.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LpgEdge {
    /// The reified relationship's own `Sid` — so an edge stays addressable as a
    /// node when something needs to link *to* it (provenance, review, memory).
    pub element_id: ElementId,
    pub edge_type: EdgeType,
    pub start: ElementId,
    pub end: ElementId,
    pub properties: PropertyMap,
}

/// Something the mapping could not carry across.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LossyMapping {
    /// A `Ref` in property position became a handle string.
    RefInProperty { subject: String, predicate: String },
    /// Facts from several named graphs collapsed onto one element. `_graph`
    /// holds one value, so a subject asserted in two graphs loses the
    /// distinction.
    NamedGraphCollapse {
        subject: String,
        graphs: Vec<String>,
    },
    /// A typed literal that LPG has no type for, projected as a string.
    ///
    /// **The one loss a round trip cannot undo**, and it is a loss of *type*
    /// rather than of value: `FlakeValue::Uuid` and `FlakeValue::Json` both
    /// become `PropertyValue::String`, and coming back there is nothing left to
    /// say which they were. The value survives exactly; the tag does not.
    ///
    /// Reported on the **forward** pass, because that is where the information
    /// goes — the reverse pass has nothing left to notice.
    TypeNarrowed {
        subject: String,
        predicate: String,
        /// `uuid` or `json`.
        from: &'static str,
    },
}

/// What a projection could not carry.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingReport {
    pub lossy: Vec<LossyMapping>,
}

impl MappingReport {
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.lossy.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    /// A subject with no `dsc:type`.
    ///
    /// **An error rather than an unlabelled node.** Every LPG query language
    /// matches on labels, so a node with none is invisible to everything but a
    /// full scan — which reads as "the data is not there" rather than "the
    /// projection could not classify it".
    #[error(
        "`{0}` has no type assertion, so it has no label; an unlabelled node is \
         invisible to every label-matching query and would read as missing data"
    )]
    Untyped(String),
    #[error(
        "`{0}` starts with `_`, which the projection reserves for `_graph` and \
         `_t`; a user property of that name would silently replace one"
    )]
    ReservedPropertyName(String),
    #[error("`{0}` is not a reified relationship: it has no `{1}`")]
    NotARelationship(String, &'static str),
    /// An element reference that does not decode.
    ///
    /// **An error rather than a dropped property.** A reference that silently
    /// vanished would turn a broken import into a successful one, and the
    /// missing edge would be discovered by whoever queried for it later.
    #[error("`{0}` is not an element id this server issued, so it names nothing")]
    UnresolvableHandle(String),
    /// A list inside a list. The flake model has no nesting: a repeated
    /// predicate is a flat set of facts, so there is no encoding for this.
    #[error("a nested list has no flake encoding; a repeated predicate is flat")]
    NestedList,
}

/// The predicates the mapping reads. Named once so the projection and its tests
/// cannot disagree about the vocabulary.
pub mod predicate {
    /// The type assertion that becomes a label.
    pub const TYPE: &str = "type";
    /// The relationship's kind — becomes the edge type.
    pub const REL_TYPE: &str = "relType";
    /// The edge's tail.
    pub const FROM_ENTITY: &str = "fromEntity";
    /// The edge's head.
    pub const TO_ENTITY: &str = "toEntity";
}

/// Project one subject's flakes into a node.
///
/// `flakes` must already be resolved to the instant the caller wants — **time
/// travel passes straight through**, because a historical property-graph view is
/// this same code over different input. That capability falls out of Epic 4 for
/// free rather than needing design here.
///
/// # Errors
///
/// [`MappingError::Untyped`] when nothing asserts a type;
/// [`MappingError::ReservedPropertyName`] when a predicate collides with the
/// projection's own keys.
pub fn node_from_flakes(
    subject: &Sid,
    flakes: &[Flake],
    report: &mut MappingReport,
) -> Result<LpgNode, MappingError> {
    let mut labels = Vec::new();
    let mut properties = PropertyMap::new();
    let mut graphs: Vec<String> = Vec::new();
    let mut newest: Option<i64> = None;

    for flake in flakes.iter().filter(|flake| &flake.s == subject) {
        // A retraction is not a fact. Projecting one would show a consumer a
        // value that was explicitly withdrawn.
        if !flake.op {
            continue;
        }
        newest = Some(newest.map_or(flake.t, |t: i64| t.max(flake.t)));

        if let Some(graph) = &flake.cx {
            let name = graph.id.clone();
            if !graphs.contains(&name) {
                graphs.push(name);
            }
        }

        if flake.p.id == predicate::TYPE {
            // A type whose object is not a reference is not a type — it is a
            // literal sharing the predicate name, and treating it as a label
            // would invent one.
            if let FlakeValue::Ref(class) = &flake.o {
                labels.push(class.id.clone());
            }
            continue;
        }

        match &flake.o {
            FlakeValue::Ref(_) => report.lossy.push(LossyMapping::RefInProperty {
                subject: subject.id.clone(),
                predicate: flake.p.id.clone(),
            }),
            // The value survives; the type tag does not. See `TypeNarrowed`.
            FlakeValue::Uuid(_) | FlakeValue::Json(_) => {
                report.lossy.push(LossyMapping::TypeNarrowed {
                    subject: subject.id.clone(),
                    predicate: flake.p.id.clone(),
                    from: if matches!(&flake.o, FlakeValue::Uuid(_)) {
                        "uuid"
                    } else {
                        "json"
                    },
                });
            }
            _ => {}
        }
        properties.insert_user(&flake.p.id, PropertyValue::from_flake(&flake.o))?;
    }

    if labels.is_empty() {
        return Err(MappingError::Untyped(subject.id.clone()));
    }
    labels.sort();
    labels.dedup();

    if graphs.len() > 1 {
        graphs.sort();
        report.lossy.push(LossyMapping::NamedGraphCollapse {
            subject: subject.id.clone(),
            graphs: graphs.clone(),
        });
    }
    if let Some(first) = graphs.first() {
        properties.insert_reserved(GRAPH_KEY, PropertyValue::String(first.clone()));
    }
    if let Some(t) = newest {
        properties.insert_reserved(TIME_KEY, PropertyValue::Integer(t));
    }

    Ok(LpgNode {
        element_id: ElementId::encode(subject),
        labels,
        properties,
    })
}

/// Project a reified relationship into an edge.
///
/// # Errors
///
/// [`MappingError::NotARelationship`] when the subject lacks `relType`,
/// `fromEntity` or `toEntity` — a reification missing an endpoint is an edge to
/// nowhere, which a traversal would count and then fail to follow.
pub fn edge_from_reified(
    relationship: &Sid,
    flakes: &[Flake],
    report: &mut MappingReport,
) -> Result<LpgEdge, MappingError> {
    let mut edge_type = None;
    let mut start = None;
    let mut end = None;
    let mut properties = PropertyMap::new();
    let mut graphs: Vec<String> = Vec::new();
    let mut newest: Option<i64> = None;

    for flake in flakes.iter().filter(|flake| &flake.s == relationship) {
        if !flake.op {
            continue;
        }
        newest = Some(newest.map_or(flake.t, |t: i64| t.max(flake.t)));
        if let Some(graph) = &flake.cx {
            let name = graph.id.clone();
            if !graphs.contains(&name) {
                graphs.push(name);
            }
        }
        match flake.p.id.as_str() {
            predicate::REL_TYPE => {
                edge_type = match &flake.o {
                    FlakeValue::Ref(kind) => Some(kind.id.clone()),
                    FlakeValue::String(kind) => Some(kind.clone()),
                    _ => edge_type,
                };
            }
            predicate::FROM_ENTITY => {
                if let FlakeValue::Ref(from) = &flake.o {
                    start = Some(ElementId::encode(from));
                }
            }
            predicate::TO_ENTITY => {
                if let FlakeValue::Ref(to) = &flake.o {
                    end = Some(ElementId::encode(to));
                }
            }
            // The reification marker is not an edge property. Carrying it would
            // put `Relationship` in the properties of every edge in the graph.
            predicate::TYPE => {}
            other => {
                if matches!(&flake.o, FlakeValue::Ref(_)) {
                    report.lossy.push(LossyMapping::RefInProperty {
                        subject: relationship.id.clone(),
                        predicate: other.to_string(),
                    });
                }
                properties.insert_user(other, PropertyValue::from_flake(&flake.o))?;
            }
        }
    }

    // **An inferred edge must not come back looking asserted.** Epic 6 writes
    // derived relationships into `graph:reasoning`, and an agent or a console
    // that cannot tell them from asserted ones is being shown inference as
    // fact. The node path had this from the start; the edge path did not, and a
    // surviving mutant is what surfaced the asymmetry.
    if graphs.len() > 1 {
        graphs.sort();
        report.lossy.push(LossyMapping::NamedGraphCollapse {
            subject: relationship.id.clone(),
            graphs: graphs.clone(),
        });
    }
    if let Some(first) = graphs.first() {
        properties.insert_reserved(GRAPH_KEY, PropertyValue::String(first.clone()));
    }
    if let Some(t) = newest {
        properties.insert_reserved(TIME_KEY, PropertyValue::Integer(t));
    }

    Ok(LpgEdge {
        element_id: ElementId::encode(relationship),
        edge_type: edge_type.ok_or_else(|| {
            MappingError::NotARelationship(relationship.id.clone(), predicate::REL_TYPE)
        })?,
        start: start.ok_or_else(|| {
            MappingError::NotARelationship(relationship.id.clone(), predicate::FROM_ENTITY)
        })?,
        end: end.ok_or_else(|| {
            MappingError::NotARelationship(relationship.id.clone(), predicate::TO_ENTITY)
        })?,
        properties,
    })
}

/// One property value back into a flake object.
///
/// **Not the exact inverse of [`PropertyValue::from_flake`], and it cannot be.**
/// `Uuid` and `Json` both project to `String`, so coming back there is nothing
/// left to distinguish them from a string that was always a string. The forward
/// pass reports that as [`LossyMapping::TypeNarrowed`] precisely because this
/// function has no way to.
///
/// # Errors
///
/// [`MappingError::UnresolvableHandle`] when an element reference is not a
/// handle this server issued — **an error rather than a dropped property**,
/// because a reference that silently vanished would turn a broken import into a
/// successful one.
fn flake_value_from(value: &PropertyValue) -> Result<FlakeValue, MappingError> {
    Ok(match value {
        PropertyValue::Boolean(flag) => FlakeValue::Boolean(*flag),
        PropertyValue::Integer(number) => FlakeValue::Int(*number),
        PropertyValue::Float(number) => FlakeValue::Float(*number),
        PropertyValue::String(text) => FlakeValue::String(text.clone()),
        PropertyValue::Bytes(bytes) => FlakeValue::Bytes(bytes.clone()),
        PropertyValue::DateTime(instant) => FlakeValue::Instant(*instant),
        PropertyValue::Duration(seconds) => FlakeValue::Duration(*seconds),
        PropertyValue::ElementRef(handle) => FlakeValue::Ref(
            handle
                .decode()
                .map_err(|_| MappingError::UnresolvableHandle(handle.to_string()))?,
        ),
        // A list is several flakes on one predicate, so it is expanded by the
        // caller rather than converted here — reaching this arm means a list
        // nested inside a list, which the flake model cannot express.
        PropertyValue::List(_) => return Err(MappingError::NestedList),
    })
}

/// The named graph a reserved `_graph` property asks for.
fn context_of(properties: &PropertyMap) -> Option<Sid> {
    match properties.get(GRAPH_KEY) {
        Some(PropertyValue::String(name)) => Some(Sid::dsc(name.clone())),
        _ => None,
    }
}

/// Turn one property into its flakes, expanding a list into several.
fn property_flakes(
    subject: &Sid,
    key: &str,
    value: &PropertyValue,
    cx: Option<Sid>,
    t: i64,
) -> Result<Vec<Flake>, MappingError> {
    let make = |object: FlakeValue| Flake {
        s: subject.clone(),
        p: Sid::dsc(key.to_string()),
        o: object,
        cx: cx.clone(),
        t,
        op: true,
    };
    match value {
        // **A list becomes several flakes on one predicate**, which is what it
        // was projected from. Order is not preserved and the plan says so:
        // flakes are a set, so any order here would be an artifact.
        PropertyValue::List(values) => values
            .iter()
            .map(|entry| Ok(make(flake_value_from(entry)?)))
            .collect(),
        single => Ok(vec![make(flake_value_from(single)?)]),
    }
}

/// A node back into flakes, at a transaction time the caller supplies.
///
/// **`t` is a parameter, never read from `_t`.** The projection exposes
/// transaction time read-only; taking it back from the payload would let a
/// caller forge history, which is the one thing an append-only log exists to
/// prevent. A `_t` in the properties is ignored rather than rejected — it is
/// what the forward pass put there, so round-tripping a node must not fail.
///
/// # Errors
///
/// [`MappingError::ReservedPropertyName`] for a `_`-prefixed key the projection
/// does not own; [`MappingError::UnresolvableHandle`] for a bad element
/// reference; [`MappingError::Untyped`] for a node with no labels.
pub fn flakes_from_node(node: &LpgNode, t: i64) -> Result<Vec<Flake>, MappingError> {
    if node.labels.is_empty() {
        return Err(MappingError::Untyped(node.element_id.to_string()));
    }
    let subject = node
        .element_id
        .decode()
        .map_err(|_| MappingError::UnresolvableHandle(node.element_id.to_string()))?;
    let cx = context_of(&node.properties);

    let mut flakes: Vec<Flake> = node
        .labels
        .iter()
        .map(|label| Flake {
            s: subject.clone(),
            p: Sid::dsc(predicate::TYPE),
            o: FlakeValue::Ref(Sid::dsc(label.clone())),
            cx: cx.clone(),
            t,
            op: true,
        })
        .collect();

    for key in node.properties.keys() {
        // `_graph` and `_t` are the projection's own and are consumed above or
        // deliberately dropped; any *other* `_` key came from somewhere it
        // should not have.
        if key == GRAPH_KEY || key == TIME_KEY {
            continue;
        }
        if key.starts_with(RESERVED_PREFIX) {
            return Err(MappingError::ReservedPropertyName(key.clone()));
        }
        let value = node
            .properties
            .get(key)
            .expect("a key the map just yielded");
        flakes.extend(property_flakes(&subject, key, value, cx.clone(), t)?);
    }
    Ok(flakes)
}

/// An edge back into the reified flakes that encode it.
///
/// Produces the four structural facts — `type`, `relType`, `fromEntity`,
/// `toEntity` — plus one per edge property. That reification is what lets an
/// edge carry properties at all, which is the whole reason this mapping is
/// cheap.
///
/// # Errors
///
/// As [`flakes_from_node`], plus an unresolvable endpoint handle.
pub fn flakes_from_edge(edge: &LpgEdge, t: i64) -> Result<Vec<Flake>, MappingError> {
    let relationship = edge
        .element_id
        .decode()
        .map_err(|_| MappingError::UnresolvableHandle(edge.element_id.to_string()))?;
    let start = edge
        .start
        .decode()
        .map_err(|_| MappingError::UnresolvableHandle(edge.start.to_string()))?;
    let end = edge
        .end
        .decode()
        .map_err(|_| MappingError::UnresolvableHandle(edge.end.to_string()))?;
    let cx = context_of(&edge.properties);

    let structural = |predicate: &str, object: FlakeValue| Flake {
        s: relationship.clone(),
        p: Sid::dsc(predicate.to_string()),
        o: object,
        cx: cx.clone(),
        t,
        op: true,
    };

    let mut flakes = vec![
        structural(predicate::TYPE, FlakeValue::Ref(Sid::dsc("Relationship"))),
        structural(
            predicate::REL_TYPE,
            FlakeValue::Ref(Sid::dsc(edge.edge_type.clone())),
        ),
        structural(predicate::FROM_ENTITY, FlakeValue::Ref(start)),
        structural(predicate::TO_ENTITY, FlakeValue::Ref(end)),
    ];

    for key in edge.properties.keys() {
        if key == GRAPH_KEY || key == TIME_KEY {
            continue;
        }
        if key.starts_with(RESERVED_PREFIX) {
            return Err(MappingError::ReservedPropertyName(key.clone()));
        }
        let value = edge
            .properties
            .get(key)
            .expect("a key the map just yielded");
        flakes.extend(property_flakes(&relationship, key, value, cx.clone(), t)?);
    }
    Ok(flakes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(id: &str) -> Sid {
        Sid::new(1, id)
    }

    fn dsc(id: &str) -> Sid {
        Sid::dsc(id)
    }

    fn fact(subject: &str, predicate: &str, object: FlakeValue) -> Flake {
        Flake {
            s: sid(subject),
            p: dsc(predicate),
            o: object,
            cx: None,
            t: 1,
            op: true,
        }
    }

    fn typed(subject: &str, class: &str) -> Flake {
        fact(subject, predicate::TYPE, FlakeValue::Ref(dsc(class)))
    }

    // ---- Slice A: nodes ----

    #[test]
    fn a_subject_projects_to_a_node_with_its_labels_and_properties() {
        let flakes = vec![
            typed("orders", "Table"),
            fact("orders", "name", FlakeValue::String("orders".into())),
            fact("orders", "rowCount", FlakeValue::Int(41_203)),
        ];
        let mut report = MappingReport::default();

        let node = node_from_flakes(&sid("orders"), &flakes, &mut report).expect("projects");

        assert_eq!(node.labels, vec!["Table".to_string()]);
        assert_eq!(
            node.properties.get("name"),
            Some(&PropertyValue::String("orders".into()))
        );
        assert_eq!(
            node.properties.get("rowCount"),
            Some(&PropertyValue::Integer(41_203))
        );
    }

    /// Several type assertions produce several labels — RDF permits it and LPG
    /// expects it.
    #[test]
    fn several_type_assertions_produce_several_labels() {
        let flakes = vec![
            typed("orders", "Table"),
            typed("orders", "Dataset"),
            typed("orders", "Table"),
        ];
        let mut report = MappingReport::default();

        let node = node_from_flakes(&sid("orders"), &flakes, &mut report).expect("projects");

        assert_eq!(
            node.labels,
            vec!["Dataset".to_string(), "Table".to_string()],
            "sorted and deduplicated: {node:?}"
        );
    }

    /// **A subject with no type is an error, not an unlabelled node.** Every LPG
    /// query language matches on labels, so an unlabelled node is invisible to
    /// everything but a full scan — which reads as missing data.
    #[test]
    fn a_subject_with_no_type_is_refused() {
        let flakes = vec![fact("orders", "name", FlakeValue::String("orders".into()))];

        let outcome = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default());

        assert!(
            matches!(outcome, Err(MappingError::Untyped(_))),
            "{outcome:?}"
        );
    }

    /// A type whose object is a literal is not a label — treating it as one
    /// would invent a label nobody asserted.
    #[test]
    fn a_literal_valued_type_does_not_become_a_label() {
        let flakes = vec![fact(
            "orders",
            predicate::TYPE,
            FlakeValue::String("Table".into()),
        )];

        let outcome = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default());

        assert!(
            matches!(outcome, Err(MappingError::Untyped(_))),
            "a string is not a class reference: {outcome:?}"
        );
    }

    /// **Every `FlakeValue` variant maps.** `from_flake` matches exhaustively so
    /// a new variant is a compile error; this asserts the decisions themselves.
    #[test]
    fn every_flake_value_variant_projects() {
        let moment = chrono::Utc::now();
        let id = uuid::Uuid::nil();
        let cases: Vec<(FlakeValue, PropertyValue)> = vec![
            (
                FlakeValue::Ref(sid("other")),
                PropertyValue::ElementRef(ElementId::encode(&sid("other"))),
            ),
            (
                FlakeValue::String("x".into()),
                PropertyValue::String("x".into()),
            ),
            (FlakeValue::Boolean(true), PropertyValue::Boolean(true)),
            (FlakeValue::Int(7), PropertyValue::Integer(7)),
            (FlakeValue::Float(1.5), PropertyValue::Float(1.5)),
            (FlakeValue::Instant(moment), PropertyValue::DateTime(moment)),
            (
                FlakeValue::Json("{\"a\":1}".into()),
                PropertyValue::String("{\"a\":1}".into()),
            ),
            (
                FlakeValue::Bytes(vec![1, 2]),
                PropertyValue::Bytes(vec![1, 2]),
            ),
            (FlakeValue::Uuid(id), PropertyValue::String(id.to_string())),
            (FlakeValue::Duration(60), PropertyValue::Duration(60)),
        ];

        for (flake_value, expected) in cases {
            assert_eq!(
                PropertyValue::from_flake(&flake_value),
                expected,
                "{flake_value:?} projected wrongly"
            );
        }
    }

    /// **A repeated predicate becomes a list**, growing rather than replacing —
    /// last-write-wins would drop values with no report.
    #[test]
    fn a_repeated_predicate_becomes_a_list() {
        let flakes = vec![
            typed("orders", "Table"),
            fact("orders", "tag", FlakeValue::String("pii".into())),
            fact("orders", "tag", FlakeValue::String("gold".into())),
            fact("orders", "tag", FlakeValue::String("daily".into())),
        ];

        let node = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default())
            .expect("projects");

        let Some(PropertyValue::List(tags)) = node.properties.get("tag") else {
            panic!("expected a list, got {:?}", node.properties.get("tag"));
        };
        assert_eq!(tags.len(), 3, "no value was dropped: {tags:?}");
    }

    /// **Deterministic order.** A `HashMap` would make every downstream
    /// serialization unstable for no gain.
    #[test]
    fn two_projections_of_the_same_input_are_byte_identical() {
        let flakes = vec![
            typed("orders", "Table"),
            fact("orders", "zebra", FlakeValue::Int(1)),
            fact("orders", "alpha", FlakeValue::Int(2)),
            fact("orders", "mike", FlakeValue::Int(3)),
        ];

        let first = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default())
            .expect("projects");
        let second = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default())
            .expect("projects");

        assert_eq!(
            serde_json::to_string(&first).expect("serialize"),
            serde_json::to_string(&second).expect("serialize")
        );
        let keys: Vec<&String> = first.properties.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "keys come out sorted: {keys:?}");
    }

    /// **A retraction is not a fact.** Projecting one would show a consumer a
    /// value that was explicitly withdrawn.
    #[test]
    fn a_retracted_flake_does_not_project() {
        let mut retracted = fact("orders", "name", FlakeValue::String("old".into()));
        retracted.op = false;
        let flakes = vec![typed("orders", "Table"), retracted];

        let node = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default())
            .expect("projects");

        assert_eq!(node.properties.get("name"), None, "{node:?}");
    }

    /// Only the subject's own flakes project — sweeping up its neighbours' would
    /// invent properties.
    #[test]
    fn another_subjects_flakes_do_not_leak_in() {
        let flakes = vec![
            typed("orders", "Table"),
            fact("customers", "name", FlakeValue::String("customers".into())),
        ];

        let node = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default())
            .expect("projects");

        assert_eq!(node.properties.get("name"), None, "{node:?}");
    }

    // ---- reserved names ----

    /// **A user property may not be called `_graph`.** It would silently replace
    /// the named-graph marker, and the loss is invisible because the value is a
    /// plausible string either way.
    #[test]
    fn a_user_property_may_not_use_a_reserved_name() {
        let mut properties = PropertyMap::new();

        for reserved in [GRAPH_KEY, TIME_KEY, "_anything"] {
            assert!(
                matches!(
                    properties.insert_user(reserved, PropertyValue::Integer(1)),
                    Err(MappingError::ReservedPropertyName(_))
                ),
                "`{reserved}` should be refused"
            );
        }
        assert!(properties.is_empty());
    }

    /// And an ordinary name is accepted — or the test above would pass against a
    /// map that refuses everything.
    #[test]
    fn an_ordinary_property_name_is_accepted() {
        let mut properties = PropertyMap::new();

        assert!(
            properties
                .insert_user("rowCount", PropertyValue::Integer(1))
                .is_ok()
        );
        assert_eq!(properties.len(), 1);
    }

    /// **Counting, at more than one count.**
    ///
    /// Every other assertion in this file happens to expect exactly one
    /// property, so a `len` that always returned `1` satisfied all of them —
    /// mutation testing found it. Zero, one and three, because no constant can
    /// satisfy all three.
    #[test]
    fn a_property_map_counts_what_it_holds() {
        let mut properties = PropertyMap::new();
        assert_eq!(properties.len(), 0);
        assert!(properties.is_empty());

        properties
            .insert_user("a", PropertyValue::Integer(1))
            .expect("accepted");
        assert_eq!(properties.len(), 1);
        assert!(
            !properties.is_empty(),
            "a map holding something is not empty"
        );

        properties
            .insert_user("b", PropertyValue::Integer(2))
            .expect("accepted");
        properties
            .insert_user("c", PropertyValue::Integer(3))
            .expect("accepted");
        assert_eq!(properties.len(), 3);
    }

    /// **A repeated key grows one entry rather than adding entries.** The list
    /// behaviour and the count have to agree, or a consumer paging by `len`
    /// walks off the end.
    #[test]
    fn a_repeated_key_does_not_grow_the_count() {
        let mut properties = PropertyMap::new();
        for value in 0..4 {
            properties
                .insert_user("tag", PropertyValue::Integer(value))
                .expect("accepted");
        }

        assert_eq!(properties.len(), 1, "one key: {properties:?}");
        let Some(PropertyValue::List(values)) = properties.get("tag") else {
            panic!("expected a list, got {:?}", properties.get("tag"));
        };
        assert_eq!(values.len(), 4, "four values under it");
    }

    /// **`keys` yields the keys**, not an empty iterator that happens to satisfy
    /// a comparison against its own sorted copy — which is all the determinism
    /// test alone required.
    #[test]
    fn keys_yields_every_key_in_sorted_order() {
        let mut properties = PropertyMap::new();
        for key in ["zebra", "alpha", "mike"] {
            properties
                .insert_user(key, PropertyValue::Integer(1))
                .expect("accepted");
        }

        let keys: Vec<&str> = properties.keys().map(String::as_str).collect();

        assert_eq!(keys, vec!["alpha", "mike", "zebra"]);
    }

    /// A named graph survives the trip.
    #[test]
    fn a_named_graph_survives_as_a_reserved_property() {
        let mut scoped = typed("orders", "Table");
        scoped.cx = Some(dsc("extraction"));
        let mut report = MappingReport::default();

        let node = node_from_flakes(&sid("orders"), &[scoped], &mut report).expect("projects");

        assert_eq!(
            node.properties.get(GRAPH_KEY),
            Some(&PropertyValue::String("extraction".into()))
        );
        assert!(report.is_lossless(), "{report:?}");
    }

    /// **Two named graphs on one subject is a reported loss**, because `_graph`
    /// holds one value.
    #[test]
    fn a_subject_in_two_named_graphs_reports_the_collapse() {
        let mut asserted = typed("orders", "Table");
        asserted.cx = Some(dsc("extraction"));
        let mut inferred = fact("orders", "rowCount", FlakeValue::Int(1));
        inferred.cx = Some(dsc("reasoning"));
        let mut report = MappingReport::default();

        node_from_flakes(&sid("orders"), &[asserted, inferred], &mut report).expect("projects");

        assert!(!report.is_lossless(), "the loss is reported: {report:?}");
        assert!(
            matches!(
                report.lossy.first(),
                Some(LossyMapping::NamedGraphCollapse { .. })
            ),
            "{report:?}"
        );
    }

    /// Transaction time comes through read-only, as the newest `t` on the
    /// subject.
    #[test]
    fn transaction_time_projects_as_a_reserved_property() {
        let mut later = fact("orders", "rowCount", FlakeValue::Int(2));
        later.t = 9;
        let flakes = vec![typed("orders", "Table"), later];

        let node = node_from_flakes(&sid("orders"), &flakes, &mut MappingReport::default())
            .expect("projects");

        assert_eq!(
            node.properties.get(TIME_KEY),
            Some(&PropertyValue::Integer(9))
        );
    }

    /// **A reference in property position is reported**, since LPG has no
    /// property whose value is another element.
    #[test]
    fn a_reference_in_property_position_is_reported_as_lossy() {
        let flakes = vec![
            typed("orders", "Table"),
            fact("orders", "owner", FlakeValue::Ref(sid("team/payments"))),
        ];
        let mut report = MappingReport::default();

        let node = node_from_flakes(&sid("orders"), &flakes, &mut report).expect("projects");

        assert!(
            matches!(
                node.properties.get("owner"),
                Some(PropertyValue::ElementRef(_))
            ),
            "{node:?}"
        );
        assert!(
            matches!(
                report.lossy.first(),
                Some(LossyMapping::RefInProperty { .. })
            ),
            "the loss is named: {report:?}"
        );
    }

    /// And an ordinary projection reports nothing — a report that is always
    /// non-empty is one nobody reads.
    #[test]
    fn a_plain_projection_reports_no_loss() {
        let flakes = vec![
            typed("orders", "Table"),
            fact("orders", "rowCount", FlakeValue::Int(1)),
        ];
        let mut report = MappingReport::default();

        node_from_flakes(&sid("orders"), &flakes, &mut report).expect("projects");

        assert!(report.is_lossless(), "{report:?}");
    }

    // ---- Slice C: edges ----

    fn relationship() -> Vec<Flake> {
        vec![
            fact(
                "rel/1",
                predicate::TYPE,
                FlakeValue::Ref(dsc("Relationship")),
            ),
            fact("rel/1", predicate::REL_TYPE, FlakeValue::Ref(dsc("feeds"))),
            fact(
                "rel/1",
                predicate::FROM_ENTITY,
                FlakeValue::Ref(sid("staging")),
            ),
            fact(
                "rel/1",
                predicate::TO_ENTITY,
                FlakeValue::Ref(sid("orders")),
            ),
            fact("rel/1", "confidence", FlakeValue::Float(0.9)),
        ]
    }

    /// **A reified relationship projects as an edge with properties**, which is
    /// the defining LPG feature and the reason this mapping is cheap.
    #[test]
    fn a_reified_relationship_projects_as_an_edge_with_properties() {
        let edge = edge_from_reified(
            &sid("rel/1"),
            &relationship(),
            &mut MappingReport::default(),
        )
        .expect("projects");

        assert_eq!(edge.edge_type, "feeds");
        assert_eq!(edge.start, ElementId::encode(&sid("staging")));
        assert_eq!(edge.end, ElementId::encode(&sid("orders")));
        assert_eq!(
            edge.properties.get("confidence"),
            Some(&PropertyValue::Float(0.9)),
            "edge properties are the whole point: {edge:?}"
        );
    }

    /// **The reification marker is not an edge property.** Carrying `type`
    /// through would put `Relationship` on every edge in the graph.
    #[test]
    fn the_reification_marker_does_not_become_a_property() {
        let edge = edge_from_reified(
            &sid("rel/1"),
            &relationship(),
            &mut MappingReport::default(),
        )
        .expect("projects");

        assert_eq!(edge.properties.get(predicate::TYPE), None, "{edge:?}");
        // The *user* properties are exactly the edge's own. Reserved keys the
        // projection owns (`_t`, and `_graph` when scoped) are counted
        // separately — asserting a bare length here would break every time the
        // projection gained a legitimate reserved key, which is what happened.
        let user: Vec<&String> = edge
            .properties
            .keys()
            .filter(|key| !key.starts_with(RESERVED_PREFIX))
            .collect();
        assert_eq!(user, vec!["confidence"], "{edge:?}");
    }

    /// **The edge is addressable as a node**, because provenance, review and
    /// memory all link *to* a relationship.
    #[test]
    fn an_edge_keeps_the_relationships_own_handle() {
        let mut report = MappingReport::default();

        let edge =
            edge_from_reified(&sid("rel/1"), &relationship(), &mut report).expect("projects");
        let node =
            node_from_flakes(&sid("rel/1"), &relationship(), &mut report).expect("also a node");

        assert_eq!(
            edge.element_id, node.element_id,
            "both views name the same object"
        );
        assert!(
            node.labels.contains(&"Relationship".to_string()),
            "{node:?}"
        );
    }

    /// **A reification missing an endpoint is refused.** An edge to nowhere is
    /// something a traversal would count and then fail to follow.
    #[test]
    fn a_reification_missing_an_endpoint_is_refused() {
        for missing in [
            predicate::REL_TYPE,
            predicate::FROM_ENTITY,
            predicate::TO_ENTITY,
        ] {
            let flakes: Vec<Flake> = relationship()
                .into_iter()
                .filter(|flake| flake.p.id != missing)
                .collect();

            let outcome = edge_from_reified(&sid("rel/1"), &flakes, &mut MappingReport::default());

            assert!(
                matches!(outcome, Err(MappingError::NotARelationship(_, m)) if m == missing),
                "missing `{missing}` should be refused, got {outcome:?}"
            );
        }
    }

    /// An edge type given as a string rather than a reference still projects —
    /// both spellings appear in the flake layer.
    #[test]
    fn an_edge_type_may_be_a_string_or_a_reference() {
        let mut as_string = relationship();
        as_string.retain(|flake| flake.p.id != predicate::REL_TYPE);
        as_string.push(fact(
            "rel/1",
            predicate::REL_TYPE,
            FlakeValue::String("feeds".into()),
        ));

        let edge = edge_from_reified(&sid("rel/1"), &as_string, &mut MappingReport::default())
            .expect("projects");

        assert_eq!(edge.edge_type, "feeds");
    }

    /// A retracted edge property does not project — the same rule nodes follow,
    /// stated separately because it is a different code path.
    #[test]
    fn a_retracted_edge_property_does_not_project() {
        let mut flakes = relationship();
        for flake in &mut flakes {
            if flake.p.id == "confidence" {
                flake.op = false;
            }
        }

        let edge = edge_from_reified(&sid("rel/1"), &flakes, &mut MappingReport::default())
            .expect("projects");

        assert_eq!(edge.properties.get("confidence"), None, "{edge:?}");
    }

    // ---- Slice D: the reverse direction ----

    /// Flakes compared as a set: the model is a set, so any order is an
    /// artifact of iteration rather than of the data.
    fn as_set(flakes: &[Flake]) -> std::collections::BTreeSet<String> {
        flakes.iter().map(|f| format!("{f:?}")).collect()
    }

    /// **The round-trip test, and it is the specification for decision 2.**
    ///
    /// Flakes → LPG → flakes, over a fixture covering every value type the
    /// round trip preserves exactly.
    #[test]
    fn a_node_round_trips_through_the_projection() {
        let original = vec![
            typed("orders", "Table"),
            typed("orders", "Dataset"),
            fact("orders", "name", FlakeValue::String("orders".into())),
            fact("orders", "rowCount", FlakeValue::Int(41_203)),
            fact("orders", "conf", FlakeValue::Float(0.875)),
            fact("orders", "active", FlakeValue::Boolean(true)),
            fact("orders", "ttl", FlakeValue::Duration(3_600)),
            fact("orders", "blob", FlakeValue::Bytes(vec![1, 2, 3])),
            fact("orders", "owner", FlakeValue::Ref(sid("team/payments"))),
        ];
        let mut report = MappingReport::default();

        let node = node_from_flakes(&sid("orders"), &original, &mut report).expect("projects");
        let back = flakes_from_node(&node, 1).expect("reverses");

        assert_eq!(
            as_set(&back),
            as_set(&original),
            "the round trip must be exact for every type it claims to preserve"
        );
    }

    /// **A `Uuid` and a `Json` survive by value and not by type**, and the
    /// forward pass says so rather than the round trip silently narrowing them.
    #[test]
    fn a_uuid_narrows_to_a_string_and_the_loss_is_named() {
        let id = uuid::Uuid::from_u128(7);
        let original = vec![
            typed("orders", "Table"),
            fact("orders", "sourceId", FlakeValue::Uuid(id)),
        ];
        let mut report = MappingReport::default();

        let node = node_from_flakes(&sid("orders"), &original, &mut report).expect("projects");
        let back = flakes_from_node(&node, 1).expect("reverses");

        assert!(
            matches!(
                report.lossy.first(),
                Some(LossyMapping::TypeNarrowed { from: "uuid", .. })
            ),
            "the specific loss is named, not a generic one: {report:?}"
        );
        // The value is intact; only the tag changed.
        assert!(
            back.iter()
                .any(|f| f.o == FlakeValue::String(id.to_string())),
            "{back:?}"
        );
    }

    #[test]
    fn json_narrows_to_a_string_and_the_loss_is_named() {
        let mut report = MappingReport::default();
        let node = node_from_flakes(
            &sid("orders"),
            &[
                typed("orders", "Table"),
                fact("orders", "raw", FlakeValue::Json("{\"a\":1}".into())),
            ],
            &mut report,
        )
        .expect("projects");

        assert!(
            matches!(
                report.lossy.first(),
                Some(LossyMapping::TypeNarrowed { from: "json", .. })
            ),
            "{report:?}"
        );
        assert!(flakes_from_node(&node, 1).is_ok());
    }

    /// **A loss annotates the operation; it does not fail it.** A caller
    /// deciding whether to proceed needs the result *and* the caveat — refusing
    /// outright would make every reference-carrying node unprojectable.
    #[test]
    fn a_reported_loss_does_not_fail_the_projection() {
        let mut report = MappingReport::default();

        let node = node_from_flakes(
            &sid("orders"),
            &[
                typed("orders", "Table"),
                fact("orders", "owner", FlakeValue::Ref(sid("team/payments"))),
            ],
            &mut report,
        );

        assert!(node.is_ok(), "annotated, not refused: {node:?}");
        assert!(!report.is_lossless(), "and the caveat is there: {report:?}");
        assert!(flakes_from_node(&node.expect("projects"), 1).is_ok());
    }

    /// A repeated predicate survives the round trip as the same set of facts,
    /// which is the only sense in which a list can round-trip.
    #[test]
    fn a_repeated_predicate_round_trips_as_a_set() {
        let original = vec![
            typed("orders", "Table"),
            fact("orders", "tag", FlakeValue::String("pii".into())),
            fact("orders", "tag", FlakeValue::String("gold".into())),
        ];

        let node = node_from_flakes(&sid("orders"), &original, &mut MappingReport::default())
            .expect("projects");
        let back = flakes_from_node(&node, 1).expect("reverses");

        assert_eq!(as_set(&back), as_set(&original));
    }

    /// **The caller supplies `t`; `_t` in the payload is ignored.** Taking it
    /// from the payload would let a caller forge history, which is the one thing
    /// an append-only log exists to prevent.
    #[test]
    fn transaction_time_comes_from_the_caller_not_the_payload() {
        let mut later = fact("orders", "rowCount", FlakeValue::Int(2));
        later.t = 9;
        let node = node_from_flakes(
            &sid("orders"),
            &[typed("orders", "Table"), later],
            &mut MappingReport::default(),
        )
        .expect("projects");
        assert_eq!(
            node.properties.get(TIME_KEY),
            Some(&PropertyValue::Integer(9))
        );

        let back = flakes_from_node(&node, 42).expect("reverses");

        assert!(
            back.iter().all(|f| f.t == 42),
            "the caller's t wins: {back:?}"
        );
        assert!(
            !back.iter().any(|f| f.p.id == TIME_KEY),
            "`_t` is not written back as a fact: {back:?}"
        );
    }

    /// The named graph survives the round trip onto every flake.
    #[test]
    fn a_named_graph_round_trips_onto_every_flake() {
        let mut scoped = typed("orders", "Table");
        scoped.cx = Some(dsc("reasoning"));
        let node = node_from_flakes(&sid("orders"), &[scoped], &mut MappingReport::default())
            .expect("projects");

        let back = flakes_from_node(&node, 1).expect("reverses");

        assert!(
            back.iter().all(|f| f.cx == Some(dsc("reasoning"))),
            "a derived fact must not come back looking asserted: {back:?}"
        );
        assert!(!back.iter().any(|f| f.p.id == GRAPH_KEY));
    }

    /// **A user-supplied reserved key is rejected on the write path**, naming it.
    #[test]
    fn a_reserved_key_on_the_write_path_is_rejected_by_name() {
        let mut properties = PropertyMap::new();
        properties.insert_reserved("_smuggled", PropertyValue::Integer(1));
        let node = LpgNode {
            element_id: ElementId::encode(&sid("orders")),
            labels: vec!["Table".to_string()],
            properties,
        };

        let outcome = flakes_from_node(&node, 1);

        assert!(
            matches!(outcome, Err(MappingError::ReservedPropertyName(ref k)) if k == "_smuggled"),
            "{outcome:?}"
        );
    }

    /// An unresolvable handle is an error, never a dropped property — a
    /// reference that vanished would turn a broken import into a successful one.
    #[test]
    fn an_unresolvable_handle_is_an_error_rather_than_a_dropped_property() {
        let mut properties = PropertyMap::new();
        properties
            .insert_user(
                "owner",
                PropertyValue::ElementRef(ElementId::from_wire("not-an-id")),
            )
            .expect("accepted");
        let node = LpgNode {
            element_id: ElementId::encode(&sid("orders")),
            labels: vec!["Table".to_string()],
            properties,
        };

        assert!(matches!(
            flakes_from_node(&node, 1),
            Err(MappingError::UnresolvableHandle(_))
        ));
    }

    #[test]
    fn a_node_with_no_labels_cannot_be_written_back() {
        let node = LpgNode {
            element_id: ElementId::encode(&sid("orders")),
            labels: Vec::new(),
            properties: PropertyMap::new(),
        };

        assert!(matches!(
            flakes_from_node(&node, 1),
            Err(MappingError::Untyped(_))
        ));
    }

    /// **The edge round trip, which is where reification earns its place.**
    #[test]
    fn an_edge_round_trips_with_its_properties() {
        let original = relationship();

        let edge = edge_from_reified(&sid("rel/1"), &original, &mut MappingReport::default())
            .expect("projects");
        let back = flakes_from_edge(&edge, 1).expect("reverses");

        assert_eq!(
            as_set(&back),
            as_set(&original),
            "an edge with properties must survive intact: {back:?}"
        );
    }

    /// **A derived edge round trips as derived**, and this is the case Slice E
    /// exists for: an agent or a UI must be able to tell an edge Epic 6
    /// *inferred* from one somebody *asserted*. Losing `_graph` on the way back
    /// makes inference indistinguishable from fact.
    ///
    /// Mutation testing found this gap — the node side had a named-graph test
    /// and the edge side did not, so `_graph` on an edge was never exercised at
    /// all and would have been rejected as a reserved key.
    #[test]
    fn a_derived_edge_round_trips_still_marked_derived() {
        let inferred: Vec<Flake> = relationship()
            .into_iter()
            .map(|mut flake| {
                flake.cx = Some(dsc("reasoning"));
                flake
            })
            .collect();

        let edge = edge_from_reified(&sid("rel/1"), &inferred, &mut MappingReport::default())
            .expect("projects");
        assert_eq!(
            edge.properties.get(GRAPH_KEY),
            Some(&PropertyValue::String("reasoning".into())),
            "the projection marks it derived: {edge:?}"
        );

        let back = flakes_from_edge(&edge, 1).expect("reverses");

        assert!(
            back.iter().all(|f| f.cx == Some(dsc("reasoning"))),
            "and it comes back derived, not asserted: {back:?}"
        );
        assert_eq!(as_set(&back), as_set(&inferred));
    }

    /// One named graph is not a collapse — a report that fired on every scoped
    /// edge would be noise, and a caller learns to ignore a flag that is always
    /// set.
    #[test]
    fn an_edge_in_one_named_graph_reports_no_collapse() {
        let inferred: Vec<Flake> = relationship()
            .into_iter()
            .map(|mut flake| {
                flake.cx = Some(dsc("reasoning"));
                flake
            })
            .collect();
        let mut report = MappingReport::default();

        edge_from_reified(&sid("rel/1"), &inferred, &mut report).expect("projects");

        assert!(report.is_lossless(), "{report:?}");
    }

    /// **Two named graphs on one edge is a reported collapse**, because
    /// `_graph` holds one value — the same rule the node side follows, stated
    /// separately because it is a different code path and mutation testing
    /// found the edge side untested.
    #[test]
    fn an_edge_spanning_two_named_graphs_reports_the_collapse() {
        let mut mixed = relationship();
        mixed[0].cx = Some(dsc("extraction"));
        for flake in mixed.iter_mut().skip(1) {
            flake.cx = Some(dsc("reasoning"));
        }
        let mut report = MappingReport::default();

        edge_from_reified(&sid("rel/1"), &mixed, &mut report).expect("projects");

        assert!(
            matches!(
                report.lossy.first(),
                Some(LossyMapping::NamedGraphCollapse { .. })
            ),
            "the loss is named: {report:?}"
        );
    }

    #[test]
    fn an_edge_with_a_broken_endpoint_is_an_error() {
        let mut edge = edge_from_reified(
            &sid("rel/1"),
            &relationship(),
            &mut MappingReport::default(),
        )
        .expect("projects");
        edge.start = ElementId::from_wire("garbage");

        assert!(matches!(
            flakes_from_edge(&edge, 1),
            Err(MappingError::UnresolvableHandle(_))
        ));
    }

    #[test]
    fn the_wire_shapes_are_camel_case() {
        let mut report = MappingReport::default();
        let node = node_from_flakes(&sid("orders"), &[typed("orders", "Table")], &mut report)
            .expect("projects");

        let json = serde_json::to_value(&node).expect("serialize");
        assert!(json.get("elementId").is_some(), "{json}");
        assert!(json.get("element_id").is_none(), "{json}");

        let edge = edge_from_reified(&sid("rel/1"), &relationship(), &mut report).expect("edge");
        let json = serde_json::to_value(&edge).expect("serialize");
        assert!(json.get("edgeType").is_some(), "{json}");
        assert!(
            json.get("startNode").is_none(),
            "start/end, not startNode: {json}"
        );
    }
}
