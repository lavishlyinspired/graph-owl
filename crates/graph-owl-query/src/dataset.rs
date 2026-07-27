//! A [`spareval::QueryableDataset`] over flakes.
//!
//! This is Epic 7's own content, and it is small because everything above it
//! is adopted. What it must get right is everything below the storage line:
//! `as_of`, the access predicate, and the bound on how much may be scanned.

use graph_owl_core::flake::Flake;
use oxrdf::Term;
use spareval::{InternalQuad, QueryableDataset};

use crate::term::{TermError, to_named_node, to_term};

/// The facts a query may see, already resolved.
///
/// **The evaluator never scans storage.** It scans this, and this was built by
/// a caller that applied the access predicate and the transaction time first.
/// That ordering is the whole design: it means adopting an external evaluator
/// costs nothing in authorization or time travel, because neither is the
/// evaluator's business by the time it runs.
///
/// Materialised rather than streamed, for two reasons. `QueryableDataset` is a
/// synchronous trait and flake storage is async, so something must bridge them.
/// And every operation here is budget-bounded anyway (`00a`) — a query allowed
/// to stream unboundedly is a query with no budget, which this project does not
/// have.
pub struct FlakeDataset {
    quads: Vec<InternalQuad<Term>>,
}

impl FlakeDataset {
    /// Build from flakes that have **already** been filtered.
    ///
    /// # Errors
    ///
    /// [`TermError`] if a flake cannot be expressed as RDF — a namespace with
    /// no assigned IRI. Refused rather than skipped: a query answered over a
    /// silently reduced fact set returns a wrong answer that looks right.
    pub fn from_flakes(flakes: &[Flake]) -> Result<Self, TermError> {
        let mut quads = Vec::with_capacity(flakes.len());
        for flake in flakes {
            quads.push(InternalQuad {
                subject: Term::NamedNode(to_named_node(&flake.s)?),
                predicate: Term::NamedNode(to_named_node(&flake.p)?),
                object: to_term(&flake.o)?,
                graph_name: match &flake.cx {
                    None => None,
                    Some(cx) => Some(Term::NamedNode(to_named_node(cx)?)),
                },
            });
        }
        Ok(Self { quads })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.quads.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error(transparent)]
    Term(#[from] TermError),
}

impl<'a> QueryableDataset<'a> for &'a FlakeDataset {
    /// **`Term` itself, not an interned index.**
    ///
    /// The first version interned terms to a `usize` and mapped anything absent
    /// from the dataset to a `usize::MAX` sentinel. That is wrong, and wrong in
    /// a way that produced no error: the evaluator internalises terms that are
    /// *not* in the data — a literal `true` while evaluating a left join, for
    /// instance — and every one of them collapsed onto the same sentinel, which
    /// then externalised to an empty literal. `OPTIONAL` silently bound nothing.
    ///
    /// An internal term has to round-trip for **any** term, including ones the
    /// dataset has never seen. Interning was an optimisation chosen before
    /// anything was measured, and it cost correctness; `Term` is what the
    /// contract actually requires.
    type InternalTerm = Term;
    type Error = DatasetError;

    fn internal_quads_for_pattern(
        &self,
        subject: Option<&Term>,
        predicate: Option<&Term>,
        object: Option<&Term>,
        graph_name: Option<Option<&Term>>,
    ) -> impl Iterator<Item = Result<InternalQuad<Term>, DatasetError>> + use<'a> {
        let (s, p, o) = (subject.cloned(), predicate.cloned(), object.cloned());
        // `Some(None)` is the default graph, `Some(Some(_))` a named one, and
        // `None` means any *named* graph but **not** the default. Getting that
        // last distinction wrong would make an unbound graph variable silently
        // include the default graph, which is the opposite of the spec.
        let g = graph_name.map(Option::<&Term>::cloned);

