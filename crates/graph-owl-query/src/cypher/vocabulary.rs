//! The IRIs Cypher lowering addresses — Epic 7b.
//!
//! **Named once, here, because the SPARQL and Cypher front ends must address the
//! same terms.** If lowering invented `dsc:relType` while the projection wrote
//! `dsc:relationshipType`, both front ends would work in isolation, agree in
//! every unit test, and return different answers over the same data. That is the
//! failure this module exists to make impossible: one spelling, one place.

use oxrdf::NamedNode;

use graph_owl_core::flake::Sid;

/// Build the IRI for a catalog-vocabulary term.
///
/// Goes through [`Sid`] rather than formatting a string, so the namespace comes
/// from the same place the storage layer's does.
fn dsc(local: &str) -> NamedNode {
    let iri = Sid::dsc(local.to_string())
        .to_iri()
        .expect("the catalog namespace always resolves");
    NamedNode::new(iri).expect("a namespace IRI plus a local name is a valid IRI")
}

/// `rdf:type` as this catalog spells it — a node's label.
#[must_use]
pub fn type_predicate() -> NamedNode {
    dsc(crate::cypher::predicate::TYPE)
}

/// The relationship's kind, which becomes the edge type.
#[must_use]
pub fn rel_type_predicate() -> NamedNode {
    dsc(crate::cypher::predicate::REL_TYPE)
}

/// The edge's tail.
#[must_use]
pub fn from_entity_predicate() -> NamedNode {
    dsc(crate::cypher::predicate::FROM_ENTITY)
}

/// The edge's head.
#[must_use]
pub fn to_entity_predicate() -> NamedNode {
    dsc(crate::cypher::predicate::TO_ENTITY)
}

/// A user property — `n.name` becomes `dsc:name`.
#[must_use]
pub fn property(name: &str) -> NamedNode {
    dsc(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Both front ends must address the same IRIs.** This asserts the exact
    /// strings rather than that they are non-empty, because the failure mode is
    /// two spellings that each work alone.
    #[test]
    fn the_vocabulary_resolves_to_the_catalog_namespace() {
        assert_eq!(
            type_predicate().as_str(),
            "https://graph-owl.dev/ns/catalog#type"
        );
        assert_eq!(
            rel_type_predicate().as_str(),
            "https://graph-owl.dev/ns/catalog#relType"
        );
        assert_eq!(
            from_entity_predicate().as_str(),
            "https://graph-owl.dev/ns/catalog#fromEntity"
        );
        assert_eq!(
            to_entity_predicate().as_str(),
            "https://graph-owl.dev/ns/catalog#toEntity"
        );
        assert_eq!(
            property("confidence").as_str(),
            "https://graph-owl.dev/ns/catalog#confidence"
        );
    }

    /// And the same term never resolves two ways — the whole reason this module
    /// exists rather than string literals at each call site.
    #[test]
    fn a_term_resolves_identically_however_it_is_reached() {
        assert_eq!(property("type"), type_predicate());
    }
}
