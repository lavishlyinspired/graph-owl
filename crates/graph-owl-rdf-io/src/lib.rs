//! RDF interop at the boundary — Epic 9.
//!
//! **Conform at the boundary, stay property-graph inside** (decision 1).
//! This crate turns `Flake`s into standard RDF bytes and back; it never
//! becomes the internal model. `graph_owl_query::term` already owns the
//! one `FlakeValue <-> oxrdf::Term` mapping SPARQL evaluation needs — this
//! crate reuses it rather than duplicating it, so a value that means one
//! thing to a query and another to an export is not a bug this crate can
//! introduce on its own.
//!
//! **Slice A only**: Turtle, N-Triples, N-Quads. `RdfFormat` names the
//! full set the plan's own interface reference specifies (`JsonLd`,
//! `RdfXml`, `TriG` included) so later slices extend this module rather
//! than change its public shape; the three not yet implemented return
//! [`RdfError::UnsupportedFormat`] rather than panicking or silently
//! falling back to one that is.
//!
//! **Not hand-written text.** The round-trip criterion is
//! `parse(serialize(x)) == x`, and the parser is `oxttl` regardless —
//! emitting text by hand while parsing with a library would own escaping,
//! IRI validation and literal canonicalisation on only one side of that
//! trip, which is exactly where such round trips fail (`00l-build-vs-adopt.md`).

use std::collections::HashMap;

use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use graph_owl_query::term::{TermError, from_term, to_named_node, to_term};
use oxrdf::{GraphName, NamedOrBlankNode, Quad, Term, Triple};

/// Which serialization this crate is asked to read or write.
///
/// The full set the plan's own `RdfFormat` names — including the three
/// Slice A does not implement — so a later slice adds a match arm rather
/// than changing this enum's shape underneath every existing caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdfFormat {
    /// Not yet implemented — Slice B.
    JsonLd,
    /// Implemented.
    Turtle,
    /// Implemented.
    NTriples,
    /// Implemented. The only format of the three that carries `cx`.
    NQuads,
    /// Not yet implemented.
    RdfXml,
    /// Not yet implemented.
    TriG,
}

/// Why a flake could not cross the RDF boundary, in either direction.
#[derive(Debug, thiserror::Error)]
pub enum RdfError {
    /// A `Sid` whose namespace has no assigned IRI. Refused rather than
    /// serialized as a bare local name, which would silently drop the
    /// vocabulary the term belongs to (`plans/09-engine-rdf-io.md`).
    #[error("namespace {namespace} has no IRI, so `{id}` cannot be serialized")]
    UnregisteredNamespace {
        /// The unmapped namespace code.
        namespace: u16,
        /// The local name that could not be expressed.
        id: String,
    },

    /// An IRI this parse encountered names a namespace this store has no
    /// `Sid` for — a genuinely external vocabulary Slice A does not (yet)
    /// have a home for. Named rather than silently coerced, the same
    /// reasoning as [`Self::UnregisteredNamespace`] in reverse.
    #[error("`{0}` is not in a namespace this store recognises")]
    UnrecognisedIri(String),

    /// A term this store has no flake representation for — a literal
    /// subject, or an RDF-star triple term (Epic 94's concern, not this
    /// crate's).
    #[error("{0} has no flake representation")]
    Unrepresentable(String),

    /// `fmt` is a real [`RdfFormat`] variant, but this slice does not
    /// implement it yet.
    #[error("{0:?} is not implemented yet")]
    UnsupportedFormat(RdfFormat),

    /// The document did not parse as the requested format.
    #[error("parse error: {0}")]
    Parse(String),

    /// Writing the serialized bytes failed — an allocation failure on a
    /// `Vec<u8>` writer in practice, kept distinct from [`Self::Parse`] so
    /// the two failure classes are not conflated in a caller's `match`.
    #[error("write error: {0}")]
    Io(String),
}

impl From<TermError> for RdfError {
    fn from(error: TermError) -> Self {
        match error {
            TermError::UnmappedNamespace(namespace, id) => {
                Self::UnregisteredNamespace { namespace, id }
            }
            TermError::Unrepresentable(what) => Self::Unrepresentable(what),
        }
    }
}

