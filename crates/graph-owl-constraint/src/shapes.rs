//! Shapes read from the graph — Epic 5 Slice B.
//!
//! **A shape is data.** Stored as triples, it is versioned, time-travellable and
//! auditable exactly like everything else in the catalog: "when did this rule
//! start applying, and who added it" is answerable by the same machinery that
//! answers it for an owner or a tag. A shape compiled from a config file is a
//! rule nobody can date.
//!
//! The vocabulary is SHACL's, per the W3C specification. Two deliberate
//! departures, both recorded in `plans/00k-standards-conformance.md`:
//!
//! - **`sh:in` is repeated rather than an RDF list.** SHACL encodes it as a
//!   `rdf:first`/`rdf:rest` chain, which costs two extra nodes per member and
//!   an ordering this graph does not use. Repeating the predicate says the same
//!   thing in the flake model's own idiom.
//! - **A combinator's branches are property shapes, not node shapes.** SHACL
//!   lets `sh:not`/`sh:and`/`sh:or` reference a full shape; here a branch is a
//!   property shape with its own `sh:path` and terms. That is the shape the
//!   useful case takes — "an owner **or** a steward" is two paths, not two
//!   node shapes — and it keeps a branch readable without resolving a second
//!   level of targeting that would then disagree with its parent's.

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};
use graph_owl_ontology::{Constraint, DataType, NodeKind, Severity, Shape, Target};
use std::collections::BTreeSet;

use crate::{CompileError, CompiledShape, compile};

fn sh(term: &str) -> Sid {
    Sid::new(namespace::SHACL, term)
}

fn rdf_type() -> Sid {
    Sid::new(namespace::RDF, "type")
}

/// A shape that could not be read out of the graph.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShapeError {
    #[error("shape `{shape}` names no target")]
    NoTarget { shape: String },
    #[error("shape `{shape}` has a property shape `{property}` with no `sh:path`")]
    NoPath { shape: String, property: String },
    #[error("shape `{shape}` uses `{term}`, which is not a constraint this engine knows")]
    UnknownConstraint { shape: String, term: String },
    #[error("shape `{shape}`: `{term}` needs {wanted}, got {got}")]
    WrongValue {
        shape: String,
        term: String,
        wanted: &'static str,
        got: String,
    },
    #[error("shape `{shape}`: property shape `{property}` references itself")]
    CircularShape { shape: String, property: String },
    #[error("shape `{shape}`: property shape `{property}` states no constraint")]
    EmptyBranch { shape: String, property: String },
    #[error(transparent)]
    Compile(#[from] CompileError),
}

/// Values of `p` on `s`, live only.
fn values<'a>(facts: &'a [Flake], s: &Sid, p: &Sid) -> Vec<&'a FlakeValue> {
    facts
        .iter()
        .filter(|f| f.op && &f.s == s && &f.p == p)
        .map(|f| &f.o)
        .collect()
}

fn first<'a>(facts: &'a [Flake], s: &Sid, p: &Sid) -> Option<&'a FlakeValue> {
    values(facts, s, p).into_iter().next()
}

fn as_ref_sid(value: &FlakeValue) -> Option<&Sid> {
    match value {
        FlakeValue::Ref(sid) => Some(sid),
        _ => None,
    }
}

fn as_text(value: &FlakeValue) -> Option<&str> {
    match value {
        FlakeValue::String(s) => Some(s),
        _ => None,
    }
}

