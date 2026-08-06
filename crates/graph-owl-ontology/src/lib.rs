//! Shape and constraint types — Epic 5.
//!
//! Pure data, no evaluation: `graph-owl-constraint` compiles and runs these,
//! and keeping the vocabulary here means a shape can be *stated* by anything —
//! a migration, a parser, an API — without depending on the validator.
//!
//! The vocabulary follows SHACL's core constraint components as specified by
//! W3C. It is deliberately a **subset**; see `plans/00k-standards-conformance.md`
//! for what is and is not implemented, and do not describe this as SHACL
//! conformance.

use graph_owl_core::flake::{FlakeValue, Sid};

pub mod pack;
pub mod profile;

/// The kind a value must be, independent of what it says.
///
/// Mirrors the `FlakeValue` variants rather than XSD names, because the check
/// runs against stored flakes: a datatype that cannot be decided by looking at
/// one is a datatype the validator would have to guess at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataType {
    Ref,
    String,
    Boolean,
    Int,
    Float,
    Instant,
    Json,
    Bytes,
    Uuid,
    Duration,
}

impl DataType {
    /// Does this value carry this type?
    #[must_use]
    pub fn matches(self, value: &FlakeValue) -> bool {
        matches!(
            (self, value),
            (Self::Ref, FlakeValue::Ref(_))
                | (Self::String, FlakeValue::String(_))
                | (Self::Boolean, FlakeValue::Boolean(_))
                | (Self::Int, FlakeValue::Int(_))
                | (Self::Float, FlakeValue::Float(_))
                | (Self::Instant, FlakeValue::Instant(_))
                | (Self::Json, FlakeValue::Json(_))
                | (Self::Bytes, FlakeValue::Bytes(_))
                | (Self::Uuid, FlakeValue::Uuid(_))
                | (Self::Duration, FlakeValue::Duration(_))
        )
    }
}

/// Reference or literal — the distinction SHACL calls node kind.
///
/// Coarser than SHACL's six because this graph has two: a value either points
/// at another node or it does not. Blank nodes do not exist here, so the four
/// combinations involving them would be constraints nothing could ever violate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Ref,
    Literal,
}

/// How loudly a finding speaks.
///
/// Only `Violation` fails conformance. A warning that failed conformance would
/// make every shape a blocking one, and the reason severities exist is that
/// some findings are advice.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Violation,
    Warning,
    Info,
}

/// What a shape applies to.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Everything carrying this type.
    Class(Sid),
    /// A named list — SHACL's `sh:targetNode`.
    Subjects(Vec<Sid>),
    /// Every subject that states this predicate.
    SubjectsOf(Sid),
    /// Every object of this predicate.
    ObjectsOf(Sid),
    /// Every literal of this datatype, wherever it appears.
    ///
    /// The only target that selects *values* rather than nodes, which is why
    /// its focus is the subject holding the literal: a literal has no identity
    /// of its own to report a violation against, and "the string `draft` is
    /// wrong" names nothing a steward can open.
    LiteralsOf(DataType),
    /// The shape is itself a class; its instances are the target.
    ///
    /// SHACL's implicit class targeting. Distinct from [`Target::Class`] only
    /// in where the class name comes from — the shape's own id rather than a
    /// separate statement — and worth having because a shape named after the
    /// class it constrains should not have to repeat itself.
    ImplicitClass,
}

/// One thing that must hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    MinCount {
        path: Sid,
        n: usize,
    },
    MaxCount {
        path: Sid,
        n: usize,
    },
    Datatype {
        path: Sid,
        dt: DataType,
    },
    NodeKind {
        path: Sid,
        kind: NodeKind,
    },
    In {
        path: Sid,
        allowed: Vec<FlakeValue>,
    },
    Pattern {
        path: Sid,
        regex: String,
    },
    MinInclusive {
        path: Sid,
        v: f64,
    },
    MaxInclusive {
        path: Sid,
        v: f64,
    },
    MinLength {
        path: Sid,
        n: usize,
    },
    MaxLength {
        path: Sid,
        n: usize,
    },
    /// The object of `path` must itself carry this type.
    Class {
        path: Sid,
        expected: Sid,
    },
    HasValue {
        path: Sid,
        v: FlakeValue,
    },
    Not(Box<Constraint>),
    And(Vec<Constraint>),
    Or(Vec<Constraint>),
}

impl Constraint {
    /// The predicate this constraint is about, when it is about one.
    ///
    /// `None` for the logical combinators: an `Or` over two paths is about
    /// neither, and naming one of them in a report would point a reader at half
    /// the reason it failed.
    #[must_use]
    pub fn path(&self) -> Option<&Sid> {
        match self {
            Self::MinCount { path, .. }
            | Self::MaxCount { path, .. }
            | Self::Datatype { path, .. }
            | Self::NodeKind { path, .. }
            | Self::In { path, .. }
            | Self::Pattern { path, .. }
            | Self::MinInclusive { path, .. }
            | Self::MaxInclusive { path, .. }
            | Self::MinLength { path, .. }
            | Self::MaxLength { path, .. }
            | Self::Class { path, .. }
            | Self::HasValue { path, .. } => Some(path),
            Self::Not(_) | Self::And(_) | Self::Or(_) => None,
        }
    }

