//! OWL 2 QL query rewriting over SPARQL algebra (pure, no I/O) — Epic 99.
//!
//! Answers an ontology-aware query by expanding it — `?x a :DataAsset`
//! becomes a `UNION` across `:DataAsset` and every known subclass — rather
//! than by deriving and materialising facts the way `graph_owl_reasoning`'s
//! RL engine does. The rewritten algebra is handed back to the same
//! `spareval` planner every other query already uses (`99-owl-ql-reasoning.md`
//! decision 1), so authorization and `as_of` scoping apply to it exactly as
//! they would to a query the caller typed by hand — that ordering is a
//! property of *where this crate is called from*, not of anything in here.
//!
//! `TBox` axioms (`rdfs:subClassOf` and the QL-forbidden constructs this crate
//! detects but does not rewrite through) are ordinary flakes, read by the
//! caller via the `TripleStore` port the same way `graph_owl_reasoning`
//! reads its own input — this crate takes the already-fetched edges as
//! plain data and performs no I/O of its own.

use graph_owl_core::flake::{Sid, namespace};
use spargebra::algebra::GraphPattern;
use spargebra::term::{NamedNode, NamedNodePattern, TermPattern, TriplePattern};
use std::collections::HashSet;

fn v(ns: u16, id: &str) -> Sid {
    Sid::new(ns, id)
}

fn rdf_type() -> Sid {
    v(namespace::RDF, "type")
}

/// A `Sid` as the `NamedNode` a rewritten pattern needs, or `None` when the
/// namespace has no fixed IRI (a runtime-registered predicate, for
/// instance) — skipped rather than a panic, because a customer's own
/// ontology choosing a class name outside the fixed prefix table is a real,
/// unremarkable case, not a bug.
fn to_named_node(sid: &Sid) -> Option<NamedNode> {
    sid.to_iri().and_then(|iri| NamedNode::new(iri).ok())
}

/// What a rewrite is allowed to expand into before it stops and reports
/// truncation rather than silently answering a narrower query — Slice D.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewriteBudget {
    /// How many `UNION` branches one rewritten triple pattern may grow.
    pub max_branches: usize,
    /// How many `rdfs:subClassOf` hops the closure walk may take.
    pub max_depth: usize,
}

impl Default for RewriteBudget {
    /// - **20** levels — the same "deeper than any hierarchy this project
    ///   models" reasoning `graph_owl_reasoning::Budget::default`'s
    ///   `max_iterations` already states for RL forward-chaining; a QL class
    ///   hierarchy is the same kind of structure and there is no reason to
    ///   expect it runs deeper.
    /// - **256** branches — a `UNION` wider than this is no longer a query a
    ///   relational planner treats as selective (this epic's whole point is
    ///   first-order rewritability to something a plain database answers
    ///   efficiently); past it the rewrite has stopped being the win it
    ///   exists for, so the limit is where it reports truncation instead of
    ///   growing further.
    fn default() -> Self {
        Self {
            max_branches: 256,
            max_depth: 20,
        }
    }
}

/// One `UNION` branch a rewrite added, and the direct edge that produced it
/// — `?explain=true`'s own unit, Slice B.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QlBranch {
    /// The class this branch matches.
    pub class: Sid,
    /// The class it was found to be a direct `rdfs:subClassOf` of. For a
    /// class reached through more than one hop, this is the *immediate*
    /// parent in the walk, not necessarily the class the original query
    /// named — the chain is reconstructable by following each branch's own
    /// `subclass_of` back to one that matches the query.
    pub subclass_of: Sid,
}

/// A `TBox` construct OWL 2 QL cannot express — Slice C. Named after
/// `graph_owl_reasoning::RuleName`'s own vocabulary, since these are the
/// identical axioms, checked for presence here rather than executed as
/// rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForbiddenConstruct {
    PropertyChain,
    TransitiveProperty,
    FunctionalProperty,
    InverseFunctionalProperty,
    HasKey,
}

/// A class the query touched that carries a construct QL cannot rewrite
/// through — reported, never silently dropped (Slice C's own acceptance
/// criterion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedAxiom {
    pub class: Sid,
    pub construct: ForbiddenConstruct,
}

