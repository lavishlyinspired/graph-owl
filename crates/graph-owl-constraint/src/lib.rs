//! Shape compilation and validation (pure, no I/O) — Epic 5.
//!
//! **Compile once, evaluate many.** A shape is turned into a [`CompiledShape`]
//! before it meets any data: regexes are parsed and a shape that cannot run is
//! rejected *then*, rather than failing halfway through a validation pass over
//! a live estate.
//!
//! **Validation never blocks a write.** It produces a report. The alternative —
//! rejecting writes that violate a shape — makes every shape change a potential
//! outage, and makes the catalog refuse to record the world as it actually is,
//! which is the one thing a catalog exists to do.

pub mod shapes;

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_ontology::{Constraint, DataType, NodeKind, Severity, Shape, Target};
use std::collections::HashMap;

/// A shape that could not be turned into something executable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileError {
    #[error("shape `{shape}` has a pattern that is not a valid regex: {detail}")]
    BadPattern { shape: String, detail: String },
    #[error("shape `{shape}` states no constraints")]
    Empty { shape: String },
}

/// A shape with everything expensive already done.
#[derive(Debug, Clone)]
pub struct CompiledShape {
    shape: Shape,
    /// Every `Pattern` in the shape, parsed once and anchored.
    patterns: HashMap<String, regex::Regex>,
}

impl CompiledShape {
    #[must_use]
    pub fn id(&self) -> &Sid {
        &self.shape.id
    }

    #[must_use]
    pub fn severity(&self) -> Severity {
        self.shape.severity
    }
}

/// Parse the regexes and reject a shape that cannot run.
///
/// **Patterns are anchored.** SHACL leaves `sh:pattern` unanchored; this
/// deliberately does not. A version check written as `\d+\.\d+` matches
/// *inside* `v1.2-draft`, so unanchored it passes exactly the values it exists
/// to catch. An author who wants a substring match writes `.*` and can be seen
/// to have meant it.
///
/// **An empty shape is refused.** A shape with no constraints conforms to
/// everything, which reads in a report as "checked and fine" when nothing was
/// checked at all.
///
/// # Errors
///
/// [`CompileError::BadPattern`] if any pattern fails to parse;
/// [`CompileError::Empty`] if the shape states no constraints.
pub fn compile(shape: &Shape) -> Result<CompiledShape, CompileError> {
    if shape.constraints.is_empty() {
        return Err(CompileError::Empty {
            shape: shape.id.to_string(),
        });
    }

    let mut patterns = HashMap::new();
    collect_patterns(&shape.constraints, &mut |regex| {
        if patterns.contains_key(regex) {
            return Ok(());
        }
        let anchored = format!("^(?:{regex})$");
        match regex::Regex::new(&anchored) {
            Ok(compiled) => {
                patterns.insert(regex.to_string(), compiled);
                Ok(())
            }
            Err(error) => Err(CompileError::BadPattern {
                shape: shape.id.to_string(),
                detail: error.to_string(),
            }),
        }
    })?;

    Ok(CompiledShape {
        shape: shape.clone(),
        patterns,
    })
}

/// Walk every constraint, including inside the combinators.
///
/// Recursive, because `Not(Or([Pattern, ...]))` hides a regex two levels down
/// and a compiler that only read the top level would accept a shape that fails
/// the first time that branch is evaluated — at validation time, against real
/// data, which is exactly when it must not fail.
fn collect_patterns(
    constraints: &[Constraint],
    visit: &mut impl FnMut(&str) -> Result<(), CompileError>,
) -> Result<(), CompileError> {
    for constraint in constraints {
        match constraint {
            Constraint::Pattern { regex, .. } => visit(regex)?,
            Constraint::Not(inner) => {
                collect_patterns(std::slice::from_ref(inner.as_ref()), visit)?;
            }
            Constraint::And(inner) | Constraint::Or(inner) => collect_patterns(inner, visit)?,
            _ => {}
        }
    }
    Ok(())
}

/// A suggested fix. **Never applied automatically.**
///
/// No `Serialize` here or on [`Violation`]: `Sid` and `FlakeValue` are domain
/// types with no wire format, and giving them one to serve this report would
/// let an HTTP shape leak into every flake in the system. The server renders
/// these, the same way it renders an explanation.
///
/// The catalog does not know whether a missing owner means "assign one" or
/// "this asset is deprecated and nobody should own it", and a repair applied on
/// its own authority makes the second case unrecoverable.
#[derive(Debug, Clone, PartialEq)]
pub enum Repair {
    AssertMissing { path: Sid, hint: String },
    RetractExcess { path: Sid, keep: usize },
    RetypeValue { path: Sid, to: DataType },
}

/// One thing that is wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    pub focus_node: Sid,
    pub path: Option<Sid>,
    /// Machine-matchable — `minCount`, `pattern`, and so on.
    pub constraint: String,
    pub shape: Sid,
    pub severity: Severity,
    pub message: String,
    /// The value that offended, when one value did. `None` for a count
    /// constraint, where the offence is the *absence* of a value.
    pub actual: Option<FlakeValue>,
    pub suggestion: Option<Repair>,
}

/// What a validation run found.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    /// `true` when nothing of `Violation` severity was found. Warnings and info
    /// do not fail conformance.
    pub conforms: bool,
    pub violations: Vec<Violation>,
}

impl ValidationReport {
    #[must_use]
    pub fn count_of(&self, severity: Severity) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == severity)
            .count()
    }
}

fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}

/// Live values of `path` on `subject`.
fn values<'a>(facts: &'a [Flake], subject: &Sid, path: &Sid) -> Vec<&'a FlakeValue> {
    facts
        .iter()
        .filter(|f| f.op && &f.s == subject && &f.p == path)
        .map(|f| &f.o)
        .collect()
}

