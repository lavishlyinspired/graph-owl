//! OWL profile detection and reasoner routing — Epic 100.
//!
//! Three engines exist: RL (`graph_owl_reasoning`), EL
//! (`graph_owl_reasoning_el`), QL (`graph_owl_reasoning_ql`). None of the
//! three profiles is a subset of another, so an ontology must be checked
//! against each one's own grammar rather than assumed "at least RL" —
//! `99-owl-ql-reasoning.md` cites the same incomparability for QL/RL, and
//! `98-owl-el-reasoning.md` for EL/RL.
//!
//! **Scope, recorded rather than silently narrowed** (see
//! `100-profile-detection-and-routing.md`'s own "scope decision"):
//! detection here checks construct *presence* — the same shape
//! `graph_owl_reasoning_ql`/`graph_owl_reasoning_el` already established
//! for their own forbidden-axiom checks — extended with a *value* check
//! for RL's `owl:maxCardinality` (legal only at 0 or 1). It does not parse
//! the full sub/super-class-position-sensitive grammar (RL's
//! `AllValuesFrom` is legal as a superclass restriction and illegal as
//! part of a subclass's own definition; this pass does not distinguish
//! the two). A construct this module flags as out-of-profile always
//! genuinely is; the positional gap could in principle miss a genuine
//! violation, never invent a false one.
//!
//! Every grammar fact here was checked verbatim against the W3C OWL 2
//! Profiles document, not summarised — a prior session found this exact
//! document self-contradicts under interpretive (rather than
//! quote-only) requests. See the plan's own methodology note.

use graph_owl_core::flake::Sid;
use std::collections::HashMap;

/// Which OWL profile a `TBox` was checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Profile {
    Rl,
    El,
    Ql,
}

/// One construct that excludes a `TBox` from a profile, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub subject: Sid,
    pub reason: String,
}

/// One profile's verdict — `member` is `true` exactly when `violations` is
/// empty, kept as two fields rather than derived so a caller need not
/// recompute it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMembership {
    pub member: bool,
    pub violations: Vec<Violation>,
}

impl ProfileMembership {
    fn from_violations(violations: Vec<Violation>) -> Self {
        Self {
            member: violations.is_empty(),
            violations,
        }
    }
}

/// A cardinality restriction's own shape — found directly on a
/// (skolemized) restriction node, never on the class it restricts. In the
/// OWL/RDF mapping a cardinality restriction is always an anonymous class
/// expression (`Person rdfs:subClassOf [ owl:onProperty p ;
/// owl:maxCardinality 2 ]`); the class reaches it only via `subClassOf`.
/// The identical fact `graph_owl_reasoning_el`'s own `restriction_constructs`
/// doc comment already records for EL's `allValuesFrom`/`unionOf`/
/// `complementOf` — cardinality needs the same treatment for RL, found by
/// this epic's own integration test after an earlier version of this
/// check put the predicate directly on the class, silently under
/// -detecting every real cardinality restriction a caller would actually
/// assert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlCardinalityShape {
    /// `owl:maxCardinality`, unqualified, with its literal value — legal
    /// only at 0 or 1 (`zeroOrOne ObjectPropertyExpression` in the
    /// grammar).
    MaxCardinality(i64),
    /// `owl:minCardinality`, `owl:cardinality`, or either qualified
    /// -cardinality predicate — forbidden at any value. RL's grammar
    /// names only `superObjectMaxCardinality`/`superDataMaxCardinality`,
    /// nothing else in the cardinality family.
    Other,
}

