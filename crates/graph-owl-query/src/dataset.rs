//! A [`spareval::QueryableDataset`] over flakes.
//!
//! This is Epic 7's own content, and it is small because everything above it
//! is adopted. What it must get right is everything below the storage line:
//! `as_of`, the access predicate, and the bound on how much may be scanned.

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use oxrdf::{Term, Triple};
use spareval::{InternalQuad, QueryableDataset};

use crate::term::{TermError, to_named_node, to_term};

/// The three predicates that make a subject a reified relationship — the
/// identical shape `graph-owl-rdf-io`'s own export-side synthesis
/// recognizes (Epic 94 Slice B). Not shared as a library function between
/// the two crates: `graph-owl-query` cannot depend on `graph-owl-rdf-io`
/// (the dependency runs the other way), and duplicating three string
/// literals is cheaper than inventing a third crate to host them.
const REL_FROM_ENTITY: &str = "fromEntity";
const REL_TO_ENTITY: &str = "toEntity";
const REL_TYPE: &str = "relType";

/// Reads `sid`'s own `fromEntity`/`toEntity`/`relType` out of `flakes`, if
/// all three are present with the shape they always have in this store.
/// **All three are required** — Epic 94 decision 7's own authorization
/// argument depends on it: a relationship missing an endpoint is exactly
/// what an access-predicate filter leaves behind when it removes an
/// entity the caller may not see, and synthesizing from a partial shape
/// would name an entity the filter had already decided to hide.
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