/// Everything one call to [`rewrite`] produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RewriteOutcome {
    /// The algebra to hand to the same planner every other query uses.
    /// Identical to the input when nothing was rewritten.
    pub pattern: GraphPattern,
    /// Empty when nothing was rewritten — silence is the signal that there
    /// is nothing to explain, not an empty list a caller has to interpret
    /// (Slice B).
    pub branches: Vec<QlBranch>,
    pub refused_axioms: Vec<RefusedAxiom>,
    /// The walk stopped against [`RewriteBudget`] before it finished —
    /// never set without also having kept only the branches actually
    /// found, so a truncated outcome's `pattern` is never presented as
    /// covering more than it does.
    pub truncated: bool,
}

/// One class's declared `rdfs:subClassOf` axioms and whatever QL-forbidden
/// constructs were found on it — the pre-fetched `TBox` slice `rewrite` reads.
/// A plain data input, not a store: see this crate's own doc comment.
#[derive(Debug, Clone, Default)]
pub struct Tbox {
    /// `(subclass, superclass)` pairs — every `rdfs:subClassOf` triple
    /// relevant to the query, already read by the caller.
    pub subclass_of: Vec<(Sid, Sid)>,
    /// A class or property found to carry a QL-forbidden construct.
    pub forbidden: Vec<RefusedAxiom>,
}

/// Rewrites `pattern` against `tbox`, expanding every `rdf:type` triple
/// pattern naming a class with known subclasses into a `UNION` across the
/// class and its transitive subclasses, bounded by `budget`. Every other
/// algebra node is walked and rebuilt unchanged around whatever its
/// children rewrote to.
///
/// Pure: no I/O, no clock, deterministic in the order axioms were supplied.
#[must_use]
pub fn rewrite(pattern: &GraphPattern, tbox: &Tbox, budget: &RewriteBudget) -> RewriteOutcome {
    let mut acc = Accumulator {
        branches: Vec::new(),
        refused_axioms: Vec::new(),
        truncated: false,
    };
    let rewritten = walk(pattern, tbox, budget, &mut acc);
    RewriteOutcome {
        pattern: rewritten,
        branches: acc.branches,
        refused_axioms: acc.refused_axioms,
        truncated: acc.truncated,
    }
}

struct Accumulator {
    branches: Vec<QlBranch>,
    refused_axioms: Vec<RefusedAxiom>,
    truncated: bool,
}

fn walk(
    pattern: &GraphPattern,
    tbox: &Tbox,
    budget: &RewriteBudget,
    acc: &mut Accumulator,
) -> GraphPattern {
    match pattern {
        GraphPattern::Bgp { patterns } => rewrite_bgp(patterns, tbox, budget, acc),
        GraphPattern::Path { .. } | GraphPattern::Values { .. } => pattern.clone(),
        GraphPattern::Join { left, right } => GraphPattern::Join {
            left: Box::new(walk(left, tbox, budget, acc)),
            right: Box::new(walk(right, tbox, budget, acc)),
        },
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => GraphPattern::LeftJoin {
            left: Box::new(walk(left, tbox, budget, acc)),
            right: Box::new(walk(right, tbox, budget, acc)),
            expression: expression.clone(),
        },
        GraphPattern::Filter { expr, inner } => GraphPattern::Filter {
            expr: expr.clone(),
            inner: Box::new(walk(inner, tbox, budget, acc)),
        },
        GraphPattern::Union { left, right } => GraphPattern::Union {
            left: Box::new(walk(left, tbox, budget, acc)),
            right: Box::new(walk(right, tbox, budget, acc)),
        },
        GraphPattern::Graph { name, inner } => GraphPattern::Graph {
            name: name.clone(),
            inner: Box::new(walk(inner, tbox, budget, acc)),
        },
        GraphPattern::Extend {
            inner,
            variable,
            expression,
        } => GraphPattern::Extend {
            inner: Box::new(walk(inner, tbox, budget, acc)),
            variable: variable.clone(),
            expression: expression.clone(),
        },
        GraphPattern::Minus { left, right } => GraphPattern::Minus {
            left: Box::new(walk(left, tbox, budget, acc)),
            right: Box::new(walk(right, tbox, budget, acc)),
        },
        GraphPattern::OrderBy { inner, expression } => GraphPattern::OrderBy {
            inner: Box::new(walk(inner, tbox, budget, acc)),
            expression: expression.clone(),
        },
        GraphPattern::Project { inner, variables } => GraphPattern::Project {
            inner: Box::new(walk(inner, tbox, budget, acc)),
            variables: variables.clone(),
        },
        GraphPattern::Distinct { inner } => GraphPattern::Distinct {
            inner: Box::new(walk(inner, tbox, budget, acc)),
        },
        GraphPattern::Reduced { inner } => GraphPattern::Reduced {
            inner: Box::new(walk(inner, tbox, budget, acc)),
        },
        GraphPattern::Slice {
            inner,
            start,
            length,
        } => GraphPattern::Slice {
            inner: Box::new(walk(inner, tbox, budget, acc)),
            start: *start,
            length: *length,
        },
        GraphPattern::Group {
            inner,
            variables,
            aggregates,
        } => GraphPattern::Group {
            inner: Box::new(walk(inner, tbox, budget, acc)),
            variables: variables.clone(),
            aggregates: aggregates.clone(),
        },
        GraphPattern::Service {
            name,
            inner,
            silent,
        } => GraphPattern::Service {
            name: name.clone(),
            // Never expanded through a federated boundary — the remote
            // endpoint has its own answer to what its classes mean, and
            // rewriting on its behalf would be a claim this crate has no
            // basis for.
            inner: inner.clone(),
            silent: *silent,
        },
    }
}

