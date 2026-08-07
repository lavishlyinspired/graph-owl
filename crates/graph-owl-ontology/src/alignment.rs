//! Ontology alignment — Epic 104.
//!
//! Answers *"is this class the same as that class"* across vocabularies,
//! distinct from Epic 17's instance resolution (individuals, not classes)
//! and Epic 33's vocabulary annotation (what a column means, not whether
//! two classes are equivalent) — see `plans/104-ontology-alignment.md`'s
//! own table for why folding this into either would be a category error.
//!
//! **Nothing is merged.** `left` and `right` remain two classes; an
//! [`Alignment`] is a fact a query traverses, never a rewrite of either
//! side — decision 2, the same argument `00b` decision 14 makes for the
//! reasoning overlay.

use graph_owl_core::flake::{Flake, FlakeValue, Sid, namespace};

/// A cross-vocabulary match, short of logical equivalence — decision 3.
/// `owl:equivalentClass` has logical force a reasoner will act on; none of
/// these do. Verified against the W3C SKOS Reference §10, 7 August 2026:
/// all four are `skos:mappingRelation` sub-properties, and `exactMatch` is
/// additionally a sub-property of `closeMatch` (transitive and symmetric,
/// where `closeMatch` is symmetric only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPredicate {
    ExactMatch,
    CloseMatch,
    BroadMatch,
    NarrowMatch,
}

impl MatchPredicate {
    #[must_use]
    pub fn local_name(self) -> &'static str {
        match self {
            Self::ExactMatch => "exactMatch",
            Self::CloseMatch => "closeMatch",
            Self::BroadMatch => "broadMatch",
            Self::NarrowMatch => "narrowMatch",
        }
    }

    /// The real `skos:` predicate this names, in [`namespace::SKOS`].
    #[must_use]
    pub fn sid(self) -> Sid {
        Sid::new(namespace::SKOS, self.local_name())
    }
}

/// Where an alignment came from — decision 1 ("curated before computed,
/// always") and the source [`Alignment::Match`] may carry.
#[derive(Debug, Clone, PartialEq)]
pub enum AlignmentSource {
    /// A human-curated cross-vocabulary mapping already existed — UMLS's
    /// CUI, a published cross-map, a SKOS mapping file. Ingesting one is
    /// cheaper *and* more accurate than computing an equivalent, per
    /// decision 1 — computed alignment is the fallback, never the default.
    Curated { authority: String },
    /// Produced by a matching algorithm this system ran.
    Computed { method: String },
    /// A person confirmed or corrected an alignment directly.
    Human { principal: String },
}

/// The subset of [`AlignmentSource`] that may back an
/// [`Alignment::EquivalentClass`] — decision 3, enforced by the compiler
/// rather than a validation rule. **There is no `Computed` variant on this
/// type.** `owl:equivalentClass` has logical force: a reasoner will draw
/// conclusions from it, and a wrong one poisons the inference set with no
/// obvious symptom. Since no value of `AssertableSource` can represent
/// "computed", `Alignment::EquivalentClass` cannot be constructed with a
/// computed source no matter what calls it — see the `compile_fail`
/// doctest on [`Alignment`] itself, which is the actual RED test this
/// slice was built to make pass.
#[derive(Debug, Clone, PartialEq)]
pub enum AssertableSource {
    Curated { authority: String },
    Human { principal: String },
}

/// A cross-vocabulary class alignment, with its own provenance — decision
/// 2. Stored as flakes carrying the real `skos:`/`owl:` predicate plus a
/// reified node for source, confidence and directionality — never a merge
/// of `left` and `right`.
///
/// ```compile_fail
/// use graph_owl_ontology::alignment::{Alignment, AssertableSource};
/// use graph_owl_core::flake::Sid;
///
/// // Decision 3: a computed match can never assert logical equivalence.
/// // AssertableSource has no `Computed` variant, so this does not compile —
/// // the type system refuses it, not a validator.
/// let _ = Alignment::EquivalentClass {
///     left: Sid::dsc("a"),
///     right: Sid::dsc("b"),
///     source: AssertableSource::Computed { method: "embedding".into() },
///     confidence: 0.9,
///     lossy_reverse: false,
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Alignment {
    /// `skos:exactMatch` / `closeMatch` / `broadMatch` / `narrowMatch` —
    /// any [`AlignmentSource`] may assert one of these.
    Match {
        left: Sid,
        right: Sid,
        predicate: MatchPredicate,
        source: AlignmentSource,
        confidence: f64,
        /// Decision 6: the source's own directionality, preserved even
        /// though the predicate is symmetric. A SNOMED → ICD-10 mapping is
        /// frequently many-to-one and lossy in one direction; `true` means
        /// walking `right` back to `left` loses information a consumer
        /// must know it is losing.
        lossy_reverse: bool,
    },
    /// `owl:equivalentClass` — logical force, so only [`AssertableSource`]
    /// may back one.
    EquivalentClass {
        left: Sid,
        right: Sid,
        source: AssertableSource,
        confidence: f64,
        lossy_reverse: bool,
    },
}

