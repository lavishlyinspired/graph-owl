//! SKOS vocabulary parsing — Epic 33 Slice A, extended Phase 3 item 3.9 /
//! decision 4.6 for OWL-native packs (FIBO).
//!
//! A pack's own concept IRIs live in whatever namespace its publisher chose
//! (FIBO, GS1, ICD-10 all differ), which `graph_owl_core::flake`'s fixed
//! namespace registry has no room for and was never meant to hold —
//! [`crate::RdfError::UnrecognisedIri`]'s own doc comment already names
//! "a genuinely external vocabulary" as unfinished business belonging
//! elsewhere. Glossary terms are not flakes, though: `GlossaryTermRecord`
//! is a plain relational row keyed by `Uuid`, and a term's SKOS relations
//! already carry an external match as an opaque `String` IRI
//! (`graph_owl_core::glossary::SkosRelation::ExactMatch`). So this module
//! reads RDF directly with `oxttl`/`oxrdfxml`, keeping every concept IRI a
//! plain string throughout — never routed through `Sid`, because a pack's
//! vocabulary was never this store's own to assign codes for.
//!
//! **`rdfs:label`/`rdfs:subClassOf` are accepted as aliases for
//! `skos:prefLabel`/`skos:broader`** — decision 4.6, verified against FIBO's
//! real production distribution (`edmcouncil/fibo`, MIT, 8 August 2026):
//! its `owl:Class` definitions use `rdfs:label` and `rdfs:subClassOf`
//! throughout and never `skos:prefLabel` at all, so the importer as
//! originally shipped would recognise zero FIBO concepts.
//!
//! **`rdfs:subClassOf` is deliberately more lenient than `skos:broader`.**
//! A `skos:broader` target missing from the same document usually means a
//! truncated or malformed vocabulary — refused, per the original design
//! below. FIBO's own modularity makes the identical shape *normal*: real
//! FIBO classes routinely `rdfs:subClassOf` a class defined in a different,
//! `owl:imports`-linked module this crate has no visibility into when only
//! one module is being imported. Refusing the whole module over that would
//! make "import one FIBO module without the other 90" impossible. An
//! unresolved `rdfs:subClassOf` target is silently omitted rather than
//! refused — a real, recorded scope cut (`plans/EPIC-COMPLETION-PLAN.md`
//! Phase 3 item 3.9), not an oversight: a full corpus import that resolved
//! `owl:imports` across every module would not need this leniency at all.

use std::collections::{BTreeMap, HashSet};

use oxrdf::{NamedOrBlankNode, Term, Triple};

const SKOS_PREF_LABEL: &str = "http://www.w3.org/2004/02/skos/core#prefLabel";
const SKOS_ALT_LABEL: &str = "http://www.w3.org/2004/02/skos/core#altLabel";
const SKOS_DEFINITION: &str = "http://www.w3.org/2004/02/skos/core#definition";
const SKOS_BROADER: &str = "http://www.w3.org/2004/02/skos/core#broader";
const SKOS_NARROWER: &str = "http://www.w3.org/2004/02/skos/core#narrower";
const SKOS_RELATED: &str = "http://www.w3.org/2004/02/skos/core#related";
const SKOS_EXACT_MATCH: &str = "http://www.w3.org/2004/02/skos/core#exactMatch";
const SKOS_CLOSE_MATCH: &str = "http://www.w3.org/2004/02/skos/core#closeMatch";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

/// One `skos:Concept`, as parsed. `iri` is the concept's own identity —
/// stable across re-imports and upgrades, since this store never mints one
/// of its own for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkosConcept {
    pub iri: String,
    pub pref_label: String,
    pub definition: Option<String>,
    pub alt_labels: Vec<String>,
    pub broader: Vec<String>,
    pub narrower: Vec<String>,
    pub related: Vec<String>,
    pub exact_match: Vec<String>,
    pub close_match: Vec<String>,
}

