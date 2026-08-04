//! Data contracts and compatibility — Epic 27.
//!
//! **The compatibility checker is the whole epic**, and it is a pure function
//! of (change, guarantee, mode). Everything else here is bookkeeping around it.
//!
//! Two things it is deliberately not. It does not *infer* compatibility from a
//! schema (decision 2): adding a column is breaking under a strict contract and
//! fine under a lenient one, and only the contract knows which. And it does not
//! *block* anything (decision 3) — graph-owl observes metadata and cannot stop a
//! warehouse `ALTER TABLE`, so a checker that refused writes would be making a
//! promise it has no way to keep.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::envelope::{ChangeDescription, EntityVersion};

/// How tolerant a contract is of schema change.
///
/// **Avro's names, and they are counterintuitive enough to be worth stating.**
/// *Backward* compatible means a **new** reader can read **old** data — so
/// removing a column breaks it, because the reader still expects that column.
/// *Forward* compatible means an **old** reader can read **new** data — so
/// adding a required column breaks it, because the old reader knows nothing
/// about it. Swapping the two is the classic mistake, which is why the matrix
/// below is written out cell by cell rather than derived.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompatibilityMode {
    /// Anything goes. A contract that guarantees nothing about schema is still
    /// useful for its SLAs and its parties.
    None,
    /// A new reader must be able to read old data: removing or renaming a
    /// column breaks it.
    Backward,
    /// An old reader must be able to read new data: adding a *required*
    /// column breaks it.
    Forward,
    /// Both directions at once — the strictest mode.
    Full,
}

impl CompatibilityMode {
    /// The wire name — the same string the database's `CHECK` constraint
    /// accepts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CompatibilityMode::None => "none",
            CompatibilityMode::Backward => "backward",
            CompatibilityMode::Forward => "forward",
            CompatibilityMode::Full => "full",
        }
    }

    /// Every mode, in the order the compatibility matrix's columns are written.
    #[must_use]
    pub const fn all() -> &'static [CompatibilityMode] {
        &[
            CompatibilityMode::None,
            CompatibilityMode::Backward,
            CompatibilityMode::Forward,
            CompatibilityMode::Full,
        ]
    }

    /// # Errors
    ///
    /// The unrecognised name, so the caller can name it back.
    pub fn parse(raw: &str) -> Result<Self, String> {
        CompatibilityMode::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == raw)
            .ok_or_else(|| raw.to_string())
    }
}

/// A schema change, classified.
///
/// **Six kinds, because the matrix has six rows.** Deriving these from a raw
/// column diff is the caller's job; this module decides what each one *means*,
/// and keeping the classification separate is what makes the decision testable
/// without a database or a diff.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "change")]
pub enum SchemaChange {
    /// A new column that may be absent — nothing that already reads the
    /// table breaks.
    AddNullableColumn {
        /// The column added.
        column: String,
    },
    /// A new column every row must have — an old reader that built its own
    /// row without it breaks.
    AddRequiredColumn {
        /// The column added.
        column: String,
    },
    /// A column no longer exists.
    RemoveColumn {
        /// The column removed.
        column: String,
    },
    /// `int → bigint`: every old value still fits, no new one does for an old
    /// reader.
    WidenType {
        /// The column widened.
        column: String,
        /// The new type.
        to: String,
    },
    /// `bigint → int`: an old value may no longer fit.
    NarrowType {
        /// The column narrowed.
        column: String,
        /// The new type.
        to: String,
    },
    /// A column keeps its type and moves to a new name.
    RenameColumn {
        /// The name before the change.
        from: String,
        /// The name after the change.
        to: String,
    },
}

impl SchemaChange {
    /// The column this change is about — the `from` name for a rename, because
    /// that is the one a guarantee would have been written against.
    #[must_use]
    pub fn column(&self) -> &str {
        match self {
            SchemaChange::AddNullableColumn { column }
            | SchemaChange::AddRequiredColumn { column }
            | SchemaChange::RemoveColumn { column }
            | SchemaChange::WidenType { column, .. }
            | SchemaChange::NarrowType { column, .. } => column,
            SchemaChange::RenameColumn { from, .. } => from,
        }
    }

