//! Local validation of a declaration directory — Epic 20 Slice A.
//!
//! **Every error, never the first.** A validator that stops at the first
//! problem turns fixing a directory into one round trip per mistake, which is
//! the same reasoning `graph_owl_api::validation` already applies to a request
//! body. Slice A's mutator watch names it explicitly: first-error-only
//! reporting must fail the multi-error fixture.
//!
//! **Purely local.** No catalog connection, so this runs in CI on a pull
//! request before any credential exists. The one check that genuinely needs
//! the catalog — whether a parent that is *not* declared here exists live —
//! is expressed as a separate, opt-in step rather than silently skipped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use graph_owl_core::AssetKind;
use serde::Deserialize as _;

use crate::declaration::{API_VERSION, Declaration};

/// One problem, addressed well enough to fix without opening a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub file: PathBuf,
    /// `None` when the problem is about the file as a whole (unreadable, or a
    /// duplicate whose location is carried by the *other* file's entry).
    pub line: Option<usize>,
    pub detail: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{}: {}", self.file.display(), line, self.detail),
            None => write!(f, "{}: {}", self.file.display(), self.detail),
        }
    }
}

/// What a directory declared, once it is known to be well-formed.
#[derive(Debug, Default)]
pub struct Declarations {
    /// Keyed by FQN, so duplicate detection and parent resolution are both
    /// lookups rather than scans. `BTreeMap` rather than `HashMap`: Slice B
    /// needs a deterministic plan, and the cheapest way to guarantee that is
    /// to never introduce non-determinism in the first place.
    pub by_fqn: BTreeMap<String, (PathBuf, Declaration)>,
}

/// Walks `root`, parses every YAML file, and reports **all** problems.
///
/// # Errors
///
/// Returns every [`ValidationError`] found, in a stable order (by file, then
/// by line) so two runs over the same tree produce byte-identical output —
/// the same determinism requirement Slice B has for a plan.
pub fn validate_directory(root: &Path) -> Result<Declarations, Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut declarations = Declarations::default();
    // `(fqn, file)` of everything seen, so a duplicate can name *both* files
    // rather than just the second one — "already declared" without saying
    // where is a message that sends someone grepping.
    let mut first_seen: BTreeMap<String, PathBuf> = BTreeMap::new();

    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("yaml" | "yml")
            )
        })
        .collect();
    // Sorted, so the error list and the resulting plan do not depend on the
    // order the filesystem happened to hand back.
    files.sort();

    for file in files {
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) => {
                errors.push(ValidationError {
                    file: file.clone(),
                    line: None,
                    detail: format!("could not be read: {error}"),
                });
                continue;
            }
        };

        // One file may declare several entities, so every document in it is
        // parsed — and a bad document does not stop the ones after it, which
        // is the same accumulate-don't-abort rule applied within a file.
        for document in serde_norway::Deserializer::from_str(&text) {
            match Declaration::deserialize(document) {
                Ok(declaration) => {
                    check_declaration(&file, &declaration, &mut errors);
                    let fqn = declaration.fully_qualified_name();
                    if let Some(original) = first_seen.get(&fqn) {
                        errors.push(ValidationError {
                            file: file.clone(),
                            line: None,
                            detail: format!(
                                "`{fqn}` is already declared in {}",
                                original.display()
                            ),
                        });
                    } else {
                        first_seen.insert(fqn.clone(), file.clone());
                        declarations.by_fqn.insert(fqn, (file.clone(), declaration));
                    }
                }
                Err(error) => {
                    errors.push(ValidationError {
                        file: file.clone(),
                        line: error.location().map(|l| l.line()),
                        detail: error.to_string(),
                    });
                    // **Stop reading this file, and this is load-bearing.**
                    // The multi-document iterator does not advance past a
                    // document it could not parse, so continuing would push
                    // the same error forever — a malformed file hung the
                    // process for 185 seconds until the runner killed it,
                    // and in a user's hands would hang the CLI and exhaust
                    // memory. Beyond that it is also the right semantic: once
                    // a parse fails, the parser's position in the file is not
                    // trustworthy, so any further "documents" read out of it
                    // are fiction. Accumulation across files is unaffected,
                    // and within a file it still accumulates for documents
                    // that parse but are semantically wrong.
                    break;
                }
            }
        }
    }

    check_parents(&declarations, &mut errors);

    if errors.is_empty() {
        Ok(declarations)
    } else {
        Err(errors)
    }
}

fn check_declaration(file: &Path, declaration: &Declaration, errors: &mut Vec<ValidationError>) {
    if declaration.api_version != API_VERSION {
        errors.push(ValidationError {
            file: file.to_path_buf(),
            line: None,
            detail: format!(
                "unknown apiVersion `{}`; this release understands `{API_VERSION}`",
                declaration.api_version
            ),
        });
    }

    // The kind vocabulary is the catalog's, not a second list maintained here
    // — a kind this binary accepted but the server rejected would be a plan
    // that cannot be applied.
    let Ok(kind) = AssetKind::parse(&declaration.kind) else {
        errors.push(ValidationError {
            file: file.to_path_buf(),
            line: None,
            detail: format!("`{}` is not a known kind", declaration.kind),
        });
        return;
    };

    // Containment is the catalog's own rule (`AssetKind::parent_kind`), asked
    // rather than restated: a `table` with no parent is as wrong in a file as
    // it is over HTTP, and catching it here is the whole point of validating
    // before apply.
    match (kind.parent_kind(), &declaration.metadata.parent) {
        (Some(expected), None) => errors.push(ValidationError {
            file: file.to_path_buf(),
            line: None,
            detail: format!(
                "a `{}` needs a parent (a `{}`), but none is declared",
                kind.as_str(),
                expected.as_str()
            ),
        }),
        (None, Some(parent)) => errors.push(ValidationError {
            file: file.to_path_buf(),
            line: None,
            detail: format!(
                "a `{}` is a root entity and cannot have a parent, but `{parent}` is declared",
                kind.as_str()
            ),
        }),
        _ => {}
    }

    if declaration.metadata.name.trim().is_empty() {
        errors.push(ValidationError {
            file: file.to_path_buf(),
            line: None,
            detail: "metadata.name is empty".to_string(),
        });
    }
}

/// A parent named by a declaration must be declared *somewhere in this tree*.
///
/// Deliberately strict while validation is local: a parent that exists only
/// in the live catalog cannot be confirmed without connecting to it, and
/// Slice A's criteria require validation to work with no catalog at all.
/// Slice B, which does have a connection, is where "declared here **or**
/// already live" becomes the rule.
fn check_parents(declarations: &Declarations, errors: &mut Vec<ValidationError>) {
    for (fqn, (file, declaration)) in &declarations.by_fqn {
        let Some(parent) = &declaration.metadata.parent else {
            continue;
        };
        if !declarations.by_fqn.contains_key(parent) {
            errors.push(ValidationError {
                file: file.clone(),
                line: None,
                detail: format!(
                    "`{fqn}` names parent `{parent}`, which is not declared in this directory"
                ),
            });
        }
    }
}
