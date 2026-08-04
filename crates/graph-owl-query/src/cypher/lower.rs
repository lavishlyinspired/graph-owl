//! Cypher → SPARQL algebra — Epic 7b Slices B and C.
//!
//! **One engine, two front ends.** Lowering produces the same
//! [`spargebra::algebra::GraphPattern`] the SPARQL front end produces, so the
//! planner, the evaluator and — the part that matters — the *authorization
//! path* are shared. Two evaluators would mean two authorization paths, and the
//! looser one would be the leak.
//!
//! # A relationship is three patterns, not one
//!
//! Epic 4 reifies every relationship, so `(a)-[r:FEEDS]->(b)` is not one triple.
//! It is three facts about `?r`:
//!
//! ```text
//! ?r dsc:relType     dsc:FEEDS
//! ?r dsc:fromEntity  ?a
//! ?r dsc:toEntity    ?b
//! ```
//!
//! That is what makes `[r:FEEDS {confidence: 0.9}]` expressible at all — an edge
//! property is simply another fact about `?r`. Lowering an edge to a single
//! predicate would be shorter, would look right, and would make edge properties
//! impossible; Epic 7c's whole argument rests on this encoding.
//!
//! # Relationship isomorphism (Slice C)
//!
//! **Cypher and SPARQL disagree about whether a pattern may match itself, and
//! the disagreement is silent.** Within one `MATCH`, Cypher forbids two
//! relationship variables binding the same relationship; SPARQL's basic graph
//! pattern is homomorphic and permits it. So `MATCH (a)-[r1]->(b)-[r2]->(c)`
//! over a self-loop returns a row in SPARQL and none in Cypher.
//!
//! Lowering therefore **injects an explicit inequality** over the relationship
//! variables of each `MATCH`. It is injected into the algebra rather than
//! enforced during execution so that it is visible in the plan — a semantic
//! difference hidden inside an operator is one nobody can review.
//!
//! Across *separate* `MATCH` clauses reuse is permitted, which is Cypher's
//! actual rule and not an approximation of it.
//!
//! # Aggregates are `GraphPattern::Group`, not a second implementation (Slice F)
//!
//! `count`, `sum`, `avg`, `min` and `max` lower onto
//! [`spargebra::algebra::AggregateExpression`] — the same operator
//! `spareval` already evaluates for SPARQL's own `GROUP BY`. Grouping is
//! **implicit**, per Cypher: every non-aggregated `RETURN`/`WITH` item becomes
//! a grouping key, with no separate `GROUP BY` clause to write.
//!
//! **`collect(...)` is refused, not approximated.** SPARQL's nearest operator,
//! `GROUP_CONCAT`, folds values into one delimited *string*; Cypher's
//! `collect()` produces a genuine list-typed value. Lowering one to the other
//! would silently hand back a string where the caller asked for a list —
//! exactly the kind of approximation this module refuses everywhere else
//! (an undirected relationship, a compound label).
//!
//! # A variable-length hop is not algebra (Slice D)
//!
//! `MATCH (a)-[:FEEDS*1..3]->(b)` cannot lower to a triple pattern the way a
//! fixed-length relationship does: **expressing it as SPARQL's own property
//! path would force a full scan** (`pushdown::scans_for` already refuses to
//! bound a property path, for the same reason Epic 7a's traversal engine
//! exists — a bounded walk pushed into one Postgres statement beats
//! materialising every candidate edge and joining it N times over).
//!
//! So [`lower`] does not resolve a variable-length hop at all. It **extracts**
//! one as a [`VariableLengthHop`] and returns it alongside the pattern for the
//! rest of the query — pure and synchronous, like everything else here. The
//! *caller* (`graph_owl_api::Catalog::cypher`, which already holds an async
//! traversal port) walks the graph, filters the reached nodes through the same
//! authorization predicate the rest of the query is scoped by, and joins the
//! result back in as a `Values` block — the identical mechanism `UNWIND`
//! already uses to bind a table graph-owl computed outside the algebra.

use oxrdf::{Literal, NamedNode, Variable};
use spargebra::algebra::{
    AggregateExpression, AggregateFunction, Expression, GraphPattern, OrderExpression,
};
use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use decypher::ast::clause::{Match, Return, SortDirection, Unwind, With};
use decypher::ast::expr::Expression as CypherExpression;
use decypher::ast::pattern::{
    LabelExpression, NodePattern, Pattern, PatternElement, Properties, RelationshipDirection,
    RelationshipPattern,
};
use decypher::ast::query::{
    QueryBody, ReadingClause, SinglePartBody, SinglePartQuery, SingleQueryKind,
};

use crate::cypher::vocabulary;

/// A variable-length relationship pattern, extracted rather than lowered.
///
/// **`start` is always the topological tail and `end` the head — whichever
/// side of the arrow they were written on.** `(a)-[*]->(b)` and `(b)<-[*]-(a)`
/// describe the same walk, and normalising here means the caller only ever
/// resolves one shape: walk *outgoing* from `start`, discover `end`. Resolving
/// a hop needs `start` to already be bound to a small, known set of nodes by
/// the rest of the pattern — a label or a property match — because the
/// traversal engine walks from a seed, not from "anything". A pattern with
/// neither endpoint constrained is refused at resolution, not lowering, since
/// lowering cannot know what the rest of the query will bind.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableLengthHop {
    pub start: Variable,
    pub end: Variable,
    /// `None` follows every relationship type. Cypher's `TYPE1|TYPE2` union
    /// form is refused, the same way a fixed-length relationship's compound
    /// label already is — see [`single_relationship_type`].
    pub relationship_type: Option<String>,
    /// Inclusive. `*` alone means 1, not 0 — a variable-length pattern with no
    /// lower bound still requires at least one relationship, per the
    /// openCypher grammar; zero hops would mean `start` and `end` are the same
    /// node, which needs an explicit `*0..` this engine does not attempt (the
    /// traversal engine's own `neighbours` never reports the seed itself).
    pub min_hops: usize,
    /// Inclusive, capped at [`UNBOUNDED_HOP_LIMIT`] however large the query
    /// asks for.
    pub max_hops: usize,
}

/// The ceiling for `*` with no upper bound, or `*N..` with no upper bound.
///
/// **Reused, not invented.** `graph-owl-server`'s `/assets/{id}/graph` handler
/// already caps a client-supplied hop count at 6, for the reason its own
/// comment states: "A client asking for 50 hops on a real estate is asking
/// for the whole graph, and the bound exists to protect the server rather
/// than to be polite to the client." A second, independent number here would
/// be a second guess at the same question.
const UNBOUNDED_HOP_LIMIT: usize = 6;

/// Why a query in the subset could not be lowered.
///
/// **Lowering fails here, not at execution.** A construct that parsed, passed
/// the subset gate and then produced a plan the evaluator chokes on would
/// surface as an engine error at query time — which reads as "the database is
/// broken" rather than "this query cannot be answered".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoweringError {
    #[error("`{0}` is in the subset's grammar but has no lowering yet")]
    Unlowerable(&'static str),
    #[error("a pattern must bind at least one variable to be answerable")]
    NothingBound,
    #[error("`{0}` is not a value this engine can compare against")]
    UnsupportedLiteral(String),
}

/// Lower one admitted query to the shared algebra.
///
/// **Takes the source text as well as the parsed query**, solely to recover
/// an aggregate argument `decypher` drops from its own typed AST — see
/// [`recover_dropped_argument`]. Nothing else here reads it; the AST is the
/// lowering input everywhere else, exactly as the module docs describe.
///
/// **Returns every [`VariableLengthHop`] the query contains, alongside the
/// pattern for the rest of it.** An empty list is the common case and costs
/// nothing to check; a non-empty one is the caller's signal to resolve each
/// hop over the traversal engine before evaluating the pattern — see the
/// module docs.
///
/// # Errors
///
/// [`LoweringError`] naming the construct that has no lowering.
pub fn lower(
    query: &decypher::ast::query::Query,
    source: &str,
) -> Result<(GraphPattern, Vec<VariableLengthHop>), LoweringError> {
    let body = query
        .statements
        .first()
        .ok_or(LoweringError::Unlowerable("an empty query"))?;
    let mut hops = Vec::new();
    let pattern = lower_body(body, source, &mut hops)?;
    Ok((pattern, hops))
}