/// Everything the shape applies to, in a stable order.
///
/// Sorted, because a report whose rows reshuffle between runs cannot be diffed,
/// and "did last night's run make this worse" is the question a violations
/// queue exists to answer.
fn focus_nodes(target: &Target, facts: &[Flake], shape: &Sid) -> Vec<Sid> {
    let mut found: Vec<Sid> = match target {
        Target::Subjects(subjects) => subjects.clone(),
        Target::Class(class) => facts
            .iter()
            .filter(|f| f.op && f.p == rdf_type() && f.o == FlakeValue::Ref(class.clone()))
            .map(|f| f.s.clone())
            .collect(),
        Target::SubjectsOf(path) => facts
            .iter()
            .filter(|f| f.op && &f.p == path)
            .map(|f| f.s.clone())
            .collect(),
        Target::ObjectsOf(path) => facts
            .iter()
            .filter(|f| f.op && &f.p == path)
            .filter_map(|f| match &f.o {
                FlakeValue::Ref(sid) => Some(sid.clone()),
                // A literal is not a node, so it cannot be a focus node.
                // Coercing one would make every constraint on it fail for a
                // reason nobody can fix.
                _ => None,
            })
            .collect(),
        // The subject that *holds* the literal, not the literal itself: a
        // literal has no identity to report a violation against, and "the
        // string `draft` is wrong" names nothing a steward can open.
        Target::LiteralsOf(dt) => facts
            .iter()
            .filter(|f| f.op && dt.matches(&f.o))
            .map(|f| f.s.clone())
            .collect(),
        // The shape's own id is the class. Saves a shape named after what it
        // constrains from repeating itself.
        Target::ImplicitClass => facts
            .iter()
            .filter(|f| f.op && f.p == rdf_type() && f.o == FlakeValue::Ref(shape.clone()))
            .map(|f| f.s.clone())
            .collect(),
    };
    found.sort_by_key(ToString::to_string);
    found.dedup();
    found
}

/// The numeric reading of a value, for the range constraints.
///
/// `Int` counts: a threshold written as `0.0` is about magnitude, and refusing
/// to compare it with `3` would make every range constraint depend on how the
/// value happened to be stored. `Datatype` is the tool for insisting on a
/// representation.
#[allow(clippy::cast_precision_loss)]
fn as_number(value: &FlakeValue) -> Option<f64> {
    match value {
        FlakeValue::Float(f) => Some(*f),
        FlakeValue::Int(i) => Some(*i as f64),
        FlakeValue::Duration(d) => Some(*d as f64),
        _ => None,
    }
}

/// The textual reading, for the length and pattern constraints.
///
/// Only genuinely textual values. Rendering an `Int` and measuring it would
/// make `MaxLength 2` reject `1000` — a constraint about digits nobody wrote.
fn as_text(value: &FlakeValue) -> Option<&str> {
    match value {
        FlakeValue::String(s) | FlakeValue::Json(s) => Some(s),
        _ => None,
    }
}

/// Everything `subject` is declared to be.
fn types_of(facts: &[Flake], subject: &Sid) -> Vec<Sid> {
    facts
        .iter()
        .filter(|f| f.op && &f.s == subject && f.p == rdf_type())
        .filter_map(|f| match &f.o {
            FlakeValue::Ref(sid) => Some(sid.clone()),
            _ => None,
        })
        .collect()
}

/// What one failure knows about itself before the shape dresses it up.
type Failure = (String, Option<Sid>, Option<FlakeValue>, Option<Repair>);

/// A single constraint against one focus node.
///
/// Returns **every** failure rather than the first: a report listing one
/// problem per node turns fixing an estate into whack-a-mole, and the same
/// decision was already taken for field validation in Epic 1.
/// Does one value satisfy one per-value constraint?
///
/// Split out from [`evaluate`] because these twelve share a shape — read the
/// values, check each — and the cardinality constraints and combinators do not.
/// Keeping them together made one function long enough that nobody would read
/// it rule by rule, which is the same argument the reasoner's eight rules rest
/// on.
///
/// Returns `None` for a constraint that is not per-value.
fn value_conforms(
    compiled: &CompiledShape,
    constraint: &Constraint,
    value: &FlakeValue,
    facts: &[Flake],
) -> Option<bool> {
    Some(match constraint {
        Constraint::Datatype { dt, .. } => dt.matches(value),
        Constraint::NodeKind { kind, .. } => {
            matches!(value, FlakeValue::Ref(_)) == (*kind == NodeKind::Ref)
        }
        Constraint::In { allowed, .. } => allowed.contains(value),
        Constraint::Pattern { regex, .. } => {
            // A non-textual value cannot match a pattern, and calling it
            // conforming would let `version = 3` past a format check.
            as_text(value).is_some_and(|text| {
                compiled
                    .patterns
                    .get(regex)
                    .is_some_and(|regex| regex.is_match(text))
            })
        }
        Constraint::MinInclusive { v, .. } => as_number(value).is_some_and(|n| n >= *v),
        Constraint::MaxInclusive { v, .. } => as_number(value).is_some_and(|n| n <= *v),
        Constraint::MinLength { n, .. } => as_text(value).is_some_and(|t| t.chars().count() >= *n),
        Constraint::MaxLength { n, .. } => as_text(value).is_some_and(|t| t.chars().count() <= *n),
        Constraint::Class { expected, .. } => match value {
            FlakeValue::Ref(target) => types_of(facts, target).contains(expected),
            _ => false,
        },
        _ => return None,
    })
}

/// The repair worth suggesting when a per-value constraint fails.
fn repair_for(constraint: &Constraint) -> Option<Repair> {
    match constraint {
        Constraint::Datatype { path, dt } => Some(Repair::RetypeValue {
            path: path.clone(),
            to: *dt,
        }),
        // The others have no mechanical fix: a value outside an allowed set, or
        // failing a pattern, needs a person to decide what it should have been.
        // Suggesting "make it match" would be restating the violation.
        _ => None,
    }
}

