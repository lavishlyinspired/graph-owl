//! Planning a push — Epic 16 Slice A.
//!
//! `POST /ingest` takes entities, relationships and lineage in one call, and two
//! of its acceptance criteria are really one algorithm:
//!
//! - "parents applied before children within the batch"
//! - "a relationship whose endpoints are in the same batch resolves"
//!
//! Both mean the same thing: **a pusher cannot be asked to submit in dependency
//! order.** A script walking a source emits what it finds when it finds it, and
//! requiring a topological submission would push the catalog's model onto every
//! adapter author — which decision 1 explicitly refuses ("custom adapters run
//! out-of-process… they ship on the adapter author's schedule").
//!
//! So the order is computed here, from the FQNs the batch declares, and it is
//! computed *purely*: no storage, no I/O, exhaustively testable.

use std::collections::{HashMap, HashSet};

/// One entity as a push declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// Position in the submitted batch.
    ///
    /// **Carried, never recomputed.** The `207` reports per-item status by index,
    /// and an index that referred to a *sorted* position would send a client to
    /// the wrong item — the one place a wrong number is silently actionable.
    pub index: usize,
    pub fully_qualified_name: String,
    /// The FQN of the containing entity, if this one has a parent.
    pub parent_fqn: Option<String>,
}

/// Why a batch cannot be ordered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// Two entities in one batch claim the same FQN.
    ///
    /// Refused rather than resolved: the batch states two different intents for
    /// one entity and nothing here can know which is meant. Applying both would
    /// make the result depend on submission order, which is exactly what this
    /// module exists to make irrelevant.
    #[error("`{fqn}` appears twice in this batch, at items {first} and {second}")]
    Duplicate {
        fqn: String,
        first: usize,
        second: usize,
    },
    /// Following parents inside the batch leads back to where it started.
    #[error("items {indexes:?} form a containment cycle")]
    Cycle { indexes: Vec<usize> },
}

