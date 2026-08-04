//! The flake: the atom of the graph.
//!
//! A flake is one assertion or retraction of one fact at one transaction time.
//! Entities live in relational tables — that is the source of truth — and are
//! *projected* here so the catalog is queryable as a graph and its history is
//! recoverable by construction rather than by a parallel audit table that can
//! drift (`plans/04-engine-triples.md` decision 1).

use std::fmt;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A subject or predicate identifier: a namespace code plus a local name.
///
/// Not an interned integer. Interning is a real optimization and a real
/// complication — a dictionary that must stay consistent with every flake ever
/// written. Postgres composite indexes on `(smallint, text)` are adequate at
/// the target scale; revisit when measurement says otherwise, not before
/// (`plans/04-engine-triples.md` decision 9).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sid {
    /// Which vocabulary `id` is drawn from. See [`namespace`].
    pub namespace_code: u16,
    /// The local name within that vocabulary.
    pub id: String,
}

impl Sid {
    /// An identifier in an arbitrary vocabulary.
    #[must_use]
    pub fn new(namespace_code: u16, id: impl Into<String>) -> Self {
        Self {
            namespace_code,
            id: id.into(),
        }
    }

    /// A predicate or class in this project's own vocabulary.
    #[must_use]
    pub fn dsc(id: impl Into<String>) -> Self {
        Self::new(namespace::DSC, id)
    }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace_code, self.id)
    }
}

/// Namespace code allocation.
///
/// The ranges follow one rule: **the codes that appear on the most flakes get
/// the smallest numbers**, because `dsc:` predicates dominate this graph by a
/// wide margin and a compact code keeps the composite index narrow.
///
/// | Range | Owner |
/// |---|---|
/// | `0` | Reserved — unset, never valid |
/// | `1–255` | graph-owl's own vocabulary |
/// | `256–511` | Standards vocabularies |
/// | `512–1023` | Reserved for vocabularies this project introduces later |
/// | `1024–65534` | Runtime, allocated by the predicate registry |
/// | `65535` | Reserved — "not found" sentinel |
///
/// **Allocation is monotonic.** A code, once assigned to an IRI prefix, is
/// never reused for a different one: a reused code silently rewrites the
/// meaning of every historical flake carrying it, and time-travel makes that
/// corruption permanent rather than transient.
pub mod namespace {
    /// Never a valid stored namespace, which makes an uninitialized `Sid` a
    /// detectable bug rather than a silent default.
    pub const UNSET: u16 = 0;

    /// This project's catalog vocabulary: `https://graph-owl.dev/ns/catalog#`.
    pub const DSC: u16 = 0x0001;

    /// `http://www.w3.org/1999/02/22-rdf-syntax-ns#`.
    pub const RDF: u16 = 0x0100;
    /// `http://www.w3.org/2000/01/rdf-schema#`.
    pub const RDFS: u16 = 0x0101;
    /// `http://www.w3.org/2001/XMLSchema#`.
    pub const XSD: u16 = 0x0102;
    /// `http://www.w3.org/2002/07/owl#`.
    pub const OWL: u16 = 0x0103;
    /// `http://www.w3.org/ns/shacl#`.
    pub const SHACL: u16 = 0x0104;
    /// `https://schema.org/`.
    pub const SCHEMA: u16 = 0x0106;
    /// `http://purl.org/dc/terms/`.
    pub const DCTERMS: u16 = 0x0107;

    /// First code the predicate registry may hand out at runtime.
    pub const RUNTIME_START: u16 = 1024;

    /// Distinct from [`UNSET`] so a lookup miss is never confusable with an
    /// uninitialized value — they are different bugs with different fixes.
    pub const NOT_FOUND: u16 = 65535;
}

