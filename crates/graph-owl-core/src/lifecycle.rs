//! Lifecycle and certification — Epic 26.
//!
//! **Two orthogonal axes** (decision 3). An asset can be Active-uncertified,
//! Active-certified, or Deprecated-certified — still trustworthy, and going
//! away. Collapsing them loses exactly the distinction that matters most to
//! somebody deciding whether to build on it.
//!
//! **Certification status is computed, never stored** — the whole of Slice D.
//! A stored status goes stale without the entity changing, so an asset would
//! read as certified for as long as nobody wrote to it. The same reasoning as
//! Epic 30's health and Epic 31's staleness.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Where an asset is in its life.
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifecycleState {
    Draft,
    Active,
    /// Going away, with a machine-readable successor (decision 2).
    Deprecated,
    /// Gone, but still readable. Excluded from search by default, which is the
    /// same distinction soft delete draws: hidden from discovery, present on a
    /// direct read, because a link that 404s is worse than one that says
    /// "retired".
    Retired,
}

impl Default for LifecycleState {
    /// **`Active`, not `Draft`.** Everything already in a catalog got there
    /// from a connector or a deliberate write; defaulting to `Draft` would
    /// retroactively mark a whole estate unfinished.
    fn default() -> Self {
        LifecycleState::Active
    }
}

impl LifecycleState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LifecycleState::Draft => "draft",
            LifecycleState::Active => "active",
            LifecycleState::Deprecated => "deprecated",
            LifecycleState::Retired => "retired",
        }
    }

    #[must_use]
    pub const fn all() -> &'static [LifecycleState] {
        &[
            LifecycleState::Draft,
            LifecycleState::Active,
            LifecycleState::Deprecated,
            LifecycleState::Retired,
        ]
    }

    /// # Errors
    ///
    /// The unrecognised name, so the caller can name it back.
    pub fn parse(raw: &str) -> Result<Self, String> {
        LifecycleState::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == raw)
            .ok_or_else(|| raw.to_string())
    }
}

/// Whether a lifecycle move is legal.
///
/// **`Retired` is terminal, and `Draft → Retired` is not a shortcut.** An asset
/// that was never active has nothing to retire *from*; letting it skip states
/// would make "retired" mean two different things — "we turned it off" and "we
/// abandoned it before it started" — and a consumer cannot tell those apart.
///
/// `Deprecated → Active` is legal on purpose: un-deprecating is a real
/// correction, and forcing a new asset to undo a mistaken deprecation would
/// break every reference to the old one.
#[must_use]
pub fn can_transition(from: LifecycleState, to: LifecycleState) -> bool {
    use LifecycleState::{Active, Deprecated, Draft, Retired};
    matches!(
        (from, to),
        (Draft | Deprecated, Active) | (Active, Deprecated) | (Deprecated, Retired)
    )
}

/// Why something is going away, and what to use instead.
#[derive(utoipa::ToSchema, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deprecation {
    /// Required. "Deprecated" with no reason is a state nobody can act on.
    pub reason: String,
    /// **A reference, not prose** (decision 2). "Use `orders_v2` instead" has
    /// to be machine-readable so an agent can redirect rather than merely warn
    /// — an agent that recommends a dead asset without saying so is the most
    /// damaging failure this system can produce, because it is confidently
    /// wrong in a way the reader cannot detect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_fqn: Option<String>,
    pub deprecated_at: DateTime<Utc>,
    /// When it becomes `Retired`. Absent means "no date decided yet", which is
    /// honest and common.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sunset_at: Option<DateTime<Utc>>,
}

/// What a certification is worth right now.
///
/// **Computed on read.** See the module note: a stored status goes stale
/// without the entity changing.
/// **`rename_all_fields` is not redundant with `rename_all`.** On an enum,
/// `rename_all` renames the *variants*; the fields inside them keep their Rust
/// spelling unless `rename_all_fields` says so. Without it this ships
/// `days_remaining` beside a wire of camelCase — and this codebase has now
/// made that exact mistake four times (`Authorship.agent_id`,
/// `SubmissionOutcome.run_id`, `AssetListQuery.data_product`, this).
#[derive(utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum CertificationStatus {
    Valid,
    /// Within the warning window, with how long is left. The number is what
    /// makes it actionable — "expiring soon" alone tells a steward nothing
    /// about whether to act today or next quarter.
    ExpiringSoon {
        days_remaining: i64,
    },
    Expired,
    /// Never certified. Distinct from `Expired`: one was never vouched for, the
    /// other was and no longer is, and a consumer should treat those
    /// differently.
    None,
}