fn lower_body(
    body: &QueryBody,
    source: &str,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<GraphPattern, LoweringError> {
    match body {
        QueryBody::SingleQuery(single) => lower_single(&single.kind, source, hops),
        // **Everything else was already refused by the subset gate**, and this
        // arm is the second line rather than a fallback that quietly copes.
        //
        // There was a `QueryBody::Regular` arm here that lowered the first
        // branch of a `UNION` and dropped the rest. It was unreachable —
        // `decypher` produces `Regular` only for `UNION`, which the gate
        // refuses — and mutation testing showed it: deleting it changed
        // nothing. Unreachable code in a lowering path is worse than absent
        // code, because it reads as "UNION is handled" to the next person.
        _ => Err(LoweringError::Unlowerable("this query shape")),
    }
}

fn lower_single(
    kind: &SingleQueryKind,
    source: &str,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<GraphPattern, LoweringError> {
    match kind {
        SingleQueryKind::SinglePart(part) => lower_part(part, source, hops),
        SingleQueryKind::MultiPart(multi) => {
            // **`WITH` is a pipeline boundary.** Each part's reading clauses
            // join onto what came before, and the `WITH` projects — which is
            // what scopes variables to the next part, exactly as Cypher says.
            let mut pattern: Option<GraphPattern> = None;
            for segment in &multi.parts {
                let reading = lower_reading(&segment.reading_clauses, hops)?;
                pattern = Some(match pattern {
                    None => reading,
                    Some(left) => GraphPattern::Join {
                        left: Box::new(left),
                        right: Box::new(reading),
                    },
                });
                pattern = Some(apply_with(
                    pattern.take().expect("just assigned"),
                    &segment.with,
                    source,
                )?);
            }
            let tail = lower_part(&multi.final_part, source, hops)?;
            Ok(match pattern {
                None => tail,
                Some(left) => GraphPattern::Join {
                    left: Box::new(left),
                    right: Box::new(tail),
                },
            })
        }
    }
}

fn lower_part(
    part: &SinglePartQuery,
    source: &str,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<GraphPattern, LoweringError> {
    let reading = lower_reading(&part.reading_clauses, hops)?;
    match &part.body {
        SinglePartBody::Return(returning) => apply_return(reading, returning, source),
        // The subset gate refuses these; reaching here is a gate/lowering
        // disagreement rather than a user error.
        SinglePartBody::Updating { .. } => Err(LoweringError::Unlowerable("a write clause")),
        SinglePartBody::Finish(_) => Err(LoweringError::Unlowerable("FINISH")),
    }
}

/// The reading clauses of one query part, joined left to right.
fn lower_reading(
    clauses: &[ReadingClause],
    hops: &mut Vec<VariableLengthHop>,
) -> Result<GraphPattern, LoweringError> {
    let mut pattern: Option<GraphPattern> = None;

    for clause in clauses {
        let (next, optional) = match clause {
            ReadingClause::Match(matching) => (lower_match(matching, hops)?, matching.optional),
            ReadingClause::Unwind(unwind) => (lower_unwind(unwind)?, false),
            _ => return Err(LoweringError::Unlowerable("this reading clause")),
        };
        pattern = Some(match (pattern, optional) {
            (None, _) => next,
            // **`OPTIONAL MATCH` is a left join**, which is the whole of its
            // semantics: rows on the left survive with unbound right-hand
            // variables rather than being dropped.
            (Some(left), true) => GraphPattern::LeftJoin {
                left: Box::new(left),
                right: Box::new(next),
                expression: None,
            },
            (Some(left), false) => GraphPattern::Join {
                left: Box::new(left),
                right: Box::new(next),
            },
        });
    }

    // A query with no reading clause — `UNWIND` only, or `RETURN 1` — still has
    // a pattern: the one-row identity, which is what every SPARQL algebra calls
    // the empty BGP.
    Ok(pattern.unwrap_or(GraphPattern::Bgp {
        patterns: Vec::new(),
    }))
}

/// One `MATCH`, with its isomorphism constraint.
fn lower_match(
    matching: &Match,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<GraphPattern, LoweringError> {
    let mut patterns = Vec::new();
    let mut relationship_variables = Vec::new();
    lower_pattern(
        &matching.pattern,
        &mut patterns,
        &mut relationship_variables,
        hops,
    )?;

    let mut graph = GraphPattern::Bgp { patterns };

    // **Slice C.** Relationship variables within one `MATCH` may not coincide.
    // Injected here rather than enforced in execution so it is visible in the
    // plan — see the module docs.
    for constraint in isomorphism_constraints(&relationship_variables) {
        graph = GraphPattern::Filter {
            expr: constraint,
            inner: Box::new(graph),
        };
    }

    if let Some(where_clause) = &matching.where_clause {
        let mut properties = Vec::new();
        collect_properties(where_clause, &mut properties);
        graph = GraphPattern::Filter {
            expr: lower_expression(where_clause)?,
            inner: Box::new(with_property_bindings(graph, &properties)?),
        };
    }
    Ok(graph)
}

/// Pairwise inequalities over the relationship variables of one `MATCH`.
///
/// **Pairwise rather than a single n-ary distinctness**, because the algebra has
/// no n-ary distinct and because each pair is separately readable in the plan —
/// which is the point of injecting it into the algebra at all.
fn isomorphism_constraints(variables: &[Variable]) -> Vec<Expression> {
    let mut constraints = Vec::new();
    for (index, left) in variables.iter().enumerate() {
        for right in &variables[index + 1..] {
            constraints.push(Expression::Not(Box::new(Expression::SameTerm(
                Box::new(Expression::Variable(left.clone())),
                Box::new(Expression::Variable(right.clone())),
            ))));
        }
    }
    constraints
}

fn lower_pattern(
    pattern: &Pattern,
    patterns: &mut Vec<TriplePattern>,
    relationships: &mut Vec<Variable>,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<(), LoweringError> {
    for part in &pattern.parts {
        match &part.anonymous.element {
            PatternElement::Path { start, chains } => {
                let mut left = lower_node(start, patterns)?;
                for chain in chains {
                    let right = lower_node(&chain.node, patterns)?;
                    lower_relationship(
                        &chain.relationship,
                        &left,
                        &right,
                        patterns,
                        relationships,
                        hops,
                    )?;
                    left = right;
                }
            }
            _ => return Err(LoweringError::Unlowerable("this pattern element")),
        }
    }
    Ok(())
}

/// A node pattern: its variable, its labels, and its inline properties.
fn lower_node(
    node: &NodePattern,
    patterns: &mut Vec<TriplePattern>,
) -> Result<TermPattern, LoweringError> {
    let subject = TermPattern::Variable(node_variable(node));

    for label in &node.labels {
        patterns.push(TriplePattern {
            subject: subject.clone(),
            predicate: NamedNodePattern::NamedNode(vocabulary::type_predicate()),
            object: TermPattern::NamedNode(label_node(label)?),
        });
    }
    if let Some(Properties::Map(map)) = &node.properties {
        for (key, value) in &map.entries {
            patterns.push(TriplePattern {
                subject: subject.clone(),
                predicate: NamedNodePattern::NamedNode(vocabulary::property(&key.name.name)),
                object: TermPattern::Literal(literal_of(value)?),
            });
        }
    }
    Ok(subject)
}

/// **A relationship is three patterns.** See the module docs.
fn lower_relationship(
    relationship: &RelationshipPattern,
    left: &TermPattern,
    right: &TermPattern,
    patterns: &mut Vec<TriplePattern>,
    relationships: &mut Vec<Variable>,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<(), LoweringError> {
    // Direction decides which endpoint is `from` and which is `to`. An
    // undirected pattern is not expressible as one BGP — it is a union — and is
    // reported rather than silently lowered as left-to-right, which would
    // return half the answer with no indication. Variable-length shares this
    // refusal: `neighbours` walks one topological direction from a seed, and
    // "either direction" is not a direction.
    let (from, to) = match relationship.direction {
        RelationshipDirection::Right => (left, right),
        RelationshipDirection::Left => (right, left),
        RelationshipDirection::Undirected | RelationshipDirection::Both => {
            return Err(LoweringError::Unlowerable(
                "an undirected relationship pattern",
            ));
        }
    };

    if let Some(detail) = &relationship.detail
        && let Some(range) = &detail.range
    {
        return lower_variable_length(detail, range, from, to, patterns, hops);
    }

    let variable = relationship_variable(relationship, relationships.len());
    relationships.push(variable.clone());
    let subject = TermPattern::Variable(variable);

    if let Some(detail) = &relationship.detail {
        if let Some(types) = &detail.types {
            patterns.push(TriplePattern {
                subject: subject.clone(),
                predicate: NamedNodePattern::NamedNode(vocabulary::rel_type_predicate()),
                object: TermPattern::NamedNode(label_node(types)?),
            });
        }
        // **Edge properties, and the reason reification pays off.** Each is
        // simply another fact about the relationship node.
        if let Some(Properties::Map(map)) = &detail.properties {
            for (key, value) in &map.entries {
                patterns.push(TriplePattern {
                    subject: subject.clone(),
                    predicate: NamedNodePattern::NamedNode(vocabulary::property(&key.name.name)),
                    object: TermPattern::Literal(literal_of(value)?),
                });
            }
        }
    }

    patterns.push(TriplePattern {
        subject: subject.clone(),
        predicate: NamedNodePattern::NamedNode(vocabulary::from_entity_predicate()),
        object: from.clone(),
    });
    patterns.push(TriplePattern {
        subject,
        predicate: NamedNodePattern::NamedNode(vocabulary::to_entity_predicate()),
        object: to.clone(),
    });
    Ok(())
}

/// Extract a `*min..max` relationship into a [`VariableLengthHop`] instead of
/// lowering it — see the module docs for why this cannot become algebra.
///
/// **Still contributes one triple pattern: a sentinel, not a real one.** If
/// `start`/`end` appeared nowhere in the BGP, `RETURN`'s own projection would
/// silently drop whichever of the two it did not name — the sentinel is what
/// keeps both threaded through every later layer (`WHERE`, `RETURN`,
/// aggregation) exactly as an ordinary relationship's triples would, so
/// nothing downstream needs to know a hop is unresolved. It matches no real
/// data (see [`variable_length_hop_marker`]) and is stripped or substituted
/// by the caller before the pattern is ever evaluated — see
/// [`strip_variable_length_hops`] and [`substitute_variable_length_hop`].
fn lower_variable_length(
    detail: &decypher::ast::pattern::RelationshipDetail,
    range: &decypher::ast::pattern::RangeLiteral,
    from: &TermPattern,
    to: &TermPattern,
    patterns: &mut Vec<TriplePattern>,
    hops: &mut Vec<VariableLengthHop>,
) -> Result<(), LoweringError> {
    if detail.variable.is_some() {
        // `[r*]` asks for the path's own edges, which `neighbours` cannot
        // supply — it reports reached nodes and their distance, not the route
        // taken. Getting the actual edges needs `all_paths`, a materially
        // more expensive call this slice does not make.
        return Err(LoweringError::Unlowerable(
            "a variable-length relationship pattern binding the relationship list",
        ));
    }
    if detail.properties.is_some() {
        // Which edge along a multi-hop walk would a property filter apply
        // to? Nothing in Cypher's own semantics answers that, so silently
        // picking one (or ignoring the filter) would be a guess dressed as
        // a query.
        return Err(LoweringError::Unlowerable(
            "a property filter on a variable-length relationship pattern",
        ));
    }
    let relationship_type = detail
        .types
        .as_ref()
        .map(single_relationship_type)
        .transpose()?;

    let min_hops = match range.start {
        None => 1,
        Some(n) => {
            usize::try_from(n).map_err(|_| LoweringError::Unlowerable("a negative hop count"))?
        }
    };
    let max_hops = match range.end {
        None => UNBOUNDED_HOP_LIMIT,
        Some(n) => usize::try_from(n)
            .map_err(|_| LoweringError::Unlowerable("a negative hop count"))?
            .min(UNBOUNDED_HOP_LIMIT),
    };
    if min_hops > max_hops {
        return Err(LoweringError::Unlowerable(
            "a variable-length pattern whose minimum exceeds its maximum",
        ));
    }

    let start = variable_of_term(from)?;
    let end = variable_of_term(to)?;

    patterns.push(TriplePattern {
        subject: TermPattern::Variable(start.clone()),
        predicate: NamedNodePattern::NamedNode(variable_length_hop_marker()),
        object: TermPattern::Variable(end.clone()),
    });

    hops.push(VariableLengthHop {
        start,
        end,
        relationship_type,
        min_hops,
        max_hops,
    });
    Ok(())
}

/// The reserved predicate a variable-length hop's sentinel triple uses.
///
/// **A different namespace from the catalog vocabulary, deliberately.**
/// Everything in `vocabulary.rs` addresses a term real projected data can
/// carry; this one must never collide with a real predicate, so it lives
/// under a namespace no connector or projection ever writes to.
fn variable_length_hop_marker() -> NamedNode {
    NamedNode::new("https://graph-owl.dev/ns/internal#variableLengthHop")
        .expect("a fixed IRI literal is always valid")
}

/// Whether a triple pattern is a [`lower_variable_length`] sentinel — matched
/// by predicate, not by which variables it names, so a real triple that
/// happens to connect the same two variables under a different predicate is
/// never mistaken for one.
fn is_variable_length_marker(pattern: &TriplePattern) -> bool {
    pattern.predicate == NamedNodePattern::NamedNode(variable_length_hop_marker())
}

/// Remove every variable-length sentinel from a lowered pattern, for
/// discovering what a hop's own starting point is bound to by the rest of
/// the query. **The sentinel matches no real data**, so evaluating a pattern
/// that still contains one always returns zero rows — this is what makes
/// discovery possible at all, by asking the question without it.
///
/// Structural, not semantic: this only rewrites tree shape, so a `Bgp` that
/// held only the sentinel becomes an empty `Bgp` (SPARQL's one-row identity)
/// rather than vanishing, and every other pattern kind is walked but
/// otherwise unchanged.
#[must_use]
pub fn strip_variable_length_hops(pattern: GraphPattern) -> GraphPattern {
    rewrite_hop_bgps(pattern, &mut |patterns| {
        patterns.retain(|triple| !is_variable_length_marker(triple));
        None
    })
}

/// Replace one hop's sentinel triple with the traversal engine's real
/// answer, wherever in the pattern it appears — the same tree position the
/// sentinel occupied, so `start`/`end`'s binding reaches every layer that
/// already expected them to be there.
///
/// # Errors
///
/// [`LoweringError::Unlowerable`] if the pattern contains no sentinel for
/// this hop — a caller resolving a hop [`lower`] never extracted.
pub fn substitute_variable_length_hop(
    pattern: GraphPattern,
    hop: &VariableLengthHop,
    bindings: &[Vec<Option<spargebra::term::GroundTerm>>],
) -> Result<GraphPattern, LoweringError> {
    let mut substituted = false;
    let rewritten = rewrite_hop_bgps(pattern, &mut |patterns| {
        let index = patterns.iter().position(|triple| {
            is_variable_length_marker(triple)
                && triple.subject == TermPattern::Variable(hop.start.clone())
                && triple.object == TermPattern::Variable(hop.end.clone())
        })?;
        patterns.remove(index);
        substituted = true;
        Some(GraphPattern::Values {
            variables: vec![hop.start.clone(), hop.end.clone()],
            bindings: bindings.to_vec(),
        })
    });
    if !substituted {
        return Err(LoweringError::Unlowerable(
            "a variable-length hop with no matching sentinel in its own pattern",
        ));
    }
    Ok(rewritten)
}

/// Walk every `Bgp` in a pattern, applying `rewrite` to its triple list.
///
/// `rewrite` mutates the list in place and may return a pattern to `Join`
/// alongside whatever remains — `None` for a plain removal (discovery),
/// `Some(values)` for a substitution. Everything that is not a `Bgp` is
/// walked structurally so a sentinel nested inside a `Filter`, `Project`,
/// `Join`, `LeftJoin`, `Distinct`, `Group`, `Extend`, `OrderBy` or `Slice` is
/// still found — which is every wrapper [`apply_return`] and [`apply_with`]
/// can produce.
fn rewrite_hop_bgps(
    pattern: GraphPattern,
    rewrite: &mut impl FnMut(&mut Vec<TriplePattern>) -> Option<GraphPattern>,
) -> GraphPattern {
    match pattern {
        GraphPattern::Bgp { mut patterns } => {
            let joined = rewrite(&mut patterns);
            let base = GraphPattern::Bgp { patterns };
            match joined {
                None => base,
                Some(values) => GraphPattern::Join {
                    left: Box::new(base),
                    right: Box::new(values),
                },
            }
        }
        GraphPattern::Join { left, right } => GraphPattern::Join {
            left: Box::new(rewrite_hop_bgps(*left, rewrite)),
            right: Box::new(rewrite_hop_bgps(*right, rewrite)),
        },
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => GraphPattern::LeftJoin {
            left: Box::new(rewrite_hop_bgps(*left, rewrite)),
            right: Box::new(rewrite_hop_bgps(*right, rewrite)),
            expression,
        },
        GraphPattern::Filter { expr, inner } => GraphPattern::Filter {
            expr,
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
        },
        GraphPattern::Project { inner, variables } => GraphPattern::Project {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
            variables,
        },
        GraphPattern::Distinct { inner } => GraphPattern::Distinct {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
        },
        GraphPattern::Reduced { inner } => GraphPattern::Reduced {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
        },
        GraphPattern::Group {
            inner,
            variables,
            aggregates,
        } => GraphPattern::Group {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
            variables,
            aggregates,
        },
        GraphPattern::Extend {
            inner,
            variable,
            expression,
        } => GraphPattern::Extend {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
            variable,
            expression,
        },
        GraphPattern::OrderBy { inner, expression } => GraphPattern::OrderBy {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
            expression,
        },
        GraphPattern::Slice {
            inner,
            start,
            length,
        } => GraphPattern::Slice {
            inner: Box::new(rewrite_hop_bgps(*inner, rewrite)),
            start,
            length,
        },
        // `Values`, `Path`, `Minus`, `Union` and `Service` never appear in
        // what this module lowers, so there is nothing inside them a
        // sentinel could hide in.
        other => other,
    }
}

/// Unwrap `RETURN`/`WITH`'s own modifiers down to the reading pattern
/// beneath — discovery needs to see past whatever a projection chose to keep,
/// since a hop's starting point may not be among the columns the query asked
/// for at all.
///
/// **Stops at the first `Join`.** A multi-part query (`WITH … MATCH …`) joins
/// each segment's own already-projected pattern onto the next; if an earlier
/// segment's `WITH` dropped the variable a later hop needs, this cannot see
/// past that boundary, and resolution then reports the hop as unconstrained —
/// a known limitation of this slice, not a silent wrong answer.
#[must_use]
pub fn reading_pattern(pattern: &GraphPattern) -> &GraphPattern {
    match pattern {
        GraphPattern::Slice { inner, .. }
        | GraphPattern::OrderBy { inner, .. }
        | GraphPattern::Distinct { inner }
        | GraphPattern::Reduced { inner }
        | GraphPattern::Project { inner, .. } => reading_pattern(inner),
        other => other,
    }
}

/// The variable a node-position term names — always a variable, because
/// [`lower_node`] never produces anything else.
fn variable_of_term(term: &TermPattern) -> Result<Variable, LoweringError> {
    match term {
        TermPattern::Variable(variable) => Ok(variable.clone()),
        _ => Err(LoweringError::Unlowerable(
            "a variable-length pattern endpoint that is not a plain node variable",
        )),
    }
}

/// The single relationship type a variable-length pattern follows, or a
/// refusal for the compound `TYPE1|TYPE2` form — the same restriction a
/// fixed-length relationship's [`label_node`] already applies. Returns the
/// raw name rather than an IRI, because the caller filters the traversal
/// engine's edges by that name directly, not by a vocabulary term.
fn single_relationship_type(types: &LabelExpression) -> Result<String, LoweringError> {
    match types {
        LabelExpression::Static(name) => Ok(name.name.clone()),
        _ => Err(LoweringError::Unlowerable("a compound label expression")),
    }
}

/// `UNWIND [..] AS x` — a table of one column.
fn lower_unwind(unwind: &Unwind) -> Result<GraphPattern, LoweringError> {
    let variable = Variable::new(unwind.variable.name.name.clone())
        .map_err(|_| LoweringError::Unlowerable("an unwind variable name"))?;
    let CypherExpression::Literal(decypher::ast::expr::Literal::List(list)) = &unwind.expression
    else {
        return Err(LoweringError::Unlowerable(
            "UNWIND over anything but a list literal",
        ));
    };
    let bindings = list
        .elements
        .iter()
        .map(|element| Ok(vec![Some(literal_of(element)?.into())]))
        .collect::<Result<Vec<_>, LoweringError>>()?;
    Ok(GraphPattern::Values {
        variables: vec![variable],
        bindings,
    })
}

/// `WITH` projects and optionally filters, which is what scopes the next part.
fn apply_with(
    inner: GraphPattern,
    with: &With,
    source: &str,
) -> Result<GraphPattern, LoweringError> {
    let mut properties = Vec::new();
    for item in &with.items {
        collect_properties(&item.expression, &mut properties);
    }
    let inner = with_property_bindings(inner, &properties)?;
    let variables = projected_variables(&with.items, source)?;
    let projected = if with
        .items
        .iter()
        .any(|item| is_aggregate_expr(&item.expression))
    {
        group_by_items(inner, &with.items, source)?
    } else {
        bind_aliases(inner, &with.items)?
    };
    let mut graph = GraphPattern::Project {
        inner: Box::new(projected),
        variables,
    };
    if with.distinct {
        graph = GraphPattern::Distinct {
            inner: Box::new(graph),
        };
    }
    if let Some(where_clause) = &with.where_clause {
        graph = GraphPattern::Filter {
            expr: lower_expression(where_clause)?,
            inner: Box::new(graph),
        };
    }
    Ok(graph)
}

/// `RETURN`, with its modifiers in Cypher's order.
///
/// **Order matters and is not arbitrary**: project, then distinct, then sort,
/// then slice. Sorting before projecting would let a query order by something it
/// does not return, which Cypher permits and SPARQL's algebra expresses by this
/// nesting; slicing before sorting would return the wrong rows entirely.
fn apply_return(
    inner: GraphPattern,
    returning: &Return,
    source: &str,
) -> Result<GraphPattern, LoweringError> {
    let mut properties = Vec::new();
    for item in &returning.items {
        collect_properties(&item.expression, &mut properties);
    }
    if let Some(order) = &returning.order {
        for item in &order.items {
            collect_properties(&item.expression, &mut properties);
        }
    }
    let inner = with_property_bindings(inner, &properties)?;
    let variables = projected_variables(&returning.items, source)?;
    let projected = if returning
        .items
        .iter()
        .any(|item| is_aggregate_expr(&item.expression))
    {
        group_by_items(inner, &returning.items, source)?
    } else {
        bind_aliases(inner, &returning.items)?
    };
    let mut graph = GraphPattern::Project {
        inner: Box::new(projected),
        variables,
    };
    if returning.distinct {
        graph = GraphPattern::Distinct {
            inner: Box::new(graph),
        };
    }
    if let Some(order) = &returning.order {
        let expression = order
            .items
            .iter()
            .map(|item| {
                let expr = lower_expression(&item.expression)?;
                // Ascending is Cypher's default when no direction is written.
                Ok(match item.direction {
                    Some(SortDirection::Descending) => OrderExpression::Desc(expr),
                    Some(SortDirection::Ascending) | None => OrderExpression::Asc(expr),
                })
            })
            .collect::<Result<Vec<_>, LoweringError>>()?;
        graph = GraphPattern::OrderBy {
            inner: Box::new(graph),
            expression,
        };
    }
    if returning.skip.is_some() || returning.limit.is_some() {
        graph = GraphPattern::Slice {
            inner: Box::new(graph),
            start: returning.skip.as_ref().map_or(Ok(0), count_of)?,
            length: returning.limit.as_ref().map(count_of).transpose()?,
        };
    }
    Ok(graph)
}

fn count_of(expression: &CypherExpression) -> Result<usize, LoweringError> {
    use decypher::ast::expr::{Literal, NumberLiteral};
    match expression {
        CypherExpression::Literal(Literal::Number(NumberLiteral::Integer(number))) => {
            usize::try_from(*number)
                .map_err(|_| LoweringError::UnsupportedLiteral(number.to_string()))
        }
        other => Err(LoweringError::UnsupportedLiteral(format!("{other:?}"))),
    }
}

fn projected_variables(
    items: &[decypher::ast::clause::ProjectionItem],
    source: &str,
) -> Result<Vec<Variable>, LoweringError> {
    let mut variables = Vec::new();
    for item in items {
        variables.push(variable_named(&projected_name(item, source)?)?);
    }
    if variables.is_empty() {
        return Err(LoweringError::NothingBound);
    }
    Ok(variables)
}

/// The output column name for one projection item — an explicit alias, or the
/// name Cypher itself would show for an unaliased one.
///
/// **A function call is checked before the alias, not after.** An alias is
/// just a name; it says nothing about whether anything binds to it. Without
/// this guard, `RETURN toUpper(n.name) AS x` would name the column `x` on the
/// strength of the alias alone and go on to select it from a pattern with
/// nothing bound to that name — an unbound-column bug rather than a refusal,
/// and the aliased form would hide it even from a reader checking for a bare
/// unsupported function call.
fn projected_name(
    item: &decypher::ast::clause::ProjectionItem,
    source: &str,
) -> Result<String, LoweringError> {
    if let CypherExpression::FunctionCall(invocation) = &item.expression
        && aggregate_function(invocation).is_none()
        && !is_refused_aggregate(invocation)
    {
        return Err(LoweringError::Unlowerable(
            "a function call that is not a supported aggregate",
        ));
    }
    Ok(match (&item.alias, &item.expression) {
        (Some(alias), _) => alias.name.name.clone(),
        (None, CypherExpression::Variable(variable)) => variable.name.name.clone(),
        (None, CypherExpression::PropertyLookup { base, property, .. }) => {
            // `RETURN n.name` with no alias projects a variable named for the
            // path, which is what Cypher shows in its own result header.
            format!("{}_{}", base_name(base)?, property.name.name)
        }
        (None, CypherExpression::CountStar { .. }) => "count_star".to_string(),
        (None, CypherExpression::FunctionCall(invocation)) => aggregate_name(invocation, source)?,
        _ => return Err(LoweringError::Unlowerable("this projection item")),
    })
}

/// A name for an unaliased aggregate column — `count(n)` becomes `count_n`,
/// mirroring the `{base}_{property}` convention [`projected_variables`]
/// already uses for an unaliased property lookup. Cypher users almost always
/// alias an aggregate (`AS c`); this exists so an unaliased one still lowers
/// rather than refusing on a technicality.
///
/// **Falls back to [`recover_dropped_argument`]** when `decypher` has already
/// dropped the argument from `invocation.arguments` — the same recovery
/// [`lower_aggregate`] needs, for the same reason: without it, an unaliased
/// `count(n)` would be named `count_` rather than `count_n`.
fn aggregate_name(
    invocation: &decypher::ast::expr::FunctionInvocation,
    source: &str,
) -> Result<String, LoweringError> {
    let function = invocation
        .name
        .last()
        .ok_or(LoweringError::Unlowerable("a function with no name"))?;
    let argument = match invocation.arguments.first() {
        Some(arg) => property_variable_name(arg)?,
        None => recover_dropped_argument(source, invocation.span.start).unwrap_or_default(),
    };
    Ok(format!("{}_{argument}", function.name.to_lowercase()))
}

/// The `{base}_{property}` name a property lookup contributes to a generated
/// identifier — shared between [`aggregate_name`] and the property-binding
/// convention so `sum(n.amount)` and `n.amount` never disagree about what the
/// bound variable is called.
fn property_variable_name(expression: &CypherExpression) -> Result<String, LoweringError> {
    match expression {
        CypherExpression::PropertyLookup { base, property, .. } => {
            Ok(format!("{}_{}", base_name(base)?, property.name.name))
        }
        other => base_name(other),
    }
}

/// Whether a projection item is an aggregate rather than a grouping key.
///
/// **`count(*)` is a distinct AST node** (`CountStar`), not a `FunctionCall`
/// named `"count"` — `decypher` gives it special grammar. Both are checked
/// here so the caller has one place to ask "is this an aggregate" rather than
/// two.
fn is_aggregate_expr(expression: &CypherExpression) -> bool {
    match expression {
        CypherExpression::CountStar { .. } => true,
        CypherExpression::FunctionCall(invocation) => {
            aggregate_function(invocation).is_some() || is_refused_aggregate(invocation)
        }
        _ => false,
    }
}

/// `collect(...)` is an aggregate by name, and is refused explicitly rather
/// than falling through to "not an aggregate" — which would misfile it as a
/// grouping key and refuse it with a confusing `variable_of` error instead of
/// the honest one in [`lower_aggregate`].
fn is_refused_aggregate(invocation: &decypher::ast::expr::FunctionInvocation) -> bool {
    invocation
        .name
        .last()
        .is_some_and(|name| name.name.eq_ignore_ascii_case("collect"))
}

/// The SPARQL aggregate function a Cypher function name maps onto, if any.
fn aggregate_function(
    invocation: &decypher::ast::expr::FunctionInvocation,
) -> Option<AggregateFunction> {
    let name = invocation.name.last()?;
    Some(match name.name.to_lowercase().as_str() {
        "count" => AggregateFunction::Count,
        "sum" => AggregateFunction::Sum,
        "avg" => AggregateFunction::Avg,
        "min" => AggregateFunction::Min,
        "max" => AggregateFunction::Max,
        _ => return None,
    })
}

/// Lower one projection item's aggregate, or say it is not one.
///
/// `Ok(None)` means "treat this as a grouping key instead" — the caller's
/// signal to fall through to [`variable_of`] rather than a special case here.
fn lower_aggregate(
    expression: &CypherExpression,
    source: &str,
) -> Result<Option<AggregateExpression>, LoweringError> {
    match expression {
        CypherExpression::CountStar { .. } => Ok(Some(AggregateExpression::CountSolutions {
            distinct: false,
        })),
        CypherExpression::FunctionCall(invocation) if is_refused_aggregate(invocation) => {
            Err(LoweringError::Unlowerable(
                "collect(...) — Cypher's list result has no lossless SPARQL \
                 equivalent; GROUP_CONCAT folds values into a string, not a list",
            ))
        }
        CypherExpression::FunctionCall(invocation) => {
            let Some(name) = aggregate_function(invocation) else {
                return Ok(None);
            };
            let expr = match invocation.arguments.as_slice() {
                [only] => lower_expression(only)?,
                // **`decypher` drops a bare-variable argument from its typed
                // AST** — confirmed for `count`, `sum`, `min`, `max`, `avg`
                // and a made-up function name, so this is a general gap in
                // the AST-building step and not specific to one aggregate. A
                // property-lookup argument (`sum(n.amount)`) is unaffected;
                // only `[]` here means it happened. Recovered from the
                // lossless CST — the same tree `subset.rs`'s gate trusts —
                // rather than guessed or silently treated as `count(*)`.
                [] => {
                    let name = recover_dropped_argument(source, invocation.span.start).ok_or(
                        LoweringError::Unlowerable(
                            "an aggregate argument this engine could not recover",
                        ),
                    )?;
                    Expression::Variable(variable_named(&name)?)
                }
                _ => {
                    return Err(LoweringError::Unlowerable(
                        "an aggregate with other than one argument",
                    ));
                }
            };
            Ok(Some(AggregateExpression::FunctionCall {
                name,
                expr,
                distinct: invocation.distinct,
            }))
        }
        _ => Ok(None),
    }
}

/// Recover a bare-variable aggregate argument `decypher` 0.2.0-alpha.6 drops
/// from its typed AST — a narrower cousin of the `CALL … YIELD` defect
/// `subset.rs` already documents. `count(n)`, `sum(r)` and every other
/// single-bare-variable function call arrive with `arguments: []`; a
/// property-lookup argument (`sum(n.amount)`) is unaffected.
///
/// **The CST is not dropped**: `decypher::parse_cst`'s `FUNCTION_INVOCATION`
/// node still carries a `VARIABLE` child after `FUNCTION_NAME`. This walks
/// that lossless tree — matched to the AST node by its span start, which both
/// trees agree on — rather than the broken AST. `None` when the shape is not
/// exactly one bare variable, so a caller that cannot recover refuses rather
/// than guesses.
fn recover_dropped_argument(source: &str, span_start: usize) -> Option<String> {
    use decypher::syntax::SyntaxKind;

    fn find(
        node: &decypher::syntax::SyntaxNode,
        start: usize,
    ) -> Option<decypher::syntax::SyntaxNode> {
        if node.kind() == SyntaxKind::FUNCTION_INVOCATION
            && usize::from(node.text_range().start()) == start
        {
            return Some(node.clone());
        }
        node.children().find_map(|child| find(&child, start))
    }

    let cst = decypher::parse_cst(source).tree;
    let invocation = find(&cst, span_start)?;
    let mut variables = invocation
        .children()
        .filter(|child| child.kind() == SyntaxKind::VARIABLE);
    let only = variables.next()?;
    if variables.next().is_some() {
        // More than one bare-variable child is not this defect's shape.
        return None;
    }
    Some(only.text().to_string())
}

/// Group a pattern by every non-aggregate item, computing every aggregate —
/// Cypher's implicit `GROUP BY`, with no clause of its own to write.
///
/// **A renamed grouping key needs `Extend` before it can appear in
/// `Group.variables`**, because `Group` only knows how to pass an *existing*
/// bound variable through unchanged; it cannot bind a new name itself.
/// `RETURN n.dept AS department, count(n) AS c` therefore binds `department`
/// from `?n_dept` first, then groups by `department` rather than `n_dept` — if
/// it grouped by the property variable instead, the alias in the final
/// projection would reference a name nothing bound.
fn group_by_items(
    inner: GraphPattern,
    items: &[decypher::ast::clause::ProjectionItem],
    source: &str,
) -> Result<GraphPattern, LoweringError> {
    let mut extended = inner;
    let mut group_variables = Vec::new();
    let mut aggregates = Vec::new();

    for item in items {
        if let Some(aggregate) = lower_aggregate(&item.expression, source)? {
            let name = variable_named(&projected_name(item, source)?)?;
            aggregates.push((name, aggregate));
            continue;
        }

        let key = variable_of(&item.expression)?;
        let name = projected_name(item, source)?;
        if name == key.as_str() {
            group_variables.push(key);
        } else {
            let renamed = variable_named(&name)?;
            extended = GraphPattern::Extend {
                inner: Box::new(extended),
                variable: renamed.clone(),
                expression: Expression::Variable(key),
            };
            group_variables.push(renamed);
        }
    }

    Ok(GraphPattern::Group {
        inner: Box::new(extended),
        variables: group_variables,
        aggregates,
    })
}

/// Bind every explicit alias that names something other than its own default
/// variable, via `Extend` — the non-aggregate counterpart of what
/// [`group_by_items`] already does for a grouping key.
///
/// **Naming a `Project` variable is not the same as binding it.**
/// `RETURN n.name AS label` must project a column called `label` *and* bind
/// `?label` from `?n_name`; without this, `Project` selects a variable
/// nothing in the pattern ever set, and the query silently returns rows with
/// that column always absent rather than erroring — the same class of bug
/// [`with_property_bindings`] exists to prevent for an unaliased property
/// reference. Caught by an end-to-end evaluation test, not a shape assertion:
/// `plan.contains("name: \"label\"")` is true whether or not `?label` is
/// bound, because the *name* is right either way.
fn bind_aliases(
    inner: GraphPattern,
    items: &[decypher::ast::clause::ProjectionItem],
) -> Result<GraphPattern, LoweringError> {
    let mut extended = inner;
    for item in items {
        let Some(alias) = &item.alias else {
            continue;
        };
        let source = variable_of(&item.expression)?;
        let target = variable_named(&alias.name.name)?;
        if target.as_str() != source.as_str() {
            extended = GraphPattern::Extend {
                inner: Box::new(extended),
                variable: target,
                expression: Expression::Variable(source),
            };
        }
    }
    Ok(extended)
}

fn base_name(expression: &CypherExpression) -> Result<String, LoweringError> {
    match expression {
        CypherExpression::Variable(variable) => Ok(variable.name.name.clone()),
        _ => Err(LoweringError::Unlowerable("a nested property access")),
    }
}

/// Every `base.property` an expression refers to.
///
/// **A property access is a join, and the join has to exist.** `WHERE n.age >
/// 21` lowers to a filter on `?n_age`, and if nothing binds `?n_age` the filter
/// is over an unbound variable — which does not error, it silently returns no
/// rows. That is the worst shape of bug available here: a query that looks
/// right, runs, and answers "nothing".
///
/// So every property an expression mentions gets its binding pattern emitted
/// into the same BGP that binds its base.
fn collect_properties(expression: &CypherExpression, into: &mut Vec<(String, String)>) {
    match expression {
        CypherExpression::PropertyLookup { base, property, .. } => {
            if let Ok(name) = base_name(base) {
                let entry = (name, property.name.name.clone());
                if !into.contains(&entry) {
                    into.push(entry);
                }
            }
        }
        CypherExpression::BinaryOp { lhs, rhs, .. } => {
            collect_properties(lhs, into);
            collect_properties(rhs, into);
        }
        CypherExpression::Comparison { lhs, operators, .. } => {
            collect_properties(lhs, into);
            for (_, rhs) in operators {
                collect_properties(rhs, into);
            }
        }
        CypherExpression::UnaryOp { operand, .. } | CypherExpression::IsNull { operand, .. } => {
            collect_properties(operand, into);
        }
        CypherExpression::Parenthesized(inner) => collect_properties(inner, into),
        // An aggregate's argument is a value expression like any other —
        // `sum(n.amount)` needs `?n_amount` bound exactly as `WHERE n.amount >
        // 0` would, or the aggregate silently sums nothing.
        CypherExpression::FunctionCall(invocation) => {
            for argument in &invocation.arguments {
                collect_properties(argument, into);
            }
        }
        _ => {}
    }
}

/// The triple patterns that bind a set of property accesses.
fn property_patterns_for(
    properties: &[(String, String)],
) -> Result<Vec<TriplePattern>, LoweringError> {
    properties
        .iter()
        .map(|(base, property)| {
            Ok(TriplePattern {
                subject: TermPattern::Variable(variable_named(base)?),
                predicate: NamedNodePattern::NamedNode(vocabulary::property(property)),
                object: TermPattern::Variable(variable_named(&format!("{base}_{property}"))?),
            })
        })
        .collect()
}

/// Join a pattern onto the bindings the expressions it uses require.
fn with_property_bindings(
    inner: GraphPattern,
    properties: &[(String, String)],
) -> Result<GraphPattern, LoweringError> {
    if properties.is_empty() {
        return Ok(inner);
    }
    Ok(GraphPattern::Join {
        left: Box::new(inner),
        right: Box::new(GraphPattern::Bgp {
            patterns: property_patterns_for(properties)?,
        }),
    })
}

/// A `WHERE` expression.
///
/// Deliberately narrow: comparison, boolean composition, negation, property
/// access and literals. Anything else is refused **at lowering**, so a query
/// that cannot be answered says so before it reaches the evaluator.
fn lower_expression(expression: &CypherExpression) -> Result<Expression, LoweringError> {
    use decypher::ast::expr::{BinaryOperator, Literal as CypherLiteral, UnaryOperator};

    Ok(match expression {
        CypherExpression::Variable(variable) => {
            Expression::Variable(variable_named(&variable.name.name)?)
        }
        CypherExpression::PropertyLookup { base, property, .. } => {
            Expression::Variable(property_variable(base, &property.name.name)?)
        }
        CypherExpression::Literal(CypherLiteral::Null) => {
            // `WHERE n.x = null` is never true in Cypher, and SPARQL has no null
            // literal at all — the honest lowering is a refusal rather than
            // something that looks like a comparison and is not.
            return Err(LoweringError::Unlowerable("a null literal in a comparison"));
        }
        CypherExpression::Literal(_) => Expression::Literal(literal_of(expression)?),

        // **Cypher chains comparisons, and `decypher` nests them to the
        // left.** `1 < n.x < 10` arrives as a `Comparison` whose *lhs is itself
        // a `Comparison`*, not as a flat list. Lowering that naively gives
        // `Less(Less(1, ?n_x), 10)` — a boolean compared to an integer, which
        // the evaluator accepts and answers wrongly. See `lower_comparison`.
        CypherExpression::Comparison { .. } => lower_comparison(expression)?.0,

        CypherExpression::BinaryOp { op, lhs, rhs, .. } => {
            let left = Box::new(lower_expression(lhs)?);
            let right = Box::new(lower_expression(rhs)?);
            match op {
                BinaryOperator::And => Expression::And(left, right),
                BinaryOperator::Or => Expression::Or(left, right),
                // `XOR` is `(a || b) && !(a && b)`. Written out rather than
                // refused because the rewrite is exact and Cypher users reach
                // for it; anything inexact would be worse than a refusal.
                BinaryOperator::Xor => Expression::And(
                    Box::new(Expression::Or(left.clone(), right.clone())),
                    Box::new(Expression::Not(Box::new(Expression::And(left, right)))),
                ),
                BinaryOperator::Add => Expression::Add(left, right),
                BinaryOperator::Subtract => Expression::Subtract(left, right),
                BinaryOperator::Multiply => Expression::Multiply(left, right),
                BinaryOperator::Divide => Expression::Divide(left, right),
                BinaryOperator::Modulo | BinaryOperator::Power => {
                    return Err(LoweringError::Unlowerable(
                        "modulo and exponentiation have no SPARQL operator",
                    ));
                }
            }
        }

        CypherExpression::UnaryOp { op, operand, .. } => match op {
            UnaryOperator::Not => Expression::Not(Box::new(lower_expression(operand)?)),
            UnaryOperator::Negate => Expression::UnaryMinus(Box::new(lower_expression(operand)?)),
            UnaryOperator::Plus => lower_expression(operand)?,
        },

        // `IS NULL` over a triple-store variable means "unbound", which is what
        // SPARQL's `BOUND` answers — not a comparison against a null value.
        CypherExpression::IsNull {
            operand, negated, ..
        } => {
            let bound = Expression::Bound(variable_of(operand)?);
            if *negated {
                bound
            } else {
                Expression::Not(Box::new(bound))
            }
        }

        CypherExpression::Parenthesized(inner) => lower_expression(inner)?,
        _ => return Err(LoweringError::Unlowerable("this expression")),
    })
}

/// A comparison chain, and the operand the next link chains from.
///
/// **Cypher's `a < b < c` means `a < b AND b < c`**, and `decypher` represents
/// it left-nested: the outer `Comparison`'s `lhs` is the inner one. So lowering
/// has to return two things — the conjunction built so far, and the *rightmost
/// operand*, because that is what the next comparison compares against.
///
/// Getting this wrong is silent rather than loud: `Less(Less(1, ?x), 10)` is a
/// boolean compared to an integer, which SPARQL evaluates without complaint and
/// answers incorrectly. That is why it is a function with a stated contract
/// rather than an inline fold.
fn lower_comparison(
    expression: &CypherExpression,
) -> Result<(Expression, Expression), LoweringError> {
    let CypherExpression::Comparison { lhs, operators, .. } = expression else {
        return Err(LoweringError::Unlowerable("a comparison"));
    };

    // If the left side is itself a comparison, it contributes its own
    // conjunction and hands back the operand this link continues from.
    let (mut conjunction, mut left) = match lhs.as_ref() {
        nested @ CypherExpression::Comparison { .. } => {
            let (inner, last) = lower_comparison(nested)?;
            (Some(inner), last)
        }
        plain => (None, lower_expression(plain)?),
    };

    for (operator, rhs) in operators {
        let right = lower_expression(rhs)?;
        let link = compare(*operator, left, right.clone())?;
        conjunction = Some(match conjunction {
            None => link,
            Some(previous) => Expression::And(Box::new(previous), Box::new(link)),
        });
        left = right;
    }

    Ok((
        conjunction.ok_or(LoweringError::Unlowerable("a comparison with no operator"))?,
        left,
    ))
}

fn compare(
    operator: decypher::ast::expr::ComparisonOperator,
    left: Expression,
    right: Expression,
) -> Result<Expression, LoweringError> {
    use decypher::ast::expr::ComparisonOperator as Cmp;
    let (left, right) = (Box::new(left), Box::new(right));
    Ok(match operator {
        Cmp::Eq => Expression::Equal(left, right),
        Cmp::Ne => Expression::Not(Box::new(Expression::Equal(left, right))),
        Cmp::Lt => Expression::Less(left, right),
        Cmp::Le => Expression::LessOrEqual(left, right),
        Cmp::Gt => Expression::Greater(left, right),
        Cmp::Ge => Expression::GreaterOrEqual(left, right),
        // These are string functions in SPARQL, and mapping them needs the
        // argument order and null semantics checked against the spec rather
        // than guessed. Refused until then.
        Cmp::RegexMatch | Cmp::StartsWith | Cmp::EndsWith | Cmp::Contains => {
            return Err(LoweringError::Unlowerable("a string-matching comparison"));
        }
    })
}

fn variable_named(name: &str) -> Result<Variable, LoweringError> {
    Variable::new(name.to_string()).map_err(|_| LoweringError::Unlowerable("a variable name"))
}

/// The variable a property access binds to.
///
/// **A property access is a join, not a function.** In a triple store the value
/// of `n.prop` is whatever `?n dsc:prop ?x` binds, so an expression naming one
/// refers to the variable that pattern introduced. `n.age` therefore lowers to
/// `?n_age`, which is the same name [`projected_variables`] and
/// [`property_patterns_for`] use — they must agree or the filter references a
/// variable nothing binds.
fn property_variable(base: &CypherExpression, property: &str) -> Result<Variable, LoweringError> {
    variable_named(&format!("{}_{}", base_name(base)?, property))
}

fn variable_of(expression: &CypherExpression) -> Result<Variable, LoweringError> {
    match expression {
        CypherExpression::Variable(variable) => variable_named(&variable.name.name),
        CypherExpression::PropertyLookup { base, property, .. } => {
            property_variable(base, &property.name.name)
        }
        _ => Err(LoweringError::Unlowerable("this operand")),
    }
}

fn literal_of(expression: &CypherExpression) -> Result<Literal, LoweringError> {
    use decypher::ast::expr::{Literal as CypherLiteral, NumberLiteral};
    match expression {
        CypherExpression::Literal(CypherLiteral::Number(NumberLiteral::Integer(number))) => {
            Ok(Literal::from(*number))
        }
        CypherExpression::Literal(CypherLiteral::Number(NumberLiteral::Float(number))) => {
            Ok(Literal::from(*number))
        }
        CypherExpression::Literal(CypherLiteral::String(text)) => {
            Ok(Literal::from(text.value.clone()))
        }
        CypherExpression::Literal(CypherLiteral::Boolean(flag)) => Ok(Literal::from(*flag)),
        other => Err(LoweringError::UnsupportedLiteral(format!("{other:?}"))),
    }
}

fn label_node(label: &LabelExpression) -> Result<NamedNode, LoweringError> {
    match label {
        LabelExpression::Static(name) => Ok(vocabulary::class(&name.name)),
        // `Person|Company` is a union of patterns rather than one, and
        // `Person&!Deleted` needs a negation the BGP cannot carry. Refused at
        // lowering, because approximating either changes the answer.
        _ => Err(LoweringError::Unlowerable("a compound label expression")),
    }
}

/// A node's variable, invented when the pattern does not name one.
///
/// **Anonymous nodes still need a variable**, because the reification joins
/// through them: `()-[r]->()` binds nothing the caller asked for and everything
/// the pattern needs. The name is derived from the span so two anonymous nodes
/// in one query never collide, and the same query always produces the same plan
/// — determinism the golden tests depend on.
fn node_variable(node: &NodePattern) -> Variable {
    node.variable
        .as_ref()
        .and_then(|variable| Variable::new(variable.name.name.clone()).ok())
        .unwrap_or_else(|| {
            Variable::new(format!("_node{}", node.span.start)).expect("a generated name is valid")
        })
}

fn relationship_variable(relationship: &RelationshipPattern, ordinal: usize) -> Variable {
    relationship
        .detail
        .as_ref()
        .and_then(|detail| detail.variable.as_ref())
        .and_then(|variable| Variable::new(variable.name.name.clone()).ok())
        .unwrap_or_else(|| {
            Variable::new(format!("_rel{}_{ordinal}", relationship.span.start))
                .expect("a generated name is valid")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cypher::parse_subset;

    fn lowered(query: &str) -> GraphPattern {
        let parsed = parse_subset(query).unwrap_or_else(|e| panic!("{query} -> {e}"));
        lower(&parsed, query)
            .unwrap_or_else(|e| panic!("{query} -> {e}"))
            .0
    }

    fn lowered_with_hops(query: &str) -> (GraphPattern, Vec<VariableLengthHop>) {
        let parsed = parse_subset(query).unwrap_or_else(|e| panic!("{query} -> {e}"));
        lower(&parsed, query).unwrap_or_else(|e| panic!("{query} -> {e}"))
    }

    fn refused(query: &str) -> LoweringError {
        let parsed = parse_subset(query).expect("in the subset");
        lower(&parsed, query).expect_err("should not lower")
    }

    fn walk_triples(pattern: &GraphPattern, out: &mut Vec<String>) {
        match pattern {
            GraphPattern::Bgp { patterns } => {
                for p in patterns {
                    out.push(format!("{} {} {}", p.subject, p.predicate, p.object));
                }
            }
            GraphPattern::Join { left, right }
            | GraphPattern::Union { left, right }
            | GraphPattern::LeftJoin { left, right, .. } => {
                walk_triples(left, out);
                walk_triples(right, out);
            }
            GraphPattern::Filter { inner, .. }
            | GraphPattern::Project { inner, .. }
            | GraphPattern::Distinct { inner }
            | GraphPattern::Reduced { inner }
            | GraphPattern::OrderBy { inner, .. }
            | GraphPattern::Slice { inner, .. }
            | GraphPattern::Group { inner, .. }
            | GraphPattern::Extend { inner, .. } => walk_triples(inner, out),
            _ => {}
        }
    }

    /// Every triple pattern in the plan, flattened, as `subject predicate object`.
    fn triples(pattern: &GraphPattern) -> Vec<String> {
        let mut out = Vec::new();
        walk_triples(pattern, &mut out);
        out
    }

    fn rendered(pattern: &GraphPattern) -> String {
        format!("{pattern:?}")
    }

    // ---- Slice B: the mapping table ----

    #[test]
    fn a_label_lowers_to_a_type_pattern() {
        let plan = triples(&lowered("MATCH (n:Person) RETURN n"));

        assert!(
            plan.iter().any(|t| t.contains("catalog#type")
                && t.contains("catalog#Person")
                && t.starts_with("?n ")),
            "{plan:?}"
        );
    }

    /// **A relationship is three patterns, not one.** Lowering it to a single
    /// predicate would be shorter, would look right, and would make edge
    /// properties inexpressible — which is the whole argument of Epic 7c.
    #[test]
    fn a_relationship_lowers_to_three_patterns() {
        let plan = triples(&lowered("MATCH (a)-[r:FEEDS]->(b) RETURN a"));

        let about_r: Vec<&String> = plan.iter().filter(|t| t.starts_with("?r ")).collect();
        assert_eq!(
            about_r.len(),
            3,
            "relType, fromEntity, toEntity — got {about_r:?}"
        );
        assert!(about_r.iter().any(|t| t.contains("catalog#relType")));
        assert!(
            about_r
                .iter()
                .any(|t| t.contains("catalog#fromEntity") && t.ends_with("?a"))
        );
        assert!(
            about_r
                .iter()
                .any(|t| t.contains("catalog#toEntity") && t.ends_with("?b"))
        );
    }

    /// **The case that proves reification pays off.** An edge property is
    /// another fact about the relationship node, which a single-predicate
    /// lowering could not express at all.
    #[test]
    fn an_edge_property_lowers_to_a_pattern_on_the_relationship() {
        let plan = triples(&lowered(
            "MATCH (a)-[r:FEEDS {confidence: 0.9}]->(b) RETURN a",
        ));

        assert!(
            plan.iter()
                .any(|t| t.starts_with("?r ") && t.contains("catalog#confidence")),
            "the property hangs off the relationship, not the endpoints: {plan:?}"
        );
    }

    #[test]
    fn direction_decides_which_endpoint_is_from() {
        let right = triples(&lowered("MATCH (a)-[r]->(b) RETURN a"));
        let left = triples(&lowered("MATCH (a)<-[r]-(b) RETURN a"));

        assert!(
            right
                .iter()
                .any(|t| t.contains("fromEntity") && t.ends_with("?a"))
        );
        assert!(
            left.iter()
                .any(|t| t.contains("fromEntity") && t.ends_with("?b"))
        );
    }

    #[test]
    fn an_inline_node_property_lowers_to_a_pattern() {
        let plan = triples(&lowered("MATCH (n:Person {name: 'Ada'}) RETURN n"));

        assert!(
            plan.iter()
                .any(|t| t.contains("catalog#name") && t.contains("Ada")),
            "{plan:?}"
        );
    }

    /// **A property in `WHERE` gets its binding pattern.** Without it the filter
    /// is over an unbound variable, which does not error — it silently returns
    /// no rows, which is the worst shape of bug available here.
    #[test]
    fn a_property_referenced_in_where_is_bound_by_a_pattern() {
        let plan = triples(&lowered("MATCH (n) WHERE n.age > 21 RETURN n"));

        assert!(
            plan.iter().any(|t| t.starts_with("?n ")
                && t.contains("catalog#age")
                && t.ends_with("?n_age")),
            "nothing would bind ?n_age: {plan:?}"
        );
    }

    #[test]
    fn a_property_returned_is_bound_by_a_pattern() {
        let plan = triples(&lowered("MATCH (n) RETURN n.name"));

        assert!(
            plan.iter()
                .any(|t| t.contains("catalog#name") && t.ends_with("?n_name")),
            "{plan:?}"
        );
    }

    #[test]
    fn optional_match_lowers_to_a_left_join() {
        let plan = lowered("MATCH (a) OPTIONAL MATCH (a)-[r]->(b) RETURN a, b");

        assert!(rendered(&plan).contains("LeftJoin"), "{plan:?}");
    }

    #[test]
    fn a_plain_second_match_lowers_to_a_join_not_a_left_join() {
        let plan = rendered(&lowered("MATCH (a) MATCH (b) RETURN a, b"));

        assert!(plan.contains("Join"), "{plan}");
        assert!(
            !plan.contains("LeftJoin"),
            "an inner join, not an outer: {plan}"
        );
    }

    #[test]
    fn distinct_order_skip_and_limit_lower_in_cypher_order() {
        let plan = rendered(&lowered(
            "MATCH (n) RETURN DISTINCT n ORDER BY n.name DESC SKIP 10 LIMIT 5",
        ));

        // Slice is outermost, then order, then distinct, then project — the
        // nesting that makes "sort then take" mean what Cypher says.
        let slice = plan.find("Slice").expect("a slice");
        let order = plan.find("OrderBy").expect("an order");
        let distinct = plan.find("Distinct").expect("a distinct");
        let project = plan.find("Project").expect("a projection");
        assert!(slice < order, "slicing wraps ordering: {plan}");
        assert!(order < distinct, "ordering wraps distinct: {plan}");
        assert!(distinct < project, "distinct wraps projection: {plan}");
    }

    #[test]
    fn descending_and_default_order_differ() {
        assert!(rendered(&lowered("MATCH (n) RETURN n ORDER BY n.name DESC")).contains("Desc"));
        assert!(rendered(&lowered("MATCH (n) RETURN n ORDER BY n.name")).contains("Asc"));
    }

    #[test]
    fn unwind_lowers_to_an_inline_table() {
        let plan = rendered(&lowered("UNWIND [1, 2, 3] AS x RETURN x"));

        assert!(plan.contains("Values"), "{plan}");
    }

    #[test]
    fn with_becomes_a_projection_boundary() {
        let plan = rendered(&lowered("MATCH (n) WITH n AS m RETURN m"));

        assert!(
            plan.matches("Project").count() >= 2,
            "one per boundary: {plan}"
        );
    }

    /// **Lowering is deterministic**, including for anonymous nodes — the plan
    /// is a cache key and a golden-test subject, so two lowerings of one query
    /// must be identical.
    #[test]
    fn lowering_the_same_query_twice_gives_the_same_plan() {
        for query in [
            "MATCH (a)-[r:FEEDS]->(b) RETURN a",
            "MATCH ()-[r]->() RETURN r",
            "MATCH (n) WHERE n.a > 1 AND n.b < 2 RETURN n",
        ] {
            assert_eq!(
                rendered(&lowered(query)),
                rendered(&lowered(query)),
                "{query}"
            );
        }
    }

    // ---- Slice C: relationship isomorphism ----

    /// **The correctness trap this slice exists for.**
    ///
    /// Cypher forbids two relationship variables in one `MATCH` binding the same
    /// relationship; SPARQL's BGP is homomorphic and permits it. Over a
    /// self-loop, the homomorphic reading returns a row Cypher would not. The
    /// distinctness is injected into the *algebra* so it is visible in the plan
    /// rather than hidden in an operator.
    #[test]
    fn two_relationships_in_one_match_may_not_be_the_same_relationship() {
        let plan = rendered(&lowered("MATCH (a)-[r1]->(b)-[r2]->(c) RETURN a"));

        assert!(
            plan.contains("SameTerm"),
            "an inequality over r1 and r2 must be in the plan: {plan}"
        );
        assert!(plan.contains("Not"), "{plan}");
    }

    /// **Across separate `MATCH` clauses reuse is permitted**, which is Cypher's
    /// actual rule rather than an approximation of it. Applying distinctness
    /// here would reject queries Cypher accepts.
    #[test]
    fn relationships_in_separate_matches_may_coincide() {
        let plan = rendered(&lowered("MATCH (a)-[r1]->(b) MATCH (c)-[r2]->(d) RETURN a"));

        assert!(
            !plan.contains("SameTerm"),
            "no cross-clause constraint: {plan}"
        );
    }

    /// One relationship needs no constraint — a filter that is always true is
    /// noise in every plan that has a single edge, which is most of them.
    #[test]
    fn a_single_relationship_gets_no_constraint() {
        let plan = rendered(&lowered("MATCH (a)-[r]->(b) RETURN a"));

        assert!(!plan.contains("SameTerm"), "{plan}");
    }

    /// **Node variables may still coincide.** Cypher's rule is about
    /// relationships; constraining nodes too would reject `MATCH (a)-[r]->(a)`,
    /// which is a legitimate self-loop query.
    #[test]
    fn node_variables_are_not_constrained() {
        let plan = rendered(&lowered("MATCH (a)-[r1]->(b)-[r2]->(c) RETURN a"));

        // Exactly one pair of relationship variables, so exactly one inequality.
        assert_eq!(
            plan.matches("SameTerm").count(),
            1,
            "one constraint per relationship pair, none for nodes: {plan}"
        );
    }

    /// Three relationships need all three pairs, not just adjacent ones —
    /// `r1 ≠ r3` matters as much as `r1 ≠ r2`.
    #[test]
    fn three_relationships_get_every_pair() {
        let plan = rendered(&lowered("MATCH (a)-[r1]->(b)-[r2]->(c)-[r3]->(d) RETURN a"));

        assert_eq!(plan.matches("SameTerm").count(), 3, "{plan}");
    }

    // ---- refusals happen at lowering, not at execution ----

    /// **A variable-length pattern lowers, but binding its relationship list
    /// does not.** `[r*1..3]` asks for the path's own edges; `neighbours`
    /// reports reached nodes and their distance, not the route taken —
    /// getting the edges needs `all_paths`, a materially more expensive call
    /// this slice does not make. Refused rather than silently returning
    /// something else for `r`.
    #[test]
    fn a_bound_relationship_list_on_a_variable_length_pattern_is_refused() {
        assert_eq!(
            refused("MATCH (a)-[r*1..3]->(b) RETURN r"),
            LoweringError::Unlowerable(
                "a variable-length relationship pattern binding the relationship list"
            ),
        );
    }

    // ---- Slice D: variable-length patterns are extracted, not lowered ----

    /// **The pattern itself carries none of the relationship's usual three
    /// triples.** They cannot: nothing in the algebra can express "1 to 3
    /// hops of this type", which is the whole reason this is extracted rather
    /// than lowered — see the module docs.
    #[test]
    fn a_variable_length_pattern_contributes_a_sentinel_not_a_real_relationship() {
        let (pattern, hops) = lowered_with_hops("MATCH (a)-[:FEEDS*1..3]->(b) RETURN b");

        assert_eq!(hops.len(), 1, "{hops:?}");
        let triples = triples(&pattern);
        assert!(
            triples.iter().all(|t| !t.contains("relType")
                && !t.contains("fromEntity")
                && !t.contains("toEntity")),
            "a hop must not also lower as a real relationship: {triples:?}"
        );
        assert!(
            triples
                .iter()
                .any(|t| t.contains("internal#variableLengthHop")),
            "and must still appear as a sentinel, so RETURN cannot drop `b`: {triples:?}"
        );
    }

    #[test]
    fn a_variable_length_hop_carries_its_bounds_and_type() {
        let (_, hops) = lowered_with_hops("MATCH (a)-[:FEEDS*1..3]->(b) RETURN b");

        assert_eq!(hops.len(), 1);
        let hop = &hops[0];
        assert_eq!(hop.start.as_str(), "a");
        assert_eq!(hop.end.as_str(), "b");
        assert_eq!(hop.relationship_type.as_deref(), Some("FEEDS"));
        assert_eq!(hop.min_hops, 1);
        assert_eq!(hop.max_hops, 3);
    }

    /// A bare `*` means "one or more", not "zero or more" — the openCypher
    /// grammar's own default, not zero, because zero hops would mean `a` and
    /// `b` are the same node.
    #[test]
    fn a_bare_star_defaults_the_minimum_to_one() {
        let (_, hops) = lowered_with_hops("MATCH (a)-[*]->(b) RETURN b");

        assert_eq!(hops[0].min_hops, 1);
        assert_eq!(hops[0].relationship_type, None, "no type names any");
    }

    /// **An unbounded upper end is capped, not refused.** Reusing
    /// `graph-owl-server`'s own server-side hop cap rather than a fresh
    /// number — see `UNBOUNDED_HOP_LIMIT`.
    #[test]
    fn an_unbounded_upper_end_is_capped_at_the_shared_limit() {
        let (_, hops) = lowered_with_hops("MATCH (a)-[*2..]->(b) RETURN b");

        assert_eq!(hops[0].min_hops, 2);
        assert_eq!(hops[0].max_hops, UNBOUNDED_HOP_LIMIT);
    }

    /// **An explicit upper bound past the cap is capped too**, not honoured —
    /// a query cannot opt itself out of the server's own protection.
    #[test]
    fn an_explicit_upper_bound_past_the_cap_is_still_capped() {
        let (_, hops) = lowered_with_hops("MATCH (a)-[*1..100]->(b) RETURN b");

        assert_eq!(hops[0].max_hops, UNBOUNDED_HOP_LIMIT);
    }

    /// **`start` is always the topological tail, `end` the head — regardless
    /// of which side of the arrow either variable was written on.** Both
    /// forms describe the same walk, and a caller that resolved them
    /// differently would answer the same question two different ways.
    #[test]
    fn direction_normalises_to_tail_and_head_however_the_arrow_points() {
        let (_, forward) = lowered_with_hops("MATCH (a)-[*1..3]->(b) RETURN b");
        let (_, backward) = lowered_with_hops("MATCH (b)<-[*1..3]-(a) RETURN b");

        assert_eq!(forward[0].start.as_str(), "a");
        assert_eq!(forward[0].end.as_str(), "b");
        assert_eq!(backward[0].start.as_str(), "a");
        assert_eq!(backward[0].end.as_str(), "b");
    }

    #[test]
    fn a_minimum_exceeding_the_maximum_is_refused() {
        assert_eq!(
            refused("MATCH (a)-[*5..2]->(b) RETURN b"),
            LoweringError::Unlowerable(
                "a variable-length pattern whose minimum exceeds its maximum"
            ),
        );
    }

    #[test]
    fn a_property_filter_on_a_variable_length_pattern_is_refused() {
        let refusal = refused("MATCH (a)-[:FEEDS*1..3 {confidence: 0.9}]->(b) RETURN b");
        assert!(
            matches!(refusal, LoweringError::Unlowerable(_)),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_compound_type_on_a_variable_length_pattern_is_refused() {
        assert_eq!(
            refused("MATCH (a)-[:FEEDS|LIKES*1..3]->(b) RETURN b"),
            LoweringError::Unlowerable("a compound label expression"),
        );
    }

    #[test]
    fn an_undirected_variable_length_pattern_is_refused() {
        assert_eq!(
            refused("MATCH (a)-[*1..3]-(b) RETURN b"),
            LoweringError::Unlowerable("an undirected relationship pattern"),
        );
    }

    /// Two independent variable-length patterns in one `MATCH` are both
    /// extracted — the accumulator must not silently keep only the last one.
    #[test]
    fn several_variable_length_patterns_are_all_extracted() {
        let (_, hops) =
            lowered_with_hops("MATCH (a)-[:FEEDS*1..2]->(b), (c)-[:LIKES*1..2]->(d) RETURN a, c");

        assert_eq!(hops.len(), 2, "{hops:?}");
        let starts: std::collections::BTreeSet<&str> =
            hops.iter().map(|hop| hop.start.as_str()).collect();
        assert_eq!(starts, ["a", "c"].into_iter().collect());
    }

    // ---- Slice D: stripping and substituting a hop's sentinel ----

    /// Stripping removes only the sentinel — a real triple pattern elsewhere
    /// in the same `Bgp` (here, the property binding `WHERE` needs) must
    /// survive, or discovery would lose the very constraint it exists to
    /// read.
    #[test]
    fn stripping_removes_only_the_sentinel_triple() {
        let (pattern, hops) =
            lowered_with_hops("MATCH (a)-[:FEEDS*1..3]->(b) WHERE a.name = 'x' RETURN b");
        let hop = &hops[0];

        let stripped = strip_variable_length_hops(pattern);
        let stripped_triples = triples(&stripped);

        assert!(
            stripped_triples
                .iter()
                .all(|t| !t.contains("internal#variableLengthHop")),
            "{stripped_triples:?}"
        );
        assert!(
            stripped_triples.iter().any(|t| t.contains("catalog#name")),
            "a.name's own binding must survive stripping: {stripped_triples:?}"
        );
        let _ = hop;
    }

    /// **The pattern is answerable without the hop, and always returns
    /// nothing while it does** — the sentinel matches no real data, so
    /// evaluating the *unstripped* pattern is how discovery would fail if
    /// stripping were skipped. Confirmed by running both for real: the
    /// unstripped form finds nothing even though `a` exists; the stripped
    /// form finds it.
    #[test]
    fn the_unstripped_pattern_never_matches_and_the_stripped_one_does() {
        use graph_owl_core::flake::{Flake, FlakeValue, Sid};

        let flakes = vec![Flake::assert(
            Sid::dsc("a"),
            Sid::dsc("name"),
            FlakeValue::String("x".into()),
            1,
        )];

        let (pattern, _hops) =
            lowered_with_hops("MATCH (a)-[:FEEDS*1..3]->(b) WHERE a.name = 'x' RETURN a.name");
        let unstripped_rows = evaluate_pattern(&flakes, pattern.clone());
        let stripped_rows = evaluate_pattern(&flakes, strip_variable_length_hops(pattern));

        assert!(unstripped_rows.is_empty(), "{unstripped_rows:?}");
        assert_eq!(stripped_rows, vec!["\"x\"".to_string()]);
    }

    /// `reading_pattern` sees past `RETURN`'s own projection — the property
    /// this exists for. `RETURN b` alone would hide `a` behind a `Project`
    /// that never named it; discovery needs the pattern underneath.
    #[test]
    fn reading_pattern_unwraps_returns_own_projection() {
        let (pattern, _hops) = lowered_with_hops(
            "MATCH (a)-[:FEEDS*1..3]->(b) WHERE a.name = 'x' RETURN b ORDER BY b LIMIT 5",
        );

        let reading = reading_pattern(&pattern);

        assert!(
            !matches!(
                reading,
                GraphPattern::Project { .. }
                    | GraphPattern::OrderBy { .. }
                    | GraphPattern::Slice { .. }
            ),
            "{reading:?}"
        );
        // `a`'s own binding must still be visible from here — the entire
        // point of unwrapping.
        let inner_triples = triples(reading);
        assert!(
            inner_triples.iter().any(|t| t.contains("catalog#name")),
            "{inner_triples:?}"
        );
    }

    /// **Substitution replaces the sentinel with real data, and the result
    /// actually evaluates to it** — not just a shape assertion. `RETURN b`
    /// only ever names `b`; if substitution bound the wrong variable, or
    /// bound it somewhere `RETURN`'s own `Project` could not see, this would
    /// still return an empty result set rather than erroring.
    #[test]
    fn substituting_a_hop_makes_the_pattern_answer_with_the_real_binding() {
        let (pattern, hops) = lowered_with_hops("MATCH (a)-[:FEEDS*1..3]->(b) RETURN b");
        let hop = &hops[0];

        let target = NamedNode::new("https://graph-owl.dev/ns/catalog#target").expect("iri");
        let bindings = vec![vec![
            Some(spargebra::term::GroundTerm::NamedNode(
                NamedNode::new("https://graph-owl.dev/ns/catalog#seed").expect("iri"),
            )),
            Some(spargebra::term::GroundTerm::NamedNode(target.clone())),
        ]];
        let substituted =
            substitute_variable_length_hop(pattern, hop, &bindings).expect("should substitute");

        let rows = evaluate_pattern(&[], substituted);

        assert_eq!(rows, vec![target.to_string()], "{rows:?}");
    }

    #[test]
    fn substituting_an_unmatched_hop_is_refused() {
        let (pattern, _hops) = lowered_with_hops("MATCH (a)-[:FEEDS*1..3]->(b) RETURN b");
        let unrelated_hop = VariableLengthHop {
            start: Variable::new("nope_start").expect("valid"),
            end: Variable::new("nope_end").expect("valid"),
            relationship_type: None,
            min_hops: 1,
            max_hops: 1,
        };

        let result = substitute_variable_length_hop(pattern, &unrelated_hop, &[]);

        assert!(result.is_err(), "{result:?}");
    }

    /// Evaluates a pattern directly (no `RETURN` lowering involved) over real
    /// flakes, and returns the sole projected column's rendered values —
    /// used where the test needs to build or rewrite the algebra itself
    /// rather than go through a fresh `lowered` call.
    fn evaluate_pattern(
        flakes: &[graph_owl_core::flake::Flake],
        pattern: GraphPattern,
    ) -> Vec<String> {
        let query = spargebra::Query::Select {
            dataset: None,
            pattern,
            base_iri: None,
        };
        let dataset = crate::dataset::FlakeDataset::from_flakes(flakes).expect("dataset");
        let results = spareval::QueryEvaluator::new()
            .prepare(&query)
            .execute(&dataset)
            .expect("evaluation should succeed");
        match results {
            spareval::QueryResults::Solutions(iter) => iter
                .map(|solution| {
                    let solution = solution.expect("solution");
                    solution
                        .iter()
                        .next()
                        .map(|(_, term)| term.to_string())
                        .unwrap_or_default()
                })
                .collect(),
            _ => panic!("a SELECT must yield solutions"),
        }
    }

    #[test]
    fn an_undirected_pattern_is_refused_rather_than_half_answered() {
        assert_eq!(
            refused("MATCH (a)-[r]-(b) RETURN a"),
            LoweringError::Unlowerable("an undirected relationship pattern")
        );
    }

    #[test]
    fn a_compound_label_expression_is_refused() {
        assert!(matches!(
            refused("MATCH (n:Person|Company) RETURN n"),
            LoweringError::Unlowerable(_)
        ));
    }

    /// A chained comparison is the conjunction, not just its first operator —
    /// taking the first would silently drop half the predicate.
    /// **A chained comparison is a conjunction, not a nested comparison.**
    ///
    /// `decypher` nests `1 < n.x < 10` to the left, so the naive lowering was
    /// `Less(Less(1, ?n_x), 10)` — a boolean compared to an integer, which
    /// SPARQL evaluates without complaint and answers wrongly. This test caught
    /// it; the assertion is that the *comparisons are siblings under an `And`*,
    /// not merely that both words appear.
    #[test]
    fn a_chained_comparison_lowers_to_a_conjunction_not_a_nested_comparison() {
        let plan = rendered(&lowered("MATCH (n) WHERE 1 < n.x < 10 RETURN n"));

        assert!(plan.contains("And("), "{plan}");
        assert_eq!(plan.matches("Less(").count(), 2, "two comparisons: {plan}");
        assert!(
            !plan.contains("Less(Less("),
            "a comparison must never be an operand of a comparison: {plan}"
        );
    }

    /// And a three-link chain is three comparisons, so the fold is not
    /// accidentally dropping the middle.
    #[test]
    fn a_three_link_comparison_chain_keeps_every_link() {
        let plan = rendered(&lowered("MATCH (n) WHERE 1 < n.x < 10 < n.y RETURN n"));

        assert_eq!(plan.matches("Less(").count(), 3, "{plan}");
        assert!(!plan.contains("Less(Less("), "{plan}");
    }

    #[test]
    fn boolean_composition_lowers() {
        let plan = rendered(&lowered(
            "MATCH (n) WHERE n.a > 1 AND (n.b < 2 OR n.c = 3) RETURN n",
        ));

        assert!(plan.contains("And") && plan.contains("Or"), "{plan}");
    }

    /// **The projected variables are the ones asked for**, by name. Asserting
    /// only that a `Project` node exists let a lowering that projected *nothing*
    /// pass — mutation testing found it.
    #[test]
    fn the_projection_names_the_variables_returned() {
        let plan = rendered(&lowered("MATCH (a)-[r]->(b) RETURN a, b"));

        assert!(plan.contains("name: \"a\""), "{plan}");
        assert!(plan.contains("name: \"b\""), "{plan}");
    }

    #[test]
    fn an_alias_names_the_projected_variable() {
        let plan = rendered(&lowered("MATCH (n) RETURN n.name AS label"));

        assert!(plan.contains("name: \"label\""), "{plan}");
    }

    /// **Naming the projected variable is not the same as binding it.** The
    /// test above passed even before an alias was ever bound to anything,
    /// because `Project { variables: [label] }` contains the string
    /// `"label"` whether or not `?label` has a value — the shape looked
    /// right and the query would have silently returned every row with that
    /// column absent. Only a real evaluation catches it.
    #[test]
    fn an_aliased_property_lookup_actually_binds_its_alias() {
        use graph_owl_core::flake::{Flake, FlakeValue, Sid};

        let flakes = vec![Flake::assert(
            Sid::dsc("a"),
            Sid::dsc("name"),
            FlakeValue::String("ada".into()),
            1,
        )];

        let names = evaluate(&flakes, "MATCH (n) RETURN n.name AS label");

        assert_eq!(
            names,
            vec!["\"ada\"".to_string()],
            "?label must actually be bound to n's name, not merely named: {names:?}"
        );
    }

    /// The same gap, for a bare-variable alias (`WITH n AS m`) rather than a
    /// property lookup — the pipeline-boundary form Slice B's `WITH` test
    /// only checked the shape of, never that `?m` held anything.
    #[test]
    fn an_aliased_bare_variable_actually_binds_its_alias() {
        use graph_owl_core::flake::{Flake, FlakeValue, Sid};

        let flakes = vec![Flake::assert(
            Sid::dsc("a"),
            Sid::dsc("type"),
            FlakeValue::Ref(Sid::dsc("Row")),
            1,
        )];

        // `:Row` labels the node, which is what gives the pattern anything to
        // bind `n` to in the first place — see
        // `an_entirely_unconstrained_node_binds_nothing`, a separate,
        // pre-existing gap this test deliberately avoids.
        let bound = evaluate(&flakes, "MATCH (n:Row) WITH n AS m RETURN m");

        assert_eq!(
            bound,
            vec!["<https://graph-owl.dev/ns/catalog#a>".to_string()],
            "?m must be bound from ?n, not merely named: {bound:?}"
        );
    }

    /// **A known, pre-existing gap this slice found but did not fix.**
    /// `lower_node` emits a triple pattern only for a label or an inline
    /// property; a node with neither — `MATCH (n) RETURN n`, the first query
    /// almost anyone would try — produces an *empty* BGP, which is SPARQL's
    /// one-row identity. `?n` is therefore never bound to anything, and the
    /// query returns one row with the column simply absent rather than an
    /// error or a real answer.
    ///
    /// No prior test caught this because every Slice B/C test before this one
    /// asserted the lowered plan's *shape*, never ran it. Fixing it needs a
    /// real design decision — what "any node" means over a triple store with
    /// no universal `rdf:type` — not a rushed patch alongside Slice E/F, so
    /// this pins the current behaviour rather than changing it. Recorded in
    /// `plans/07b-engine-cypher.md` for a future slice.
    #[test]
    fn an_entirely_unconstrained_node_binds_nothing() {
        use graph_owl_core::flake::{Flake, FlakeValue, Sid};

        let flakes = vec![Flake::assert(
            Sid::dsc("a"),
            Sid::dsc("type"),
            FlakeValue::Ref(Sid::dsc("Row")),
            1,
        )];

        let rendered = evaluate(&flakes, "MATCH (n) RETURN n");

        assert_eq!(
            rendered,
            vec![String::new()],
            "documents today's behaviour: one row, ?n absent from it — not a \
             row per node in the estate, which is what a reader would expect"
        );
    }

    /// **`SKIP` and `LIMIT` carry their actual numbers.** A lowering that
    /// returned a constant offset would satisfy "a Slice exists" and silently
    /// page wrongly.
    #[test]
    fn skip_and_limit_carry_their_values() {
        let plan = rendered(&lowered("MATCH (n) RETURN n SKIP 10 LIMIT 5"));

        assert!(plan.contains("start: 10"), "{plan}");
        assert!(plan.contains("length: Some(5)"), "{plan}");
    }

    /// Either alone still slices — the pair is an `||`, and an `&&` would drop
    /// a bare `LIMIT`, which is the commonest form of both.
    #[test]
    fn skip_alone_and_limit_alone_each_slice() {
        let limit_only = rendered(&lowered("MATCH (n) RETURN n LIMIT 5"));
        assert!(limit_only.contains("Slice"), "{limit_only}");
        assert!(limit_only.contains("start: 0"), "{limit_only}");
        assert!(limit_only.contains("length: Some(5)"), "{limit_only}");

        let skip_only = rendered(&lowered("MATCH (n) RETURN n SKIP 3"));
        assert!(skip_only.contains("Slice"), "{skip_only}");
        assert!(skip_only.contains("start: 3"), "{skip_only}");
        assert!(skip_only.contains("length: None"), "{skip_only}");
    }

    /// And no modifiers means no slice — a `Slice` that is always present would
    /// be a plan node the optimiser has to reason about for nothing.
    #[test]
    fn a_query_with_no_paging_has_no_slice() {
        assert!(!rendered(&lowered("MATCH (n) RETURN n")).contains("Slice"));
    }

    /// **A property nested inside a boolean still gets bound.** The collector
    /// recurses, and a missing recursion is invisible: the filter references a
    /// variable nothing binds, and the query returns no rows rather than erroring.
    #[test]
    fn properties_inside_boolean_and_negation_and_parentheses_are_bound() {
        for query in [
            "MATCH (n) WHERE n.a > 1 AND n.b < 2 RETURN n",
            "MATCH (n) WHERE NOT n.a = 1 RETURN n",
            "MATCH (n) WHERE (n.a = 1) RETURN n",
            "MATCH (n) WHERE n.a IS NULL RETURN n",
        ] {
            let plan = triples(&lowered(query));
            assert!(
                plan.iter().any(|t| t.ends_with("?n_a")),
                "`{query}` left ?n_a unbound: {plan:?}"
            );
        }
    }

    /// A bare variable in a comparison lowers as a variable — the arm that
    /// makes `WHERE a = b` work at all.
    #[test]
    fn a_bare_variable_comparison_lowers() {
        let plan = rendered(&lowered("MATCH (a) MATCH (b) WHERE a = b RETURN a"));

        assert!(plan.contains("Equal"), "{plan}");
        assert!(
            plan.contains("Variable(Variable { name: \"a\" })"),
            "{plan}"
        );
    }

    /// **`IS NULL` is unboundedness, not a comparison against a null value.**
    /// SPARQL has no null literal, so lowering it as one would compare against
    /// something that does not exist.
    #[test]
    fn is_null_lowers_to_boundedness() {
        let null = rendered(&lowered("MATCH (n) WHERE n.a IS NULL RETURN n"));
        assert!(null.contains("Bound"), "{null}");
        assert!(null.contains("Not("), "IS NULL is NOT BOUND: {null}");

        let not_null = rendered(&lowered("MATCH (n) WHERE n.a IS NOT NULL RETURN n"));
        assert!(not_null.contains("Bound"), "{not_null}");
    }

    /// And an explicit `= null` is refused rather than lowered into something
    /// that looks like a comparison and is not.
    #[test]
    fn an_explicit_null_comparison_is_refused() {
        assert_eq!(
            refused("MATCH (n) WHERE n.a = null RETURN n"),
            LoweringError::Unlowerable("a null literal in a comparison")
        );
    }

    /// **`IS NULL` on a bare variable**, not only on a property. The arm that
    /// handles it was unexercised until mutation testing said so.
    #[test]
    fn is_null_on_a_bare_variable_lowers() {
        let plan = rendered(&lowered("MATCH (a) WHERE a IS NULL RETURN a"));

        assert!(plan.contains("Bound"), "{plan}");
        assert!(plan.contains("name: \"a\""), "{plan}");
    }

    /// **The gate is the first line and lowering is the second.** A `UNION`
    /// reaching lowering means they disagree, and the honest answer is a
    /// refusal rather than lowering one branch and dropping the rest — which is
    /// what the arm removed here used to do.
    #[test]
    fn a_union_cannot_be_lowered_even_if_the_gate_let_it_through() {
        let query = "MATCH (n) RETURN n UNION MATCH (m) RETURN m";
        let parsed = decypher::parse(query).expect("valid Cypher");

        assert_eq!(
            lower(&parsed, query),
            Err(LoweringError::Unlowerable("this query shape")),
            "half a UNION is a wrong answer, not a partial one"
        );
    }

    #[test]
    fn not_lowers_to_a_negation() {
        assert!(rendered(&lowered("MATCH (n) WHERE NOT n.a = 1 RETURN n")).contains("Not"));
    }

    // ---- Slice F: aggregates ----

    #[test]
    fn count_star_lowers_to_count_solutions() {
        let plan = rendered(&lowered("MATCH (n) RETURN count(*)"));

        assert!(plan.contains("CountSolutions"), "{plan}");
    }

    /// **`count(*)` and `count(expr)` must lower to different aggregate
    /// forms**, because they answer different questions: `count(*)` counts
    /// rows, `count(expr)` counts non-null bindings of `expr`. Lowering both
    /// to `CountSolutions` would silently make `count(n.optional)` count rows
    /// that never bound the property at all.
    #[test]
    fn count_of_an_expression_lowers_to_a_function_call_not_count_solutions() {
        let plan = rendered(&lowered("MATCH (n) RETURN count(n)"));

        assert!(plan.contains("FunctionCall"), "{plan}");
        assert!(plan.contains("Count"), "{plan}");
        assert!(
            !plan.contains("CountSolutions"),
            "count(n) is not count(*): {plan}"
        );
    }

    #[test]
    fn count_distinct_sets_the_distinct_flag() {
        let with = rendered(&lowered("MATCH (n) RETURN count(DISTINCT n)"));
        let without = rendered(&lowered("MATCH (n) RETURN count(n)"));

        assert!(with.contains("distinct: true"), "{with}");
        assert!(without.contains("distinct: false"), "{without}");
    }

    #[test]
    fn sum_avg_min_max_lower_to_their_aggregate_functions() {
        for (query, function) in [
            ("MATCH (n) RETURN sum(n.amount)", "Sum"),
            ("MATCH (n) RETURN avg(n.amount)", "Avg"),
            ("MATCH (n) RETURN min(n.amount)", "Min"),
            ("MATCH (n) RETURN max(n.amount)", "Max"),
        ] {
            let plan = rendered(&lowered(query));
            assert!(plan.contains(function), "`{query}` -> {plan}");
        }
    }

    /// **The correctness trap this slice exists for.** `GROUP_CONCAT` folds
    /// values into one delimited *string*; Cypher's `collect()` returns a
    /// genuine list. Lowering one to the other would hand back a string where
    /// the caller asked for a list — silently, since both round-trip through
    /// JSON as *some* value.
    #[test]
    fn collect_is_refused_rather_than_approximated_as_group_concat() {
        let refused = refused("MATCH (n) RETURN collect(n.name)");

        assert!(
            matches!(refused, LoweringError::Unlowerable(_)),
            "{refused:?}"
        );
        assert!(
            refused.to_string().contains("list"),
            "names why, not just that it failed: {refused}"
        );
    }

    /// A function this engine does not know at all — aggregate or not — is
    /// refused even when aliased. An alias is a name, not a binding: nothing
    /// in the lowered pattern would bind `x`, so accepting it here would be a
    /// `Project` selecting an unbound variable instead of a refusal.
    #[test]
    fn an_unrecognised_function_call_is_refused_even_when_aliased() {
        assert!(matches!(
            refused("MATCH (n) RETURN toUpper(n.name) AS x"),
            LoweringError::Unlowerable(_)
        ));
        assert!(matches!(
            refused("MATCH (n) RETURN toUpper(n.name)"),
            LoweringError::Unlowerable(_)
        ));
    }

    /// **Implicit grouping**: every non-aggregated `RETURN` item is a grouping
    /// key, with no `GROUP BY` clause to write — that is Cypher's actual rule.
    #[test]
    fn non_aggregated_return_items_become_implicit_grouping_keys() {
        let plan = rendered(&lowered("MATCH (n) RETURN n.dept, count(n)"));

        assert!(plan.contains("Group"), "{plan}");
        assert!(
            plan.contains("variables: [Variable { name: \"n_dept\" }]"),
            "n.dept is the sole grouping key: {plan}"
        );
    }

    /// An aggregate with nothing else in `RETURN` groups the *whole* result —
    /// SPARQL's own reading of an empty `GROUP BY` list, reused rather than
    /// special-cased.
    #[test]
    fn an_aggregate_alone_groups_the_whole_result() {
        let plan = rendered(&lowered("MATCH (n) RETURN count(n)"));

        assert!(plan.contains("variables: []"), "{plan}");
    }

    /// **A renamed grouping key is bound by `Extend` before `Group` sees it.**
    /// `Group` can only pass an *existing* variable through; it cannot bind a
    /// new name. Asserting only that `Group` appears would pass even if the
    /// alias in the final projection referenced a name nothing bound.
    #[test]
    fn a_renamed_grouping_key_is_extended_before_grouping() {
        let plan = rendered(&lowered(
            "MATCH (n) RETURN n.dept AS department, count(n) AS c",
        ));

        assert!(plan.contains("Extend"), "{plan}");
        assert!(
            plan.contains("variable: Variable { name: \"department\" }"),
            "{plan}"
        );
        assert!(
            plan.contains("variables: [Variable { name: \"department\" }]"),
            "grouped by the renamed variable, not n_dept: {plan}"
        );
    }

    /// The aggregate's own output variable carries its alias — asserting only
    /// that `Group` exists would pass even if the aggregate bound the wrong
    /// name for the final projection to select.
    #[test]
    fn an_aggregates_output_variable_carries_its_alias() {
        let plan = rendered(&lowered("MATCH (n) RETURN count(n) AS total"));

        assert!(
            plan.contains("Variable { name: \"total\" }"),
            "the aggregate must bind the alias: {plan}"
        );
    }

    /// An unaliased aggregate still gets a usable name, mirroring the
    /// `{base}_{property}` convention an unaliased property lookup already
    /// uses.
    #[test]
    fn an_unaliased_aggregate_is_named_from_the_function_and_its_argument() {
        assert_eq!(
            projected_variables_of("MATCH (n) RETURN count(n)"),
            vec![Variable::new("count_n").expect("valid")]
        );
        assert_eq!(
            projected_variables_of("MATCH (n) RETURN count(*)"),
            vec![Variable::new("count_star").expect("valid")]
        );
    }

    fn projected_variables_of(query: &str) -> Vec<Variable> {
        let parsed = parse_subset(query).expect("in the subset");
        let QueryBody::SingleQuery(single) = &parsed.statements[0] else {
            panic!("a single query");
        };
        let SingleQueryKind::SinglePart(part) = &single.kind else {
            panic!("a single part");
        };
        let SinglePartBody::Return(returning) = &part.body else {
            panic!("a RETURN body");
        };
        super::projected_variables(&returning.items, query).expect("should project")
    }

    /// **Aggregates compose with `ORDER BY` and `LIMIT`**, ordering by the
    /// aggregate's own alias — the modifiers wrap the grouped projection
    /// exactly as they wrap a plain one.
    #[test]
    fn aggregates_compose_with_order_by_and_limit() {
        let plan = rendered(&lowered(
            "MATCH (n) RETURN n.dept AS dept, count(n) AS c ORDER BY c DESC LIMIT 5",
        ));

        // Outer wrappers render first in the derived `Debug` output, so
        // `Slice { inner: OrderBy { inner: Project { inner: Group { ...` means
        // `Slice` appears textually before `OrderBy`, which appears before
        // `Group` — the same convention `distinct_order_skip_and_limit_lower_in_cypher_order`
        // already relies on.
        let slice = plan.find("Slice").expect("a slice");
        let order = plan.find("OrderBy").expect("an order");
        let group = plan.find("Group").expect("a group");
        assert!(slice < order, "slicing wraps ordering: {plan}");
        assert!(order < group, "ordering wraps grouping: {plan}");
        assert!(plan.contains("Desc"), "{plan}");
        assert!(plan.contains("start: 0"), "{plan}");
        assert!(plan.contains("length: Some(5)"), "{plan}");
    }

    /// **The classic bug, run for real.** `count(*)` counts rows; `count(expr)`
    /// counts only rows where `expr` is bound. Over one node with an
    /// `optional` property that a second node lacks, the two must disagree —
    /// an algebra-shape assertion could not catch a lowering that collapsed
    /// both onto `CountSolutions`, only a real evaluation can.
    #[test]
    fn count_star_and_count_of_a_property_disagree_when_a_property_is_missing() {
        use graph_owl_core::flake::{Flake, FlakeValue, Sid};

        let flakes = vec![
            Flake::assert(
                Sid::dsc("a"),
                Sid::dsc("type"),
                FlakeValue::Ref(Sid::dsc("Row")),
                1,
            ),
            Flake::assert(
                Sid::dsc("a"),
                Sid::dsc("optional"),
                FlakeValue::String("present".into()),
                1,
            ),
            Flake::assert(
                Sid::dsc("b"),
                Sid::dsc("type"),
                FlakeValue::Ref(Sid::dsc("Row")),
                1,
            ),
            // `b` has no `optional` property at all.
        ];

        let star = evaluate(&flakes, "MATCH (n:Row) RETURN count(*) AS c");
        let of_property = evaluate(&flakes, "MATCH (n:Row) RETURN count(n.optional) AS c");

        let count = |rendered: &str| -> i64 {
            rendered
                .trim_matches('"')
                .split("^^")
                .next()
                .expect("a value")
                .trim_matches('"')
                .parse()
                .unwrap_or_else(|_| panic!("not an integer: {rendered}"))
        };

        assert_eq!(
            star.iter()
                .map(String::as_str)
                .map(count)
                .collect::<Vec<_>>(),
            vec![2],
            "count(*) counts every row: {star:?}"
        );
        assert_eq!(
            of_property
                .iter()
                .map(String::as_str)
                .map(count)
                .collect::<Vec<_>>(),
            vec![1],
            "count(n.optional) must not count the row where it is unbound: {of_property:?}"
        );
    }

    /// Runs a lowered Cypher query over real flakes through the same
    /// evaluator the catalog uses, and returns the sole column's rendered
    /// values — the round trip Slice E wires up at the API boundary.
    fn evaluate(flakes: &[graph_owl_core::flake::Flake], query: &str) -> Vec<String> {
        let pattern = lowered(query);
        let sparql_query = spargebra::Query::Select {
            dataset: None,
            pattern,
            base_iri: None,
        };
        let dataset = crate::dataset::FlakeDataset::from_flakes(flakes).expect("dataset");
        let results = spareval::QueryEvaluator::new()
            .prepare(&sparql_query)
            .execute(&dataset)
            .expect("evaluation should succeed");
        match results {
            spareval::QueryResults::Solutions(iter) => iter
                .map(|solution| {
                    let solution = solution.expect("solution");
                    solution
                        .iter()
                        .next()
                        .map(|(_, term)| term.to_string())
                        .unwrap_or_default()
                })
                .collect(),
            _ => panic!("a SELECT must yield solutions"),
        }
    }
}