/// The IRI a namespace code stands for.
///
/// A `Sid` is a compact internal address; an IRI is what leaves the system.
/// This is the one mapping between them, so a code cannot mean one IRI on
/// export and another in a query.
///
/// Returns `None` for a code with no assigned IRI — including [`namespace::UNSET`].
/// A `Sid` that cannot be expressed as an IRI must fail loudly rather than
/// serialize as a bare local name, which would silently drop the vocabulary it
/// belongs to (`plans/09-engine-rdf-io.md`).
#[must_use]
pub fn namespace_iri(code: u16) -> Option<&'static str> {
    Some(match code {
        namespace::DSC => "https://graph-owl.dev/ns/catalog#",
        namespace::RDF => "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
        namespace::RDFS => "http://www.w3.org/2000/01/rdf-schema#",
        namespace::XSD => "http://www.w3.org/2001/XMLSchema#",
        namespace::OWL => "http://www.w3.org/2002/07/owl#",
        namespace::SHACL => "http://www.w3.org/ns/shacl#",
        namespace::SCHEMA => "https://schema.org/",
        namespace::DCTERMS => "http://purl.org/dc/terms/",
        _ => return None,
    })
}

impl Sid {
    /// The full IRI this identifier denotes.
    ///
    /// # Errors
    ///
    /// `None` when the namespace has no assigned IRI.
    #[must_use]
    pub fn to_iri(&self) -> Option<String> {
        namespace_iri(self.namespace_code).map(|base| format!("{base}{}", self.id))
    }

    /// The inverse: split a full IRI back into a namespace code and local name.
    ///
    /// Longest-prefix wins, which matters because two vocabulary IRIs could in
    /// principle share a prefix — matching the first would silently attribute a
    /// term to the wrong vocabulary.
    #[must_use]
    pub fn from_iri(iri: &str) -> Option<Self> {
        [
            namespace::DSC,
            namespace::RDF,
            namespace::RDFS,
            namespace::XSD,
            namespace::OWL,
            namespace::SHACL,
            namespace::SCHEMA,
            namespace::DCTERMS,
        ]
        .into_iter()
        .filter_map(|code| {
            let base = namespace_iri(code)?;
            iri.strip_prefix(base)
                .map(|local| (base.len(), code, local))
        })
        .max_by_key(|(len, _, _)| *len)
        .map(|(_, code, local)| Sid::new(code, local))
    }
}

/// The object position of a flake.
///
/// The discriminants returned by [`FlakeValue::value_type`] are a **wire
/// contract**: they are written into every flake row. Adding a variant is
/// cheap; renumbering one is a data migration over every flake ever stored.
#[derive(Debug, Clone, PartialEq)]
pub enum FlakeValue {
    /// A reference to another node. Every edge in the graph is one of these.
    Ref(Sid),
    /// A text value.
    String(String),
    /// A true/false value.
    Boolean(bool),
    /// A signed integer.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// A point in time.
    Instant(DateTime<Utc>),
    /// An opaque JSON document, stored as text.
    Json(String),
    /// Without this, binary data has no home at all — and the one variant that
    /// cannot be encoded as a string is the worst possible retrofit.
    Bytes(Vec<u8>),
    /// A UUID value.
    Uuid(Uuid),
    /// Whole seconds. Freshness SLAs are the use case and none of them need
    /// sub-second resolution; `i64` seconds round-trips through Postgres
    /// `BIGINT` without the ambiguity `INTERVAL` carries around months.
    Duration(i64),
}

/// Discriminants, pinned. Named constants rather than bare numbers in a match,
/// so the pinning test and the adapter reference the same thing.
pub mod value_type {
    /// [`super::FlakeValue::Ref`].
    pub const REF: i16 = 0;
    /// [`super::FlakeValue::String`].
    pub const STRING: i16 = 1;
    /// [`super::FlakeValue::Boolean`].
    pub const BOOLEAN: i16 = 2;
    /// [`super::FlakeValue::Int`].
    pub const INT: i16 = 3;
    /// [`super::FlakeValue::Float`].
    pub const FLOAT: i16 = 4;
    /// [`super::FlakeValue::Instant`].
    pub const INSTANT: i16 = 5;
    /// [`super::FlakeValue::Json`].
    pub const JSON: i16 = 6;
    /// [`super::FlakeValue::Bytes`].
    pub const BYTES: i16 = 7;
    /// [`super::FlakeValue::Uuid`].
    pub const UUID: i16 = 8;
    /// [`super::FlakeValue::Duration`].
    pub const DURATION: i16 = 9;
}

