//! Turning a SPARQL query into the narrowest set of flake scans that can
//! answer it.
//!
//! Without this the engine materialises every fact the caller may see and lets
//! the evaluator filter — correct, and O(estate) per query regardless of how
//! specific the question was. Asking for one predicate should read one
//! predicate's flakes.
//!
//! **This does not decide *how* to scan.** It produces flake patterns; which
//! index serves each is the storage adapter's judgement (`04-engine-triples.md`
//! Slice B), and keeping that split is what stops query planning leaking into
//! the caller.

use graph_owl_core::flake::{Sid, TriplePattern as FlakePattern, namespace};
use spargebra::Query;
use spargebra::algebra::GraphPattern;
use spargebra::term::{TermPattern, TriplePattern};

use crate::term::from_term;

/// The DSC namespace's own relationship-shape predicates — duplicated from
/// `graph-owl-query::dataset`'s identical constants rather than shared,
/// for the same reason `dataset.rs` gives for not sharing with
/// `graph-owl-rdf-io`: three string literals are cheaper than a crate to
/// host them, and the two copies are pinned together by
/// `an_rdf_reifies_pattern_scans_the_relationship_shape_not_the_synthetic_predicate`.
const REL_FROM_ENTITY: &str = "fromEntity";
const REL_TO_ENTITY: &str = "toEntity";
const REL_TYPE: &str = "relType";

/// The scans that together cover everything a query could match.
///
/// A **superset** of what the answer needs, deliberately. A pattern sharing a
/// variable with another matches more rows alone than it does after the join,
/// and narrowing further would mean doing the join here — which is the
/// evaluator's work, done better.
///
/// `None` means "no pattern could be extracted, scan everything". Returning an
/// empty list instead would be a silent wrong answer: no scans, no facts, no
/// rows, no error.
#[must_use]
pub fn scans_for(query: &Query) -> Option<Vec<FlakePattern>> {
    let root = match query {
        Query::Select { pattern, .. }
        | Query::Ask { pattern, .. }
        | Query::Construct { pattern, .. }
        | Query::Describe { pattern, .. } => pattern,
    };

    let mut out = Vec::new();
    if !collect(root, &mut out) {
        return None;
    }
    // Every pattern unbound is the same as no pushdown, and pretending
    // otherwise would issue a full scan per pattern instead of one.
    if out.iter().all(is_wide_open) {
        return None;
    }
    Some(out)
}

/// Walks the algebra. Returns `false` when it meets a construct whose facts it
/// cannot bound, which forces a full scan rather than a guess.
// The wildcard below is deliberate and must stay a wildcard: its whole purpose
// is that an algebra node this build has never seen falls into it. Enumerating
// today's variants would turn "a future construct scans everything, safely"
// into "a future construct is narrowed by accident".
#[allow(clippy::match_wildcard_for_single_variants)]
fn collect(pattern: &GraphPattern, out: &mut Vec<FlakePattern>) -> bool {
    match pattern {
        GraphPattern::Bgp { patterns } => {
            out.extend(patterns.iter().flat_map(flake_patterns_for));
            true
        }
        // Every binary combinator: both sides contribute patterns, and both
        // must be boundable for the whole to be.
        GraphPattern::Join { left, right }
        | GraphPattern::Union { left, right }
        | GraphPattern::Minus { left, right }
        | GraphPattern::LeftJoin { left, right, .. } => collect(left, out) && collect(right, out),
        GraphPattern::Filter { inner, .. }
        | GraphPattern::Extend { inner, .. }
        | GraphPattern::OrderBy { inner, .. }
        | GraphPattern::Project { inner, .. }
        | GraphPattern::Distinct { inner }
        | GraphPattern::Reduced { inner }
        | GraphPattern::Slice { inner, .. }
        | GraphPattern::Group { inner, .. }
        | GraphPattern::Service { inner, .. }
        // A named graph restricts *where* to look, not what — its inner
        // patterns still bound the facts, so it belongs with the rest.
        | GraphPattern::Graph { inner, .. } => collect(inner, out),
        // Inline values bind variables without touching storage.
        GraphPattern::Values { .. } => true,
        // Everything else scans the whole store rather than guessing. Two
        // cases land here and both must:
        //
        // - **A property path** walks an unknown number of hops, so the facts
        //   it reaches are not bounded by the pattern. Narrowing would return a
        //   *subset* — a wrong answer that looks like a small one.
        // - **An algebra node this build does not recognise.** A new construct
        //   must never silently narrow a query.
        //
        // Written as one arm rather than an explicit `Path` case plus a
        // fallback: the two were behaviourally identical, so the extra branch
        // was a claim no test could ever check.
        _ => false,
    }
}

