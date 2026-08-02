//! The extraction submission pipeline — Epic 21, the half that makes the
//! ports real.
//!
//! **This is the surface an out-of-process worker submits to**, and it exists
//! because without it decision 0's "the worker runs in Python" is a claim with
//! nowhere to send. A worker parses a PDF, runs OCR, prompts a model — none of
//! which graph-owl does — and then hands over an [`ExtractionResult`] as JSON.
//! Everything that decides what that result *buys* runs here, on this side of
//! the process boundary.
//!
//! That placement is the whole security property. A worker proposes a
//! confidence; [`Disposition::for_confidence`] decides what it means. A worker
//! proposes a predicate; [`constrain`] decides whether it exists. A worker
//! re-proposes something a human already rejected; [`submit`] drops it. None
//! of those checks are available to a compromised or merely mis-tuned worker,
//! because none of them are in the worker.

use std::collections::HashSet;

use graph_owl_connectors::extraction::constrain;
use graph_owl_core::extraction::{Claim, Disposition, ExtractionResult, ParsedDocument};
use graph_owl_core::extraction_run::content_fingerprint;
use graph_owl_storage::{
    DiscardedClaimRecord, ExtractionRunRecord, QueuedClaimRecord, StorageError,
};
use uuid::Uuid;

/// The predicates an extracted claim may use.
///
/// **Owned by Epic 1's model, written down once.** A worker naming anything
/// else is proposing a fact the catalog has no way to store or query, which is
/// decision 1's whole point: open information extraction produces a graph
/// nothing can ask a question of.
pub const CATALOG_PREDICATES: &[&str] = &[
    "description",
    "owner",
    "tag",
    "term",
    "feeds",
    "derivedFrom",
    "dependsOn",
];

/// What a submitted run did.
///
/// **`rename_all_fields` is not redundant with `rename_all`.** On an enum,
/// `rename_all` renames the *variants*; the fields inside them keep their Rust
/// spelling unless `rename_all_fields` says otherwise. Without it this ships
/// `run_id` beside a wire full of camelCase, and no domain or repository test
/// would notice — they compare Rust values and database columns, never the
/// serialized bytes. This project has now shipped that exact bug twice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "outcome"
)]
pub enum SubmissionOutcome {
    /// The run was processed.
    Recorded {
        run_id: Uuid,
        asserted: usize,
        surfaced: usize,
        discarded: usize,
    },
    /// This exact document has already been through this exact extractor, so
    /// nothing was done.
    ///
    /// **A distinct outcome rather than a silent no-op**, because a worker
    /// re-submitting is normal (at-least-once delivery, a retried job, an
    /// operator re-running a backfill) and it should be able to tell that
    /// graph-owl recognised the document rather than found nothing in it.
    AlreadyExtracted { run_id: Uuid },
}

/// A queued claim as a reviewer sees it.
///
/// **Carries the evidence text, not just the span.** Decision 5 made usable:
/// a reviewer shown "svc.db.orders description append-only" with no sentence
/// behind it is being asked to trust the extractor, which is the thing under
/// review. The span alone would put the burden of resolving it on every client.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingClaim {
    pub id: Uuid,
    pub run_id: Uuid,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f64,
    pub evidence: String,
}

/// The states a queued claim can be in.
///
/// `Asserted` is separate from `Confirmed` on purpose: one means the
/// confidence band was high enough that no human was asked, the other means a
/// human was asked and said yes. Collapsing them would record every
/// machine-asserted claim as human-reviewed, which is precisely the
/// provenance lie this epic exists to avoid.
pub const STATE_ASSERTED: &str = "asserted";
pub const STATE_PENDING: &str = "pending";

/// How many pending claims one page of the review queue returns.
///
/// A reviewer works through a queue a screenful at a time; a request that
/// returned every pending claim in a large backfill would be a slow query
/// answering a question nobody asked.
pub const QUEUE_PAGE: i64 = 100;