impl FlakeValue {
    /// The stored discriminant. See [`value_type`].
    #[must_use]
    pub fn value_type(&self) -> i16 {
        match self {
            FlakeValue::Ref(_) => value_type::REF,
            FlakeValue::String(_) => value_type::STRING,
            FlakeValue::Boolean(_) => value_type::BOOLEAN,
            FlakeValue::Int(_) => value_type::INT,
            FlakeValue::Float(_) => value_type::FLOAT,
            FlakeValue::Instant(_) => value_type::INSTANT,
            FlakeValue::Json(_) => value_type::JSON,
            FlakeValue::Bytes(_) => value_type::BYTES,
            FlakeValue::Uuid(_) => value_type::UUID,
            FlakeValue::Duration(_) => value_type::DURATION,
        }
    }

    /// Whether this value can be traversed backwards.
    ///
    /// The OPST index is partial (`WHERE value_type = 0`) because reverse
    /// lookup is only meaningful for references — indexing literal objects
    /// there would cost a full index for no query that can use it.
    #[must_use]
    pub fn is_reference(&self) -> bool {
        matches!(self, FlakeValue::Ref(_))
    }
}

/// One assertion or retraction, at one transaction time.
#[derive(Debug, Clone, PartialEq)]
pub struct Flake {
    /// The subject — the entity this fact is about.
    pub s: Sid,
    /// The predicate — which fact.
    pub p: Sid,
    /// The object — the fact's value.
    pub o: FlakeValue,
    /// Named graph. `None` is the default graph holding core catalog facts.
    pub cx: Option<Sid>,
    /// Transaction time — a logical clock, not a wall clock. Every flake in
    /// one logical change shares a `t`, which is what makes "the state after
    /// change N" well-defined.
    pub t: i64,
    /// `true` asserts, `false` retracts. A retraction is a **new row**, never
    /// a delete: deleting would break time-travel, which is the whole reason
    /// history here is native rather than a feature.
    pub op: bool,
}

impl Flake {
    /// An assertion in the default graph.
    #[must_use]
    pub fn assert(s: Sid, p: Sid, o: FlakeValue, t: i64) -> Self {
        Self {
            s,
            p,
            o,
            cx: None,
            t,
            op: true,
        }
    }

    /// The retraction of this flake at a later `t`.
    ///
    /// Same subject, predicate, object and graph — a retraction that did not
    /// name the exact fact it withdraws could not be matched against it during
    /// current-state resolution.
    #[must_use]
    pub fn retracted_at(&self, t: i64) -> Self {
        Self {
            t,
            op: false,
            ..self.clone()
        }
    }
}

/// A query over flakes. `None` in any position matches anything.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriplePattern {
    /// The subject to match, or any.
    pub s: Option<Sid>,
    /// The predicate to match, or any.
    pub p: Option<Sid>,
    /// The object to match, or any.
    pub o: Option<FlakeValue>,
    /// Outer `None` means any graph; `Some(None)` means the default graph
    /// specifically. Without the nesting there is no way to ask for "facts not
    /// in any named graph", which is the common case.
    pub cx: Option<Option<Sid>>,
    /// `None` is current state.
    pub as_of: Option<i64>,
}

#[cfg(test)]
mod flake_value_tests {
    use super::*;

    /// The pinning test. These numbers are written into every flake row; a
    /// reordering of the enum would silently reinterpret stored data, and
    /// nothing else in the system would notice.
    #[test]
    fn discriminants_are_pinned_to_their_stored_numbers() {
        let uuid = Uuid::nil();
        let now = Utc::now();
        for (value, expected) in [
            (FlakeValue::Ref(Sid::dsc("x")), 0),
            (FlakeValue::String("x".into()), 1),
            (FlakeValue::Boolean(true), 2),
            (FlakeValue::Int(1), 3),
            (FlakeValue::Float(1.0), 4),
            (FlakeValue::Instant(now), 5),
            (FlakeValue::Json("{}".into()), 6),
            (FlakeValue::Bytes(vec![1]), 7),
            (FlakeValue::Uuid(uuid), 8),
            (FlakeValue::Duration(1), 9),
        ] {
            assert_eq!(
                value.value_type(),
                expected,
                "{value:?} must stay at discriminant {expected} — changing it \
                 is a migration over every flake ever written"
            );
        }
    }