/// How long before expiry a certification starts warning.
///
/// Thirty days is a review cycle: long enough that a steward can schedule the
/// re-check into ordinary work, short enough that the warning still means
/// something when it appears. Configurable per deployment because review
/// cadences differ; the default has to be *some* number and this one is
/// derived from the cadence it exists to trigger.
pub const DEFAULT_EXPIRY_WINDOW_DAYS: i64 = 30;

/// The status of a certification expiring at `expires_at`, as of `now`.
///
/// **The boundary is `expires_at` itself: at exactly that instant it is
/// expired.** A certification valid *through* its expiry instant and invalid
/// one nanosecond later is a distinction nobody can act on, and "expires at
/// noon" universally means it is no good at noon.
#[must_use]
pub fn certification_status(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    window_days: i64,
) -> CertificationStatus {
    let Some(expires_at) = expires_at else {
        return CertificationStatus::None;
    };
    if now >= expires_at {
        return CertificationStatus::Expired;
    }
    let remaining = expires_at - now;
    if remaining <= Duration::days(window_days) {
        // Rounded **up**, so a certification with six hours left reports one
        // day rather than zero. Reporting zero days on something not yet
        // expired reads as expired, which is the wrong side to be wrong on.
        let days = remaining.num_seconds().div_euclid(86_400)
            + i64::from(remaining.num_seconds().rem_euclid(86_400) > 0);
        return CertificationStatus::ExpiringSoon {
            days_remaining: days,
        };
    }
    CertificationStatus::Valid
}

/// The strongest status among several certifications.
///
/// An asset may hold more than one — "Gold" and "Finance-Approved" are
/// different claims. The summary takes the **best**, because "is this
/// certified" is answered yes by any valid one; an expired Gold beside a valid
/// Finance-Approved is not an expired asset.
#[must_use]
pub fn best_status(statuses: &[CertificationStatus]) -> CertificationStatus {
    statuses
        .iter()
        .copied()
        .max_by_key(|status| match status {
            CertificationStatus::Valid => 3,
            CertificationStatus::ExpiringSoon { .. } => 2,
            CertificationStatus::Expired => 1,
            CertificationStatus::None => 0,
        })
        .unwrap_or(CertificationStatus::None)
}

/// What a certification type demands before it may be issued.
///
/// **Open text, not an enum.** The list of things an organization treats as
/// evidence is theirs — "a passing freshness test", "the owner confirmed in
/// writing", "SOC2 control 4.1" — and an enum would mean a release per
/// organization.
pub type EvidenceKind = String;

/// Which required evidence a submission is missing.
///
/// **Named, not counted.** "Evidence is missing" tells an issuer nothing they
/// can act on; the list tells them what to go and get. Comparison is exact:
/// an organization that distinguishes `qualityTests` from `qualityTest` means
/// two different things by them, and normalising would silently accept the
/// wrong one.
#[must_use]
pub fn missing_evidence(required: &[EvidenceKind], supplied: &[EvidenceKind]) -> Vec<String> {
    required
        .iter()
        .filter(|kind| !supplied.contains(kind))
        .cloned()
        .collect()
}