/// Every rewritable `rdf:type` triple in one BGP becomes a `UNION`,
/// `Join`-ed back against whatever triples in the same BGP were not
/// rewritten (a BGP is a conjunction, so splitting one triple out and
/// rejoining it is the algebra-preserving move — not a special case).
fn rewrite_bgp(
    patterns: &[TriplePattern],
    tbox: &Tbox,
    budget: &RewriteBudget,
    acc: &mut Accumulator,
) -> GraphPattern {
    let rdf_type_iri = to_named_node(&rdf_type());

    let mut rewritable: Vec<(usize, Vec<QlBranch>)> = Vec::new();
    for (index, triple) in patterns.iter().enumerate() {
        let NamedNodePattern::NamedNode(predicate) = &triple.predicate else {
            continue;
        };
        if Some(predicate) != rdf_type_iri.as_ref() {
            continue;
        }
        let TermPattern::NamedNode(object) = &triple.object else {
            continue;
        };
        let Some(class) = Sid::from_iri(object.as_str()) else {
            continue;
        };
        report_forbidden(&class, tbox, acc);
        let (branches, truncated) = subclasses_of(&class, &tbox.subclass_of, budget);
        if truncated {
            acc.truncated = true;
        }
        if !branches.is_empty() {
            rewritable.push((index, branches));
        }
    }

    if rewritable.is_empty() {
        return GraphPattern::Bgp {
            patterns: patterns.to_vec(),
        };
    }

    let rewritten_indices: HashSet<usize> = rewritable.iter().map(|(index, _)| *index).collect();
    let remaining: Vec<TriplePattern> = patterns
        .iter()
        .enumerate()
        .filter(|(index, _)| !rewritten_indices.contains(index))
        .map(|(_, triple)| triple.clone())
        .collect();

    let mut result = if remaining.is_empty() {
        None
    } else {
        Some(GraphPattern::Bgp {
            patterns: remaining,
        })
    };

    for (index, branches) in rewritable {
        let original = patterns[index].clone();
        let mut expansion = GraphPattern::Bgp {
            patterns: vec![original.clone()],
        };
        for branch in branches {
            let Some(node) = to_named_node(&branch.class) else {
                continue;
            };
            let branch_triple = TriplePattern {
                subject: original.subject.clone(),
                predicate: original.predicate.clone(),
                object: TermPattern::NamedNode(node),
            };
            acc.branches.push(branch);
            expansion = GraphPattern::Union {
                left: Box::new(expansion),
                right: Box::new(GraphPattern::Bgp {
                    patterns: vec![branch_triple],
                }),
            };
        }
        result = Some(match result {
            None => expansion,
            Some(existing) => GraphPattern::Join {
                left: Box::new(existing),
                right: Box::new(expansion),
            },
        });
    }

    result.unwrap_or(GraphPattern::Bgp {
        patterns: Vec::new(),
    })
}