    /// Whether this change introduces a column that was not there before.
    #[must_use]
    pub fn is_addition(&self) -> bool {
        matches!(
            self,
            SchemaChange::AddNullableColumn { .. } | SchemaChange::AddRequiredColumn { .. }
        )
    }
}

/// One column a contract promises will be there.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnGuarantee {
    /// The column name.
    pub name: String,
    /// The type the contract promises, in the producer's own vocabulary.
    pub data_type: String,
    /// Whether the column may be absent on a row.
    pub nullable: bool,
}

/// What a contract promises about shape.
#[derive(utoipa::ToSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaGuarantee {
    /// The columns the contract promises will be there.
    pub required_columns: Vec<ColumnGuarantee>,
    /// **Overrides the mode when false.** A consumer that reads with
    /// `SELECT *` into a fixed struct breaks on *any* new column, however
    /// nullable — so a contract may forbid additions outright, and that
    /// forbidding has to beat a lenient mode or the flag would mean nothing.
    pub allow_additional: bool,
}

/// Where a contract stands.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContractStatus {
    /// Proposed but not yet agreed to — not enforced.
    Draft,
    /// Agreed to and enforced.
    Active,
    /// A breach was recorded. **Not cleared by a later compatible change** —
    /// silent clearing would hide the incident, which is the thing a contract
    /// exists to surface.
    Violated,
    /// No longer in force — not enforced, and history rather than a promise.
    Terminated,
}

impl ContractStatus {
    /// The wire name — the same string the database's `CHECK` constraint
    /// accepts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ContractStatus::Draft => "draft",
            ContractStatus::Active => "active",
            ContractStatus::Violated => "violated",
            ContractStatus::Terminated => "terminated",
        }
    }

    /// Every status, in the order the wire's `CHECK` constraint lists them.
    #[must_use]
    pub const fn all() -> &'static [ContractStatus] {
        &[
            ContractStatus::Draft,
            ContractStatus::Active,
            ContractStatus::Violated,
            ContractStatus::Terminated,
        ]
    }

    /// # Errors
    ///
    /// The unrecognised name.
    pub fn parse(raw: &str) -> Result<Self, String> {
        ContractStatus::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == raw)
            .ok_or_else(|| raw.to_string())
    }

    /// Whether a contract in this state is evaluated against schema changes.
    ///
    /// A `Draft` contract is a proposal nobody has agreed to, and a
    /// `Terminated` one is history — breaching either is not a fact about the
    /// world. **A `Violated` one is still checked**, because breaches
    /// accumulate: a second incident on an already-broken contract is still an
    /// incident, and stopping at the first would hide everything after it.
    #[must_use]
    pub fn is_enforced(self) -> bool {
        matches!(self, ContractStatus::Active | ContractStatus::Violated)
    }
}

/// What a compatibility check concluded.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "verdict")]
pub enum Compatibility {
    /// The change does not breach the guarantee.
    Compatible,
    /// **Names what was breached and why**, because "incompatible" tells a
    /// producer nothing they can act on — they need to know which column and
    /// which promise.
    Breach {
        /// The column the breach is about.
        column: String,
        /// What was breached, in words a producer can act on.
        detail: String,
    },
}

impl Compatibility {
    /// Whether this verdict is a breach.
    #[must_use]
    pub fn is_breach(&self) -> bool {
        matches!(self, Compatibility::Breach { .. })
    }
}