/// Apply the policy to a worker's output and record what it bought.
///
/// The order matters, and each step earns its place:
///
/// 1. **Idempotence first**, so a re-submitted document costs one indexed read
///    rather than a full re-evaluation and a duplicate run.
/// 2. **Vocabulary and subject**, because a claim about an unknown entity or
///    with an unknown predicate cannot be stored no matter how confident the
///    worker is — confidence is not the failing.
/// 3. **Human rejections**, because a reviewer who said no should not be asked
///    the same question by the next run of the same extractor.
/// 4. **Confidence bands last**, on what survived.
///
/// # Errors
///
/// [`StorageError`] if the run cannot be read or written.
pub async fn submit(
    storage: &dyn graph_owl_storage::Storage,
    document: &ParsedDocument,
    result: ExtractionResult,
    extractor: &str,
    version: &str,
    // **`+ Sync` is load-bearing, not decoration.** A bare `&dyn Fn` makes
    // this future non-`Send`, and axum only accepts `Send` handlers — so the
    // omission surfaces as "`submit_extraction` does not implement `Handler`"
    // at the route, three layers away from the cause.
    known_subject: &(dyn Fn(&str) -> bool + Sync),
) -> Result<SubmissionOutcome, StorageError> {
    let fingerprint = content_fingerprint(document.text.as_bytes());

    if let Some(previous) = storage
        .find_extraction_run(&document.source_id, &fingerprint, extractor, version)
        .await?
    {
        return Ok(SubmissionOutcome::AlreadyExtracted {
            run_id: previous.id,
        });
    }

    let constrained = constrain(result, CATALOG_PREDICATES, known_subject);

    let rejected: HashSet<(String, String, String)> =
        storage.rejected_assertions().await?.into_iter().collect();
    let (survived, previously_rejected) = split_rejected(constrained, &rejected);

    let (assert, surface, mut discarded) = survived.partition();
    discarded.extend(previously_rejected);

    let run_id = Uuid::new_v4();
    let run = ExtractionRunRecord {
        id: run_id,
        source_id: document.source_id.clone(),
        source_fingerprint: fingerprint,
        extractor: extractor.to_string(),
        extractor_version: version.to_string(),
        source_text: document.text.clone(),
        media_type: document.media_type.clone(),
        asserted: i32::try_from(assert.len()).unwrap_or(i32::MAX),
        surfaced: i32::try_from(surface.len()).unwrap_or(i32::MAX),
        discarded: i32::try_from(discarded.len()).unwrap_or(i32::MAX),
    };

    let queued: Vec<QueuedClaimRecord> = assert
        .iter()
        .map(|claim| record(run_id, claim, STATE_ASSERTED))
        .chain(
            surface
                .iter()
                .map(|claim| record(run_id, claim, STATE_PENDING)),
        )
        .collect();

    let discards: Vec<DiscardedClaimRecord> = discarded
        .iter()
        .map(|discard| DiscardedClaimRecord {
            id: Uuid::new_v4(),
            run_id,
            subject: discard.claim.subject.clone(),
            predicate: discard.claim.predicate.clone(),
            object: discard.claim.object.clone(),
            confidence: discard.claim.confidence,
            reason: discard.reason.clone(),
        })
        .collect();

    storage
        .save_extraction_run(&run, &queued, &discards)
        .await?;

    Ok(SubmissionOutcome::Recorded {
        run_id,
        asserted: assert.len(),
        surfaced: surface.len(),
        discarded: discarded.len(),
    })
}

/// Moves claims a human already rejected out of the result, with a reason.
///
/// **Matched on the assertion, never on the run.** The submission that would
/// re-propose a rejected claim is by definition a later run with a different
/// id, so matching on the run would make this never fire — and a reviewer
/// would answer the same question on every re-ingestion forever.
fn split_rejected(
    result: ExtractionResult,
    rejected: &HashSet<(String, String, String)>,
) -> (
    ExtractionResult,
    Vec<graph_owl_core::extraction::DiscardedClaim>,
) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for claim in result.claims {
        let key = (
            claim.subject.clone(),
            claim.predicate.clone(),
            claim.object.clone(),
        );
        if rejected.contains(&key) {
            dropped.push(graph_owl_core::extraction::DiscardedClaim {
                claim,
                reason: "a reviewer already rejected this assertion".to_string(),
            });
        } else {
            kept.push(claim);
        }
    }
    (
        ExtractionResult {
            claims: kept,
            discarded: result.discarded,
        },
        dropped,
    )
}