/// The RL-relevant slice of a `TBox` — presence of the constructs RL's own
/// grammar (§4.2.3, verified verbatim) forbids. A caller fetches this the
/// same way `graph_owl_reasoning_ql`/`_el`'s own callers fetch their
/// `Tbox`es: via `TripleStore::query_pattern`, independent of
/// `scoped_facts`' visibility filter, since a `TBox` is schema, not row
/// data with an owner.
#[derive(Debug, Clone, Default)]
pub struct RlTbox {
    /// `owl:disjointUnionOf` subjects — the `DisjointUnion` axiom, absent
    /// from RL's grammar entirely. Asserted directly on the class, no
    /// restriction node involved (`Person owl:disjointUnionOf (A B)`).
    pub disjoint_unions: Vec<Sid>,
    /// Subjects typed `owl:ReflexiveProperty` — `ReflexiveObjectProperty`,
    /// the one property characteristic RL's own `ObjectPropertyAxiom`
    /// list omits (Functional, `InverseFunctional`, Transitive, Symmetric,
    /// Asymmetric, Irreflexive are all present in that same list).
    /// Asserted directly on the property.
    pub reflexive_properties: Vec<Sid>,
    /// Every `rdfs:subClassOf` edge — needed to connect a named class back
    /// to a cardinality restriction it references, the same walk
    /// `graph_owl_reasoning_el::find_forbidden_axioms` already does.
    pub subclass_of: Vec<(Sid, Sid)>,
    /// A (skolemized) restriction node's own `Sid` and cardinality shape.
    pub restriction_cardinalities: Vec<(Sid, RlCardinalityShape)>,
}

/// **Slice A.** RL membership from real predicate presence, a `subClassOf`
/// walk into cardinality restrictions, and (for `maxCardinality`) value.
/// Pure: no I/O.
#[must_use]
pub fn detect_rl(tbox: &RlTbox) -> ProfileMembership {
    let mut violations: Vec<Violation> = Vec::new();
    violations.extend(tbox.disjoint_unions.iter().map(|sid| Violation {
        subject: sid.clone(),
        reason: "owl:disjointUnionOf (DisjointUnion) is not permitted in OWL 2 RL".to_string(),
    }));
    violations.extend(tbox.reflexive_properties.iter().map(|sid| Violation {
        subject: sid.clone(),
        reason: "ReflexiveObjectProperty is not permitted in OWL 2 RL".to_string(),
    }));

    let restrictions: HashMap<&Sid, RlCardinalityShape> = tbox
        .restriction_cardinalities
        .iter()
        .map(|(sid, shape)| (sid, *shape))
        .collect();
    violations.extend(tbox.subclass_of.iter().filter_map(|(class, object)| {
        restrictions.get(object).and_then(|shape| match shape {
            RlCardinalityShape::MaxCardinality(value) if *value > 1 => Some(Violation {
                subject: class.clone(),
                reason: format!("owl:maxCardinality {value} exceeds OWL 2 RL's 0-or-1 limit"),
            }),
            RlCardinalityShape::MaxCardinality(_) => None,
            RlCardinalityShape::Other => Some(Violation {
                subject: class.clone(),
                reason:
                    "only unqualified owl:maxCardinality valued 0 or 1 is permitted in OWL 2 RL"
                        .to_string(),
            }),
        })
    }));
    ProfileMembership::from_violations(violations)
}

/// **Slice A.** EL membership, reusing `graph_owl_reasoning_el`'s own
/// forbidden-axiom check directly (verified in Epic 98) rather than
/// re-deriving it. Pure: no I/O.
#[must_use]
pub fn detect_el(tbox: &graph_owl_reasoning_el::Tbox) -> ProfileMembership {
    let violations = graph_owl_reasoning_el::find_forbidden_axioms(tbox)
        .into_iter()
        .map(|axiom| Violation {
            subject: axiom.subject,
            reason: format!("{:?} is not permitted in OWL 2 EL", axiom.construct),
        })
        .collect();
    ProfileMembership::from_violations(violations)
}

/// **Slice B.** QL membership. `graph_owl_reasoning_ql::Tbox::forbidden`
/// is already the resolved violation list (that crate's own caller
/// populates it directly via predicate presence, with no restriction
/// -walk needed — every QL-forbidden construct sits directly on its
/// subject). Pure: no I/O.
#[must_use]
pub fn detect_ql(tbox: &graph_owl_reasoning_ql::Tbox) -> ProfileMembership {
    let violations = tbox
        .forbidden
        .iter()
        .map(|axiom| Violation {
            subject: axiom.class.clone(),
            reason: format!("{:?} is not permitted in OWL 2 QL", axiom.construct),
        })
        .collect();
    ProfileMembership::from_violations(violations)
}

/// Every profile's own verdict over the same ontology — decision 1's own
/// requirement: "in RL and QL, in none, or in all three", never collapsed
/// to a single answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub rl: ProfileMembership,
    pub el: ProfileMembership,
    pub ql: ProfileMembership,
}