/// Whether a schema change breaches a contract.
///
/// **The 24-cell matrix from the plan, written out rather than derived.** Every
/// shortcut that would compress it — "removal is always breaking", "Full is
/// Backward plus Forward" — is a place a future edit could get one cell wrong
/// while the others keep passing. The table-driven test below is the
/// specification; this function is its implementation.
///
/// Two rules sit *outside* the matrix and are applied first, in this order:
///
/// 1. **A change to a column the contract never mentioned is not a breach** of
///    a column guarantee — a contract promising `id` and `amount` says nothing
///    about `internal_notes`, and reporting a breach there would make every
///    contract a whole-table lock.
/// 2. **`allow_additional: false` makes any addition a breach**, whatever the
///    mode. A consumer reading `SELECT *` into a fixed struct breaks on a new
///    column however nullable it is, and a lenient mode must not override an
///    explicit refusal.
///
/// **`match_same_arms` is allowed here on purpose.** Clippy is right that a
/// dozen arms return `false` and could be merged into one — and merging them is
/// precisely what makes a future edit able to get a single cell wrong while the
/// other twenty-three keep passing. The table is the specification; it is
/// written out to be *read against the plan*, not to be compressed.
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn check_compatibility(
    change: &SchemaChange,
    guarantee: &SchemaGuarantee,
    mode: CompatibilityMode,
) -> Compatibility {
    use CompatibilityMode::{Backward, Forward, Full, None};

    // Rule 2 first, because it beats the mode — including `None`. A contract
    // that says "no new columns" and a mode that says "anything goes" is a
    // contract whose author was explicit about one thing and vague about the
    // other, and the explicit one wins.
    if change.is_addition() && !guarantee.allow_additional {
        return Compatibility::Breach {
            column: change.column().to_string(),
            detail: "this contract does not permit additional columns".to_string(),
        };
    }

    // Rule 1: a guarantee that does not mention the column cannot be breached
    // by a change to it. Additions are exempt — an added column is by
    // definition not in the guarantee, and rule 2 has already had its say.
    let guaranteed = guarantee
        .required_columns
        .iter()
        .any(|column| column.name == change.column());
    if !guaranteed && !change.is_addition() {
        return Compatibility::Compatible;
    }

    let breaks = match (change, mode) {
        // Adding is always fine once `allow_additional` has permitted it —
        // except that a *required* addition breaks an old reader, which is
        // what Forward protects.
        (SchemaChange::AddNullableColumn { .. }, _) => false,
        (SchemaChange::AddRequiredColumn { .. }, None | Backward) => false,
        (SchemaChange::AddRequiredColumn { .. }, Forward | Full) => true,

        // Removal breaks a *new* reader that still expects the column.
        (SchemaChange::RemoveColumn { .. }, None | Forward) => false,
        (SchemaChange::RemoveColumn { .. }, Backward | Full) => true,

        // Widening: every old value fits the new type, so a new reader is
        // fine — but an old reader cannot hold the new range.
        (SchemaChange::WidenType { .. }, None | Backward) => false,
        (SchemaChange::WidenType { .. }, Forward | Full) => true,

        // Narrowing is the mirror image.
        (SchemaChange::NarrowType { .. }, None | Forward) => false,
        (SchemaChange::NarrowType { .. }, Backward | Full) => true,

        // A rename is a removal and an addition at once, so it breaks
        // everything except a contract that guarantees nothing.
        (SchemaChange::RenameColumn { .. }, None) => false,
        (SchemaChange::RenameColumn { .. }, Backward | Forward | Full) => true,
    };

    if breaks {
        Compatibility::Breach {
            column: change.column().to_string(),
            detail: format!(
                "`{}` is not permitted under `{}` compatibility",
                describe(change),
                mode.as_str()
            ),
        }
    } else {
        Compatibility::Compatible
    }
}

/// A change in words, for the breach detail.
fn describe(change: &SchemaChange) -> String {
    match change {
        SchemaChange::AddNullableColumn { column } => format!("adding nullable column {column}"),
        SchemaChange::AddRequiredColumn { column } => format!("adding required column {column}"),
        SchemaChange::RemoveColumn { column } => format!("removing column {column}"),
        SchemaChange::WidenType { column, to } => format!("widening {column} to {to}"),
        SchemaChange::NarrowType { column, to } => format!("narrowing {column} to {to}"),
        SchemaChange::RenameColumn { from, to } => format!("renaming {from} to {to}"),
    }
}

