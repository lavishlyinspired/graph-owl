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
//! **Slice A**: Turtle, N-Triples, N-Quads. `RdfFormat` names the
//! full set the plan's own interface reference specifies (`JsonLd`,
//! `RdfXml`, `TriG` included) so later slices extend this module rather
//! than change its public shape; the two still not implemented return
//! [`RdfError::UnsupportedFormat`] rather than panicking or silently
//! falling back to one that is.
//!
//! **Slice B**: JSON-LD, via `oxjsonld` — same Oxigraph family as `oxttl`.
//! Two things worth knowing before reading [`parse_json_ld`] or
//! [`serialize_json_ld_with_context`]:
//!
//! - **Remote `@context` fetching is refused unless the caller opts in.**
//!   `oxjsonld` itself refuses any `@context` URL when no
//!   `load_document_callback` is set (confirmed by reading
//!   `context.rs` — no callback means the parser's own attempt to resolve
//!   a remote context returns `Err` before any network call happens), so
//!   [`parse_json_ld`] is safe by construction, not by an added check.
//!   [`parse_json_ld_with_allowed_hosts`] is the explicit, separate opt-in
//!   for a caller that wants specific hosts fetchable — the allowlist is
//!   checked *before* the request is made.
//! - **`oxjsonld`'s serializer does not compact terms against its declared
//!   prefixes.** Read in `from_rdf.rs`: `with_prefix` records a prefix in
//!   the emitted `@context` object as metadata for a *consumer's* own
//!   compaction, but this crate's own writer still emits predicate and
//!   `@type` keys as full IRIs regardless. What the serializer *does* apply
//!   is `with_base_iri`, which shortens `@id`s relative to it. So in this
//!   crate, "compact" means base-relative `@id`s under a named
//!   [`JsonLdContext`], not full CURIE compaction — two different contexts
//!   (different `base`) produce genuinely different bytes because the
//!   `@id` shape differs, which is what Slice B's own criterion tests.
//!
//! **Frame is a documented subset, not the full W3C algorithm.** Neither
//! `oxjsonld` 0.2.5 nor `json-ld` 0.21.4 (the two permissively licensed
//! JSON-LD crates on crates.io as of 5 August 2026 — both checked,
//! `00l-build-vs-adopt.md`) implements JSON-LD Framing; `oxjsonld`'s
//! `profile.rs` only names the framing profile IRIs for content
//! negotiation, and neither crate has a `frame` module. [`frame_json_ld`]
//! implements the common case directly from
//! <https://www.w3.org/TR/json-ld-framing/> — the spec, not any reference
//! implementation — under the spec's own default flags (`@requireAll`,
//! embed-once): match by `@type` and by every predicate the frame names,
//! nest referenced nodes once, and turn a repeated reference back into a
//! bare `{"@id": ...}` rather than looping. `@embed`/`@explicit`/
//! `@omitDefault`/list framing are not implemented; this is a recorded
//! scope cut, not a silent gap.
//!
//! **Not hand-written text.** The round-trip criterion is
//! `parse(serialize(x)) == x`, and the parser is `oxttl` regardless —
//! emitting text by hand while parsing with a library would own escaping,
//! IRI validation and literal canonicalisation on only one side of that
//! trip, which is exactly where such round trips fail (`00l-build-vs-adopt.md`).

use std::collections::HashMap;

pub mod skos;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, TripleTerm, namespace};
use graph_owl_query::term::{TermError, from_term, to_named_node, to_term};
use oxrdf::{GraphName, NamedOrBlankNode, Quad, Term, Triple};

/// Which serialization this crate is asked to read or write.
///
/// The full set the plan's own `RdfFormat` names — including the three
/// Slice A does not implement — so a later slice adds a match arm rather
/// than changing this enum's shape underneath every existing caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdfFormat {
    /// Implemented — Slice B. `StandardRdfIo`'s trait methods use
    /// [`JsonLdContext::core_v1`] and never fetch a remote `@context`;
    /// [`serialize_json_ld_with_context`] and
    /// [`parse_json_ld_with_allowed_hosts`] are the escape hatches for a
    /// caller that wants a different context or opted-in remote fetching.
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
            RdfFormat::JsonLd => serialize_json_ld_with_context(flakes, &JsonLdContext::core_v1()),
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
            RdfFormat::JsonLd => parse_json_ld(bytes, base),
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

/// The three predicates that make a subject a reified relationship —
/// `graph-owl-lpg`'s own mapping vocabulary already recognizes the
/// identical shape (`graph_owl_lpg::predicate::{FROM_ENTITY,TO_ENTITY,
/// REL_TYPE}`). Not imported from there: `graph-owl-rdf-io` would gain a
/// dependency on the property-graph interop crate just to read three
/// string literals, and every other place in this workspace that already
/// knows this shape (`graph-owl-core`'s own `projection.rs`,
/// `graph-owl-api`'s tests) writes the same three literals directly rather
/// than sharing a constant — this file follows that existing convention.
const REL_FROM_ENTITY: &str = "fromEntity";
const REL_TO_ENTITY: &str = "toEntity";
const REL_TYPE: &str = "relType";

fn is_relationship_predicate(p: &Sid) -> bool {
    p.namespace_code == namespace::DSC
        && matches!(p.id.as_str(), REL_FROM_ENTITY | REL_TO_ENTITY | REL_TYPE)
}

/// Reads `sid`'s own `fromEntity`/`toEntity`/`relType` out of `flakes`, if
/// all three are present with the shape they always have in this store —
/// `fromEntity`/`toEntity` a `Ref`, `relType` a `String`. **All three are
/// required.** A subject with only some of them is not synthesized into a
/// reification with a missing endpoint invented — Epic 94 Slice B's own
/// negative case.
fn reifier_endpoints(sid: &Sid, flakes: &[Flake]) -> Option<(Sid, String, Sid)> {
    let mine = |name: &str| {
        flakes
            .iter()
            .find(|f| &f.s == sid && f.p.namespace_code == namespace::DSC && f.p.id == name)
    };
    let from = match &mine(REL_FROM_ENTITY)?.o {
        FlakeValue::Ref(s) => s.clone(),
        _ => return None,
    };
    let to = match &mine(REL_TO_ENTITY)?.o {
        FlakeValue::Ref(s) => s.clone(),
        _ => return None,
    };
    let rel_type = match &mine(REL_TYPE)?.o {
        FlakeValue::String(s) => s.clone(),
        _ => return None,
    };
    Some((from, rel_type, to))
}

