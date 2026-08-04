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

use oxrdf::{Literal, NamedNode, Variable};
use spargebra::algebra::{Expression, GraphPattern, OrderExpression};
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
/// # Errors
///
/// [`LoweringError`] naming the construct that has no lowering.
pub fn lower(query: &decypher::ast::query::Query) -> Result<GraphPattern, LoweringError> {
    let body = query
        .statements
        .first()
        .ok_or(LoweringError::Unlowerable("an empty query"))?;
    lower_body(body)
}

fn lower_body(body: &QueryBody) -> Result<GraphPattern, LoweringError> {
    match body {
        QueryBody::SingleQuery(single) => lower_single(&single.kind),
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

fn lower_single(kind: &SingleQueryKind) -> Result<GraphPattern, LoweringError> {
    match kind {
        SingleQueryKind::SinglePart(part) => lower_part(part),
        SingleQueryKind::MultiPart(multi) => {
            // **`WITH` is a pipeline boundary.** Each part's reading clauses
            // join onto what came before, and the `WITH` projects — which is
            // what scopes variables to the next part, exactly as Cypher says.
            let mut pattern: Option<GraphPattern> = None;
            for segment in &multi.parts {
                let reading = lower_reading(&segment.reading_clauses)?;
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
                )?);
            }
            let tail = lower_part(&multi.final_part)?;
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

fn lower_part(part: &SinglePartQuery) -> Result<GraphPattern, LoweringError> {
    let reading = lower_reading(&part.reading_clauses)?;
    match &part.body {
        SinglePartBody::Return(returning) => apply_return(reading, returning),
        // The subset gate refuses these; reaching here is a gate/lowering
        // disagreement rather than a user error.
        SinglePartBody::Updating { .. } => Err(LoweringError::Unlowerable("a write clause")),
        SinglePartBody::Finish(_) => Err(LoweringError::Unlowerable("FINISH")),
    }
}

/// The reading clauses of one query part, joined left to right.
fn lower_reading(clauses: &[ReadingClause]) -> Result<GraphPattern, LoweringError> {
    let mut pattern: Option<GraphPattern> = None;

    for clause in clauses {
        let (next, optional) = match clause {
            ReadingClause::Match(matching) => (lower_match(matching)?, matching.optional),
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
fn lower_match(matching: &Match) -> Result<GraphPattern, LoweringError> {
    let mut patterns = Vec::new();
    let mut relationship_variables = Vec::new();
    lower_pattern(
        &matching.pattern,
        &mut patterns,
        &mut relationship_variables,
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
) -> Result<(), LoweringError> {
    let variable = relationship_variable(relationship, relationships.len());
    relationships.push(variable.clone());
    let subject = TermPattern::Variable(variable);

    // Direction decides which endpoint is `from` and which is `to`. An
    // undirected pattern is not expressible as one BGP — it is a union — and is
    // reported rather than silently lowered as left-to-right, which would
    // return half the answer with no indication.
    let (from, to) = match relationship.direction {
        RelationshipDirection::Right => (left, right),
        RelationshipDirection::Left => (right, left),
        RelationshipDirection::Undirected | RelationshipDirection::Both => {
            return Err(LoweringError::Unlowerable(
                "an undirected relationship pattern",
            ));
        }
    };

    if let Some(detail) = &relationship.detail {
        if detail.range.is_some() {
            // Slice D's work. Refused at lowering rather than approximated,
            // because an approximation here silently changes the answer.
            return Err(LoweringError::Unlowerable(
                "a variable-length relationship pattern",
            ));
        }
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
fn apply_with(inner: GraphPattern, with: &With) -> Result<GraphPattern, LoweringError> {
    let mut properties = Vec::new();
    for item in &with.items {
        collect_properties(&item.expression, &mut properties);
    }
    let inner = with_property_bindings(inner, &properties)?;
    let variables = projected_variables(&with.items)?;
    let mut graph = GraphPattern::Project {
        inner: Box::new(inner),
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
fn apply_return(inner: GraphPattern, returning: &Return) -> Result<GraphPattern, LoweringError> {
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
    let variables = projected_variables(&returning.items)?;
    let mut graph = GraphPattern::Project {
        inner: Box::new(inner),
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
) -> Result<Vec<Variable>, LoweringError> {
    let mut variables = Vec::new();
    for item in items {
        let name = match (&item.alias, &item.expression) {
            (Some(alias), _) => alias.name.name.clone(),
            (None, CypherExpression::Variable(variable)) => variable.name.name.clone(),
            (None, CypherExpression::PropertyLookup { base, property, .. }) => {
                // `RETURN n.name` with no alias projects a variable named for
                // the path, which is what Cypher shows in its own result header.
                format!("{}_{}", base_name(base)?, property.name.name)
            }
            _ => return Err(LoweringError::Unlowerable("this projection item")),
        };
        variables
            .push(Variable::new(name).map_err(|_| LoweringError::Unlowerable("a variable name"))?);
    }
    if variables.is_empty() {
        return Err(LoweringError::NothingBound);
    }
    Ok(variables)
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
        lower(&parsed).unwrap_or_else(|e| panic!("{query} -> {e}"))
    }

    fn refused(query: &str) -> LoweringError {
        let parsed = parse_subset(query).expect("in the subset");
        lower(&parsed).expect_err("should not lower")
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

    #[test]
    fn a_variable_length_pattern_is_refused_at_lowering() {
        assert_eq!(
            refused("MATCH (a)-[r*1..3]->(b) RETURN a"),
            LoweringError::Unlowerable("a variable-length relationship pattern"),
            "Slice D's work — refused rather than approximated"
        );
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
        let parsed =
            decypher::parse("MATCH (n) RETURN n UNION MATCH (m) RETURN m").expect("valid Cypher");

        assert_eq!(
            lower(&parsed),
            Err(LoweringError::Unlowerable("this query shape")),
            "half a UNION is a wrong answer, not a partial one"
        );
    }

    #[test]
    fn not_lowers_to_a_negation() {
        assert!(rendered(&lowered("MATCH (n) WHERE NOT n.a = 1 RETURN n")).contains("Not"));
    }
}
