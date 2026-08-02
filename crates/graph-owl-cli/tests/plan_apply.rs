//! Epic 20 Slices B–G: plan, apply ordering, prune guards, drift, export,
//! and the CI gate.

use graph_owl_cli::apply::{in_dependency_order, may_proceed};
use graph_owl_cli::declaration::{API_VERSION, Declaration, Metadata};
use graph_owl_cli::drift::{DriftKind, detect};
use graph_owl_cli::exit::{CHANGES_PENDING, ERROR, FailOn, NO_CHANGES, code_for, redact};
use graph_owl_cli::export::{render, to_declarations};
use graph_owl_cli::plan::{Change, LiveEntity, compute};
use graph_owl_cli::prune::{DEFAULT_PRUNE_THRESHOLD, Refusal, Scope, authorize};
use graph_owl_cli::validate::Declarations;
use std::path::PathBuf;

fn declared(fqn: &str, kind: &str, description: Option<&str>) -> (String, (PathBuf, Declaration)) {
    let (parent, name) = match fqn.rsplit_once('.') {
        Some((p, n)) => (Some(p.to_string()), n.to_string()),
        None => (None, fqn.to_string()),
    };
    (
        fqn.to_string(),
        (
            PathBuf::from("decl.yaml"),
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

// ── Slice B: the plan ──────────────────────────────────────────────────

#[test]
fn a_plan_against_an_empty_catalog_is_all_creates() {
    let decls = declarations(vec![
        declared("svc", "service", None),
        declared("svc.db", "database", None),
    ]);

    let plan = compute(&decls, &[]);

    assert_eq!(plan.counts().create, 2);
    assert_eq!(plan.counts().update, 0);
    assert_eq!(plan.counts().prune, 0);
    assert!(plan.has_changes());
}

#[test]
fn a_plan_with_unchanged_declarations_is_all_no_change() {
    let decls = declarations(vec![declared("svc", "service", Some("the warehouse"))]);
    let existing = vec![live("svc", "service", Some("the warehouse"))];

    let plan = compute(&decls, &existing);

    assert_eq!(plan.counts().no_change, 1);
    assert!(
        !plan.has_changes(),
        "an unchanged plan must report no pending changes, or CI gates on nothing"
    );
}

#[test]
fn an_update_shows_the_field_before_and_after() {
    let decls = declarations(vec![declared("svc", "service", Some("new text"))]);
    let existing = vec![live("svc", "service", Some("old text"))];

    let plan = compute(&decls, &existing);

    match &plan.entities[0].change {
        Change::Update { fields } => {
            assert_eq!(fields[0].field, "description");
            assert_eq!(fields[0].before.as_deref(), Some("old text"));
            assert_eq!(fields[0].after.as_deref(), Some("new text"));
        }
        other => panic!("expected an update, got {other:?}"),
    }
}

/// **Slice B's stated mutator watch**: non-deterministic ordering is "the
/// likely real bug", because a plan that reorders between runs is unusable in
/// CI — every diff looks like a change.
#[test]
fn two_plans_over_the_same_inputs_are_byte_identical() {
    let decls = declarations(vec![
        declared("z", "service", None),
        declared("a", "service", None),
        declared("m", "service", None),
        declared("a.child", "database", Some("x")),
    ]);
    let existing = vec![live("m", "service", None), live("stale", "service", None)];

    let first = graph_owl_cli::plan::render(&compute(&decls, &existing));
    let second = graph_owl_cli::plan::render(&compute(&decls, &existing));

    assert_eq!(first, second, "a plan must be reproducible byte for byte");
}

/// **Decision 4, and the failure mode it names.** A description that exists
/// live but is *not declared* must not appear as a change — treating absent
/// as null is what would silently reset every hand-curated field.
#[test]
fn an_undeclared_field_is_not_planned_as_a_change() {
    let decls = declarations(vec![declared("svc", "service", None)]);
    let existing = vec![live("svc", "service", Some("someone wrote this by hand"))];

    let plan = compute(&decls, &existing);

    assert_eq!(
        plan.entities[0].change,
        Change::NoChange,
        "an undeclared description must be left alone, not reset"
    );
}

#[test]
fn a_live_entity_that_is_not_declared_is_planned_as_a_prune() {
    let decls = declarations(vec![declared("svc", "service", None)]);
    let existing = vec![live("svc", "service", None), live("gone", "service", None)];

    let plan = compute(&decls, &existing);

    assert_eq!(plan.counts().prune, 1);
}

// ── Slice C: apply ─────────────────────────────────────────────────────

/// Parents before children — an FQN is derived from its parent chain, so a
/// child applied first has nothing to resolve against.
#[test]
fn apply_order_puts_every_parent_before_its_children() {
    let decls = declarations(vec![
        declared("svc.db.schema", "schema", None),
        declared("svc", "service", None),
        declared("svc.db", "database", None),
    ]);

    let plan = compute(&decls, &[]);
    let ordered: Vec<&str> = in_dependency_order(&plan)
        .iter()
        .map(|e| e.fully_qualified_name.as_str())
        .collect();

    assert_eq!(ordered, vec!["svc", "svc.db", "svc.db.schema"]);
}

/// No-change entities are not sent at all — sending an unchanged update
/// would produce a version and a change event, which is precisely what the
/// "second apply is a no-op" criterion forbids.
#[test]
fn apply_order_omits_entities_with_nothing_to_do() {
    let decls = declarations(vec![declared("svc", "service", Some("same"))]);
    let existing = vec![live("svc", "service", Some("same"))];

    let plan = compute(&decls, &existing);

    assert!(
        in_dependency_order(&plan).is_empty(),
        "an unchanged entity must not be sent"
    );
}

/// Without `--yes` and without a TTY, apply refuses rather than assuming
/// consent — a pipeline that forgot the flag must fail loudly, not mutate.
#[test]
fn apply_refuses_without_a_tty_and_without_yes() {
    assert!(!may_proceed(false, false), "no flag, no human: refuse");
    assert!(may_proceed(true, false), "--yes is explicit consent");
    assert!(may_proceed(false, true), "a human can be asked");
}

// ── Slice D: pruning is scoped and guarded ─────────────────────────────

/// **The critical scope test** the plan names: declare only `service_a`, and
/// `service_b` must be untouched.
#[test]
fn a_prune_never_reaches_outside_the_declared_scope() {
    let decls = declarations(vec![declared("service_a", "service", None)]);
    let existing = vec![
        live("service_a", "service", None),
        live("service_a.orphan", "database", None),
        live("service_b", "service", None),
        live("service_b.child", "database", None),
    ];
    let plan = compute(&decls, &existing);
    let scope = Scope {
        prefixes: vec!["service_a".to_string()],
    };

    let to_prune = authorize(&plan, &scope, DEFAULT_PRUNE_THRESHOLD).expect("within threshold");

    assert_eq!(
        to_prune,
        vec!["service_a.orphan"],
        "only the in-scope orphan; service_b is not this directory's business"
    );
}

#[test]
fn a_prune_over_the_threshold_is_refused_and_deletes_nothing() {
    let decls = declarations(vec![declared("svc", "service", None)]);
    let mut existing = vec![live("svc", "service", None)];
    for i in 0..20 {
        existing.push(live(&format!("svc.child{i}"), "database", None));
    }
    let plan = compute(&decls, &existing);
    let scope = Scope {
        prefixes: vec!["svc".to_string()],
    };

    let refusal = authorize(&plan, &scope, DEFAULT_PRUNE_THRESHOLD).expect_err("over threshold");

    assert!(matches!(
        refusal,
        Refusal::OverThreshold {
            would_prune: 20,
            ..
        }
    ));
}

/// An undeclared scope is refused, never treated as "the whole catalog" —
/// "I forgot to say" and "I mean everything" must not be the same
/// instruction.
#[test]
fn a_prune_with_no_declared_scope_is_refused() {
    let decls = declarations(vec![declared("svc", "service", None)]);
    let existing = vec![live("other", "service", None)];
    let plan = compute(&decls, &existing);

    let refusal = authorize(&plan, &Scope { prefixes: vec![] }, 100).expect_err("no scope");

    assert_eq!(refusal, Refusal::NoScope);
}

/// A scope prefix must match on a segment boundary — `service_a` must not
/// claim authority over `service_ab`.
#[test]
fn a_scope_prefix_does_not_match_a_longer_sibling_name() {
    let scope = Scope {
        prefixes: vec!["service_a".to_string()],
    };
    assert!(scope.covers("service_a"));
    assert!(scope.covers("service_a.child"));
    assert!(
        !scope.covers("service_ab"),
        "a prefix must match on a separator, not on characters"
    );
}

// ── Slice E: drift is visible, never corrected ─────────────────────────

/// **Decision 3's distinction**, which a plain diff cannot draw: someone
/// editing live state is a different event from a file that was never
/// applied, and they want opposite responses.
#[test]
fn drift_distinguishes_a_live_edit_from_an_unapplied_file() {
    let decls = declarations(vec![
        declared("edited", "service", Some("declared")),
        declared("pending", "service", Some("declared")),
    ]);
    let existing = vec![
        live("edited", "service", Some("someone changed this")),
        live("pending", "service", Some("last applied")),
    ];
    let plan = compute(&decls, &existing);

    let report = detect(&plan, &|fqn| match fqn {
        // Live no longer matches what we last wrote: a human moved it.
        "edited" => Some(false),
        // Live still matches what we wrote; the file moved instead.
        "pending" => Some(true),
        _ => None,
    });

    let kinds: Vec<(&str, DriftKind)> = report
        .drifted
        .iter()
        .map(|d| (d.fully_qualified_name.as_str(), d.kind))
        .collect();
    assert!(
        kinds.contains(&("edited", DriftKind::LiveEdited)),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("pending", DriftKind::Unapplied)),
        "{kinds:?}"
    );
}

/// An unchanged catalog drifts not at all — and the report type carries no
/// method that could mutate, which is a stronger guarantee than a comment.
#[test]
fn drift_over_matching_state_is_clean() {
    let decls = declarations(vec![declared("svc", "service", Some("same"))]);
    let existing = vec![live("svc", "service", Some("same"))];

    let report = detect(&compute(&decls, &existing), &|_| Some(true));

    assert!(report.is_clean());
}

/// Without knowledge of what was last applied, the report takes the
/// conservative reading rather than accusing someone of editing live state.
#[test]
fn drift_with_no_applied_record_never_reports_a_live_edit() {
    let decls = declarations(vec![declared("svc", "service", Some("new"))]);
    let existing = vec![live("svc", "service", Some("old"))];

    let report = detect(&compute(&decls, &existing), &|_| None);

    assert_eq!(report.drifted[0].kind, DriftKind::Unapplied);
}

// ── Slice F: export round-trips ────────────────────────────────────────

/// **The round-trip test is the specification.** Export must emit exactly
/// what apply can send: re-planning the exported declarations against the
/// same live state must show no changes at all.
#[test]
fn exported_declarations_re_plan_as_a_no_op() {
    let existing = vec![
        live("svc", "service", Some("a warehouse")),
        live("svc.db", "database", None),
        live("svc.db.public", "schema", Some("the public schema")),
    ];

    let exported = to_declarations(&existing);
    let round_tripped = declarations(
        exported
            .into_iter()
            .map(|declaration| {
                (
                    declaration.fully_qualified_name(),
                    (PathBuf::from("exported.yaml"), declaration),
                )
            })
            .collect(),
    );

    let plan = compute(&round_tripped, &existing);

    assert!(
        !plan.has_changes(),
        "export -> apply must be a no-op, got:\n{}",
        graph_owl_cli::plan::render(&plan)
    );
}

#[test]
fn two_exports_of_the_same_catalog_are_byte_identical() {
    let existing = vec![
        live("z", "service", None),
        live("a", "service", Some("first")),
        live("a.b", "database", None),
    ];

    let first = render(&to_declarations(&existing)).expect("render");
    let second = render(&to_declarations(&existing)).expect("render");

    assert_eq!(first, second);
    assert!(
        first.find("name: a").unwrap() < first.find("name: z").unwrap(),
        "ordered by FQN, so a git diff shows real changes only:\n{first}"
    );
}

/// A parent FQN may itself contain separators — splitting on the first would
/// make `a.b.c`'s parent `a`, a different entity that may well exist.
#[test]
fn export_splits_a_nested_fqn_at_the_last_separator() {
    let exported = to_declarations(&[live("a.b.c", "schema", None)]);

    assert_eq!(exported[0].metadata.name, "c");
    assert_eq!(exported[0].metadata.parent.as_deref(), Some("a.b"));
}

// ── Slice G: CI gates on the plan ──────────────────────────────────────

#[test]
fn exit_codes_distinguish_no_changes_from_pending_changes() {
    let unchanged = compute(
        &declarations(vec![declared("svc", "service", Some("same"))]),
        &[live("svc", "service", Some("same"))],
    );
    let pending = compute(&declarations(vec![declared("svc", "service", None)]), &[]);

    assert_eq!(code_for(&unchanged, FailOn::Nothing), NO_CHANGES);
    assert_eq!(
        code_for(&pending, FailOn::Nothing),
        CHANGES_PENDING,
        "pending work is not an error — conflating them makes every real diff a broken build"
    );
}

/// **The gate that matters**: a pull request whose plan would delete assets
/// fails. An exit code of 0 with deletions present is the mutator watch.
#[test]
fn ci_fails_a_plan_that_would_delete() {
    let plan = compute(
        &declarations(vec![declared("svc", "service", None)]),
        &[
            live("svc", "service", None),
            live("doomed", "service", None),
        ],
    );

    assert_eq!(code_for(&plan, FailOn::Deletions), ERROR);
    assert_ne!(
        code_for(&plan, FailOn::Deletions),
        NO_CHANGES,
        "a deleting plan must never exit 0 under a deletions gate"
    );
}

#[test]
fn a_plan_with_only_creates_passes_a_deletions_gate() {
    let plan = compute(&declarations(vec![declared("svc", "service", None)]), &[]);

    assert_eq!(
        code_for(&plan, FailOn::Deletions),
        CHANGES_PENDING,
        "adding is routine; only deletion needs a human"
    );
}

#[test]
fn plan_output_never_carries_a_credential() {
    let text = "+ create svc\nAuthorization: Bearer abc123secret\ntoken=xyz\n+ create db\n";

    let safe = redact(text);

    assert!(!safe.contains("abc123secret"), "{safe}");
    assert!(!safe.contains("xyz"), "{safe}");
    assert!(safe.contains("+ create svc"), "non-secrets survive: {safe}");
}
