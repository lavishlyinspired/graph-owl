//! Fully-qualified names: the hierarchy's addressing scheme.
//!
//! An FQN is derived, never client-set. Two entities with the same address are
//! the same entity, which is what makes a connector's re-run converge instead
//! of duplicating (Epic 15) and what lets Epic 17 resolve identity at all.

use std::fmt;

/// Separator between levels. A dot, because that is what every warehouse's own
/// tooling uses, so an operator can paste an FQN from a SQL client.
pub const SEPARATOR: char = '.';

/// Why a set of segments could not be joined into an FQN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FqnError {
    /// A segment was empty — `warehouse..orders` addresses nothing.
    EmptySegment {
        /// The index of the empty segment.
        position: usize,
    },
    /// A segment contained the separator, which would make the FQN ambiguous:
    /// `a.b` as one segment is indistinguishable from two segments `a` and `b`.
    SeparatorInSegment {
        /// The index of the offending segment.
        position: usize,
        /// The segment that contained the separator.
        segment: String,
    },
}

impl fmt::Display for FqnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FqnError::EmptySegment { position } => {
                write!(f, "name segment {position} is empty")
            }
            FqnError::SeparatorInSegment { position, segment } => write!(
                f,
                "name segment {position} ('{segment}') contains '{SEPARATOR}', \
                 which would make the fully-qualified name ambiguous"
            ),
        }
    }
}

/// Joins segments into an FQN, rejecting anything that would make it ambiguous.
///
/// # Errors
///
/// Returns [`FqnError`] if a segment is empty or contains the separator.
pub fn derive(segments: &[&str]) -> Result<String, FqnError> {
    for (position, segment) in segments.iter().enumerate() {
        if segment.trim().is_empty() {
            return Err(FqnError::EmptySegment { position });
        }
        if segment.contains(SEPARATOR) {
            return Err(FqnError::SeparatorInSegment {
                position,
                segment: (*segment).to_string(),
            });
        }
    }
    Ok(segments.join(&SEPARATOR.to_string()))
}

/// Appends one segment to an already-joined parent FQN.
///
/// Distinct from [`derive`] because the parent is *already* a joined FQN and
/// legitimately contains separators — passing it to `derive` as a segment is a
/// mistake the type system cannot catch, so it gets its own function.
///
/// # Errors
///
/// Returns [`FqnError`] if `name` is empty or contains the separator.
pub fn child_of(parent_fqn: &str, name: &str) -> Result<String, FqnError> {
    // Only the new segment is validated; the parent is trusted because it was
    // itself produced by this module.
    let leaf = derive(&[name])?;
    Ok(format!("{parent_fqn}{SEPARATOR}{leaf}"))
}

/// Splits an FQN back into its segments.
#[must_use]
pub fn segments(fqn: &str) -> Vec<&str> {
    fqn.split(SEPARATOR).collect()
}

/// The FQN of the parent, or `None` for a root.
#[must_use]
pub fn parent(fqn: &str) -> Option<&str> {
    fqn.rsplit_once(SEPARATOR).map(|(head, _)| head)
}

/// The last segment — the entity's own name.
#[must_use]
pub fn leaf(fqn: &str) -> &str {
    fqn.rsplit_once(SEPARATOR).map_or(fqn, |(_, tail)| tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_join_with_dots() {
        assert_eq!(
            derive(&["warehouse", "sales", "public", "orders"]),
            Ok("warehouse.sales.public.orders".to_string())
        );
    }

    #[test]
    fn a_single_segment_is_its_own_fqn() {
        assert_eq!(derive(&["warehouse"]), Ok("warehouse".to_string()));
    }

    #[test]
    fn an_empty_segment_is_rejected_and_names_its_position() {
        // `warehouse..orders` addresses nothing, and silently collapsing it
        // would make two different hierarchies produce one FQN.
        assert_eq!(
            derive(&["warehouse", "", "orders"]),
            Err(FqnError::EmptySegment { position: 1 })
        );
    }

    #[test]
    fn a_whitespace_only_segment_is_empty() {
        assert_eq!(
            derive(&["warehouse", "   ", "orders"]),
            Err(FqnError::EmptySegment { position: 1 })
        );
    }

    #[test]
    fn a_segment_containing_the_separator_is_rejected() {
        // The ambiguity that matters: if `a.b` were allowed as one segment,
        // `parent()` and `leaf()` would disagree with how it was constructed,
        // and a connector re-run could address a different entity.
        assert_eq!(
            derive(&["warehouse", "sales.public", "orders"]),
            Err(FqnError::SeparatorInSegment {
                position: 1,
                segment: "sales.public".to_string()
            })
        );
    }

    #[test]
    fn child_of_appends_to_a_parent_that_already_contains_separators() {
        // The bug this function exists to prevent: passing a joined parent FQN
        // to derive() as a segment, which correctly but unhelpfully rejects it.
        assert_eq!(
            child_of("warehouse.sales.public", "orders"),
            Ok("warehouse.sales.public.orders".to_string())
        );
    }

    #[test]
    fn child_of_still_validates_the_new_segment() {
        assert_eq!(
            child_of("warehouse.sales", "public.orders"),
            Err(FqnError::SeparatorInSegment {
                position: 0,
                segment: "public.orders".to_string()
            })
        );
        assert_eq!(
            child_of("warehouse.sales", ""),
            Err(FqnError::EmptySegment { position: 0 })
        );
    }

    #[test]
    fn child_of_and_parent_are_inverses() {
        let child = child_of("warehouse.sales.public", "orders").expect("valid");
        assert_eq!(parent(&child), Some("warehouse.sales.public"));
        assert_eq!(leaf(&child), "orders");
    }

    #[test]
    fn parent_of_a_nested_fqn_drops_the_last_segment() {
        assert_eq!(
            parent("warehouse.sales.public.orders"),
            Some("warehouse.sales.public")
        );
    }

    #[test]
    fn a_root_has_no_parent() {
        assert_eq!(parent("warehouse"), None);
    }

    #[test]
    fn leaf_is_the_entity_own_name() {
        assert_eq!(leaf("warehouse.sales.public.orders"), "orders");
        assert_eq!(leaf("warehouse"), "warehouse");
    }

    #[test]
    fn derive_and_segments_round_trip() {
        let parts = ["warehouse", "sales", "public", "orders"];
        let fqn = derive(&parts).expect("valid");
        assert_eq!(segments(&fqn), parts);
    }

    #[test]
    fn walking_parents_reaches_the_root_in_exactly_depth_steps() {
        let mut current = "warehouse.sales.public.orders";
        let mut steps = 0;
        while let Some(up) = parent(current) {
            current = up;
            steps += 1;
        }
        assert_eq!(current, "warehouse");
        assert_eq!(steps, 3, "four levels means three hops to the root");
    }
}