/// One entry per source kind, matched to how [`alignment_to_flakes`] reads
/// it back — a discriminant plus the one string every variant carries
/// (authority, method, or the confirming principal).
enum SourceEncoding<'a> {
    Curated(&'a str),
    Computed(&'a str),
    Human(&'a str),
}

fn encode_source(source: &AlignmentSource) -> SourceEncoding<'_> {
    match source {
        AlignmentSource::Curated { authority } => SourceEncoding::Curated(authority),
        AlignmentSource::Computed { method } => SourceEncoding::Computed(method),
        AlignmentSource::Human { principal } => SourceEncoding::Human(principal),
    }
}

fn encode_assertable_source(source: &AssertableSource) -> SourceEncoding<'_> {
    match source {
        AssertableSource::Curated { authority } => SourceEncoding::Curated(authority),
        AssertableSource::Human { principal } => SourceEncoding::Human(principal),
    }
}

impl SourceEncoding<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Self::Curated(_) => "curated",
            Self::Computed(_) => "computed",
            Self::Human(_) => "human",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::Curated(s) | Self::Computed(s) | Self::Human(s) => s,
        }
    }
}

impl Alignment {
    #[must_use]
    fn parts(&self) -> (&Sid, &Sid, Sid, SourceEncoding<'_>, f64, bool) {
        match self {
            Self::Match {
                left,
                right,
                predicate,
                source,
                confidence,
                lossy_reverse,
            } => (
                left,
                right,
                predicate.sid(),
                encode_source(source),
                *confidence,
                *lossy_reverse,
            ),
            Self::EquivalentClass {
                left,
                right,
                source,
                confidence,
                lossy_reverse,
            } => (
                left,
                right,
                Sid::new(namespace::OWL, "equivalentClass"),
                encode_assertable_source(source),
                *confidence,
                *lossy_reverse,
            ),
        }
    }

    /// The reified node this alignment's own metadata (source, confidence,
    /// directionality) attaches to.
    ///
    /// **Deterministic, not a fresh id per call.** Derived from
    /// `(left, predicate, right)` alone — not the source — so re-ingesting
    /// the identical curated row twice (Slice B's resumability
    /// requirement) lands on the same subject rather than accumulating a
    /// duplicate, and a later run updating this alignment's source or
    /// confidence retracts and re-asserts the same node rather than
    /// leaving the old one stranded.
    #[must_use]
    pub fn subject(&self) -> Sid {
        let (left, right, predicate, ..) = self.parts();
        Sid::dsc(format!(
            "alignment:{}:{}:{}",
            left.to_iri().unwrap_or_else(|| left.to_string()),
            predicate.to_iri().unwrap_or_else(|| predicate.to_string()),
            right.to_iri().unwrap_or_else(|| right.to_string()),
        ))
    }
}

