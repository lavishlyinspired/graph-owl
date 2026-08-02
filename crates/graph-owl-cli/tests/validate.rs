//! Epic 20 Slice A: declarations parse and validate, locally.

use graph_owl_cli::validate::validate_directory;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A directory is walked **recursively** and one file may declare several
/// entities — both are criteria, and the fixture exercises them together:
/// two documents in one file at the root, a third in a subdirectory.
#[test]
fn a_valid_directory_yields_every_declaration() {
    let declarations = validate_directory(&fixture("valid")).expect("should validate");

    let fqns: Vec<&str> = declarations.by_fqn.keys().map(String::as_str).collect();
    assert_eq!(
        fqns,
        vec![
            "snowflake_prod",
            "snowflake_prod.analytics",
            "snowflake_prod.analytics.public",
        ],
        "every declaration, addressed by derived FQN"
    );
}

/// **The mutator watch, and the reason this function returns a `Vec`.**
/// First-error-only reporting must fail this: the fixture holds five
/// independent problems of five different kinds across **three** files, so
/// passing requires accumulating both within a file and across the walk. A
/// validator that stops at the first turns fixing a directory into five
/// round trips.
#[test]
fn every_error_is_reported_in_one_run_not_just_the_first() {
    let errors = validate_directory(&fixture("broken")).expect_err("should not validate");

    let rendered: Vec<String> = errors.iter().map(ToString::to_string).collect();
    let joined = rendered.join("\n");

    assert!(
        errors.len() >= 5,
        "expected at least the five seeded problems, got {}:\n{joined}",
        errors.len()
    );

    // Unknown kind, named.
    assert!(
        joined.contains("`warehouse` is not a known kind"),
        "{joined}"
    );
    // Missing required field — serde's own message names the field.
    assert!(joined.contains("name"), "{joined}");
    // Duplicate FQN naming **both** files, not just the second.
    assert!(
        joined.contains("`duplicated` is already declared in") && joined.contains("b.yaml"),
        "{joined}"
    );
    // Dangling parent.
    assert!(joined.contains("nobody.declared.this"), "{joined}");
    // An unrecognised apiVersion, in a third file — proving the walk keeps
    // accumulating after a file that already produced errors.
    assert!(joined.contains("unknown apiVersion"), "{joined}");
}

/// Every error carries the file it came from — an error list that does not
/// say *where* is a list somebody has to grep for.
#[test]
fn every_error_names_its_file() {
    let errors = validate_directory(&fixture("broken")).expect_err("should not validate");

    for error in &errors {
        assert!(
            error.file.to_string_lossy().ends_with(".yaml"),
            "an error with no file: {error:?}"
        );
    }
}

/// A parse failure carries the **line**, which is the whole reason this
/// crate adopts a YAML parser that reports source locations rather than one
/// that only reports "invalid".
#[test]
fn a_malformed_document_reports_a_line_number() {
    let directory = tempdir();
    std::fs::write(
        directory.path().join("bad.yaml"),
        "apiVersion: graph-owl.dev/v1\nkind: service\nmetadata:\n  name: [unclosed\n",
    )
    .expect("write");

    let errors = validate_directory(directory.path()).expect_err("should not validate");
    assert!(
        errors.iter().any(|e| e.line.is_some()),
        "no error carried a line number: {errors:?}"
    );
}

/// Containment is checked here, not left for the server to reject after a
/// plan has already been shown and approved.
#[test]
fn a_kind_that_requires_a_parent_is_refused_without_one() {
    let directory = tempdir();
    std::fs::write(
        directory.path().join("t.yaml"),
        "apiVersion: graph-owl.dev/v1\nkind: table\nmetadata:\n  name: orders\n",
    )
    .expect("write");

    let errors = validate_directory(directory.path()).expect_err("should not validate");
    assert!(
        errors.iter().any(|e| e.detail.contains("needs a parent")),
        "{errors:?}"
    );
}

/// And the converse, which a one-sided check would miss: a root entity that
/// declares a parent is equally wrong.
#[test]
fn a_root_kind_is_refused_with_a_parent() {
    let directory = tempdir();
    std::fs::write(
        directory.path().join("s.yaml"),
        "apiVersion: graph-owl.dev/v1\nkind: service\nmetadata:\n  name: svc\n  parent: something\n",
    )
    .expect("write");

    let errors = validate_directory(directory.path()).expect_err("should not validate");
    assert!(
        errors
            .iter()
            .any(|e| e.detail.contains("cannot have a parent")),
        "{errors:?}"
    );
}

/// An unrecognised `apiVersion` is refused rather than parsed hopefully —
/// the day the format changes, files written against the old shape must be
/// recognisable as such.
#[test]
fn an_unknown_api_version_is_refused() {
    let directory = tempdir();
    std::fs::write(
        directory.path().join("s.yaml"),
        "apiVersion: graph-owl.dev/v99\nkind: service\nmetadata:\n  name: svc\n",
    )
    .expect("write");

    let errors = validate_directory(directory.path()).expect_err("should not validate");
    assert!(
        errors
            .iter()
            .any(|e| e.detail.contains("unknown apiVersion")),
        "{errors:?}"
    );
}

/// A typo'd key is an error, not a silently ignored field — otherwise the
/// entity is created without the value its author believed they set, which
/// no amount of plan review would catch.
#[test]
fn an_unknown_field_is_refused_rather_than_ignored() {
    let directory = tempdir();
    std::fs::write(
        directory.path().join("s.yaml"),
        "apiVersion: graph-owl.dev/v1\nkind: service\nmetadata:\n  name: svc\n  descriptoin: typo\n",
    )
    .expect("write");

    let errors = validate_directory(directory.path()).expect_err("should not validate");
    assert!(
        errors.iter().any(|e| e.detail.contains("descriptoin")),
        "the typo'd key must be named: {errors:?}"
    );
}

/// Validation is **purely local** — this runs with no server, no
/// `DATABASE_URL`, and no token, which is what makes it usable as the first
/// step of a pull-request check.
#[test]
fn validation_needs_no_catalog_connection() {
    // The assertion is the absence of setup: every other test in this file
    // would fail to compile or run if a connection were required, and this
    // one states the property so a future change that adds one is a visible
    // decision rather than a silent regression.
    let declarations = validate_directory(&fixture("valid")).expect("should validate");
    assert!(!declarations.by_fqn.is_empty());
}

/// A minimal stand-in for `tempfile`, which this crate does not otherwise
/// need — one dependency avoided for four lines.
struct TempDir(PathBuf);
impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tempdir() -> TempDir {
    let path = std::env::temp_dir().join(format!(
        "graph-owl-cli-test-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    TempDir(path)
}