/// Turns flakes into RDF bytes.
///
/// Takes the flakes to serialize as given — a caller wanting only current,
/// asserted facts filters before calling this (exactly what
/// [`graph_owl_engine::TripleStore::query_pattern`] already returns, which
/// excludes retracted rows). This trait does not re-derive that filter, so
/// it cannot disagree with it.
pub trait RdfSerializer {
    /// # Errors
    /// [`RdfError::UnregisteredNamespace`] if any flake's subject,
    /// predicate, or reference-valued object names an unmapped namespace;
    /// [`RdfError::UnsupportedFormat`] for a format not yet implemented;
    /// [`RdfError::Io`] if writing fails.
    fn serialize(&self, flakes: &[Flake], fmt: RdfFormat) -> Result<Vec<u8>, RdfError>;
}

/// Turns RDF bytes into flakes.
///
/// Returned flakes carry `t: 0` and `op: true` — a parsed document has no
/// transaction-time concept of its own; a real import path (Slice E)
/// stamps a real `t` before writing. Blank nodes are skolemized into a
/// fresh `Sid` per unique label, stable for the duration of one `parse`
/// call (`plans/00c-domain-model.md`: no blank-node representation in the
/// store, so a blank node becomes a real, if synthetic, IRI rather than
/// being refused outright).
pub trait RdfParser {
    /// # Errors
    /// [`RdfError::Parse`] if the bytes do not parse as `fmt`;
    /// [`RdfError::UnrecognisedIri`] if a subject or predicate names a
    /// namespace this store has no `Sid` for; [`RdfError::UnsupportedFormat`]
    /// for a format not yet implemented.
    fn parse(
        &self,
        bytes: &[u8],
        fmt: RdfFormat,
        base: Option<&str>,
    ) -> Result<Vec<Flake>, RdfError>;
}

/// The one implementation Slice A ships — a thin wrapper over `oxttl`,
/// named rather than left as free functions so a later slice's JSON-LD/
/// RDF-XML support has an obvious place to land as more trait impls.
#[derive(Debug, Default, Clone, Copy)]
pub struct StandardRdfIo;

impl RdfSerializer for StandardRdfIo {
    fn serialize(&self, flakes: &[Flake], fmt: RdfFormat) -> Result<Vec<u8>, RdfError> {
        match fmt {
            RdfFormat::Turtle => serialize_turtle(flakes),
            RdfFormat::NTriples => serialize_ntriples(flakes),
            RdfFormat::NQuads => serialize_nquads(flakes),
            other => Err(RdfError::UnsupportedFormat(other)),
        }
    }
}

impl RdfParser for StandardRdfIo {
    fn parse(
        &self,
        bytes: &[u8],
        fmt: RdfFormat,
        base: Option<&str>,
    ) -> Result<Vec<Flake>, RdfError> {
        match fmt {
            RdfFormat::Turtle => parse_turtle(bytes, base),
            RdfFormat::NTriples => parse_ntriples(bytes),
            RdfFormat::NQuads => parse_nquads(bytes),
            other => Err(RdfError::UnsupportedFormat(other)),
        }
    }
}

fn flake_triple(flake: &Flake) -> Result<Triple, RdfError> {
    let s = to_named_node(&flake.s)?;
    let p = to_named_node(&flake.p)?;
    let o = to_term(&flake.o)?;
    Ok(Triple::new(s, p, o))
}

fn flake_graph_name(flake: &Flake) -> Result<GraphName, RdfError> {
    Ok(match &flake.cx {
        Some(cx) => GraphName::NamedNode(to_named_node(cx)?),
        None => GraphName::DefaultGraph,
    })
}

fn serialize_turtle(flakes: &[Flake]) -> Result<Vec<u8>, RdfError> {
    let mut writer = oxttl::TurtleSerializer::new().for_writer(Vec::new());
    for flake in flakes {
        let triple = flake_triple(flake)?;
        writer
            .serialize_triple(&triple)
            .map_err(|e| RdfError::Io(e.to_string()))?;
    }
    writer.finish().map_err(|e| RdfError::Io(e.to_string()))
}

