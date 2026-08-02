//! Drift detection — Epic 20 Slice E.
//!
//! **Reported, never corrected.** Decision 3, and the reason `drift` is a
//! separate command rather than a flag on `apply`: automatic correction turns
//! every manual fix into a silent revert, and someone who edited a
//! description in the console at 2am to explain an incident would find it
//! gone with no record that it ever existed.
//!
//! The distinction this module exists to draw is one a plain diff cannot:
//! **"someone changed live state" is a different event from "the file changed
//! and was never applied"**, and they call for opposite responses — the first
//! wants a conversation, the second wants `apply`.

use crate::plan::{Change, Plan};

/// Which side moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftKind {
    /// Live state differs from what was last applied — someone edited outside
    /// the declarations. **The interesting case**: it means the repository is
    /// no longer the truth, and reverting it silently is what decision 3
    /// forbids.
    LiveEdited,
    /// The declarations differ from live, and nothing has been applied since.
    /// Ordinary pending work, not a conflict — `apply` resolves it.
    Unapplied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub fully_qualified_name: String,
    pub kind: DriftKind,
    pub detail: String,
}

/// A drift report. Deliberately carries no method that mutates anything —
/// the type itself is the guarantee that running `drift` cannot write, which
/// is stronger than a comment saying so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriftReport {
    pub drifted: Vec<Drift>,
}

impl DriftReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.drifted.is_empty()
    }
}

/// Computes drift from a plan plus knowledge of what was last applied.
///
/// `last_applied_matches` answers, per FQN, "does live state still equal what
/// this tool last wrote?". Without it every difference looks the same and the
/// report cannot distinguish the two cases above — which is the whole value
/// of the command. Slice F's export is what makes that record available;
/// until then a caller that cannot answer passes `None` and gets the
/// conservative reading (`Unapplied`), never a false accusation that someone
/// edited live state.
#[must_use]
pub fn detect(plan: &Plan, last_applied_matches: &dyn Fn(&str) -> Option<bool>) -> DriftReport {
    let mut drifted = Vec::new();

    for entity in &plan.entities {
        let kind = match &entity.change {
            Change::NoChange => continue,
            Change::Update { .. } | Change::Create | Change::Prune => {
                match last_applied_matches(&entity.fully_qualified_name) {
                    // Live no longer matches what was applied: someone else
                    // moved it.
                    Some(false) => DriftKind::LiveEdited,
                    // Live matches what was applied, but differs from the
                    // files — so the files moved.
                    Some(true) | None => DriftKind::Unapplied,
                }
            }
        };

        drifted.push(Drift {
            fully_qualified_name: entity.fully_qualified_name.clone(),
            kind,
            detail: match &entity.change {
                Change::Create => "declared but absent from the catalog".to_string(),
                Change::Prune => "present in the catalog but no longer declared".to_string(),
                Change::Update { fields } => fields
                    .iter()
                    .map(|f| {
                        format!(
                            "{}: {} -> {}",
                            f.field,
                            f.before.as_deref().unwrap_or("(unset)"),
                            f.after.as_deref().unwrap_or("(unset)")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
                Change::NoChange => unreachable!("filtered above"),
            },
        });
    }

    DriftReport { drifted }
}