/// Why a document was refused before any of it was written anywhere.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkosParseError {
    #[error("parse error: {0}")]
    Parse(String),
    /// A subject carries a SKOS relation but no `skos:prefLabel` — the one
    /// thing every concept must have to be addressable at all.
    #[error("`{0}` has SKOS relations but no skos:prefLabel — every concept needs a label")]
    MissingPrefLabel(String),
    /// `broader`/`narrower`/`related` point at a concept this document never
    /// defines. Refused rather than imported as a dangling edge, because a
    /// term's hierarchy is most of what a pack is for (Slice A's own
    /// acceptance criterion) and a hierarchy with a missing end is not the
    /// one the publisher shipped.
    #[error(
        "`{0}` names `{1}` as broader/narrower/related, but `{1}` is not defined in this document"
    )]
    DanglingReference(String, String),
}

/// Parses a Turtle document into its SKOS concepts.
///
/// Non-SKOS triples are ignored rather than refused — a real vocabulary
/// carries `rdf:type skos:Concept`, `dct:*` provenance, and similar
/// alongside the relations this pack import actually uses, and refusing
/// the whole document over a predicate nobody asked to import would make
/// every future SKOS addition a breaking change to this parser.
///
/// # Errors
///
/// [`SkosParseError::Parse`] if the bytes are not valid Turtle;
/// [`SkosParseError::MissingPrefLabel`] or [`SkosParseError::DanglingReference`]
/// if the document is structurally malformed — both checked **before**
/// returning any concept, so a caller never has to unwind a partial import.
pub fn parse_skos_turtle(bytes: &[u8]) -> Result<Vec<SkosConcept>, SkosParseError> {
    concepts_from_triples(
        oxttl::TurtleParser::new()
            .for_slice(bytes)
            .map(|result| result.map_err(|e| SkosParseError::Parse(e.to_string()))),
    )
}

/// Parses an RDF/XML document into its SKOS concepts — FIBO's own
/// production distribution shape (decision 4.6). Same acceptance rules as
/// [`parse_skos_turtle`], including its leniency toward an unresolved
/// `rdfs:subClassOf` target (see the module's own doc comment).
///
/// # Errors
/// Identical to [`parse_skos_turtle`], reading [`SkosParseError::Parse`] as
/// "not well-formed RDF/XML" instead of "not well-formed Turtle".
pub fn parse_skos_rdfxml(bytes: &[u8]) -> Result<Vec<SkosConcept>, SkosParseError> {
    concepts_from_triples(
        oxrdfxml::RdfXmlParser::new()
            .for_slice(bytes)
            .map(|result| result.map_err(|e| SkosParseError::Parse(e.to_string()))),
    )
}

/// Converts an RDF/XML document to Turtle, triple for triple — plain syntax
/// translation, with no SKOS-specific logic at all.
///
/// **Why this exists rather than teaching pack storage a second source
/// format.** `Catalog::import_pack` (Epic 33) accepts only `skos_turtle:
/// &[u8]`, storing it verbatim as `source_turtle` — the pack's own declared
/// state, re-parsed on every future upgrade diff. Giving `import_pack` a
/// second, RDF/XML-shaped code path would mean either widening that stored
/// field's meaning or adding a parallel one, both a bigger, separate change
/// than "let FIBO's real distribution format reach the existing pipeline
/// unchanged." A publisher's RDF/XML release converts once, here, and
/// everything downstream — storage, upgrade-diffing, override resolution —
/// stays exactly as shipped.
///
/// # Errors
/// [`SkosParseError::Parse`] if the bytes are not well-formed RDF/XML, or if
/// serializing the Turtle output fails — kept as the same variant rather
/// than a new one, since a caller cannot act on the two differently.
pub fn rdfxml_to_turtle(bytes: &[u8]) -> Result<Vec<u8>, SkosParseError> {
    let mut writer = oxttl::TurtleSerializer::new().for_writer(Vec::new());
    for result in oxrdfxml::RdfXmlParser::new().for_slice(bytes) {
        let triple = result.map_err(|e| SkosParseError::Parse(e.to_string()))?;
        writer
            .serialize_triple(&triple)
            .map_err(|e| SkosParseError::Parse(e.to_string()))?;
    }
    writer
        .finish()
        .map_err(|e| SkosParseError::Parse(e.to_string()))
}

