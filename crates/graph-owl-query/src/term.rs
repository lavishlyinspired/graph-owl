//! Between a `FlakeValue` and an RDF term.
//!
//! Pure, and the one place the mapping lives. A value that means one thing to
//! the store and another to a query is the class of bug that produces answers
//! nobody can reproduce.

use graph_owl_core::flake::{FlakeValue, Sid};
use oxrdf::{Literal, NamedNode, Term, vocab::xsd};

/// Why a term could not cross the boundary.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TermError {
    /// A `Sid` whose namespace has no assigned IRI. Refused rather than
    /// serialized as a bare local name, which would silently drop the
    /// vocabulary the term belongs to and make it unresolvable.
    #[error("namespace {0} has no IRI, so `{1}` cannot be expressed as an RDF term")]
    UnmappedNamespace(u16, String),

    /// An RDF term this store has no representation for. Named rather than
    /// coerced: silently turning a blank node into a string would make a query
    /// match rows it should not.
    #[error("{0} has no flake representation")]
    Unrepresentable(String),
}

/// A flake value as an RDF term.
///
/// # Errors
///
/// [`TermError::UnmappedNamespace`] if a reference names a namespace with no IRI.
pub fn to_term(value: &FlakeValue) -> Result<Term, TermError> {
    Ok(match value {
        FlakeValue::Ref(sid) => Term::NamedNode(to_named_node(sid)?),
        FlakeValue::String(s) => Literal::new_simple_literal(s).into(),
        FlakeValue::Boolean(b) => Literal::new_typed_literal(b.to_string(), xsd::BOOLEAN).into(),
        FlakeValue::Int(i) => Literal::new_typed_literal(i.to_string(), xsd::INTEGER).into(),
        // `{:?}` rather than `{}`: it round-trips every f64 including the
        // non-finite ones, and `xsd:double` has lexical forms for all three.
        FlakeValue::Float(f) => Literal::new_typed_literal(format!("{f:?}"), xsd::DOUBLE).into(),
        FlakeValue::Instant(dt) => Literal::new_typed_literal(
            dt.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
            xsd::DATE_TIME,
        )
        .into(),
        // A JSON value is a string with a datatype, not a structure RDF can
        // see into. Expanding it into triples would invent a shape the source
        // never declared.
        FlakeValue::Json(raw) => Literal::new_simple_literal(raw).into(),
        FlakeValue::Bytes(bytes) => Literal::new_typed_literal(
            bytes.iter().fold(String::new(), |mut acc, b| {
                use std::fmt::Write as _;
                let _ = write!(acc, "{b:02x}");
                acc
            }),
            xsd::HEX_BINARY,
        )
        .into(),
        FlakeValue::Uuid(uuid) => Literal::new_simple_literal(uuid.to_string()).into(),
        // Seconds, as an integer — not `xsd:duration`, whose lexical space
        // includes months and would reintroduce the ambiguity the storage
        // representation deliberately avoids.
        FlakeValue::Duration(seconds) => {
            Literal::new_typed_literal(seconds.to_string(), xsd::INTEGER).into()
        }
    })
}

/// A `Sid` as an IRI node.
///
/// # Errors
///
/// [`TermError::UnmappedNamespace`] if the namespace has no assigned IRI.
pub fn to_named_node(sid: &Sid) -> Result<NamedNode, TermError> {
    let iri = sid
        .to_iri()
        .ok_or_else(|| TermError::UnmappedNamespace(sid.namespace_code, sid.id.clone()))?;
    NamedNode::new(&iri).map_err(|_| TermError::UnmappedNamespace(sid.namespace_code, iri))
}