/// A promise about behaviour rather than shape.
///
/// **Per-field `rename`, not the enum-level `rename_all_fields`.** Serde honours
/// either, but utoipa 5's schema derive reads only the per-field form — so
/// `rename_all_fields` produces correct JSON and a *wrong OpenAPI schema*, which
/// is worse than either being wrong alone because the two then disagree and
/// every generated client is built against the lie. `graph_owl_core::resolution`
/// learned this first; this is the same rule.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Sla {
    /// Data must not be older than this.
    Freshness {
        /// The maximum age, in seconds.
        #[serde(rename = "maxAgeSeconds")]
        max_age_seconds: i64,
    },
    /// The asset must be queryable at least this fraction of the time.
    Availability {
        /// The minimum uptime, as a percentage.
        #[serde(rename = "minUptimePct")]
        min_uptime_pct: f64,
    },
    /// The asset must hold at least this many rows.
    Completeness {
        /// The minimum row count.
        #[serde(rename = "minRowCount")]
        min_row_count: i64,
    },
    /// Quality checks over a trailing window must pass at least this often.
    QualityPassRate {
        /// The minimum pass rate, as a percentage.
        #[serde(rename = "minPct")]
        min_pct: f64,
        /// The trailing window the rate is measured over, in seconds.
        #[serde(rename = "windowSeconds")]
        window_seconds: i64,
    },
}

/// Whether an SLA is being met.
///
/// **`Unknown` is not `Met`, and the distinction is the whole point.** An SLA
/// with no corresponding signal has not been satisfied — it has not been
/// *measured*, and reporting it as satisfied manufactures confidence out of
/// missing data. The same principle as Epic 30's health and Epic 26's
/// certification status.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum SlaEvaluation {
    /// The SLA is being met.
    Met,
    /// The SLA is not being met.
    Breached {
        /// What was actually observed, in words a reader can compare against
        /// the promise.
        observed: String,
    },
    /// Nothing has been measured. Distinct from `Met` and from `Breached`.
    Unknown,
}

/// A data contract between a producer and its consumers.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contract {
    /// The stable identifier.
    pub id: Uuid,
    /// A human-readable name.
    pub name: String,
    /// The asset the promise is about.
    pub asset_fqn: String,
    /// The team that owns the asset and makes the promise.
    pub producer: String,
    /// The teams that depend on it. **Plural**, because a contract with one
    /// consumer is a special case of the real thing, not the other way round.
    #[serde(default)]
    pub consumers: Vec<String>,
    /// What the contract promises about shape.
    #[serde(default)]
    pub schema_guarantee: SchemaGuarantee,
    /// What the contract promises about behaviour.
    #[serde(default)]
    pub slas: Vec<Sla>,
    /// How tolerant the contract is of schema change.
    pub compatibility: CompatibilityMode,
    /// Where the contract stands.
    pub status: ContractStatus,
    /// The envelope's version, bumped on every change.
    pub version: EntityVersion,
    /// Who or what made the most recent change.
    pub updated_by: String,
    /// A human-readable note on the most recent change, if one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_description: Option<ChangeDescription>,
    /// When the contract was first created.
    pub created_at: DateTime<Utc>,
    /// When the contract was most recently changed.
    pub updated_at: DateTime<Utc>,
}

