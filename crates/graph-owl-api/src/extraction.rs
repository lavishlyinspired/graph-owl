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

use std::collections::{HashMap, HashSet};

use graph_owl_connectors::extraction::constrain;
use graph_owl_core::extraction::{Claim, Disposition, ExtractionResult, ParsedDocument};
use graph_owl_core::extraction_run::{EXTRACTION_GRAPH, content_fingerprint};
use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use graph_owl_core::projection::entity_sid;
use graph_owl_storage::{
    DiscardedClaimRecord, ExtractionRunRecord, QueuedClaimRecord, StorageError,
};
use uuid::Uuid;

/// Which mention text resolved to which catalog entity.
///
/// **The one identity map extraction uses**, built by Epic 17's resolver and
/// passed in whole. A worker's `subject` is a string it read in a document;
/// what the catalog stores a fact against is an entity id, and the step
/// between them is resolution — not string equality against a fully-qualified
/// name, which is what silently produces a second logical copy of a table the
/// catalog already has.
pub type ResolvedMentions = HashMap<String, Uuid>;

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

/// What a catalog predicate's object *is*.
///
/// Extraction produces prose on both sides of a claim, and the graph does not:
/// `dsc:feeds` is a reference, indexed in OPST so that "what feeds this table"
/// is answerable by reverse traversal. Writing the string `"the orders table"`
/// into that position type-checks, stores, and makes the edge unreachable from
/// the end that needed it — a lineage graph that quietly loses edges, which is
/// the one thing a lineage graph must never do.
///
/// So the shape decides how a claim projects, and a reference-shaped claim
/// whose object does not resolve to an entity is discarded with a reason
/// rather than written as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectShape {
    /// The object is the value: `description`, `tag`.
    Literal,
    /// The object names another entity: `feeds`, `derivedFrom`, `dependsOn`,
    /// `owner`, `term`.
    Reference,
}

/// The shape of `predicate`'s object, or `None` if it is not a catalog
/// predicate at all.
#[must_use]
pub fn object_shape(predicate: &str) -> Option<ObjectShape> {
    match predicate {
        "description" | "tag" => Some(ObjectShape::Literal),
        "owner" | "term" | "feeds" | "derivedFrom" | "dependsOn" => Some(ObjectShape::Reference),
        _ => None,
    }
}

/// The named graph every extracted fact is written to, as a subject id.
#[must_use]
pub fn extraction_graph() -> Sid {
    Sid::dsc(EXTRACTION_GRAPH)
}

/// The flake one claim contributes to `graph:extraction`.
///
/// **Never the default graph** (decision 2). Epic 6's reasoning reads the
/// default graph and whatever named graphs a run was told to include, so a
/// claim landing here is invisible to inference until a deployment opts in —
/// which is the whole containment property, and it only holds if every write
/// carries the `cx`.
///
/// `None` when the predicate is outside the vocabulary, or when it is
/// reference-shaped and `object_entity` is absent. Both are refusals to write
/// something the graph cannot represent honestly, not errors.
#[must_use]
pub fn claim_flake(
    subject: Uuid,
    predicate: &str,
    object: &str,
    object_entity: Option<Uuid>,
    t: i64,
) -> Option<Flake> {
    let value = match object_shape(predicate)? {
        ObjectShape::Literal => FlakeValue::String(object.to_string()),
        ObjectShape::Reference => FlakeValue::Ref(entity_sid(object_entity?)),
    };
    Some(Flake {
        s: entity_sid(subject),
        p: Sid::dsc(predicate),
        o: value,
        cx: Some(extraction_graph()),
        t,
        op: true,
    })
}