/// An RDF term as a flake value, for matching a query pattern against storage.
///
/// Deliberately *not* the exact inverse of [`to_term`]. Several flake values
/// share a lexical form — an `Int` and a `Duration` are both `xsd:integer` —
/// so a term maps to whichever value would compare equal, and the query
/// compares on the stored key. Trying to recover the original variant would be
/// guessing, and guessing wrong makes a pattern silently match nothing.
///
/// # Errors
///
/// [`TermError::Unrepresentable`] for blank nodes and triple terms, which this
/// store has no address for.
pub fn from_term(term: &Term) -> Result<FlakeValue, TermError> {
    match term {
        Term::NamedNode(node) => Sid::from_iri(node.as_str())
            .map(FlakeValue::Ref)
            // An IRI outside the registry is a legitimate thing to *ask* for —
            // it simply matches nothing. Representing it as a string keeps the
            // query answerable (with zero rows) rather than erroring, which is
            // what a SPARQL engine should do with an unknown IRI.
            .map_or_else(|| Ok(FlakeValue::String(node.as_str().to_string())), Ok),
        Term::Literal(literal) => Ok(from_literal(literal)),
        // Named rather than caught by a wildcard, so when RDF 1.2 triple terms
        // arrive (Epic 94) this becomes a compile error rather than silently
        // taking the "unrepresentable" path they may not belong on.
        Term::BlankNode(node) => Err(TermError::Unrepresentable(node.to_string())),
    }
}

