//! Plan computation — Epic 20 Slice B.
//!
//! **The trust-building step.** Decision 1: nothing mutates until a human has
//! seen what would. That makes two properties load-bearing rather than nice —
//! the plan must be *complete* (every entity classified, so nothing happens
//! that was not shown) and *deterministic* (byte-identical for identical
//! inputs, so it is diffable in CI and a reviewer can tell a real change from
//! reordering noise).
//!
//! Determinism here is structural rather than a sorting step bolted on the
//! end: `Declarations` is a `BTreeMap`, live state is collected into one, and
//! the plan is built by walking them in key order. There is no map iteration
//! anywhere in this module to accidentally reintroduce it — which is Slice
//! B's own stated mutator watch ("non-deterministic ordering must fail the
//! determinism test — this is the likely real bug").

use std::collections::BTreeMap;

use crate::declaration::Declaration;
use crate::validate::Declarations;

/// What would happen to one entity.
///
/// `NoChange` is a first-class outcome, not an absence: a plan that omitted
/// unchanged entities could not answer "did you consider this one?", which is
/// the question a reviewer actually has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Create,
    /// Per-field `before → after`, only for fields the declaration actually
    /// declares — decision 4. A field absent from the declaration is "not
    /// declared", never "set to null", so it never appears here and is never
    /// touched by the apply that follows.
    Update {
        fields: Vec<FieldChange>,
    },
    NoChange,
    /// Live, within scope, and not declared. Only ever *shown* unless
    /// `--prune` is given (Slice D) — decision 5.
    Prune,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEntity {
    pub fully_qualified_name: String,
    pub kind: String,
    pub change: Change,
}

/// The whole plan, in a stable order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub entities: Vec<PlannedEntity>,
}

/// Live state as the planner needs it — deliberately a small struct rather
/// than `graph_owl_core::Asset`, because the CLI is an HTTP client (decision
/// 6) and must not be coupled to the server's internal representation to
/// compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveEntity {
    /// The catalog's own id. Carried because the write API addresses a
    /// parent **by id, not by FQN** — a mismatch found only by reading the
    /// server's actual DTO, and one a test double would happily have
    /// accepted forever.
    pub id: String,
    pub fully_qualified_name: String,
    pub kind: String,
    pub description: Option<String>,
}

impl Plan {
    /// Whether anything would actually change — what the exit code is
    /// derived from, so CI can branch without parsing text.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.entities
            .iter()
            .any(|entity| entity.change != Change::NoChange)
    }

    #[must_use]
    pub fn counts(&self) -> PlanCounts {
        let mut counts = PlanCounts::default();
        for entity in &self.entities {
            match entity.change {
                Change::Create => counts.create += 1,
                Change::Update { .. } => counts.update += 1,
                Change::NoChange => counts.no_change += 1,
                Change::Prune => counts.prune += 1,
            }
        }
        counts
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub create: usize,
    pub update: usize,
    pub no_change: usize,
    pub prune: usize,
}

/// Diffs declarations against live state.
///
/// `live` is everything **within the declared scope** — the caller is
/// responsible for that filter, because scope is what decision 2 makes
/// authoritative and computing it from the declarations alone would make a
/// tree that declares nothing look like a request to prune the catalog.
#[must_use]
pub fn compute(declarations: &Declarations, live: &[LiveEntity]) -> Plan {
    let live_by_fqn: BTreeMap<&str, &LiveEntity> = live
        .iter()
        .map(|entity| (entity.fully_qualified_name.as_str(), entity))
        .collect();

    let mut entities = Vec::new();

    // Declared: create, update, or no-change. `BTreeMap` iteration is
    // ordered, so this half of the plan is deterministic by construction.
    for (fqn, (_file, declaration)) in &declarations.by_fqn {
        let change = match live_by_fqn.get(fqn.as_str()) {
            None => Change::Create,
            Some(existing) => {
                let fields = declared_field_changes(declaration, existing);
                if fields.is_empty() {
                    Change::NoChange
                } else {
                    Change::Update { fields }
                }
            }
        };
        entities.push(PlannedEntity {
            fully_qualified_name: fqn.clone(),
            kind: declaration.kind.clone(),
            change,
        });
    }

    // Live but undeclared, within scope. Also ordered — `live_by_fqn` is a
    // `BTreeMap`, so this does not depend on the order the API returned.
    for (fqn, existing) in &live_by_fqn {
        if !declarations.by_fqn.contains_key(*fqn) {
            entities.push(PlannedEntity {
                fully_qualified_name: (*fqn).to_string(),
                kind: existing.kind.clone(),
                change: Change::Prune,
            });
        }
    }

    Plan { entities }
}

/// **Only fields the declaration declares.** Decision 4's failure mode is
/// treating absent-from-declaration as null, which would silently reset every
/// field a person curated in the console the first time anyone ran `apply`.
/// An absent `description` produces no `FieldChange`, so the apply that
/// follows has nothing to send for it.
fn declared_field_changes(declaration: &Declaration, existing: &LiveEntity) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    if let Some(declared) = &declaration.metadata.description
        && existing.description.as_deref() != Some(declared.as_str())
    {
        changes.push(FieldChange {
            field: "description".to_string(),
            before: existing.description.clone(),
            after: Some(declared.clone()),
        });
    }

    changes
}

/// Renders the plan for a human, deterministically.
///
/// Text to stdout, per the CLI conventions — `--format json` is the
/// machine-readable path and lives beside this rather than replacing it.
#[must_use]
pub fn render(plan: &Plan) -> String {
    let mut out = String::new();
    for entity in &plan.entities {
        match &entity.change {
            Change::Create => {
                out.push_str(&format!(
                    "+ create  {} ({})\n",
                    entity.fully_qualified_name, entity.kind
                ));
            }
            Change::Update { fields } => {
                out.push_str(&format!(
                    "~ update  {} ({})\n",
                    entity.fully_qualified_name, entity.kind
                ));
                for field in fields {
                    out.push_str(&format!(
                        "    {}: {} -> {}\n",
                        field.field,
                        field.before.as_deref().unwrap_or("(unset)"),
                        field.after.as_deref().unwrap_or("(unset)")
                    ));
                }
            }
            Change::NoChange => {
                out.push_str(&format!(
                    "  no-change {} ({})\n",
                    entity.fully_qualified_name, entity.kind
                ));
            }
            Change::Prune => {
                out.push_str(&format!(
                    "- prune   {} ({})\n",
                    entity.fully_qualified_name, entity.kind
                ));
            }
        }
    }
    let counts = plan.counts();
    out.push_str(&format!(
        "\n{} to create, {} to update, {} unchanged, {} to prune\n",
        counts.create, counts.update, counts.no_change, counts.prune
    ));
    out
}
