//! Extraction runs, the confirmation queue, and idempotent re-ingestion —
//! Epic 21 Slices C, D and F.
//!
//! **Everything lands in `graph:extraction`, never the default graph**
//! (decision 2). Epic 6's reasoning does not run over that named graph by
//! default, which is what stops unconfirmed machine output from silently
//! feeding inference — the failure this epic is most able to cause, since a
//! confidently-wrong extraction becomes a confidently-wrong memory an agent
//! later cites.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::extraction::Claim;

/// The named graph every extracted fact is written to.
///
/// A constant rather than a string at each call site: decision 2 is only
/// true if *every* write honours it, and one forgotten literal would be a
/// silent breach with no compile error.
pub const EXTRACTION_GRAPH: &str = "graph:extraction";

/// Where a surfaced claim sits in review.
///
/// `Rejected` is terminal and **kept**, not deleted — a rejection that
/// vanished would be re-proposed by the next run of the same extractor over
/// the same document, and a reviewer would answer it forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewState {
    Pending,
    Confirmed,
    Rejected,
}

/// A claim awaiting a human, with everything needed to judge it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedClaim {
    pub id: Uuid,
    pub claim: Claim,
    pub state: ReviewState,
    /// **The claim's own words from the source.** A reviewer asked to
    /// confirm "the orders table is append-only" without seeing the
    /// sentence it came from is being asked to trust the extractor, which
    /// is the thing under review.
    pub evidence_text: String,
    pub queued_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by: Option<String>,
}

impl QueuedClaim {
    /// Builds a queue entry, resolving the evidence text from the source.
    ///
    /// Takes the document text so the evidence is captured **now**, while
    /// the source is in hand. Resolving it later would mean re-reading a
    /// document that may have changed — and a reviewer shown the *current*
    /// text of a sentence that has since been edited is being shown
    /// something the extractor never saw.
    #[must_use]
    pub fn new(claim: Claim, source_text: &str) -> Self {
        let evidence_text = claim
            .provenance
            .evidence
            .resolve(source_text)
            .unwrap_or("(the evidence span does not resolve against the source)")
            .trim()
            .to_string();
        Self {
            id: Uuid::new_v4(),
            claim,
            state: ReviewState::Pending,
            evidence_text,
            queued_at: Utc::now(),
            decided_at: None,
            decided_by: None,
        }
    }
}

/// One execution of one extractor over one document.
///
/// The unit of undo. Decision 0 puts extraction in a named graph "so a bad
/// run is deletable wholesale" — which requires knowing which facts came
/// from which run, hence an identity rather than a loose pile of claims.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionRun {
    pub id: Uuid,
    pub source_id: String,
    /// Content hash of the source at extraction time — the whole basis of
    /// idempotent re-ingestion (Slice F). See [`content_fingerprint`].
    pub source_fingerprint: String,
    pub extractor: String,
    pub extractor_version: String,
    pub started_at: DateTime<Utc>,
    pub asserted: usize,
    pub surfaced: usize,
    pub discarded: usize,
}

