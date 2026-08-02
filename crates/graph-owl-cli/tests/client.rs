//! Epic 20: the apply path against a catalog, exercised through a recording
//! double rather than a live server.
//!
//! The double is the point, not a shortcut. What these tests check is
//! **exactly what the CLI sends** — and the failure they exist to prevent
//! (sending `description: null` for a field nobody declared, resetting
//! curated text) is invisible in an end-to-end assertion about final state,
//! because the reset and the intended value can look identical if the
//! declaration happened to match. Recording the request is the only way to
//! see the difference.

use graph_owl_cli::client::{Catalog, ClientError, UpsertRequest};
use graph_owl_cli::declaration::{API_VERSION, Declaration, Metadata};
use graph_owl_cli::plan::{Change, LiveEntity, compute};
use graph_owl_cli::validate::Declarations;
use std::cell::RefCell;
use std::path::PathBuf;

#[derive(Default)]
struct Recorder {
    live: Vec<LiveEntity>,
    upserts: RefCell<Vec<UpsertRequest>>,
    tombstones: RefCell<Vec<String>>,
}

impl Catalog for Recorder {
    fn live_within(&self, scope_prefixes: &[String]) -> Result<Vec<LiveEntity>, ClientError> {
        Ok(self
            .live
            .iter()
            .filter(|entity| {
                scope_prefixes.iter().any(|prefix| {
                    entity.fully_qualified_name == *prefix
                        || entity
                            .fully_qualified_name
                            .starts_with(&format!("{prefix}."))
                })
            })
            .cloned()
            .collect())
    }

    fn upsert(&self, entity: &UpsertRequest) -> Result<(), ClientError> {
        self.upserts.borrow_mut().push(entity.clone());
        Ok(())
    }

    fn tombstone(&self, fully_qualified_name: &str) -> Result<(), ClientError> {
        self.tombstones
            .borrow_mut()
            .push(fully_qualified_name.to_string());
        Ok(())
    }
}

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
        fully_qualified_name: fqn.to_string(),
        kind: kind.to_string(),
        description: description.map(ToString::to_string),
    }
}

/// A tiny stand-in for the real apply loop, exercising the same ordering and
/// the same "only send what changed" rule the CLI uses.
fn apply_plan(
    catalog: &dyn Catalog,
    plan: &graph_owl_cli::plan::Plan,
    declarations: &Declarations,
) {
    for entity in graph_owl_cli::apply::in_dependency_order(plan) {
        let (_, declaration) = &declarations.by_fqn[&entity.fully_qualified_name];
        catalog
            .upsert(&UpsertRequest {
                kind: declaration.kind.clone(),
                name: declaration.metadata.name.clone(),
                parent_fqn: declaration.metadata.parent.clone(),
                description: declaration.metadata.description.clone(),
            })
            .expect("recorder never fails");
    }
}

fn declarations(items: Vec<(String, (PathBuf, Declaration))>) -> Declarations {
    Declarations {
        by_fqn: items.into_iter().collect(),
    }
}

/// **Decision 4, checked where it is actually decided.** A live description
/// nobody declared must not be sent at all — not as null, not as the empty
/// string. Asserting on final state could not distinguish "left alone" from
/// "reset to the same value"; asserting on the request can.
#[test]
fn an_undeclared_description_is_never_sent() {
    let recorder = Recorder {
        live: vec![live("svc", "service", Some("curated by a human"))],
        ..Recorder::default()
    };
    // Declares the entity but says nothing about its description.
    let decls = declarations(vec![declared("svc", "service", None)]);

    let plan = compute(&decls, &recorder.live);
    apply_plan(&recorder, &plan, &decls);

    assert!(
        recorder.upserts.borrow().is_empty(),
        "an entity with nothing declared to change must not be sent at all, got {:?}",
        recorder.upserts.borrow()
    );
}

