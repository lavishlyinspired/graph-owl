//! Applying a plan — Epic 20 Slice C.
//!
//! **Parents before children, always.** An FQN is derived from the parent
//! chain, so a child applied first has no parent to resolve against and the
//! catalog would refuse it — the same ordering guarantee Epic 15 makes a
//! connector contract, arrived at here by sorting rather than by trusting the
//! author of a directory to name their files helpfully.
//!
//! **A per-entity failure does not abort the run.** One unappliable entity
//! must not cost the other nine hundred, which is the same reasoning Epic
//! 16's batch push already applies — and the reason this returns a report
//! rather than a `Result`.

use crate::plan::{Change, Plan, PlannedEntity};

/// What one entity's apply did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Updated,
    /// Nothing sent — the entity already matched. **Sending an unchanged
    /// update anyway would produce a version and a change event**, which is
    /// exactly what the "second apply is a no-op" criterion forbids.
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityResult {
    pub fully_qualified_name: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub results: Vec<EntityResult>,
}

impl ApplyReport {
    #[must_use]
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, Outcome::Failed(_)))
            .count()
    }

    #[must_use]
    pub fn created(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.outcome == Outcome::Created)
            .count()
    }

    #[must_use]
    pub fn updated(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.outcome == Outcome::Updated)
            .count()
    }
}

/// Orders a plan's entities so every parent precedes its children.
///
/// Sorting by FQN **segment depth, then lexically** is enough and needs no
/// graph: an FQN contains its own ancestry, so `a` sorts before `a.b` before
/// `a.b.c` by construction. A topological sort would be the same answer at
/// more cost, and would need a cycle check for a cycle the FQN format cannot
/// express.
#[must_use]
pub fn in_dependency_order(plan: &Plan) -> Vec<&PlannedEntity> {
    let mut ordered: Vec<&PlannedEntity> = plan
        .entities
        .iter()
        .filter(|entity| matches!(entity.change, Change::Create | Change::Update { .. }))
        .collect();
    ordered.sort_by(|a, b| {
        let depth = |fqn: &str| fqn.matches('.').count();
        depth(&a.fully_qualified_name)
            .cmp(&depth(&b.fully_qualified_name))
            .then_with(|| a.fully_qualified_name.cmp(&b.fully_qualified_name))
    });
    ordered
}

/// Whether an apply may proceed without an explicit `--yes`.
///
/// **Refuses rather than assumes** when there is no TTY and no `--yes`: a
/// pipeline that meant to pass `--yes` and did not must fail loudly, not
/// mutate a catalog because nobody was watching. Decision 1 is that a tool
/// which mutates without showing its plan will not be trusted; silently
/// treating "no human present" as consent is the same failure wearing a
/// different hat.
#[must_use]
pub const fn may_proceed(yes_flag: bool, stdin_is_tty: bool) -> bool {
    yes_flag || stdin_is_tty
}

/// Resolves parent FQNs to catalog ids as it goes.
///
/// **Why this exists at all**: the write API addresses a parent by id, and a
/// just-created parent's id is only knowable from the response that created
/// it. That makes apply inherently stateful — and tractable only because it
/// runs parents-first, so every child's parent has already been seen.
///
/// Seeded from live state so an existing parent needs no write to be
/// addressable.
pub struct ParentIds {
    by_fqn: std::collections::HashMap<String, String>,
}

impl ParentIds {
    #[must_use]
    pub fn from_live(live: &[crate::plan::LiveEntity]) -> Self {
        Self {
            by_fqn: live
                .iter()
                .map(|entity| (entity.fully_qualified_name.clone(), entity.id.clone()))
                .collect(),
        }
    }

    /// Records what a write returned, so children sent later can address it.
    pub fn learn(&mut self, fully_qualified_name: &str, id: String) {
        self.by_fqn.insert(fully_qualified_name.to_string(), id);
    }

    /// The id for a parent FQN, if known.
    ///
    /// `None` for a genuinely unknown parent is deliberately *not* an error
    /// here: validation already refused a dangling parent locally, so
    /// reaching this with `None` means the parent is live-but-out-of-scope,
    /// and the catalog is the right place to refuse that — it knows whether
    /// the caller may see it.
    #[must_use]
    pub fn get(&self, parent_fqn: &str) -> Option<&str> {
        self.by_fqn.get(parent_fqn).map(String::as_str)
    }
}
