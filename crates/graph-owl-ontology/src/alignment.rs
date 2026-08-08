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

    /// This alignment's own confidence — decision 4's gating input.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        match self {
            Self::Match { confidence, .. } | Self::EquivalentClass { confidence, .. } => {
                *confidence
            }
        }
    }

    /// Whether a human directly confirmed this alignment — decision:
    /// "a later automated run does not overwrite it". Only
    /// [`AlignmentSource::Human`] and [`AssertableSource::Human`] count;
    /// `Curated` is trustworthy (decision 1) but is not the same claim as
    /// "a person looked at this specific alignment and confirmed it".
    #[must_use]
    pub fn is_human_confirmed(&self) -> bool {
        match self {
            Self::Match {
                source: AlignmentSource::Human { .. },
                ..
            }
            | Self::EquivalentClass {
                source: AssertableSource::Human { .. },
                ..
            } => true,
            Self::Match { .. } | Self::EquivalentClass { .. } => false,
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
    ///
    /// **`Sid`'s own compact `Display` form (`namespace_code:id`), not
    /// [`Sid::to_iri`].** A real IRI carries its own `#` (every namespace
    /// this crate uses is a `#`-delimited vocabulary), and concatenating
    /// three of them into one `dsc:`-namespaced local name produces a
    /// string with more than one fragment delimiter — syntactically
    /// invalid as an IRI, which surfaces only once something actually
    /// tries to convert this subject to an RDF term (a real end-to-end
    /// query, not `alignment_to_flakes`'s own unit tests, is what caught
    /// this).
    #[must_use]
    pub fn subject(&self) -> Sid {
        let (left, right, predicate, ..) = self.parts();
        alignment_subject_for(left, &predicate, right)
    }
}

/// [`Alignment::subject`]'s formula, exposed standalone for the inverse
/// direction: given a direct triple *already read from the graph*
/// (`left {predicate} right`), find the reified metadata node describing
/// it — without constructing a full [`Alignment`] value, which needs a
/// source and confidence a caller reading the graph may not have in hand.
///
/// Used by `graph-owl-api`'s query-time alignment attribution (Epic 104's
/// console acceptance criterion: "on any cross-vocabulary result the
/// alignment that made it reachable is inspectable") — a SPARQL result that
/// crossed this triple looks up its metadata node directly, rather than
/// re-deriving this same format string a second time.
#[must_use]
pub fn alignment_subject_for(left: &Sid, predicate: &Sid, right: &Sid) -> Sid {
    Sid::dsc(format!("alignment:{left}:{predicate}:{right}"))
}

/// Every predicate an alignment's own direct triple can be written under —
/// the four `skos:*Match` variants plus `owl:equivalentClass`. Used to
/// recognise, among facts a query already scanned, which ones are alignment
/// edges at all.
#[must_use]
pub fn known_alignment_predicates() -> [Sid; 5] {
    [
        MatchPredicate::ExactMatch.sid(),
        MatchPredicate::CloseMatch.sid(),
        MatchPredicate::BroadMatch.sid(),
        MatchPredicate::NarrowMatch.sid(),
        Sid::new(namespace::OWL, "equivalentClass"),
    ]
}

/// The **reified metadata node**'s own flakes — source, confidence and
/// directionality — never the direct semantic triple. Split out from
/// [`alignment_to_flakes`] so [`alignment_to_flakes_gated`] can write
/// metadata for a `Surface`-disposition alignment (what a review queue
/// reads) while withholding the direct triple.
fn metadata_flakes(alignment: &Alignment, t: i64) -> Vec<Flake> {
    let (left, right, predicate, source, confidence, lossy_reverse) = alignment.parts();
    let subject = alignment.subject();

    vec![
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
        // **Found while building Epic 42's alignment review queue (Phase 3
        // item 3.14).** `Alignment::subject()` already encodes the
        // predicate in its own compound local name, but nothing wrote it
        // as its own readable flake — a review-queue reader had
        // `left`/`right`/`source`/`confidence` but no way to reconstruct
        // which `skos:*Match` a "confirm" action should resubmit, short of
        // parsing a compound subject string.
        Flake::assert(
            subject.clone(),
            Sid::dsc("alignmentPredicate"),
            FlakeValue::String(predicate.id.clone()),
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

/// The **direct semantic triple** alone (`left {predicate} right`, e.g.
/// `left skos:exactMatch right`) — the "graph edge" a plain SPARQL query
/// traverses with no special handling.
///
/// **Public**, unlike [`metadata_flakes`]: a caller upserting this
/// alignment needs to know this triple's exact `(s, p, o)` to find and
/// retract a *stale* one — e.g. a previous run wrote it at `Assert`
/// disposition and this run's confidence dropped into `Surface` — and
/// `(left, predicate, right)` alone, not this function, is what a
/// `TriplePattern` query needs.
#[must_use]
pub fn direct_triple(alignment: &Alignment, t: i64) -> Flake {
    let (left, right, predicate, ..) = alignment.parts();
    Flake::assert(left.clone(), predicate, FlakeValue::Ref(right.clone()), t)
}

/// This alignment as flakes, at transaction time `t`, **ungated**.
///
/// Writes both the direct triple and the reified metadata node
/// unconditionally. Decision 4's confidence-band gating (whether a
/// computed alignment's direct triple should be withheld pending review)
/// is not this function's concern — see [`alignment_to_flakes_gated`] for
/// that. Kept as the simple, always-both primitive because Slice B's
/// curated UMLS ingestion has nothing to gate: `AlignmentSource::Curated`
/// is definitionally trustworthy (decision 1), and gating it against a
/// numeric threshold would be gating a decision already made.
#[must_use]
pub fn alignment_to_flakes(alignment: &Alignment, t: i64) -> Vec<Flake> {
    let mut flakes = metadata_flakes(alignment, t);
    flakes.push(direct_triple(alignment, t));
    flakes
}

/// This alignment as flakes, respecting decision 4's confidence bands —
/// `graph_owl_core::extraction::Disposition`, `00c-domain-model.md`'s
/// generic bands reused rather than re-derived: `≥0.8` writes everything
/// (the direct triple included), `0.5..0.8` writes only the metadata (so
/// a review-queue listing can find it, but the direct triple never
/// "quietly becomes a graph edge" — the plan's own words), and `<0.5`
/// writes nothing at all.
#[must_use]
pub fn alignment_to_flakes_gated(alignment: &Alignment, t: i64) -> Vec<Flake> {
    use graph_owl_core::extraction::Disposition;

    let confidence = alignment.confidence();
    match Disposition::for_confidence(confidence) {
        Disposition::Assert => alignment_to_flakes(alignment, t),
        Disposition::Surface => metadata_flakes(alignment, t),
        Disposition::Ignore => Vec::new(),
    }
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

    /// **Found while building Epic 42's alignment review queue (Phase 3
    /// item 3.14) — a real, pre-existing gap.** `Alignment::subject()`
    /// itself encodes the predicate (`alignment:{left}:{predicate}:{right}`),
    /// but nothing wrote it as its *own* readable flake, so a review-queue
    /// reader with `left`/`right`/`source`/`confidence` in hand still had
    /// no way to reconstruct which `skos:*Match` a "confirm" action should
    /// resubmit — short of parsing a compound subject string, which is
    /// exactly the kind of fragile string-splitting this project's own
    /// flake model exists to avoid.
    #[test]
    fn to_flakes_writes_the_match_predicate_itself_on_the_reified_node() {
        let alignment = Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::CloseMatch,
            source: AlignmentSource::Computed {
                method: "embedding-cosine".to_string(),
            },
            confidence: 0.62,
            lossy_reverse: false,
        };
        let subject = alignment.subject();
        let flakes = alignment_to_flakes(&alignment, 1);

        let predicate_flake = flakes
            .iter()
            .find(|f| f.s == subject && f.p == Sid::dsc("alignmentPredicate"))
            .unwrap_or_else(|| panic!("missing alignmentPredicate: {flakes:#?}"));
        assert_eq!(
            predicate_flake.o,
            FlakeValue::String("closeMatch".to_string()),
            "must name the real predicate, not always exactMatch"
        );
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

    /// **Regression, found only by a real end-to-end query, not by any
    /// test in this file.** An earlier version built `subject()` from
    /// `Sid::to_iri()` — each of `left`/`predicate`/`right` is already a
    /// real, `#`-delimited IRI, so concatenating three of them into one
    /// `dsc:`-namespaced local name produced a string with more than one
    /// fragment delimiter: syntactically invalid as an IRI. Every
    /// alignment's reified subject must itself be convertible to a real
    /// IRI, not just constructible as a `Sid` — this is exactly the
    /// distinction the earlier bug fell through, since `Sid` construction
    /// never validates its own local name against the namespace's syntax.
    #[test]
    fn the_reified_subject_is_itself_a_valid_iri() {
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
        let subject = alignment.subject();
        let iri = subject.to_iri().expect("has an IRI at all");
        assert_eq!(
            iri.matches('#').count(),
            1,
            "more than one fragment delimiter is not a valid IRI: {iri}"
        );
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

    fn computed(confidence: f64) -> Alignment {
        Alignment::Match {
            left: cui("C0009044"),
            right: snomed("22298006"),
            predicate: MatchPredicate::CloseMatch,
            source: AlignmentSource::Computed {
                method: "embedding".to_string(),
            },
            confidence,
            lossy_reverse: false,
        }
    }

    /// Decision 4, `>= 0.8`: a confident computed alignment is written in
    /// full — the direct triple *and* the metadata — indistinguishable
    /// from a curated one at query time.
    #[test]
    fn a_confident_alignment_writes_the_direct_triple() {
        let alignment = computed(0.9);
        let gated = alignment_to_flakes_gated(&alignment, 1);
        assert_eq!(gated, alignment_to_flakes(&alignment, 1));
        assert!(
            gated
                .iter()
                .any(|f| f.s == cui("C0009044") && f.p == MatchPredicate::CloseMatch.sid()),
            "{gated:#?}"
        );
    }

    /// **The acceptance criterion, verbatim.** "A computed alignment at
    /// 0.62 goes to a review queue; it does not quietly become a graph
    /// edge" — the direct triple must be absent, while the metadata (which
    /// a review-queue listing reads) must still be written.
    #[test]
    fn a_review_band_alignment_withholds_the_direct_triple_but_keeps_metadata() {
        let alignment = computed(0.62);
        let gated = alignment_to_flakes_gated(&alignment, 1);

        assert!(
            !gated
                .iter()
                .any(|f| f.s == cui("C0009044") && f.p == MatchPredicate::CloseMatch.sid()),
            "the direct triple must not quietly become a graph edge: {gated:#?}"
        );
        let subject = alignment.subject();
        assert!(
            gated
                .iter()
                .any(|f| f.s == subject && f.p == Sid::dsc("confidence")),
            "the review queue must still be able to find it: {gated:#?}"
        );
    }

    /// Decision 4, `< 0.5`: not recorded at all — no direct triple, no
    /// metadata, nothing for a review queue to even list.
    #[test]
    fn a_low_confidence_alignment_writes_nothing() {
        let gated = alignment_to_flakes_gated(&computed(0.3), 1);
        assert!(gated.is_empty(), "{gated:#?}");
    }

    /// The exact boundaries decision 4 states, both inclusive on their
    /// upper side — matching `graph_owl_core::extraction::Disposition`'s
    /// own documented boundary semantics, which this reuses rather than
    /// re-deriving a second copy of `00c`'s bands.
    #[test]
    fn the_boundaries_are_inclusive_on_their_upper_side() {
        assert!(!alignment_to_flakes_gated(&computed(0.8), 1).is_empty());
        assert!(
            alignment_to_flakes_gated(&computed(0.8), 1)
                .iter()
                .any(|f| f.p == MatchPredicate::CloseMatch.sid()),
            "0.8 must assert, not merely surface"
        );
        assert!(!alignment_to_flakes_gated(&computed(0.5), 1).is_empty());
        assert!(alignment_to_flakes_gated(&computed(0.499_999), 1).is_empty());
    }

    /// [`alignment_subject_for`] must agree with [`Alignment::subject`] on
    /// the identical triple — it is the same formula, exposed for a caller
    /// that has only read a direct triple off the graph and has no
    /// `Alignment` value to build.
    #[test]
    fn alignment_subject_for_agrees_with_alignment_subject() {
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
            alignment_subject_for(
                &cui("C0009044"),
                &MatchPredicate::ExactMatch.sid(),
                &snomed("22298006")
            ),
            alignment.subject()
        );
    }

    /// The inverse direction from a different left/right must not collide —
    /// the same property [`a_different_pair_names_a_different_subject`]
    /// pins on [`Alignment::subject`] itself.
    #[test]
    fn alignment_subject_for_differs_for_a_different_pair() {
        let a = alignment_subject_for(
            &cui("C0009044"),
            &MatchPredicate::ExactMatch.sid(),
            &snomed("22298006"),
        );
        let b = alignment_subject_for(
            &cui("C0000001"),
            &MatchPredicate::ExactMatch.sid(),
            &snomed("22298006"),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn known_alignment_predicates_names_the_four_skos_matches_and_equivalent_class() {
        let predicates = known_alignment_predicates();
        assert!(predicates.contains(&MatchPredicate::ExactMatch.sid()));
        assert!(predicates.contains(&MatchPredicate::CloseMatch.sid()));
        assert!(predicates.contains(&MatchPredicate::BroadMatch.sid()));
        assert!(predicates.contains(&MatchPredicate::NarrowMatch.sid()));
        assert!(predicates.contains(&Sid::new(namespace::OWL, "equivalentClass")));
        // An ordinary catalog predicate must never be mistaken for an
        // alignment edge — the mutator this guards against is the "always
        // true" membership check.
        assert!(!predicates.contains(&Sid::dsc("name")));
    }

    #[test]
    fn only_a_human_source_is_human_confirmed() {
        let human_match = Alignment::Match {
            left: cui("C1"),
            right: snomed("1"),
            predicate: MatchPredicate::ExactMatch,
            source: AlignmentSource::Human {
                principal: "asha".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert!(human_match.is_human_confirmed());

        let human_equiv = Alignment::EquivalentClass {
            left: cui("C1"),
            right: snomed("1"),
            source: AssertableSource::Human {
                principal: "asha".to_string(),
            },
            confidence: 1.0,
            lossy_reverse: false,
        };
        assert!(human_equiv.is_human_confirmed());

        assert!(!computed(0.9).is_human_confirmed());
        assert!(
            !Alignment::Match {
                left: cui("C1"),
                right: snomed("1"),
                predicate: MatchPredicate::ExactMatch,
                source: AlignmentSource::Curated {
                    authority: "UMLS".to_string()
                },
                confidence: 1.0,
                lossy_reverse: false,
            }
            .is_human_confirmed(),
            "curated is trustworthy but is not the same claim as a human confirming *this* alignment"
        );
    }
}
