//! Output formats — the `--format json` half of the CLI conventions.
//!
//! **Data to stdout, diagnostics to stderr** is the other half, and it is
//! the caller's job: nothing here prints. These functions return strings so
//! the decision about *which stream* stays at the one place that knows
//! whether a given string is the answer or a comment about it — a helper
//! that printed would take that choice away from every caller.
//!
//! Every structured command renders through here, so adding a command
//! cannot accidentally ship human-only output.

use serde::Serialize;

use crate::drift::{Drift, DriftKind, DriftReport};
use crate::plan::{Change, Plan};
use crate::validate::ValidationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Aligned, human-scannable. The default, because the common case is a
    /// person reading a plan before approving it.
    #[default]
    Text,
    /// Machine-readable. **A stable contract**, not a debug dump: a
    /// pipeline that parses this should not break because a message was
    /// reworded, which is exactly why the JSON carries enum-ish `change`
    /// and `kind` strings rather than the rendered prose.
    Json,
}

impl std::str::FromStr for Format {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "text" => Ok(Format::Text),
            "json" => Ok(Format::Json),
            other => Err(format!(
                "unknown format `{other}`; expected `text` or `json`"
            )),
        }
    }
}

// ── the JSON shapes, named so they are a contract rather than an accident ──

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanJson<'a> {
    entities: Vec<PlanEntityJson<'a>>,
    summary: SummaryJson,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanEntityJson<'a> {
    fully_qualified_name: &'a str,
    kind: &'a str,
    /// `create` | `update` | `noChange` | `prune` — the field a pipeline
    /// branches on, deliberately independent of any human wording.
    change: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<FieldJson<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldJson<'a> {
    field: &'a str,
    before: Option<&'a str>,
    after: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryJson {
    create: usize,
    update: usize,
    no_change: usize,
    prune: usize,
    /// Duplicated from the counts on purpose: it is the single question CI
    /// asks, and making every consumer re-derive it invites two of them to
    /// derive it differently.
    has_changes: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DriftJson<'a> {
    drifted: Vec<DriftEntryJson<'a>>,
    clean: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DriftEntryJson<'a> {
    fully_qualified_name: &'a str,
    /// `liveEdited` | `unapplied` — the distinction the whole command
    /// exists to draw, so it must survive into the machine output rather
    /// than living only in prose.
    kind: &'static str,
    detail: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorsJson<'a> {
    errors: Vec<ErrorJson<'a>>,
    count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorJson<'a> {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    detail: &'a str,
}

// ── rendering ──────────────────────────────────────────────────────────

const fn change_tag(change: &Change) -> &'static str {
    match change {
        Change::Create => "create",
        Change::Update { .. } => "update",
        Change::NoChange => "noChange",
        Change::Prune => "prune",
    }
}

/// # Errors
///
/// Propagates a serialization failure, which these types cannot actually
/// produce — but a CLI that `unwrap`s while writing output a pipeline
/// depends on is a CLI that panics in someone's CI.
pub fn plan(plan: &Plan, format: Format) -> Result<String, serde_json::Error> {
    match format {
        Format::Text => Ok(crate::plan::render(plan)),
        Format::Json => {
            let counts = plan.counts();
            let document = PlanJson {
                entities: plan
                    .entities
                    .iter()
                    .map(|entity| PlanEntityJson {
                        fully_qualified_name: &entity.fully_qualified_name,
                        kind: &entity.kind,
                        change: change_tag(&entity.change),
                        fields: match &entity.change {
                            Change::Update { fields } => fields
                                .iter()
                                .map(|f| FieldJson {
                                    field: &f.field,
                                    before: f.before.as_deref(),
                                    after: f.after.as_deref(),
                                })
                                .collect(),
                            _ => Vec::new(),
                        },
                    })
                    .collect(),
                summary: SummaryJson {
                    create: counts.create,
                    update: counts.update,
                    no_change: counts.no_change,
                    prune: counts.prune,
                    has_changes: plan.has_changes(),
                },
            };
            serde_json::to_string_pretty(&document)
        }
    }
}

/// # Errors
///
/// Propagates a serialization failure.
pub fn drift(report: &DriftReport, format: Format) -> Result<String, serde_json::Error> {
    match format {
        Format::Text => {
            let mut out = String::new();
            for entry in &report.drifted {
                out.push_str(&format!(
                    "{} {}: {}\n",
                    match entry.kind {
                        DriftKind::LiveEdited => "live-edited",
                        DriftKind::Unapplied => "unapplied  ",
                    },
                    entry.fully_qualified_name,
                    entry.detail
                ));
            }
            if report.is_clean() {
                out.push_str("no drift\n");
            }
            Ok(out)
        }
        Format::Json => serde_json::to_string_pretty(&DriftJson {
            drifted: report.drifted.iter().map(drift_entry).collect(),
            clean: report.is_clean(),
        }),
    }
}

fn drift_entry(entry: &Drift) -> DriftEntryJson<'_> {
    DriftEntryJson {
        fully_qualified_name: &entry.fully_qualified_name,
        kind: match entry.kind {
            DriftKind::LiveEdited => "liveEdited",
            DriftKind::Unapplied => "unapplied",
        },
        detail: &entry.detail,
    }
}

/// Validation errors, which are **data** when asked for as JSON — a CI job
/// that annotates a pull request needs them structured, not scraped out of
/// a human message.
///
/// # Errors
///
/// Propagates a serialization failure.
pub fn errors(errors: &[ValidationError], format: Format) -> Result<String, serde_json::Error> {
    match format {
        Format::Text => Ok(errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")),
        Format::Json => serde_json::to_string_pretty(&ErrorsJson {
            errors: errors
                .iter()
                .map(|e| ErrorJson {
                    file: e.file.display().to_string(),
                    line: e.line,
                    detail: &e.detail,
                })
                .collect(),
            count: errors.len(),
        }),
    }
}
