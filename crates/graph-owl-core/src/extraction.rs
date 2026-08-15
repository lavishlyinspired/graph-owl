//! Extracted claims and their provenance — Epic 21.
//!
//! **These types are a wire contract, not merely a domain model, and that
//! is the constraint that shapes every decision here.** Decision 0 puts the
//! interesting extractors — PDF layout, OCR, LLM, multimodal — *out of
//! process, in Python*. Those workers do not exist yet, and the point of
//! designing against them now is that adding one must never require
//! changing what is in this file. Concretely, that rules out three things
//! that would otherwise be natural in Rust:
//!
//! - **No enum naming the kind of extractor.** An `ExtractorKind { Rules,
//!   Llm, Ocr }` would need a new variant for every worker anyone ever
//!   writes, and each variant is a breaking change to a type that has
//!   already been persisted. [`Provenance`] carries an extractor's
//!   *identity* as data instead.
//! - **No Rust-specific document representation.** A parsed document is
//!   text plus spans ([`ParsedDocument`]), which a Python worker can
//!   produce as readily as a Rust one. An AST would be a shape only Rust
//!   could speak.
//! - **No claim type that only in-process code could build.** A [`Claim`]
//!   names its subject and predicate as strings, so a worker that has never
//!   heard of `AssetKind` can still emit one and be told it was wrong.
//!
//! Everything here is `Serialize + Deserialize` for the same reason: the
//! boundary these cross is a process boundary, and a type that cannot round
//! trip through JSON cannot cross it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Where in a source document a claim came from.
///
/// Byte offsets rather than line/column: a PDF worker and an OCR worker
/// have no meaningful notion of a line, and every representation can agree
/// on "this range of the extracted text".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    /// The byte offset of the span's start.
    pub start: usize,
    /// The byte offset just past the span's end.
    pub end: usize,
}

impl TextSpan {
    /// A span over `[start, end)`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The text this span refers to, if it is within bounds.
    ///
    /// Returns `None` rather than panicking on an out-of-range span,
    /// because a span is **untrusted input** the moment an external worker
    /// can produce one — and a worker that miscounts must not be able to
    /// crash the ingesting process.
    #[must_use]
    pub fn resolve<'a>(&self, text: &'a str) -> Option<&'a str> {
        text.get(self.start..self.end)
    }
}

/// A document reduced to what every parser can agree on.
///
/// Deliberately *not* an AST. Markdown, PDF, OCR and a chat export have
/// nothing structural in common, and a representation rich enough for one
/// is wrong for the others — but all of them can produce text, and all of
/// them can say which part of it a claim came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParsedDocument {
    /// Stable identity of the source, used for idempotent re-ingestion and
    /// for `capturedAs` (decision 5). A path, a URL, a ticket id — whatever
    /// the worker can reproduce for the same document next time.
    pub source_id: String,
    /// A hint at the original medium (`markdown`, `pdf`, `chat`), carried
    /// as a string rather than an enum for the same reason the extractor's
    /// identity is: the list is open, and it is owned by whoever writes the
    /// next worker.
    pub media_type: String,
    /// The document reduced to plain text.
    pub text: String,
    /// Optional structure a parser happened to recover — a heading, a
    /// cell, a speaker turn. Nothing downstream *requires* sections, so a
    /// parser that recovers none is not degraded, merely less specific.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
}

/// A piece of document structure a parser happened to recover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// The section's heading, if it has one.
    pub heading: Option<String>,
    /// Where in the document this section lies.
    pub span: TextSpan,
}

/// Where a claim's evidence lives, for a source shape `TextSpan` cannot
/// express.
///
/// **Data, not a variant — the same trade `Provenance`'s `extractor` field
/// already makes.** `kind` names the shape (`"text"`, `"tabular"`, `"json"`,
/// more later) as an open string; `location`'s shape is a documented
/// convention per `kind`, not enforced by the Rust type system. A closed
/// enum would need a recompile for every new source format a future worker
/// introduces — exactly what this epic's binding requirement (adding a
/// worker must not change the Rust domain model) rules out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLocation {
    /// The shape of `location` — `"text"`, `"tabular"`, `"json"`, or a kind
    /// a future worker introduces.
    pub kind: String,
    /// The location itself, shaped per `kind`'s own convention. For
    /// `"text"`, this is a serialized [`TextSpan`].
    pub location: serde_json::Value,
}

impl EvidenceLocation {
    /// The text this location refers to, for a `"text"` location whose
    /// `location` deserializes to an in-bounds [`TextSpan`].
    ///
    /// Returns `None` for a non-`"text"` `kind` (a tabular cell or a JSON
    /// path has no meaningful position in a document's plain text) and for
    /// an out-of-range or malformed span, for the same reason
    /// [`TextSpan::resolve`] does: the location is untrusted input the
    /// moment an external worker can produce it.
    #[must_use]
    pub fn resolve<'a>(&self, text: &'a str) -> Option<&'a str> {
        if self.kind != "text" {
            return None;
        }
        let span: TextSpan = serde_json::from_value(self.location.clone()).ok()?;
        span.resolve(text)
    }
}