/// The order to apply a batch in, as indexes into the submitted list.
///
/// A parent that is **not** in the batch is not an error: it may already exist in
/// the catalog, and deciding that is storage's job. What this guarantees is only
/// that where a batch contains both, the parent comes first.
///
/// **Stable otherwise.** Items with no dependency between them keep their
/// submitted order, so a client reading a `207` sees its own batch reflected back
/// rather than a permutation it has to re-derive.
///
/// # Errors
///
/// [`PlanError::Duplicate`] when two items claim one FQN — the batch states two
/// intents for the same entity and nothing here can know which is meant.
/// [`PlanError::Cycle`] when following parents inside the batch returns to where
/// it started.
pub fn apply_order(drafts: &[Draft]) -> Result<Vec<usize>, PlanError> {
    // Index by FQN so a parent reference resolves in one lookup. Built first
    // because a duplicate makes every later question ambiguous.
    let mut by_fqn: HashMap<&str, &Draft> = HashMap::new();
    for draft in drafts {
        if let Some(existing) = by_fqn.insert(draft.fully_qualified_name.as_str(), draft) {
            return Err(PlanError::Duplicate {
                fqn: draft.fully_qualified_name.clone(),
                first: existing.index.min(draft.index),
                second: existing.index.max(draft.index),
            });
        }
    }

    // Depth-first, emitting a parent before the child that named it. Iterative
    // rather than recursive: the depth is client-supplied, and a deep batch should
    // be slow rather than a stack overflow.
    let mut emitted: Vec<usize> = Vec::with_capacity(drafts.len());
    let mut done: HashSet<&str> = HashSet::new();
    let mut unplaceable: Vec<usize> = Vec::new();

    for draft in drafts {
        if done.contains(draft.fully_qualified_name.as_str()) {
            continue;
        }
        // The chain from this draft up to its highest in-batch ancestor. Collected
        // before anything is emitted, so a cycle is discovered without having
        // already written half of it out.
        let mut chain: Vec<&Draft> = Vec::new();
        let mut walking: HashSet<&str> = HashSet::new();
        let mut current = draft;
        let cyclic = loop {
            if !walking.insert(current.fully_qualified_name.as_str()) {
                break true;
            }
            chain.push(current);
            let Some(parent_fqn) = current.parent_fqn.as_deref() else {
                break false;
            };
            // A parent outside the batch stops the walk rather than failing it: it
            // may already exist in the catalog, and deciding that is storage's job.
            let Some(parent) = by_fqn.get(parent_fqn) else {
                break false;
            };
            if done.contains(parent_fqn) {
                break false;
            }
            current = parent;
        };

        if cyclic {
            unplaceable.extend(chain.iter().map(|d| d.index));
            continue;
        }
        // Root-most first: the walk collected child → parent, and application
        // needs the reverse.
        for placed in chain.iter().rev() {
            if done.insert(placed.fully_qualified_name.as_str()) {
                emitted.push(placed.index);
            }
        }
    }

    if !unplaceable.is_empty() {
        // Deduplicated and sorted: every member of a cycle is reached from every
        // other member's walk, so the raw list repeats each one. A client reading
        // "items [2, 2, 3, 3] form a cycle" would reasonably wonder what the
        // repetition meant.
        unplaceable.sort_unstable();
        unplaceable.dedup();
        return Err(PlanError::Cycle {
            indexes: unplaceable,
        });
    }
    Ok(emitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(index: usize, fqn: &str, parent: Option<&str>) -> Draft {
        Draft {
            index,
            fully_qualified_name: fqn.to_string(),
            parent_fqn: parent.map(ToString::to_string),
        }
    }

    /// The order `apply_order` produced, as FQNs — easier to read than indexes
    /// when a test fails.
    fn names(drafts: &[Draft], order: &[usize]) -> Vec<String> {
        order
            .iter()
            .map(|i| {
                drafts
                    .iter()
                    .find(|d| d.index == *i)
                    .expect("an index in the batch")
                    .fully_qualified_name
                    .clone()
            })
            .collect()
    }

    #[test]
    fn an_empty_batch_orders_to_nothing() {
        assert_eq!(apply_order(&[]).expect("order"), Vec::<usize>::new());
    }

    // **The criterion.** A pusher emits what it finds when it finds it, so a child
    // arriving before its parent is the normal case, not an error to report.
    #[test]
    fn a_child_submitted_first_is_applied_after_its_parent() {
        let drafts = vec![
            draft(0, "svc.db.public.orders", Some("svc.db.public")),
            draft(1, "svc.db.public", Some("svc.db")),
            draft(2, "svc.db", Some("svc")),
            draft(3, "svc", None),
        ];

        let order = apply_order(&drafts).expect("order");

        assert_eq!(
            names(&drafts, &order),
            vec!["svc", "svc.db", "svc.db.public", "svc.db.public.orders"]
        );
    }

    // Four levels, because a single pass that only pulls direct parents forward
    // produces the right answer for two and the wrong one for four.
    #[test]
    fn a_deep_chain_is_ordered_root_first_however_it_arrives() {
        let drafts = vec![
            draft(0, "a.b.c.d", Some("a.b.c")),
            draft(1, "a.b", Some("a")),
            draft(2, "a.b.c", Some("a.b")),
            draft(3, "a", None),
        ];

        let order = apply_order(&drafts).expect("order");

        assert_eq!(names(&drafts, &order), vec!["a", "a.b", "a.b.c", "a.b.c.d"]);
    }

    // **Stable otherwise.** A client reading a `207` should see its own batch
    // reflected back, not a permutation it has to re-derive.
    #[test]
    fn independent_items_keep_their_submitted_order() {
        let drafts = vec![
            draft(0, "zebra", None),
            draft(1, "apple", None),
            draft(2, "mango", None),
        ];

        let order = apply_order(&drafts).expect("order");

        assert_eq!(order, vec![0, 1, 2]);
    }

    // A parent outside the batch is not an error: it may already exist in the
    // catalog, and deciding that is storage's job rather than this function's.
    #[test]
    fn a_parent_outside_the_batch_is_not_an_error() {
        let drafts = vec![draft(0, "svc.db.public", Some("svc.db"))];

        let order = apply_order(&drafts).expect("order");

        assert_eq!(order, vec![0]);
    }

    // Indexes are the submitted positions, not positions in the sorted output —
    // the `207` reports per-item status by index, and a wrong number here sends a
    // client to the wrong item.
    #[test]
    fn the_order_carries_submitted_indexes_not_sorted_positions() {
        let drafts = vec![draft(7, "child", Some("parent")), draft(3, "parent", None)];

        assert_eq!(apply_order(&drafts).expect("order"), vec![3, 7]);
    }

    // Two entities claiming one FQN state two intents for the same thing. Applying
    // both would make the result depend on submission order, which is precisely
    // what this module exists to make irrelevant.
    #[test]
    fn a_duplicate_fqn_is_refused_naming_both_items() {
        let drafts = vec![
            draft(0, "svc", None),
            draft(1, "svc.db", Some("svc")),
            draft(2, "svc", None),
        ];

        let err = apply_order(&drafts).expect_err("a duplicate should be refused");

        assert_eq!(
            err,
            PlanError::Duplicate {
                fqn: "svc".to_string(),
                first: 0,
                second: 2
            }
        );
    }

    // Contrived, but a batch is client-supplied and a cycle would otherwise be an
    // infinite walk. Reported rather than looped.
    #[test]
    fn a_containment_cycle_is_reported_rather_than_walked() {
        let drafts = vec![draft(0, "a", Some("b")), draft(1, "b", Some("a"))];

        let err = apply_order(&drafts).expect_err("a cycle should be refused");

        let PlanError::Cycle { mut indexes } = err else {
            panic!("expected a cycle, got {err:?}");
        };
        indexes.sort_unstable();
        assert_eq!(indexes, vec![0, 1]);
    }

    #[test]
    fn a_self_parenting_item_is_a_cycle() {
        let drafts = vec![draft(0, "a", Some("a"))];

        assert!(matches!(apply_order(&drafts), Err(PlanError::Cycle { .. })));
    }

    // And the negative that makes the two above about cycles rather than about a
    // walk that refuses anything deep.
    #[test]
    fn a_long_legitimate_chain_is_not_a_cycle() {
        let drafts: Vec<Draft> = (0..30)
            .map(|i| {
                let fqn = (0..=i).map(|_| "a").collect::<Vec<_>>().join(".");
                let parent = if i == 0 {
                    None
                } else {
                    Some((0..i).map(|_| "a").collect::<Vec<_>>().join("."))
                };
                draft(i, &fqn, parent.as_deref())
            })
            .collect();

        assert_eq!(apply_order(&drafts).expect("order").len(), 30);
    }

    // A cycle elsewhere must not take an unrelated healthy subtree with it —
    // the report names the items that are actually unplaceable.
    #[test]
    fn a_cycle_names_only_the_items_in_it() {
        let drafts = vec![
            draft(0, "good", None),
            draft(1, "good.child", Some("good")),
            draft(2, "x", Some("y")),
            draft(3, "y", Some("x")),
        ];

        let PlanError::Cycle { mut indexes } = apply_order(&drafts).expect_err("cycle") else {
            panic!("expected a cycle");
        };
        indexes.sort_unstable();
        assert_eq!(indexes, vec![2, 3]);
    }
}
