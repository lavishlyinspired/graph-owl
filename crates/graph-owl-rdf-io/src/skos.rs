//! SKOS vocabulary parsing — Epic 33 Slice A.
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
//! reads Turtle directly with `oxttl`, keeping every concept IRI a plain
//! string throughout — never routed through `Sid`, because a pack's
//! vocabulary was never this store's own to assign codes for.

use std::collections::{BTreeMap, HashSet};

use oxrdf::{NamedOrBlankNode, Term};

const SKOS_PREF_LABEL: &str = "http://www.w3.org/2004/02/skos/core#prefLabel";
const SKOS_ALT_LABEL: &str = "http://www.w3.org/2004/02/skos/core#altLabel";
const SKOS_DEFINITION: &str = "http://www.w3.org/2004/02/skos/core#definition";
const SKOS_BROADER: &str = "http://www.w3.org/2004/02/skos/core#broader";
const SKOS_NARROWER: &str = "http://www.w3.org/2004/02/skos/core#narrower";
const SKOS_RELATED: &str = "http://www.w3.org/2004/02/skos/core#related";
const SKOS_EXACT_MATCH: &str = "http://www.w3.org/2004/02/skos/core#exactMatch";
const SKOS_CLOSE_MATCH: &str = "http://www.w3.org/2004/02/skos/core#closeMatch";

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
    let mut by_subject: BTreeMap<String, SkosConcept> = BTreeMap::new();

    for result in oxttl::TurtleParser::new().for_slice(bytes) {
        let triple = result.map_err(|e| SkosParseError::Parse(e.to_string()))?;
        // A blank-node subject has no identity that survives a second parse
        // of the same document, so it cannot be a pack concept — SKOS
        // concepts are always named.
        let NamedOrBlankNode::NamedNode(subject_node) = &triple.subject else {
            continue;
        };
        let subject = subject_node.as_str().to_string();
        let predicate = triple.predicate.as_str();

        // Only a subject carrying at least one recognised SKOS predicate
        // gets an entry — a `skos:ConceptScheme` header or an unrelated
        // resource must not surface as a concept with an empty label.
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
            SKOS_PREF_LABEL => concept.pref_label = literal_value(&triple.object),
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

    let concepts: Vec<SkosConcept> = by_subject.into_values().collect();

    for concept in &concepts {
        if concept.pref_label.is_empty() {
            return Err(SkosParseError::MissingPrefLabel(concept.iri.clone()));
        }
    }

    let known: HashSet<&str> = concepts.iter().map(|c| c.iri.as_str()).collect();
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

    Ok(concepts)
}

fn literal_value(term: &Term) -> String {
    match term {
        Term::Literal(literal) => literal.value().to_string(),
        Term::NamedNode(node) => node.as_str().to_string(),
        Term::BlankNode(_) => String::new(),
    }
}

/// The far end of a relation, or an empty string for anything that is not a
/// named node — which then correctly fails as a dangling reference, since no
/// concept's IRI is ever empty.
fn node_iri(term: &Term) -> String {
    match term {
        Term::NamedNode(node) => node.as_str().to_string(),
        Term::Literal(_) | Term::BlankNode(_) => String::new(),
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
}