fn from_literal(literal: &Literal) -> FlakeValue {
    let lexical = literal.value();
    match literal.datatype().as_str() {
        d if d == xsd::BOOLEAN.as_str() => lexical.parse().map_or_else(
            |_| FlakeValue::String(lexical.to_string()),
            FlakeValue::Boolean,
        ),
        d if d == xsd::INTEGER.as_str() || d == xsd::LONG.as_str() => lexical
            .parse()
            .map_or_else(|_| FlakeValue::String(lexical.to_string()), FlakeValue::Int),
        d if d == xsd::DOUBLE.as_str() || d == xsd::DECIMAL.as_str() => {
            lexical.parse().map_or_else(
                |_| FlakeValue::String(lexical.to_string()),
                FlakeValue::Float,
            )
        }
        d if d == xsd::DATE_TIME.as_str() => chrono::DateTime::parse_from_rfc3339(lexical)
            .map_or_else(
                |_| FlakeValue::String(lexical.to_string()),
                |dt| FlakeValue::Instant(dt.with_timezone(&chrono::Utc)),
            ),
        // Untyped, language-tagged, or a datatype this store does not model:
        // compare as a string. That is what the stored key does too.
        _ => FlakeValue::String(lexical.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use graph_owl_core::flake::namespace;

    #[test]
    fn a_reference_becomes_its_iri() {
        let term = to_term(&FlakeValue::Ref(Sid::dsc("upi_transactions"))).expect("term");
        assert_eq!(
            term.to_string(),
            "<https://graph-owl.dev/ns/catalog#upi_transactions>"
        );
    }

    /// A term whose namespace has no IRI must be refused, not emitted as a
    /// bare local name — that would drop the vocabulary and make the term
    /// unresolvable by whoever received it.
    #[test]
    fn a_reference_with_no_iri_is_refused_by_name() {
        let error =
            to_term(&FlakeValue::Ref(Sid::new(namespace::UNSET, "x"))).expect_err("must refuse");
        assert!(
            matches!(error, TermError::UnmappedNamespace(0, _)),
            "{error:?}"
        );
    }

    #[test]
    fn literals_carry_the_datatype_a_query_would_filter_on() {
        let cases = [
            (FlakeValue::Boolean(true), "boolean"),
            (FlakeValue::Int(42), "integer"),
            (FlakeValue::Float(1.5), "double"),
            (
                FlakeValue::Instant(Utc.timestamp_opt(1, 0).unwrap()),
                "dateTime",
            ),
        ];
        for (value, datatype) in cases {
            let rendered = to_term(&value).expect("term").to_string();
            assert!(
                rendered.contains(datatype),
                "{value:?} rendered as {rendered}, expected datatype {datatype}"
            );
        }
    }

    /// Without a datatype, `FILTER(?n > 5)` cannot compare — SPARQL would treat
    /// the value as a string and `"10" > "5"` is false.
    #[test]
    fn an_integer_is_not_a_plain_string() {
        let rendered = to_term(&FlakeValue::Int(10)).expect("term").to_string();
        assert_ne!(
            rendered, "\"10\"",
            "an untyped literal cannot be compared numerically"
        );
    }

    #[test]
    fn values_round_trip_where_the_mapping_is_unambiguous() {
        for value in [
            FlakeValue::Ref(Sid::dsc("x")),
            FlakeValue::String("hello".into()),
            FlakeValue::Boolean(false),
            FlakeValue::Int(-7),
            FlakeValue::Instant(Utc.timestamp_opt(1_700_000_000, 0).unwrap()),
        ] {
            let term = to_term(&value).expect("term");
            assert_eq!(from_term(&term), Ok(value.clone()), "{value:?}");
        }
    }

    /// `Int` and `Duration` share a lexical form and a datatype. The inverse
    /// therefore cannot recover which was stored, and must not pretend to —
    /// it returns the one that compares equal, which is what matching needs.
    #[test]
    fn a_duration_and_an_integer_map_to_the_same_term_by_design() {
        let duration = to_term(&FlakeValue::Duration(3600)).expect("term");
        let integer = to_term(&FlakeValue::Int(3600)).expect("term");
        assert_eq!(duration, integer);
        assert_eq!(from_term(&duration), Ok(FlakeValue::Int(3600)));
    }

    /// An IRI outside the registry is a legitimate query — it matches nothing.
    /// Erroring would make an unanswerable question into a failure, which is
    /// not what a SPARQL engine should do with an unknown IRI.
    #[test]
    fn an_unknown_iri_is_answerable_and_matches_nothing() {
        let term = Term::NamedNode(NamedNode::new("https://example.org/nope").expect("iri"));
        assert!(from_term(&term).is_ok(), "an unknown IRI is not an error");
    }

    /// Blank nodes have no address in a flake store. Coercing one to a string
    /// would make a pattern match rows that do not contain it.
    /// Each datatype guard must be checked, not fallen through. A guard that
    /// always matched would parse every literal as a dateTime — and a boolean
    /// that failed to parse as one would silently become a string, so the
    /// damage is invisible without checking a *non*-dateTime typed literal.
    #[test]
    fn a_typed_literal_is_not_parsed_as_the_wrong_datatype() {
        let boolean = Literal::new_typed_literal("true", xsd::BOOLEAN);
        assert_eq!(
            from_term(&Term::Literal(boolean)),
            Ok(FlakeValue::Boolean(true)),
            "a boolean must not be routed through the dateTime branch"
        );

        let integer = Literal::new_typed_literal("42", xsd::INTEGER);
        assert_eq!(from_term(&Term::Literal(integer)), Ok(FlakeValue::Int(42)));

        // The one that actually exercises the dateTime guard. A *string* whose
        // text happens to be a timestamp must stay a string — the datatype
        // decides, not the shape of the characters. Testing this with a boolean
        // or an integer proves nothing, because earlier arms catch those before
        // the guard is ever reached.
        let looks_like_a_date = Literal::new_typed_literal("2024-01-01T00:00:00Z", xsd::STRING);
        assert_eq!(
            from_term(&Term::Literal(looks_like_a_date)),
            Ok(FlakeValue::String("2024-01-01T00:00:00Z".to_string())),
            "a string that resembles a timestamp is still a string"
        );
    }

    #[test]
    fn a_blank_node_is_refused_rather_than_coerced() {
        let term = Term::BlankNode(oxrdf::BlankNode::default());
        assert!(matches!(
            from_term(&term),
            Err(TermError::Unrepresentable(_))
        ));
    }

    #[test]
    fn non_finite_floats_survive_the_crossing() {
        for f in [f64::INFINITY, f64::NEG_INFINITY] {
            let term = to_term(&FlakeValue::Float(f)).expect("term");
            assert_eq!(from_term(&term), Ok(FlakeValue::Float(f)));
        }
        let nan = to_term(&FlakeValue::Float(f64::NAN)).expect("term");
        match from_term(&nan) {
            Ok(FlakeValue::Float(f)) => assert!(f.is_nan()),
            other => panic!("NaN became {other:?}"),
        }
    }
}