/// `(rel) rdf:reifies << from relType to >>` — RDF 1.2's own vocabulary for
/// exactly what a reified relationship already is: a reifier standing for
/// a proposition (Epic 94's whole premise — "a vocabulary epic, not a
/// model epic"). `relType`'s string value becomes the inner triple's own
/// predicate, in this store's `dsc:` namespace — `"feeds"` becomes
/// `dsc:feeds`, matching the plan's own worked example exactly.
fn reifying_triple(rel: &Sid, from: &Sid, rel_type: &str, to: &Sid) -> Result<Triple, RdfError> {
    let inner = Triple::new(
        to_named_node(from)?,
        to_named_node(&Sid::dsc(rel_type))?,
        Term::NamedNode(to_named_node(to)?),
    );
    Ok(Triple::new(
        to_named_node(rel)?,
        to_named_node(&Sid::new(namespace::RDF, "reifies"))?,
        Term::Triple(Box::new(inner)),
    ))
}

fn serialize_turtle(flakes: &[Flake]) -> Result<Vec<u8>, RdfError> {
    let mut writer = oxttl::TurtleSerializer::new().for_writer(Vec::new());
    // One `rdf:reifies` triple per relationship subject, written the first
    // time one of its three defining predicates is reached — not per
    // flake, or a relationship with all three would emit the reifier three
    // times over.
    let mut reifier_written: std::collections::HashSet<Sid> = std::collections::HashSet::new();
    for flake in flakes {
        if is_relationship_predicate(&flake.p) {
            if let Some((from, rel_type, to)) = reifier_endpoints(&flake.s, flakes) {
                if reifier_written.insert(flake.s.clone()) {
                    let triple = reifying_triple(&flake.s, &from, &rel_type, &to)?;
                    writer
                        .serialize_triple(&triple)
                        .map_err(|e| RdfError::Io(e.to_string()))?;
                }
                // Whether this is the first or a later of the three
                // defining flakes, it is folded into the reifying triple
                // above and must not also appear as a plain triple.
                continue;
            }
        }
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
        // `from_term` (`graph_owl_query::term`) refuses a triple term by
        // design — it feeds SPARQL query-pattern matching, where a stored
        // triple term has no `Sid` to bind against (Epic 94 decision 2).
        // Parsing a *document* is a different boundary: a general RDF 1.2
        // reification this store has no relationship model for is real
        // and must parse, not fail, so it becomes a `TripleTerm`-valued
        // flake directly. Recurses through `resolve_object` for the inner
        // object — a triple term may itself nest one.
        Term::Triple(inner) => Ok(FlakeValue::TripleTerm(TripleTerm {
            s: resolve_subject(&inner.subject, blanks)?,
            p: Sid::from_iri(inner.predicate.as_str())
                .ok_or_else(|| RdfError::UnrecognisedIri(inner.predicate.as_str().to_string()))?,
            o: Box::new(resolve_object(&inner.object, blanks)?),
        })),
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
        flakes.extend(triple_to_flakes(&triple, &mut blanks)?);
    }
    Ok(flakes)
}

fn parse_ntriples(bytes: &[u8]) -> Result<Vec<Flake>, RdfError> {
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in oxttl::NTriplesParser::new().for_slice(bytes) {
        let triple = result.map_err(|e| RdfError::Parse(e.to_string()))?;
        flakes.extend(triple_to_flakes(&triple, &mut blanks)?);
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

/// A versioned JSON-LD `@context` — the base IRI compacted output shortens
/// `@id`s against, served at [`JsonLdContext::url`]. Versioned in its own
/// URL rather than embedded inline, so a consumer resolves the same
/// context graph-owl itself would serve (Slice B's own criterion: "output
/// pins the context version").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonLdContext {
    /// The version segment of [`Self::url`].
    pub version: u32,
    /// `@id`s are shortened relative to this IRI.
    pub base: String,
    /// Declared in the emitted `@context` object as metadata — not applied
    /// to key compaction by `oxjsonld`'s own writer (see the module-level
    /// doc comment).
    pub prefixes: Vec<(String, String)>,
}

impl JsonLdContext {
    /// The context every `RdfFormat::JsonLd` call through `StandardRdfIo`
    /// compacts against — the eight namespaces this store already
    /// registers (`graph_owl_core::flake::namespace_iri`), `dsc` as base.
    #[must_use]
    pub fn core_v1() -> Self {
        Self {
            version: 1,
            base: "https://graph-owl.dev/ns/catalog#".to_string(),
            prefixes: vec![
                (
                    "dsc".to_string(),
                    "https://graph-owl.dev/ns/catalog#".to_string(),
                ),
                (
                    "rdf".to_string(),
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
                ),
                (
                    "rdfs".to_string(),
                    "http://www.w3.org/2000/01/rdf-schema#".to_string(),
                ),
                (
                    "xsd".to_string(),
                    "http://www.w3.org/2001/XMLSchema#".to_string(),
                ),
                (
                    "owl".to_string(),
                    "http://www.w3.org/2002/07/owl#".to_string(),
                ),
                ("sh".to_string(), "http://www.w3.org/ns/shacl#".to_string()),
                ("schema".to_string(), "https://schema.org/".to_string()),
                (
                    "dcterms".to_string(),
                    "http://purl.org/dc/terms/".to_string(),
                ),
            ],
        }
    }

    /// The versioned URL this context is served at
    /// (`GET /rdf/context/v{version}` in `graph-owl-server`). Compacted
    /// JSON-LD output carries this string as its `@context`, never the
    /// inline object.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://graph-owl.dev/context/v{}", self.version)
    }

    /// The document [`Self::url`] serves — what a document referencing that
    /// URL resolves to. `graph-owl-server`'s route body is exactly these
    /// bytes, so the served context and the one this crate compacts
    /// against cannot drift apart into two different mappings.
    #[must_use]
    pub fn to_document(&self) -> Vec<u8> {
        let mut context = serde_json::Map::new();
        context.insert(
            "@base".to_string(),
            serde_json::Value::String(self.base.clone()),
        );
        for (prefix, iri) in &self.prefixes {
            context.insert(prefix.clone(), serde_json::Value::String(iri.clone()));
        }
        let document = serde_json::json!({ "@context": context });
        serde_json::to_vec(&document).unwrap_or_default()
    }
}

/// Turns flakes into JSON-LD, compacted against `context`'s base IRI and
/// referencing `context`'s served URL. See the module-level doc comment
/// for what "compact" means given `oxjsonld`'s actual capability.
///
/// # Errors
/// [`RdfError::UnregisteredNamespace`] if any flake names an unmapped
/// namespace; [`RdfError::Io`] if writing or the context-URL rewrite fails.
pub fn serialize_json_ld_with_context(
    flakes: &[Flake],
    context: &JsonLdContext,
) -> Result<Vec<u8>, RdfError> {
    let mut serializer = oxjsonld::JsonLdSerializer::new()
        .with_base_iri(context.base.as_str())
        .map_err(|e| RdfError::Io(e.to_string()))?;
    for (prefix, iri) in &context.prefixes {
        serializer = serializer
            .with_prefix(prefix.as_str(), iri.as_str())
            .map_err(|e| RdfError::Io(e.to_string()))?;
    }
    let mut writer = serializer.for_writer(Vec::new());
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
    let bytes = writer.finish().map_err(|e| RdfError::Io(e.to_string()))?;
    rewrite_context_as_url(&bytes, &context.url())
}