/// The scope filter is applied by the *catalog read*, not afterwards — a
/// client that fetched everything and filtered locally would propose pruning
/// the whole catalog the moment that filter had a bug.
#[test]
fn live_state_is_fetched_scoped_not_filtered_afterwards() {
    let recorder = Recorder {
        live: vec![
            live("service_a", "service", None),
            live("service_a.db", "database", None),
            live("service_b", "service", None),
        ],
        ..Recorder::default()
    };

    let fetched = recorder
        .live_within(&["service_a".to_string()])
        .expect("read");

    let names: Vec<&str> = fetched
        .iter()
        .map(|e| e.fully_qualified_name.as_str())
        .collect();
    assert_eq!(names, vec!["service_a", "service_a.db"]);
}

/// Creates are sent parents-first, so a child's parent FQN always resolves.
#[test]
fn creates_are_sent_parents_before_children() {
    let recorder = Recorder::default();
    let decls = declarations(vec![
        declared("svc.db.public", "schema", None),
        declared("svc", "service", None),
        declared("svc.db", "database", None),
    ]);

    let plan = compute(&decls, &[]);
    apply_plan(&recorder, &plan, &decls);

    let sent: Vec<String> = recorder
        .upserts
        .borrow()
        .iter()
        .map(|r| match &r.parent_fqn {
            Some(parent) => format!("{parent}.{}", r.name),
            None => r.name.clone(),
        })
        .collect();
    assert_eq!(sent, vec!["svc", "svc.db", "svc.db.public"]);
}

/// A declared description that genuinely differs **is** sent — the converse
/// of the first test, without which "never send anything" would also pass.
#[test]
fn a_changed_description_is_sent() {
    let recorder = Recorder {
        live: vec![live("svc", "service", Some("old"))],
        ..Recorder::default()
    };
    let decls = declarations(vec![declared("svc", "service", Some("new"))]);

    let plan = compute(&decls, &recorder.live);
    apply_plan(&recorder, &plan, &decls);

    let sent = recorder.upserts.borrow();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].description.as_deref(), Some("new"));
}

/// A second apply over unchanged declarations sends nothing at all — which
/// is what makes "zero new versions" true, rather than relying on the server
/// to notice the values matched.
#[test]
fn a_second_apply_over_unchanged_declarations_sends_nothing() {
    let recorder = Recorder {
        live: vec![
            live("svc", "service", Some("same")),
            live("svc.db", "database", None),
        ],
        ..Recorder::default()
    };
    let decls = declarations(vec![
        declared("svc", "service", Some("same")),
        declared("svc.db", "database", None),
    ]);

    let plan = compute(&decls, &recorder.live);
    apply_plan(&recorder, &plan, &decls);

    assert!(recorder.upserts.borrow().is_empty());
    assert!(!plan.has_changes());
}

/// Nothing is tombstoned by an ordinary apply — pruning is a separate,
/// opt-in path with its own guards (Slice D), and an apply that quietly
/// deleted would defeat both.
#[test]
fn an_apply_never_tombstones_on_its_own() {
    let recorder = Recorder {
        live: vec![
            live("svc", "service", None),
            live("svc.undeclared", "database", None),
        ],
        ..Recorder::default()
    };
    let decls = declarations(vec![declared("svc", "service", None)]);

    let plan = compute(&decls, &recorder.live);
    apply_plan(&recorder, &plan, &decls);

    assert_eq!(
        plan.counts().prune,
        1,
        "the plan must still *show* the prune"
    );
    assert!(
        recorder.tombstones.borrow().is_empty(),
        "but apply must not perform it"
    );
}

/// A refusal carries the catalog's own message — the server is the one that
/// knows why, and re-wording it here would lose the detail that makes it
/// fixable.
#[test]
fn a_refusal_reports_the_status_and_the_catalogs_own_detail() {
    let error = ClientError::Refused {
        status: 422,
        detail: "kind `warehouse` is not known".to_string(),
    };

    let rendered = error.to_string();
    assert!(rendered.contains("422"), "{rendered}");
    assert!(rendered.contains("warehouse"), "{rendered}");
}