/// The syntax-independent core both [`parse_skos_turtle`] and
/// [`parse_skos_rdfxml`] share — every RDF serialization parses into the
/// same triple model, so the concept-building and validation logic has no
/// reason to know which syntax produced it.
fn concepts_from_triples(
    triples: impl Iterator<Item = Result<Triple, SkosParseError>>,
) -> Result<Vec<SkosConcept>, SkosParseError> {
    let mut by_subject: BTreeMap<String, SkosConcept> = BTreeMap::new();
    // `rdfs:subClassOf` candidates, held apart from `broader` until the
    // in-document concept set is known — see the module's own doc comment
    // on why this predicate is resolved leniently rather than strictly.
    let mut owl_broader_candidates: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for result in triples {
        let triple = result?;
        // A blank-node subject has no identity that survives a second parse
        // of the same document, so it cannot be a pack concept — SKOS
        // concepts are always named.
        let NamedOrBlankNode::NamedNode(subject_node) = &triple.subject else {
            continue;
        };
        let subject = subject_node.as_str().to_string();
        let predicate = triple.predicate.as_str();

        if predicate == RDFS_SUBCLASS_OF {
            // A blank-node object is an OWL restriction (an anonymous
            // class expression), never a named broader concept — pushing
            // its empty `node_iri` would make every restricted class fail
            // as a dangling reference to `""`.
            if let Term::NamedNode(node) = &triple.object {
                owl_broader_candidates
                    .entry(subject)
                    .or_default()
                    .push(node.as_str().to_string());
            }
            continue;
        }

        // Only a subject carrying at least one recognised predicate gets an
        // entry — a `skos:ConceptScheme` header or an unrelated resource
        // must not surface as a concept with an empty label.
        if !matches!(
            predicate,
            SKOS_PREF_LABEL
                | SKOS_ALT_LABEL
                | SKOS_DEFINITION
                | SKOS_BROADER
                | SKOS_NARROWER
                | SKOS_RELATED
                | SKOS_EXACT_MATCH
                | SKOS_CLOSE_MATCH
                | RDFS_LABEL
        ) {
            continue;
        }
        let concept = by_subject
            .entry(subject.clone())
            .or_insert_with(|| SkosConcept {
                iri: subject.clone(),
                ..Default::default()
            });
        match predicate {
            SKOS_PREF_LABEL | RDFS_LABEL => concept.pref_label = literal_value(&triple.object),
            SKOS_ALT_LABEL => concept.alt_labels.push(literal_value(&triple.object)),
            SKOS_DEFINITION => concept.definition = Some(literal_value(&triple.object)),
            SKOS_BROADER => concept.broader.push(node_iri(&triple.object)),
            SKOS_NARROWER => concept.narrower.push(node_iri(&triple.object)),
            SKOS_RELATED => concept.related.push(node_iri(&triple.object)),
            SKOS_EXACT_MATCH => concept.exact_match.push(node_iri(&triple.object)),
            SKOS_CLOSE_MATCH => concept.close_match.push(node_iri(&triple.object)),
            _ => unreachable!("filtered above"),
        }
    }

    let mut concepts: Vec<SkosConcept> = by_subject.into_values().collect();

    for concept in &concepts {
        if concept.pref_label.is_empty() {
            return Err(SkosParseError::MissingPrefLabel(concept.iri.clone()));
        }
    }

    let known: HashSet<String> = concepts.iter().map(|c| c.iri.clone()).collect();
    for concept in &concepts {
        for target in concept
            .broader
            .iter()
            .chain(&concept.narrower)
            .chain(&concept.related)
        {
            if !known.contains(target.as_str()) {
                return Err(SkosParseError::DanglingReference(
                    concept.iri.clone(),
                    target.clone(),
                ));
            }
        }
    }

    // `rdfs:subClassOf` candidates merge in last, after the strict SKOS
    // check above — a target outside this document is silently omitted
    // rather than refused, the leniency the module doc comment explains.
    for concept in &mut concepts {
        if let Some(candidates) = owl_broader_candidates.get(&concept.iri) {
            concept.broader.extend(
                candidates
                    .iter()
                    .filter(|t| known.contains(t.as_str()))
                    .cloned(),
            );
        }
    }

    Ok(concepts)
}