fn record(run_id: Uuid, claim: &Claim, state: &str) -> QueuedClaimRecord {
    QueuedClaimRecord {
        id: Uuid::new_v4(),
        run_id,
        subject: claim.subject.clone(),
        predicate: claim.predicate.clone(),
        object: claim.object.clone(),
        confidence: claim.confidence,
        evidence_start: i32::try_from(claim.provenance.evidence.start).unwrap_or(i32::MAX),
        evidence_end: i32::try_from(claim.provenance.evidence.end).unwrap_or(i32::MAX),
        state: state.to_string(),
        decided_by: None,
    }
}

/// The confidence a confirmed claim carries.
///
/// A human said yes, so the machine's uncertainty no longer applies — the
/// acceptance criteria call for exactly this, and leaving the extractor's 0.6
/// on a reviewed claim would make human confirmation invisible to anything
/// that later filters on confidence.
pub const CONFIRMED_CONFIDENCE: f64 = 1.0;

/// Whether a disposition means the claim reaches the graph without a human.
#[must_use]
pub fn enters_graph_unreviewed(confidence: f64) -> bool {
    Disposition::for_confidence(confidence) == Disposition::Assert
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::extraction::{Provenance, TextSpan};

    fn claim(subject: &str, predicate: &str, confidence: f64) -> Claim {
        Claim {
            subject: subject.to_string(),
            predicate: predicate.to_string(),
            object: "append-only".to_string(),
            confidence,
            provenance: Provenance {
                source_id: "runbook.md".to_string(),
                extractor: "markdown-rules".to_string(),
                extractor_version: "1".to_string(),
                extracted_at: chrono::Utc::now(),
                evidence: TextSpan::new(0, 4),
            },
        }
    }

    fn rejected(entries: &[(&str, &str, &str)]) -> HashSet<(String, String, String)> {
        entries
            .iter()
            .map(|(s, p, o)| ((*s).to_string(), (*p).to_string(), (*o).to_string()))
            .collect()
    }

    /// **The core of the rejection-persistence criterion.** A reviewer said no;
    /// re-ingesting the document must not ask again.
    #[test]
    fn a_previously_rejected_assertion_is_dropped_with_a_reason() {
        let result = ExtractionResult {
            claims: vec![claim("svc.db.orders", "description", 0.6)],
            discarded: Vec::new(),
        };

        let (kept, dropped) = split_rejected(
            result,
            &rejected(&[("svc.db.orders", "description", "append-only")]),
        );

        assert!(kept.claims.is_empty(), "{:?}", kept.claims);
        assert_eq!(dropped.len(), 1);
        assert!(
            dropped[0].reason.contains("already rejected"),
            "{}",
            dropped[0].reason
        );
    }

    /// **The negative case, which is what a mutation would survive without.**
    /// Rejecting one assertion must not suppress a *different* one — an
    /// over-broad match would silently stop the extractor ever proposing
    /// anything about that entity again.
    #[test]
    fn rejecting_one_assertion_does_not_suppress_a_different_one() {
        let result = ExtractionResult {
            claims: vec![
                claim("svc.db.orders", "description", 0.6),
                claim("svc.db.payments", "description", 0.6),
            ],
            discarded: Vec::new(),
        };

        let (kept, dropped) = split_rejected(
            result,
            &rejected(&[("svc.db.orders", "description", "append-only")]),
        );

        assert_eq!(kept.claims.len(), 1);
        assert_eq!(kept.claims[0].subject, "svc.db.payments");
        assert_eq!(dropped.len(), 1);
    }

    /// A rejection is of the whole assertion, so the same subject and predicate
    /// with a *different object* is a new question and must still be asked.
    #[test]
    fn a_rejection_does_not_suppress_a_different_object() {
        let mut different = claim("svc.db.orders", "description", 0.6);
        different.object = "partitioned by day".to_string();
        let result = ExtractionResult {
            claims: vec![different],
            discarded: Vec::new(),
        };

        let (kept, dropped) = split_rejected(
            result,
            &rejected(&[("svc.db.orders", "description", "append-only")]),
        );

        assert_eq!(kept.claims.len(), 1, "a new object is a new question");
        assert!(dropped.is_empty());
    }

    /// Discards the constraint step already recorded survive this one — a
    /// filter that rebuilt the list would lose every reason `constrain` wrote.
    #[test]
    fn earlier_discards_are_carried_through() {
        let result = ExtractionResult {
            claims: Vec::new(),
            discarded: vec![graph_owl_core::extraction::DiscardedClaim {
                claim: claim("svc.db.orders", "isFriendsWith", 0.9),
                reason: "predicate is not in the vocabulary".to_string(),
            }],
        };

        let (kept, _) = split_rejected(result, &rejected(&[]));

        assert_eq!(kept.discarded.len(), 1);
    }

    /// The vocabulary is the constraint decision 1 rests on, so an empty one
    /// would silently make every claim off-ontology.
    #[test]
    fn the_catalog_vocabulary_is_not_empty_and_holds_the_epic_1_predicates() {
        assert!(CATALOG_PREDICATES.contains(&"description"));
        assert!(CATALOG_PREDICATES.contains(&"owner"));
        assert!(
            !CATALOG_PREDICATES.contains(&"isFriendsWith"),
            "an open vocabulary is exactly what decision 1 refuses"
        );
    }

    /// **The band decides, not the worker.** A claim at the surface band must
    /// not reach the graph however sure the extractor says it is.
    #[test]
    fn only_the_assert_band_reaches_the_graph_unreviewed() {
        assert!(enters_graph_unreviewed(0.8), "0.8 asserts");
        assert!(!enters_graph_unreviewed(0.79), "just below does not");
        assert!(!enters_graph_unreviewed(0.6), "the surface band waits");
        assert!(
            !enters_graph_unreviewed(f64::NAN),
            "a broken worker does not"
        );
    }

    /// **The assertion that would have caught the `run_id` bug here rather than
    /// three layers up.**
    ///
    /// `rename_all` on an enum renames its *variants*, not the fields inside
    /// them, so this shipped `run_id` next to a wire of camelCase. Nothing
    /// below HTTP noticed: the domain tests compare Rust values and the
    /// repository tests compare columns, and neither ever looks at the bytes.
    /// Any type whose wire shape matters needs one assertion against the
    /// serialized JSON.
    #[test]
    fn the_submission_outcome_is_camel_case_on_the_wire() {
        let recorded = serde_json::to_value(SubmissionOutcome::Recorded {
            run_id: Uuid::nil(),
            asserted: 1,
            surfaced: 2,
            discarded: 3,
        })
        .expect("serialize");

        assert_eq!(recorded["outcome"], "recorded");
        assert!(recorded.get("runId").is_some(), "{recorded}");
        assert!(
            recorded.get("run_id").is_none(),
            "a snake_case key beside camelCase ones: {recorded}"
        );

        let already = serde_json::to_value(SubmissionOutcome::AlreadyExtracted {
            run_id: Uuid::nil(),
        })
        .expect("serialize");
        assert_eq!(already["outcome"], "alreadyExtracted");
        assert!(already.get("runId").is_some(), "{already}");
    }

    /// A pending claim crosses the same boundary and needs the same guard.
    #[test]
    fn a_pending_claim_is_camel_case_on_the_wire() {
        let pending = serde_json::to_value(PendingClaim {
            id: Uuid::nil(),
            run_id: Uuid::nil(),
            subject: "svc.db.orders".to_string(),
            predicate: "description".to_string(),
            object: "append-only".to_string(),
            confidence: 0.6,
            evidence: "the orders table is append-only".to_string(),
        })
        .expect("serialize");

        assert!(pending.get("runId").is_some(), "{pending}");
        assert!(pending.get("run_id").is_none(), "{pending}");
    }

    #[test]
    fn a_confirmed_claim_carries_full_confidence() {
        assert!((CONFIRMED_CONFIDENCE - 1.0).abs() < f64::EPSILON);
    }

    /// Asserted and confirmed are different facts about a claim, and a queue
    /// that used one string for both would record every machine assertion as
    /// human-reviewed.
    #[test]
    fn asserted_and_pending_are_distinct_states() {
        assert_ne!(STATE_ASSERTED, STATE_PENDING);
        assert_ne!(STATE_ASSERTED, "confirmed");
    }
}