/// A single constraint against one focus node.
///
/// Returns **every** failure rather than the first: a report listing one
/// problem per node turns fixing an estate into whack-a-mole, and the same
/// decision was already taken for field validation in Epic 1.
fn evaluate(
    compiled: &CompiledShape,
    constraint: &Constraint,
    subject: &Sid,
    facts: &[Flake],
) -> Vec<Failure> {
    let token = constraint.kind().to_string();
    let mut failures = Vec::new();

    match constraint {
        Constraint::MinCount { path, n } => {
            let held = values(facts, subject, path).len();
            if held < *n {
                failures.push((
                    token,
                    Some(path.clone()),
                    None,
                    Some(Repair::AssertMissing {
                        path: path.clone(),
                        hint: format!("at least {n} required, {held} present"),
                    }),
                ));
            }
        }
        Constraint::MaxCount { path, n } => {
            if values(facts, subject, path).len() > *n {
                failures.push((
                    token,
                    Some(path.clone()),
                    None,
                    Some(Repair::RetractExcess {
                        path: path.clone(),
                        keep: *n,
                    }),
                ));
            }
        }
        Constraint::HasValue { path, v } => {
            if !values(facts, subject, path)
                .into_iter()
                .any(|held| held == v)
            {
                failures.push((
                    token,
                    Some(path.clone()),
                    None,
                    Some(Repair::AssertMissing {
                        path: path.clone(),
                        hint: format!("{v:?} is required"),
                    }),
                ));
            }
        }
        Constraint::Not(inner) => {
            if evaluate(compiled, inner, subject, facts).is_empty() {
                failures.push((token, inner.path().cloned(), None, None));
            }
        }
        Constraint::And(inner) => {
            for branch in inner {
                failures.extend(evaluate(compiled, branch, subject, facts));
            }
        }
        Constraint::Or(inner) => {
            // **One violation, not one per branch.** An `Or` over four branches
            // that all fail is one thing wrong, and reporting four makes the
            // queue's size a function of how the shape was written rather than
            // of how bad the data is.
            let any = inner
                .iter()
                .any(|branch| evaluate(compiled, branch, subject, facts).is_empty());
            if !any {
                failures.push((token, None, None, None));
            }
        }
        // The twelve per-value constraints, all of the same shape: read the
        // values, check each. A constraint over *no* values conforms
        // vacuously — `MinCount` is the tool for requiring one, and checking it
        // here as well would make every constraint implicitly mandatory.
        per_value => {
            if let Some(path) = per_value.path() {
                for value in values(facts, subject, path) {
                    if value_conforms(compiled, per_value, value, facts) == Some(false) {
                        failures.push((
                            token.clone(),
                            Some(path.clone()),
                            Some(value.clone()),
                            repair_for(per_value),
                        ));
                    }
                }
            }
        }
    }

    failures
}

/// Check every shape against every node it targets.
///
/// Pure: `facts` is everything about the focus nodes, and the caller fetched it.
#[must_use]
pub fn validate(shapes: &[CompiledShape], facts: &[Flake]) -> ValidationReport {
    let mut violations = Vec::new();

    for compiled in shapes {
        for subject in focus_nodes(&compiled.shape.target, facts, &compiled.shape.id) {
            for constraint in &compiled.shape.constraints {
                for (token, path, actual, suggestion) in
                    evaluate(compiled, constraint, &subject, facts)
                {
                    let message = compiled.shape.message.clone().unwrap_or_else(|| {
                        describe(&token, path.as_ref(), actual.as_ref(), &compiled.shape.id)
                    });
                    violations.push(Violation {
                        focus_node: subject.clone(),
                        path,
                        constraint: token,
                        shape: compiled.shape.id.clone(),
                        severity: compiled.shape.severity,
                        message,
                        actual,
                        suggestion,
                    });
                }
            }
        }
    }

    ValidationReport {
        conforms: !violations.iter().any(|v| v.severity == Severity::Violation),
        violations,
    }
}