fn literal_value(term: &Term) -> String {
    match term {
        Term::Literal(literal) => literal.value().to_string(),
        Term::NamedNode(node) => node.as_str().to_string(),
        // A triple term is never produced here: this crate's Turtle parser
        // does not read the `<< s p o >>` literal (RDF 1.2 Turtle syntax is
        // Working Draft — explicitly deferred, `94-rdf12-alignment.md`), so
        // nothing a SKOS document parses can reach this arm. Treated the
        // same as `BlankNode` — not a literal — should that ever change.
        Term::BlankNode(_) | Term::Triple(_) => String::new(),
    }
}

/// The far end of a relation, or an empty string for anything that is not a
/// named node — which then correctly fails as a dangling reference, since no
/// concept's IRI is ever empty.
fn node_iri(term: &Term) -> String {
    match term {
        Term::NamedNode(node) => node.as_str().to_string(),
        // Same "cannot reach this parsing SKOS Turtle today" reasoning as
        // `literal_value` above — a triple term is not a named node either
        // way, so it belongs beside `Literal`/`BlankNode` regardless.
        Term::Literal(_) | Term::BlankNode(_) | Term::Triple(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(body: &str) -> Vec<u8> {
        format!("@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n{body}").into_bytes()
    }

    #[test]
    fn a_single_concept_with_a_label_parses() {
        let concepts = parse_skos_turtle(&doc(r#"<http://ex.org/v#Loan> skos:prefLabel "Loan" ."#))
            .expect("parse");

        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].iri, "http://ex.org/v#Loan");
        assert_eq!(concepts[0].pref_label, "Loan");
    }

    #[test]
    fn definition_and_alt_labels_are_captured() {
        let concepts = parse_skos_turtle(&doc(r#"
            <http://ex.org/v#Loan> skos:prefLabel "Loan" ;
                skos:definition "A sum of money lent" ;
                skos:altLabel "Credit" ;
                skos:altLabel "Advance" .
            "#))
        .expect("parse");

        assert_eq!(
            concepts[0].definition.as_deref(),
            Some("A sum of money lent")
        );
        assert_eq!(concepts[0].alt_labels, vec!["Credit", "Advance"]);
    }

    // **Hierarchy fidelity is the whole value of a pack import** — a flattened
    // import loses the vocabulary's structure. Depth 3, because a mapping that
    // checks only the immediate parent passes depth 1 and fails here (the same
    // trap `graph_owl_core::glossary`'s own cycle test names).
    #[test]
    fn broader_narrower_related_are_preserved_at_depth_three() {
        let concepts = parse_skos_turtle(&doc(r#"
            <http://ex.org/v#Asset> skos:prefLabel "Asset" ;
                skos:narrower <http://ex.org/v#FinancialInstrument> .
            <http://ex.org/v#FinancialInstrument> skos:prefLabel "Financial Instrument" ;
                skos:broader <http://ex.org/v#Asset> .
            <http://ex.org/v#DebtInstrument> skos:prefLabel "Debt Instrument" ;
                skos:broader <http://ex.org/v#FinancialInstrument> .
            <http://ex.org/v#Loan> skos:prefLabel "Loan" ;
                skos:broader <http://ex.org/v#DebtInstrument> ;
                skos:related <http://ex.org/v#Asset> .
            "#))
        .expect("parse");

        let by_iri: BTreeMap<&str, &SkosConcept> =
            concepts.iter().map(|c| (c.iri.as_str(), c)).collect();
        assert_eq!(
            by_iri["http://ex.org/v#Loan"].broader,
            vec!["http://ex.org/v#DebtInstrument"]
        );
        assert_eq!(
            by_iri["http://ex.org/v#DebtInstrument"].broader,
            vec!["http://ex.org/v#FinancialInstrument"]
        );
        assert_eq!(
            by_iri["http://ex.org/v#FinancialInstrument"].broader,
            vec!["http://ex.org/v#Asset"]
        );
        assert_eq!(
            by_iri["http://ex.org/v#Loan"].related,
            vec!["http://ex.org/v#Asset"]
        );
        assert_eq!(
            by_iri["http://ex.org/v#Asset"].narrower,
            vec!["http://ex.org/v#FinancialInstrument"]
        );
    }

    #[test]
    fn exact_and_close_matches_are_kept_as_external_iris() {
        let concepts = parse_skos_turtle(&doc(r#"
            <http://ex.org/v#Loan> skos:prefLabel "Loan" ;
                skos:exactMatch <http://other.org/fibo#Loan> ;
                skos:closeMatch <http://other.org/legacy#Credit> .
            "#))
        .expect("parse");

        assert_eq!(concepts[0].exact_match, vec!["http://other.org/fibo#Loan"]);
        assert_eq!(
            concepts[0].close_match,
            vec!["http://other.org/legacy#Credit"]
        );
    }

    // ---- malformed packs fail before anything lands ----

    #[test]
    fn a_concept_with_no_pref_label_is_refused() {
        let error = parse_skos_turtle(&doc(
            r#"<http://ex.org/v#Loan> skos:definition "no label at all" ."#,
        ))
        .expect_err("must be refused");

        assert_eq!(
            error,
            SkosParseError::MissingPrefLabel("http://ex.org/v#Loan".to_string())
        );
    }

    #[test]
    fn a_broader_reference_to_an_undefined_concept_is_refused() {
        let error = parse_skos_turtle(&doc(r#"
            <http://ex.org/v#Loan> skos:prefLabel "Loan" ;
                skos:broader <http://ex.org/v#Nowhere> .
            "#))
        .expect_err("must be refused");

        assert_eq!(
            error,
            SkosParseError::DanglingReference(
                "http://ex.org/v#Loan".to_string(),
                "http://ex.org/v#Nowhere".to_string()
            )
        );
    }

    #[test]
    fn invalid_turtle_syntax_is_a_parse_error() {
        let error = parse_skos_turtle(b"this is not turtle at all {{{").expect_err("must fail");
        assert!(matches!(error, SkosParseError::Parse(_)));
    }

    // The negative that makes the two malformed-pack tests above mean
    // something: a well-formed document with no relations at all must not be
    // refused for lacking them.
    #[test]
    fn a_flat_vocabulary_with_no_relations_is_not_malformed() {
        let concepts = parse_skos_turtle(&doc(r#"<http://ex.org/v#Loan> skos:prefLabel "Loan" ."#))
            .expect("a flat concept is valid");
        assert_eq!(concepts.len(), 1);
    }

    // A `skos:ConceptScheme` header (or any resource that never carries a
    // recognised SKOS predicate) must not surface as a phantom concept with
    // an empty label — only subjects this parser actually recognises appear
    // in its output at all.
    #[test]
    fn a_subject_with_no_recognised_skos_predicate_is_not_a_concept() {
        let concepts = parse_skos_turtle(&doc(r#"
            <http://ex.org/v#Scheme> a skos:ConceptScheme .
            <http://ex.org/v#Loan> skos:prefLabel "Loan" .
            "#))
        .expect("parse");

        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].iri, "http://ex.org/v#Loan");
    }

    // ---- Phase 3 item 3.9 / decision 4.6: OWL-native aliases ----

    fn owl_doc(body: &str) -> Vec<u8> {
        format!(
            "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
             @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
             @prefix owl: <http://www.w3.org/2002/07/owl#> .\n{body}"
        )
        .into_bytes()
    }

    /// **The RED case decision 4.6 exists for.** FIBO's real `owl:Class`
    /// definitions use `rdfs:label`, never `skos:prefLabel` — verified
    /// directly against `edmcouncil/fibo`'s `FND/AgentsAndPeople/People.rdf`,
    /// 8 August 2026. Without this alias the importer as originally shipped
    /// recognises zero FIBO concepts.
    #[test]
    fn rdfs_label_is_accepted_as_an_alias_for_skos_pref_label() {
        let concepts = parse_skos_turtle(&owl_doc(
            r#"<http://ex.org/v#Adult> a owl:Class ; rdfs:label "adult" ."#,
        ))
        .expect("parse");

        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].pref_label, "adult");
    }

    #[test]
    fn rdfs_subclass_of_a_locally_defined_class_becomes_broader() {
        let concepts = parse_skos_turtle(&owl_doc(
            r#"
            <http://ex.org/v#Person> rdfs:label "person" .
            <http://ex.org/v#Adult> rdfs:label "adult" ;
                rdfs:subClassOf <http://ex.org/v#Person> .
            "#,
        ))
        .expect("parse");

        let adult = concepts
            .iter()
            .find(|c| c.iri == "http://ex.org/v#Adult")
            .expect("present");
        assert_eq!(adult.broader, vec!["http://ex.org/v#Person"]);
    }

    /// **The leniency the module doc comment names.** A real FIBO module
    /// routinely `rdfs:subClassOf` a class defined in a different,
    /// `owl:imports`-linked module — importing one module at a time must not
    /// refuse over that, unlike a genuine `skos:broader` dangling reference
    /// (`a_broader_reference_to_an_undefined_concept_is_refused`, above,
    /// still refuses).
    #[test]
    fn rdfs_subclass_of_an_undefined_class_is_omitted_not_refused() {
        let concepts = parse_skos_turtle(&owl_doc(
            r#"
            <http://ex.org/v#Adult> rdfs:label "adult" ;
                rdfs:subClassOf <http://ex.org/v#SomeOtherFiboModule.Person> .
            "#,
        ))
        .expect("must not be refused");

        let adult = &concepts[0];
        assert!(
            adult.broader.is_empty(),
            "an out-of-document target must be silently omitted: {:?}",
            adult.broader
        );
    }

    /// **An OWL restriction is not a broader concept.** FIBO routinely
    /// expresses `rdfs:subClassOf` against a blank-node `owl:Restriction`
    /// (a property cardinality constraint, not a named superclass) — this
    /// must not become a broader entry pointing at nothing, which the old
    /// unconditional `node_iri` mapping would have done.
    #[test]
    fn rdfs_subclass_of_a_blank_node_restriction_is_ignored() {
        let concepts = parse_skos_turtle(&owl_doc(
            r#"
            <http://ex.org/v#Adult> rdfs:label "adult" ;
                rdfs:subClassOf [ a owl:Restriction ] .
            "#,
        ))
        .expect("must not be refused");

        assert!(concepts[0].broader.is_empty(), "{:?}", concepts[0].broader);
    }

    /// The real *structural* shape FIBO's production distribution actually
    /// uses, verified against `edmcouncil/fibo` (MIT, checked 8 August
    /// 2026) — DOCTYPE entity IRIs, `owl:Class`, `rdfs:label`,
    /// `skos:definition`, and both a locally-resolvable and an
    /// externally-defined `rdfs:subClassOf` target in the same document.
    /// **Content is invented, on an `http://ex.org/` domain**: packs are
    /// never vendored into this repo, in a test fixture or otherwise
    /// (`33-ontology-packs.md` decision 1, restated explicitly for fixtures
    /// by `graph-owl-server/tests/ontology_packs.rs`'s own module doc
    /// comment).
    fn owl_native_rdfxml_fixture() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE rdf:RDF [
	<!ENTITY ex "http://ex.org/v#">
	<!ENTITY ext "http://ex.org/other-module#">
	<!ENTITY owl "http://www.w3.org/2002/07/owl#">
	<!ENTITY rdf "http://www.w3.org/1999/02/22-rdf-syntax-ns#">
	<!ENTITY rdfs "http://www.w3.org/2000/01/rdf-schema#">
	<!ENTITY skos "http://www.w3.org/2004/02/skos/core#">
]>
<rdf:RDF xml:base="http://ex.org/v#"
	xmlns:ex="http://ex.org/v#"
	xmlns:ext="http://ex.org/other-module#"
	xmlns:owl="http://www.w3.org/2002/07/owl#"
	xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
	xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
	xmlns:skos="http://www.w3.org/2004/02/skos/core#">

	<owl:Class rdf:about="&ex;FinancialInstrument">
		<rdfs:label>financial instrument</rdfs:label>
		<skos:definition>a monetary contract between parties</skos:definition>
	</owl:Class>

	<owl:Class rdf:about="&ex;Loan">
		<rdfs:subClassOf rdf:resource="&ex;FinancialInstrument"/>
		<rdfs:label>loan</rdfs:label>
		<skos:definition>a sum of money lent</skos:definition>
	</owl:Class>

	<owl:Class rdf:about="&ex;IdentificationScheme">
		<rdfs:subClassOf rdf:resource="&ext;Scheme"/>
		<rdfs:label>identification scheme</rdfs:label>
		<skos:definition>system for allocating identifiers</skos:definition>
	</owl:Class>

</rdf:RDF>
"#
    }

    /// **The real distribution shape, end to end.**
    #[test]
    fn an_owl_native_rdfxml_document_parses_into_its_concepts() {
        let concepts = parse_skos_rdfxml(owl_native_rdfxml_fixture()).expect("must parse");

        assert_eq!(concepts.len(), 3, "{concepts:#?}");
        let by_label: BTreeMap<&str, &SkosConcept> = concepts
            .iter()
            .map(|c| (c.pref_label.as_str(), c))
            .collect();

        let loan = by_label["loan"];
        assert_eq!(
            loan.broader,
            vec!["http://ex.org/v#FinancialInstrument"],
            "a same-document rdfs:subClassOf target must resolve"
        );

        let scheme = by_label["identification scheme"];
        assert!(
            scheme.broader.is_empty(),
            "a cross-module target (a different module, not this document) must be omitted, \
             not refused: {:?}",
            scheme.broader
        );
    }

    /// **The pipeline `Catalog::import_pack` actually needs.** Converting
    /// RDF/XML to Turtle and then parsing the *Turtle* must yield the
    /// identical concepts `parse_skos_rdfxml` reads directly — this is what
    /// makes it safe to feed FIBO's real distribution shape into the
    /// existing, unmodified Turtle-only pack-import pipeline rather than
    /// widening it to a second source format.
    #[test]
    fn rdfxml_to_turtle_round_trips_to_the_same_concepts() {
        let direct = parse_skos_rdfxml(owl_native_rdfxml_fixture()).expect("direct parse");

        let turtle = rdfxml_to_turtle(owl_native_rdfxml_fixture()).expect("convert");
        let via_turtle = parse_skos_turtle(&turtle).expect("parse the converted Turtle");

        let sort_key = |c: &&SkosConcept| c.iri.clone();
        let mut direct_sorted = direct.iter().collect::<Vec<_>>();
        let mut via_turtle_sorted = via_turtle.iter().collect::<Vec<_>>();
        direct_sorted.sort_by_key(sort_key);
        via_turtle_sorted.sort_by_key(sort_key);
        assert_eq!(direct_sorted, via_turtle_sorted);
    }
}