/// Every flake a batch of claims contributes, skipping the ones that cannot
/// be represented.
///
/// Driven by the **persisted** claim records rather than the in-memory result,
/// so the confirmation path and the submission path share one projection: a
/// human saying yes writes exactly the flake a confident extractor would have.
#[must_use]
pub fn claim_flakes(
    claims: &[QueuedClaimRecord],
    resolved: &ResolvedMentions,
    t: i64,
) -> Vec<Flake> {
    claims
        .iter()
        .filter_map(|claim| {
            claim_flake(
                *resolved.get(&claim.subject)?,
                &claim.predicate,
                &claim.object,
                resolved.get(&claim.object).copied(),
                t,
            )
        })
        .collect()
}

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
        /// The run's stable identifier.
        run_id: Uuid,
        /// How many claims were written without review.
        asserted: usize,
        /// How many claims were queued for review.
        surfaced: usize,
        /// How many claims were discarded, with a reason each.
        discarded: usize,
    },
    /// This exact document has already been through this exact extractor, so
    /// nothing was done.
    ///
    /// **A distinct outcome rather than a silent no-op**, because a worker
    /// re-submitting is normal (at-least-once delivery, a retried job, an
    /// operator re-running a backfill) and it should be able to tell that
    /// graph-owl recognised the document rather than found nothing in it.
    AlreadyExtracted {
        /// The id of the run that already processed this document.
        run_id: Uuid,
    },
}

/// A queued claim as a reviewer sees it.
///
/// **Carries the surrounding sentence, not just the span.** Decision 5 made
/// usable, then widened for Epic 42 Slice D: a reviewer shown
/// "svc.db.orders description append-only" with no sentence behind it is
/// being asked to trust the extractor, which is the thing under review, and
/// the bare matched phrase alone is not enough context to judge it in. The
/// span alone would put the burden of resolving it on every client.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingClaim {
    /// The stable identifier.
    pub id: Uuid,
    /// The run this claim was extracted by.
    pub run_id: Uuid,
    /// The subject entity, as text.
    pub subject: String,
    /// The predicate, from [`CATALOG_PREDICATES`].
    pub predicate: String,
    /// The object, as text.
    pub object: String,
    /// The extractor's confidence.
    pub confidence: f64,
    /// The sentence this claim was extracted from — Epic 42 Slice D's
    /// "source passage with the extracted span highlighted". Reviewing "X"
    /// with no sentence behind it is guessing, and a reviewer who is
    /// guessing approves everything, which launders machine output as
    /// human-verified.
    pub passage: String,
    /// The extracted span's `(start, end)` byte offsets into `passage`.
    pub span: (usize, usize),
}

/// The states a queued claim can be in.
///
/// `Asserted` is separate from `Confirmed` on purpose: one means the
/// confidence band was high enough that no human was asked, the other means a
/// human was asked and said yes. Collapsing them would record every
/// machine-asserted claim as human-reviewed, which is precisely the
/// provenance lie this epic exists to avoid.
pub const STATE_ASSERTED: &str = "asserted";
/// A human was asked and has not yet answered.
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
/// 3. **Reference objects**, by the same rule applied to the other endpoint:
///    both ends of a reference must be entities the catalog knows.
/// 4. **Human rejections**, because a reviewer who said no should not be asked
///    the same question by the next run of the same extractor.
/// 5. **Confidence bands last**, on what survived.
///
/// Returns the outcome and the records written in the **asserted** state —
/// what the caller projects into `graph:extraction`. Returning them rather
/// than making the caller read them back is what keeps the projection driven
/// by the rows that were actually stored.
///
/// # Errors
///
/// [`StorageError`] if the run cannot be read or written.
pub async fn submit(
    storage: &dyn graph_owl_storage::Storage,
    run_id: Uuid,
    document: &ParsedDocument,
    result: ExtractionResult,
    extractor: &str,
    version: &str,
    resolved: &ResolvedMentions,
) -> Result<(SubmissionOutcome, Vec<QueuedClaimRecord>), StorageError> {
    let fingerprint = content_fingerprint(document.text.as_bytes());

    if let Some(previous) = storage
        .find_extraction_run(&document.source_id, &fingerprint, extractor, version)
        .await?
    {
        return Ok((
            SubmissionOutcome::AlreadyExtracted {
                run_id: previous.id,
            },
            Vec::new(),
        ));
    }

    // **`+ Sync` on the closure is load-bearing, not decoration.** A bare
    // `&dyn Fn` makes this future non-`Send`, and axum only accepts `Send`
    // handlers — so the omission surfaces as "`submit_extraction` does not
    // implement `Handler`" at the route, three layers away from the cause.
    let known_subject = |subject: &str| resolved.contains_key(subject);
    let constrained = constrain(result, CATALOG_PREDICATES, &known_subject);
    let (constrained, unresolved_objects) = split_unresolved_objects(constrained, resolved);

    let rejected: HashSet<(String, String, String)> =
        storage.rejected_assertions().await?.into_iter().collect();
    let (survived, previously_rejected) = split_rejected(constrained, &rejected);

    let (assert, surface, mut discarded) = survived.partition();
    discarded.extend(unresolved_objects);
    discarded.extend(previously_rejected);

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

    let asserted: Vec<QueuedClaimRecord> = assert
        .iter()
        .map(|claim| record(run_id, claim, STATE_ASSERTED))
        .collect();
    let queued: Vec<QueuedClaimRecord> = asserted
        .iter()
        .cloned()
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

    Ok((
        SubmissionOutcome::Recorded {
            run_id,
            asserted: assert.len(),
            surfaced: surface.len(),
            discarded: discarded.len(),
        },
        asserted,
    ))
}