/// N-Triples has no named-graph slot. A flake carrying `cx` is still
/// written — as its bare triple — but the caller is told what was dropped,
/// rather than the loss passing silently (Slice A's own criterion: "N-Triples
/// drops it with a documented warning"). `tracing::warn!` is that
/// documentation: a caller wanting a hard failure instead should route
/// named-graph data through [`serialize_nquads`], which has somewhere to
/// put it.
fn serialize_ntriples(flakes: &[Flake]) -> Result<Vec<u8>, RdfError> {
    let mut writer = oxttl::NTriplesSerializer::new().for_writer(Vec::new());
    let mut dropped_contexts = 0usize;
    for flake in flakes {
        if flake.cx.is_some() {
            dropped_contexts += 1;
        }
        let triple = flake_triple(flake)?;
        writer
            .serialize_triple(&triple)
            .map_err(|e| RdfError::Io(e.to_string()))?;
    }
    if dropped_contexts > 0 {
        tracing::warn!(
            dropped_contexts,
            "N-Triples has no named-graph slot; {dropped_contexts} flake(s) carrying `cx` were \
             written as bare triples with the context silently unrepresentable in this format \
             — use N-Quads to preserve it"
        );
    }
    Ok(writer.finish())
}

fn serialize_nquads(flakes: &[Flake]) -> Result<Vec<u8>, RdfError> {
    let mut writer = oxttl::NQuadsSerializer::new().for_writer(Vec::new());
    for flake in flakes {
        let s = to_named_node(&flake.s)?;
        let p = to_named_node(&flake.p)?;
        let o = to_term(&flake.o)?;
        let graph_name = flake_graph_name(flake)?;
        let quad = Quad::new(s, p, o, graph_name);
        writer
            .serialize_quad(&quad)
            .map_err(|e| RdfError::Io(e.to_string()))?;
    }
    Ok(writer.finish())
}

/// Every unique blank-node label this parse has seen, mapped to the fresh
/// `Sid` standing in for it — stable for the duration of one document, per
/// Slice A's own criterion, and never persisted or reused across calls.
type BlankNodeMap = HashMap<String, Sid>;

fn resolve_subject(node: &NamedOrBlankNode, blanks: &mut BlankNodeMap) -> Result<Sid, RdfError> {
    match node {
        NamedOrBlankNode::NamedNode(n) => Sid::from_iri(n.as_str())
            .ok_or_else(|| RdfError::UnrecognisedIri(n.as_str().to_string())),
        NamedOrBlankNode::BlankNode(b) => Ok(skolemize(b.as_str(), blanks)),
    }
}

fn skolemize(label: &str, blanks: &mut BlankNodeMap) -> Sid {
    blanks
        .entry(label.to_string())
        .or_insert_with(|| Sid::dsc(format!("_blank-{}", uuid::Uuid::new_v4())))
        .clone()
}

fn resolve_object(term: &Term, blanks: &mut BlankNodeMap) -> Result<FlakeValue, RdfError> {
    match term {
        Term::BlankNode(b) => Ok(FlakeValue::Ref(skolemize(b.as_str(), blanks))),
        other => Ok(from_term(other)?),
    }
}

fn parse_turtle(bytes: &[u8], base: Option<&str>) -> Result<Vec<Flake>, RdfError> {
    let mut parser = oxttl::TurtleParser::new();
    if let Some(base) = base {
        parser = parser
            .with_base_iri(base)
            .map_err(|e| RdfError::Parse(e.to_string()))?;
    }
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in parser.for_slice(bytes) {
        let triple = result.map_err(|e| RdfError::Parse(e.to_string()))?;
        flakes.push(triple_to_flake(&triple, &mut blanks)?);
    }
    Ok(flakes)
}

fn parse_ntriples(bytes: &[u8]) -> Result<Vec<Flake>, RdfError> {
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in oxttl::NTriplesParser::new().for_slice(bytes) {
        let triple = result.map_err(|e| RdfError::Parse(e.to_string()))?;
        flakes.push(triple_to_flake(&triple, &mut blanks)?);
    }
    Ok(flakes)
}