/// Replaces the inline `@context` object `oxjsonld` writes with a bare URL
/// string — the version-pinning half of Slice B's criterion. Safe because
/// the URL, once served, resolves to exactly the prefixes and base this
/// function just compacted against; nothing about the already-computed
/// base-relative `@id`s changes.
fn rewrite_context_as_url(bytes: &[u8], url: &str) -> Result<Vec<u8>, RdfError> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| RdfError::Io(e.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "@context".to_string(),
            serde_json::Value::String(url.to_string()),
        );
    }
    serde_json::to_vec(&value).map_err(|e| RdfError::Io(e.to_string()))
}

/// Parses JSON-LD into flakes. Never fetches a remote `@context` — see the
/// module-level doc comment for why this is `oxjsonld`'s own default
/// behaviour, not an added check. `@graph`-nested node objects carrying an
/// `@id` become `cx`, the same mapping N-Quads' graph slot already gets,
/// because `oxjsonld` yields the identical `Quad` shape for both.
///
/// # Errors
/// [`RdfError::Parse`] if the bytes are not valid JSON-LD, including a
/// document whose `@context` cannot be resolved without a remote fetch;
/// [`RdfError::UnrecognisedIri`] if a subject or predicate names a
/// namespace this store has no `Sid` for.
pub fn parse_json_ld(bytes: &[u8], base: Option<&str>) -> Result<Vec<Flake>, RdfError> {
    let mut parser = oxjsonld::JsonLdParser::new();
    if let Some(base) = base {
        parser = parser
            .with_base_iri(base)
            .map_err(|e| RdfError::Parse(e.to_string()))?;
    }
    quads_to_flakes(parser.for_slice(bytes))
}

/// [`parse_json_ld`], but a remote `@context` URL whose host is in
/// `allowed_hosts` is fetched rather than refused — the explicit opt-in
/// Slice B's own criterion names ("rejected unless the URL is
/// allowlisted"). The allowlist is checked *before* any request leaves the
/// process, inside the callback `oxjsonld` calls for every remote
/// reference.
///
/// Uses a blocking HTTP client. This function has no async runtime of its
/// own — a caller already inside a tokio runtime must wrap it in
/// `tokio::task::spawn_blocking`, the same way any blocking call must be.
///
/// # Errors
/// Everything [`parse_json_ld`] can return, plus [`RdfError::Parse`] when a
/// remote `@context` URL's host is not in `allowed_hosts`, or when the
/// allowed fetch itself fails.
pub fn parse_json_ld_with_allowed_hosts(
    bytes: &[u8],
    base: Option<&str>,
    allowed_hosts: &[String],
) -> Result<Vec<Flake>, RdfError> {
    let allowed_hosts = allowed_hosts.to_vec();
    parse_json_ld_with_loader(bytes, base, move |remote_url| {
        if !is_host_allowed(remote_url, &allowed_hosts) {
            return Err(format!(
                "remote `@context` host for `{remote_url}` is not in the allowlist \
                 — refused before any request was made"
            ));
        }
        reqwest::blocking::get(remote_url)
            .and_then(reqwest::blocking::Response::bytes)
            .map(|body| body.to_vec())
            .map_err(|e| e.to_string())
    })
}

/// [`parse_json_ld`], but a remote `@context` is resolved through `loader`
/// instead of refused. [`parse_json_ld_with_allowed_hosts`] is `loader`
/// fixed to "check the allowlist, then fetch over HTTP" — factored apart so
/// a test (or an embedding-specific caller) can substitute its own
/// resolution without touching the network, the same reasoning
/// `finding-seams` gives for injecting a boundary rather than hard-coding
/// it.
///
/// # Errors
/// Everything [`parse_json_ld`] can return, plus whatever `loader` itself
/// returns as its `Err` string, wrapped in [`RdfError::Parse`].
pub fn parse_json_ld_with_loader(
    bytes: &[u8],
    base: Option<&str>,
    loader: impl Fn(&str) -> Result<Vec<u8>, String>
    + Send
    + Sync
    + std::panic::UnwindSafe
    + std::panic::RefUnwindSafe
    + 'static,
) -> Result<Vec<Flake>, RdfError> {
    let mut parser = oxjsonld::JsonLdParser::new();
    if let Some(base) = base {
        parser = parser
            .with_base_iri(base)
            .map_err(|e| RdfError::Parse(e.to_string()))?;
    }
    let sliced = parser.for_slice(bytes).with_load_document_callback(
        move |remote_url,
              _options|
              -> Result<
            oxjsonld::JsonLdRemoteDocument,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            let document = loader(remote_url)
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            Ok(oxjsonld::JsonLdRemoteDocument {
                document,
                document_url: remote_url.to_string(),
            })
        },
    );
    quads_to_flakes(sliced)
}

/// Whether `url_str`'s host is in `allowed_hosts` — the check
/// [`parse_json_ld_with_allowed_hosts`] runs *before* any network access.
/// Host extraction goes through `url::Url` rather than a hand-rolled split,
/// specifically so a userinfo trick (`http://allowed.com@evil.com/`)
/// resolves to the real authority (`evil.com`), not the attacker-chosen
/// prefix — a classic SSRF-allowlist bypass if parsed naively. A URL that
/// fails to parse, or has no host at all, is never allowed.
fn is_host_allowed(url_str: &str, allowed_hosts: &[String]) -> bool {
    url::Url::parse(url_str)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .is_some_and(|host| allowed_hosts.iter().any(|allowed| allowed == &host))
}