/// Moves reference-shaped claims whose object named nothing the catalog knows.
///
/// **The subject rule, applied to the other endpoint.** `constrain` already
/// refuses a claim about an unknown entity; a `feeds` claim whose *object* is
/// unknown has the same defect from the other side — there is no edge to draw,
/// only half of one. Discarding it with a reason is what makes the gap
/// diagnosable from the run's own output.
fn split_unresolved_objects(
    result: ExtractionResult,
    resolved: &ResolvedMentions,
) -> (
    ExtractionResult,
    Vec<graph_owl_core::extraction::DiscardedClaim>,
) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for claim in result.claims {
        let references = object_shape(&claim.predicate) == Some(ObjectShape::Reference);
        if references && !resolved.contains_key(&claim.object) {
            let reason = format!(
                "`{}` names another entity, and `{}` did not resolve to one",
                claim.predicate, claim.object
            );
            dropped.push(graph_owl_core::extraction::DiscardedClaim { claim, reason });
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
        reason: None,
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

/// Scanned no further than this many characters from the span in either
/// direction when widening to a sentence — Epic 21 x Epic 42 Slice D.
/// Unpunctuated text (a transcript, a log dump) has no sentence terminator
/// to stop at, and without a cap that would return the entire document as
/// "context" for one claim. 400 holds a few genuine sentences comfortably
/// (English averages roughly 15-25 words, ~100-150 characters, per
/// sentence) without risking a multi-KB payload per claim in a queue that
/// may list dozens of them at once.
const PASSAGE_SCAN_CAP: usize = 400;

fn is_sentence_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '\n')
}

/// An extracted span widened to the sentence around it, so a reviewer sees
/// what the extractor saw rather than an isolated phrase with the sentence
/// it was cut from stripped away.
///
/// `span` is a byte range into `passage` (not into the original source), so
/// a caller highlights it by slicing `passage` directly rather than
/// re-deriving an offset.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceWindow {
    /// The widened sentence (or as much of it as `PASSAGE_SCAN_CAP` allows).
    pub passage: String,
    /// The extracted span's `(start, end)` byte offsets into `passage`.
    pub span: (usize, usize),
}

