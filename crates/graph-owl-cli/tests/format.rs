//! `--format json` on every structured command — the CLI conventions.

use graph_owl_cli::declaration::{API_VERSION, Declaration, Metadata};
use graph_owl_cli::drift::detect;
use graph_owl_cli::format::{Format, drift, errors, plan};
use graph_owl_cli::plan::{LiveEntity, compute};
use graph_owl_cli::validate::{Declarations, ValidationError, validate_directory};
use std::path::PathBuf;
use std::str::FromStr;

fn declared(fqn: &str, kind: &str, description: Option<&str>) -> (String, (PathBuf, Declaration)) {
    let (parent, name) = match fqn.rsplit_once('.') {
        Some((p, n)) => (Some(p.to_string()), n.to_string()),
        None => (None, fqn.to_string()),
    };
    (
        fqn.to_string(),
        (
            PathBuf::from("d.yaml"),
            Declaration {
                api_version: API_VERSION.to_string(),
                kind: kind.to_string(),
                metadata: Metadata {
                    name,
                    parent,
                    description: description.map(ToString::to_string),
                },
            },
        ),
    )
}

fn live(fqn: &str, kind: &str, description: Option<&str>) -> LiveEntity {
    LiveEntity {
        id: format!("id-{fqn}"),
        fully_qualified_name: fqn.to_string(),
        kind: kind.to_string(),
        description: description.map(ToString::to_string),
    }
}

fn declarations(items: Vec<(String, (PathBuf, Declaration))>) -> Declarations {
    Declarations {
        by_fqn: items.into_iter().collect(),
    }
}

#[test]
fn the_format_flag_parses_both_values_and_rejects_anything_else() {
    assert_eq!(Format::from_str("text"), Ok(Format::Text));
    assert_eq!(Format::from_str("json"), Ok(Format::Json));
    assert!(
        Format::from_str("yaml").is_err(),
        "an unknown format must be refused, not silently defaulted — a \
         pipeline asking for a format it will not get should hear so"
    );
}

