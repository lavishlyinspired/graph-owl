//! Between a `FlakeValue` and an RDF term.
//!
//! Pure, and the one place the mapping lives. A value that means one thing to
//! the store and another to a query is the class of bug that produces answers
//! nobody can reproduce.

use graph_owl_core::flake::{Direction, FlakeValue, LangString, Sid};
use oxrdf::{BaseDirection, Literal, NamedNode, Term, Triple, vocab::xsd};

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
        // `oxrdf`'s `rdf-12` feature landed in Epic 94 Slice B (forced on
        // by the dependency graph, not chosen here — see this crate's own
        // `Cargo.toml`), so `Term::Triple` is real and this is a genuine
        // construction, not a refusal. Recurses through `to_term` for the
        // inner object — a triple term may itself nest one.
        FlakeValue::TripleTerm(term) => Term::Triple(Box::new(Triple::new(
            to_named_node(&term.s)?,
            to_named_node(&term.p)?,
            to_term(&term.o)?,
        ))),
        FlakeValue::LangString(ls) => match ls.direction {
            None => Literal::new_language_tagged_literal(&ls.text, &ls.language)
                .map_err(|e| invalid_language_tag(&ls.language, &e))?
                .into(),
            Some(direction) => Literal::new_directional_language_tagged_literal(
                &ls.text,
                &ls.language,
                to_base_direction(direction),
            )
            .map_err(|e| invalid_language_tag(&ls.language, &e))?
            .into(),
        },
    })
}

fn to_base_direction(direction: Direction) -> BaseDirection {
    match direction {
        Direction::Ltr => BaseDirection::Ltr,
        Direction::Rtl => BaseDirection::Rtl,
    }
}

fn from_base_direction(direction: BaseDirection) -> Direction {
    match direction {
        BaseDirection::Ltr => Direction::Ltr,
        BaseDirection::Rtl => Direction::Rtl,
    }
}

fn invalid_language_tag(language: &str, error: &oxrdf::LanguageTagParseError) -> TermError {
    TermError::Unrepresentable(format!(
        "{language:?} is not a valid BCP 47 language tag: {error}"
    ))
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
        // This is the general "an ordinary bound term happens to be a
        // triple term" case — a real one, once `rdf-12` is enabled, is
        // *never* addressable through this function: this store has no
        // `Sid` for a triple term, so there is nothing for the caller to
        // filter or bind against. `rdf:reifies` matching is a special
        // pattern the query surface recognizes and synthesizes separately
        // (Epic 94 Slice D, `dataset.rs`) — it never reaches an ordinary
        // term conversion like this one.
        Term::Triple(triple) => Err(TermError::Unrepresentable(format!("triple term {triple}"))),
    }
}

fn from_literal(literal: &Literal) -> FlakeValue {
    // A language-tagged literal's own `datatype()` is `rdf:langString` /
    // `rdf:dirLangString`, never one of the `xsd:` datatypes matched
    // below — checked first, and exclusively, because RDF's own model
    // never lets a value carry both a language tag and an `xsd:` datatype
    // at once. Before Epic 94 Slice C this fell through to the generic
    // `_ => String` arm below, silently dropping the language tag on
    // every import — the plan's own stated RED test.
    if let Some(language) = literal.language() {
        return FlakeValue::LangString(LangString {
            text: literal.value().to_string(),
            language: language.to_string(),
            direction: literal.direction().map(from_base_direction),
        });
    }
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
            FlakeValue::LangString(LangString {
                text: "hello".into(),
                language: "en".into(),
                direction: None,
            }),
        ] {
            let term = to_term(&value).expect("term");
            assert_eq!(from_term(&term), Ok(value.clone()), "{value:?}");
        }
    }

    /// **The RED test, Epic 94 Slice C's own stated acceptance criterion**:
    /// an `rtl` literal survives storage and serialization with its
    /// direction intact — asserted with real Arabic and Hebrew text, not a
    /// placeholder, because a direction-handling bug could easily pass on
    /// ASCII input (which has no strong direction of its own to get wrong).
    #[test]
    fn an_rtl_literal_keeps_its_direction_through_the_crossing() {
        for (text, language) in [("مرحبا", "ar"), ("שלום", "he")] {
            let value = FlakeValue::LangString(LangString {
                text: text.into(),
                language: language.into(),
                direction: Some(Direction::Rtl),
            });
            let term = to_term(&value).expect("term");
            assert!(
                term.to_string().contains("--rtl"),
                "{text}: {term} does not carry its direction in the lexical form"
            );
            assert_eq!(from_term(&term), Ok(value), "{text} did not round-trip");
        }
    }

    /// The negative case matters as much: a plain string must not acquire
    /// a direction on the way through, or every literal in the catalog
    /// would gain a meaningless `ltr`.
    #[test]
    fn a_plain_string_does_not_acquire_a_language_or_direction() {
        let term = to_term(&FlakeValue::String("hello".into())).expect("term");
        assert_eq!(from_term(&term), Ok(FlakeValue::String("hello".into())));
    }

    /// A language-tagged literal with **no** direction is `rdf:langString`,
    /// not `rdf:dirLangString` — it must not silently acquire one either.
    #[test]
    fn a_language_tagged_literal_without_direction_stays_without_one() {
        let value = FlakeValue::LangString(LangString {
            text: "hello".into(),
            language: "en".into(),
            direction: None,
        });
        let term = to_term(&value).expect("term");
        assert!(
            !term.to_string().contains("--"),
            "a plain rdf:langString must not gain a direction suffix: {term}"
        );
        assert_eq!(from_term(&term), Ok(value));
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

    /// **The RED test**: `Term::Triple` exists only under `oxrdf`'s
    /// `rdf-12` feature, which this crate does not enable yet (Epic 94
    /// Slice D). Until then a stored triple term genuinely has no `Term`
    /// to become — refused by name, not coerced into a string that would
    /// make a query silently match on the wrong thing.
    #[test]
    fn a_triple_term_becomes_a_real_term_triple() {
        let value = FlakeValue::TripleTerm(graph_owl_core::flake::TripleTerm {
            s: Sid::dsc("a"),
            p: Sid::dsc("b"),
            o: Box::new(FlakeValue::Ref(Sid::dsc("c"))),
        });
        let Term::Triple(inner) = to_term(&value).expect("term") else {
            panic!("expected Term::Triple");
        };
        assert_eq!(
            inner.subject.to_string(),
            "<https://graph-owl.dev/ns/catalog#a>"
        );
        assert_eq!(
            inner.predicate.to_string(),
            "<https://graph-owl.dev/ns/catalog#b>"
        );
        assert_eq!(
            inner.object.to_string(),
            "<https://graph-owl.dev/ns/catalog#c>"
        );
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