/// One recorded breach.
///
/// **Kept, and they accumulate.** A later compatible change does not clear an
/// earlier breach: the incident happened, and a contract that forgot it would
/// let a producer break something on Monday and look clean on Tuesday.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractBreach {
    /// The stable identifier.
    pub id: Uuid,
    /// The contract that was breached.
    pub contract_id: Uuid,
    /// The column the breach is about.
    pub column: String,
    /// What was breached, in words a producer can act on.
    pub detail: String,
    /// The asset version that caused it, so "when did this break" is answerable
    /// against the asset's own history.
    pub asset_version: String,
    /// When the breach was detected.
    pub detected_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guarantee(allow_additional: bool) -> SchemaGuarantee {
        SchemaGuarantee {
            required_columns: vec![
                ColumnGuarantee {
                    name: "id".to_string(),
                    data_type: "int".to_string(),
                    nullable: false,
                },
                ColumnGuarantee {
                    name: "amount".to_string(),
                    data_type: "int".to_string(),
                    nullable: true,
                },
            ],
            allow_additional,
        }
    }

    fn changes() -> Vec<(&'static str, SchemaChange)> {
        vec![
            (
                "add nullable",
                SchemaChange::AddNullableColumn {
                    column: "note".to_string(),
                },
            ),
            (
                "add required",
                SchemaChange::AddRequiredColumn {
                    column: "note".to_string(),
                },
            ),
            (
                "remove",
                SchemaChange::RemoveColumn {
                    column: "amount".to_string(),
                },
            ),
            (
                "widen",
                SchemaChange::WidenType {
                    column: "amount".to_string(),
                    to: "bigint".to_string(),
                },
            ),
            (
                "narrow",
                SchemaChange::NarrowType {
                    column: "amount".to_string(),
                    to: "smallint".to_string(),
                },
            ),
            (
                "rename",
                SchemaChange::RenameColumn {
                    from: "amount".to_string(),
                    to: "total".to_string(),
                },
            ),
        ]
    }

    /// **The specification, cell by cell.** Six changes × four modes = 24, and
    /// the table is written out rather than derived so that a future edit
    /// getting one cell wrong cannot hide behind a formula that still produces
    /// the other 23.
    ///
    /// `true` means breach. Read the columns as None, Backward, Forward, Full.
    #[test]
    fn the_compatibility_matrix_is_exactly_the_plans_table() {
        let expected: [(&str, [bool; 4]); 6] = [
            //                    None,  Backward, Forward, Full
            ("add nullable", [false, false, false, false]),
            ("add required", [false, false, true, true]),
            ("remove", [false, true, false, true]),
            ("widen", [false, false, true, true]),
            ("narrow", [false, true, false, true]),
            ("rename", [false, true, true, true]),
        ];

        for (label, change) in changes() {
            let row = expected
                .iter()
                .find(|(name, _)| *name == label)
                .unwrap_or_else(|| panic!("no expectation for {label}"))
                .1;
            for (index, mode) in CompatibilityMode::all().iter().enumerate() {
                let verdict = check_compatibility(&change, &guarantee(true), *mode);
                assert_eq!(
                    verdict.is_breach(),
                    row[index],
                    "{label} under {}: expected breach={}, got {verdict:?}",
                    mode.as_str(),
                    row[index]
                );
            }
        }
    }

    /// **`None` breaches nothing**, which is the row an unconditional
    /// "incompatible" would fail six times over.
    #[test]
    fn the_none_mode_permits_every_change() {
        for (label, change) in changes() {
            assert!(
                !check_compatibility(&change, &guarantee(true), CompatibilityMode::None)
                    .is_breach(),
                "{label} must be permitted under `none`"
            );
        }
    }

    /// **`Full` breaches everything except a nullable addition**, which is the
    /// row an unconditional "compatible" would fail five times over.
    #[test]
    fn the_full_mode_permits_only_a_nullable_addition() {
        for (label, change) in changes() {
            let breach =
                check_compatibility(&change, &guarantee(true), CompatibilityMode::Full).is_breach();
            assert_eq!(
                breach,
                label != "add nullable",
                "{label} under `full`: got breach={breach}"
            );
        }
    }

    /// **Backward and Forward are not the same, and swapping them is the
    /// classic error** — the names are counterintuitive, so this asserts the
    /// two cells where they differ most visibly.
    #[test]
    fn backward_and_forward_disagree_about_removal_and_required_additions() {
        let remove = SchemaChange::RemoveColumn {
            column: "amount".to_string(),
        };
        let add = SchemaChange::AddRequiredColumn {
            column: "note".to_string(),
        };

        assert!(
            check_compatibility(&remove, &guarantee(true), CompatibilityMode::Backward).is_breach(),
            "a new reader still expects the removed column"
        );
        assert!(
            !check_compatibility(&remove, &guarantee(true), CompatibilityMode::Forward).is_breach(),
            "an old reader never knew about it going away"
        );
        assert!(
            !check_compatibility(&add, &guarantee(true), CompatibilityMode::Backward).is_breach(),
            "a new reader knows about the new column"
        );
        assert!(
            check_compatibility(&add, &guarantee(true), CompatibilityMode::Forward).is_breach(),
            "an old reader knows nothing about it"
        );
    }

    // ---- the two rules outside the matrix ----

    /// **`allow_additional: false` beats the mode**, including `None`. A
    /// consumer reading `SELECT *` into a fixed struct breaks on any new
    /// column, and a lenient mode must not override an explicit refusal.
    #[test]
    fn forbidding_additions_overrides_even_the_most_lenient_mode() {
        let add = SchemaChange::AddNullableColumn {
            column: "note".to_string(),
        };

        for mode in CompatibilityMode::all() {
            let verdict = check_compatibility(&add, &guarantee(false), *mode);
            assert!(
                verdict.is_breach(),
                "an addition must breach under {} when additions are forbidden",
                mode.as_str()
            );
        }
    }

    /// And the negative, or the flag would be a ban rather than a switch.
    #[test]
    fn permitting_additions_lets_a_nullable_one_through_under_every_mode() {
        let add = SchemaChange::AddNullableColumn {
            column: "note".to_string(),
        };

        for mode in CompatibilityMode::all() {
            assert!(!check_compatibility(&add, &guarantee(true), *mode).is_breach());
        }
    }

    /// **A change to a column the contract never mentioned is not a breach.**
    /// Otherwise every contract becomes a whole-table lock, and nobody could
    /// add an internal column to a table anyone depends on.
    #[test]
    fn a_change_to_an_unguaranteed_column_is_never_a_breach() {
        let remove = SchemaChange::RemoveColumn {
            column: "internal_notes".to_string(),
        };

        for mode in CompatibilityMode::all() {
            assert!(
                !check_compatibility(&remove, &guarantee(true), *mode).is_breach(),
                "under {}",
                mode.as_str()
            );
        }
    }

    /// And the guaranteed column *is* checked, or the rule above would silently
    /// exempt everything.
    #[test]
    fn a_change_to_a_guaranteed_column_is_checked() {
        let remove = SchemaChange::RemoveColumn {
            column: "amount".to_string(),
        };

        assert!(
            check_compatibility(&remove, &guarantee(true), CompatibilityMode::Backward).is_breach()
        );
    }

    /// A breach names the column and the reason — "incompatible" alone tells a
    /// producer nothing they can act on.
    #[test]
    fn a_breach_names_the_column_and_why() {
        let change = SchemaChange::RemoveColumn {
            column: "amount".to_string(),
        };

        match check_compatibility(&change, &guarantee(true), CompatibilityMode::Full) {
            Compatibility::Breach { column, detail } => {
                assert_eq!(column, "amount");
                assert!(detail.contains("amount"), "{detail}");
                assert!(detail.contains("full"), "{detail}");
            }
            other @ Compatibility::Compatible => panic!("expected a breach, got {other:?}"),
        }
    }

    // ---- enforcement states ----

    /// **A violated contract keeps being checked.** Breaches accumulate: a
    /// second incident on an already-broken contract is still an incident, and
    /// stopping at the first would hide everything after it.
    #[test]
    fn draft_and_terminated_contracts_are_not_enforced_but_violated_ones_are() {
        assert!(ContractStatus::Active.is_enforced());
        assert!(
            ContractStatus::Violated.is_enforced(),
            "breaches accumulate rather than stopping at the first"
        );
        assert!(!ContractStatus::Draft.is_enforced());
        assert!(!ContractStatus::Terminated.is_enforced());
    }

    // ---- wire shapes ----

    #[test]
    fn modes_and_statuses_round_trip_through_their_wire_names() {
        for mode in CompatibilityMode::all() {
            assert_eq!(CompatibilityMode::parse(mode.as_str()), Ok(*mode));
        }
        for status in ContractStatus::all() {
            assert_eq!(ContractStatus::parse(status.as_str()), Ok(*status));
        }
        assert!(CompatibilityMode::parse("sideways").is_err());
        assert!(ContractStatus::parse("broken").is_err());
        // Pinned to the literals as well as round-tripped: these are the values
        // the database's own `CHECK` accepts, and a round trip alone passes with
        // both halves renamed in step.
        assert_eq!(CompatibilityMode::Backward.as_str(), "backward");
        assert_eq!(ContractStatus::Violated.as_str(), "violated");
    }

    #[test]
    fn a_schema_change_is_camel_case_and_tagged_on_the_wire() {
        let json = serde_json::to_value(SchemaChange::AddRequiredColumn {
            column: "note".to_string(),
        })
        .expect("serialize");

        assert_eq!(json["change"], "addRequiredColumn");
        assert!(json.get("column").is_some(), "{json}");
    }

    /// **The assertion that was missing, and the fifth occurrence it would have
    /// caught.** Every earlier test on a tagged enum here happened to pick a
    /// variant whose fields are single words — `column`, `detail`, `observed` —
    /// which are immune to the bug by luck. `Sla` is the first with a
    /// multi-word field, and it shipped `max_age_seconds` on a camelCase wire.
    ///
    /// So the rule this test encodes: **assert the multi-word field, or the
    /// test proves nothing.**
    #[test]
    fn every_sla_variant_is_camel_case_on_the_wire() {
        let cases = [
            (
                Sla::Freshness {
                    max_age_seconds: 3600,
                },
                "maxAgeSeconds",
                "max_age_seconds",
            ),
            (
                Sla::Availability {
                    min_uptime_pct: 99.9,
                },
                "minUptimePct",
                "min_uptime_pct",
            ),
            (
                Sla::Completeness { min_row_count: 10 },
                "minRowCount",
                "min_row_count",
            ),
            (
                Sla::QualityPassRate {
                    min_pct: 95.0,
                    window_seconds: 86_400,
                },
                "windowSeconds",
                "window_seconds",
            ),
        ];

        for (sla, expected, forbidden) in cases {
            let json = serde_json::to_value(&sla).expect("serialize");
            assert!(json.get(expected).is_some(), "{json}");
            assert!(
                json.get(forbidden).is_none(),
                "a snake_case key beside camelCase ones: {json}"
            );
            // And it round-trips, so the rename is on both halves rather than
            // producing something the server cannot read back.
            let parsed: Sla = serde_json::from_value(json).expect("deserialize");
            assert_eq!(parsed, sla);
        }
    }

    #[test]
    fn an_sla_evaluation_is_camel_case_and_tagged_on_the_wire() {
        let json = serde_json::to_value(SlaEvaluation::Breached {
            observed: "4h".to_string(),
        })
        .expect("serialize");

        assert_eq!(json["state"], "breached");
        assert!(json.get("observed").is_some(), "{json}");

        let unknown = serde_json::to_value(SlaEvaluation::Unknown).expect("serialize");
        assert_eq!(unknown["state"], "unknown");
    }

    #[test]
    fn a_guarantee_round_trips_through_json() {
        let original = guarantee(false);

        let json = serde_json::to_value(&original).expect("serialize");
        assert!(json.get("allowAdditional").is_some(), "{json}");
        assert!(json.get("requiredColumns").is_some(), "{json}");

        let parsed: SchemaGuarantee = serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, original);
    }
}