impl Detection {
    /// **Slice B.** Every profile this `TBox` is actually a member of, in
    /// no particular preference order — [`route`] is what applies
    /// decision 5's preference.
    #[must_use]
    pub fn member_profiles(&self) -> Vec<Profile> {
        [
            (self.rl.member, Profile::Rl),
            (self.el.member, Profile::El),
            (self.ql.member, Profile::Ql),
        ]
        .into_iter()
        .filter_map(|(member, profile)| member.then_some(profile))
        .collect()
    }
}

/// What routing decided — Slice C.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingDecision {
    /// The chosen reasoner — decision 5's own preference order applied.
    Route(Profile),
    /// No profile permits it. Names the *first* offending axiom found
    /// (RL's own violations checked first, then EL, then QL) — decision
    /// 2's "actionable, not just a verdict" requirement.
    Refused {
        first_offending_axiom: Sid,
        reason: String,
    },
}

/// **Slice C.** RL preferred (materialises, and materialised facts are
/// explainable as chains — decision 5's own reasoning), then EL, then QL;
/// refused, naming the first offending axiom, if none apply. Pure: no I/O.
#[must_use]
pub fn route(detection: &Detection) -> RoutingDecision {
    if detection.rl.member {
        return RoutingDecision::Route(Profile::Rl);
    }
    if detection.el.member {
        return RoutingDecision::Route(Profile::El);
    }
    if detection.ql.member {
        return RoutingDecision::Route(Profile::Ql);
    }

    let first = detection
        .rl
        .violations
        .first()
        .or_else(|| detection.el.violations.first())
        .or_else(|| detection.ql.violations.first());
    match first {
        Some(violation) => RoutingDecision::Refused {
            first_offending_axiom: violation.subject.clone(),
            reason: violation.reason.clone(),
        },
        // Logically unreachable — every `member: false` implies at least
        // one violation, and the three checks above already returned if
        // any profile were a member — but a safe fallback beats a panic
        // for a case this type alone cannot prove exhaustive.
        None => RoutingDecision::Route(Profile::Rl),
    }
}

/// A caller explicitly proceeding past a [`RoutingDecision::Refused`] —
/// decision 3's override, and the "marked partial" half of Slice C's own
/// acceptance criterion. The result this carries is never mistakable for
/// a complete one: `ignored` names exactly what the chosen profile could
/// not account for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialRouting {
    pub profile: Profile,
    pub ignored: Vec<Violation>,
}

