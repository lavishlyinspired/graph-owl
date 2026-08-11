//! Findings — what a domain pack's rules concluded, and why.
//!
//! **One fact shape for every domain.** A GST mismatch, a duplicate guest, a
//! missing audit trail and an overdue filing are the same thing structurally:
//! something a rule concluded about a subject, with the evidence behind it and
//! the authority it was concluded under. The platform plan's §6 makes that a
//! runtime rather than a per-domain type, and this is its data.
//!
//! **The invariant that shapes the whole type: no finding exists without its
//! citation.** `governed_by` is a required field rather than an `Option`, so
//! "which rule says so" cannot be omitted by a caller in a hurry — a finding
//! nobody can trace to an authority is an accusation, and a reviewer has no
//! way to judge it. The same reasoning makes `evidence` non-empty: a finding
//! with no supporting facts is an assertion the reviewer must take on trust,
//! which is precisely what this system refuses everywhere else.
//!
//! **The LLM narrates; it never invents one.** Construction goes through
//! [`Finding::new`], which refuses anything missing either, so the narration
//! layer has no path to a finding that was not derived.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Why a finding could not be constructed.
///
/// Each variant is an invariant the review surface depends on: a reviewer
/// looking at a queue needs to know what was concluded, about what, on what
/// evidence, and under whose authority. A finding missing any of those is not
/// a weaker finding — it is unreviewable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingError {
    /// The label naming what kind of finding this is (`gst:MissingInGstr2b`).
    MissingLabel,
    /// The graph subject the finding is about.
    MissingSubject,
    /// A one-line statement of what was concluded.
    MissingSummary,
    /// The rule, statute or policy the conclusion rests on.
    ///
    /// **Required, never defaulted.** A finding whose authority is unknown
    /// cannot be judged, only believed.
    MissingCitation,
    /// The facts the conclusion was drawn from.
    NoEvidence,
}

impl std::fmt::Display for FindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::MissingLabel => "a finding must carry a label naming what kind it is",
            Self::MissingSubject => "a finding must name the subject it is about",
            Self::MissingSummary => "a finding must state what was concluded",
            Self::MissingCitation => {
                "a finding must cite the rule it rests on — one that cannot be traced \
                 to an authority is an accusation rather than a finding"
            }
            Self::NoEvidence => {
                "a finding must carry the facts it was drawn from — one with none is \
                 an assertion the reviewer has to take on trust"
            }
        };
        write!(f, "{reason}")
    }
}

impl std::error::Error for FindingError {}

/// Where a finding is in its review.
///
/// Deliberately the same three-state shape every other review queue in this
/// system uses, because they are the same interaction: something proposes, a
/// human decides, the decision is recorded with a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingStatus {
    /// Nobody has decided yet.
    Pending,
    /// A reviewer agreed it is real.
    Accepted,
    /// A reviewer judged it wrong or not worth acting on. Always with a
    /// reason — the same rule Epic 17's merge queue enforces, and for the
    /// same purpose: the next run must be able to tell "already considered
    /// and dismissed" from "not yet seen".
    Rejected,
}

impl FindingStatus {
    /// The wire and storage spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    /// Parse the wire spelling.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "accepted" => Some(Self::Accepted),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// One fact the conclusion rests on.
///
/// A subject/predicate/value triple rather than free text, so a reviewer can
/// follow it back into the graph rather than read somebody's summary of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Evidence {
    /// The graph subject, in `Sid`'s own `{namespace}:{id}` wire form.
    pub subject: String,
    /// Which predicate.
    pub predicate: String,
    /// Its value, rendered.
    pub value: String,
    /// The rule's own query variable this value was bound from
    /// (`"filedGstin"`), when the rule that produced this finding named one.
    ///
    /// **Needed because `predicate` alone cannot disambiguate.** A rule can
    /// bind the same predicate to two different variables — `gst:supplierGstin`
    /// as both `claimedGstin` and `filedGstin` in `GstinTransposition` — and
    /// both entries then carry `subject`/`predicate` values that are
    /// identical, differing only in `value`. Anything that needs to recover
    /// *which* bound value a rule's own config refers to by name (Epic 105
    /// P7's near-miss linking, `plans/105g-...`) has no way to do that from
    /// `subject`+`predicate` alone. `None` for evidence built before this
    /// field existed, or for a rule with no named variables to carry.
    #[serde(default)]
    pub var: Option<String>,
}

