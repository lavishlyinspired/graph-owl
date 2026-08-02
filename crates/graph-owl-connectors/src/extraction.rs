//! Ontology-constrained extraction — Epic 21 Slices B and C.
//!
//! **Decision 1: the model is the constraint, and it is enforced here, not
//! in the worker.** Free-form triple extraction produces a graph nothing
//! can query, so a claim naming a predicate outside the Epic 1 vocabulary
//! is discarded with a reason rather than stored as an untyped triple.
//!
//! Doing that check on *this* side of the boundary is the point. The
//! workers that matter are external and, in the LLM case, not deterministic
//! — an extractor that policed its own vocabulary would be trusting the
//! component least able to promise anything. [`constrain`] therefore
//! applies to every claim from every source, including the in-process
//! extractor below, which gets no exemption for being local.

use graph_owl_core::extraction::{
    Claim, DiscardedClaim, ExtractionResult, ParsedDocument, Provenance, TextSpan,
};

/// The predicates a claim may use.
///
/// Passed in rather than hard-coded so the vocabulary stays owned by Epic 1
/// and this module does not become a second place it is written down.
pub type Vocabulary<'a> = &'a [&'a str];

/// Turns a worker's raw output into a result whose claims are known to fit
/// the model.
///
/// Every rejection **names what was wrong and keeps the claim**, so a run
/// against a mis-prompted extractor can be diagnosed from its output rather
/// than by re-running it with logging.
#[must_use]
pub fn constrain(
    result: ExtractionResult,
    vocabulary: Vocabulary<'_>,
    known_subject: &dyn Fn(&str) -> bool,
) -> ExtractionResult {
    let mut claims = Vec::new();
    let mut discarded = result.discarded;

    for claim in result.claims {
        if !vocabulary.contains(&claim.predicate.as_str()) {
            let reason = format!(
                "predicate `{}` is not in the vocabulary; open information extraction \
                 produces a graph nothing can query (decision 1)",
                claim.predicate
            );
            discarded.push(DiscardedClaim { claim, reason });
            continue;
        }
        // A claim about an entity the catalog has never heard of cannot be
        // attached to anything. Resolving *which* entity a mention refers to
        // is Epic 17's job; this only asks whether the answer exists.
        if !known_subject(&claim.subject) {
            let reason = format!("`{}` is not a known entity", claim.subject);
            discarded.push(DiscardedClaim { claim, reason });
            continue;
        }
        if claim.object.trim().is_empty() {
            let reason = "the claim has no object".to_string();
            discarded.push(DiscardedClaim { claim, reason });
            continue;
        }
        claims.push(claim);
    }

    ExtractionResult { claims, discarded }
}

/// Produces claims from a parsed document.
///
/// The one method takes and returns plain data, so an out-of-process worker
/// satisfies the same contract by exchanging JSON — see
/// `graph_owl_core::extraction`'s own note on why the boundary is shaped
/// this way.
pub trait ClaimExtractor: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;

    /// Claims found in `document`, **before** [`constrain`] runs. An
    /// extractor is not trusted to police itself.
    fn extract(&self, document: &ParsedDocument) -> ExtractionResult;
}

/// Finds entities by name and attributes the sentence they appear in.
///
/// **Deterministic and modest on purpose.** It is the default so that
/// something works with no Python runtime (decision 4), and its confidence
/// is set to reflect what it actually knows: a name matched in prose is
/// evidence, not proof, so it lands in the *surface* band and waits for a
/// human. An extractor that claimed 0.9 for string matching would be
/// asserting straight into the graph on the strength of a substring.
pub struct MentionExtractor {
    /// FQNs to look for, longest first so `svc.db.orders` wins over
    /// `orders` when both appear — the more specific match is the more
    /// informative one.
    subjects: Vec<String>,
}

/// What a name match is worth.
///
/// **Not a tuning number**: `00c`'s bands make 0.5–0.8 "stored, flagged
/// uncertain, shown for confirmation", and that is exactly the epistemic
/// status of "this string appeared near that string". 0.6 sits inside the
/// band rather than at either edge, so it cannot silently cross a threshold
/// if a band is ever adjusted by a hair.
const MENTION_CONFIDENCE: f64 = 0.6;

impl MentionExtractor {
    #[must_use]
    pub fn new(mut subjects: Vec<String>) -> Self {
        subjects.sort_by_key(|s| std::cmp::Reverse(s.len()));
        Self { subjects }
    }
}

impl ClaimExtractor for MentionExtractor {
    fn name(&self) -> &str {
        "mention-rules"
    }

    fn version(&self) -> &str {
        "1"
    }