/// The JSON is **parseable**, which is the whole point, and carries the
/// summary a CI job branches on.
#[test]
fn a_plan_renders_as_valid_parseable_json() {
    let decls = declarations(vec![
        declared("svc", "service", Some("new")),
        declared("fresh", "service", None),
    ]);
    let existing = vec![
        live("svc", "service", Some("old")),
        live("gone", "service", None),
    ];

    let rendered = plan(&compute(&decls, &existing), Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("must be valid JSON");

    assert_eq!(parsed["summary"]["create"], 1);
    assert_eq!(parsed["summary"]["update"], 1);
    assert_eq!(parsed["summary"]["prune"], 1);
    assert_eq!(parsed["summary"]["hasChanges"], true);
    assert_eq!(parsed["entities"].as_array().expect("array").len(), 3);
}

/// **The `change` tag is a contract, not prose.** A pipeline branches on it,
/// so rewording a human message must never move it — this test is what makes
/// that promise enforceable.
#[test]
fn the_change_tag_is_stable_and_independent_of_human_wording() {
    let decls = declarations(vec![
        declared("a", "service", None),
        declared("b", "service", Some("changed")),
        declared("c", "service", Some("same")),
    ]);
    let existing = vec![
        live("b", "service", Some("before")),
        live("c", "service", Some("same")),
        live("d", "service", None),
    ];

    let rendered = plan(&compute(&decls, &existing), Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    let tags: Vec<&str> = parsed["entities"]
        .as_array()
        .expect("array")
        .iter()
        .map(|e| e["change"].as_str().expect("change tag"))
        .collect();
    assert_eq!(tags, vec!["create", "update", "noChange", "prune"]);
}

/// An update carries its per-field before/after in the machine output too —
/// a reviewer bot needs the same detail a human plan shows.
#[test]
fn a_json_update_carries_the_field_diff() {
    let decls = declarations(vec![declared("svc", "service", Some("after"))]);
    let existing = vec![live("svc", "service", Some("before"))];

    let rendered = plan(&compute(&decls, &existing), Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    let fields = &parsed["entities"][0]["fields"];
    assert_eq!(fields[0]["field"], "description");
    assert_eq!(fields[0]["before"], "before");
    assert_eq!(fields[0]["after"], "after");
}

/// Entries with nothing to diff omit `fields` rather than emitting an empty
/// array — a consumer checking `fields` for presence should not have to
/// distinguish "absent" from "empty".
#[test]
fn a_json_entity_with_no_field_diff_omits_the_key() {
    let decls = declarations(vec![declared("svc", "service", None)]);

    let rendered = plan(&compute(&decls, &[]), Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    assert!(parsed["entities"][0].get("fields").is_none(), "{rendered}");
}

/// JSON is deterministic for the same reason the text plan is — it lands in
/// CI logs and gets diffed.
#[test]
fn json_output_is_byte_identical_across_runs() {
    let decls = declarations(vec![
        declared("z", "service", None),
        declared("a", "service", Some("x")),
    ]);
    let existing = vec![live("a", "service", None), live("stale", "service", None)];

    let first = plan(&compute(&decls, &existing), Format::Json).expect("render");
    let second = plan(&compute(&decls, &existing), Format::Json).expect("render");

    assert_eq!(first, second);
}

/// **Drift's central distinction survives into JSON.** If `liveEdited` and
/// `unapplied` collapsed to one tag, the machine output would lose the only
/// thing the command exists to tell you.
#[test]
fn drift_json_preserves_the_live_edited_distinction() {
    let decls = declarations(vec![
        declared("edited", "service", Some("declared")),
        declared("pending", "service", Some("declared")),
    ]);
    let existing = vec![
        live("edited", "service", Some("someone changed it")),
        live("pending", "service", Some("last applied")),
    ];
    let report = detect(&compute(&decls, &existing), &|fqn| match fqn {
        "edited" => Some(false),
        _ => Some(true),
    });

    let rendered = drift(&report, Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    let kinds: Vec<&str> = parsed["drifted"]
        .as_array()
        .expect("array")
        .iter()
        .map(|d| d["kind"].as_str().expect("kind"))
        .collect();
    assert!(kinds.contains(&"liveEdited"), "{rendered}");
    assert!(kinds.contains(&"unapplied"), "{rendered}");
    assert_eq!(parsed["clean"], false);
}

#[test]
fn a_clean_drift_report_says_so_in_both_formats() {
    let decls = declarations(vec![declared("svc", "service", Some("same"))]);
    let existing = vec![live("svc", "service", Some("same"))];
    let report = detect(&compute(&decls, &existing), &|_| Some(true));

    let text = drift(&report, Format::Text).expect("render");
    let json: serde_json::Value =
        serde_json::from_str(&drift(&report, Format::Json).expect("render")).expect("valid");

    assert!(text.contains("no drift"), "{text}");
    assert_eq!(json["clean"], true);
}

/// **Validation errors are data when asked for as JSON** — a CI job that
/// annotates a pull request needs file and line structured, not scraped out
/// of a rendered sentence.
#[test]
fn validation_errors_render_as_structured_json() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/broken");
    let found = validate_directory(&root).expect_err("the fixture is broken on purpose");

    let rendered = errors(&found, Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    assert_eq!(parsed["count"], found.len());
    assert!(
        parsed["errors"][0]["file"]
            .as_str()
            .expect("file")
            .ends_with(".yaml"),
        "{rendered}"
    );
}

/// A line number is omitted rather than emitted as null when the problem is
/// about the file as a whole — same reasoning as the plan's `fields`.
#[test]
fn an_error_without_a_line_omits_the_key() {
    let found = vec![ValidationError {
        file: PathBuf::from("a.yaml"),
        line: None,
        detail: "whole-file problem".to_string(),
    }];

    let rendered = errors(&found, Format::Json).expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

    assert!(parsed["errors"][0].get("line").is_none(), "{rendered}");
}