/// A count in the graph is a non-negative `Int`.
///
/// Refusing a `Float` rather than rounding it: `sh:minCount 1.5` is a shape
/// somebody got wrong, and reading it as 1 or 2 hides the mistake behind a rule
/// that half works. `try_from` is what rejects a negative — an explicit
/// `>= 0` guard beside it looked like a second check and was unreachable.
fn as_count(value: &FlakeValue) -> Option<usize> {
    match value {
        FlakeValue::Int(i) => usize::try_from(*i).ok(),
        _ => None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn as_number(value: &FlakeValue) -> Option<f64> {
    match value {
        FlakeValue::Float(f) => Some(*f),
        FlakeValue::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Every constraint term this compiler understands, and how to read one.
///
/// A table rather than a chain of `if`s, so that "which terms are supported" is
/// a list somebody can read — and so [`ShapeError::UnknownConstraint`] can be
/// raised for anything not on it. **A term silently ignored is a validation
/// hole**: the shape looks like it constrains something and does not.
const KNOWN: &[&str] = &[
    "minCount",
    "maxCount",
    "datatype",
    "nodeKind",
    "in",
    "pattern",
    "minInclusive",
    "maxInclusive",
    "minLength",
    "maxLength",
    "class",
    "hasValue",
];

/// Terms that appear on a property shape without being constraints.
const STRUCTURAL: &[&str] = &["path"];

/// The logical combinators, whose values are references to other property
/// shapes rather than literal bounds.
const COMBINATORS: &[&str] = &["not", "and", "or"];

fn datatype_from(name: &str) -> Option<DataType> {
    Some(match name {
        "ref" => DataType::Ref,
        "string" => DataType::String,
        "boolean" => DataType::Boolean,
        "int" => DataType::Int,
        "float" => DataType::Float,
        "instant" => DataType::Instant,
        "json" => DataType::Json,
        "bytes" => DataType::Bytes,
        "uuid" => DataType::Uuid,
        "duration" => DataType::Duration,
        _ => return None,
    })
}

fn severity_from(name: &str) -> Option<Severity> {
    Some(match name {
        "Violation" => Severity::Violation,
        "Warning" => Severity::Warning,
        "Info" => Severity::Info,
        _ => return None,
    })
}

/// Where one term's value is read from, and what it must look like.
struct Reading<'a> {
    shape: &'a Sid,
    term: &'a str,
    facts: &'a [Flake],
    property: &'a Sid,
}

impl Reading<'_> {
    fn wrong(&self, wanted: &'static str, got: &FlakeValue) -> ShapeError {
        ShapeError::WrongValue {
            shape: self.shape.to_string(),
            term: self.term.to_string(),
            wanted,
            got: format!("{got:?}"),
        }
    }

    /// The single value stated for this term.
    ///
    /// Absent is an error rather than a skip: `sh:minCount` with no value is a
    /// constraint somebody started writing, and ignoring it leaves a shape that
    /// looks like it checks something and does not.
    fn only(&self, wanted: &'static str) -> Result<&FlakeValue, ShapeError> {
        first(self.facts, self.property, &sh(self.term)).ok_or_else(|| ShapeError::WrongValue {
            shape: self.shape.to_string(),
            term: self.term.to_string(),
            wanted,
            got: "nothing".to_string(),
        })
    }

    fn count(&self) -> Result<usize, ShapeError> {
        let v = self.only("a non-negative integer")?;
        as_count(v).ok_or_else(|| self.wrong("a non-negative integer", v))
    }

    fn number(&self) -> Result<f64, ShapeError> {
        let v = self.only("a number")?;
        as_number(v).ok_or_else(|| self.wrong("a number", v))
    }

    fn text(&self) -> Result<String, ShapeError> {
        let v = self.only("a string")?;
        Ok(as_text(v)
            .ok_or_else(|| self.wrong("a string", v))?
            .to_string())
    }
}

fn constraint_from(
    shape: &Sid,
    term: &str,
    path: &Sid,
    facts: &[Flake],
    property: &Sid,
) -> Result<Constraint, ShapeError> {
    let read = Reading {
        shape,
        term,
        facts,
        property,
    };

    Ok(match term {
        "minCount" => Constraint::MinCount {
            path: path.clone(),
            n: read.count()?,
        },
        "maxCount" => Constraint::MaxCount {
            path: path.clone(),
            n: read.count()?,
        },
        "minLength" => Constraint::MinLength {
            path: path.clone(),
            n: read.count()?,
        },
        "maxLength" => Constraint::MaxLength {
            path: path.clone(),
            n: read.count()?,
        },
        "minInclusive" => Constraint::MinInclusive {
            path: path.clone(),
            v: read.number()?,
        },
        "maxInclusive" => Constraint::MaxInclusive {
            path: path.clone(),
            v: read.number()?,
        },
        "pattern" => Constraint::Pattern {
            path: path.clone(),
            regex: read.text()?,
        },
        "datatype" => Constraint::Datatype {
            path: path.clone(),
            dt: {
                let v = read.only("a datatype name")?;
                as_text(v)
                    .and_then(datatype_from)
                    .ok_or_else(|| read.wrong("a datatype name", v))?
            },
        },
        "nodeKind" => Constraint::NodeKind {
            path: path.clone(),
            kind: {
                let v = read.only("`ref` or `literal`")?;
                match as_text(v) {
                    Some("ref") => NodeKind::Ref,
                    Some("literal") => NodeKind::Literal,
                    _ => return Err(read.wrong("`ref` or `literal`", v)),
                }
            },
        },
        "class" => Constraint::Class {
            path: path.clone(),
            expected: {
                let v = read.only("a reference")?;
                as_ref_sid(v)
                    .ok_or_else(|| read.wrong("a reference", v))?
                    .clone()
            },
        },
        "hasValue" => Constraint::HasValue {
            path: path.clone(),
            v: read.only("a value")?.clone(),
        },
        // Repeated rather than an RDF list — see the module docs.
        "in" => Constraint::In {
            path: path.clone(),
            allowed: values(facts, property, &sh("in"))
                .into_iter()
                .cloned()
                .collect(),
        },
        other => {
            return Err(ShapeError::UnknownConstraint {
                shape: shape.to_string(),
                term: other.to_string(),
            });
        }
    })
}

/// Every constraint stated directly on one property shape, as one constraint.
///
/// Several terms on one property shape are an implicit `And` — that is what
/// `sh:minCount 1` beside `sh:maxCount 3` already means, and reading them as
/// anything else would change the meaning of every shape written so far.
///
/// `open` carries the shapes already being read, so a shape that references
/// itself is **refused rather than recursed into**. A cycle here is a hang, and
/// a hang in shape compilation is a server that will not start.
fn read_branch(
    shape: &Sid,
    property: &Sid,
    facts: &[Flake],
    open: &mut Vec<Sid>,
) -> Result<Constraint, ShapeError> {
    if open.contains(property) {
        return Err(ShapeError::CircularShape {
            shape: shape.to_string(),
            property: property.to_string(),
        });
    }
    open.push(property.clone());

    let path = first(facts, property, &sh("path"))
        .and_then(as_ref_sid)
        .cloned();

    let terms: BTreeSet<&str> = facts
        .iter()
        .filter(|f| f.op && &f.s == property && f.p.namespace_code == namespace::SHACL)
        .map(|f| f.p.id.as_str())
        .collect();

    let mut constraints = Vec::new();
    for term in &terms {
        if STRUCTURAL.contains(term) {
            continue;
        }
        if COMBINATORS.contains(term) {
            constraints.push(read_combinator(shape, property, term, facts, open)?);
            continue;
        }
        if !KNOWN.contains(term) {
            return Err(ShapeError::UnknownConstraint {
                shape: shape.to_string(),
                term: (*term).to_string(),
            });
        }
        // A term needs a path to be about anything. A branch that is *only* a
        // combinator legitimately has none.
        let path = path.as_ref().ok_or_else(|| ShapeError::NoPath {
            shape: shape.to_string(),
            property: property.to_string(),
        })?;
        constraints.push(constraint_from(shape, term, path, facts, property)?);
    }

    open.pop();

    match constraints.len() {
        0 => Err(ShapeError::EmptyBranch {
            shape: shape.to_string(),
            property: property.to_string(),
        }),
        // **An `And` of one is fine, and a special case for it was dead code.**
        // The comment here previously claimed unwrapping mattered because `And`
        // would render as `and` in a violation report. It does not: `And`
        // contributes no violation of its own, it collects its branches' — so
        // the report is identical either way, and mutation testing said so by
        // deleting the arm with nothing failing.
        _ => Ok(Constraint::And(constraints)),
    }
}

/// One combinator, with its branches resolved.
fn read_combinator(
    shape: &Sid,
    property: &Sid,
    term: &str,
    facts: &[Flake],
    open: &mut Vec<Sid>,
) -> Result<Constraint, ShapeError> {
    let branches: Vec<&Sid> = values(facts, property, &sh(term))
        .into_iter()
        .filter_map(as_ref_sid)
        .collect();

    if branches.is_empty() {
        return Err(ShapeError::WrongValue {
            shape: shape.to_string(),
            term: term.to_string(),
            wanted: "a reference to at least one property shape",
            got: "nothing".to_string(),
        });
    }
    // `sh:not` over several branches is ambiguous — "not any" and "not all" are
    // different statements — so it is refused rather than guessed at.
    if term == "not" && branches.len() > 1 {
        return Err(ShapeError::WrongValue {
            shape: shape.to_string(),
            term: term.to_string(),
            wanted: "exactly one branch; `not` over several is ambiguous \
                     between \"not any\" and \"not all\"",
            got: format!("{} branches", branches.len()),
        });
    }

    let mut resolved = Vec::new();
    for branch in branches {
        resolved.push(read_branch(shape, branch, facts, open)?);
    }

    Ok(match term {
        "not" => Constraint::Not(Box::new(
            resolved.pop().expect("exactly one branch checked above"),
        )),
        "and" => Constraint::And(resolved),
        _ => Constraint::Or(resolved),
    })
}

fn target_of(shape: &Sid, facts: &[Flake]) -> Result<Target, ShapeError> {
    if let Some(class) = first(facts, shape, &sh("targetClass")).and_then(as_ref_sid) {
        return Ok(Target::Class(class.clone()));
    }
    if let Some(path) = first(facts, shape, &sh("targetSubjectsOf")).and_then(as_ref_sid) {
        return Ok(Target::SubjectsOf(path.clone()));
    }
    if let Some(path) = first(facts, shape, &sh("targetObjectsOf")).and_then(as_ref_sid) {
        return Ok(Target::ObjectsOf(path.clone()));
    }
    if let Some(name) = first(facts, shape, &sh("targetLiteralsOf"))
        .and_then(as_text)
        .and_then(datatype_from)
    {
        return Ok(Target::LiteralsOf(name));
    }
    // SHACL spells this by making the shape a class; here it is an explicit
    // flag, because `sh:NodeShape` is already the shape's type and a second
    // type statement would be indistinguishable from an ordinary class
    // membership somebody asserted by mistake.
    if first(facts, shape, &sh("targetImplicit")).is_some() {
        return Ok(Target::ImplicitClass);
    }
    let nodes: Vec<Sid> = values(facts, shape, &sh("targetNode"))
        .into_iter()
        .filter_map(as_ref_sid)
        .cloned()
        .collect();
    if !nodes.is_empty() {
        return Ok(Target::Subjects(nodes));
    }
    // A shape with no target constrains nothing while looking like a rule.
    Err(ShapeError::NoTarget {
        shape: shape.to_string(),
    })
}

/// Read one shape, by id.
///
/// # Errors
///
/// A [`ShapeError`] naming the shape and what is wrong with it.
pub fn read_shape(id: &Sid, facts: &[Flake]) -> Result<CompiledShape, ShapeError> {
    let target = target_of(id, facts)?;

    let severity = first(facts, id, &sh("severity"))
        .and_then(as_text)
        .and_then(severity_from)
        // Absent means `Violation`, matching SHACL's own default. A shape whose
        // author did not say is a shape whose author meant the rule to bite.
        .unwrap_or(Severity::Violation);

    let message = first(facts, id, &sh("message"))
        .and_then(as_text)
        .map(ToString::to_string);

    let mut constraints = Vec::new();
    for property in values(facts, id, &sh("property"))
        .into_iter()
        .filter_map(as_ref_sid)
    {
        // One branch reader for the top level and for every nested one, so a
        // combinator's branch is read by exactly the same rules as a plain
        // property shape. Two readers would be two vocabularies.
        let mut open = Vec::new();
        constraints.push(read_branch(id, property, facts, &mut open)?);
    }

    Ok(compile(&Shape {
        id: id.clone(),
        target,
        constraints,
        severity,
        message,
    })?)
}

/// Every shape stated in these facts, and every one that could not be read.
///
/// **Both halves are returned.** One malformed shape must not stop the other
/// twenty from running: an estate goes unvalidated the moment a single bad
/// shape can veto the whole pass, and that is the failure nobody notices
/// because the report simply looks empty.
#[must_use]
pub fn read_all(facts: &[Flake]) -> (Vec<CompiledShape>, Vec<ShapeError>) {
    let mut ids: Vec<Sid> = facts
        .iter()
        .filter(|f| f.op && f.p == rdf_type() && f.o == FlakeValue::Ref(sh("NodeShape")))
        .map(|f| f.s.clone())
        .collect();
    ids.sort_by_key(ToString::to_string);
    ids.dedup();

    let mut shapes = Vec::new();
    let mut failures = Vec::new();
    for id in ids {
        match read_shape(&id, facts) {
            Ok(shape) => shapes.push(shape),
            Err(error) => failures.push(error),
        }
    }
    (shapes, failures)
}

/// One `(term, value)` on a seed shape's property.
///
/// Named, with `Property` and `Definition` below, because the seed table nests
/// three deep and a reader should not have to count brackets to see what it is.
type Term<'a> = (&'a str, FlakeValue);
/// `(property suffix, path, terms)`.
type Property<'a> = (&'a str, &'a str, &'a [Term<'a>]);
/// `(shape, target, properties)`.
type Definition<'a> = (&'a str, &'a str, &'a [Property<'a>]);

/// The shapes the core entity model ships with — Epic 5's seed set.
///
/// **Returned as flakes rather than written by a migration.** A migration would
/// have to hand-encode each object into the flake table's text encoding, which
/// is computed in Rust and would then exist in two places — and the second copy
/// is the one that silently drifts. Writing them through the ordinary graph
/// path also means they are dated, attributable and withdrawable like any other
/// shape, which is the entire argument for shapes being data.
///
/// Every one is `Violation` severity except `EnvelopeShape`, which is advice:
/// a version that does not match the expected format is worth flagging and is
/// not worth blocking an estate over.
#[must_use]
pub fn core_shapes(t: i64) -> Vec<Flake> {
    let mut flakes = Vec::new();
    let mut state = |s: Sid, p: Sid, o: FlakeValue| {
        flakes.push(Flake {
            s,
            p,
            o,
            cx: Some(Sid::dsc("graph:shapes")),
            t,
            op: true,
        });
    };

    // (shape, target class, [(property suffix, path, [(term, value)])])
    let definitions: &[Definition<'_>] = &[
        (
            "TableShape",
            "table",
            &[
                ("name", "name", &[("minCount", FlakeValue::Int(1))]),
                (
                    "fqn",
                    "fqn",
                    &[
                        ("minCount", FlakeValue::Int(1)),
                        ("maxCount", FlakeValue::Int(1)),
                    ],
                ),
                (
                    "parent",
                    "parentSchema",
                    &[
                        ("maxCount", FlakeValue::Int(1)),
                        ("nodeKind", FlakeValue::String("ref".into())),
                    ],
                ),
            ],
        ),
        (
            "ColumnShape",
            "column",
            &[
                ("parent", "parentTable", &[("minCount", FlakeValue::Int(1))]),
                (
                    "ordinal",
                    "ordinalPosition",
                    &[
                        ("datatype", FlakeValue::String("int".into())),
                        ("minInclusive", FlakeValue::Int(0)),
                    ],
                ),
            ],
        ),
        (
            "ConfidenceShape",
            "confidence",
            &[(
                "value",
                "confidence",
                &[
                    ("datatype", FlakeValue::String("float".into())),
                    ("minInclusive", FlakeValue::Float(0.0)),
                    ("maxInclusive", FlakeValue::Float(1.0)),
                ],
            )],
        ),
    ];

    for (shape, target, properties) in definitions {
        let id = Sid::dsc(*shape);
        state(id.clone(), rdf_type(), FlakeValue::Ref(sh("NodeShape")));
        // `ConfidenceShape` is about a *predicate*, not a class: anything
        // carrying a confidence is in scope, whatever kind it is.
        let target_term = if *shape == "ConfidenceShape" {
            "targetSubjectsOf"
        } else {
            "targetClass"
        };
        state(
            id.clone(),
            sh(target_term),
            FlakeValue::Ref(Sid::dsc(*target)),
        );

        for (suffix, path, terms) in *properties {
            let property = Sid::dsc(format!("{shape}/{suffix}"));
            state(
                id.clone(),
                sh("property"),
                FlakeValue::Ref(property.clone()),
            );
            state(
                property.clone(),
                sh("path"),
                FlakeValue::Ref(Sid::dsc(*path)),
            );
            for (term, value) in *terms {
                state(property.clone(), sh(term), value.clone());
            }
        }
    }

    flakes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate;

    fn a(id: &str) -> Sid {
        Sid::dsc(id)
    }

    fn f(s: Sid, p: Sid, o: FlakeValue) -> Flake {
        Flake::assert(s, p, o, 1)
    }

    fn node_shape(id: &str) -> Flake {
        f(a(id), rdf_type(), FlakeValue::Ref(sh("NodeShape")))
    }

    /// `TableShape` requires an owner. Stated entirely as triples, the way a
    /// migration or an API call would state it.
    fn table_shape() -> Vec<Flake> {
        vec![
            node_shape("TableShape"),
            f(
                a("TableShape"),
                sh("targetClass"),
                FlakeValue::Ref(a("Table")),
            ),
            f(
                a("TableShape"),
                sh("property"),
                FlakeValue::Ref(a("TableShape/owner")),
            ),
            f(
                a("TableShape/owner"),
                sh("path"),
                FlakeValue::Ref(a("owner")),
            ),
            f(a("TableShape/owner"), sh("minCount"), FlakeValue::Int(1)),
        ]
    }

    fn typed(s: &str, class: &str) -> Flake {
        f(a(s), rdf_type(), FlakeValue::Ref(a(class)))
    }

    /// The round trip: triples in, violations out. This is the whole slice —
    /// a shape stated as data has to behave exactly like one built in code.
    #[test]
    fn a_shape_stated_as_triples_catches_what_it_describes() {
        let mut facts = table_shape();
        facts.push(typed("orders", "Table"));

        let (shapes, failures) = read_all(&facts);

        assert!(failures.is_empty(), "{failures:#?}");
        assert_eq!(shapes.len(), 1);
        let report = validate(&shapes, &facts);
        assert!(!report.conforms);
        assert_eq!(report.violations[0].focus_node, a("orders"));
        assert_eq!(report.violations[0].constraint, "minCount");
    }

    /// And the negative: an estate that satisfies the shape conforms, so the
    /// test above is about the constraint rather than about a validator that
    /// always complains.
    #[test]
    fn the_same_shape_passes_an_estate_that_satisfies_it() {
        let mut facts = table_shape();
        facts.push(typed("orders", "Table"));
        facts.push(f(a("orders"), a("owner"), FlakeValue::Ref(a("finance"))));

        let (shapes, _) = read_all(&facts);

        assert!(validate(&shapes, &facts).conforms);
    }

    /// **A term nobody implemented must be an error, never a silent skip.** A
    /// dropped constraint leaves a shape that looks like it checks something
    /// and does not — the validation hole that is hardest to notice, because
    /// the report is clean.
    #[test]
    fn an_unknown_constraint_term_is_refused_rather_than_ignored() {
        let mut facts = table_shape();
        facts.push(f(
            a("TableShape/owner"),
            sh("qualifiedMinCount"),
            FlakeValue::Int(1),
        ));

        let (shapes, failures) = read_all(&facts);

        assert!(shapes.is_empty(), "the shape must not compile");
        assert!(
            matches!(&failures[0], ShapeError::UnknownConstraint { term, .. } if term == "qualifiedMinCount"),
            "{failures:#?}"
        );
    }

    #[test]
    fn a_shape_with_no_target_is_refused() {
        let facts = vec![node_shape("Untargeted")];

        let (_, failures) = read_all(&facts);

        assert!(
            matches!(&failures[0], ShapeError::NoTarget { shape } if shape.contains("Untargeted")),
            "{failures:#?}"
        );
    }

    #[test]
    fn a_property_shape_with_no_path_is_refused() {
        let facts = vec![
            node_shape("S"),
            f(a("S"), sh("targetClass"), FlakeValue::Ref(a("Table"))),
            f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
            f(a("S/p"), sh("minCount"), FlakeValue::Int(1)),
        ];

        let (_, failures) = read_all(&facts);

        assert!(
            matches!(&failures[0], ShapeError::NoPath { .. }),
            "{failures:#?}"
        );
    }

    /// A count that is not a count is a mistake, not something to round. Reading
    /// `1.5` as `1` hides the error behind a rule that half works.
    #[test]
    fn a_count_that_is_not_an_integer_is_refused() {
        let mut facts = table_shape();
        facts.retain(|flake| flake.p != sh("minCount"));
        facts.push(f(
            a("TableShape/owner"),
            sh("minCount"),
            FlakeValue::Float(1.5),
        ));

        let (_, failures) = read_all(&facts);

        assert!(
            matches!(&failures[0], ShapeError::WrongValue { term, .. } if term == "minCount"),
            "{failures:#?}"
        );
    }

    /// **One bad shape must not silence the rest.** An estate goes unvalidated
    /// the moment a single malformed shape can veto the whole pass, and nobody
    /// notices because the report simply looks clean.
    #[test]
    fn a_malformed_shape_does_not_stop_the_others_running() {
        let mut facts = table_shape();
        facts.push(node_shape("Untargeted"));
        facts.push(typed("orders", "Table"));

        let (shapes, failures) = read_all(&facts);

        assert_eq!(shapes.len(), 1, "the good shape must still compile");
        assert_eq!(failures.len(), 1);
        assert!(!validate(&shapes, &facts).conforms);
    }

    /// Same triples, same compiled shape — the property the cache in Slice D
    /// depends on, and the reason constraint order is sorted rather than
    /// whatever the store returned.
    #[test]
    fn the_same_triples_compile_to_the_same_shape_every_time() {
        let mut facts = table_shape();
        facts.push(f(a("TableShape/owner"), sh("maxCount"), FlakeValue::Int(3)));
        facts.push(f(
            a("TableShape/owner"),
            sh("nodeKind"),
            FlakeValue::String("ref".into()),
        ));
        facts.push(typed("orders", "Table"));

        let forwards = read_all(&facts).0;
        facts.reverse();
        let backwards = read_all(&facts).0;

        assert_eq!(
            validate(&forwards, &facts).violations,
            validate(&backwards, &facts).violations,
            "fact order changed the shape"
        );
    }

    /// A shape targeting a class nothing has yet is not an error — the class
    /// may arrive tomorrow, and refusing the shape would make governance
    /// impossible to write ahead of the data.
    #[test]
    fn a_shape_targeting_an_absent_class_compiles_and_matches_nothing() {
        let facts = table_shape();

        let (shapes, failures) = read_all(&facts);

        assert!(failures.is_empty());
        assert!(validate(&shapes, &facts).conforms);
    }

    /// Severity is read, and absent means `Violation` — SHACL's own default,
    /// and the reading that makes an unannotated rule bite.
    #[test]
    fn severity_is_read_and_defaults_to_violation() {
        let mut warning = table_shape();
        warning.push(f(
            a("TableShape"),
            sh("severity"),
            FlakeValue::String("Warning".into()),
        ));
        warning.push(typed("orders", "Table"));

        let report = validate(&read_all(&warning).0, &warning);
        assert!(report.conforms, "a warning does not fail conformance");
        assert_eq!(report.violations.len(), 1);

        let mut unannotated = table_shape();
        unannotated.push(typed("orders", "Table"));
        assert!(!validate(&read_all(&unannotated).0, &unannotated).conforms);
    }

    #[test]
    fn a_shape_message_is_read_from_the_graph() {
        let mut facts = table_shape();
        facts.push(f(
            a("TableShape"),
            sh("message"),
            FlakeValue::String("every table needs an owner".into()),
        ));
        facts.push(typed("orders", "Table"));

        let report = validate(&read_all(&facts).0, &facts);

        assert_eq!(report.violations[0].message, "every table needs an owner");
    }

    /// Each target form, so a compiler that read only `targetClass` could not
    /// pass. They select genuinely different node sets.
    #[test]
    fn every_target_form_is_read() {
        let base = |term: &str, object: FlakeValue| {
            vec![
                node_shape("S"),
                f(a("S"), sh(term), object),
                f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
                f(a("S/p"), sh("path"), FlakeValue::Ref(a("owner"))),
                f(a("S/p"), sh("minCount"), FlakeValue::Int(1)),
            ]
        };
        let estate = [
            typed("orders", "Table"),
            f(
                a("orders"),
                a("fqn"),
                FlakeValue::String("db.orders".into()),
            ),
            f(a("orders"), a("parentSchema"), FlakeValue::Ref(a("public"))),
        ];

        for (term, object, expected) in [
            ("targetClass", FlakeValue::Ref(a("Table")), a("orders")),
            ("targetNode", FlakeValue::Ref(a("orders")), a("orders")),
            ("targetSubjectsOf", FlakeValue::Ref(a("fqn")), a("orders")),
            (
                "targetObjectsOf",
                FlakeValue::Ref(a("parentSchema")),
                a("public"),
            ),
        ] {
            let mut facts = base(term, object);
            facts.extend(estate.iter().cloned());

            let (shapes, failures) = read_all(&facts);
            assert!(failures.is_empty(), "{term}: {failures:#?}");
            let report = validate(&shapes, &facts);

            assert_eq!(
                report.violations.len(),
                1,
                "{term}: {:#?}",
                report.violations
            );
            assert_eq!(report.violations[0].focus_node, expected, "{term}");
        }
    }

    /// `sh:in` repeats rather than nesting an RDF list — the module's stated
    /// departure from the specification, and a test so the departure is a
    /// decision rather than a gap.
    #[test]
    fn an_in_constraint_reads_every_repeated_value() {
        let mut facts = vec![
            node_shape("S"),
            f(a("S"), sh("targetClass"), FlakeValue::Ref(a("Edge"))),
            f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
            f(a("S/p"), sh("path"), FlakeValue::Ref(a("relType"))),
            f(a("S/p"), sh("in"), FlakeValue::String("feeds".into())),
            f(a("S/p"), sh("in"), FlakeValue::String("derivedFrom".into())),
        ];
        facts.push(typed("e1", "Edge"));
        facts.push(f(a("e1"), a("relType"), FlakeValue::String("feeds".into())));
        facts.push(typed("e2", "Edge"));
        facts.push(f(
            a("e2"),
            a("relType"),
            FlakeValue::String("influences".into()),
        ));

        let report = validate(&read_all(&facts).0, &facts);

        assert_eq!(report.violations.len(), 1, "{:#?}", report.violations);
        assert_eq!(report.violations[0].focus_node, a("e2"));
    }

    /// A pattern that does not parse fails at read time, carried through from
    /// `compile`. The shape is refused rather than half-applied.
    #[test]
    fn an_unparseable_pattern_fails_the_read() {
        let mut facts = table_shape();
        facts.push(f(
            a("TableShape/owner"),
            sh("pattern"),
            FlakeValue::String("(".into()),
        ));

        let (_, failures) = read_all(&facts);

        assert!(
            matches!(
                &failures[0],
                ShapeError::Compile(CompileError::BadPattern { .. })
            ),
            "{failures:#?}"
        );
    }

    /// Every datatype name round-trips. A misspelling in this table would make
    /// a datatype constraint unstateable, and the error would point at the
    /// shape author rather than at the table.
    #[test]
    fn every_datatype_name_is_readable() {
        for (name, expected) in [
            ("ref", DataType::Ref),
            ("string", DataType::String),
            ("boolean", DataType::Boolean),
            ("int", DataType::Int),
            ("float", DataType::Float),
            ("instant", DataType::Instant),
            ("json", DataType::Json),
            ("bytes", DataType::Bytes),
            ("uuid", DataType::Uuid),
            ("duration", DataType::Duration),
        ] {
            assert_eq!(datatype_from(name), Some(expected), "{name}");
        }
        assert_eq!(
            datatype_from("decimal"),
            None,
            "an unknown name is not read"
        );
    }

    #[test]
    fn an_unknown_severity_name_is_not_read() {
        assert_eq!(severity_from("Violation"), Some(Severity::Violation));
        assert_eq!(severity_from("Warning"), Some(Severity::Warning));
        assert_eq!(severity_from("Info"), Some(Severity::Info));
        assert_eq!(severity_from("Critical"), None);
    }

    /// A retracted shape is not a shape. Leaving it in force would make
    /// withdrawing governance impossible.
    #[test]
    fn a_retracted_shape_is_not_read() {
        let mut facts = table_shape();
        facts[0].op = false;
        facts.push(typed("orders", "Table"));

        let (shapes, failures) = read_all(&facts);

        assert!(shapes.is_empty(), "{shapes:#?}");
        assert!(failures.is_empty(), "a withdrawn shape is not a broken one");
    }

    /// # Every term, stated as triples and then actually applied
    ///
    /// The tests above prove a shape *reads*. These prove it **bites**: each
    /// term is written into the graph and then run against a value that
    /// satisfies it and one that does not. A reader that parsed `minInclusive`
    /// into the wrong number, or read `nodeKind` and always returned `ref`,
    /// passes every parse test and fails here.
    mod each_term_round_trips {
        use super::*;

        /// A shape stating one term on `dsc:v`, over everything of type `T`.
        fn one_term(term: &str, value: FlakeValue) -> Vec<Flake> {
            vec![
                node_shape("S"),
                f(a("S"), sh("targetClass"), FlakeValue::Ref(a("T"))),
                f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
                f(a("S/p"), sh("path"), FlakeValue::Ref(a("v"))),
                f(a("S/p"), sh(term), value),
            ]
        }

        fn holds(value: FlakeValue) -> Vec<Flake> {
            vec![typed("n", "T"), f(a("n"), a("v"), value)]
        }

        #[test]
        fn every_stated_term_admits_what_it_should_and_refuses_what_it_should_not() {
            let cases: Vec<(&str, FlakeValue, FlakeValue, FlakeValue)> = vec![
                // term, stated value, a conforming value, a violating one
                (
                    "minInclusive",
                    FlakeValue::Float(0.0),
                    FlakeValue::Float(0.0),
                    FlakeValue::Float(-0.5),
                ),
                (
                    "maxInclusive",
                    FlakeValue::Float(1.0),
                    FlakeValue::Float(1.0),
                    FlakeValue::Float(1.5),
                ),
                (
                    "minLength",
                    FlakeValue::Int(3),
                    FlakeValue::String("abc".into()),
                    FlakeValue::String("ab".into()),
                ),
                (
                    "maxLength",
                    FlakeValue::Int(3),
                    FlakeValue::String("abc".into()),
                    FlakeValue::String("abcd".into()),
                ),
                (
                    "maxCount",
                    FlakeValue::Int(1),
                    FlakeValue::String("only".into()),
                    // Two values violate; the second is added below.
                    FlakeValue::String("first".into()),
                ),
                (
                    "pattern",
                    FlakeValue::String(r"\d+".into()),
                    FlakeValue::String("42".into()),
                    FlakeValue::String("4x".into()),
                ),
                (
                    "datatype",
                    FlakeValue::String("int".into()),
                    FlakeValue::Int(1),
                    FlakeValue::String("1".into()),
                ),
                (
                    "nodeKind",
                    FlakeValue::String("ref".into()),
                    FlakeValue::Ref(a("other")),
                    FlakeValue::String("other".into()),
                ),
                (
                    "hasValue",
                    FlakeValue::String("required".into()),
                    FlakeValue::String("required".into()),
                    FlakeValue::String("something else".into()),
                ),
            ];

            for (term, stated, ok, bad) in cases {
                let shape = one_term(term, stated.clone());

                let mut conforming = shape.clone();
                conforming.extend(holds(ok.clone()));
                let (shapes, failures) = read_all(&conforming);
                assert!(failures.is_empty(), "{term}: {failures:#?}");
                assert!(
                    validate(&shapes, &conforming).conforms,
                    "{term}: {ok:?} should satisfy {stated:?}"
                );

                let mut violating = shape;
                violating.extend(holds(bad.clone()));
                if term == "maxCount" {
                    violating.push(f(a("n"), a("v"), FlakeValue::String("second".into())));
                }
                let (shapes, _) = read_all(&violating);
                assert!(
                    !validate(&shapes, &violating).conforms,
                    "{term}: {bad:?} should violate {stated:?}"
                );
            }
        }

        /// `nodeKind literal` is the mirror. Without it a reader that ignored
        /// the stated kind and always demanded a reference would pass the case
        /// above.
        #[test]
        fn node_kind_literal_is_read_as_literal() {
            let shape = one_term("nodeKind", FlakeValue::String("literal".into()));

            let mut conforming = shape.clone();
            conforming.extend(holds(FlakeValue::String("plain".into())));
            assert!(validate(&read_all(&conforming).0, &conforming).conforms);

            let mut violating = shape;
            violating.extend(holds(FlakeValue::Ref(a("other"))));
            assert!(!validate(&read_all(&violating).0, &violating).conforms);
        }

        /// An unrecognised node kind is refused rather than defaulted. Reading
        /// `sh:nodeKind "IRI"` as `ref` would silently apply SHACL's vocabulary
        /// to a term this engine does not implement.
        #[test]
        fn an_unrecognised_node_kind_is_refused() {
            let facts = one_term("nodeKind", FlakeValue::String("IRI".into()));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::WrongValue { term, .. } if term == "nodeKind"),
                "{failures:#?}"
            );
        }

        /// A bound stated as an `Int` is read as the number it is. Reading it
        /// as zero — or as anything constant — makes every range shape agree
        /// with every other one.
        #[test]
        fn a_bound_stated_as_an_integer_is_read_at_its_own_value() {
            let facts = |stated: i64, held: i64| {
                let mut all = one_term("maxInclusive", FlakeValue::Int(stated));
                all.extend(holds(FlakeValue::Int(held)));
                all
            };

            let under = facts(10, 9);
            assert!(validate(&read_all(&under).0, &under).conforms);
            let over = facts(10, 11);
            assert!(!validate(&read_all(&over).0, &over).conforms);
            // A different bound must behave differently, or a constant would
            // pass both assertions above.
            let generous = facts(100, 11);
            assert!(validate(&read_all(&generous).0, &generous).conforms);
        }

        /// A negative count is not a count. `usize::try_from` is what rejects
        /// it, and this pins that so the rejection cannot be optimised away.
        #[test]
        fn a_negative_count_is_refused() {
            let facts = one_term("minCount", FlakeValue::Int(-1));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::WrongValue { term, .. } if term == "minCount"),
                "{failures:#?}"
            );
        }

        /// A bound that is not a number at all is refused rather than read as
        /// zero.
        #[test]
        fn a_bound_that_is_not_a_number_is_refused() {
            let facts = one_term("maxInclusive", FlakeValue::String("ten".into()));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::WrongValue { term, .. } if term == "maxInclusive"),
                "{failures:#?}"
            );
        }

        /// A `class` constraint stated as triples checks the far node's type.
        #[test]
        fn a_class_constraint_stated_as_triples_checks_the_far_type() {
            let shape = one_term("class", FlakeValue::Ref(a("Schema")));

            let mut right = shape.clone();
            right.extend(holds(FlakeValue::Ref(a("s"))));
            right.push(typed("s", "Schema"));
            assert!(validate(&read_all(&right).0, &right).conforms);

            let mut wrong = shape;
            wrong.extend(holds(FlakeValue::Ref(a("s"))));
            wrong.push(typed("s", "Database"));
            assert!(!validate(&read_all(&wrong).0, &wrong).conforms);
        }
    }

    /// # The two targets that select differently
    mod the_remaining_targets {
        use super::*;
        use crate::validate;

        /// A literal has no identity of its own, so the focus is whatever holds
        /// it. Reporting against the value would name nothing a steward can
        /// open.
        #[test]
        fn a_literal_target_focuses_the_node_that_holds_it() {
            let facts = vec![
                node_shape("S"),
                f(
                    a("S"),
                    sh("targetLiteralsOf"),
                    FlakeValue::String("float".into()),
                ),
                f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
                f(a("S/p"), sh("path"), FlakeValue::Ref(a("confidence"))),
                f(a("S/p"), sh("maxInclusive"), FlakeValue::Float(1.0)),
                // Holds a float, so it is a focus node — and its value is out
                // of range.
                f(a("guess"), a("confidence"), FlakeValue::Float(1.5)),
                // Holds no float at all, so the shape never looks at it.
                f(a("solid"), a("name"), FlakeValue::String("orders".into())),
            ];

            let (shapes, failures) = read_all(&facts);

            assert!(failures.is_empty(), "{failures:#?}");
            let report = validate(&shapes, &facts);
            assert_eq!(report.violations.len(), 1, "{:#?}", report.violations);
            assert_eq!(report.violations[0].focus_node, a("guess"));
        }

        /// The shape's own id is the class, so a shape named after what it
        /// constrains does not repeat itself.
        #[test]
        fn an_implicit_class_target_uses_the_shapes_own_id() {
            let facts = vec![
                node_shape("Regulatory"),
                f(
                    a("Regulatory"),
                    sh("targetImplicit"),
                    FlakeValue::Boolean(true),
                ),
                f(
                    a("Regulatory"),
                    sh("property"),
                    FlakeValue::Ref(a("Regulatory/p")),
                ),
                f(a("Regulatory/p"), sh("path"), FlakeValue::Ref(a("owner"))),
                f(a("Regulatory/p"), sh("minCount"), FlakeValue::Int(1)),
                typed("payments", "Regulatory"),
                typed("scratch", "Sandbox"),
            ];

            let (shapes, failures) = read_all(&facts);

            assert!(failures.is_empty(), "{failures:#?}");
            let report = validate(&shapes, &facts);
            assert_eq!(report.violations.len(), 1, "{:#?}", report.violations);
            assert_eq!(report.violations[0].focus_node, a("payments"));
        }

        /// An unknown datatype name is refused rather than silently targeting
        /// nothing — a shape that quietly matches no rows is a rule that looks
        /// enforced and is not.
        #[test]
        fn an_unknown_literal_datatype_falls_through_to_no_target() {
            let facts = vec![
                node_shape("S"),
                f(
                    a("S"),
                    sh("targetLiteralsOf"),
                    FlakeValue::String("decimal".into()),
                ),
            ];

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::NoTarget { .. }),
                "{failures:#?}"
            );
        }
    }

    /// # The seed shapes
    mod the_core_shapes {
        use super::*;
        use crate::validate;

        /// **Every one compiles.** A seed shape that cannot be read is a rule
        /// the product ships and never runs, and the refusal would be logged
        /// somewhere nobody looks.
        #[test]
        fn every_seed_shape_compiles() {
            let (shapes, failures) = read_all(&core_shapes(1));

            assert!(failures.is_empty(), "{failures:#?}");
            assert_eq!(shapes.len(), 3, "{shapes:#?}");
        }

        /// They live in the shapes graph, so they are not themselves assets the
        /// catalog validates.
        #[test]
        fn they_are_stated_in_the_shapes_graph() {
            assert!(
                core_shapes(1)
                    .iter()
                    .all(|flake| flake.cx == Some(Sid::dsc("graph:shapes"))),
            );
        }

        /// And they catch what they describe — a table with no name and no
        /// FQN, a column with no parent, a confidence outside its range.
        #[test]
        fn they_catch_what_they_describe() {
            let mut facts = core_shapes(1);
            facts.push(typed("orders", "table"));
            facts.push(typed("orders_id", "column"));
            facts.push(f(a("guess"), a("confidence"), FlakeValue::Float(1.5)));

            let (shapes, _) = read_all(&facts);
            let report = validate(&shapes, &facts);

            let offenders: Vec<&Sid> = report.violations.iter().map(|v| &v.focus_node).collect();
            assert!(
                offenders.contains(&&a("orders")),
                "{:#?}",
                report.violations
            );
            assert!(
                offenders.contains(&&a("orders_id")),
                "{:#?}",
                report.violations
            );
            assert!(offenders.contains(&&a("guess")), "{:#?}", report.violations);
        }

        /// And the negative: a well-formed estate passes them. A seed set that
        /// complains about correct data is a seed set everybody disables.
        #[test]
        fn a_well_formed_estate_passes_them() {
            let mut facts = core_shapes(1);
            facts.push(typed("orders", "table"));
            facts.push(f(
                a("orders"),
                a("name"),
                FlakeValue::String("orders".into()),
            ));
            facts.push(f(
                a("orders"),
                a("fqn"),
                FlakeValue::String("svc.db.public.orders".into()),
            ));
            facts.push(typed("orders_id", "column"));
            facts.push(f(
                a("orders_id"),
                a("parentTable"),
                FlakeValue::Ref(a("orders")),
            ));
            facts.push(f(a("orders_id"), a("ordinalPosition"), FlakeValue::Int(0)));
            facts.push(f(a("guess"), a("confidence"), FlakeValue::Float(0.9)));

            let (shapes, _) = read_all(&facts);
            let report = validate(&shapes, &facts);

            assert!(report.conforms, "{:#?}", report.violations);
        }

        /// Seeded twice at different instants, the shapes are the same shapes —
        /// only their `t` differs. Re-seeding must be a no-op in meaning.
        #[test]
        fn seeding_twice_states_the_same_shapes() {
            let first: Vec<_> = core_shapes(1)
                .into_iter()
                .map(|f| (f.s, f.p, format!("{:?}", f.o)))
                .collect();
            let second: Vec<_> = core_shapes(99)
                .into_iter()
                .map(|f| (f.s, f.p, format!("{:?}", f.o)))
                .collect();

            assert_eq!(first, second);
        }
    }

    /// # Combinators, stated as triples
    ///
    /// The branches are property shapes with their own `sh:path`, because the
    /// useful case is two *paths* — "an owner or a steward" — and not two node
    /// shapes whose targeting could then disagree with the parent's.
    mod combinators_from_triples {
        use super::*;
        use crate::validate;

        /// A shape whose single property carries `term` pointing at `branches`.
        fn with_combinator(term: &str, branches: &[&str]) -> Vec<Flake> {
            let mut facts = vec![
                node_shape("S"),
                f(a("S"), sh("targetClass"), FlakeValue::Ref(a("T"))),
                f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
            ];
            for branch in branches {
                facts.push(f(a("S/p"), sh(term), FlakeValue::Ref(a(branch))));
            }
            facts
        }

        /// One branch: requires `path` to be present.
        fn requires(id: &str, path: &str) -> Vec<Flake> {
            vec![
                f(a(id), sh("path"), FlakeValue::Ref(a(path))),
                f(a(id), sh("minCount"), FlakeValue::Int(1)),
            ]
        }

        fn subject_of(class: &str) -> Flake {
            typed("n", class)
        }

        /// **The case combinators exist for**: an owner *or* a steward, either
        /// will do. Two paths, which a node-shape encoding could not express
        /// without a second target.
        #[test]
        fn an_or_over_two_paths_is_satisfied_by_either() {
            let mut shape = with_combinator("or", &["S/owner", "S/steward"]);
            shape.extend(requires("S/owner", "owner"));
            shape.extend(requires("S/steward", "steward"));

            for held in ["owner", "steward"] {
                let mut facts = shape.clone();
                facts.push(subject_of("T"));
                facts.push(f(a("n"), a(held), FlakeValue::Ref(a("someone"))));

                let (shapes, failures) = read_all(&facts);
                assert!(failures.is_empty(), "{held}: {failures:#?}");
                assert!(
                    validate(&shapes, &facts).conforms,
                    "{held} alone should satisfy the or"
                );
            }

            // And neither violates — once, not per branch.
            let mut neither = shape;
            neither.push(subject_of("T"));
            let (shapes, _) = read_all(&neither);
            let report = validate(&shapes, &neither);
            assert_eq!(report.violations.len(), 1, "{:#?}", report.violations);
            assert_eq!(report.violations[0].constraint, "or");
        }

        #[test]
        fn an_and_needs_every_branch() {
            let mut shape = with_combinator("and", &["S/owner", "S/steward"]);
            shape.extend(requires("S/owner", "owner"));
            shape.extend(requires("S/steward", "steward"));

            let mut one = shape.clone();
            one.push(subject_of("T"));
            one.push(f(a("n"), a("owner"), FlakeValue::Ref(a("someone"))));
            assert!(!validate(&read_all(&one).0, &one).conforms);

            let mut both = shape;
            both.push(subject_of("T"));
            both.push(f(a("n"), a("owner"), FlakeValue::Ref(a("someone"))));
            both.push(f(a("n"), a("steward"), FlakeValue::Ref(a("ops"))));
            assert!(validate(&read_all(&both).0, &both).conforms);
        }

        #[test]
        fn a_not_inverts_its_branch() {
            let mut shape = with_combinator("not", &["S/deprecated"]);
            shape.extend(requires("S/deprecated", "deprecated"));

            let mut absent = shape.clone();
            absent.push(subject_of("T"));
            assert!(validate(&read_all(&absent).0, &absent).conforms);

            let mut present = shape;
            present.push(subject_of("T"));
            present.push(f(a("n"), a("deprecated"), FlakeValue::Boolean(true)));
            assert!(!validate(&read_all(&present).0, &present).conforms);
        }

        /// Combinators nest. A branch is read by the same reader as the top
        /// level, so `or(and(a, b), c)` needs no second vocabulary.
        #[test]
        fn combinators_nest() {
            let mut facts = with_combinator("or", &["S/both", "S/alone"]);
            // `S/both` is itself an `and`.
            facts.push(f(a("S/both"), sh("and"), FlakeValue::Ref(a("S/x"))));
            facts.push(f(a("S/both"), sh("and"), FlakeValue::Ref(a("S/y"))));
            facts.extend(requires("S/x", "x"));
            facts.extend(requires("S/y", "y"));
            facts.extend(requires("S/alone", "z"));

            let satisfied_by_both = {
                let mut f2 = facts.clone();
                f2.push(subject_of("T"));
                f2.push(f(a("n"), a("x"), FlakeValue::Int(1)));
                f2.push(f(a("n"), a("y"), FlakeValue::Int(1)));
                f2
            };
            let satisfied_by_z = {
                let mut f2 = facts.clone();
                f2.push(subject_of("T"));
                f2.push(f(a("n"), a("z"), FlakeValue::Int(1)));
                f2
            };
            let satisfied_by_half = {
                let mut f2 = facts;
                f2.push(subject_of("T"));
                f2.push(f(a("n"), a("x"), FlakeValue::Int(1)));
                f2
            };

            assert!(validate(&read_all(&satisfied_by_both).0, &satisfied_by_both).conforms);
            assert!(validate(&read_all(&satisfied_by_z).0, &satisfied_by_z).conforms);
            assert!(
                !validate(&read_all(&satisfied_by_half).0, &satisfied_by_half).conforms,
                "half of the `and` branch should not satisfy the `or`"
            );
        }

        /// **A shape referencing itself is refused, not recursed into.** A cycle
        /// here is a hang, and a hang in shape compilation is a server that will
        /// not start.
        #[test]
        fn a_self_referencing_shape_is_refused_rather_than_hanging() {
            let mut facts = with_combinator("not", &["S/loop"]);
            facts.push(f(a("S/loop"), sh("not"), FlakeValue::Ref(a("S/loop"))));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::CircularShape { .. }),
                "{failures:#?}"
            );
        }

        /// A longer cycle too — `a → b → a` is the one a single-step guard
        /// misses.
        #[test]
        fn an_indirect_cycle_is_refused() {
            let mut facts = with_combinator("and", &["S/a"]);
            facts.push(f(a("S/a"), sh("and"), FlakeValue::Ref(a("S/b"))));
            facts.push(f(a("S/b"), sh("and"), FlakeValue::Ref(a("S/a"))));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::CircularShape { .. }),
                "{failures:#?}"
            );
        }

        /// **`sh:not` over several branches is ambiguous** — "not any" and "not
        /// all" are different statements — so it is refused rather than one of
        /// them being guessed at.
        #[test]
        fn a_not_over_several_branches_is_refused() {
            let mut facts = with_combinator("not", &["S/a", "S/b"]);
            facts.extend(requires("S/a", "a"));
            facts.extend(requires("S/b", "b"));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::WrongValue { term, .. } if term == "not"),
                "{failures:#?}"
            );
        }

        /// A combinator pointing at nothing is a shape somebody started. It
        /// constrains nothing while looking like a rule.
        #[test]
        fn a_combinator_with_no_branches_is_refused() {
            let facts = vec![
                node_shape("S"),
                f(a("S"), sh("targetClass"), FlakeValue::Ref(a("T"))),
                f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
                f(
                    a("S/p"),
                    sh("or"),
                    FlakeValue::String("not a reference".into()),
                ),
            ];

            let (_, failures) = read_all(&facts);

            assert!(!failures.is_empty(), "an unusable `or` was accepted");
        }

        /// A branch that states nothing is refused. An empty branch in an `or`
        /// is satisfied by everything, which silently disables the whole
        /// combinator.
        #[test]
        fn an_empty_branch_is_refused() {
            let mut facts = with_combinator("or", &["S/real", "S/empty"]);
            facts.extend(requires("S/real", "owner"));
            // `S/empty` has a path and no terms.
            facts.push(f(a("S/empty"), sh("path"), FlakeValue::Ref(a("steward"))));

            let (_, failures) = read_all(&facts);

            assert!(
                matches!(&failures[0], ShapeError::EmptyBranch { .. }),
                "{failures:#?}"
            );
        }

        /// Several terms on one property shape remain an implicit `And`, and a
        /// single term remains itself — rendering `and` in a violation for a
        /// lone `minCount` would tell a reader nothing.
        #[test]
        fn a_single_term_is_not_wrapped_in_an_and() {
            let mut facts = vec![
                node_shape("S"),
                f(a("S"), sh("targetClass"), FlakeValue::Ref(a("T"))),
                f(a("S"), sh("property"), FlakeValue::Ref(a("S/p"))),
            ];
            facts.extend(requires("S/p", "owner"));
            facts.push(subject_of("T"));

            let report = validate(&read_all(&facts).0, &facts);

            assert_eq!(report.violations[0].constraint, "minCount");
        }
    }
}