    /// Guards the mutant that collapses two variants onto one discriminant,
    /// which the per-variant assertions above would not all catch on their own.
    #[test]
    fn every_variant_has_a_distinct_discriminant() {
        let all = [
            FlakeValue::Ref(Sid::dsc("x")),
            FlakeValue::String("x".into()),
            FlakeValue::Boolean(true),
            FlakeValue::Int(1),
            FlakeValue::Float(1.0),
            FlakeValue::Instant(Utc::now()),
            FlakeValue::Json("{}".into()),
            FlakeValue::Bytes(vec![1]),
            FlakeValue::Uuid(Uuid::nil()),
            FlakeValue::Duration(1),
        ];
        let mut seen: Vec<i16> = all.iter().map(FlakeValue::value_type).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), all.len(), "two variants share a discriminant");
    }

    #[test]
    fn only_references_are_reverse_traversable() {
        assert!(FlakeValue::Ref(Sid::dsc("x")).is_reference());
        for literal in [
            FlakeValue::String("x".into()),
            FlakeValue::Int(1),
            FlakeValue::Boolean(false),
            FlakeValue::Bytes(vec![]),
        ] {
            assert!(
                !literal.is_reference(),
                "{literal:?} is a literal — the OPST index excludes it on purpose"
            );
        }
    }
}

#[cfg(test)]
mod namespace_tests {
    use super::*;

    #[test]
    fn zero_is_never_a_real_namespace() {
        assert_eq!(namespace::UNSET, 0);
        assert_ne!(
            namespace::DSC,
            namespace::UNSET,
            "an uninitialized Sid must be detectable, not a valid default"
        );
    }

    /// A miss and an uninitialized value are different bugs; collapsing them
    /// onto one number makes the first look like the second.
    #[test]
    fn the_not_found_sentinel_is_distinct_from_unset() {
        assert_ne!(namespace::NOT_FOUND, namespace::UNSET);
        assert_eq!(namespace::NOT_FOUND, 65535);
    }

    #[test]
    fn the_hot_vocabulary_holds_the_low_byte() {
        // dsc: is on most flakes in the graph, and a wide code widens three of
        // the four composite indexes. Checked at compile time because both
        // sides are constants — a runtime assertion here could only ever fail
        // in a build that already knew it would.
        const { assert!(namespace::DSC < 256) };
        for standard in [
            namespace::RDF,
            namespace::RDFS,
            namespace::XSD,
            namespace::OWL,
            namespace::SHACL,
            namespace::SCHEMA,
            namespace::DCTERMS,
        ] {
            assert!(
                (256..512).contains(&standard),
                "{standard} is outside the standards range"
            );
        }
    }

    #[test]
    fn runtime_allocation_starts_above_the_reserved_ranges() {
        assert_eq!(namespace::RUNTIME_START, 1024);
        const { assert!(namespace::RUNTIME_START < namespace::NOT_FOUND) };
    }

    /// `Sid` renders as `code:id`, and the code stays a number.
    ///
    /// Resolving it to a prefix would need the registry, and a `Display` that
    /// reaches for I/O is a `Display` that cannot be called from an error path.
    /// A bare number is cryptic but unambiguous — and it is the value actually
    /// stored, which is what someone reading the error needs to match against.
    #[test]
    fn a_sid_renders_as_its_namespace_code_and_local_name() {
        assert_eq!(Sid::dsc("parentTable").to_string(), "1:parentTable");
        assert_eq!(Sid::new(namespace::RDF, "type").to_string(), "256:type");
    }

    /// Two sids differing only by namespace must not render identically —
    /// otherwise an error message cannot distinguish `dsc:type` from `rdf:type`,
    /// which are different predicates that both appear on the same subjects.
    #[test]
    fn sids_from_different_vocabularies_render_differently() {
        assert_ne!(
            Sid::dsc("type").to_string(),
            Sid::new(namespace::RDF, "type").to_string()
        );
    }

    #[test]
    fn standard_namespaces_are_all_distinct() {
        let mut codes = vec![
            namespace::DSC,
            namespace::RDF,
            namespace::RDFS,
            namespace::XSD,
            namespace::OWL,
            namespace::SHACL,
            namespace::SCHEMA,
            namespace::DCTERMS,
        ];
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            codes.len(),
            total,
            "two vocabularies sharing a code rewrites the meaning of stored flakes"
        );
    }
}

#[cfg(test)]
mod iri_tests {
    use super::*;