fn report_forbidden(class: &Sid, tbox: &Tbox, acc: &mut Accumulator) {
    for refusal in &tbox.forbidden {
        if &refusal.class == class {
            acc.refused_axioms.push(refusal.clone());
        }
    }
}

/// Breadth-first closure of `target`'s subclasses, stopping at `budget` and
/// reporting whether it had to.
fn subclasses_of(
    target: &Sid,
    edges: &[(Sid, Sid)],
    budget: &RewriteBudget,
) -> (Vec<QlBranch>, bool) {
    let mut branches: Vec<QlBranch> = Vec::new();
    let mut seen: HashSet<Sid> = HashSet::from([target.clone()]);
    let mut frontier = vec![target.clone()];
    let mut truncated = false;

    for _ in 0..budget.max_depth {
        if frontier.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for parent in &frontier {
            for (child, super_class) in edges {
                if super_class != parent || !seen.insert(child.clone()) {
                    continue;
                }
                if branches.len() >= budget.max_branches {
                    truncated = true;
                    continue;
                }
                branches.push(QlBranch {
                    class: child.clone(),
                    subclass_of: parent.clone(),
                });
                next.push(child.clone());
            }
        }
        frontier = next;
    }
    if !frontier.is_empty() {
        truncated = true;
    }
    (branches, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsc(id: &str) -> Sid {
        Sid::dsc(id)
    }

    fn parse(query: &str) -> GraphPattern {
        let spargebra::Query::Select { pattern, .. } = spargebra::SparqlParser::new()
            .parse_query(query)
            .expect("valid query")
        else {
            panic!("expected a SELECT query");
        };
        pattern
    }

    fn budget() -> RewriteBudget {
        RewriteBudget::default()
    }

    /// **The slice's own RED test.** Instances are typed only as `Table` or
    /// `View`, never directly as `DataAsset` — a rewrite that silently left
    /// the pattern unchanged would still fail here, because there would be
    /// nothing for the unrewritten query to match.
    #[test]
    fn a_type_pattern_naming_a_class_with_subclasses_expands_into_a_union() {
        let pattern =
            parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#DataAsset> }");
        let tbox = Tbox {
            subclass_of: vec![
                (dsc("Table"), dsc("DataAsset")),
                (dsc("View"), dsc("DataAsset")),
            ],
            forbidden: Vec::new(),
        };

        let outcome = rewrite(&pattern, &tbox, &budget());

        assert_ne!(outcome.pattern, pattern, "the pattern must actually change");
        let classes: HashSet<Sid> = outcome.branches.iter().map(|b| b.class.clone()).collect();
        assert_eq!(
            classes,
            HashSet::from([dsc("Table"), dsc("View")]),
            "{classes:?}"
        );
        assert!(!outcome.truncated);
    }

    /// The negative that makes the positive above about *subclasses*
    /// specifically: a class nothing is declared under rewrites to itself,
    /// so the expansion is conditional rather than unconditional.
    #[test]
    fn a_class_with_no_known_subclasses_rewrites_to_itself() {
        let pattern =
            parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#Standalone> }");
        let tbox = Tbox::default();

        let outcome = rewrite(&pattern, &tbox, &budget());

        assert_eq!(outcome.pattern, pattern);
        assert!(outcome.branches.is_empty());
    }

    /// A `Bgp` is a conjunction, not just its rewritable triple — the second
    /// pattern must survive the split, joined back against the union rather
    /// than dropped.
    #[test]
    fn a_bgp_with_an_unrelated_triple_keeps_it_alongside_the_union() {
        let pattern = parse(
            "SELECT ?x ?n WHERE { ?x a <https://graph-owl.dev/ns/catalog#DataAsset> . \
             ?x <https://graph-owl.dev/ns/catalog#name> ?n }",
        );
        let tbox = Tbox {
            subclass_of: vec![(dsc("Table"), dsc("DataAsset"))],
            forbidden: Vec::new(),
        };

        let outcome = rewrite(&pattern, &tbox, &budget());

        let rendered = outcome.pattern.to_string();
        assert!(rendered.contains("catalog#name"), "{rendered}");
    }

    /// The closure walks more than one hop — a grandchild subclass is found,
    /// not just direct children.
    #[test]
    fn subclasses_deeper_than_one_hop_are_still_found() {
        let pattern =
            parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#DataAsset> }");
        let tbox = Tbox {
            subclass_of: vec![
                (dsc("Table"), dsc("DataAsset")),
                (dsc("PartitionedTable"), dsc("Table")),
            ],
            forbidden: Vec::new(),
        };

        let outcome = rewrite(&pattern, &tbox, &budget());

        let classes: HashSet<Sid> = outcome.branches.iter().map(|b| b.class.clone()).collect();
        assert!(classes.contains(&dsc("PartitionedTable")), "{classes:?}");
    }

    /// **Slice C's own RED test.** A construct QL cannot express is named,
    /// not silently absorbed into a query that looks complete.
    #[test]
    fn a_construct_ql_cannot_express_is_reported_not_silently_dropped() {
        let pattern = parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#Person> }");
        let tbox = Tbox {
            subclass_of: Vec::new(),
            forbidden: vec![RefusedAxiom {
                class: dsc("Person"),
                construct: ForbiddenConstruct::HasKey,
            }],
        };

        let outcome = rewrite(&pattern, &tbox, &budget());

        assert_eq!(outcome.refused_axioms.len(), 1);
        assert_eq!(
            outcome.refused_axioms[0].construct,
            ForbiddenConstruct::HasKey
        );
    }

    /// The negative that makes the positive above about *this* class: an
    /// axiom refusal elsewhere in the ontology must not over-refuse an
    /// unrelated class in the same query.
    #[test]
    fn a_forbidden_axiom_on_an_unrelated_class_does_not_refuse_this_one() {
        let pattern =
            parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#DataAsset> }");
        let tbox = Tbox {
            subclass_of: vec![(dsc("Table"), dsc("DataAsset"))],
            forbidden: vec![RefusedAxiom {
                class: dsc("Person"),
                construct: ForbiddenConstruct::HasKey,
            }],
        };

        let outcome = rewrite(&pattern, &tbox, &budget());

        assert!(
            outcome.refused_axioms.is_empty(),
            "{:?}",
            outcome.refused_axioms
        );
    }

    /// **Slice D's own RED test, depth half.** A chain deeper than the
    /// budget stops rather than silently completing a narrower walk and
    /// calling it whole.
    #[test]
    fn a_hierarchy_deeper_than_the_budget_truncates_rather_than_silently_narrowing() {
        let pattern = parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#Root> }");
        let tbox = Tbox {
            subclass_of: vec![
                (dsc("L1"), dsc("Root")),
                (dsc("L2"), dsc("L1")),
                (dsc("L3"), dsc("L2")),
            ],
            forbidden: Vec::new(),
        };
        let tight = RewriteBudget {
            max_branches: 256,
            max_depth: 1,
        };

        let outcome = rewrite(&pattern, &tbox, &tight);

        assert!(outcome.truncated);
        let classes: HashSet<Sid> = outcome.branches.iter().map(|b| b.class.clone()).collect();
        assert!(classes.contains(&dsc("L1")), "{classes:?}");
        assert!(
            !classes.contains(&dsc("L3")),
            "a depth-truncated walk must not silently reach the excluded level: {classes:?}"
        );
    }

    /// **Slice D's own RED test, branch-count half.** Depth and branch
    /// count are different dimensions — a wide, shallow hierarchy must
    /// truncate on its own terms too.
    #[test]
    fn a_branch_count_over_budget_also_truncates() {
        let pattern = parse("SELECT ?x WHERE { ?x a <https://graph-owl.dev/ns/catalog#Root> }");
        let tbox = Tbox {
            subclass_of: vec![
                (dsc("A"), dsc("Root")),
                (dsc("B"), dsc("Root")),
                (dsc("C"), dsc("Root")),
            ],
            forbidden: Vec::new(),
        };
        let tight = RewriteBudget {
            max_branches: 1,
            max_depth: 20,
        };

        let outcome = rewrite(&pattern, &tbox, &tight);

        assert!(outcome.truncated);
        assert_eq!(outcome.branches.len(), 1, "{:?}", outcome.branches);
    }
}