/// One BGP triple pattern normally becomes one scan. `(?rel) rdf:reifies
/// <<( ... )>>` is the exception: `rdf:reifies` is never a stored predicate
/// (Epic 94 decision 3 — a triple term is synthesized at query time), so
/// narrowing to it the way `flake_pattern` narrows any other bound
/// predicate would scan for flakes that provably do not exist and hand
/// `dataset.rs`'s synthesis pass nothing to work with — the exact zero-rows
/// failure Epic 94 Slice D exists to prevent, reached through pushdown
/// instead of through synthesis itself. The fix is to scan for the shape
/// synthesis actually reads: the reified relationship's own
/// `fromEntity`/`toEntity`/`relType` flakes.
fn flake_patterns_for(pattern: &TriplePattern) -> Vec<FlakePattern> {
    if is_reifies_pattern(pattern) {
        return reification_scans(named(&pattern.subject).as_ref());
    }
    vec![flake_pattern(pattern)]
}

/// `true` for exactly the shape `dataset.rs::reifying_quad` answers: a
/// bound `rdf:reifies` predicate against a triple-term object. A variable
/// predicate (`?p rdf:reifies ...` never happens in practice, but `?rel ?p
/// <<(...)>>` does) or a non-triple-term object is an ordinary pattern and
/// falls through to the generic narrowing instead.
fn is_reifies_pattern(pattern: &TriplePattern) -> bool {
    let is_reifies_predicate = pattern
        .predicate
        .clone()
        .into_term_pattern_named()
        .and_then(|node| Sid::from_iri(node.as_str()))
        == Some(Sid::new(namespace::RDF, "reifies"));
    is_reifies_predicate && matches!(pattern.object, TermPattern::Triple(_))
}

/// The three scans that together cover every reified relationship in the
/// estate — a superset, same as every other pushdown scan (see the module
/// doc): narrowing further would mean resolving which relationship the
/// triple-term's own variables could match, which is the evaluator's join
/// to do, not pushdown's.
fn reification_scans(subject: Option<&Sid>) -> Vec<FlakePattern> {
    [REL_FROM_ENTITY, REL_TO_ENTITY, REL_TYPE]
        .into_iter()
        .map(|id| FlakePattern {
            s: subject.cloned(),
            p: Some(Sid::new(namespace::DSC, id)),
            o: None,
            cx: None,
            as_of: None,
        })
        .collect()
}

fn flake_pattern(pattern: &TriplePattern) -> FlakePattern {
    FlakePattern {
        s: named(&pattern.subject),
        p: pattern
            .predicate
            .clone()
            .into_term_pattern_named()
            .and_then(|node| Sid::from_iri(node.as_str())),
        o: match &pattern.object {
            TermPattern::Variable(_) | TermPattern::BlankNode(_) => None,
            other => oxrdf::Term::try_from(other.clone())
                .ok()
                .and_then(|term| from_term(&term).ok()),
        },
        // Deliberately unset. The caller owns `as_of` — it is a property of the
        // question, not of the pattern, and letting pushdown decide it would
        // put time travel in two places.
        cx: None,
        as_of: None,
    }
}

fn named(pattern: &TermPattern) -> Option<Sid> {
    match pattern {
        TermPattern::NamedNode(node) => Sid::from_iri(node.as_str()),
        // Anything else constrains nothing. Binding a variable subject would
        // return one subject's facts for a question about all of them.
        // A triple term belongs here too, not as an oversight: this store
        // has no `Sid` for one (Epic 94 decision 2, object position only),
        // so a triple-term-shaped pattern narrows a scan exactly as much
        // as a variable does — nothing. `rdf:reifies` matching is handled
        // as a separate, specially-recognized pattern shape (Slice D),
        // never through this generic subject/predicate narrowing.
        TermPattern::Variable(_)
        | TermPattern::BlankNode(_)
        | TermPattern::Literal(_)
        | TermPattern::Triple(_) => None,
    }
}

/// A pattern that constrains nothing scans the whole store.
fn is_wide_open(pattern: &FlakePattern) -> bool {
    pattern.s.is_none() && pattern.p.is_none() && pattern.o.is_none()
}

/// `spargebra` models a predicate as its own type; this is the only place that
/// shape matters.
trait NamedNodePatternExt {
    fn into_term_pattern_named(self) -> Option<oxrdf::NamedNode>;
}