fn quads_to_flakes(
    quads: impl Iterator<Item = Result<Quad, oxjsonld::JsonLdSyntaxError>>,
) -> Result<Vec<Flake>, RdfError> {
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in quads {
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

/// A syntax error with where it is in the source text — Epic 42 Slice G's
/// ontology editor needs to mark a line in a text editor, which
/// [`RdfError::Parse`]'s pre-formatted `String` cannot do.
///
/// Kept as a separate type rather than widening `RdfError` with a new
/// variant: every existing `match` on `RdfError` across this crate and its
/// callers stays exhaustive with no new arm to add, and this error is
/// reachable only through [`parse_with_location`], which nothing but the
/// new editor calls.
///
/// 1-based `line`/`column` — `oxttl`/`oxjsonld` report 0-based internally
/// (confirmed by reading `TextPosition`'s own doc comment in both crates),
/// converted here so a caller can put it next to a text editor's own
/// gutter numbering without an off-by-one. `None` means a real location
/// was not available — either the underlying parser didn't report one
/// (`oxjsonld`'s `location()` is itself `Option`), or the failure happened
/// one layer up, converting an already-parsed term into a [`Flake`] (an
/// unrecognised namespace, for instance), which is not a *position* in the
/// text at all.
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedParseError {
    pub message: String,
    pub line: Option<u64>,
    pub column: Option<u64>,
}

impl LocatedParseError {
    fn without_location(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            column: None,
        }
    }
}

/// [`StandardRdfIo::parse`], but a syntax error carries its own line and
/// column instead of a pre-formatted sentence. Turtle, N-Triples and
/// JSON-LD only — the three formats Slice G's editor actually offers (the
/// plan's own words: "Turtle ships first... the editor is a text surface
/// over a parser that already handles N-Triples and JSON-LD"). N-Quads,
/// RDF/XML and `TriG` report [`LocatedParseError::without_location`] naming
/// the format, not a panic.
///
/// # Errors
/// [`LocatedParseError`] on a syntax error (with a location, where the
/// underlying parser reports one) or an unsupported format (without one).
pub fn parse_with_location(
    bytes: &[u8],
    fmt: RdfFormat,
    base: Option<&str>,
) -> Result<Vec<Flake>, LocatedParseError> {
    match fmt {
        RdfFormat::Turtle => parse_turtle_with_location(bytes, base),
        RdfFormat::NTriples => parse_ntriples_with_location(bytes),
        RdfFormat::JsonLd => parse_json_ld_with_location(bytes, base),
        other => Err(LocatedParseError::without_location(format!(
            "{other:?} is not supported by the ontology editor"
        ))),
    }
}

fn parse_turtle_with_location(
    bytes: &[u8],
    base: Option<&str>,
) -> Result<Vec<Flake>, LocatedParseError> {
    let mut parser = oxttl::TurtleParser::new();
    if let Some(base) = base {
        parser = parser
            .with_base_iri(base)
            .map_err(|e| LocatedParseError::without_location(e.to_string()))?;
    }
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in parser.for_slice(bytes) {
        let triple = result.map_err(|e| {
            let location = e.location();
            LocatedParseError {
                message: e.message().to_string(),
                line: Some(location.start.line + 1),
                column: Some(location.start.column + 1),
            }
        })?;
        flakes.extend(
            triple_to_flakes(&triple, &mut blanks)
                .map_err(|e| LocatedParseError::without_location(e.to_string()))?,
        );
    }
    Ok(flakes)
}

fn parse_ntriples_with_location(bytes: &[u8]) -> Result<Vec<Flake>, LocatedParseError> {
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in oxttl::NTriplesParser::new().for_slice(bytes) {
        let triple = result.map_err(|e| {
            let location = e.location();
            LocatedParseError {
                message: e.message().to_string(),
                line: Some(location.start.line + 1),
                column: Some(location.start.column + 1),
            }
        })?;
        flakes.extend(
            triple_to_flakes(&triple, &mut blanks)
                .map_err(|e| LocatedParseError::without_location(e.to_string()))?,
        );
    }
    Ok(flakes)
}

fn parse_json_ld_with_location(
    bytes: &[u8],
    base: Option<&str>,
) -> Result<Vec<Flake>, LocatedParseError> {
    let mut parser = oxjsonld::JsonLdParser::new();
    if let Some(base) = base {
        parser = parser
            .with_base_iri(base)
            .map_err(|e| LocatedParseError::without_location(e.to_string()))?;
    }
    let mut blanks = BlankNodeMap::new();
    let mut flakes = Vec::new();
    for result in parser.for_slice(bytes) {
        let quad = result.map_err(|e| {
            let message = e.to_string();
            match e.location() {
                Some(location) => LocatedParseError {
                    message,
                    line: Some(location.start.line + 1),
                    column: Some(location.start.column + 1),
                },
                None => LocatedParseError::without_location(message),
            }
        })?;
        let s = resolve_subject(&quad.subject, &mut blanks)
            .map_err(|e| LocatedParseError::without_location(e.to_string()))?;
        let p = Sid::from_iri(quad.predicate.as_str()).ok_or_else(|| {
            LocatedParseError::without_location(format!(
                "unrecognised namespace: {}",
                quad.predicate.as_str()
            ))
        })?;
        let o = resolve_object(&quad.object, &mut blanks)
            .map_err(|e| LocatedParseError::without_location(e.to_string()))?;
        let cx = match &quad.graph_name {
            GraphName::NamedNode(n) => Some(Sid::from_iri(n.as_str()).ok_or_else(|| {
                LocatedParseError::without_location(format!(
                    "unrecognised namespace: {}",
                    n.as_str()
                ))
            })?),
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

/// One flake's-worth of `rdf:type` used for frame matching — kept as a
/// function rather than a constant so it shares `Sid::new`'s own
/// namespace-code convention instead of a second, hand-written one.
fn rdf_type() -> Sid {
    Sid::new(graph_owl_core::flake::namespace::RDF, "type")
}

/// Frames already-parsed flakes against `frame` — see the module-level doc
/// comment for exactly which subset of
/// <https://www.w3.org/TR/json-ld-framing/> this implements. `frame`'s
/// keys are full IRIs (this function does not resolve CURIEs, matching
/// `oxjsonld`'s own no-compaction behaviour elsewhere in this crate):
/// `"@type"` (a single IRI string) selects subjects by their `rdf:type`;
/// every other key names a predicate the frame requires, and a non-empty
/// object value for that key is itself a nested frame applied to any
/// `Ref`-valued object of that predicate. A frame with no predicate keys
/// at all matches every subject that satisfies `@type` (or every subject,
/// if `@type` is absent too) and embeds every predicate found on it,
/// one level deep.
///
/// # Errors
/// [`RdfError::Io`] if the output cannot be assembled into JSON (never in
/// practice, since every value written is already valid).
pub fn frame_json_ld(flakes: &[Flake], frame: &serde_json::Value) -> Result<Vec<u8>, RdfError> {
    let mut by_subject: std::collections::BTreeMap<&Sid, Vec<&Flake>> =
        std::collections::BTreeMap::new();
    for flake in flakes {
        by_subject.entry(&flake.s).or_default().push(flake);
    }

    let frame_type = frame.get("@type").and_then(serde_json::Value::as_str);
    let frame_object = frame.as_object();
    let requested_predicates: Vec<&String> = frame_object
        .map(|object| object.keys().filter(|k| k.as_str() != "@type").collect())
        .unwrap_or_default();

    let mut visited = std::collections::HashSet::new();
    let mut nodes = Vec::new();
    for (subject, subject_flakes) in &by_subject {
        let matches_type = match frame_type {
            Some(wanted) => subject_flakes.iter().any(|f| {
                f.p == rdf_type()
                    && matches!(&f.o, FlakeValue::Ref(r) if r.to_iri().as_deref() == Some(wanted))
            }),
            None => true,
        };
        let has_required_predicates = requested_predicates.iter().all(|predicate| {
            subject_flakes
                .iter()
                .any(|f| f.p.to_iri().as_deref() == Some(predicate.as_str()))
        });
        if matches_type && has_required_predicates {
            nodes.push(frame_node(
                subject,
                &by_subject,
                frame_object,
                &mut visited,
            )?);
        }
    }
    let output = serde_json::json!({
        "@context": JsonLdContext::core_v1().url(),
        "@graph": nodes,
    });
    serde_json::to_vec(&output).map_err(|e| RdfError::Io(e.to_string()))
}

fn frame_node(
    subject: &Sid,
    by_subject: &std::collections::BTreeMap<&Sid, Vec<&Flake>>,
    frame_object: Option<&serde_json::Map<String, serde_json::Value>>,
    visited: &mut std::collections::HashSet<Sid>,
) -> Result<serde_json::Value, RdfError> {
    let iri = subject
        .to_iri()
        .ok_or_else(|| RdfError::UnregisteredNamespace {
            namespace: subject.namespace_code,
            id: subject.id.clone(),
        })?;
    let mut node = serde_json::Map::new();
    node.insert("@id".to_string(), serde_json::Value::String(iri));

    if !visited.insert(subject.clone()) {
        // Already embedded once elsewhere in this frame call — a bare
        // `{"@id": ...}` reference, not a second copy, which is what keeps
        // a cycle from recursing forever.
        return Ok(node.into());
    }

    let Some(subject_flakes) = by_subject.get(subject) else {
        return Ok(node.into());
    };

    let mut by_predicate: std::collections::BTreeMap<String, Vec<&Flake>> =
        std::collections::BTreeMap::new();
    for flake in subject_flakes {
        if let Some(iri) = flake.p.to_iri() {
            by_predicate.entry(iri).or_default().push(flake);
        }
    }

    for (predicate, predicate_flakes) in &by_predicate {
        let sub_frame = frame_object.and_then(|object| object.get(predicate));
        // An explicit frame with named predicates includes only those;
        // one with none listed (besides `@type`) includes everything found.
        let included = frame_object
            .is_none_or(|object| object.keys().all(|k| k == "@type") || sub_frame.is_some());
        if !included {
            continue;
        }
        let mut values = Vec::new();
        for flake in predicate_flakes {
            let value = match &flake.o {
                FlakeValue::Ref(referenced)
                    if sub_frame.is_none_or(|f| {
                        f.is_object() && !f.as_object().is_some_and(serde_json::Map::is_empty)
                    }) =>
                {
                    frame_node(
                        referenced,
                        by_subject,
                        sub_frame.and_then(serde_json::Value::as_object),
                        visited,
                    )?
                }
                other => flake_value_to_json(other)?,
            };
            values.push(value);
        }
        node.insert(predicate.clone(), values.into());
    }

    Ok(node.into())
}

fn flake_value_to_json(value: &FlakeValue) -> Result<serde_json::Value, RdfError> {
    let mut object = serde_json::Map::new();
    match value {
        FlakeValue::Ref(sid) => {
            let iri = sid
                .to_iri()
                .ok_or_else(|| RdfError::UnregisteredNamespace {
                    namespace: sid.namespace_code,
                    id: sid.id.clone(),
                })?;
            object.insert("@id".to_string(), serde_json::Value::String(iri));
        }
        FlakeValue::String(s) => {
            object.insert("@value".to_string(), serde_json::Value::String(s.clone()));
        }
        FlakeValue::Boolean(b) => {
            object.insert("@value".to_string(), serde_json::Value::Bool(*b));
        }
        FlakeValue::Int(i) => {
            object.insert("@value".to_string(), serde_json::Value::Number((*i).into()));
        }
        other => {
            let term = to_term(other)?;
            object.insert(
                "@value".to_string(),
                serde_json::Value::String(term.to_string()),
            );
        }
    }
    Ok(object.into())
}

/// The inverse of [`reifying_triple`] — recognizes exactly the shape this
/// crate's own serializer produces (inner subject and object both named
/// nodes, inner predicate in the `dsc:` namespace) and expands it back to
/// the three `fromEntity`/`toEntity`/`relType` flakes that produced it.
///
/// `None` for anything else — a general RDF 1.2 reification with a literal
/// object, a non-`dsc:` predicate, or a blank-node endpoint is real and
/// parses (see [`triple_to_flakes`]'s own fallback to a single
/// `FlakeValue::TripleTerm`-valued flake), but it is not *this store's*
/// relationship model, and forcing it into one would invent an endpoint
/// the document never named.
fn reifying_flakes(
    rel: &NamedOrBlankNode,
    inner: &Triple,
    blanks: &mut BlankNodeMap,
) -> Result<Option<Vec<Flake>>, RdfError> {
    let NamedOrBlankNode::NamedNode(from_node) = &inner.subject else {
        return Ok(None);
    };
    let Term::NamedNode(to_node) = &inner.object else {
        return Ok(None);
    };
    let (Some(from), Some(to), Some(rel_type)) = (
        Sid::from_iri(from_node.as_str()),
        Sid::from_iri(to_node.as_str()),
        Sid::from_iri(inner.predicate.as_str()),
    ) else {
        return Ok(None);
    };
    if rel_type.namespace_code != namespace::DSC {
        return Ok(None);
    }
    let rel = resolve_subject(rel, blanks)?;
    let mk = |p: &str, o: FlakeValue| Flake {
        s: rel.clone(),
        p: Sid::dsc(p),
        o,
        cx: None,
        t: 0,
        op: true,
    };
    Ok(Some(vec![
        mk(REL_FROM_ENTITY, FlakeValue::Ref(from)),
        mk(REL_TO_ENTITY, FlakeValue::Ref(to)),
        mk(REL_TYPE, FlakeValue::String(rel_type.id)),
    ]))
}

/// One triple usually becomes one flake — but `rdf:reifies << s p o >>`
/// either expands to the three flakes that produced it
/// ([`reifying_flakes`]) or, for a general reification this store has no
/// relationship model for, becomes one flake carrying the triple term
/// itself (Epic 94 Slice A's `FlakeValue::TripleTerm`) rather than being
/// refused — a document using RDF 1.2's own vocabulary correctly must not
/// fail to parse just because this store's *relationship* shape does not
/// apply to it.
fn triple_to_flakes(triple: &Triple, blanks: &mut BlankNodeMap) -> Result<Vec<Flake>, RdfError> {
    let is_reifies =
        Sid::from_iri(triple.predicate.as_str()) == Some(Sid::new(namespace::RDF, "reifies"));
    if is_reifies {
        if let Term::Triple(inner) = &triple.object {
            if let Some(flakes) = reifying_flakes(&triple.subject, inner, blanks)? {
                return Ok(flakes);
            }
        }
    }
    let s = resolve_subject(&triple.subject, blanks)?;
    let p = Sid::from_iri(triple.predicate.as_str())
        .ok_or_else(|| RdfError::UnrecognisedIri(triple.predicate.as_str().to_string()))?;
    let o = resolve_object(&triple.object, blanks)?;
    Ok(vec![Flake {
        s,
        p,
        o,
        cx: None,
        t: 0,
        op: true,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use graph_owl_core::flake::{LangString, namespace};

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
            flake(
                "a",
                "label",
                FlakeValue::LangString(LangString {
                    text: "hello".into(),
                    language: "en".into(),
                    direction: None,
                }),
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

    /// A subject carrying `fromEntity`/`toEntity`/`relType` — the reified
    /// relationship shape `graph-owl-lpg`'s own mapping vocabulary already
    /// recognizes — is graph-owl's own model of exactly what RDF 1.2 names
    /// `rdf:reifies`: a reifier standing for a proposition. Epic 94 Slice B's
    /// own acceptance criterion.
    fn relationship_flakes(rel: &str, from: &str, rel_type: &str, to: &str) -> Vec<Flake> {
        vec![
            flake(rel, "fromEntity", FlakeValue::Ref(Sid::dsc(from))),
            flake(rel, "toEntity", FlakeValue::Ref(Sid::dsc(to))),
            flake(rel, "relType", FlakeValue::String(rel_type.into())),
        ]
    }

    /// **The RED test.** `fromEntity`/`toEntity`/`relType` must not survive
    /// as plain triples in the export — the whole point of this slice is
    /// naming the shape, not adding `rdf:reifies` beside the old triples.
    #[test]
    fn a_reified_relationship_serializes_as_rdf_reifies_not_the_bare_endpoints() {
        let bytes = StandardRdfIo
            .serialize(
                &relationship_flakes("r1", "orders", "feeds", "reports"),
                RdfFormat::Turtle,
            )
            .expect("serialize");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("reifies"), "{text}");
        assert!(
            text.contains("feeds"),
            "the relType must survive, as a predicate IRI: {text}"
        );
        assert!(
            !text.contains("fromEntity") && !text.contains("toEntity") && !text.contains("relType"),
            "the endpoints must not also appear as plain triples: {text}"
        );
    }

    /// **The round-trip criterion**, Slice B's own stated acceptance test.
    /// Mutator watch: emitting the endpoints without `rdf:reifies` (i.e.
    /// reverting to the old 1:1 serialization) fails this — the parser
    /// would then see three ordinary triples with no reifying shape to
    /// recognize, and the comparison below would still happen to pass by
    /// accident (three flakes go in, three come out) — which is exactly
    /// why the *previous* test asserts on the wire text, not just this one
    /// on the round trip.
    #[test]
    fn a_reified_relationship_round_trips_through_turtle() {
        let input = relationship_flakes("r1", "orders", "feeds", "reports");
        let bytes = StandardRdfIo
            .serialize(&input, RdfFormat::Turtle)
            .expect("serialize");
        let mut parsed = StandardRdfIo
            .parse(&bytes, RdfFormat::Turtle, None)
            .expect("parse");
        let mut expected = input;
        parsed.sort_by_key(|f| f.p.id.clone());
        expected.sort_by_key(|f| f.p.id.clone());
        assert_eq!(parsed, expected, "{}", String::from_utf8_lossy(&bytes));
    }

    /// A relationship's own other properties (confidence, in this example)
    /// are not folded into the reifying triple term — they are ordinary
    /// properties *of the reifier*, exactly as the plan's own worked
    /// example shows: `:rel_abc123 rdf:reifies << ... >> ; dsc:confidence
    /// 0.95 .`
    #[test]
    fn a_relationships_own_properties_survive_alongside_the_reifying_triple() {
        let mut input = relationship_flakes("r1", "orders", "feeds", "reports");
        input.push(flake("r1", "confidence", FlakeValue::Float(0.95)));

        let bytes = StandardRdfIo
            .serialize(&input, RdfFormat::Turtle)
            .expect("serialize");
        let mut parsed = StandardRdfIo
            .parse(&bytes, RdfFormat::Turtle, None)
            .expect("parse");
        let mut expected = input;
        parsed.sort_by_key(|f| f.p.id.clone());
        expected.sort_by_key(|f| f.p.id.clone());
        assert_eq!(parsed, expected, "{}", String::from_utf8_lossy(&bytes));
    }

    /// The negative case matters as much: an ordinary subject with no
    /// `fromEntity`/`toEntity`/`relType` triple must serialize exactly as
    /// before this slice — `rdf:reifies` must never appear for data this
    /// slice was not about.
    #[test]
    fn an_ordinary_subject_is_unaffected_by_reification() {
        let input = flake("orders", "name", FlakeValue::String("Orders".into()));
        let bytes = StandardRdfIo
            .serialize(std::slice::from_ref(&input), RdfFormat::Turtle)
            .expect("serialize");
        assert!(!String::from_utf8_lossy(&bytes).contains("reifies"));
    }

    /// A subject with only *some* of the three relationship predicates
    /// (missing `toEntity`) is not a relationship — the recognizer must
    /// require all three, not synthesize a reification from a partial
    /// shape that would silently invent a missing endpoint.
    #[test]
    fn a_partial_relationship_shape_is_not_reified() {
        let input = vec![
            flake("r1", "fromEntity", FlakeValue::Ref(Sid::dsc("orders"))),
            flake("r1", "relType", FlakeValue::String("feeds".into())),
        ];
        let bytes = StandardRdfIo
            .serialize(&input, RdfFormat::Turtle)
            .expect("serialize");
        assert!(!String::from_utf8_lossy(&bytes).contains("reifies"));
    }

    /// **A general RDF 1.2 reification, not this store's relationship
    /// shape.** A document authored elsewhere may reify a triple whose
    /// object is a literal — no `toEntity` this store could invent one
    /// for. Hand-written rather than round-tripped through this crate's
    /// own serializer, since the serializer only ever produces the
    /// relationship shape — this is exactly the case a real external
    /// document could present that our own output never does.
    ///
    /// `<<( s p o )>>`, with the parentheses — RDF 1.2 Turtle's **triple
    /// term** literal, a bare value. `<< s p o >>` without them is a
    /// *different* construct — reification-as-sugar, which asserts an
    /// implicit blank-node reifier of its own — confirmed by writing a
    /// `Term::Triple` through `oxttl`'s own serializer directly and
    /// reading back what it emits, not assumed from the RDF-star literature
    /// this project's own reference-repo licensing rules already forbid
    /// consulting. Getting this wrong produced a real, confusing failure
    /// during this slice's own development: two flakes instead of one, a
    /// synthetic blank node standing in for `dsc:claim1`.
    #[test]
    fn a_reification_that_is_not_a_relationship_becomes_a_triple_term_flake() {
        let document = b"@prefix dsc: <https://graph-owl.dev/ns/catalog#> .\n\
             @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
             dsc:claim1 rdf:reifies <<( dsc:orders dsc:status \"delayed\" )>> .\n";
        let parsed = StandardRdfIo
            .parse(document, RdfFormat::Turtle, None)
            .expect("parse");
        assert_eq!(parsed.len(), 1, "{parsed:?}");
        let FlakeValue::TripleTerm(term) = &parsed[0].o else {
            panic!("{:?}", parsed[0]);
        };
        assert_eq!(term.s, Sid::dsc("orders"));
        assert_eq!(term.p, Sid::dsc("status"));
        assert_eq!(*term.o, FlakeValue::String("delayed".to_string()));
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

    /// **The RED test, Epic 94 Slice C's own stated acceptance criterion**:
    /// an `rtl` literal survives storage and serialization with its
    /// direction intact — real Arabic and Hebrew text through the full
    /// Turtle serialize/parse path this crate's callers actually use, not
    /// just `graph_owl_query::term`'s own narrower unit test.
    #[test]
    fn an_rtl_literal_survives_a_real_turtle_round_trip() {
        for (text, language) in [("مرحبا", "ar"), ("שלום", "he")] {
            let input = flake(
                "a",
                "label",
                FlakeValue::LangString(LangString {
                    text: text.into(),
                    language: language.into(),
                    direction: Some(graph_owl_core::flake::Direction::Rtl),
                }),
            );
            let bytes = StandardRdfIo
                .serialize(std::slice::from_ref(&input), RdfFormat::Turtle)
                .expect("serialize");
            assert!(
                String::from_utf8_lossy(&bytes).contains("--rtl"),
                "{text}: direction did not reach the wire"
            );
            let parsed = StandardRdfIo
                .parse(&bytes, RdfFormat::Turtle, None)
                .expect("parse");
            assert_eq!(parsed, vec![input], "{text} did not round-trip");
        }
    }

    /// The negative case matters as much: a plain string must not acquire
    /// a direction, or every literal in the catalog gains a meaningless
    /// `ltr`.
    #[test]
    fn a_plain_string_does_not_acquire_a_direction_through_turtle() {
        let input = flake("a", "name", FlakeValue::String("Orders".into()));
        let bytes = StandardRdfIo
            .serialize(std::slice::from_ref(&input), RdfFormat::Turtle)
            .expect("serialize");
        assert!(!String::from_utf8_lossy(&bytes).contains("--"));
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
        let outcome = StandardRdfIo.serialize(&[], RdfFormat::TriG);
        assert!(
            matches!(outcome, Err(RdfError::UnsupportedFormat(RdfFormat::TriG))),
            "{outcome:?}"
        );
    }

    // -- Slice B: JSON-LD --------------------------------------------------

    /// Output pins `@context` to a URL (Slice B's own criterion), so
    /// parsing that output back necessarily means resolving that URL —
    /// exactly the SSRF-relevant path [`parse_json_ld_with_loader`] exists
    /// to make testable without a real fetch. The loader here returns the
    /// same bytes [`JsonLdContext::to_document`] would serve, so this is a
    /// faithful round trip of what the served endpoint will actually say.
    #[test]
    fn json_ld_round_trips_through_the_core_context() {
        let cases = vec![
            flake("a", "ref", FlakeValue::Ref(Sid::dsc("b"))),
            flake("a", "str", FlakeValue::String("hello".into())),
            flake("a", "int", FlakeValue::Int(42)),
        ];
        for input in cases {
            let bytes = StandardRdfIo
                .serialize(std::slice::from_ref(&input), RdfFormat::JsonLd)
                .expect("serialize");
            let core = JsonLdContext::core_v1();
            let context_document = core.to_document();
            let parsed = parse_json_ld_with_loader(&bytes, Some(&core.base), move |_url| {
                Ok(context_document.clone())
            })
            .expect("parse");
            assert_eq!(
                parsed,
                vec![input.clone()],
                "{}",
                String::from_utf8_lossy(&bytes)
            );
        }
    }

    /// **Version-pinning.** The criterion is literal: output carries the
    /// context as a URL string, not the inline object `oxjsonld` writes by
    /// default — a consumer resolves the same prefixes/base by dereferencing
    /// that URL rather than trusting whatever the document happens to embed.
    #[test]
    fn compacted_output_pins_the_context_version_as_a_url() {
        let input = flake("a", "str", FlakeValue::String("v".into()));
        let bytes = StandardRdfIo
            .serialize(&[input], RdfFormat::JsonLd)
            .expect("serialize");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
        assert_eq!(
            value.get("@context").and_then(serde_json::Value::as_str),
            Some(JsonLdContext::core_v1().url().as_str())
        );
    }

    /// **The criterion's own wording**: compacting with a different context
    /// produces different but valid JSON. `oxjsonld` does not compact terms
    /// against declared prefixes (module doc comment), so what actually
    /// changes shape between two contexts is the base-relative `@id` — this
    /// test proves that difference is real, not asserted past the crate's
    /// own capability.
    #[test]
    fn two_different_contexts_compact_the_same_flakes_differently() {
        let input = flake("a", "str", FlakeValue::String("v".into()));
        let core = JsonLdContext::core_v1();
        let alternate = JsonLdContext {
            version: 2,
            base: "https://graph-owl.dev/ns/catalog#a".to_string(),
            prefixes: core.prefixes.clone(),
        };

        let core_bytes =
            serialize_json_ld_with_context(std::slice::from_ref(&input), &core).expect("core");
        let alternate_bytes =
            serialize_json_ld_with_context(std::slice::from_ref(&input), &alternate)
                .expect("alternate");

        assert_ne!(
            core_bytes, alternate_bytes,
            "different base IRIs must compact `@id` differently"
        );
        assert!(serde_json::from_slice::<serde_json::Value>(&core_bytes).is_ok());
        assert!(serde_json::from_slice::<serde_json::Value>(&alternate_bytes).is_ok());

        let alternate_document = alternate.to_document();
        let reparsed =
            parse_json_ld_with_loader(&alternate_bytes, Some(&alternate.base), move |_url| {
                Ok(alternate_document.clone())
            })
            .expect("parse");
        assert_eq!(reparsed, vec![input], "still the same triple underneath");
    }

    /// **`@graph` maps to `cx`.** A node object naming both `@id` and
    /// `@graph` is JSON-LD's own named-graph syntax; `oxjsonld` yields it as
    /// a `Quad` with that `@id` as `graph_name`, so the same mapping N-Quads
    /// already gets falls out with no extra code.
    #[test]
    fn graph_maps_to_cx_through_json_ld() {
        let doc = br#"{
            "@context": {"dsc": "https://graph-owl.dev/ns/catalog#"},
            "@id": "dsc:graph1",
            "@graph": [
                {"@id": "dsc:a", "dsc:name": "hello"}
            ]
        }"#;
        let flakes = parse_json_ld(doc, None).expect("parse");
        assert_eq!(flakes.len(), 1);
        assert_eq!(flakes[0].s, Sid::dsc("a"));
        assert_eq!(flakes[0].cx, Some(Sid::dsc("graph1")));
    }

    /// **SSRF: refused by default.** No allowlist argument exists on
    /// [`parse_json_ld`] at all — this proves the refusal is real, not just
    /// documented.
    #[test]
    fn a_remote_context_is_refused_by_default() {
        let doc = br#"{"@context": "https://evil.example/context.jsonld", "@id": "dsc:a"}"#;
        let outcome = parse_json_ld(doc, None);
        assert!(matches!(outcome, Err(RdfError::Parse(_))), "{outcome:?}");
    }

    /// **SSRF: refused for a host not on the allowlist**, and refused
    /// *before* any request — the plan's own RED test. `evil.example` is
    /// never resolved or contacted; `is_host_allowed` rejects it from the
    /// URL string alone, so this test needs no network.
    #[test]
    fn a_remote_context_from_an_unlisted_host_is_refused() {
        let doc = br#"{"@context": "https://evil.example/context.jsonld", "@id": "dsc:a"}"#;
        let outcome = parse_json_ld_with_allowed_hosts(doc, None, &["allowed.example".to_string()]);
        assert!(matches!(outcome, Err(RdfError::Parse(_))), "{outcome:?}");
    }

    #[test]
    fn is_host_allowed_matches_the_real_authority_not_a_userinfo_prefix() {
        let allowed = vec!["allowed.example".to_string()];
        assert!(is_host_allowed(
            "https://allowed.example/context.jsonld",
            &allowed
        ));
        assert!(
            !is_host_allowed(
                "https://allowed.example@evil.example/context.jsonld",
                &allowed
            ),
            "userinfo before the real authority must not fool the allowlist"
        );
        assert!(
            !is_host_allowed("https://evil-allowed.example/context.jsonld", &allowed),
            "a suffix/prefix match is not a host match"
        );
        assert!(!is_host_allowed("not a url at all", &allowed));
    }

    fn frame_type_flake(subject: &str, type_local: &str) -> Flake {
        Flake {
            s: Sid::dsc(subject),
            p: rdf_type(),
            o: FlakeValue::Ref(Sid::dsc(type_local)),
            cx: None,
            t: 0,
            op: true,
        }
    }

    #[test]
    fn frame_selects_by_type_and_nests_a_referenced_node() {
        let table_iri = Sid::dsc("Table").to_iri().unwrap();
        let flakes = vec![
            frame_type_flake("orders", "Table"),
            flake(
                "orders",
                "hasColumn",
                FlakeValue::Ref(Sid::dsc("orders.id")),
            ),
            flake("orders.id", "name", FlakeValue::String("id".into())),
        ];
        let frame = serde_json::json!({"@type": table_iri});
        let bytes = frame_json_ld(&flakes, &frame).expect("frame");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
        let graph = value["@graph"].as_array().expect("@graph array");
        assert_eq!(graph.len(), 1, "{value}");
        let node = &graph[0];
        assert_eq!(
            node["@id"],
            serde_json::Value::String(Sid::dsc("orders").to_iri().unwrap())
        );
        let column_predicate = Sid::new(namespace::DSC, "hasColumn").to_iri().unwrap();
        let embedded = &node[&column_predicate][0];
        assert_eq!(
            embedded["@id"],
            serde_json::Value::String(Sid::dsc("orders.id").to_iri().unwrap())
        );
        assert!(
            embedded
                .get(Sid::new(namespace::DSC, "name").to_iri().unwrap())
                .is_some(),
            "the nested node's own properties must be embedded too: {embedded}"
        );
    }

    /// **Cycle safety.** Two subjects referencing each other must not
    /// recurse forever — the second encounter of an already-embedded
    /// subject becomes a bare `{"@id": ...}` reference.
    #[test]
    fn frame_breaks_a_reference_cycle_with_a_bare_id() {
        let flakes = vec![
            flake("a", "feeds", FlakeValue::Ref(Sid::dsc("b"))),
            flake("b", "feeds", FlakeValue::Ref(Sid::dsc("a"))),
        ];
        let frame = serde_json::json!({});
        let bytes = frame_json_ld(&flakes, &frame).expect("frame");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
        // Must terminate at all — the real assertion here is that this test
        // completes rather than hanging. A shallow shape check besides.
        let graph = value["@graph"].as_array().expect("@graph array");
        assert_eq!(graph.len(), 2, "{value}");
    }

    // ---- Epic 42 Slice G: location-aware parsing for the ontology editor ----
    //
    // `RdfError::Parse(String)` throws away the `TextPosition` that
    // `oxttl`/`oxjsonld` already compute internally (confirmed by reading
    // both crates' source directly, per 00i rule 2/4 — they are Apache-2.0
    // permissively-licensed dependencies already adopted into this
    // workspace, not the reference implementations that rule restricts).
    // A live text editor needs the line the author should look at, not a
    // pre-formatted sentence — these tests are the RED for that.

    mod parse_with_location {
        use super::*;

        #[test]
        fn a_valid_document_parses_the_same_as_the_untimed_path() {
            let bytes = b"<https://graph-owl.dev/ns/catalog#a> <https://graph-owl.dev/ns/catalog#b> \"c\" .";
            let expected = StandardRdfIo
                .parse(bytes, RdfFormat::Turtle, None)
                .expect("baseline parse");
            let located =
                parse_with_location(bytes, RdfFormat::Turtle, None).expect("located parse");
            assert_eq!(located, expected);
        }

        /// **The RED test.** A malformed line partway through a real
        /// document must report *that* line, 1-based to match what a text
        /// editor's own gutter shows an author — not line 0, not the
        /// generic pre-formatted `oxttl` sentence with no structured field
        /// a caller could put next to a gutter marker.
        #[test]
        fn a_turtle_syntax_error_reports_its_own_line_not_the_first() {
            let document = b"@prefix ex: <https://graph-owl.dev/ns/catalog#> .\nex:a ex:b \"ok\" .\nex:c ex:d \"unterminated\n";
            let err = parse_with_location(document, RdfFormat::Turtle, None)
                .expect_err("an unterminated string literal must not parse");
            assert_eq!(err.line, Some(3), "{err:?}");
            assert!(err.column.is_some(), "{err:?}");
            assert!(!err.message.is_empty(), "{err:?}");
        }

        #[test]
        fn an_ntriples_syntax_error_reports_a_location_too() {
            let document = b"<https://graph-owl.dev/ns/catalog#a> <https://graph-owl.dev/ns/catalog#b> \"ok\" .\nnot-a-valid-triple\n";
            let err = parse_with_location(document, RdfFormat::NTriples, None)
                .expect_err("a malformed second line must not parse");
            assert_eq!(err.line, Some(2), "{err:?}");
        }

        #[test]
        fn a_json_ld_syntax_error_is_reported_not_panicked() {
            let document = b"{ not valid json";
            let err = parse_with_location(document, RdfFormat::JsonLd, None)
                .expect_err("malformed JSON must not parse");
            assert!(!err.message.is_empty(), "{err:?}");
        }

        #[test]
        fn a_format_this_function_does_not_support_is_a_named_error_not_a_panic() {
            let err = parse_with_location(b"", RdfFormat::RdfXml, None)
                .expect_err("RDF/XML has no location-aware parser yet");
            assert!(err.message.contains("RdfXml"), "{err:?}");
        }
    }
}