/// `(rel) rdf:reifies << from relType to >>`, as a synthesized quad — Epic
/// 94 decision 7. `graph_name` matches the flake the reifier shape was
/// read from, since a synthesized quad must carry the same graph as the
/// facts it was built from to answer a graph-scoped query correctly.
fn reifying_quad(
    rel: &Sid,
    from: &Sid,
    rel_type: &str,
    to: &Sid,
    graph_name: Option<Term>,
) -> Result<InternalQuad<Term>, TermError> {
    let inner = Triple::new(
        to_named_node(from)?,
        to_named_node(&Sid::dsc(rel_type))?,
        Term::NamedNode(to_named_node(to)?),
    );
    Ok(InternalQuad {
        subject: Term::NamedNode(to_named_node(rel)?),
        predicate: Term::NamedNode(to_named_node(&Sid::new(namespace::RDF, "reifies"))?),
        object: Term::Triple(Box::new(inner)),
        graph_name,
    })
}

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

        // Epic 94 decision 7: `rdf:reifies` is answered by synthesizing a
        // quad here, never by storing one (decision 3) and never in
        // pushdown (`pushdown.rs` narrows *which flakes are fetched*; it
        // cannot conjure a quad with no flake behind it). Reads only
        // `flakes` — already filtered by the caller's access predicate and
        // `as_of` before this function ever runs — so a synthesized quad
        // cannot name an entity those flakes did not already name. One
        // quad per qualifying subject, not per flake: a relationship's
        // three defining flakes must not each synthesize their own copy.
        let mut reified: std::collections::HashSet<&Sid> = std::collections::HashSet::new();
        for flake in flakes {
            if reified.contains(&flake.s) {
                continue;
            }
            let Some((from, rel_type, to)) = reifier_endpoints(&flake.s, flakes) else {
                continue;
            };
            reified.insert(&flake.s);
            let graph_name = match &flake.cx {
                None => None,
                Some(cx) => Some(Term::NamedNode(to_named_node(cx)?)),
            };
            quads.push(reifying_quad(&flake.s, &from, &rel_type, &to, graph_name)?);
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

    /// **Diagnostic scratch test, not a permanent fixture.** A self-join
    /// BGP (`?cui p o1 . ?cui p ?x . FILTER(?x != o1)`) against a plain
    /// `oxrdf::Dataset` correctly returns one row for this exact fact
    /// shape (verified separately in a scratch reproduction outside this
    /// crate). This test checks whether `FlakeDataset` reproduces that or
    /// diverges — isolating whether a real end-to-end bug (Epic 104 Slice
    /// C) lives in `FlakeDataset`'s own `QueryableDataset` impl or
    /// upstream of it (pushdown, `scope_facts`, `execute_algebra`).
    #[test]
    fn self_join_with_inequality_filter_returns_one_row_not_two() {
        let d = dataset(&[
            flake("cui1", "exactMatch", FlakeValue::Ref(Sid::dsc("snomed1"))),
            flake("cui1", "exactMatch", FlakeValue::Ref(Sid::dsc("rxnorm1"))),
        ]);
        let query = spargebra::SparqlParser::new()
            .parse_query(&format!(
                "SELECT ?rxnorm WHERE {{ \
                    ?cui <{DSC}exactMatch> <{DSC}snomed1> . \
                    ?cui <{DSC}exactMatch> ?rxnorm . \
                    FILTER(?rxnorm != <{DSC}snomed1>) \
                 }}",
                DSC = "https://graph-owl.dev/ns/catalog#"
            ))
            .expect("parses");
        let results = spareval::QueryEvaluator::new()
            .prepare(&query)
            .execute(&d)
            .expect("evaluates");
        let rows = match results {
            spareval::QueryResults::Solutions(solutions) => {
                solutions.collect::<Result<Vec<_>, _>>().expect("solutions")
            }
            _ => panic!("expected solutions"),
        };
        assert_eq!(rows.len(), 1, "{rows:#?}");
    }

    /// **The RED test, Epic 94 decision 7 / Slice D's own stated criterion**:
    /// a query using the standard `rdf:reifies` vocabulary against an
    /// estate that plainly contains a reified relationship must not return
    /// an empty result — the failure this whole decision exists to
    /// prevent, since a caller cannot tell a synthesis gap from a genuinely
    /// empty graph.
    #[test]
    fn a_reified_relationship_answers_an_rdf_reifies_pattern() {
        let d = dataset(&[
            flake("r1", "fromEntity", FlakeValue::Ref(Sid::dsc("orders"))),
            flake("r1", "toEntity", FlakeValue::Ref(Sid::dsc("reports"))),
            flake("r1", "relType", FlakeValue::String("feeds".into())),
        ]);

        let reifies = Term::NamedNode(
            oxrdf::NamedNode::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies")
                .expect("node"),
        );
        let matches: Vec<_> = (&d)
            .internal_quads_for_pattern(None, Some(&reifies), None, Some(None))
            .collect::<Result<_, _>>()
            .expect("no term errors");
        assert_eq!(matches.len(), 1, "expected exactly one synthesized quad");
        let Term::Triple(inner) = &matches[0].object else {
            panic!("expected a triple term");
        };
        assert_eq!(
            inner.predicate.to_string(),
            "<https://graph-owl.dev/ns/catalog#feeds>"
        );
    }

    /// **Store flake count unchanged** — decision 7's own stated invariant.
    /// Synthesis adds to the *quad* list `from_flakes` builds in memory; it
    /// must never be mistaken for a reason to write anything, and this
    /// dataset has no write path at all to accidentally exercise.
    #[test]
    fn synthesis_adds_a_quad_without_adding_a_flake() {
        let flakes = [
            flake("r1", "fromEntity", FlakeValue::Ref(Sid::dsc("orders"))),
            flake("r1", "toEntity", FlakeValue::Ref(Sid::dsc("reports"))),
            flake("r1", "relType", FlakeValue::String("feeds".into())),
        ];
        let d = dataset(&flakes);
        // Three flakes in; the dataset carries a fourth quad (the
        // synthesized reifier) that names nothing beyond what the three
        // already asserted — `len()` proves the *quad* count grew, which
        // is the whole point, not a flake-count claim this type has no way
        // to make since it never touches storage.
        assert_eq!(flakes.len(), 3);
        assert_eq!(d.len(), 4, "3 ordinary quads + 1 synthesized rdf:reifies");
    }

    /// **The authorization RED test.** A relationship missing one endpoint
    /// — exactly what an access-predicate filter leaves behind when it
    /// removes an entity the caller may not see — must not synthesize a
    /// reifying quad. Mutator watch: emitting the quad from an
    /// unfiltered/partial flake set must fail this.
    #[test]
    fn a_relationship_missing_an_endpoint_synthesizes_nothing() {
        let d = dataset(&[
            flake("r1", "fromEntity", FlakeValue::Ref(Sid::dsc("orders"))),
            flake("r1", "relType", FlakeValue::String("feeds".into())),
            // No `toEntity` — as if the access predicate removed it.
        ]);

        let reifies = Term::NamedNode(
            oxrdf::NamedNode::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies")
                .expect("node"),
        );
        let matches: Vec<_> = (&d)
            .internal_quads_for_pattern(None, Some(&reifies), None, Some(None))
            .collect::<Result<Vec<_>, _>>()
            .expect("no term errors");
        assert!(matches.is_empty(), "expected no synthesized quad");
    }

    /// The negative case: an ordinary subject with no relationship shape
    /// must not gain a phantom `rdf:reifies` quad.
    #[test]
    fn an_ordinary_subject_synthesizes_nothing() {
        let d = dataset(&[flake("t1", "name", FlakeValue::String("orders".into()))]);
        assert_eq!(d.len(), 1, "no synthesized quad for a non-relationship");
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