/// Widens `source[start..end]` to the sentence it sits in — Epic 21 x Epic
/// 42 Slice D's "source passage with the extracted span highlighted".
/// `None` when the span itself does not resolve (out of bounds, reversed,
/// or landing mid-character) — the same cases the bare-span lookup this
/// replaces already refused to answer.
#[must_use]
pub fn windowed_passage(source: &str, start: usize, end: usize) -> Option<EvidenceWindow> {
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }

    let scan_start = source[..start]
        .char_indices()
        .rev()
        .nth(PASSAGE_SCAN_CAP.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    let passage_start = source[scan_start..start]
        .char_indices()
        .rev()
        .find(|&(_, c)| is_sentence_terminator(c))
        .map_or(scan_start, |(i, c)| scan_start + i + c.len_utf8());
    // A terminator is followed by whitespace before the next sentence
    // starts — included in the scan so it can be excluded from the passage.
    // No non-whitespace found means the gap is whitespace all the way to
    // `start` (or there is no gap at all), so the correct trim target is
    // `start` itself, not the untrimmed `passage_start`.
    let passage_start = source[passage_start..start]
        .find(|c: char| !c.is_whitespace())
        .map_or(start, |offset| passage_start + offset);

    let scan_end = source[end..]
        .char_indices()
        .nth(PASSAGE_SCAN_CAP.saturating_sub(1))
        .map_or(source.len(), |(i, c)| end + i + c.len_utf8());
    let passage_end = source[end..scan_end]
        .char_indices()
        .find(|&(_, c)| is_sentence_terminator(c))
        .map_or(scan_end, |(i, c)| end + i + c.len_utf8());

    Some(EvidenceWindow {
        passage: source[passage_start..passage_end].to_string(),
        span: (start - passage_start, end - passage_start),
    })
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
            passage: "the orders table is append-only".to_string(),
            span: (4, 9),
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

    // ── the projection into `graph:extraction` ──────────────────────────────

    fn resolved(pairs: &[(&str, Uuid)]) -> ResolvedMentions {
        pairs
            .iter()
            .map(|(text, id)| ((*text).to_string(), *id))
            .collect()
    }

    fn asserted_record(subject: &str, predicate: &str, object: &str) -> QueuedClaimRecord {
        QueuedClaimRecord {
            id: Uuid::new_v4(),
            run_id: Uuid::nil(),
            subject: subject.to_string(),
            predicate: predicate.to_string(),
            object: object.to_string(),
            confidence: 0.9,
            evidence_start: 0,
            evidence_end: 4,
            state: STATE_ASSERTED.to_string(),
            decided_by: None,
            reason: None,
        }
    }

    /// **The containment property, asserted on the flake itself.** Decision 2
    /// says extracted facts land in `graph:extraction` and never the default
    /// graph, and that is only true if every write carries the `cx` — one
    /// forgotten `None` puts machine output where Epic 6 reasons over it
    /// unconditionally, with nothing failing.
    #[test]
    fn an_extracted_fact_is_stamped_into_the_extraction_graph() {
        let subject = Uuid::new_v4();

        let flake = claim_flake(subject, "description", "append-only", None, 7)
            .expect("a literal claim projects");

        assert_eq!(flake.cx, Some(extraction_graph()));
        assert_ne!(flake.cx, None, "the default graph is the failure mode");
        assert_eq!(flake.s, entity_sid(subject));
        assert_eq!(flake.p, Sid::dsc("description"));
        assert_eq!(flake.o, FlakeValue::String("append-only".to_string()));
        assert_eq!(flake.t, 7);
        assert!(flake.op);
    }

    /// A reference-shaped predicate projects a `Ref`, not the text it was
    /// extracted from. A string in that position stores fine and is unreachable
    /// by reverse traversal, which is how a lineage edge disappears without an
    /// error.
    #[test]
    fn a_reference_claim_projects_the_resolved_entity_not_its_words() {
        let subject = Uuid::new_v4();
        let object = Uuid::new_v4();

        let flake = claim_flake(subject, "feeds", "the payments table", Some(object), 1)
            .expect("a resolved reference claim projects");

        assert_eq!(flake.o, FlakeValue::Ref(entity_sid(object)));
    }

    /// **The negative that a mutation would otherwise survive.** Without the
    /// object requirement, `feeds "the payments table"` projects as a string
    /// and the graph gains an edge to a sentence.
    #[test]
    fn a_reference_claim_with_no_resolved_object_projects_nothing() {
        assert!(
            claim_flake(Uuid::new_v4(), "feeds", "the payments table", None, 1).is_none(),
            "half an edge is not an edge"
        );
    }

    /// And a predicate outside the vocabulary projects nothing even if
    /// everything else about the claim resolved — the vocabulary check is not
    /// something the projection may re-decide.
    #[test]
    fn a_claim_outside_the_vocabulary_projects_nothing() {
        assert!(claim_flake(Uuid::new_v4(), "isFriendsWith", "bob", None, 1).is_none());
    }

    /// The vocabulary and the shape table are one thing said twice, and the
    /// pair drifts the first time a predicate is added to one of them: a
    /// vocabulary entry with no shape is a claim that passes `constrain` and
    /// then silently never projects.
    #[test]
    fn every_catalog_predicate_has_an_object_shape() {
        for predicate in CATALOG_PREDICATES {
            assert!(
                object_shape(predicate).is_some(),
                "`{predicate}` is in the vocabulary with no object shape, so it \
                 would be accepted and then never reach the graph"
            );
        }
        assert!(
            object_shape("isFriendsWith").is_none(),
            "and nothing outside the vocabulary has one"
        );
    }

    /// Lineage predicates are references and prose predicates are literals.
    /// Getting this backwards is not a compile error anywhere.
    #[test]
    fn the_lineage_predicates_are_references_and_description_is_not() {
        assert_eq!(object_shape("description"), Some(ObjectShape::Literal));
        assert_eq!(object_shape("tag"), Some(ObjectShape::Literal));
        assert_eq!(object_shape("feeds"), Some(ObjectShape::Reference));
        assert_eq!(object_shape("derivedFrom"), Some(ObjectShape::Reference));
        assert_eq!(object_shape("dependsOn"), Some(ObjectShape::Reference));
    }

    /// A batch skips what it cannot represent and keeps what it can, rather
    /// than failing the whole run — one unresolvable object must not cost the
    /// other facts in the same document.
    #[test]
    fn a_batch_projects_what_resolves_and_skips_what_does_not() {
        let orders = Uuid::new_v4();
        let map = resolved(&[("svc.db.orders", orders)]);
        let claims = vec![
            asserted_record("svc.db.orders", "description", "append-only"),
            asserted_record("svc.db.orders", "feeds", "nobody has catalogued this"),
        ];

        let flakes = claim_flakes(&claims, &map, 3);

        assert_eq!(flakes.len(), 1, "{flakes:#?}");
        assert_eq!(flakes[0].p, Sid::dsc("description"));
    }

    /// A claim whose *subject* never resolved cannot project either — and this
    /// is the case that would otherwise panic on an unwrap rather than skip.
    #[test]
    fn a_batch_skips_a_claim_whose_subject_did_not_resolve() {
        let claims = vec![asserted_record(
            "svc.db.nobody-knows",
            "description",
            "append-only",
        )];

        assert!(claim_flakes(&claims, &resolved(&[]), 3).is_empty());
    }

    // ── both ends of a reference must be known ──────────────────────────────

    /// **The other half of `constrain`'s subject rule.** A `feeds` claim whose
    /// object resolved to nothing is half an edge, and storing it would leave a
    /// lineage fact that no traversal from either end can reach.
    #[test]
    fn a_reference_claim_with_an_unresolvable_object_is_discarded_with_a_reason() {
        let mut lineage = claim("svc.db.orders", "feeds", 0.9);
        lineage.object = "the payments table".to_string();
        let result = ExtractionResult {
            claims: vec![lineage],
            discarded: Vec::new(),
        };

        let (kept, dropped) =
            split_unresolved_objects(result, &resolved(&[("svc.db.orders", Uuid::new_v4())]));

        assert!(kept.claims.is_empty(), "{:?}", kept.claims);
        assert_eq!(dropped.len(), 1);
        assert!(
            dropped[0].reason.contains("did not resolve"),
            "{}",
            dropped[0].reason
        );
    }

    /// **The negative.** A literal-shaped predicate must not be held to the
    /// same rule — requiring `description "append-only"` to name an entity
    /// would discard every description anyone ever extracted.
    #[test]
    fn a_literal_claim_is_never_asked_to_resolve_its_object() {
        let result = ExtractionResult {
            claims: vec![claim("svc.db.orders", "description", 0.9)],
            discarded: Vec::new(),
        };

        let (kept, dropped) =
            split_unresolved_objects(result, &resolved(&[("svc.db.orders", Uuid::new_v4())]));

        assert_eq!(kept.claims.len(), 1);
        assert!(dropped.is_empty());
    }

    /// And a reference claim whose object *did* resolve survives, or the rule
    /// above would be indistinguishable from "references never work".
    #[test]
    fn a_reference_claim_with_a_resolved_object_survives() {
        let mut lineage = claim("svc.db.orders", "feeds", 0.9);
        lineage.object = "svc.db.payments".to_string();
        let result = ExtractionResult {
            claims: vec![lineage],
            discarded: Vec::new(),
        };

        let (kept, dropped) = split_unresolved_objects(
            result,
            &resolved(&[
                ("svc.db.orders", Uuid::new_v4()),
                ("svc.db.payments", Uuid::new_v4()),
            ]),
        );

        assert_eq!(kept.claims.len(), 1);
        assert!(dropped.is_empty());
    }

    mod windowed_passage_tests {
        use super::super::windowed_passage;

        #[test]
        fn expands_to_the_containing_sentence_not_only_the_matched_phrase() {
            let source = "The orders table is append-only. It never accepts updates. Deletes are also refused.";
            // "append-only" — the extractor's exact match, mid-sentence.
            let start = source.find("append-only").expect("fixture");
            let end = start + "append-only".len();

            let window = windowed_passage(source, start, end).expect("resolves");

            assert_eq!(window.passage, "The orders table is append-only.");
            assert_eq!(&window.passage[window.span.0..window.span.1], "append-only");
        }

        #[test]
        fn stops_at_the_previous_sentence_terminator_not_the_document_start() {
            let source = "Unrelated context here. The table is a fact table. More context follows.";
            let start = source.find("a fact table").expect("fixture");
            let end = start + "a fact table".len();

            let window = windowed_passage(source, start, end).expect("resolves");

            assert!(
                !window.passage.contains("Unrelated"),
                "the passage must not reach back past the prior sentence: {}",
                window.passage
            );
            assert_eq!(
                &window.passage[window.span.0..window.span.1],
                "a fact table"
            );
        }

        #[test]
        fn a_span_at_the_very_start_of_the_document_has_no_left_context_to_include() {
            let source = "Append-only from the first byte. Trailing sentence.";
            let end = "Append-only".len();

            let window = windowed_passage(source, 0, end).expect("resolves");

            assert_eq!(&window.passage[window.span.0..window.span.1], "Append-only");
            assert!(window.passage.starts_with("Append-only"));
        }

        #[test]
        fn an_unpunctuated_document_is_capped_rather_than_returned_whole() {
            // No sentence terminator anywhere — the cap is what stops this from
            // becoming "the whole document" on a transcript or a log dump.
            // "word " is 5 ASCII bytes, repeated an exact multiple of the cap so
            // the expected boundary is computable by hand rather than merely
            // bounded — a weaker `len() < source.len()` version of this test
            // passed even when the boundary arithmetic was wrong in three
            // different ways, because almost any wrong-but-not-panicking
            // offset still lands somewhere short of the whole document.
            let filler = "word ".repeat(400); // 2000 bytes, no '.', '!', '?', or '\n'
            let source = format!("{filler}TARGET{filler}");
            let start = source.find("TARGET").expect("fixture");
            let end = start + "TARGET".len();

            let window = windowed_passage(&source, start, end).expect("resolves");

            // 400 chars capped each side of the 6-byte span.
            assert_eq!(window.passage.len(), 400 + 6 + 400);
            assert_eq!(window.span, (400, 406));
            assert_eq!(&window.passage[window.span.0..window.span.1], "TARGET");
        }

        #[test]
        fn a_zero_width_span_is_a_valid_degenerate_case_not_a_reversed_one() {
            let source = "The cat sat on the mat.";
            let window = windowed_passage(source, 4, 4).expect("start == end is not reversed");
            assert_eq!(window.span, (4, 4));
        }

        #[test]
        fn a_span_ending_exactly_at_the_source_length_is_in_bounds() {
            let source = "A short passage.";
            let end = source.len();
            let window =
                windowed_passage(source, 0, end).expect("end == source.len() is not out of bounds");
            assert_eq!(window.span, (0, end));
        }

        #[test]
        fn a_start_that_splits_a_character_does_not_resolve_even_when_end_is_valid() {
            // 'é' occupies bytes 3-4; byte 4 is mid-character, byte 5 (the
            // space after it) is a valid boundary on its own.
            let source = "café shop";
            assert!(windowed_passage(source, 4, 5).is_none());
        }

        #[test]
        fn an_end_that_splits_a_character_does_not_resolve_even_when_start_is_valid() {
            let source = "café shop";
            assert!(windowed_passage(source, 0, 4).is_none());
        }

        #[test]
        fn a_terminator_found_within_a_capped_backward_scan_is_still_located_correctly() {
            // 450 filler characters exceed the 400-char cap, so the scan
            // window's own start (not 0) is what the terminator search is
            // offset from — the arithmetic a shorter fixture cannot exercise.
            let filler = "x".repeat(450);
            let source = format!("{filler}. TARGET follows.");
            let start = source.find("TARGET").expect("fixture");
            let end = start + "TARGET".len();

            let window = windowed_passage(&source, start, end).expect("resolves");

            assert_eq!(window.passage, "TARGET follows.");
            assert_eq!(&window.passage[window.span.0..window.span.1], "TARGET");
        }

        #[test]
        fn the_whitespace_trim_offset_is_added_to_passage_start_not_subtracted() {
            // "AAAA" is non-whitespace, so trimming stops there rather than at
            // "TARGET" — the offset it stops at is what this pins down.
            // Asserting only the span-sliced substring (as the earlier tests
            // do) cannot catch a wrong sign here: `start - passage_start` and
            // the passage slice shift together under this specific mutation,
            // so the two stay internally consistent with each other while
            // both being wrong — only a full-string assertion is external to
            // that symmetry.
            let source = "First. AAAA TARGET here.";
            let start = source.find("TARGET").expect("fixture");
            let end = start + "TARGET".len();

            let window = windowed_passage(source, start, end).expect("resolves");

            assert_eq!(window.passage, "AAAA TARGET here.");
        }

        #[test]
        fn a_gap_that_is_entirely_whitespace_is_trimmed_all_the_way_to_the_span() {
            let source = "First.   TARGET here.";
            let start = source.find("TARGET").expect("fixture");
            let end = start + "TARGET".len();

            let window = windowed_passage(source, start, end).expect("resolves");

            assert_eq!(window.passage, "TARGET here.");
            assert_eq!(&window.passage[window.span.0..window.span.1], "TARGET");
        }

        #[test]
        fn an_out_of_bounds_span_does_not_resolve() {
            let source = "short";
            assert!(windowed_passage(source, 10, 20).is_none());
        }

        #[test]
        fn a_span_with_start_after_end_does_not_resolve() {
            let source = "the orders table";
            assert!(windowed_passage(source, 5, 2).is_none());
        }
    }
}