        self.quads
            .iter()
            .filter(move |quad| {
                s.as_ref().is_none_or(|s| &quad.subject == s)
                    && p.as_ref().is_none_or(|p| &quad.predicate == p)
                    && o.as_ref().is_none_or(|o| &quad.object == o)
                    && match &g {
                        None => quad.graph_name.is_some(),
                        Some(None) => quad.graph_name.is_none(),
                        Some(Some(name)) => quad.graph_name.as_ref() == Some(name),
                    }
            })
            .map(|quad| {
                Ok(InternalQuad {
                    subject: quad.subject.clone(),
                    predicate: quad.predicate.clone(),
                    object: quad.object.clone(),
                    graph_name: quad.graph_name.clone(),
                })
            })
    }

    fn internalize_term(&self, term: Term) -> Result<Term, DatasetError> {
        Ok(term)
    }

    fn externalize_term(&self, term: Term) -> Result<Term, DatasetError> {
        Ok(term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::{FlakeValue, Sid, namespace};

    fn flake(s: &str, p: &str, o: FlakeValue) -> Flake {
        Flake::assert(Sid::dsc(s), Sid::dsc(p), o, 1)
    }

    fn dataset(flakes: &[Flake]) -> FlakeDataset {
        FlakeDataset::from_flakes(flakes).expect("builds")
    }

    #[test]
    fn every_flake_becomes_a_quad() {
        let d = dataset(&[
            flake("t1", "name", FlakeValue::String("upi".into())),
            flake("t1", "deleted", FlakeValue::Boolean(false)),
        ]);
        assert_eq!(d.len(), 2);
    }

    /// **Regression test for the bug interning caused.**
    ///
    /// The evaluator internalises terms that are *not* in the dataset — a
    /// literal `true` while evaluating a left join, for one. The first version
    /// mapped every such term onto a single `usize::MAX` sentinel, so they all
    /// collided and externalised to an empty literal. `OPTIONAL` bound nothing
    /// and reported no error.
    ///
    /// An internal term must round-trip for *any* term, present or absent.
    #[test]
    fn a_term_absent_from_the_dataset_still_round_trips() {
        let d = dataset(&[flake("t1", "name", FlakeValue::String("a".into()))]);

        for absent in [
            Term::Literal(oxrdf::Literal::new_typed_literal(
                "true",
                oxrdf::vocab::xsd::BOOLEAN,
            )),
            Term::Literal(oxrdf::Literal::new_simple_literal("never stored")),
            Term::NamedNode(to_named_node(&Sid::dsc("unknown")).expect("node")),
        ] {
            let internal = (&d)
                .internalize_term(absent.clone())
                .expect("internalizing any term must succeed");
            assert_eq!(
                (&d).externalize_term(internal).expect("externalize"),
                absent,
                "an absent term must survive the round trip unchanged"
            );
        }
    }

    /// And two different absent terms must not collide — the specific failure
    /// the sentinel produced.
    #[test]
    fn two_absent_terms_do_not_collide() {
        let d = dataset(&[flake("t1", "name", FlakeValue::String("a".into()))]);
        let a = (&d)
            .internalize_term(Term::Literal(oxrdf::Literal::new_simple_literal("a")))
            .expect("internalize");
        let b = (&d)
            .internalize_term(Term::Literal(oxrdf::Literal::new_simple_literal("b")))
            .expect("internalize");
        assert_ne!(a, b);
    }

    #[test]
    fn a_bound_subject_narrows_the_scan() {
        let d = dataset(&[
            flake("t1", "name", FlakeValue::String("a".into())),
            flake("t2", "name", FlakeValue::String("b".into())),
        ]);
        let subject = (&d)
            .internalize_term(Term::NamedNode(
                to_named_node(&Sid::dsc("t1")).expect("node"),
            ))
            .expect("internalize");

        // `Some(None)` — the default graph. Passing `None` here means "any
        // *named* graph", which would match nothing, and writing this test
        // wrongly the first time is how I learned the distinction is worth its
        // own test below.
        let found: Vec<_> = (&d)
            .internal_quads_for_pattern(Some(&subject), None, None, Some(None))
            .collect::<Result<_, _>>()
            .expect("scan");
        assert_eq!(found.len(), 1);
    }

    /// **The graph-name distinction the spec is easy to get backwards.**
    /// `None` means any *named* graph and specifically **not** the default —
    /// getting it wrong makes an unbound graph variable silently include
    /// default-graph facts.
    #[test]
    fn an_unbound_graph_name_excludes_the_default_graph() {
        let named = Flake {
            cx: Some(Sid::dsc("graph:extraction")),
            ..flake("t1", "description", FlakeValue::String("extracted".into()))
        };
        let d = dataset(&[flake("t1", "name", FlakeValue::String("a".into())), named]);

        let any_named: Vec<_> = (&d)
            .internal_quads_for_pattern(None, None, None, None)
            .collect::<Result<_, _>>()
            .expect("scan");
        assert_eq!(any_named.len(), 1, "only the named-graph quad");

        let default_only: Vec<_> = (&d)
            .internal_quads_for_pattern(None, None, None, Some(None))
            .collect::<Result<_, _>>()
            .expect("scan");
        assert_eq!(default_only.len(), 1, "only the default-graph quad");
        assert!(default_only[0].graph_name.is_none());
    }

    /// A term the dataset has never seen must match nothing, not fail. Erroring
    /// would turn "no rows" into "query failed", and "nothing matched" is a
    /// perfectly good answer.
    #[test]
    fn an_unknown_term_matches_nothing_without_erroring() {
        let d = dataset(&[flake("t1", "name", FlakeValue::String("a".into()))]);
        let absent = (&d)
            .internalize_term(Term::NamedNode(
                to_named_node(&Sid::dsc("never-seen")).expect("node"),
            ))
            .expect("internalizing an absent term is not an error");

        let found: Vec<_> = (&d)
            .internal_quads_for_pattern(Some(&absent), None, None, Some(None))
            .collect::<Result<_, _>>()
            .expect("scan");
        assert!(found.is_empty());
    }

    /// A flake that cannot be expressed as RDF must fail the build, not be
    /// skipped. A query answered over a silently reduced fact set returns a
    /// wrong answer that looks right.
    #[test]
    fn a_flake_with_an_unmappable_namespace_fails_the_build() {
        let bad = Flake::assert(
            Sid::new(namespace::UNSET, "x"),
            Sid::dsc("name"),
            FlakeValue::String("a".into()),
            1,
        );
        assert!(FlakeDataset::from_flakes(&[bad]).is_err());
    }

    /// Two *different* named graphs must not be conflated. With one named
    /// graph a comparison bug is invisible — both the right and the wrong
    /// predicate return the same single row.
    #[test]
    fn two_named_graphs_stay_distinct() {
        let extraction = Flake {
            cx: Some(Sid::dsc("graph:extraction")),
            ..flake("t1", "description", FlakeValue::String("extracted".into()))
        };
        let reasoning = Flake {
            cx: Some(Sid::dsc("graph:reasoning")),
            ..flake("t1", "description", FlakeValue::String("derived".into()))
        };
        let d = dataset(&[extraction, reasoning]);

        let name = Term::NamedNode(to_named_node(&Sid::dsc("graph:extraction")).expect("node"));
        let from_extraction: Vec<_> = (&d)
            .internal_quads_for_pattern(None, None, None, Some(Some(&name)))
            .collect::<Result<_, _>>()
            .expect("scan");

        assert_eq!(from_extraction.len(), 1, "one graph, one quad");
        assert_eq!(
            from_extraction[0].object,
            Term::Literal(oxrdf::Literal::new_simple_literal("extracted")),
            "the reasoning graph's fact must not appear"
        );
    }

    #[test]
    fn a_dataset_holding_facts_is_not_empty() {
        let d = dataset(&[flake("t1", "name", FlakeValue::String("a".into()))]);
        assert!(!d.is_empty());
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn an_empty_fact_set_is_a_valid_dataset() {
        let d = dataset(&[]);
        assert!(d.is_empty());
        let found: Vec<_> = (&d)
            .internal_quads_for_pattern(None, None, None, Some(None))
            .collect::<Result<_, _>>()
            .expect("scan");
        assert!(found.is_empty());
    }
}
