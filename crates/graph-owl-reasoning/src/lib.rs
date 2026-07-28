//! OWL 2 RL forward-chaining overlay (pure, no I/O) — Epic 6.
//!
//! Eight axioms as **built-in functions** rather than a rule interpreter over an
//! OWL encoding (`06-engine-reasoning.md` Slice A). Eight things that can be
//! read and tested beat an interpreter plus an encoding nobody can debug.

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use std::collections::HashSet;

fn v(ns: u16, id: &str) -> Sid {
    Sid::new(ns, id)
}

fn rdf_type() -> Sid {
    v(namespace::RDF, "type")
}

/// The object as a reference, or `None` for a literal.
///
/// Every one of the eight axioms relates *entities*. A literal object cannot be
/// a class, a property or an identity, so a rule reading one would be reasoning
/// about something that cannot participate.
fn obj(flake: &Flake) -> Option<&Sid> {
    match &flake.o {
        FlakeValue::Ref(sid) => Some(sid),
        _ => None,
    }
}

/// Flakes stating `p` — asserted only.
fn with_predicate<'a>(facts: &'a [Flake], p: &Sid) -> impl Iterator<Item = &'a Flake> {
    let p = p.clone();
    facts.iter().filter(move |f| f.op && f.p == p)
}

/// Subjects declared to be of type `class`.
fn typed_as(facts: &[Flake], class: &Sid) -> HashSet<Sid> {
    with_predicate(facts, &rdf_type())
        .filter(|f| obj(f) == Some(class))
        .map(|f| f.s.clone())
        .collect()
}

/// A conclusion, stamped so it cannot appear older than what produced it.
///
/// `t` is the **maximum** of the premises. A derived fact carrying an earlier
/// `t` would be visible at an instant before the facts that imply it, which
/// would make time travel and reasoning disagree about the same moment.
fn conclude(s: Sid, p: Sid, o: Sid, premises: &[&Flake]) -> Flake {
    Flake {
        s,
        p,
        o: FlakeValue::Ref(o),
        cx: None,
        t: premises.iter().map(|f| f.t).max().unwrap_or(0),
        op: true,
    }
}

fn rule_sub_class_of(facts: &[Flake], out: &mut Vec<Flake>) {
    // rdfs:subClassOf — (a type C1), (C1 subClassOf C2) => (a type C2)
    for axiom in with_predicate(facts, &v(namespace::RDFS, "subClassOf")) {
        let Some(super_class) = obj(axiom) else {
            continue;
        };
        for member in with_predicate(facts, &rdf_type()).filter(|f| obj(f) == Some(&axiom.s)) {
            out.push(conclude(
                member.s.clone(),
                rdf_type(),
                super_class.clone(),
                &[axiom, member],
            ));
        }
    }
}

fn rule_sub_property_of(facts: &[Flake], out: &mut Vec<Flake>) {
    // rdfs:subPropertyOf — (a p1 b), (p1 subPropertyOf p2) => (a p2 b)
    for axiom in with_predicate(facts, &v(namespace::RDFS, "subPropertyOf")) {
        let Some(super_property) = obj(axiom) else {
            continue;
        };
        for used in with_predicate(facts, &axiom.s) {
            let Some(object) = obj(used) else { continue };
            out.push(conclude(
                used.s.clone(),
                super_property.clone(),
                object.clone(),
                &[axiom, used],
            ));
        }
    }
}

fn rule_transitive(facts: &[Flake], out: &mut Vec<Flake>) {
    // owl:TransitiveProperty — (a p b), (b p c) => (a p c)
    for property in typed_as(facts, &v(namespace::OWL, "TransitiveProperty")) {
        let edges: Vec<&Flake> = with_predicate(facts, &property).collect();
        for left in &edges {
            let Some(mid) = obj(left) else { continue };
            for right in edges.iter().filter(|r| &r.s == mid) {
                let Some(end) = obj(right) else { continue };
                out.push(conclude(
                    left.s.clone(),
                    property.clone(),
                    end.clone(),
                    &[left, right],
                ));
            }
        }
    }
}

fn rule_symmetric(facts: &[Flake], out: &mut Vec<Flake>) {
    // owl:SymmetricProperty — (a p b) => (b p a)
    for property in typed_as(facts, &v(namespace::OWL, "SymmetricProperty")) {
        for edge in with_predicate(facts, &property) {
            let Some(object) = obj(edge) else { continue };
            out.push(conclude(
                object.clone(),
                property.clone(),
                edge.s.clone(),
                &[edge],
            ));
        }
    }
}