/// This alignment as flakes, at transaction time `t`.
///
/// Two things are written: the **direct semantic triple** (`left
/// {predicate} right`, e.g. `left skos:exactMatch right`) — the "graph
/// edge" a plain SPARQL query traverses with no special handling — and a
/// **reified metadata node**, [`Alignment::subject`], carrying everything
/// a triple alone cannot: source, confidence, and directionality. Decision
/// 4's confidence-band gating (whether a computed alignment's direct
/// triple should be withheld pending review) is not this function's
/// concern — it always emits both; the caller decides whether to call it.
#[must_use]
pub fn alignment_to_flakes(alignment: &Alignment, t: i64) -> Vec<Flake> {
    let (left, right, predicate, source, confidence, lossy_reverse) = alignment.parts();
    let subject = alignment.subject();

    vec![
        Flake::assert(left.clone(), predicate, FlakeValue::Ref(right.clone()), t),
        Flake::assert(
            subject.clone(),
            Sid::new(namespace::RDF, "type"),
            FlakeValue::Ref(Sid::dsc("Alignment")),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("alignmentLeft"),
            FlakeValue::Ref(left.clone()),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("alignmentRight"),
            FlakeValue::Ref(right.clone()),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("alignmentSourceKind"),
            FlakeValue::String(source.kind().to_string()),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("alignmentSourceDetail"),
            FlakeValue::String(source.detail().to_string()),
            t,
        ),
        Flake::assert(
            subject.clone(),
            Sid::dsc("confidence"),
            FlakeValue::Float(confidence),
            t,
        ),
        Flake::assert(
            subject,
            Sid::dsc("lossyReverse"),
            FlakeValue::Boolean(lossy_reverse),
            t,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cui(id: &str) -> Sid {
        Sid::new(namespace::CUI, id)
    }
    fn snomed(id: &str) -> Sid {
        Sid::new(namespace::SNOMED_CT, id)
    }

    #[test]
    fn match_predicates_are_the_real_verified_skos_terms() {
        for (predicate, local_name) in [
            (MatchPredicate::ExactMatch, "exactMatch"),
            (MatchPredicate::CloseMatch, "closeMatch"),
            (MatchPredicate::BroadMatch, "broadMatch"),
            (MatchPredicate::NarrowMatch, "narrowMatch"),
        ] {
            assert_eq!(
                predicate.sid().to_iri().as_deref(),
                Some(format!("http://www.w3.org/2004/02/skos/core#{local_name}").as_str())
            );
        }
    }

    #[test]
    fn a_curated_match_is_constructible() {
        let alignment = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert_eq!(
            alignment,
            Alignment::Match {
                left: cui("C0009044"),
                right: snomed("22298006"),
                predicate: MatchPredicate::ExactMatch,
                source: AlignmentSource::Curated {
                    authority: "UMLS".to_string()
                },
                confidence: 1.0,
                lossy_reverse: false,
            }
        );
    }

    /// The positive complement to the `compile_fail` doctest on
    /// [`Alignment`]: a curated or human source *can* back
    /// `EquivalentClass` — only `Computed` is refused.
    #[test]
    fn equivalent_class_is_constructible_from_a_curated_or_human_source() {
        let curated = Alignment::EquivalentClass {
            left: cui("C0009044"),
            right: snomed("22298006"),
            source: AssertableSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        let human = Alignment::EquivalentClass {
            left: cui("C0009044"),
            right: snomed("22298006"),
            source: AssertableSource::Human {
                principal: "asha".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert_ne!(curated, human);
    }

    #[test]
    fn to_flakes_writes_the_direct_triple_with_the_real_skos_predicate() {
        let alignment = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        let flakes = alignment_to_flakes(&alignment, 1);

        let direct = flakes
            .iter()
            .find(|f| f.s == cui("C0009044") && f.p == MatchPredicate::ExactMatch.sid())
            .expect("the direct semantic triple must be written");
        assert_eq!(direct.o, FlakeValue::Ref(snomed("22298006")));
    }

    #[test]
    fn to_flakes_writes_source_confidence_and_lossy_reverse_on_the_reified_node() {
        let alignment = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::CloseMatch,
            source: AlignmentSource::Computed {
                method: "embedding-cosine".to_string(),
            },
            confidence: 0.62,
            lossy_reverse: true,
        };
        let subject = alignment.subject();
        let flakes = alignment_to_flakes(&alignment, 1);

        let get = |predicate: &str| {
            flakes
                .iter()
                .find(|f| f.s == subject && f.p == Sid::dsc(predicate))
                .unwrap_or_else(|| panic!("missing {predicate}: {flakes:#?}"))
        };
        assert_eq!(
            get("alignmentSourceKind").o,
            FlakeValue::String("computed".to_string())
        );
        assert_eq!(
            get("alignmentSourceDetail").o,
            FlakeValue::String("embedding-cosine".to_string())
        );
        assert_eq!(get("confidence").o, FlakeValue::Float(0.62));
        assert_eq!(get("lossyReverse").o, FlakeValue::Boolean(true));
    }

    /// The subject a computed alignment writes to and the subject a
    /// curated alignment of the *same* (left, predicate, right) writes to
    /// must be identical — otherwise a later curated ingestion could never
    /// find and supersede an earlier computed guess, and decision 1
    /// ("curated before computed, always") would have nothing to act on.
    #[test]
    fn the_same_left_predicate_right_always_names_the_same_subject_regardless_of_source() {
        let computed = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Computed {
                method: "embedding".to_string(),
            },
            confidence: 0.9,
            lossy_reverse: false,
        };
        let curated = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert_eq!(computed.subject(), curated.subject());
    }

    /// And the inverse: a genuinely different alignment must not collide.
    #[test]
    fn a_different_pair_names_a_different_subject() {
        let a = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        let b = Alignment::Match {
            left: cui("C0000001"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert_ne!(a.subject(), b.subject());
    }

    /// Re-ingesting the identical row twice (a resume after interruption,
    /// Slice B) must produce byte-identical flakes, not merely an
    /// equivalent-looking set — the acceptance criterion is "the result
    /// must equal the uninterrupted run", checked by equality.
    #[test]
    fn ingesting_the_same_alignment_twice_produces_identical_flakes() {
        let build = || Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Curated {
                authority: "UMLS".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert_eq!(
            alignment_to_flakes(&build(), 1),
            alignment_to_flakes(&build(), 1)
        );
    }
}
