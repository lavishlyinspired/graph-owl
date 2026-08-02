//! Export — Epic 20 Slice F.
//!
//! **Lossy by design** (decision 7): declarable state only, never history,
//! versions, or system-derived fields. The round-trip test is the
//! specification — export → apply must produce zero versions — and that only
//! holds if export emits exactly the fields apply can send. Emitting a
//! derived field would either be rejected on apply or churn the entity on
//! every run, which is why the mutator watch names it.

use crate::declaration::{API_VERSION, Declaration, Metadata};
use crate::plan::LiveEntity;

/// Turns live state back into declarations.
///
/// Ordered by FQN so two exports of the same catalog are byte-identical —
/// the same determinism requirement the plan has, for the same reason: these
/// files land in git, and a diff full of reordering noise is a diff nobody
/// reviews.
#[must_use]
pub fn to_declarations(live: &[LiveEntity]) -> Vec<Declaration> {
    let mut sorted: Vec<&LiveEntity> = live.iter().collect();
    sorted.sort_by(|a, b| a.fully_qualified_name.cmp(&b.fully_qualified_name));

    sorted
        .into_iter()
        .map(|entity| {
            let (parent, name) = split_fqn(&entity.fully_qualified_name);
            Declaration {
                api_version: API_VERSION.to_string(),
                kind: entity.kind.clone(),
                metadata: Metadata {
                    name,
                    parent,
                    // The only other declarable field today. Everything else
                    // a `LiveEntity` could carry — version, timestamps,
                    // owners, the change description — is derived or
                    // system-owned and is deliberately not emitted.
                    description: entity.description.clone(),
                },
            }
        })
        .collect()
}

/// `a.b.c` → (`Some("a.b")`, `"c"`); `a` → (`None`, `"a"`).
///
/// Splits on the **last** separator, because a parent FQN may itself contain
/// separators — splitting on the first would make `a.b.c`'s parent `a`, which
/// is a different entity that may well exist.
fn split_fqn(fully_qualified_name: &str) -> (Option<String>, String) {
    match fully_qualified_name.rsplit_once('.') {
        Some((parent, name)) => (Some(parent.to_string()), name.to_string()),
        None => (None, fully_qualified_name.to_string()),
    }
}

/// Renders declarations as a multi-document YAML file.
///
/// # Errors
///
/// Propagates a serialization failure, which in practice cannot happen for
/// these types but is not worth an `unwrap` in a tool that writes files
/// people commit.
pub fn render(declarations: &[Declaration]) -> Result<String, serde_norway::Error> {
    let mut out = String::new();
    for (index, declaration) in declarations.iter().enumerate() {
        if index > 0 {
            out.push_str("---\n");
        }
        out.push_str(&serde_norway::to_string(declaration)?);
    }
    Ok(out)
}