    /// The machine-matchable name a report carries.
    ///
    /// A caller filtering for "every minCount failure" needs a token, not a
    /// sentence — the sentence is the message, and it is written for a person.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MinCount { .. } => "minCount",
            Self::MaxCount { .. } => "maxCount",
            Self::Datatype { .. } => "datatype",
            Self::NodeKind { .. } => "nodeKind",
            Self::In { .. } => "in",
            Self::Pattern { .. } => "pattern",
            Self::MinInclusive { .. } => "minInclusive",
            Self::MaxInclusive { .. } => "maxInclusive",
            Self::MinLength { .. } => "minLength",
            Self::MaxLength { .. } => "maxLength",
            Self::Class { .. } => "class",
            Self::HasValue { .. } => "hasValue",
            Self::Not(_) => "not",
            Self::And(_) => "and",
            Self::Or(_) => "or",
        }
    }
}

/// A named set of constraints and what they apply to.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub id: Sid,
    pub target: Target,
    pub constraints: Vec<Constraint>,
    pub severity: Severity,
    /// Overrides the generated message. A shape author knows *why* the rule
    /// exists; the validator only knows that it failed.
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn a_datatype_matches_its_own_value_and_no_other() {
        let cases: [(DataType, FlakeValue); 10] = [
            (DataType::Ref, FlakeValue::Ref(Sid::dsc("x"))),
            (DataType::String, FlakeValue::String("x".into())),
            (DataType::Boolean, FlakeValue::Boolean(true)),
            (DataType::Int, FlakeValue::Int(1)),
            (DataType::Float, FlakeValue::Float(1.0)),
            (DataType::Instant, FlakeValue::Instant(Utc::now())),
            (DataType::Json, FlakeValue::Json("{}".into())),
            (DataType::Bytes, FlakeValue::Bytes(vec![1])),
            (DataType::Uuid, FlakeValue::Uuid(uuid::Uuid::nil())),
            (DataType::Duration, FlakeValue::Duration(1)),
        ];

        for (dt, value) in &cases {
            assert!(dt.matches(value), "{dt:?} should match its own value");
            // And the negative, which is what makes the table worth writing: a
            // datatype that matched everything would pass every positive
            // assertion above.
            for (other, _) in cases.iter().filter(|(other, _)| other != dt) {
                assert!(
                    !other.matches(value),
                    "{other:?} must not match a {dt:?} value"
                );
            }
        }
    }

    /// An `Int` is not a `Float`. Accepting one for the other looks harmless
    /// until a confidence of `1` passes a `0.0..=1.0` range check written to
    /// reject `1.5` — the datatype constraint is what stops a value reaching a
    /// range check that cannot read it.
    #[test]
    fn an_integer_does_not_satisfy_a_float_constraint() {
        assert!(!DataType::Float.matches(&FlakeValue::Int(1)));
        assert!(!DataType::Int.matches(&FlakeValue::Float(1.0)));
    }

    fn every_kind(p: &Sid) -> Vec<Constraint> {
        vec![
            Constraint::MinCount {
                path: p.clone(),
                n: 1,
            },
            Constraint::MaxCount {
                path: p.clone(),
                n: 1,
            },
            Constraint::Datatype {
                path: p.clone(),
                dt: DataType::String,
            },
            Constraint::NodeKind {
                path: p.clone(),
                kind: NodeKind::Literal,
            },
            Constraint::In {
                path: p.clone(),
                allowed: vec![],
            },
            Constraint::Pattern {
                path: p.clone(),
                regex: "x".into(),
            },
            Constraint::MinInclusive {
                path: p.clone(),
                v: 0.0,
            },
            Constraint::MaxInclusive {
                path: p.clone(),
                v: 1.0,
            },
            Constraint::MinLength {
                path: p.clone(),
                n: 1,
            },
            Constraint::MaxLength {
                path: p.clone(),
                n: 1,
            },
            Constraint::Class {
                path: p.clone(),
                expected: Sid::dsc("C"),
            },
            Constraint::HasValue {
                path: p.clone(),
                v: FlakeValue::Int(1),
            },
        ]
    }

    #[test]
    fn every_path_bearing_constraint_reports_its_path() {
        let p = Sid::dsc("name");
        for constraint in every_kind(&p) {
            assert_eq!(
                constraint.path(),
                Some(&p),
                "{} lost its path",
                constraint.kind()
            );
        }

        // And the combinators report none.
        let inner = Constraint::MinCount {
            path: p.clone(),
            n: 1,
        };
        for combinator in [
            Constraint::Not(Box::new(inner.clone())),
            Constraint::And(vec![inner.clone()]),
            Constraint::Or(vec![inner]),
        ] {
            assert_eq!(combinator.path(), None, "{}", combinator.kind());
        }
    }

    /// Every kind is distinct. Two constraints sharing a token would make a
    /// report unfilterable in exactly the case a filter is for.
    #[test]
    fn every_constraint_kind_has_its_own_token() {
        let p = Sid::dsc("x");
        let mut all = every_kind(&p);
        all.push(Constraint::Not(Box::new(Constraint::And(vec![]))));
        all.push(Constraint::And(vec![]));
        all.push(Constraint::Or(vec![]));

        let tokens: std::collections::HashSet<&str> = all.iter().map(Constraint::kind).collect();
        assert_eq!(tokens.len(), all.len(), "{tokens:?}");
    }

    /// Severity orders loudest first, which is the order a queue sorts by. A
    /// reversed ordering buries the failures under the advice.
    #[test]
    fn severity_orders_violations_ahead_of_advice() {
        let mut severities = vec![Severity::Info, Severity::Violation, Severity::Warning];
        severities.sort_unstable();
        assert_eq!(
            severities,
            vec![Severity::Violation, Severity::Warning, Severity::Info]
        );
    }
}