fn rule_inverse_of(facts: &[Flake], out: &mut Vec<Flake>) {
    // owl:inverseOf — (a p1 b) => (b p2 a), read in **both** directions of the
    // axiom: `p inverseOf q` states `q inverseOf p` just as strongly.
    for axiom in with_predicate(facts, &v(namespace::OWL, "inverseOf")) {
        let Some(right) = obj(axiom) else { continue };
        for (from, to) in [(&axiom.s, right), (right, &axiom.s)] {
            for edge in with_predicate(facts, from) {
                let Some(object) = obj(edge) else { continue };
                out.push(conclude(
                    object.clone(),
                    to.clone(),
                    edge.s.clone(),
                    &[axiom, edge],
                ));
            }
        }
    }
}

fn rule_domain(facts: &[Flake], out: &mut Vec<Flake>) {
    // rdfs:domain — (a p b), (p domain C) => (a type C). Subject side.
    for axiom in with_predicate(facts, &v(namespace::RDFS, "domain")) {
        let Some(class) = obj(axiom) else { continue };
        for used in with_predicate(facts, &axiom.s) {
            out.push(conclude(
                used.s.clone(),
                rdf_type(),
                class.clone(),
                &[axiom, used],
            ));
        }
    }
}

fn rule_range(facts: &[Flake], out: &mut Vec<Flake>) {
    // rdfs:range — (a p b), (p range C) => (b type C). Object side.
    for axiom in with_predicate(facts, &v(namespace::RDFS, "range")) {
        let Some(class) = obj(axiom) else { continue };
        for used in with_predicate(facts, &axiom.s) {
            let Some(object) = obj(used) else { continue };
            out.push(conclude(
                object.clone(),
                rdf_type(),
                class.clone(),
                &[axiom, used],
            ));
        }
    }
}

fn rule_same_as(facts: &[Flake], out: &mut Vec<Flake>) {
    // owl:sameAs — (a sameAs b), (a p o) => (b p o), in both directions,
    // because identity that depended on assertion order would not be identity.
    for axiom in with_predicate(facts, &v(namespace::OWL, "sameAs")) {
        let Some(right) = obj(axiom) else { continue };
        for (from, to) in [(&axiom.s, right), (right, &axiom.s)] {
            for held in facts.iter().filter(|f| f.op && &f.s == from) {
                let Some(object) = obj(held) else { continue };
                out.push(conclude(
                    to.clone(),
                    held.p.clone(),
                    object.clone(),
                    &[axiom, held],
                ));
            }
        }
    }
}

/// One pass of all eight axioms.
///
/// Eight named functions rather than one body: `06-engine-reasoning.md` calls
/// for "eight testable functions", and a single pass long enough to trip the
/// line limit is a single pass nobody reviews rule by rule.
fn one_pass(facts: &[Flake]) -> Vec<Flake> {
    let mut out = Vec::new();
    for rule in [
        rule_sub_class_of,
        rule_sub_property_of,
        rule_transitive,
        rule_symmetric,
        rule_inverse_of,
        rule_domain,
        rule_range,
        rule_same_as,
    ] {
        rule(facts, &mut out);
    }
    out
}