/// Who produced a claim, and from what.
///
/// **Identity as data, not as a variant.** `extractor` is a name a worker
/// chooses for itself (`markdown-rules`, `gpt-5-extractor`, `tesseract-ocr`)
/// and `extractor_version` distinguishes two runs of the same one. Adding a
/// worker is therefore a deployment, not a schema migration — which is the
/// whole point, since the workers are the part of this epic that does not
/// exist yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    /// The document the claim came from.
    pub source_id: String,
    /// The extractor's own chosen name.
    pub extractor: String,
    /// The extractor's own version.
    pub extractor_version: String,
    /// When extraction ran.
    pub extracted_at: DateTime<Utc>,
    /// Where in the source this claim was found. **A claim without its
    /// source is unverifiable** (decision 5), so this is not optional — a
    /// worker that cannot say where a claim came from is telling you
    /// something about the claim.
    pub evidence: EvidenceLocation,
}

/// One extracted assertion, before any policy has been applied to it.
///
/// Subject and predicate are strings because the worker that produced them
/// may know nothing of this codebase's types. Validating them against the
/// real vocabulary is graph-owl's job, not the worker's — and doing it here
/// rather than there is what makes decision 1 (ontology-constrained, never
/// open information extraction) enforceable against a worker nobody in this
/// repository wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    /// The FQN of the entity this is about.
    pub subject: String,
    /// A predicate name from the Epic 1 vocabulary.
    pub predicate: String,
    /// The claimed value.
    pub object: String,
    /// The worker's own confidence, `0.0..=1.0`.
    ///
    /// **A proposal, never a decision.** [`Disposition::for_confidence`]
    /// decides what happens to it, in this process — so a worker cannot
    /// assert something below the threshold by claiming otherwise.
    pub confidence: f64,
    /// Who produced it, and from what.
    pub provenance: Provenance,
}

/// What the confidence bands say to do with a claim.
///
/// `00c-domain-model.md`: ≥0.8 assert, 0.5–0.8 surface for confirmation,
/// <0.5 ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    /// Enters the graph — though still in `graph:extraction`, never the
    /// default graph (decision 2).
    Assert,
    /// Stored, flagged uncertain, shown for confirmation.
    Surface,
    /// Not stored at all.
    Ignore,
}

/// The assert threshold, from `00c-domain-model.md`'s confidence bands.
pub const ASSERT_THRESHOLD: f64 = 0.8;
/// The surface threshold, below which a claim is discarded outright.
pub const SURFACE_THRESHOLD: f64 = 0.5;

impl Disposition {
    /// **The policy, applied in graph-owl rather than in the worker.**
    ///
    /// An external extractor proposes a confidence; this decides what it
    /// buys. Keeping the decision here is what stops a mis-tuned or
    /// compromised worker from writing straight into the graph by asserting
    /// its own certainty.
    #[must_use]
    pub fn for_confidence(confidence: f64) -> Self {
        // NaN is not "low confidence", it is a broken worker — and `>=`
        // against NaN is false, so it would silently fall through to
        // `Ignore` without this. Explicit is better than incidental for a
        // value arriving from another process.
        if confidence.is_nan() {
            return Disposition::Ignore;
        }
        if confidence >= ASSERT_THRESHOLD {
            Disposition::Assert
        } else if confidence >= SURFACE_THRESHOLD {
            Disposition::Surface
        } else {
            Disposition::Ignore
        }
    }
}

/// A reviewer's decision on a queued claim — Epic 21 x Epic 42 decision 2.
///
/// `Accept` and `Edit` both confirm the claim — it earns the same
/// projection a confident extractor's claim gets — and differ only in
/// whether the subject/predicate/object that get projected are the ones the
/// extractor proposed or the ones the reviewer corrected them to. `Reject`
/// carries a `reason` for the same standing rule resolution's review queue
/// already enforces: an unreasoned rejection teaches the extractor nothing
/// and is unauditable when someone asks about it later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(
    tag = "outcome",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ReviewDecision {
    /// The claim as extracted is correct.
    Accept,
    /// The claim is correct in substance but the extractor got a detail
    /// wrong — the reviewer's values are what get projected, not the
    /// extractor's.
    Edit {
        /// The corrected subject.
        subject: String,
        /// The corrected predicate.
        predicate: String,
        /// The corrected object.
        object: String,
    },
    /// The claim does not belong in the graph at all.
    Reject {
        /// Why — required, not merely accepted.
        reason: String,
    },
}