/// What a rule concluded, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable identity, so a decision can be recorded against it.
    pub id: Uuid,
    /// Which pack's rules produced it. Provenance, and the filter a console
    /// queue scopes by.
    pub pack: String,
    /// What kind of finding — the pack's own vocabulary
    /// (`gst:MissingInGstr2b`, `hosp:DuplicateGuest`).
    pub label: String,
    /// The graph subject this is about.
    pub subject: String,
    /// One line: what was concluded.
    pub summary: String,
    /// The rule, statute or policy it rests on. **Never absent.**
    pub governed_by: String,
    /// The facts behind it. **Never empty.**
    pub evidence: Vec<Evidence>,
    /// Where it is in review.
    pub status: FindingStatus,
    /// When the runtime concluded it.
    pub detected_at: DateTime<Utc>,
    /// Who decided, once somebody has.
    pub decided_by: Option<String>,
    /// Why they decided that. Required on rejection.
    pub reason: Option<String>,
}

impl Finding {
    /// Construct a finding, refusing one that could not be reviewed.
    ///
    /// # Errors
    ///
    /// [`FindingError`] when the label, subject, summary or citation is blank,
    /// or when there is no evidence.
    pub fn new(
        pack: impl Into<String>,
        label: impl Into<String>,
        subject: impl Into<String>,
        summary: impl Into<String>,
        governed_by: impl Into<String>,
        evidence: Vec<Evidence>,
    ) -> Result<Self, FindingError> {
        let label = label.into();
        let subject = subject.into();
        let summary = summary.into();
        let governed_by = governed_by.into();

        if label.trim().is_empty() {
            return Err(FindingError::MissingLabel);
        }
        if subject.trim().is_empty() {
            return Err(FindingError::MissingSubject);
        }
        if summary.trim().is_empty() {
            return Err(FindingError::MissingSummary);
        }
        if governed_by.trim().is_empty() {
            return Err(FindingError::MissingCitation);
        }
        if evidence.is_empty() {
            return Err(FindingError::NoEvidence);
        }

        Ok(Self {
            id: Uuid::new_v4(),
            pack: pack.into(),
            label,
            subject,
            summary,
            governed_by,
            evidence,
            status: FindingStatus::Pending,
            detected_at: Utc::now(),
            decided_by: None,
            reason: None,
        })
    }

    /// The identity a re-run must recognise as *the same problem*.
    ///
    /// **Not the `id`.** A reconciliation run twice produces two `Finding`
    /// values with different `id`s describing one problem, and a queue that
    /// showed both would grow without bound while a reviewer worked. Keyed on
    /// what makes the problem itself distinct — which pack concluded it, what
    /// kind it is, about what, and **on what evidence** — so a second run of
    /// an unchanged corpus updates nothing rather than duplicating everything.
    ///
    /// **The evidence is part of the identity, and that was a correction.**
    /// Keyed on `(pack, label, subject)` alone, a dismissal held only while
    /// the finding stayed pending: the next scheduled run re-derived it over
    /// the very same data and put it back in the queue. A reviewer who
    /// dismisses a finding on Monday and sees it again on Tuesday, unchanged,
    /// stops reading the queue — which costs more than the duplicate ever
    /// would. Found by running the real GST reconciliation twice around a
    /// decision.
    ///
    /// Including the evidence draws the line where it belongs: the *same*
    /// conclusion from the *same* facts is the one already decided, and a
    /// changed amount is genuinely a new situation the reviewer must see.
    #[must_use]
    pub fn dedup_key(&self) -> String {
        format!(
            "{}\u{1}{}\u{1}{}\u{1}{}",
            self.pack,
            self.label,
            self.subject,
            self.evidence_digest()
        )
    }