    fn extract(&self, document: &ParsedDocument) -> ExtractionResult {
        let mut claims = Vec::new();

        for (start, sentence) in sentences(&document.text) {
            for subject in &self.subjects {
                let Some(offset) = sentence.find(subject.as_str()) else {
                    continue;
                };
                // The sentence *is* the evidence — a span covering only the
                // matched name would prove the name occurs, which nobody
                // doubts, rather than what was said about it.
                let span = TextSpan::new(start, start + sentence.len());
                claims.push(Claim {
                    subject: subject.clone(),
                    predicate: "description".to_string(),
                    object: sentence.trim().to_string(),
                    confidence: MENTION_CONFIDENCE,
                    provenance: Provenance {
                        source_id: document.source_id.clone(),
                        extractor: self.name().to_string(),
                        extractor_version: self.version().to_string(),
                        extracted_at: chrono::Utc::now(),
                        evidence: span,
                    },
                });
                let _ = offset;
                // One claim per sentence: the longest matching subject wins,
                // because a sentence about `svc.db.orders` is not
                // independently a sentence about `orders`.
                break;
            }
        }

        ExtractionResult {
            claims,
            discarded: Vec::new(),
        }
    }
}

/// Splits on sentence terminators and newlines, yielding `(byte offset,
/// text)`.
///
/// **A period only ends a sentence when whitespace or the end of the text
/// follows it**, and that is not a refinement — it is the difference
/// between working and not. Every subject this extractor looks for is an
/// FQN, and FQNs are dotted: naive splitting on `.` tore `svc.db.orders`
/// into `The svc.`, `db.` and `orders table is append-only.`, so no
/// sentence ever contained the name and the extractor found nothing at all.
/// Found by the tests below, which is exactly what they are for.
///
/// Offsets are returned because a claim without a resolvable span is
/// unverifiable (decision 5), and recomputing them by searching for the
/// sentence later would find the *first* occurrence rather than this one.
fn sentences(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0usize;

    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let terminates = match character {
            // A newline always ends a line, and a line is close enough to a
            // sentence for prose that was written as a list or a runbook.
            '\n' => true,
            '.' | '!' | '?' => characters
                .peek()
                // End of text, or whitespace after it. `svc.db` keeps its
                // dot; `append-only. Then` does not.
                .is_none_or(|(_, next)| next.is_whitespace()),
            _ => false,
        };
        if terminates {
            let end = index + character.len_utf8();
            let slice = &text[start..end];
            if !slice.trim().is_empty() {
                out.push((start, slice));
            }
            start = end;
        }
    }
    if start < text.len() {
        let slice = &text[start..];
        if !slice.trim().is_empty() {
            out.push((start, slice));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::extraction::Disposition;

    const VOCABULARY: &[&str] = &["description", "owner", "feeds"];

    fn document(text: &str) -> ParsedDocument {
        ParsedDocument {
            source_id: "runbook.md".to_string(),
            media_type: "markdown".to_string(),
            text: text.to_string(),
            sections: Vec::new(),
        }
    }

    fn claim(subject: &str, predicate: &str, object: &str) -> Claim {
        Claim {
            subject: subject.to_string(),
            predicate: predicate.to_string(),
            object: object.to_string(),
            confidence: 0.9,
            provenance: Provenance {
                source_id: "s".to_string(),
                extractor: "test".to_string(),
                extractor_version: "1".to_string(),
                extracted_at: chrono::Utc::now(),
                evidence: TextSpan::new(0, 1),
            },
        }
    }

    fn result(claims: Vec<Claim>) -> ExtractionResult {
        ExtractionResult {
            claims,
            discarded: Vec::new(),
        }
    }

    // ── decision 1: the vocabulary is the constraint ────────────────────

    /// **The core of decision 1.** A predicate nobody defined is discarded
    /// with a reason, never stored as an untyped triple.
    #[test]
    fn a_predicate_outside_the_vocabulary_is_discarded_naming_it() {
        let constrained = constrain(
            result(vec![claim("svc", "vibes", "good")]),
            VOCABULARY,
            &|_| true,
        );

        assert!(constrained.claims.is_empty());
        assert!(
            constrained.discarded[0].reason.contains("vibes"),
            "{:?}",
            constrained.discarded[0]
        );
    }

    /// And the converse, without which "discard everything" would pass too.
    #[test]
    fn a_predicate_in_the_vocabulary_survives() {
        let constrained = constrain(
            result(vec![claim("svc", "description", "the orders service")]),
            VOCABULARY,
            &|_| true,
        );

        assert_eq!(constrained.claims.len(), 1);
        assert!(constrained.discarded.is_empty());
    }

    #[test]
    fn a_claim_about_an_unknown_entity_is_discarded() {
        let constrained = constrain(
            result(vec![claim("nobody.knows.this", "description", "x")]),
            VOCABULARY,
            &|subject| subject == "svc",
        );

        assert!(constrained.claims.is_empty());
        assert!(
            constrained.discarded[0]
                .reason
                .contains("nobody.knows.this")
        );
    }

    #[test]
    fn a_claim_with_an_empty_object_is_discarded() {
        let constrained = constrain(
            result(vec![claim("svc", "description", "   ")]),
            VOCABULARY,
            &|_| true,
        );

        assert!(constrained.claims.is_empty());
        assert!(constrained.discarded[0].reason.contains("no object"));
    }

    /// **The in-process extractor gets no exemption.** Its output goes
    /// through the same constraint as a worker's, which is what makes the
    /// rule a property of the system rather than of each extractor's
    /// discipline.
    #[test]
    fn the_local_extractor_is_constrained_like_any_other() {
        let extractor = MentionExtractor::new(vec!["svc.db.orders".to_string()]);
        let raw = extractor.extract(&document("The svc.db.orders table is append-only."));
        assert_eq!(raw.claims.len(), 1);

        // An empty vocabulary rejects even a locally produced claim.
        let constrained = constrain(raw, &[], &|_| true);

        assert!(constrained.claims.is_empty());
        assert_eq!(constrained.discarded.len(), 1);
    }

    // ── the mention extractor ───────────────────────────────────────────

    #[test]
    fn a_named_entity_in_prose_produces_a_claim() {
        let extractor = MentionExtractor::new(vec!["svc.db.orders".to_string()]);

        let found = extractor.extract(&document("The svc.db.orders table is append-only."));

        assert_eq!(found.claims.len(), 1);
        assert_eq!(found.claims[0].subject, "svc.db.orders");
        assert!(found.claims[0].object.contains("append-only"));
    }

    /// **A name match is evidence, not proof.** It lands in the band that
    /// waits for a human — an extractor claiming certainty on the strength
    /// of a substring would write straight into the graph.
    #[test]
    fn a_mention_is_surfaced_for_confirmation_never_asserted() {
        let extractor = MentionExtractor::new(vec!["svc".to_string()]);

        let found = extractor.extract(&document("svc is the billing service."));

        assert_eq!(
            Disposition::for_confidence(found.claims[0].confidence),
            Disposition::Surface,
            "string matching must not assert"
        );
    }

    /// The **longest** match wins: a sentence about `svc.db.orders` is not
    /// independently a sentence about `orders`.
    #[test]
    fn the_most_specific_subject_wins_over_a_shorter_one() {
        let extractor =
            MentionExtractor::new(vec!["orders".to_string(), "svc.db.orders".to_string()]);

        let found = extractor.extract(&document("The svc.db.orders table is append-only."));

        assert_eq!(found.claims.len(), 1, "one claim, not two");
        assert_eq!(found.claims[0].subject, "svc.db.orders");
    }

    #[test]
    fn a_document_mentioning_nothing_known_produces_no_claims() {
        let extractor = MentionExtractor::new(vec!["svc.db.orders".to_string()]);

        let found = extractor.extract(&document("Nothing relevant is discussed here."));

        assert!(found.claims.is_empty());
    }

    /// **Every claim's evidence span resolves**, which is what makes it
    /// verifiable (decision 5). A span that pointed nowhere would be a
    /// citation to a page that does not exist.
    #[test]
    fn every_claims_evidence_span_resolves_against_the_source_text() {
        let text = "First sentence about svc.db.orders. Then another about svc.db.orders here.";
        let doc = document(text);
        let extractor = MentionExtractor::new(vec!["svc.db.orders".to_string()]);

        let found = extractor.extract(&doc);

        assert_eq!(found.claims.len(), 2);
        for claim in &found.claims {
            let evidence = claim
                .provenance
                .evidence
                .resolve(&doc.text)
                .expect("every span must resolve");
            assert!(
                evidence.contains("svc.db.orders"),
                "the evidence must contain what it is evidence for: {evidence:?}"
            );
        }
    }

    /// The second mention's span points at the **second** sentence, not the
    /// first — the reason offsets are carried rather than recomputed by
    /// searching, which would find the earlier occurrence every time.
    #[test]
    fn the_second_mention_cites_the_second_sentence() {
        let text = "First about svc. Second about svc here.";
        let doc = document(text);
        let extractor = MentionExtractor::new(vec!["svc".to_string()]);

        let found = extractor.extract(&doc);

        let second = found.claims[1]
            .provenance
            .evidence
            .resolve(&doc.text)
            .expect("resolves");
        assert!(second.contains("Second"), "{second:?}");
        assert!(!second.contains("First"), "{second:?}");
    }

    /// Multi-byte text must not produce spans that split a character —
    /// `resolve` would return `None` and the claim would be uncitable.
    #[test]
    fn spans_are_character_safe_in_non_ascii_text() {
        let text = "Le café sert svc.db.orders. Très bien.";
        let doc = document(text);
        let extractor = MentionExtractor::new(vec!["svc.db.orders".to_string()]);

        let found = extractor.extract(&doc);

        assert!(!found.claims.is_empty());
        for claim in &found.claims {
            assert!(
                claim.provenance.evidence.resolve(&doc.text).is_some(),
                "a span split a multi-byte character"
            );
        }
    }

    #[test]
    fn the_extractor_identifies_itself_in_every_claims_provenance() {
        let extractor = MentionExtractor::new(vec!["svc".to_string()]);

        let found = extractor.extract(&document("svc is fine."));

        assert_eq!(found.claims[0].provenance.extractor, "mention-rules");
        assert_eq!(found.claims[0].provenance.extractor_version, "1");
        assert_eq!(found.claims[0].provenance.source_id, "runbook.md");
    }
}