fn describe(token: &str, path: Option<&Sid>, actual: Option<&FlakeValue>, shape: &Sid) -> String {
    match (path, actual) {
        (Some(path), Some(actual)) => format!("{shape}: {path} fails {token} ({actual:?})"),
        (Some(path), None) => format!("{shape}: {path} fails {token}"),
        _ => format!("{shape}: fails {token}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: &str) -> Sid {
        Sid::dsc(id)
    }

    fn fact(s: &str, p: &str, o: FlakeValue) -> Flake {
        Flake::assert(a(s), a(p), o, 1)
    }

    fn typed(s: &str, class: &str) -> Flake {
        Flake {
            s: a(s),
            p: Sid::new(namespace::RDF, "type"),
            o: FlakeValue::Ref(a(class)),
            cx: None,
            t: 1,
            op: true,
        }
    }

    /// A shape over one constraint, targeting everything of type `Table`.
    fn shape_of(constraint: Constraint) -> CompiledShape {
        compile(&Shape {
            id: a("TableShape"),
            target: Target::Class(a("Table")),
            constraints: vec![constraint],
            severity: Severity::Violation,
            message: None,
        })
        .expect("the shape should compile")
    }

    fn run(constraint: Constraint, facts: &[Flake]) -> ValidationReport {
        validate(&[shape_of(constraint)], facts)
    }

    /// Every constraint kind, each with a conforming case *and* a violating
    /// one. The pair is the point: a validator that always conforms passes
    /// every positive case, and one that never conforms passes every negative.
    mod each_constraint {
        use super::*;

        #[test]
        fn min_count_wants_at_least_that_many() {
            let none = vec![typed("t", "Table")];
            let one = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("orders".into())),
            ];
            let constraint = || Constraint::MinCount {
                path: a("name"),
                n: 1,
            };

            assert!(!run(constraint(), &none).conforms);
            assert!(run(constraint(), &one).conforms);
        }

        #[test]
        fn max_count_wants_at_most_that_many() {
            let one = vec![
                typed("t", "Table"),
                fact("t", "fqn", FlakeValue::String("a".into())),
            ];
            let two = vec![
                typed("t", "Table"),
                fact("t", "fqn", FlakeValue::String("a".into())),
                fact("t", "fqn", FlakeValue::String("b".into())),
            ];
            let constraint = || Constraint::MaxCount {
                path: a("fqn"),
                n: 1,
            };

            assert!(run(constraint(), &one).conforms);
            assert!(!run(constraint(), &two).conforms);
        }

        /// **Inclusive means inclusive.** `0.0` conforms to `MinInclusive 0.0`;
        /// a `>` implementation rejects it, and a confidence of exactly zero is
        /// a real value that a strict comparison quietly outlaws.
        #[test]
        fn min_inclusive_admits_the_boundary_and_rejects_below_it() {
            let constraint = || Constraint::MinInclusive {
                path: a("confidence"),
                v: 0.0,
            };
            let at = vec![
                typed("t", "Table"),
                fact("t", "confidence", FlakeValue::Float(0.0)),
            ];
            let below = vec![
                typed("t", "Table"),
                fact("t", "confidence", FlakeValue::Float(-0.1)),
            ];

            assert!(run(constraint(), &at).conforms, "0.0 is not below 0.0");
            assert!(!run(constraint(), &below).conforms);
        }

        #[test]
        fn max_inclusive_admits_the_boundary_and_rejects_above_it() {
            let constraint = || Constraint::MaxInclusive {
                path: a("confidence"),
                v: 1.0,
            };
            let at = vec![
                typed("t", "Table"),
                fact("t", "confidence", FlakeValue::Float(1.0)),
            ];
            let above = vec![
                typed("t", "Table"),
                fact("t", "confidence", FlakeValue::Float(1.1)),
            ];

            assert!(run(constraint(), &at).conforms);
            assert!(!run(constraint(), &above).conforms);
        }

        #[test]
        fn max_length_counts_characters_and_admits_the_boundary() {
            let constraint = || Constraint::MaxLength {
                path: a("name"),
                n: 10,
            };
            let exactly = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("0123456789".into())),
            ];
            let over = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("01234567890".into())),
            ];

            assert!(run(constraint(), &exactly).conforms, "10 is not over 10");
            assert!(!run(constraint(), &over).conforms);
        }

        #[test]
        fn min_length_admits_the_boundary() {
            let constraint = || Constraint::MinLength {
                path: a("name"),
                n: 3,
            };
            let exactly = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("abc".into())),
            ];
            let under = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("ab".into())),
            ];

            assert!(run(constraint(), &exactly).conforms);
            assert!(!run(constraint(), &under).conforms);
        }

        /// Characters, not bytes. Three CJK characters are three characters
        /// long to everyone except a byte count, which calls them nine and
        /// rejects the value.
        #[test]
        fn length_is_measured_in_characters_not_bytes() {
            let facts = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("日本語".into())),
            ];

            assert!(
                run(
                    Constraint::MaxLength {
                        path: a("name"),
                        n: 3
                    },
                    &facts
                )
                .conforms
            );
        }

        /// **The pattern is anchored.** Unanchored, `\d+\.\d+` matches inside
        /// `v1.2-draft` and the constraint passes exactly the value it exists
        /// to reject.
        #[test]
        fn a_pattern_matches_the_whole_value_not_a_substring() {
            let constraint = || Constraint::Pattern {
                path: a("version"),
                regex: r"\d+\.\d+".into(),
            };
            let clean = vec![
                typed("t", "Table"),
                fact("t", "version", FlakeValue::String("1.2".into())),
            ];
            let embedded = vec![
                typed("t", "Table"),
                fact("t", "version", FlakeValue::String("v1.2-draft".into())),
            ];

            assert!(run(constraint(), &clean).conforms);
            assert!(
                !run(constraint(), &embedded).conforms,
                "an unanchored pattern passes the value it exists to reject"
            );
        }

        /// A non-textual value cannot match a pattern. Treating it as
        /// conforming lets `version = 3` past a version format check.
        #[test]
        fn a_non_textual_value_never_matches_a_pattern() {
            let facts = vec![
                typed("t", "Table"),
                fact("t", "version", FlakeValue::Int(3)),
            ];

            assert!(
                !run(
                    Constraint::Pattern {
                        path: a("version"),
                        regex: r"\d+\.\d+".into()
                    },
                    &facts
                )
                .conforms
            );
        }

        #[test]
        fn in_admits_the_listed_values_and_refuses_the_rest() {
            let constraint = || Constraint::In {
                path: a("relType"),
                allowed: vec![
                    FlakeValue::String("feeds".into()),
                    FlakeValue::String("derivedFrom".into()),
                ],
            };
            let listed = vec![
                typed("t", "Table"),
                fact("t", "relType", FlakeValue::String("feeds".into())),
            ];
            let unlisted = vec![
                typed("t", "Table"),
                fact("t", "relType", FlakeValue::String("influences".into())),
            ];

            assert!(run(constraint(), &listed).conforms);
            assert!(!run(constraint(), &unlisted).conforms);
        }

        #[test]
        fn datatype_refuses_a_value_of_the_wrong_kind() {
            let constraint = || Constraint::Datatype {
                path: a("ordinalPosition"),
                dt: DataType::Int,
            };
            let right = vec![
                typed("t", "Table"),
                fact("t", "ordinalPosition", FlakeValue::Int(0)),
            ];
            let wrong = vec![
                typed("t", "Table"),
                fact("t", "ordinalPosition", FlakeValue::String("0".into())),
            ];

            assert!(run(constraint(), &right).conforms);
            assert!(!run(constraint(), &wrong).conforms);
        }

        #[test]
        fn node_kind_separates_a_reference_from_a_literal() {
            let wants_ref = || Constraint::NodeKind {
                path: a("parentSchema"),
                kind: NodeKind::Ref,
            };
            let reference = vec![
                typed("t", "Table"),
                fact("t", "parentSchema", FlakeValue::Ref(a("s"))),
            ];
            let literal = vec![
                typed("t", "Table"),
                fact("t", "parentSchema", FlakeValue::String("s".into())),
            ];

            assert!(run(wants_ref(), &reference).conforms);
            assert!(!run(wants_ref(), &literal).conforms);
            // And the mirror, so a rule that ignored `kind` could not pass both.
            let wants_literal = || Constraint::NodeKind {
                path: a("parentSchema"),
                kind: NodeKind::Literal,
            };
            assert!(!run(wants_literal(), &reference).conforms);
            assert!(run(wants_literal(), &literal).conforms);
        }

        #[test]
        fn class_checks_the_type_of_what_the_path_points_at() {
            let constraint = || Constraint::Class {
                path: a("parentSchema"),
                expected: a("Schema"),
            };
            let right = vec![
                typed("t", "Table"),
                fact("t", "parentSchema", FlakeValue::Ref(a("s"))),
                typed("s", "Schema"),
            ];
            let wrong = vec![
                typed("t", "Table"),
                fact("t", "parentSchema", FlakeValue::Ref(a("s"))),
                typed("s", "Database"),
            ];

            assert!(run(constraint(), &right).conforms);
            assert!(!run(constraint(), &wrong).conforms);
        }

        #[test]
        fn has_value_wants_that_exact_value_present() {
            let constraint = || Constraint::HasValue {
                path: a("lifecycle"),
                v: FlakeValue::String("production".into()),
            };
            let present = vec![
                typed("t", "Table"),
                fact("t", "lifecycle", FlakeValue::String("draft".into())),
                fact("t", "lifecycle", FlakeValue::String("production".into())),
            ];
            let absent = vec![
                typed("t", "Table"),
                fact("t", "lifecycle", FlakeValue::String("draft".into())),
            ];

            assert!(run(constraint(), &present).conforms);
            assert!(!run(constraint(), &absent).conforms);
        }
    }

    mod the_combinators {
        use super::*;

        #[test]
        fn not_inverts_its_inner_constraint() {
            let constraint = || {
                Constraint::Not(Box::new(Constraint::MinCount {
                    path: a("deprecated"),
                    n: 1,
                }))
            };
            let absent = vec![typed("t", "Table")];
            let present = vec![
                typed("t", "Table"),
                fact("t", "deprecated", FlakeValue::Boolean(true)),
            ];

            assert!(run(constraint(), &absent).conforms, "not(minCount 1) holds");
            assert!(!run(constraint(), &present).conforms);
        }

        #[test]
        fn and_needs_every_branch() {
            let constraint = || {
                Constraint::And(vec![
                    Constraint::MinCount {
                        path: a("name"),
                        n: 1,
                    },
                    Constraint::MinCount {
                        path: a("owner"),
                        n: 1,
                    },
                ])
            };
            let both = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("orders".into())),
                fact("t", "owner", FlakeValue::Ref(a("finance"))),
            ];
            let one = vec![
                typed("t", "Table"),
                fact("t", "name", FlakeValue::String("orders".into())),
            ];

            assert!(run(constraint(), &both).conforms);
            assert!(!run(constraint(), &one).conforms);
        }

        #[test]
        fn or_needs_one_branch() {
            let constraint = || {
                Constraint::Or(vec![
                    Constraint::MinCount {
                        path: a("owner"),
                        n: 1,
                    },
                    Constraint::MinCount {
                        path: a("steward"),
                        n: 1,
                    },
                ])
            };
            let one = vec![
                typed("t", "Table"),
                fact("t", "steward", FlakeValue::Ref(a("ops"))),
            ];
            let neither = vec![typed("t", "Table")];

            assert!(run(constraint(), &one).conforms);
            assert!(!run(constraint(), &neither).conforms);
        }

        /// **One violation, not one per branch.** Three failing branches is one
        /// thing wrong; reporting three makes the queue's size a function of
        /// how the shape was written rather than of how bad the data is.
        #[test]
        fn an_or_with_every_branch_failing_reports_once() {
            let constraint = Constraint::Or(vec![
                Constraint::MinCount {
                    path: a("owner"),
                    n: 1,
                },
                Constraint::MinCount {
                    path: a("steward"),
                    n: 1,
                },
                Constraint::MinCount {
                    path: a("custodian"),
                    n: 1,
                },
            ]);

            let report = run(constraint, &[typed("t", "Table")]);

            assert_eq!(report.violations.len(), 1, "{:#?}", report.violations);
            assert_eq!(report.violations[0].constraint, "or");
        }

        /// De Morgan, as a differential test: `not(and(a, b))` and
        /// `or(not a, not b)` are the same statement, so an implementation
        /// where they disagree has a bug in one of the three.
        #[test]
        fn not_of_an_and_agrees_with_an_or_of_nots() {
            let has = |path: &str| Constraint::MinCount {
                path: a(path),
                n: 1,
            };
            let left = Constraint::Not(Box::new(Constraint::And(vec![has("name"), has("owner")])));
            let right = Constraint::Or(vec![
                Constraint::Not(Box::new(has("name"))),
                Constraint::Not(Box::new(has("owner"))),
            ]);

            for facts in [
                vec![typed("t", "Table")],
                vec![
                    typed("t", "Table"),
                    fact("t", "name", FlakeValue::String("x".into())),
                ],
                vec![
                    typed("t", "Table"),
                    fact("t", "name", FlakeValue::String("x".into())),
                    fact("t", "owner", FlakeValue::Ref(a("f"))),
                ],
            ] {
                assert_eq!(
                    run(left.clone(), &facts).conforms,
                    run(right.clone(), &facts).conforms,
                    "de Morgan disagrees on {facts:#?}"
                );
            }
        }
    }

    mod what_a_report_says {
        use super::*;

        /// A warning does not fail conformance. If it did, severities would be
        /// decoration and every shape would be blocking.
        #[test]
        fn a_warning_is_reported_without_failing_conformance() {
            let shape = compile(&Shape {
                id: a("AdviceShape"),
                target: Target::Class(a("Table")),
                constraints: vec![Constraint::MinCount {
                    path: a("description"),
                    n: 1,
                }],
                severity: Severity::Warning,
                message: None,
            })
            .expect("compiles");

            let report = validate(&[shape], &[typed("t", "Table")]);

            assert!(report.conforms, "a warning must not fail conformance");
            assert_eq!(report.violations.len(), 1, "but it must still be reported");
            assert_eq!(report.count_of(Severity::Warning), 1);
            assert_eq!(report.count_of(Severity::Violation), 0);
        }

        /// The offending value is carried. "Something is wrong with this
        /// column's type" is not actionable; "it holds the string `first`" is.
        #[test]
        fn a_violation_carries_the_value_that_offended() {
            let report = run(
                Constraint::Datatype {
                    path: a("ordinalPosition"),
                    dt: DataType::Int,
                },
                &[
                    typed("t", "Table"),
                    fact("t", "ordinalPosition", FlakeValue::String("first".into())),
                ],
            );

            assert_eq!(
                report.violations[0].actual,
                Some(FlakeValue::String("first".into()))
            );
            assert_eq!(report.violations[0].focus_node, a("t"));
            assert_eq!(report.violations[0].path, Some(a("ordinalPosition")));
            assert_eq!(report.violations[0].shape, a("TableShape"));
        }

        /// A count violation carries **no** value, because the offence is the
        /// absence of one. Inventing a value here would name a fact nobody
        /// asserted.
        #[test]
        fn a_missing_value_is_reported_without_inventing_one() {
            let report = run(
                Constraint::MinCount {
                    path: a("name"),
                    n: 1,
                },
                &[typed("t", "Table")],
            );

            assert_eq!(report.violations[0].actual, None);
            assert_eq!(
                report.violations[0].suggestion,
                Some(Repair::AssertMissing {
                    path: a("name"),
                    hint: "at least 1 required, 0 present".to_string(),
                })
            );
        }

        /// A wrong type suggests retyping — the only per-value constraint with
        /// a mechanical fix, and the reason `Repair` has three variants rather
        /// than two.
        #[test]
        fn a_wrongly_typed_value_suggests_retyping_it() {
            let report = run(
                Constraint::Datatype {
                    path: a("ordinalPosition"),
                    dt: DataType::Int,
                },
                &[
                    typed("t", "Table"),
                    fact("t", "ordinalPosition", FlakeValue::String("first".into())),
                ],
            );

            assert_eq!(
                report.violations[0].suggestion,
                Some(Repair::RetypeValue {
                    path: a("ordinalPosition"),
                    to: DataType::Int
                })
            );
        }

        /// And the negative: a constraint with no mechanical fix suggests
        /// nothing. "Make it match the pattern" restates the violation, and a
        /// suggestion that says nothing trains a steward to ignore the column.
        #[test]
        fn a_constraint_with_no_mechanical_fix_suggests_nothing() {
            let report = run(
                Constraint::Pattern {
                    path: a("version"),
                    regex: r"\d+\.\d+".into(),
                },
                &[
                    typed("t", "Table"),
                    fact("t", "version", FlakeValue::String("draft".into())),
                ],
            );

            assert_eq!(report.violations[0].suggestion, None);
        }

        #[test]
        fn too_many_values_suggests_keeping_the_allowed_number() {
            let report = run(
                Constraint::MaxCount {
                    path: a("fqn"),
                    n: 1,
                },
                &[
                    typed("t", "Table"),
                    fact("t", "fqn", FlakeValue::String("a".into())),
                    fact("t", "fqn", FlakeValue::String("b".into())),
                ],
            );

            assert_eq!(
                report.violations[0].suggestion,
                Some(Repair::RetractExcess {
                    path: a("fqn"),
                    keep: 1
                })
            );
        }

        /// Every problem, not the first. A report naming one issue per node
        /// makes fixing an estate whack-a-mole — the same decision Epic 1 took
        /// for field validation.
        #[test]
        fn a_node_with_three_problems_reports_all_three() {
            let shape = compile(&Shape {
                id: a("TableShape"),
                target: Target::Class(a("Table")),
                constraints: vec![
                    Constraint::MinCount {
                        path: a("name"),
                        n: 1,
                    },
                    Constraint::MinCount {
                        path: a("fqn"),
                        n: 1,
                    },
                    Constraint::MinCount {
                        path: a("owner"),
                        n: 1,
                    },
                ],
                severity: Severity::Violation,
                message: None,
            })
            .expect("compiles");

            let report = validate(&[shape], &[typed("t", "Table")]);

            assert_eq!(report.violations.len(), 3, "{:#?}", report.violations);
        }

        /// An author's message wins. The validator knows a rule failed; the
        /// author knows why the rule exists.
        #[test]
        fn a_shape_message_replaces_the_generated_one() {
            let shape = compile(&Shape {
                id: a("RegulatoryShape"),
                target: Target::Class(a("Table")),
                constraints: vec![Constraint::MinCount {
                    path: a("owner"),
                    n: 1,
                }],
                severity: Severity::Violation,
                message: Some("every regulatory table needs an owner".into()),
            })
            .expect("compiles");

            let report = validate(&[shape], &[typed("t", "Table")]);

            assert_eq!(
                report.violations[0].message,
                "every regulatory table needs an owner"
            );
        }

        /// And without one, the generated message names the shape, the path and
        /// the rule — enough to act on without opening the shape.
        #[test]
        fn a_generated_message_names_the_shape_the_path_and_the_rule() {
            let report = run(
                Constraint::MinCount {
                    path: a("name"),
                    n: 1,
                },
                &[typed("t", "Table")],
            );

            let message = &report.violations[0].message;
            assert!(message.contains("TableShape"), "{message}");
            assert!(message.contains("name"), "{message}");
            assert!(message.contains("minCount"), "{message}");
        }

        /// An estate with nothing wrong conforms and says nothing. A report
        /// that always found something would make the queue meaningless.
        #[test]
        fn a_conforming_estate_produces_an_empty_report() {
            let report = run(
                Constraint::MinCount {
                    path: a("name"),
                    n: 1,
                },
                &[
                    typed("t", "Table"),
                    fact("t", "name", FlakeValue::String("orders".into())),
                ],
            );

            assert!(report.conforms);
            assert!(report.violations.is_empty());
        }
    }

    mod what_a_shape_targets {
        use super::*;

        fn owner_required(target: Target) -> CompiledShape {
            compile(&Shape {
                id: a("S"),
                target,
                constraints: vec![Constraint::MinCount {
                    path: a("owner"),
                    n: 1,
                }],
                severity: Severity::Violation,
                message: None,
            })
            .expect("compiles")
        }

        /// A class target reaches its instances and nothing else. Reaching
        /// everything would report a violation on every node in the estate for
        /// a rule about tables.
        #[test]
        fn a_class_target_reaches_only_that_class() {
            let facts = vec![typed("t", "Table"), typed("c", "Column")];

            let report = validate(&[owner_required(Target::Class(a("Table")))], &facts);

            assert_eq!(report.violations.len(), 1);
            assert_eq!(report.violations[0].focus_node, a("t"));
        }

        #[test]
        fn a_subject_list_reaches_exactly_the_named_nodes() {
            let facts = vec![typed("t", "Table"), typed("u", "Table")];

            let report = validate(&[owner_required(Target::Subjects(vec![a("t")]))], &facts);

            assert_eq!(report.violations.len(), 1);
            assert_eq!(report.violations[0].focus_node, a("t"));
        }

        #[test]
        fn subjects_of_reaches_whatever_states_the_predicate() {
            let facts = vec![
                fact("t", "fqn", FlakeValue::String("a".into())),
                typed("u", "Table"),
            ];

            let report = validate(&[owner_required(Target::SubjectsOf(a("fqn")))], &facts);

            assert_eq!(report.violations.len(), 1);
            assert_eq!(report.violations[0].focus_node, a("t"));
        }

        #[test]
        fn objects_of_reaches_the_far_end_of_the_predicate() {
            let facts = vec![fact("t", "parentSchema", FlakeValue::Ref(a("s")))];

            let report = validate(
                &[owner_required(Target::ObjectsOf(a("parentSchema")))],
                &facts,
            );

            assert_eq!(report.violations.len(), 1);
            assert_eq!(report.violations[0].focus_node, a("s"));
        }

        /// A literal is not a node. Coercing one into a focus node would make
        /// every constraint on it fail for a reason nobody can fix.
        #[test]
        fn objects_of_skips_literals() {
            let facts = vec![fact("t", "name", FlakeValue::String("orders".into()))];

            let report = validate(&[owner_required(Target::ObjectsOf(a("name")))], &facts);

            assert!(report.conforms, "{:#?}", report.violations);
        }

        /// A retracted fact is not a fact. Validating over one reports on data
        /// the graph no longer states.
        #[test]
        fn a_retracted_value_does_not_satisfy_a_constraint() {
            let mut retracted = fact("t", "name", FlakeValue::String("orders".into()));
            retracted.op = false;
            let facts = vec![typed("t", "Table"), retracted];

            let report = run(
                Constraint::MinCount {
                    path: a("name"),
                    n: 1,
                },
                &facts,
            );

            assert!(!report.conforms, "a withdrawn name is not a name");
        }

        /// Focus nodes come out in a stable order, so two runs over the same
        /// estate produce diffable reports.
        #[test]
        fn the_same_estate_reports_in_the_same_order_every_run() {
            let facts = vec![
                typed("c", "Table"),
                typed("a", "Table"),
                typed("b", "Table"),
            ];
            let shape = || owner_required(Target::Class(a("Table")));

            let first: Vec<Sid> = validate(&[shape()], &facts)
                .violations
                .into_iter()
                .map(|v| v.focus_node)
                .collect();

            assert_eq!(first, vec![a("a"), a("b"), a("c")]);
            let second: Vec<Sid> = validate(&[shape()], &facts)
                .violations
                .into_iter()
                .map(|v| v.focus_node)
                .collect();
            assert_eq!(first, second);
        }
    }

    mod compilation {
        use super::*;

        /// A bad regex is caught when the shape is compiled, not when it first
        /// meets data. That is the whole point of compiling once: a broken
        /// shape cannot fail halfway through a run over a live estate.
        #[test]
        fn a_shape_with_an_unparseable_pattern_is_refused() {
            let outcome = compile(&Shape {
                id: a("Broken"),
                target: Target::Class(a("Table")),
                constraints: vec![Constraint::Pattern {
                    path: a("version"),
                    regex: "[unclosed".into(),
                }],
                severity: Severity::Violation,
                message: None,
            });

            assert!(
                matches!(outcome, Err(CompileError::BadPattern { .. })),
                "{outcome:?}"
            );
        }

        /// Including one buried inside a combinator — a compiler that only read
        /// the top level would accept this and fail at validation time, against
        /// real data, which is exactly when it must not.
        #[test]
        fn a_bad_pattern_nested_in_a_combinator_is_still_refused() {
            let outcome = compile(&Shape {
                id: a("Broken"),
                target: Target::Class(a("Table")),
                constraints: vec![Constraint::Not(Box::new(Constraint::Or(vec![
                    Constraint::MinCount {
                        path: a("name"),
                        n: 1,
                    },
                    Constraint::Pattern {
                        path: a("version"),
                        regex: "(".into(),
                    },
                ])))],
                severity: Severity::Violation,
                message: None,
            });

            assert!(
                matches!(outcome, Err(CompileError::BadPattern { .. })),
                "{outcome:?}"
            );
        }

        /// A shape with no constraints conforms to everything, which reads in a
        /// report as "checked and fine" when nothing was checked.
        #[test]
        fn a_shape_with_no_constraints_is_refused() {
            let outcome = compile(&Shape {
                id: a("Empty"),
                target: Target::Class(a("Table")),
                constraints: vec![],
                severity: Severity::Violation,
                message: None,
            });

            assert!(
                matches!(outcome, Err(CompileError::Empty { .. })),
                "{outcome:?}"
            );
        }

        /// The error names the shape. A compile failure that says only "bad
        /// regex" leaves an operator grepping a migration for parentheses.
        #[test]
        fn a_compile_error_names_the_shape_that_failed() {
            let Err(error) = compile(&Shape {
                id: a("VersionShape"),
                target: Target::Class(a("Table")),
                constraints: vec![Constraint::Pattern {
                    path: a("version"),
                    regex: "(".into(),
                }],
                severity: Severity::Violation,
                message: None,
            }) else {
                panic!("should not compile")
            };

            assert!(error.to_string().contains("VersionShape"), "{error}");
        }

        /// Compiled once, run many times, same answer. The cache in Slice D is
        /// only safe because this holds.
        #[test]
        fn one_compiled_shape_gives_the_same_answer_every_run() {
            let shape = shape_of(Constraint::Pattern {
                path: a("version"),
                regex: r"\d+\.\d+".into(),
            });
            let facts = vec![
                typed("t", "Table"),
                fact("t", "version", FlakeValue::String("1.2".into())),
            ];

            assert_eq!(
                validate(std::slice::from_ref(&shape), &facts),
                validate(&[shape], &facts)
            );
        }
    }

    /// # The reads underneath every constraint
    ///
    /// Selecting focus nodes and reading a node's types are two small
    /// functions the whole validator rests on. A wrong answer here does not
    /// look like a bug in either — it looks like a constraint that fires on the
    /// wrong rows.
    mod what_the_validator_reads {
        use super::*;

        fn owner_required(target: Target) -> CompiledShape {
            compile(&Shape {
                id: a("S"),
                target,
                constraints: vec![Constraint::MinCount {
                    path: a("owner"),
                    n: 1,
                }],
                severity: Severity::Violation,
                message: None,
            })
            .expect("compiles")
        }

        /// A withdrawn edge names nobody. Reporting on the far end of a
        /// retracted reference produces a violation about a relationship the
        /// graph no longer states, which a steward cannot act on because there
        /// is nothing there to fix.
        #[test]
        fn a_retracted_edge_contributes_no_focus_node() {
            let mut retracted = fact("t", "parentSchema", FlakeValue::Ref(a("s")));
            retracted.op = false;

            let report = validate(
                &[owner_required(Target::ObjectsOf(a("parentSchema")))],
                &[retracted],
            );

            assert!(report.conforms, "{:#?}", report.violations);
        }

        /// And a different predicate contributes none either, so the selection
        /// is about `parentSchema` rather than about references in general.
        #[test]
        fn another_predicates_object_is_not_a_focus_node() {
            let report = validate(
                &[owner_required(Target::ObjectsOf(a("parentSchema")))],
                &[fact("t", "parentDatabase", FlakeValue::Ref(a("d")))],
            );

            assert!(report.conforms, "{:#?}", report.violations);
        }

        /// A literal target reads live values only. A withdrawn value would
        /// make its holder a focus node for a constraint about data the graph
        /// no longer states — a violation nobody can fix, because there is
        /// nothing there.
        ///
        /// The constraint is deliberately about a **different** path: a
        /// per-value rule over the retracted value itself conforms vacuously,
        /// so it cannot tell a node that was skipped from one that was checked
        /// and had nothing to check.
        #[test]
        fn a_retracted_literal_contributes_no_focus_node() {
            let shape = || {
                compile(&Shape {
                    id: a("S"),
                    target: Target::LiteralsOf(DataType::Float),
                    constraints: vec![Constraint::MinCount {
                        path: a("owner"),
                        n: 1,
                    }],
                    severity: Severity::Violation,
                    message: None,
                })
                .expect("compiles")
            };

            let mut value = fact("guess", "confidence", FlakeValue::Float(0.5));
            value.op = false;
            assert!(
                validate(&[shape()], &[value.clone()]).conforms,
                "a withdrawn value selected its holder"
            );

            // And the positive, so the assertion above is about the retraction
            // rather than about a target that selects nothing.
            value.op = true;
            assert!(!validate(&[shape()], &[value]).conforms);
        }

        /// The same for the subject side: a withdrawn statement does not make
        /// its subject a focus node.
        #[test]
        fn a_retracted_statement_contributes_no_subject() {
            let mut retracted = fact("t", "fqn", FlakeValue::String("db.t".into()));
            retracted.op = false;

            let report = validate(
                &[owner_required(Target::SubjectsOf(a("fqn")))],
                &[retracted],
            );

            assert!(report.conforms, "{:#?}", report.violations);
        }

        /// A `Class` constraint reads the *target's* types, live, and only
        /// those stated with `rdf:type`. Each of the three negatives below
        /// would otherwise let a wrong reference pass as correctly typed.
        #[test]
        fn a_class_constraint_reads_only_the_targets_live_rdf_types() {
            let shape = || {
                compile(&Shape {
                    id: a("S"),
                    target: Target::Class(a("Table")),
                    constraints: vec![Constraint::Class {
                        path: a("parentSchema"),
                        expected: a("Schema"),
                    }],
                    severity: Severity::Violation,
                    message: None,
                })
                .expect("compiles")
            };
            let base = vec![
                typed("t", "Table"),
                fact("t", "parentSchema", FlakeValue::Ref(a("s"))),
            ];

            // Withdrawn: the type no longer holds, so neither does the
            // constraint.
            let mut withdrawn = base.clone();
            let mut retracted = typed("s", "Schema");
            retracted.op = false;
            withdrawn.push(retracted);
            assert!(
                !validate(&[shape()], &withdrawn).conforms,
                "a retracted type is not a type"
            );

            // Somebody else's type. Reading types without checking the subject
            // makes every node in the estate inherit every type in it.
            let mut elsewhere = base.clone();
            elsewhere.push(typed("other", "Schema"));
            assert!(
                !validate(&[shape()], &elsewhere).conforms,
                "another node's type satisfied this one's constraint"
            );

            // Stated with a different predicate. `dsc:kind = Schema` is not a
            // type assertion, and treating it as one makes the check depend on
            // a predicate nobody agreed meant that.
            let mut mislabelled = base.clone();
            mislabelled.push(fact("s", "kind", FlakeValue::Ref(a("Schema"))));
            assert!(!validate(&[shape()], &mislabelled).conforms);

            // And the positive, so the three above are about the reads rather
            // than about a constraint that never passes.
            let mut correct = base;
            correct.push(typed("s", "Schema"));
            assert!(validate(&[shape()], &correct).conforms);
        }

        /// A range constraint reads every numeric representation. An `Int` and
        /// a `Duration` are magnitudes, and a range that silently skipped them
        /// would report *nothing* on the values it was written to bound —
        /// clean, and wrong.
        #[test]
        fn a_range_constraint_reads_integers_and_durations_as_numbers() {
            let bounded = || Constraint::MaxInclusive {
                path: a("size"),
                v: 10.0,
            };

            for (over, under) in [
                (FlakeValue::Int(11), FlakeValue::Int(10)),
                (FlakeValue::Duration(11), FlakeValue::Duration(10)),
            ] {
                let violating = vec![typed("t", "Table"), fact("t", "size", over.clone())];
                let conforming = vec![typed("t", "Table"), fact("t", "size", under.clone())];

                assert!(
                    !run(bounded(), &violating).conforms,
                    "{over:?} is over the bound and must be reported"
                );
                assert!(run(bounded(), &conforming).conforms, "{under:?}");
            }
        }

        /// A value that is not a number at all fails a range constraint rather
        /// than passing it. Skipping the unreadable value would let a string
        /// through a bound written to keep one out.
        #[test]
        fn a_non_numeric_value_fails_a_range_constraint() {
            let facts = vec![
                typed("t", "Table"),
                fact("t", "size", FlakeValue::String("large".into())),
            ];

            assert!(
                !run(
                    Constraint::MaxInclusive {
                        path: a("size"),
                        v: 10.0
                    },
                    &facts
                )
                .conforms
            );
        }

        /// A generated message carries the offending value when there is one.
        /// "`ordinalPosition` fails datatype" sends a steward to look the value
        /// up; naming it saves the round trip.
        #[test]
        fn a_generated_message_names_the_offending_value_when_there_is_one() {
            let report = run(
                Constraint::Datatype {
                    path: a("ordinalPosition"),
                    dt: DataType::Int,
                },
                &[
                    typed("t", "Table"),
                    fact("t", "ordinalPosition", FlakeValue::String("first".into())),
                ],
            );

            let message = &report.violations[0].message;
            assert!(message.contains("first"), "{message}");
            assert!(message.contains("ordinalPosition"), "{message}");

            // And a count violation has no value to name, so its message must
            // not invent one.
            let missing = run(
                Constraint::MinCount {
                    path: a("name"),
                    n: 1,
                },
                &[typed("t", "Table")],
            );
            assert!(
                !missing.violations[0].message.contains('('),
                "{}",
                missing.violations[0].message
            );
        }
    }
}