impl NamedNodePatternExt for spargebra::term::NamedNodePattern {
    fn into_term_pattern_named(self) -> Option<oxrdf::NamedNode> {
        match self {
            spargebra::term::NamedNodePattern::NamedNode(node) => Some(node),
            spargebra::term::NamedNodePattern::Variable(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spargebra::SparqlParser;

    const DSC: &str = "https://graph-owl.dev/ns/catalog#";

    fn parse(sparql: &str) -> Query {
        SparqlParser::new().parse_query(sparql).expect("parses")
    }

    #[test]
    fn a_bound_predicate_becomes_a_narrowed_scan() {
        let scans = scans_for(&parse(&format!("SELECT ?n WHERE {{ ?s <{DSC}name> ?n }}")))
            .expect("pushdown");

        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].p.as_ref().map(|p| p.id.as_str()), Some("name"));
        assert!(scans[0].s.is_none(), "the subject is a variable");
    }

    #[test]
    fn every_pattern_in_a_join_contributes_a_scan() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?n WHERE {{ ?s <{DSC}type> \"table\" . ?s <{DSC}name> ?n }}"
        )))
        .expect("pushdown");

        let mut predicates: Vec<&str> = scans
            .iter()
            .filter_map(|s| s.p.as_ref().map(|p| p.id.as_str()))
            .collect();
        predicates.sort_unstable();
        assert_eq!(predicates, vec!["name", "type"]);
    }

    #[test]
    fn a_bound_object_narrows_the_scan_too() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?s WHERE {{ ?s <{DSC}type> \"table\" }}"
        )))
        .expect("pushdown");
        assert!(scans[0].o.is_some(), "the object literal is bound");
    }

    #[test]
    fn patterns_inside_optional_and_filter_are_still_found() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?n WHERE {{
               ?s <{DSC}name> ?n .
               OPTIONAL {{ ?s <{DSC}description> ?d }}
               FILTER(BOUND(?n))
             }}"
        )))
        .expect("pushdown");

        let predicates: Vec<&str> = scans
            .iter()
            .filter_map(|s| s.p.as_ref().map(|p| p.id.as_str()))
            .collect();
        assert!(predicates.contains(&"description"), "{predicates:?}");
    }

    #[test]
    fn both_sides_of_a_union_contribute() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?s WHERE {{
               {{ ?s <{DSC}name> ?n }} UNION {{ ?s <{DSC}fqn> ?f }}
             }}"
        )))
        .expect("pushdown");
        assert_eq!(scans.len(), 2, "{scans:?}");
    }

    /// **The safety property.** A property path walks an unbounded number of
    /// hops, so the facts it reaches are not bounded by the pattern. Narrowing
    /// anyway would return a subset — a wrong answer that looks like a small
    /// one, which is the worst kind.
    #[test]
    fn a_property_path_forces_a_full_scan() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?s WHERE {{ ?s <{DSC}parentSchema>+ ?root }}"
        )));
        assert!(scans.is_none(), "must refuse to narrow: {scans:?}");
    }

    /// A **bound subject** narrows the scan too, and nothing tested it — every
    /// pattern above uses a variable subject, so the whole subject branch was
    /// unexercised.
    #[test]
    fn a_bound_subject_narrows_the_scan() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?n WHERE {{ <{DSC}upi> <{DSC}name> ?n }}"
        )))
        .expect("pushdown");

        assert_eq!(
            scans[0].s.as_ref().map(|s| s.id.as_str()),
            Some("upi"),
            "the subject IRI must reach the scan"
        );
    }

    /// A variable subject must *not* be narrowed — binding it to something
    /// would return one subject's facts for a query about all of them.
    #[test]
    fn a_variable_subject_leaves_the_scan_open() {
        let scans = scans_for(&parse(&format!("SELECT ?n WHERE {{ ?s <{DSC}name> ?n }}")))
            .expect("pushdown");
        assert!(scans[0].s.is_none());
    }

    /// `GRAPH` restricts *where* to look, not what. Its inner patterns still
    /// bound the facts, so a graph-scoped query must still push down rather
    /// than falling back to a full scan.
    #[test]
    fn a_graph_scoped_query_still_pushes_down() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?n WHERE {{ GRAPH <{DSC}graph:extraction> {{ ?s <{DSC}name> ?n }} }}"
        )))
        .expect("pushdown");

        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].p.as_ref().map(|p| p.id.as_str()), Some("name"));
    }

    /// `VALUES` binds variables inline without touching storage, so it neither
    /// contributes a scan nor forces a full one.
    #[test]
    fn inline_values_do_not_force_a_full_scan() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?n WHERE {{
               VALUES ?s {{ <{DSC}upi> }}
               ?s <{DSC}name> ?n
             }}"
        )))
        .expect("VALUES must not defeat pushdown");

        assert!(
            scans
                .iter()
                .any(|s| s.p.as_ref().is_some_and(|p| p.id == "name")),
            "{scans:?}"
        );
    }

    /// A wholly unbound query gains nothing from pushdown, and issuing a full
    /// scan *per pattern* would be strictly worse than one.
    #[test]
    fn a_completely_open_query_does_not_push_down() {
        assert!(scans_for(&parse("SELECT ?s WHERE { ?s ?p ?o }")).is_none());
    }

    /// One bound pattern is enough to be worth pushing down, even alongside an
    /// open one.
    #[test]
    fn a_mixed_query_still_pushes_down() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?s WHERE {{ ?s <{DSC}name> ?n . ?s ?p ?o }}"
        )))
        .expect("pushdown");
        assert_eq!(scans.len(), 2, "both patterns are scanned: {scans:?}");
    }

    #[test]
    fn an_ask_query_pushes_down_like_a_select() {
        let scans =
            scans_for(&parse(&format!("ASK {{ ?s <{DSC}name> \"x\" }}"))).expect("pushdown");
        assert_eq!(scans.len(), 1);
    }

    /// Pushdown must never set `as_of`. Time is a property of the question and
    /// the caller owns it; deciding it here would put time travel in two
    /// places that could disagree.
    #[test]
    fn pushdown_never_decides_the_transaction_time() {
        let scans = scans_for(&parse(&format!("SELECT ?n WHERE {{ ?s <{DSC}name> ?n }}")))
            .expect("pushdown");
        assert!(scans.iter().all(|s| s.as_of.is_none()));
    }

    /// **Epic 94 Slice D's own RED test, at the pushdown layer.** `rdf:reifies`
    /// is never a stored predicate (decision 3), so narrowing to it the way
    /// every other bound predicate narrows would scan for flakes that
    /// provably do not exist — zero facts reach `dataset.rs`'s synthesis
    /// pass, and a query against a real relationship returns zero rows. The
    /// fix scans for the relationship shape synthesis actually reads instead.
    #[test]
    fn an_rdf_reifies_pattern_scans_the_relationship_shape_not_the_synthetic_predicate() {
        let scans = scans_for(&parse(
            "SELECT ?from ?to WHERE { \
                ?rel <http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies> \
                     <<( ?from <https://graph-owl.dev/ns/catalog#feeds> ?to )>> . \
             }",
        ))
        .expect("pushdown");

        assert!(
            scans
                .iter()
                .all(|s| s.p.as_ref().is_some_and(|p| p.id.as_str() != "reifies")),
            "must never scan for the synthetic predicate itself, which no flake has: {scans:?}"
        );

        let mut predicates: Vec<&str> = scans
            .iter()
            .filter_map(|s| s.p.as_ref().map(|p| p.id.as_str()))
            .collect();
        predicates.sort_unstable();
        assert_eq!(
            predicates,
            vec!["fromEntity", "relType", "toEntity"],
            "must scan the relationship shape dataset.rs::reifier_endpoints reads: {scans:?}"
        );
    }

    /// A bound relationship IRI narrows the reification scans by subject too,
    /// same as any other pattern — the object being a triple term changes
    /// *which* predicates are scanned, not whether a bound subject narrows.
    #[test]
    fn a_bound_relationship_narrows_the_reification_scans_by_subject() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?from ?to WHERE {{ \
                <{DSC}edge-orders-feeds-reports> \
                     <http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies> \
                     <<( ?from <{DSC}feeds> ?to )>> . \
             }}"
        )))
        .expect("pushdown");

        assert!(
            scans
                .iter()
                .all(|s| s.s.as_ref().map(|s| s.id.as_str()) == Some("edge-orders-feeds-reports")),
            "{scans:?}"
        );
    }

    /// A variable predicate against a triple-term object is an ordinary,
    /// wholly unbound pattern, not the `rdf:reifies` shape — same
    /// fall-to-full-scan behaviour as any other pattern where subject,
    /// predicate and object all constrain nothing
    /// (`a_completely_open_query_does_not_push_down`), not the reification
    /// scans.
    #[test]
    fn a_variable_predicate_against_a_triple_term_object_is_not_the_reifies_shape() {
        let scans = scans_for(&parse(&format!(
            "SELECT ?p WHERE {{ ?rel ?p <<( ?from <{DSC}feeds> ?to )>> }}"
        )));
        assert!(
            scans.is_none(),
            "wholly unbound must force a full scan: {scans:?}"
        );
    }
}