    /// A stable digest of the facts this finding rests on.
    ///
    /// Stored, so it must not depend on the Rust version or the platform —
    /// `DefaultHasher` is explicitly not stable across either, and a digest
    /// that changed under a toolchain upgrade would silently re-open every
    /// decided finding in the estate.
    ///
    /// Order-sensitive on purpose: a rule's `evidence` list is written in the
    /// manifest, so its order is a stated fact about the rule rather than an
    /// accident, and re-ordering it is a change worth showing a reviewer.
    #[must_use]
    pub fn evidence_digest(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        for fact in &self.evidence {
            // Length-prefixed rather than delimiter-joined: a delimiter that
            // can occur inside a value lets two different evidence lists hash
            // the same, which would hide a real change.
            for part in [&fact.subject, &fact.predicate, &fact.value] {
                hasher.update(part.len().to_le_bytes());
                hasher.update(part.as_bytes());
            }
        }
        format!("{:x}", hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> Vec<Evidence> {
        vec![Evidence {
            subject: "1025:pr-INV-1003".to_string(),
            predicate: "1025:taxAmount".to_string(),
            value: "45000.00".to_string(),
            var: None,
        }]
    }

    fn finding() -> Finding {
        Finding::new(
            "gst",
            "gst:MissingInGstr2b",
            "1025:pr-INV-1003",
            "Claimed in the register, never filed by the supplier",
            "gst:Section16",
            evidence(),
        )
        .expect("a complete finding")
    }

    #[test]
    fn a_complete_finding_starts_pending_and_undecided() {
        let found = finding();

        assert_eq!(found.status, FindingStatus::Pending);
        assert_eq!(found.decided_by, None);
        assert_eq!(found.reason, None);
    }

    // ── the invariants, each as its own refusal ─────────────────────────

    #[test]
    fn a_finding_without_a_citation_is_refused() {
        // **The most important one.** A finding whose authority is unknown
        // cannot be judged, only believed — and a reviewer asked to believe
        // a machine is exactly what this system refuses everywhere else.
        let refused = Finding::new(
            "gst",
            "gst:MissingInGstr2b",
            "1025:pr-INV-1003",
            "something",
            "   ",
            evidence(),
        );

        assert_eq!(refused, Err(FindingError::MissingCitation));
    }

    #[test]
    fn a_finding_with_no_evidence_is_refused() {
        let refused = Finding::new("gst", "l", "s", "summary", "gst:Section16", vec![]);

        assert_eq!(refused, Err(FindingError::NoEvidence));
    }

    #[test]
    fn a_blank_label_subject_or_summary_is_refused() {
        // Whitespace, not just empty — a label of `" "` renders as nothing in
        // a queue and is the same failure as omitting it.
        assert_eq!(
            Finding::new("gst", " ", "s", "sum", "cite", evidence()),
            Err(FindingError::MissingLabel)
        );
        assert_eq!(
            Finding::new("gst", "l", "\t", "sum", "cite", evidence()),
            Err(FindingError::MissingSubject)
        );
        assert_eq!(
            Finding::new("gst", "l", "s", "  ", "cite", evidence()),
            Err(FindingError::MissingSummary)
        );
    }

    #[test]
    fn each_refusal_explains_what_it_protects() {
        // These reach a pack author debugging their `findings` configuration.
        // "invalid finding" would send them to a debugger; naming the missing
        // thing sends them to their manifest.
        assert!(
            FindingError::MissingCitation
                .to_string()
                .contains("accusation")
        );
        assert!(FindingError::NoEvidence.to_string().contains("trust"));
        assert!(!FindingError::MissingLabel.to_string().is_empty());
        assert!(!FindingError::MissingSubject.to_string().is_empty());
        assert!(!FindingError::MissingSummary.to_string().is_empty());
    }

    // ── dedup identity ──────────────────────────────────────────────────

    #[test]
    fn two_findings_about_the_same_problem_share_a_dedup_key() {
        // A reconciliation run twice must not double the queue while a
        // reviewer is working in it. The `id`s differ; the problem does not.
        let first = finding();
        let second = finding();

        assert_ne!(first.id, second.id, "each construction is its own value");
        assert_eq!(first.dedup_key(), second.dedup_key());
    }

    #[test]
    fn a_different_subject_label_or_pack_is_a_different_problem() {
        // The negative half: a key that collapsed everything would make one
        // reviewed finding silently suppress every later one.
        let base = finding();

        let other_subject = Finding::new(
            "gst",
            "gst:MissingInGstr2b",
            "1025:pr-INV-9999",
            "s",
            "c",
            evidence(),
        )
        .expect("valid");
        let other_label = Finding::new(
            "gst",
            "gst:TaxAmountMismatch",
            "1025:pr-INV-1003",
            "s",
            "c",
            evidence(),
        )
        .expect("valid");
        let other_pack = Finding::new(
            "hosp",
            "gst:MissingInGstr2b",
            "1025:pr-INV-1003",
            "s",
            "c",
            evidence(),
        )
        .expect("valid");

        assert_ne!(base.dedup_key(), other_subject.dedup_key());
        assert_ne!(base.dedup_key(), other_label.dedup_key());
        assert_ne!(base.dedup_key(), other_pack.dedup_key());
    }

    #[test]
    fn the_dedup_key_cannot_be_forged_by_moving_a_character() {
        // `("ab","c")` and `("a","bc")` must not collide — the same separator
        // discipline `blocking_strategy` uses, for the same reason.
        let left = Finding::new("ab", "c", "s", "sum", "cite", evidence()).expect("valid");
        let right = Finding::new("a", "bc", "s", "sum", "cite", evidence()).expect("valid");

        assert_ne!(left.dedup_key(), right.dedup_key());
    }

    // ── status round trip ───────────────────────────────────────────────

    #[test]
    fn every_status_round_trips_through_its_wire_spelling() {
        for status in [
            FindingStatus::Pending,
            FindingStatus::Accepted,
            FindingStatus::Rejected,
        ] {
            assert_eq!(FindingStatus::parse(status.as_str()), Some(status));
        }
    }

    #[test]
    fn an_unknown_status_is_not_silently_a_default() {
        // Defaulting an unrecognised status to `Pending` would resurrect a
        // decided finding into the queue on a schema drift.
        assert_eq!(FindingStatus::parse("approved"), None);
        assert_eq!(FindingStatus::parse(""), None);
        assert_eq!(FindingStatus::parse("PENDING"), None);
    }
}

#[cfg(test)]
mod wire_shape_tests {
    //! **The one assertion neither the domain nor the storage tests make.**
    //!
    //! `CLAUDE.md` records this exact trap: the domain tests compare Rust
    //! values and the repository tests compare columns, so a field that goes
    //! out in snake_case while every struct beside it is camelCase reaches the
    //! console and is silently `undefined`. A finding whose `governedBy`
    //! arrives as `governed_by` renders a citation-less finding, which is the
    //! one thing this type refuses to construct.

    use super::*;

    #[test]
    fn a_finding_serializes_in_the_wire_shape_the_console_reads() {
        let finding = Finding::new(
            "gst",
            "gst:MissingInGstr2b",
            "1025:inv-1",
            "Claimed, never filed",
            "gst:Section16",
            vec![Evidence {
                subject: "1025:inv-1".to_string(),
                predicate: "1025:taxAmount".to_string(),
                value: "45000.00".to_string(),
                var: None,
            }],
        )
        .expect("valid");

        let json = serde_json::to_value(&finding).expect("serializes");

        assert!(json.get("governedBy").is_some(), "camelCase on the wire");
        assert!(json.get("governed_by").is_none());
        assert!(json.get("detectedAt").is_some());
        assert!(json.get("decidedBy").is_some());
        assert_eq!(json["status"], "pending", "the status is a lowercase name");
        assert_eq!(
            json["evidence"][0]["predicate"], "1025:taxAmount",
            "evidence survives whole — a reviewer follows it back into the graph"
        );
    }

    #[test]
    fn a_status_round_trips_through_its_wire_name() {
        for status in [
            FindingStatus::Pending,
            FindingStatus::Accepted,
            FindingStatus::Rejected,
        ] {
            let json = serde_json::to_value(status).expect("serializes");
            assert_eq!(
                json.as_str(),
                Some(status.as_str()),
                "serde and `as_str` must agree — two spellings of the same \
                 status would let a row be written that no filter matches"
            );
            let back: FindingStatus = serde_json::from_value(json).expect("deserializes");
            assert_eq!(back, status);
        }
    }
}

#[cfg(test)]
mod recurrence_tests {
    //! **What counts as "the same problem" across runs.**
    //!
    //! Written after the real GST reconciliation was run twice around a
    //! decision and the dismissed finding came straight back. The rule these
    //! pin down: same conclusion from the same facts is already decided;
    //! same conclusion from *changed* facts is a new situation.

    use super::*;

    fn with_evidence(value: &str) -> Finding {
        Finding::new(
            "gst",
            "gst:TaxAmountMismatch",
            "1025:inv-1",
            "The tax differs",
            "gst:Section16",
            vec![Evidence {
                subject: "1025:inv-1".to_string(),
                predicate: "1025:taxAmount".to_string(),
                value: value.to_string(),
                var: None,
            }],
        )
        .expect("valid")
    }

    #[test]
    fn the_same_conclusion_from_the_same_facts_is_the_same_finding() {
        assert_eq!(
            with_evidence("18000.00").dedup_key(),
            with_evidence("18000.00").dedup_key(),
            "a scheduled re-run over unchanged data must not resurrect a \
             decision somebody already made"
        );
    }

    #[test]
    fn the_same_conclusion_from_changed_facts_is_a_new_finding() {
        assert_ne!(
            with_evidence("18000.00").dedup_key(),
            with_evidence("17100.00").dedup_key(),
            "the amount moved, so the reviewer's earlier judgement was about \
             a different situation and they must see this one"
        );
    }

    #[test]
    fn evidence_that_differs_only_in_where_a_boundary_falls_still_differs() {
        // The reason the digest is length-prefixed rather than joined with a
        // delimiter: a delimiter that can occur inside a value lets two
        // different evidence lists hash identically, and the second one would
        // then be silently suppressed as "already decided".
        let ab = Finding::new(
            "p",
            "l",
            "s",
            "y",
            "g",
            vec![Evidence {
                subject: "a".to_string(),
                predicate: "b".to_string(),
                value: "c".to_string(),
                var: None,
            }],
        )
        .expect("valid");
        let joined = Finding::new(
            "p",
            "l",
            "s",
            "y",
            "g",
            vec![Evidence {
                subject: "ab".to_string(),
                predicate: String::new(),
                value: "c".to_string(),
                var: None,
            }],
        )
        .expect("valid");

        assert_ne!(ab.evidence_digest(), joined.evidence_digest());
    }

    #[test]
    fn a_digest_does_not_depend_on_anything_that_can_change_underneath_it() {
        // A stored digest that moved under a toolchain upgrade would re-open
        // every decided finding in the estate at once. Pinned to a literal so
        // the test fails if the algorithm is ever swapped by accident.
        let finding = with_evidence("18000.00");
        assert_eq!(
            finding.evidence_digest(),
            "def523490a6d5b3dcadf77d8aab22189aa9a234dbbc3ca2c77b82bd3da5975b4",
            "computed independently as SHA-256 over the length-prefixed \
             fields, not read back off this implementation"
        );
    }
}