/// **Slice C.** Proceeds with `profile` anyway, carrying what that
/// profile's own check found. Pure: no I/O.
#[must_use]
pub fn override_refusal(detection: &Detection, profile: Profile) -> PartialRouting {
    let ignored = match profile {
        Profile::Rl => detection.rl.violations.clone(),
        Profile::El => detection.el.violations.clone(),
        Profile::Ql => detection.ql.violations.clone(),
    };
    PartialRouting { profile, ignored }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn dsc(id: &str) -> Sid {
        Sid::dsc(id)
    }

    mod slice_a_rl_and_el {
        use super::*;

        /// **The slice's own RED test.** `owl:inverseOf` is permitted in
        /// RL (`InverseObjectProperties` is in RL's own `ObjectPropertyAxiom`
        /// list) and forbidden in EL — the same real axiom, checked
        /// against two genuinely different grammars, proving the profiles
        /// are incomparable rather than nested.
        #[test]
        fn an_inverse_property_is_rl_member_but_not_el() {
            let rl = RlTbox::default();
            let el = graph_owl_reasoning_el::Tbox {
                subclass_of: Vec::new(),
                restriction_constructs: Vec::new(),
                inverse_properties: vec![dsc("hasParent")],
                watermark: 0,
            };

            assert!(detect_rl(&rl).member);
            assert!(!detect_el(&el).member);
        }

        /// A cardinality restriction is a (skolemized) restriction node,
        /// reached via `subClassOf` — not a predicate directly on the
        /// class — matching how the OWL/RDF mapping actually represents
        /// it.
        #[test]
        fn max_cardinality_one_is_rl_member_two_is_not() {
            let legal = RlTbox {
                subclass_of: vec![(dsc("Person"), dsc("restriction-1"))],
                restriction_cardinalities: vec![(
                    dsc("restriction-1"),
                    RlCardinalityShape::MaxCardinality(1),
                )],
                ..Default::default()
            };
            let illegal = RlTbox {
                subclass_of: vec![(dsc("Person"), dsc("restriction-1"))],
                restriction_cardinalities: vec![(
                    dsc("restriction-1"),
                    RlCardinalityShape::MaxCardinality(2),
                )],
                ..Default::default()
            };

            assert!(detect_rl(&legal).member, "{:?}", detect_rl(&legal));
            let result = detect_rl(&illegal);
            assert!(!result.member);
            assert_eq!(result.violations[0].subject, dsc("Person"));
        }

        #[test]
        fn other_cardinality_predicates_are_forbidden_regardless_of_value() {
            let tbox = RlTbox {
                subclass_of: vec![(dsc("Person"), dsc("restriction-1"))],
                restriction_cardinalities: vec![(dsc("restriction-1"), RlCardinalityShape::Other)],
                ..Default::default()
            };
            assert!(!detect_rl(&tbox).member);
        }

        /// The negative that makes the positive above about *this* class:
        /// a restriction nothing points to via `subClassOf` never excludes
        /// anything, however forbidden its own shape.
        #[test]
        fn a_cardinality_restriction_nothing_references_excludes_nothing() {
            let tbox = RlTbox {
                subclass_of: Vec::new(),
                restriction_cardinalities: vec![(
                    dsc("orphan-restriction"),
                    RlCardinalityShape::Other,
                )],
                ..Default::default()
            };
            assert!(detect_rl(&tbox).member, "{:?}", detect_rl(&tbox));
        }

        #[test]
        fn a_disjoint_union_is_outside_rl() {
            let tbox = RlTbox {
                disjoint_unions: vec![dsc("DataAsset")],
                ..Default::default()
            };
            assert!(!detect_rl(&tbox).member);
        }

        #[test]
        fn a_reflexive_property_is_outside_rl() {
            let tbox = RlTbox {
                reflexive_properties: vec![dsc("sameOrgAs")],
                ..Default::default()
            };
            assert!(!detect_rl(&tbox).member);
        }
    }

    mod slice_b_ql_and_aggregate {
        use super::*;

        /// **The slice's own RED test.** A construct outside every
        /// profile is named independently by each, not copied from one
        /// verdict.
        #[test]
        fn a_cardinality_two_restriction_is_outside_all_three_profiles() {
            let rl = RlTbox {
                subclass_of: vec![(dsc("Person"), dsc("restriction-1"))],
                restriction_cardinalities: vec![(
                    dsc("restriction-1"),
                    RlCardinalityShape::MaxCardinality(2),
                )],
                ..Default::default()
            };
            let el = graph_owl_reasoning_el::Tbox {
                subclass_of: vec![(dsc("Person"), dsc("restriction-1"))],
                restriction_constructs: vec![(
                    dsc("restriction-1"),
                    graph_owl_reasoning_el::ForbiddenElConstruct::Cardinality,
                )],
                inverse_properties: Vec::new(),
                watermark: 0,
            };
            let ql = graph_owl_reasoning_ql::Tbox {
                subclass_of: Vec::new(),
                forbidden: vec![graph_owl_reasoning_ql::RefusedAxiom {
                    class: dsc("Person"),
                    construct: graph_owl_reasoning_ql::ForbiddenConstruct::Cardinality,
                }],
            };

            let rl_result = detect_rl(&rl);
            let el_result = detect_el(&el);
            let ql_result = detect_ql(&ql);

            assert!(!rl_result.member, "{rl_result:?}");
            assert!(!el_result.member, "{el_result:?}");
            assert!(!ql_result.member, "{ql_result:?}");
            assert_eq!(rl_result.violations[0].subject, dsc("Person"));
            assert_eq!(el_result.violations[0].subject, dsc("Person"));
            assert_eq!(ql_result.violations[0].subject, dsc("Person"));
        }

        /// **Decision 1's own case**, shown rather than assumed possible:
        /// a `TBox` with nothing any of the three checks flags is a
        /// member of all three at once.
        #[test]
        fn a_tbox_with_no_forbidden_constructs_is_a_member_of_all_three_at_once() {
            let detection = Detection {
                rl: detect_rl(&RlTbox::default()),
                el: detect_el(&graph_owl_reasoning_el::Tbox::default()),
                ql: detect_ql(&graph_owl_reasoning_ql::Tbox {
                    subclass_of: Vec::new(),
                    forbidden: Vec::new(),
                }),
            };

            let profiles = detection.member_profiles();
            assert_eq!(profiles.len(), 3, "{profiles:?}");
        }
    }

    mod slice_c_routing {
        use super::*;

        fn membership(member: bool, violations: Vec<Violation>) -> ProfileMembership {
            ProfileMembership { member, violations }
        }

        fn violation(name: &str) -> Violation {
            Violation {
                subject: dsc(name),
                reason: format!("{name} reason"),
            }
        }

        /// **Decision 5's own preference**, proven rather than asserted.
        #[test]
        fn route_prefers_rl_when_both_rl_and_el_are_members() {
            let detection = Detection {
                rl: membership(true, Vec::new()),
                el: membership(true, Vec::new()),
                ql: membership(false, vec![violation("q")]),
            };
            assert_eq!(route(&detection), RoutingDecision::Route(Profile::Rl));
        }

        /// The negative that makes the preference test about *choosing*
        /// rather than *always returning RL*: when RL is not a member,
        /// EL is chosen instead.
        #[test]
        fn route_chooses_el_when_only_el_is_a_member() {
            let detection = Detection {
                rl: membership(false, vec![violation("r")]),
                el: membership(true, Vec::new()),
                ql: membership(false, vec![violation("q")]),
            };
            assert_eq!(route(&detection), RoutingDecision::Route(Profile::El));
        }

        #[test]
        fn route_chooses_ql_when_only_ql_is_a_member() {
            let detection = Detection {
                rl: membership(false, vec![violation("r")]),
                el: membership(false, vec![violation("e")]),
                ql: membership(true, Vec::new()),
            };
            assert_eq!(route(&detection), RoutingDecision::Route(Profile::Ql));
        }

        /// **The slice's own RED test.** Refused, naming the *first*
        /// offending axiom (RL's own violations checked first) rather
        /// than an arbitrary one.
        #[test]
        fn route_refuses_naming_the_first_offending_axiom_when_no_profile_applies() {
            let detection = Detection {
                rl: membership(false, vec![violation("RlOffender")]),
                el: membership(false, vec![violation("ElOffender")]),
                ql: membership(false, vec![violation("QlOffender")]),
            };

            let decision = route(&detection);

            assert_eq!(
                decision,
                RoutingDecision::Refused {
                    first_offending_axiom: dsc("RlOffender"),
                    reason: "RlOffender reason".to_string(),
                }
            );
        }

        #[test]
        fn override_refusal_carries_the_ignored_axioms() {
            let offender = violation("Offender");
            let detection = Detection {
                rl: membership(false, vec![offender.clone()]),
                el: membership(false, Vec::new()),
                ql: membership(false, Vec::new()),
            };

            let partial = override_refusal(&detection, Profile::Rl);

            assert_eq!(partial.profile, Profile::Rl);
            assert_eq!(partial.ignored, vec![offender]);
        }
    }

    mod slice_d_scale {
        use super::*;

        /// **Slice D's own RED test, measured rather than assumed** —
        /// this project's own "measured, not assumed" discipline, the
        /// same one Epic 98's Slice C used after its first (hung) attempt.
        #[test]
        fn detection_over_400k_axioms_completes_in_seconds() {
            let n: usize = 400_000;
            let subclass_of: Vec<(Sid, Sid)> = (0..n)
                .map(|i| (dsc(&format!("C{i}")), dsc(&format!("R{i}"))))
                .collect();
            let restriction_cardinalities: Vec<(Sid, RlCardinalityShape)> = (0..n)
                .map(|i| {
                    let shape = if i % 2 == 0 {
                        RlCardinalityShape::MaxCardinality(1)
                    } else {
                        RlCardinalityShape::MaxCardinality(2)
                    };
                    (dsc(&format!("R{i}")), shape)
                })
                .collect();
            let rl = RlTbox {
                subclass_of,
                restriction_cardinalities,
                ..Default::default()
            };

            let start = Instant::now();
            let result = detect_rl(&rl);
            let elapsed = start.elapsed();

            assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
            assert_eq!(
                result.violations.len(),
                n / 2,
                "{}",
                result.violations.len()
            );
        }
    }
}