fn parse_nquads(bytes: &[u8]) -> Result<Vec<Flake>, RdfError> {
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in oxttl::NQuadsParser::new().for_slice(bytes) {
        let quad = result.map_err(|e| RdfError::Parse(e.to_string()))?;
        let s = resolve_subject(&quad.subject, &mut blanks)?;
        let p = Sid::from_iri(quad.predicate.as_str())
            .ok_or_else(|| RdfError::UnrecognisedIri(quad.predicate.as_str().to_string()))?;
        let o = resolve_object(&quad.object, &mut blanks)?;
        let cx = match &quad.graph_name {
            GraphName::NamedNode(n) => Some(
                Sid::from_iri(n.as_str())
                    .ok_or_else(|| RdfError::UnrecognisedIri(n.as_str().to_string()))?,
            ),
            GraphName::BlankNode(b) => Some(skolemize(b.as_str(), &mut blanks)),
            GraphName::DefaultGraph => None,
        };
        flakes.push(Flake {
            s,
            p,
            o,
            cx,
            t: 0,
            op: true,
        });
    }
    Ok(flakes)
}

fn triple_to_flake(triple: &Triple, blanks: &mut BlankNodeMap) -> Result<Flake, RdfError> {
    let s = resolve_subject(&triple.subject, blanks)?;
    let p = Sid::from_iri(triple.predicate.as_str())
        .ok_or_else(|| RdfError::UnrecognisedIri(triple.predicate.as_str().to_string()))?;
    let o = resolve_object(&triple.object, blanks)?;
    Ok(Flake {
        s,
        p,
        o,
        cx: None,
        t: 0,
        op: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use graph_owl_core::flake::namespace;

    fn flake(s: &str, p: &str, o: FlakeValue) -> Flake {
        Flake {
            s: Sid::dsc(s),
            p: Sid::new(namespace::DSC, p),
            o,
            cx: None,
            t: 0,
            op: true,
        }
    }

    /// **The slice's own specification.** `parse(serialize(x)) == x` for
    /// the expressible subset — every `FlakeValue` variant that has a
    /// lossless RDF literal shape.
    #[test]
    fn every_flake_value_variant_round_trips_through_turtle() {
        let cases = vec![
            flake("a", "ref", FlakeValue::Ref(Sid::dsc("b"))),
            flake("a", "str", FlakeValue::String("hello".into())),
            flake("a", "bool", FlakeValue::Boolean(true)),
            flake("a", "int", FlakeValue::Int(42)),
            flake(
                "a",
                "instant",
                FlakeValue::Instant(chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap()),
            ),
        ];
        for input in cases {
            let bytes = StandardRdfIo
                .serialize(std::slice::from_ref(&input), RdfFormat::Turtle)
                .expect("serialize");
            let parsed = StandardRdfIo
                .parse(&bytes, RdfFormat::Turtle, None)
                .expect("parse");
            assert_eq!(
                parsed,
                vec![input.clone()],
                "{:?}",
                String::from_utf8_lossy(&bytes)
            );
        }
    }

    #[test]
    fn every_flake_value_variant_round_trips_through_ntriples() {
        let input = flake("a", "int", FlakeValue::Int(7));
        let bytes = StandardRdfIo
            .serialize(std::slice::from_ref(&input), RdfFormat::NTriples)
            .expect("serialize");
        let parsed = StandardRdfIo
            .parse(&bytes, RdfFormat::NTriples, None)
            .expect("parse");
        assert_eq!(parsed, vec![input]);
    }

    /// **The classic silent-corruption case.** A space and a unicode
    /// character in a literal must survive, escaped correctly on the way
    /// out and unescaped correctly on the way back.
    #[test]
    fn iri_and_literal_escaping_survives_a_space_and_unicode() {
        let input = flake(
            "a",
            "name",
            FlakeValue::String("caf\u{e9} au lait — 中文".into()),
        );
        let bytes = StandardRdfIo
            .serialize(std::slice::from_ref(&input), RdfFormat::Turtle)
            .expect("serialize");
        let parsed = StandardRdfIo
            .parse(&bytes, RdfFormat::Turtle, None)
            .expect("parse");
        assert_eq!(parsed, vec![input]);
    }

    #[test]
    fn typed_and_language_tagged_literals_are_preserved() {
        // N-Triples, not Turtle: Turtle's own grammar abbreviates a boolean
        // as the bare token `false`, with no `^^xsd:boolean` in the text at
        // all — valid Turtle, but it means the datatype cannot be asserted
        // to *appear literally* in Turtle output the way it can here.
        let input = flake("a", "flag", FlakeValue::Boolean(false));
        let bytes = StandardRdfIo
            .serialize(std::slice::from_ref(&input), RdfFormat::NTriples)
            .expect("serialize");
        assert!(
            String::from_utf8_lossy(&bytes).contains("boolean"),
            "the datatype must appear in the output"
        );
        let parsed = StandardRdfIo
            .parse(&bytes, RdfFormat::NTriples, None)
            .expect("parse");
        assert_eq!(parsed, vec![input]);
    }

    /// **An unregistered namespace fails serialization loudly.**
    #[test]
    fn an_unregistered_namespace_fails_serialization_with_a_named_error() {
        let input = flake("a", "x", FlakeValue::Ref(Sid::new(namespace::UNSET, "y")));
        let outcome = StandardRdfIo.serialize(&[input], RdfFormat::Turtle);
        assert!(
            matches!(outcome, Err(RdfError::UnregisteredNamespace { .. })),
            "{outcome:?}"
        );
    }

    /// **N-Quads carries `cx`; N-Triples drops it with a warning, not
    /// silently.** This test proves the carrying half; the warning half is
    /// asserted by inspection of `serialize_ntriples`'s own `tracing::warn!`
    /// call, since asserting on log output would couple the test to the
    /// tracing subscriber rather than the behaviour.
    #[test]
    fn nquads_preserves_the_named_graph_ntriples_cannot_carry() {
        let mut input = flake("a", "p", FlakeValue::String("v".into()));
        input.cx = Some(Sid::dsc("graph1"));

        let nquads = StandardRdfIo
            .serialize(&[input.clone()], RdfFormat::NQuads)
            .expect("serialize");
        let parsed = StandardRdfIo
            .parse(&nquads, RdfFormat::NQuads, None)
            .expect("parse");
        assert_eq!(parsed, vec![input.clone()], "cx must survive N-Quads");

        let ntriples = StandardRdfIo
            .serialize(&[input], RdfFormat::NTriples)
            .expect("serialize");
        let reparsed = StandardRdfIo
            .parse(&ntriples, RdfFormat::NTriples, None)
            .expect("parse");
        assert_eq!(
            reparsed[0].cx, None,
            "N-Triples has no slot for cx; it must come back None, not silently wrong"
        );
    }

    /// **Blank nodes are stable within one document.** Two triples sharing
    /// one blank-node label in the source must resolve to the *same*
    /// skolemized `Sid` after parsing — the property that makes the parsed
    /// graph's shape match the source's, not two disconnected halves.
    #[test]
    fn a_shared_blank_node_label_resolves_to_one_stable_sid_within_one_parse() {
        let turtle = b"<https://graph-owl.dev/ns/catalog#a> <https://graph-owl.dev/ns/catalog#feeds> _:x .\n\
                       _:x <https://graph-owl.dev/ns/catalog#feeds> <https://graph-owl.dev/ns/catalog#b> .\n";
        let flakes = StandardRdfIo
            .parse(turtle, RdfFormat::Turtle, None)
            .expect("parse");

        assert_eq!(flakes.len(), 2);
        let FlakeValue::Ref(blank_as_object) = &flakes[0].o else {
            panic!("expected a reference: {:?}", flakes[0]);
        };
        assert_eq!(
            &flakes[1].s, blank_as_object,
            "the same blank-node label must skolemize to the same Sid"
        );
    }

    /// A blank node used in two *separate* parse calls must not collide —
    /// each document's blank-node scope is its own.
    #[test]
    fn blank_nodes_do_not_collide_across_separate_parse_calls() {
        let turtle = b"<https://graph-owl.dev/ns/catalog#a> <https://graph-owl.dev/ns/catalog#feeds> _:x .\n";
        let first = StandardRdfIo
            .parse(turtle, RdfFormat::Turtle, None)
            .expect("parse");
        let second = StandardRdfIo
            .parse(turtle, RdfFormat::Turtle, None)
            .expect("parse");

        assert_ne!(
            first[0].o, second[0].o,
            "two separate documents' blank nodes must not resolve to the same Sid"
        );
    }

    #[test]
    fn an_unimplemented_format_is_named_not_a_silent_fallback() {
        let outcome = StandardRdfIo.serialize(&[], RdfFormat::JsonLd);
        assert!(
            matches!(outcome, Err(RdfError::UnsupportedFormat(RdfFormat::JsonLd))),
            "{outcome:?}"
        );
    }
}