    #[test]
    fn a_sid_round_trips_through_its_iri() {
        for sid in [
            Sid::dsc("parentTable"),
            Sid::new(namespace::RDF, "type"),
            Sid::new(namespace::XSD, "dateTime"),
        ] {
            let iri = sid.to_iri().expect("has an IRI");
            assert_eq!(Sid::from_iri(&iri), Some(sid.clone()), "{iri}");
        }
    }

    #[test]
    fn the_catalog_vocabulary_has_the_iri_the_domain_model_states() {
        assert_eq!(
            Sid::dsc("name").to_iri().as_deref(),
            Some("https://graph-owl.dev/ns/catalog#name")
        );
    }

    /// A `Sid` that cannot be expressed as an IRI must fail rather than
    /// serialize as a bare local name, which would drop the vocabulary it
    /// belongs to and make the term unresolvable.
    #[test]
    fn an_unassigned_namespace_has_no_iri() {
        assert!(Sid::new(namespace::UNSET, "x").to_iri().is_none());
        assert!(Sid::new(9999, "x").to_iri().is_none());
    }

    #[test]
    fn an_unknown_iri_does_not_resolve_to_a_sid() {
        assert!(Sid::from_iri("https://example.org/thing").is_none());
    }

    /// Longest-prefix wins. Matching the first prefix found would silently
    /// attribute a term to whichever vocabulary happened to be checked first.
    #[test]
    fn the_longest_matching_prefix_wins() {
        // `rdf:` and `rdfs:` do not nest, but the property must hold anyway —
        // it is what makes the mapping safe to extend.
        let rdfs = Sid::new(namespace::RDFS, "label");
        let iri = rdfs.to_iri().expect("iri");
        assert_eq!(Sid::from_iri(&iri), Some(rdfs));
    }

    #[test]
    fn a_local_name_containing_the_separator_survives() {
        // FQNs appear as local names and contain dots.
        let sid = Sid::dsc("hdfc-core.postgres.payments.upi_transactions");
        let iri = sid.to_iri().expect("iri");
        assert_eq!(Sid::from_iri(&iri), Some(sid));
    }
}

#[cfg(test)]
mod flake_tests {
    use super::*;

    fn a_flake() -> Flake {
        Flake::assert(
            Sid::dsc("table-1"),
            Sid::dsc("name"),
            FlakeValue::String("upi_transactions".into()),
            10,
        )
    }

    #[test]
    fn an_assertion_defaults_to_the_default_graph() {
        let flake = a_flake();
        assert!(flake.op);
        assert_eq!(flake.cx, None);
    }

    /// A retraction that did not name the exact fact it withdraws could not be
    /// matched against that fact during current-state resolution.
    #[test]
    fn a_retraction_names_the_same_fact_at_a_later_time() {
        let asserted = a_flake();
        let retracted = asserted.retracted_at(20);

        assert!(!retracted.op);
        assert_eq!(retracted.t, 20);
        assert_eq!(retracted.s, asserted.s);
        assert_eq!(retracted.p, asserted.p);
        assert_eq!(retracted.o, asserted.o);
        assert_eq!(retracted.cx, asserted.cx);
    }

    #[test]
    fn retracting_does_not_alter_the_assertion() {
        let asserted = a_flake();
        let _ = asserted.retracted_at(20);
        assert!(
            asserted.op,
            "the assertion row survives — that is time-travel"
        );
        assert_eq!(asserted.t, 10);
    }

    /// The nesting exists so "facts in no named graph" is expressible. A plain
    /// `Option<Sid>` collapses "any graph" and "the default graph" into one
    /// value, and they are different queries.
    #[test]
    fn a_default_pattern_matches_every_position_including_any_graph() {
        let pattern = TriplePattern::default();
        assert_eq!(pattern.s, None);
        assert_eq!(pattern.p, None);
        assert_eq!(pattern.o, None);
        assert_eq!(pattern.cx, None);
        assert_eq!(pattern.as_of, None, "None means current state");
    }

    #[test]
    fn asking_for_the_default_graph_is_distinct_from_asking_for_any() {
        let any = TriplePattern::default();
        let default_graph = TriplePattern {
            cx: Some(None),
            ..TriplePattern::default()
        };
        assert_ne!(any.cx, default_graph.cx);
    }
}