/// A stable fingerprint of a document's bytes.
///
/// **What idempotence is judged on.** Re-running an extractor over an
/// unchanged document must do nothing; re-running it over an *edited* one
/// must extract again. Comparing modification times would get both wrong —
/// a touched file is not a changed one, and a restored backup is not an
/// unchanged one.
#[must_use]
pub fn content_fingerprint(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Whether an extractor needs to run again.
///
/// **Keyed on all three**, because any one alone is wrong: the same
/// document through a *better* extractor should re-extract, the same
/// extractor over an *edited* document should re-extract, and neither
/// changing means there is nothing new to learn.
#[must_use]
pub fn needs_extraction(
    previous: Option<&ExtractionRun>,
    fingerprint: &str,
    extractor: &str,
    version: &str,
) -> bool {
    match previous {
        None => true,
        Some(run) => {
            run.source_fingerprint != fingerprint
                || run.extractor != extractor
                || run.extractor_version != version
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::{Provenance, TextSpan};

    fn claim() -> Claim {
        Claim {
            subject: "svc.db.orders".to_string(),
            predicate: "description".to_string(),
            object: "the orders table is append-only".to_string(),
            confidence: 0.6,
            provenance: Provenance {
                source_id: "runbook.md".to_string(),
                extractor: "mention-rules".to_string(),
                extractor_version: "1".to_string(),
                extracted_at: Utc::now(),
                evidence: TextSpan::new(4, 39),
            },
        }
    }

    fn run(fingerprint: &str, extractor: &str, version: &str) -> ExtractionRun {
        ExtractionRun {
            id: Uuid::new_v4(),
            source_id: "runbook.md".to_string(),
            source_fingerprint: fingerprint.to_string(),
            extractor: extractor.to_string(),
            extractor_version: version.to_string(),
            started_at: Utc::now(),
            asserted: 0,
            surfaced: 0,
            discarded: 0,
        }
    }

    // ── Slice F: idempotent re-ingestion ────────────────────────────────

    #[test]
    fn a_document_never_extracted_needs_extraction() {
        assert!(needs_extraction(None, "abc", "mention-rules", "1"));
    }

    /// **The core of Slice F.** Nothing changed, so nothing is re-extracted
    /// — re-running the pipeline over a stable corpus must be free.
    #[test]
    fn an_unchanged_document_and_extractor_needs_nothing() {
        let previous = run("abc", "mention-rules", "1");

        assert!(!needs_extraction(
            Some(&previous),
            "abc",
            "mention-rules",
            "1"
        ));
    }

    #[test]
    fn an_edited_document_is_extracted_again() {
        let previous = run("abc", "mention-rules", "1");

        assert!(needs_extraction(
            Some(&previous),
            "def",
            "mention-rules",
            "1"
        ));
    }

    /// **A better extractor re-reads old documents.** Keying only on
    /// content would freeze a corpus at whatever the first extractor
    /// managed, and the whole point of an upgraded model is what it finds in
    /// what you already have.
    #[test]
    fn an_upgraded_extractor_re_reads_unchanged_documents() {
        let previous = run("abc", "mention-rules", "1");

        assert!(
            needs_extraction(Some(&previous), "abc", "mention-rules", "2"),
            "a new version must re-extract"
        );
        assert!(
            needs_extraction(Some(&previous), "abc", "llm-extractor", "1"),
            "a different extractor must re-extract"
        );
    }

    /// The fingerprint is of **content**, so identical bytes fingerprint
    /// identically regardless of where they came from.
    #[test]
    fn the_fingerprint_follows_content_not_identity() {
        assert_eq!(
            content_fingerprint(b"# Orders\nappend-only."),
            content_fingerprint(b"# Orders\nappend-only.")
        );
        assert_ne!(
            content_fingerprint(b"# Orders\nappend-only."),
            content_fingerprint(b"# Orders\nappend-only!")
        );
    }

    /// A one-byte edit changes it — a fingerprint that missed small edits
    /// would skip re-extraction on exactly the corrections people make.
    #[test]
    fn a_single_byte_edit_changes_the_fingerprint() {
        let before = content_fingerprint(b"the table is append-only");
        let after = content_fingerprint(b"the table is append-ony");

        assert_ne!(before, after);
    }

    // ── Slices C and D: the queue and its evidence ──────────────────────

    #[test]
    fn a_queued_claim_starts_pending_and_undecided() {
        let queued = QueuedClaim::new(claim(), "The the orders table is append-only. More text.");

        assert_eq!(queued.state, ReviewState::Pending);
        assert!(queued.decided_at.is_none());
        assert!(queued.decided_by.is_none());
    }

    /// **Decision 5, made usable.** The reviewer sees the sentence, not just
    /// the assertion — being asked to confirm a claim without its source is
    /// being asked to trust the extractor, which is the thing under review.
    #[test]
    fn a_queued_claim_carries_the_source_text_it_came_from() {
        let source = "The the orders table is append-only. More text.";

        let queued = QueuedClaim::new(claim(), source);

        assert!(
            queued.evidence_text.contains("append-only"),
            "{:?}",
            queued.evidence_text
        );
    }

    /// A span that does not resolve says so, rather than silently showing a
    /// reviewer nothing and letting them approve on an empty quote.
    #[test]
    fn an_unresolvable_span_is_reported_not_hidden() {
        let queued = QueuedClaim::new(claim(), "short");

        assert!(
            queued.evidence_text.contains("does not resolve"),
            "{:?}",
            queued.evidence_text
        );
    }

    /// The extraction graph is a constant so decision 2 cannot be breached
    /// by one forgotten literal.
    #[test]
    fn extracted_facts_have_one_named_graph() {
        assert_eq!(EXTRACTION_GRAPH, "graph:extraction");
    }

    #[test]
    fn a_run_round_trips_through_json() {
        let original = run("abc", "mention-rules", "1");

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: ExtractionRun = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
    }

    #[test]
    fn a_queued_claim_round_trips_through_json() {
        let original = QueuedClaim::new(claim(), "The the orders table is append-only.");

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: QueuedClaim = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
    }
}