impl ReviewDecision {
    /// The `extraction_claims.state` value this decision writes.
    #[must_use]
    pub fn state(&self) -> &'static str {
        match self {
            ReviewDecision::Accept | ReviewDecision::Edit { .. } => "confirmed",
            ReviewDecision::Reject { .. } => "rejected",
        }
    }
}

/// Why a claim was thrown away, kept so a run can be diagnosed rather than
/// merely counted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscardedClaim {
    /// The claim that was thrown away.
    pub claim: Claim,
    /// Why it was thrown away.
    pub reason: String,
}

/// What one extraction run produced.
///
/// **The wire contract between graph-owl and any worker.** A Python worker
/// returns exactly this shape as JSON; the in-process rule-based extractor
/// returns it as a value. Neither path is privileged, which is what makes
/// the boundary stable — the in-process extractor is not a shortcut around
/// the contract, it is a client of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    /// The claims extracted.
    pub claims: Vec<Claim>,
    /// **Discards are reported, not silent.** Decision 1 says anything that
    /// does not fit the model is discarded *with a reason*; a run that
    /// quietly dropped half its output would be indistinguishable from one
    /// that found nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discarded: Vec<DiscardedClaim>,
}

impl ExtractionResult {
    /// Splits claims by what the bands say to do with them.
    ///
    /// Returns `(assert, surface)`; anything below the surface threshold is
    /// moved into `discarded` with its reason, so nothing vanishes without
    /// a record.
    #[must_use]
    pub fn partition(mut self) -> (Vec<Claim>, Vec<Claim>, Vec<DiscardedClaim>) {
        let mut assert = Vec::new();
        let mut surface = Vec::new();
        for claim in std::mem::take(&mut self.claims) {
            match Disposition::for_confidence(claim.confidence) {
                Disposition::Assert => assert.push(claim),
                Disposition::Surface => surface.push(claim),
                Disposition::Ignore => {
                    let reason = format!(
                        "confidence {:.2} is below the {SURFACE_THRESHOLD} threshold",
                        claim.confidence
                    );
                    self.discarded.push(DiscardedClaim { claim, reason });
                }
            }
        }
        (assert, surface, self.discarded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> Provenance {
        Provenance {
            source_id: "runbook.md".to_string(),
            extractor: "markdown-rules".to_string(),
            extractor_version: "1".to_string(),
            extracted_at: Utc::now(),
            evidence: EvidenceLocation {
                kind: "text".to_string(),
                location: serde_json::to_value(TextSpan::new(0, 4)).expect("TextSpan serializes"),
            },
        }
    }

    fn claim(confidence: f64) -> Claim {
        Claim {
            subject: "svc.db.orders".to_string(),
            predicate: "description".to_string(),
            object: "the orders table".to_string(),
            confidence,
            provenance: provenance(),
        }
    }

    #[test]
    fn the_bands_match_the_domain_model() {
        assert_eq!(Disposition::for_confidence(0.95), Disposition::Assert);
        assert_eq!(Disposition::for_confidence(0.65), Disposition::Surface);
        assert_eq!(Disposition::for_confidence(0.2), Disposition::Ignore);
    }

    /// The boundaries themselves, which is where an off-by-one lives: `0.8`
    /// asserts and `0.5` surfaces, per `00c`'s own `≥`.
    #[test]
    fn the_thresholds_are_inclusive_at_the_bottom_of_each_band() {
        assert_eq!(Disposition::for_confidence(0.8), Disposition::Assert);
        assert_eq!(Disposition::for_confidence(0.5), Disposition::Surface);
        // And just below each.
        assert_eq!(Disposition::for_confidence(0.799), Disposition::Surface);
        assert_eq!(Disposition::for_confidence(0.499), Disposition::Ignore);
    }

    /// **NaN is a broken worker, not low confidence.** Every comparison
    /// against NaN is false, so without the explicit guard it would reach
    /// `Ignore` by accident — the right answer for the wrong reason, which
    /// stops being the right answer the moment the branches are reordered.
    #[test]
    fn a_nonsense_confidence_is_ignored_deliberately() {
        assert_eq!(Disposition::for_confidence(f64::NAN), Disposition::Ignore);
        assert_eq!(
            Disposition::for_confidence(f64::INFINITY),
            Disposition::Assert,
            "infinity is >= the threshold; a worker sending it is wrong but not dangerous"
        );
        assert_eq!(
            Disposition::for_confidence(f64::NEG_INFINITY),
            Disposition::Ignore
        );
    }

    #[test]
    fn partition_sorts_claims_into_their_bands() {
        let result = ExtractionResult {
            claims: vec![claim(0.9), claim(0.6), claim(0.1)],
            discarded: Vec::new(),
        };

        let (assert, surface, discarded) = result.partition();

        assert_eq!(assert.len(), 1);
        assert_eq!(surface.len(), 1);
        assert_eq!(discarded.len(), 1, "the low-confidence claim is recorded");
    }

    /// A discard **says why**. A run that dropped claims silently could not
    /// be told apart from one that found nothing.
    #[test]
    fn a_discarded_claim_carries_its_reason() {
        let result = ExtractionResult {
            claims: vec![claim(0.1)],
            discarded: Vec::new(),
        };

        let (_, _, discarded) = result.partition();

        assert!(discarded[0].reason.contains("0.10"), "{:?}", discarded[0]);
        assert!(discarded[0].reason.contains("0.5"), "{:?}", discarded[0]);
    }

    /// Discards a worker already made are **preserved**, not replaced —
    /// otherwise the band filter would erase the extractor's own reasoning
    /// about what it chose not to emit.
    #[test]
    fn partition_keeps_discards_the_extractor_already_reported() {
        let result = ExtractionResult {
            claims: vec![claim(0.9)],
            discarded: vec![DiscardedClaim {
                claim: claim(0.9),
                reason: "predicate `vibes` is not in the vocabulary".to_string(),
            }],
        };

        let (assert, _, discarded) = result.partition();

        assert_eq!(assert.len(), 1);
        assert_eq!(discarded.len(), 1);
        assert!(discarded[0].reason.contains("vibes"));
    }

    /// **The wire contract.** These types cross a process boundary, so a
    /// round trip through JSON is a property the design depends on — not a
    /// nicety. A future Python worker sends exactly this.
    #[test]
    fn an_extraction_result_round_trips_through_json() {
        let original = ExtractionResult {
            claims: vec![claim(0.9)],
            discarded: vec![DiscardedClaim {
                claim: claim(0.3),
                reason: "too low".to_string(),
            }],
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: ExtractionResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
    }

    /// The JSON is camelCase throughout, matching every other wire type in
    /// this codebase — a worker author should not have to learn a second
    /// convention.
    #[test]
    fn the_wire_shape_is_camel_case() {
        let json = serde_json::to_string(&claim(0.9)).expect("serialize");

        assert!(json.contains("\"extractorVersion\""), "{json}");
        assert!(json.contains("\"sourceId\""), "{json}");
        assert!(!json.contains("extractor_version"), "{json}");
    }

    /// An out-of-range span is untrusted input, not a panic. A worker that
    /// miscounts bytes must not be able to bring down the ingest.
    #[test]
    fn an_out_of_range_span_resolves_to_nothing_rather_than_panicking() {
        let text = "short";
        assert_eq!(TextSpan::new(0, 5).resolve(text), Some("short"));
        assert_eq!(TextSpan::new(2, 400).resolve(text), None);
        assert_eq!(TextSpan::new(400, 401).resolve(text), None);
    }

    /// A span that splits a multi-byte character resolves to `None` rather
    /// than panicking — `str::get` is what makes that true, and this test is
    /// what stops someone "optimising" it into slicing.
    #[test]
    fn a_span_splitting_a_multibyte_character_does_not_panic() {
        let text = "café"; // 'é' occupies bytes 3..5
        assert_eq!(TextSpan::new(0, 3).resolve(text), Some("caf"));
        assert_eq!(
            TextSpan::new(0, 4).resolve(text),
            None,
            "byte 4 is mid-é: None, not a panic and not a silent truncation"
        );
        assert_eq!(TextSpan::new(0, 5).resolve(text), Some("café"));
    }

    /// **Tabular evidence.** A spreadsheet cell has no byte offset into
    /// prose — `EvidenceLocation` is what lets a claim cite "this row, this
    /// column" instead of forcing every source into `TextSpan`'s shape.
    #[test]
    fn tabular_evidence_round_trips_through_json() {
        let mut original = claim(0.9);
        original.provenance.evidence = EvidenceLocation {
            kind: "tabular".to_string(),
            location: serde_json::json!({
                "sheet": "Purchases",
                "row": 1821,
                "column": "Invoice No"
            }),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: Claim = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
        assert_eq!(parsed.provenance.evidence.kind, "tabular");
        assert_eq!(
            parsed.provenance.evidence.location["row"],
            serde_json::json!(1821),
            "the round trip must preserve location's actual contents, not just avoid panicking"
        );
    }

    /// **JSON-path evidence.** A claim sourced from a JSON payload (an API
    /// response, a GST return) names a `$`-path, not a byte range.
    #[test]
    fn json_path_evidence_round_trips_through_json() {
        let mut original = claim(0.9);
        original.provenance.evidence = EvidenceLocation {
            kind: "json".to_string(),
            location: serde_json::json!({ "path": "$.b2b[14].inv[3].inum" }),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: Claim = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
        assert_eq!(parsed.provenance.evidence.kind, "json");
        assert_eq!(
            parsed.provenance.evidence.location["path"],
            serde_json::json!("$.b2b[14].inv[3].inum")
        );
    }
}