/// Everything the facts imply but do not state.
///
/// Iterates because one pass reaches depth one: `C1 ⊑ C2 ⊑ C3` needs the first
/// conclusion in hand before the second can be drawn. Deduplication against
/// everything already known is what makes it **terminate** — a symmetric
/// property otherwise re-derives its own reverse forever.
#[must_use]
pub fn derive(facts: &[Flake]) -> Vec<Flake> {
    // Identity ignores `t`: the same triple reached by two routes is one
    // conclusion, and letting the stamp distinguish them would loop.
    //
    // The object is rendered rather than hashed because `FlakeValue` carries a
    // float, so it is deliberately not `Eq`/`Hash` — a NaN is not equal to
    // itself. Rendering sidesteps that without asserting an equality the type
    // declines to offer.
    fn key(f: &Flake) -> (Sid, Sid, String) {
        (f.s.clone(), f.p.clone(), format!("{:?}", f.o))
    }

    let mut known: HashSet<_> = facts.iter().filter(|f| f.op).map(key).collect();
    let mut all: Vec<Flake> = facts.to_vec();
    let mut derived = Vec::new();

    loop {
        let fresh: Vec<Flake> = one_pass(&all)
            .into_iter()
            .filter(|f| known.insert(key(f)))
            .collect();
        if fresh.is_empty() {
            return derived;
        }
        all.extend(fresh.iter().cloned());
        derived.extend(fresh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};

    fn sid(ns: u16, id: &str) -> Sid {
        Sid::new(ns, id)
    }

    /// An asserted fact at t=1, default graph.
    fn f(s: Sid, p: Sid, o: Sid) -> Flake {
        Flake {
            s,
            p,
            o: FlakeValue::Ref(o),
            cx: None,
            t: 1,
            op: true,
        }
    }

    fn a(id: &str) -> Sid {
        sid(namespace::DSC, id)
    }
    fn rdf_type() -> Sid {
        sid(namespace::RDF, "type")
    }
    fn sub_class_of() -> Sid {
        sid(namespace::RDFS, "subClassOf")
    }
    fn sub_property_of() -> Sid {
        sid(namespace::RDFS, "subPropertyOf")
    }
    fn domain() -> Sid {
        sid(namespace::RDFS, "domain")
    }
    fn range() -> Sid {
        sid(namespace::RDFS, "range")
    }
    fn transitive() -> Sid {
        sid(namespace::OWL, "TransitiveProperty")
    }
    fn symmetric() -> Sid {
        sid(namespace::OWL, "SymmetricProperty")
    }
    fn inverse_of() -> Sid {
        sid(namespace::OWL, "inverseOf")
    }
    fn same_as() -> Sid {
        sid(namespace::OWL, "sameAs")
    }

    /// Did `derive` produce this exact triple?
    fn derived(facts: &[Flake], s: Sid, p: Sid, o: Sid) -> bool {
        let want = (s, p, FlakeValue::Ref(o));
        derive(facts)
            .iter()
            .any(|d| (d.s.clone(), d.p.clone(), d.o.clone()) == want)
    }

    mod subsumption {
        use super::*;

        #[test]
        fn a_type_flows_up_three_levels_of_sub_class() {
            // Depth 3 is the specification, not depth 1: a single-step
            // implementation passes the shallow case and fails here.
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("C2"), sub_class_of(), a("C3")),
            ];

            assert!(derived(&facts, a("thing"), rdf_type(), a("C3")));
        }

        #[test]
        fn nothing_flows_up_without_the_sub_class_axiom() {
            let facts = vec![f(a("thing"), rdf_type(), a("C1"))];

            assert!(!derived(&facts, a("thing"), rdf_type(), a("C2")));
        }

        #[test]
        fn a_predicate_flows_up_its_sub_property_chain() {
            let facts = vec![
                f(a("x"), a("p1"), a("y")),
                f(a("p1"), sub_property_of(), a("p2")),
                f(a("p2"), sub_property_of(), a("p3")),
            ];

            assert!(derived(&facts, a("x"), a("p3"), a("y")));
        }
    }

    mod property_characteristics {
        use super::*;

        #[test]
        fn a_transitive_property_reaches_the_far_end_of_a_three_hop_chain() {
            let facts = vec![
                f(a("partOf"), rdf_type(), transitive()),
                f(a("a"), a("partOf"), a("b")),
                f(a("b"), a("partOf"), a("c")),
                f(a("c"), a("partOf"), a("d")),
            ];

            assert!(derived(&facts, a("a"), a("partOf"), a("d")));
        }

        #[test]
        fn a_property_not_declared_transitive_does_not_compose() {
            let facts = vec![
                f(a("likes"), rdf_type(), transitive()),
                f(a("a"), a("knows"), a("b")),
                f(a("b"), a("knows"), a("c")),
            ];

            // The declaration is present but names a *different* predicate. A
            // rule that fired on any transitive declaration anywhere would pass
            // this by accident.
            assert!(!derived(&facts, a("a"), a("knows"), a("c")));
        }

        #[test]
        fn two_disconnected_edges_do_not_compose_just_because_the_property_is_transitive() {
            // Transitivity joins on the *midpoint*. Without asserting a
            // non-composition, an implementation that joins on "any other edge"
            // still produces the far edge of a real chain and looks correct.
            let facts = vec![
                f(a("partOf"), rdf_type(), transitive()),
                f(a("a"), a("partOf"), a("b")),
                f(a("c"), a("partOf"), a("d")),
            ];

            assert!(derive(&facts).is_empty());
        }

        #[test]
        fn a_symmetric_property_derives_its_reverse() {
            let facts = vec![
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ];

            assert!(derived(&facts, a("y"), a("marriedTo"), a("x")));
        }

        #[test]
        fn a_property_not_declared_symmetric_does_not_reverse() {
            let facts = vec![f(a("parentOf"), a("x"), a("y"))];

            assert!(!derived(&facts, a("y"), a("parentOf"), a("x")));
        }

        #[test]
        fn an_inverse_property_derives_the_opposite_direction() {
            let facts = vec![
                f(a("hasParent"), inverse_of(), a("hasChild")),
                f(a("kid"), a("hasParent"), a("adult")),
            ];

            assert!(derived(&facts, a("adult"), a("hasChild"), a("kid")));
        }

        #[test]
        fn inverse_is_read_in_both_directions_of_the_axiom() {
            // `p inverseOf q` also means `q inverseOf p`. Implementing only one
            // direction silently halves the rule.
            let facts = vec![
                f(a("hasParent"), inverse_of(), a("hasChild")),
                f(a("adult"), a("hasChild"), a("kid")),
            ];

            assert!(derived(&facts, a("kid"), a("hasParent"), a("adult")));
        }
    }

    mod domain_and_range {
        use super::*;

        #[test]
        fn domain_types_the_subject() {
            let facts = vec![
                f(a("worksAt"), domain(), a("Person")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("alice"), rdf_type(), a("Person")));
        }

        #[test]
        fn domain_does_not_type_the_object() {
            // The classic bug is a swap. Asserting only the positive case lets
            // an implementation that types the wrong side pass.
            let facts = vec![
                f(a("worksAt"), domain(), a("Person")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(!derived(&facts, a("acme"), rdf_type(), a("Person")));
        }

        #[test]
        fn range_types_the_object() {
            let facts = vec![
                f(a("worksAt"), range(), a("Company")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("acme"), rdf_type(), a("Company")));
        }

        #[test]
        fn range_does_not_type_the_subject() {
            let facts = vec![
                f(a("worksAt"), range(), a("Company")),
                f(a("alice"), a("worksAt"), a("acme")),
            ];

            assert!(!derived(&facts, a("alice"), rdf_type(), a("Company")));
        }
    }

    mod identity {
        use super::*;

        #[test]
        fn same_as_copies_properties_forward() {
            let facts = vec![
                f(a("x"), same_as(), a("y")),
                f(a("x"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("y"), a("worksAt"), a("acme")));
        }

        #[test]
        fn same_as_copies_only_from_the_identified_pair() {
            // A third entity's properties are nobody else's. Without this, a
            // condition that widened to "any asserted fact" would copy the
            // whole graph onto both sides of every identity.
            let facts = vec![
                f(a("x"), same_as(), a("y")),
                f(a("z"), a("worksAt"), a("acme")),
            ];

            assert!(!derived(&facts, a("x"), a("worksAt"), a("acme")));
            assert!(!derived(&facts, a("y"), a("worksAt"), a("acme")));
        }

        #[test]
        fn same_as_does_not_copy_a_retracted_property() {
            let mut withdrawn = f(a("x"), a("worksAt"), a("acme"));
            withdrawn.op = false;
            let facts = vec![f(a("x"), same_as(), a("y")), withdrawn];

            assert!(!derived(&facts, a("y"), a("worksAt"), a("acme")));
        }

        #[test]
        fn same_as_copies_properties_backward() {
            // `sameAs` is symmetric. Copying only left-to-right makes identity
            // depend on which way the assertion happened to be written.
            let facts = vec![
                f(a("x"), same_as(), a("y")),
                f(a("y"), a("worksAt"), a("acme")),
            ];

            assert!(derived(&facts, a("x"), a("worksAt"), a("acme")));
        }
    }

    mod what_derivation_returns {
        use super::*;

        #[test]
        fn asserted_facts_are_not_returned_as_derivations() {
            // The result is what reasoning *added*. Echoing the input back would
            // make the overlay indistinguishable from the base — the thing
            // `00b` decision 14 exists to prevent.
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
            ];

            let out = derive(&facts);
            assert!(!out.iter().any(|d| facts.contains(d)));
            assert!(!out.is_empty());
        }

        #[test]
        fn a_derived_fact_never_predates_the_premises_that_produced_it() {
            let mut old = f(a("thing"), rdf_type(), a("C1"));
            old.t = 3;
            let mut newer = f(a("C1"), sub_class_of(), a("C2"));
            newer.t = 9;

            let out = derive(&[old, newer]);
            assert!(out.iter().all(|d| d.t == 9));
        }

        #[test]
        fn a_retraction_derives_nothing() {
            // `op = false` withdraws the fact. Reasoning over it would derive
            // conclusions from a premise the graph no longer states.
            let mut retracted = f(a("thing"), rdf_type(), a("C1"));
            retracted.op = false;
            let facts = vec![retracted, f(a("C1"), sub_class_of(), a("C2"))];

            assert!(derive(&facts).is_empty());
        }

        #[test]
        fn deriving_twice_over_the_same_facts_produces_the_same_set() {
            let facts = vec![
                f(a("thing"), rdf_type(), a("C1")),
                f(a("C1"), sub_class_of(), a("C2")),
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ];

            assert_eq!(derive(&facts), derive(&facts));
        }

        #[test]
        fn a_symmetric_property_terminates_rather_than_ping_ponging() {
            // (x p y) -> (y p x) -> (x p y) -> ... A run without deduplication
            // does not return.
            let facts = vec![
                f(a("marriedTo"), rdf_type(), symmetric()),
                f(a("x"), a("marriedTo"), a("y")),
            ];

            assert_eq!(derive(&facts).len(), 1);
        }
    }
}