/// Whether `principal` may issue this certification type.
///
/// **An empty allowlist means anyone**, and that is deliberate rather than an
/// oversight: a type nobody has restricted is a type the organization has not
/// decided about, and refusing every issuance would make defining a type
/// useless until somebody also configured issuers. The restriction is opt-in,
/// and `authorized_issuers` being non-empty is the opt-in.
#[must_use]
pub fn may_issue(authorized_issuers: &[String], principal: &str) -> bool {
    authorized_issuers.is_empty() || authorized_issuers.iter().any(|id| id == principal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use LifecycleState::{Active, Deprecated, Draft, Retired};

    // ---- the transition matrix ----

    #[test]
    fn the_legal_moves_are_permitted() {
        assert!(can_transition(Draft, Active));
        assert!(can_transition(Active, Deprecated));
        assert!(can_transition(Deprecated, Retired));
    }

    /// Un-deprecating is a real correction. Forcing a new asset to undo a
    /// mistaken deprecation would break every reference to the old one.
    #[test]
    fn deprecated_can_go_back_to_active() {
        assert!(can_transition(Deprecated, Active));
    }

    /// **`Draft → Retired` is not a shortcut.** An asset that was never active
    /// has nothing to retire from, and permitting it would make "retired" mean
    /// both "we turned it off" and "we abandoned it before it started" — which
    /// a consumer cannot tell apart.
    #[test]
    fn draft_cannot_skip_straight_to_retired_or_deprecated() {
        assert!(!can_transition(Draft, Retired));
        assert!(!can_transition(Draft, Deprecated));
    }

    /// **Retired is terminal.** This is the assertion an always-permit mutation
    /// fails, and the one that stops a retired asset quietly coming back.
    #[test]
    fn retired_is_terminal() {
        for to in LifecycleState::all() {
            assert!(
                !can_transition(Retired, *to),
                "retired must not move to {to:?}"
            );
        }
    }

    /// A state cannot transition to itself: a no-op that bumped a version and
    /// emitted an event would put noise in every history.
    #[test]
    fn a_state_does_not_transition_to_itself() {
        for state in LifecycleState::all() {
            assert!(!can_transition(*state, *state), "{state:?}");
        }
    }

    /// Active cannot jump to Retired — deprecation is the notice period, and
    /// skipping it removes the only warning consumers get.
    #[test]
    fn active_cannot_skip_deprecation() {
        assert!(!can_transition(Active, Retired));
    }

    // ---- certification status, computed ----

    fn at(days: i64) -> DateTime<Utc> {
        Utc::now() + Duration::days(days)
    }

    #[test]
    fn a_far_off_expiry_is_valid() {
        assert_eq!(
            certification_status(Some(at(90)), Utc::now(), DEFAULT_EXPIRY_WINDOW_DAYS),
            CertificationStatus::Valid
        );
    }

    #[test]
    fn an_expiry_inside_the_window_reports_the_days_left() {
        let status = certification_status(Some(at(10)), Utc::now(), DEFAULT_EXPIRY_WINDOW_DAYS);

        match status {
            CertificationStatus::ExpiringSoon { days_remaining } => {
                assert_eq!(days_remaining, 10, "the number is the actionable part");
            }
            other => panic!("expected ExpiringSoon, got {other:?}"),
        }
    }

    /// **The boundary, at exactly the expiry instant.** "Expires at noon"
    /// universally means it is no good at noon, and a certification valid
    /// *through* its own expiry is a distinction nobody can act on.
    #[test]
    fn at_exactly_the_expiry_instant_it_is_expired() {
        let expiry = Utc::now();

        assert_eq!(
            certification_status(Some(expiry), expiry, DEFAULT_EXPIRY_WINDOW_DAYS),
            CertificationStatus::Expired
        );
    }

    #[test]
    fn past_the_expiry_it_is_expired() {
        assert_eq!(
            certification_status(Some(at(-1)), Utc::now(), DEFAULT_EXPIRY_WINDOW_DAYS),
            CertificationStatus::Expired
        );
    }

    /// **Never certified is not the same as expired.** One was never vouched
    /// for; the other was and no longer is, and a consumer should treat those
    /// differently.
    #[test]
    fn no_certification_is_none_not_expired() {
        assert_eq!(
            certification_status(None, Utc::now(), DEFAULT_EXPIRY_WINDOW_DAYS),
            CertificationStatus::None
        );
    }

    /// **The status changes with the clock and nothing else.** This is the
    /// property a stored status cannot have, and the reason the whole thing is
    /// computed: same input, two different instants, two different answers.
    #[test]
    fn the_same_certification_reads_differently_as_the_clock_advances() {
        let expiry = Utc::now() + Duration::days(10);

        assert_eq!(
            certification_status(Some(expiry), Utc::now(), DEFAULT_EXPIRY_WINDOW_DAYS),
            CertificationStatus::ExpiringSoon { days_remaining: 10 }
        );
        assert_eq!(
            certification_status(
                Some(expiry),
                expiry + Duration::days(1),
                DEFAULT_EXPIRY_WINDOW_DAYS
            ),
            CertificationStatus::Expired,
            "nothing was written; only time passed"
        );
    }

    /// A narrower window moves the boundary, or the parameter would be
    /// decoration.
    #[test]
    fn the_warning_window_is_configurable() {
        let expiry = at(10);

        assert_eq!(
            certification_status(Some(expiry), Utc::now(), 5),
            CertificationStatus::Valid,
            "ten days out is not soon under a five-day window"
        );
        assert!(matches!(
            certification_status(Some(expiry), Utc::now(), 30),
            CertificationStatus::ExpiringSoon { .. }
        ));
    }

    /// Rounded up, so something with hours left never reports zero days — which
    /// would read as expired.
    #[test]
    fn a_part_day_remaining_rounds_up_rather_than_to_zero() {
        let expiry = Utc::now() + Duration::hours(6);

        match certification_status(Some(expiry), Utc::now(), DEFAULT_EXPIRY_WINDOW_DAYS) {
            CertificationStatus::ExpiringSoon { days_remaining } => {
                assert_eq!(days_remaining, 1);
            }
            other => panic!("expected ExpiringSoon, got {other:?}"),
        }
    }

    // ---- several certifications ----

    /// **The best wins.** An expired Gold beside a valid Finance-Approved is
    /// not an expired asset — "is this certified" is answered yes by any valid
    /// one.
    #[test]
    fn the_strongest_certification_decides_the_summary() {
        assert_eq!(
            best_status(&[
                CertificationStatus::Expired,
                CertificationStatus::Valid,
                CertificationStatus::None
            ]),
            CertificationStatus::Valid
        );
        assert_eq!(
            best_status(&[
                CertificationStatus::Expired,
                CertificationStatus::ExpiringSoon { days_remaining: 3 }
            ]),
            CertificationStatus::ExpiringSoon { days_remaining: 3 }
        );
    }

    #[test]
    fn no_certifications_at_all_is_none() {
        assert_eq!(best_status(&[]), CertificationStatus::None);
    }

    // ---- evidence and issuers ----

    /// **Named, not counted.** "Evidence is missing" tells an issuer nothing;
    /// the list tells them what to go and get.
    #[test]
    fn missing_evidence_is_named() {
        let required = vec!["qualityTests".to_string(), "ownerConfirmed".to_string()];
        let supplied = vec!["qualityTests".to_string()];

        assert_eq!(
            missing_evidence(&required, &supplied),
            vec!["ownerConfirmed"]
        );
    }

    /// And the negative: complete evidence blocks nothing, or certification
    /// would be impossible rather than guarded.
    #[test]
    fn complete_evidence_is_missing_nothing() {
        let required = vec!["qualityTests".to_string()];

        assert!(missing_evidence(&required, &required).is_empty());
        assert!(missing_evidence(&[], &[]).is_empty());
    }

    /// Extra evidence is not an error — an issuer who attached more than was
    /// asked for has done nothing wrong.
    #[test]
    fn extra_evidence_is_not_a_problem() {
        let required = vec!["qualityTests".to_string()];
        let supplied = vec!["qualityTests".to_string(), "soc2".to_string()];

        assert!(missing_evidence(&required, &supplied).is_empty());
    }

    /// **An empty allowlist means anyone**, deliberately: a type nobody has
    /// restricted is one the organization has not decided about, and refusing
    /// every issuance would make defining a type useless until somebody also
    /// configured issuers.
    #[test]
    fn an_unrestricted_type_may_be_issued_by_anyone() {
        assert!(may_issue(&[], "asha"));
    }

    /// And a restricted one may not — the assertion an ignored allowlist fails.
    #[test]
    fn a_restricted_type_refuses_anyone_not_named() {
        let issuers = vec!["asha".to_string(), "data-governance".to_string()];

        assert!(may_issue(&issuers, "asha"));
        assert!(may_issue(&issuers, "data-governance"));
        assert!(!may_issue(&issuers, "someone-else"));
    }

    // ---- wire shapes ----

    #[test]
    fn a_status_is_camel_case_and_tagged_on_the_wire() {
        let json = serde_json::to_value(CertificationStatus::ExpiringSoon { days_remaining: 3 })
            .expect("serialize");

        assert_eq!(json["status"], "expiringSoon");
        assert!(json.get("daysRemaining").is_some(), "{json}");
        assert!(json.get("days_remaining").is_none(), "{json}");
    }

    #[test]
    fn a_deprecation_is_camel_case_on_the_wire() {
        let json = serde_json::to_value(Deprecation {
            reason: "superseded".to_string(),
            successor_fqn: Some("svc.db.public.orders_v2".to_string()),
            deprecated_at: Utc::now(),
            sunset_at: None,
        })
        .expect("serialize");

        assert!(json.get("successorFqn").is_some(), "{json}");
        assert!(json.get("deprecatedAt").is_some(), "{json}");
        assert!(json.get("successor_fqn").is_none(), "{json}");
    }

    #[test]
    fn lifecycle_states_round_trip_through_their_wire_names() {
        for state in LifecycleState::all() {
            assert_eq!(LifecycleState::parse(state.as_str()), Ok(*state));
        }
        assert!(LifecycleState::parse("mothballed").is_err());
    }
}
