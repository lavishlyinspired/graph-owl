//! A comment-line regression check for every registered pack query, not
//! server behavior — no `mod common`, no container, no HTTP.
//!
//! **Found three times while writing Plan 107** (`plans/107-filing-
//! period.md`): `Catalog::run_pack_query`'s `substitute_pack_query_bindings`
//! calls `placeholder_names`, which scans a registered query's *whole*
//! text for `{{...}}` — comments included, not only the SPARQL body. A
//! doc comment that names a placeholder in literal double-brace form,
//! even descriptively (`period-summary.sparql` naming `provision-in-
//! force.sparql`'s own `{{invoice}}`, `period-diff.sparql` doing the
//! same, `period-list.sparql` writing `{{...}}` to describe having no
//! placeholder at all), silently adds a phantom required binding no
//! caller can satisfy — the query returns a `400` instead of ever
//! running. Each instance was caught only by running the query live
//! against the real server; this test catches it mechanically instead,
//! for every `.sparql` file this pack registers, not just the ones a
//! human happened to re-read.

use std::path::{Path, PathBuf};

fn packs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

/// Every `.sparql` file under `packs/*/queries/`, found by walking the
/// directory tree directly — `walkdir` is already a dependency of
/// `graph-owl-cli`, but a plain recursive scan needs no new dependency
/// for a two-level-deep, small directory tree.
fn all_query_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let packs = packs_dir();
    let Ok(pack_entries) = std::fs::read_dir(&packs) else {
        return files;
    };
    for pack_entry in pack_entries.flatten() {
        let queries_dir = pack_entry.path().join("queries");
        let Ok(query_entries) = std::fs::read_dir(&queries_dir) else {
            continue;
        };
        for query_entry in query_entries.flatten() {
            let path = query_entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("sparql") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Mirrors `graph-owl-api`'s own `placeholder_names` scan closely enough
/// to answer the one question this test needs, without depending on
/// that function being `pub` (a full reimplementation would risk
/// drifting from the real one; this only needs to find `{{name}}`
/// occurrences on one line, not reproduce exact substitution semantics).
fn placeholder_names_in(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("{{") {
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            break;
        };
        names.push(after_open[..end].to_string());
        rest = &after_open[end + 2..];
    }
    names
}

/// Naming a query's *own* real placeholder in a comment is harmless —
/// `placeholder_names` (the real function) dedupes by name, so a comment
/// mentioning `{{invoice}}` where the body already requires `{{invoice}}`
/// adds nothing new. The actual bug is a comment naming a placeholder the
/// body never declares — a **phantom** requirement no caller can satisfy.
#[test]
fn no_sparql_comment_names_a_placeholder_the_body_does_not_declare() {
    let files = all_query_files();
    assert!(
        !files.is_empty(),
        "expected to find at least one .sparql file under {}",
        packs_dir().display()
    );

    let mut offenders = Vec::new();
    for file in &files {
        let text =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));

        let mut body_names = std::collections::BTreeSet::new();
        let mut comment_names: Vec<(usize, String)> = Vec::new();
        for (line_number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                for name in placeholder_names_in(trimmed) {
                    comment_names.push((line_number + 1, name));
                }
            } else {
                body_names.extend(placeholder_names_in(line));
            }
        }

        for (line_number, name) in comment_names {
            if !body_names.contains(&name) {
                offenders.push(format!(
                    "{}:{}: comment names `{{{{{name}}}}}`, which the query body never declares",
                    relative(file),
                    line_number
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a comment names a placeholder the query body does not declare, \
         which silently demands that binding from every caller and the \
         query can never run — reword the comment to describe it without \
         writing the double-brace form literally:\n{}",
        offenders.join("\n")
    );
}

fn relative(path: &Path) -> String {
    path.strip_prefix(packs_dir())
        .unwrap_or(path)
        .display()
        .to_string()
}
