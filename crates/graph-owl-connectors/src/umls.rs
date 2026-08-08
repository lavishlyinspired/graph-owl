//! UMLS RRF ingestion — Epic 104 Slice B.
//!
//! **The RRF reader is a connector module, not a serialization format**
//! (`plans/104-ontology-alignment.md`'s own framing): UMLS ships
//! pipe-delimited `MRCONSO`/`MRREL`/`MRSTY` files, not OWL, so this
//! belongs beside every other external system this crate reads, not in
//! `graph-owl-rdf-io`, whose job is W3C serializations.
//!
//! Format verified against the UMLS Reference Manual (NCBI Bookshelf,
//! `NBK9685`), 7 August 2026: `MRCONSO.RRF` is 18 pipe-delimited fields
//! per row, no header row, each row terminated by a trailing `|`. RRF
//! carries **no quoting** — a plain split, not a CSV parser reconfigured
//! for a different delimiter, which would apply escaping rules this
//! format never had.

use graph_owl_core::flake::{Flake, Sid, namespace};
use graph_owl_ontology::alignment::{
    Alignment, AlignmentSource, MatchPredicate, alignment_to_flakes,
};

/// One `MRCONSO.RRF` row, the fields this system uses.
///
/// Not every column: `TS`/`LUI`/`STT`/`SUI`/`AUI`/`SAUI`/`SCUI`/`SDUI`/
/// `TTY`/`SRL`/`CVF` describe the atom's own identity inside UMLS's
/// internal model, which this system has no use for. It only needs the
/// concept (`CUI`), language (`LAT`), the source-vocabulary identity
/// (`SAB`, `CODE`, `STR`), and whether the row is suppressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MrconsoAtom {
    pub cui: String,
    pub lat: String,
    pub sab: String,
    pub code: String,
    pub str_: String,
    /// `O` obsolete, `E` editorially suppressed, `Y` suppressed, `N`
    /// active. Only `N` rows become alignments — see [`atom_to_alignment`].
    pub suppress: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MrconsoParseError {
    #[error("row {line}: expected 18 pipe-delimited fields (MRCONSO.RRF), found {found}")]
    WrongFieldCount { line: u64, found: usize },
}

/// Parse one `MRCONSO.RRF` line. `line_number` is 1-based and only used to
/// name the row in a parse error — a client greps their file with it.
///
/// # Errors
///
/// [`MrconsoParseError::WrongFieldCount`] if the line does not carry
/// RRF's 18 fields.
pub fn parse_mrconso_line(line: &str, line_number: u64) -> Result<MrconsoAtom, MrconsoParseError> {
    // RRF terminates every row with a trailing `|`, so a naive split of an
    // 18-field row yields 19 parts with the last always empty. Trimming it
    // once here is what keeps every field index below from being off by one.
    let trimmed = line.strip_suffix('|').unwrap_or(line);
    let fields: Vec<&str> = trimmed.split('|').collect();
    if fields.len() != 18 {
        return Err(MrconsoParseError::WrongFieldCount {
            line: line_number,
            found: fields.len(),
        });
    }
    Ok(MrconsoAtom {
        cui: fields[0].to_string(),
        lat: fields[1].to_string(),
        sab: fields[11].to_string(),
        code: fields[13].to_string(),
        str_: fields[14].to_string(),
        suppress: fields[16].to_string(),
    })
}

/// The namespace a `SAB` (source vocabulary abbreviation) maps to, for the
/// vocabularies this system has a verified IRI for.
///
/// **Deliberately partial.** UMLS names ~190 source vocabularies (`SAB`
/// column); this system has verified real, authoritative IRIs for exactly
/// two (`graph_owl_core::flake::namespace::SNOMED_CT`, `::RXNORM`). Adding
/// a namespace for a `SAB` this function does not yet recognise means
/// verifying its IRI first — inventing one to widen coverage would risk
/// producing an IRI nobody checked, which is exactly what this project's
/// licensing and standards discipline refuses to do silently.
#[must_use]
pub fn source_namespace(sab: &str) -> Option<u16> {
    match sab {
        "SNOMEDCT_US" => Some(namespace::SNOMED_CT),
        "RXNORM" => Some(namespace::RXNORM),
        _ => None,
    }
}

/// This atom's alignment fact, if its `SAB` is one [`source_namespace`]
/// recognises and it is not suppressed.
///
/// `None`, not an error: an unsupported `SAB` or a suppressed row is an
/// expected, common shape of real UMLS data (per SUPPRESS's own
/// documented meaning — `O`/`E`/`Y` all mean "not current"), not a
/// malformed one.
#[must_use]
pub fn atom_to_alignment(atom: &MrconsoAtom) -> Option<Alignment> {
    if atom.suppress != "N" {
        return None;
    }
    let namespace = source_namespace(&atom.sab)?;
    Some(Alignment::Match {
        left: Sid::new(namespace::CUI, atom.cui.clone()),
        right: Sid::new(namespace, atom.code.clone()),
        predicate: MatchPredicate::ExactMatch,
        // Decision 1: UMLS's CUI is curated by the NLM, not computed by
        // this system — "UMLS" names the curating body, not the
        // individual source vocabulary the atom happened to come from.
        source: AlignmentSource::Curated {
            authority: "UMLS".to_string(),
        },
        confidence: 1.0,
        // Not decision 6's concern: a CUI-to-atom membership is what
        // MRCONSO states directly, not a derived cross-map between two
        // source vocabularies (Slice C's job). Neither direction loses
        // information MRCONSO itself carries.
        lossy_reverse: false,
    })
}

/// How far one [`ingest_mrconso`] call got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IngestProgress {
    /// Rows read **by this call** — not cumulative across calls, so a
    /// caller resuming from a checkpoint can tell "did this call actually
    /// skip the already-processed prefix" from the count alone.
    pub rows_processed: u64,
    pub aligned: u64,
    pub skipped: u64,
    pub errors: u64,
}

/// Parse `MRCONSO.RRF` lines into raw alignments, skipping the first `skip`
/// lines.
///
/// **Resumable by construction, not by tracked cross-row state.** Every
/// row maps to its own self-contained alignment — [`atom_to_alignment`]
/// never reads a previous row — so "skip `N`, continue" and "process from
/// the start" produce the identical set of alignments once every row has
/// been seen exactly once between the two calls. The caller owns
/// persisting `skip` between calls; this function is pure, no I/O, and
/// does not know whether it is the first call or a resume.
///
/// **This, not [`ingest_mrconso`], is what a caller with no local graph to
/// write into needs** — `graph-owl-cli` (Phase 3 item 3.13/4.8) has no
/// direct engine access by design (decision 6, `20-metadata-as-code.md`),
/// so it submits each `Alignment` individually over the existing, already
/// human-confirmation-protected `POST /alignments`, never pre-converted
/// flakes it has nowhere local to write.
#[must_use]
pub fn mrconso_alignments<'a>(
    lines: impl Iterator<Item = &'a str>,
    skip: u64,
) -> (Vec<Alignment>, IngestProgress) {
    let mut alignments = Vec::new();
    let mut progress = IngestProgress::default();

    for (index, line) in lines.enumerate() {
        let line_number = index as u64 + 1;
        if line_number <= skip {
            continue;
        }
        progress.rows_processed += 1;
        match parse_mrconso_line(line, line_number) {
            Err(_) => progress.errors += 1,
            Ok(atom) => match atom_to_alignment(&atom) {
                None => progress.skipped += 1,
                Some(alignment) => {
                    alignments.push(alignment);
                    progress.aligned += 1;
                }
            },
        }
    }

    (alignments, progress)
}

/// Ingest `MRCONSO.RRF` lines into alignment flakes, skipping the first
/// `skip` lines — [`mrconso_alignments`] plus the flake conversion, for a
/// caller with direct graph-engine access.
#[must_use]
pub fn ingest_mrconso<'a>(
    lines: impl Iterator<Item = &'a str>,
    skip: u64,
    t: i64,
) -> (Vec<Flake>, IngestProgress) {
    let (alignments, progress) = mrconso_alignments(lines, skip);
    let flakes = alignments
        .iter()
        .flat_map(|alignment| alignment_to_flakes(alignment, t))
        .collect();
    (flakes, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real line, verified via a live fetch of the UMLS Reference
    /// Manual's own MRCONSO.RRF example, 7 August 2026 — not invented.
    const REAL_MRCONSO_LINE: &str = "C0001175|ENG|P|L0001175|VO|S0010340|Y|A0019182||M0000245|D000163|MSH|PM|D000163|Acquired Immunodeficiency Syndromes|0|N||";

    fn snomed_line(cui: &str, code: &str, suppress: &str) -> String {
        format!(
            "{cui}|ENG|P|L1|PF|S1|Y|A1||{code}||SNOMEDCT_US|PT|{code}|Some SNOMED term|0|{suppress}||"
        )
    }

    fn rxnorm_line(cui: &str, code: &str) -> String {
        format!("{cui}|ENG|P|L1|PF|S1|Y|A1||{code}||RXNORM|IN|{code}|Some RxNorm drug|0|N||")
    }

    #[test]
    fn a_real_mrconso_line_parses_into_its_documented_fields() {
        let atom = parse_mrconso_line(REAL_MRCONSO_LINE, 1).expect("real line parses");
        assert_eq!(atom.cui, "C0001175");
        assert_eq!(atom.lat, "ENG");
        assert_eq!(atom.sab, "MSH");
        assert_eq!(atom.code, "D000163");
        assert_eq!(atom.str_, "Acquired Immunodeficiency Syndromes");
        assert_eq!(atom.suppress, "N");
    }

    #[test]
    fn a_line_with_the_wrong_field_count_is_refused_by_name() {
        let err = parse_mrconso_line("C0001175|ENG|only three", 42).unwrap_err();
        assert_eq!(
            err,
            MrconsoParseError::WrongFieldCount { line: 42, found: 3 }
        );
    }

    #[test]
    fn a_snomed_atom_aligns_to_its_cui_as_curated() {
        let atom = parse_mrconso_line(&snomed_line("C0009044", "22298006", "N"), 1).unwrap();
        let alignment = atom_to_alignment(&atom).expect("SNOMEDCT_US is supported");
        assert_eq!(
            alignment,
            Alignment::Match {
                left: Sid::new(namespace::CUI, "C0009044"),
                right: Sid::new(namespace::SNOMED_CT, "22298006"),
                predicate: MatchPredicate::ExactMatch,
                source: AlignmentSource::Curated {
                    authority: "UMLS".to_string()
                },
                confidence: 1.0,
                lossy_reverse: false,
            }
        );
    }

    #[test]
    fn an_rxnorm_atom_aligns_to_its_cui_as_curated() {
        let atom = parse_mrconso_line(&rxnorm_line("C0004057", "9801"), 1).unwrap();
        let alignment = atom_to_alignment(&atom).expect("RXNORM is supported");
        assert_eq!(
            alignment,
            Alignment::Match {
                left: Sid::new(namespace::CUI, "C0004057"),
                right: Sid::new(namespace::RXNORM, "9801"),
                predicate: MatchPredicate::ExactMatch,
                source: AlignmentSource::Curated {
                    authority: "UMLS".to_string()
                },
                confidence: 1.0,
                lossy_reverse: false,
            }
        );
    }

    /// The honest gap: MSH (`MeSH`) has no verified namespace, so this must
    /// be skipped, not silently mapped to an unverified IRI.
    #[test]
    fn an_unsupported_source_vocabulary_is_skipped_not_guessed() {
        let atom = parse_mrconso_line(REAL_MRCONSO_LINE, 1).unwrap();
        assert_eq!(atom.sab, "MSH");
        assert_eq!(atom_to_alignment(&atom), None);
    }

    #[test]
    fn a_suppressed_atom_is_not_aligned() {
        for suppress in ["O", "E", "Y"] {
            let atom = parse_mrconso_line(&snomed_line("C1", "1", suppress), 1).unwrap();
            assert_eq!(
                atom_to_alignment(&atom),
                None,
                "suppress={suppress} must not align"
            );
        }
    }

    fn synthetic_file(rows: usize) -> Vec<String> {
        (0..rows)
            .map(|i| snomed_line(&format!("C{i:07}"), &format!("{i:07}"), "N"))
            .collect()
    }

    #[test]
    fn ingest_reports_progress_and_produces_flakes_for_every_row() {
        let file = synthetic_file(10);
        let (flakes, progress) = ingest_mrconso(file.iter().map(String::as_str), 0, 1);
        assert_eq!(progress.rows_processed, 10);
        assert_eq!(progress.aligned, 10);
        assert_eq!(progress.errors, 0);
        // 8 flakes per alignment: the direct triple + 7 reified metadata
        // flakes (see alignment_to_flakes).
        assert_eq!(flakes.len(), 80, "{flakes:#?}");
    }

    /// `synthetic_file` alone never exercises the skip-or-error counters —
    /// every row it produces is a supported, unsuppressed SNOMED atom, so
    /// `ingest_reports_progress_and_produces_flakes_for_every_row` never
    /// touches `progress.skipped`/`progress.errors` at all. This is the
    /// direct RED test for both counters at the `ingest_mrconso` level,
    /// not just at `atom_to_alignment`'s own.
    #[test]
    fn ingest_counts_skipped_and_errored_rows_separately_from_aligned_ones() {
        let mut file = synthetic_file(2); // 2 aligned SNOMED rows
        file.push(REAL_MRCONSO_LINE.to_string()); // 1 skipped: SAB=MSH
        file.push("too|few|fields".to_string()); // 1 errored

        let (flakes, progress) = ingest_mrconso(file.iter().map(String::as_str), 0, 1);

        assert_eq!(progress.rows_processed, 4, "{progress:?}");
        assert_eq!(progress.aligned, 2, "{progress:?}");
        assert_eq!(progress.skipped, 1, "{progress:?}");
        assert_eq!(progress.errors, 1, "{progress:?}");
        assert_eq!(
            flakes.len(),
            16,
            "only the 2 aligned rows write flakes: {flakes:#?}"
        );
    }

    /// **The acceptance criterion.** Interrupting at 80% and resuming from
    /// a checkpoint must produce the identical result as an uninterrupted
    /// run — proven two ways, per the plan's own mutator watch: the
    /// *data* must match (a resume that silently restarted from zero
    /// would happen to pass this alone, since alignment flakes are
    /// idempotent per row) **and** the *row count actually processed*
    /// during the resume must be exactly the remainder, not the whole
    /// file again.
    #[test]
    fn resuming_after_an_interruption_at_80_percent_equals_an_uninterrupted_run() {
        let file = synthetic_file(100);
        let checkpoint = 80;

        let (full_flakes, full_progress) = ingest_mrconso(file.iter().map(String::as_str), 0, 1);
        assert_eq!(full_progress.rows_processed, 100);

        // Simulate the interrupted run: process only the first 80 rows.
        let file_prefix: Vec<&str> = file.iter().take(checkpoint).map(String::as_str).collect();
        let (first_half, first_progress) = ingest_mrconso(file_prefix.into_iter(), 0, 1);
        assert_eq!(first_progress.rows_processed, 80);

        // Resume: same full file, skip the first 80 lines this time.
        let (second_half, resume_progress) =
            ingest_mrconso(file.iter().map(String::as_str), checkpoint as u64, 1);

        // The row-count assertion the plan's mutator watch names: a resume
        // that restarted from zero would report 100 here, not 20.
        assert_eq!(
            resume_progress.rows_processed, 20,
            "a resume must process only the remainder, not the whole file again"
        );

        let mut combined: Vec<Flake> = first_half.into_iter().chain(second_half).collect();
        let mut expected = full_flakes;
        let sort_key = |f: &Flake| format!("{f:?}");
        combined.sort_by_key(sort_key);
        expected.sort_by_key(sort_key);
        assert_eq!(
            combined, expected,
            "resumed result must equal the uninterrupted run"
        );
    }

    /// Phase 3 item 3.13/4.8: `mrconso_alignments` is what
    /// `graph-owl-cli`'s new `umls-ingest` subcommand calls — it has no
    /// direct engine access (decision 6, `20-metadata-as-code.md`), so it
    /// needs the raw `Alignment`s to submit one at a time over the existing
    /// `POST /alignments`, not pre-converted flakes with nowhere local to
    /// write them.
    #[test]
    fn mrconso_alignments_yields_the_same_alignments_ingest_mrconso_would_flake() {
        let file = synthetic_file(3);

        let (alignments, progress) = mrconso_alignments(file.iter().map(String::as_str), 0);

        assert_eq!(progress.rows_processed, 3);
        assert_eq!(progress.aligned, 3);
        assert_eq!(alignments.len(), 3);
        assert_eq!(
            alignments[0],
            Alignment::Match {
                left: Sid::new(namespace::CUI, "C0000000"),
                right: Sid::new(namespace::SNOMED_CT, "0000000"),
                predicate: MatchPredicate::ExactMatch,
                source: AlignmentSource::Curated {
                    authority: "UMLS".to_string()
                },
                confidence: 1.0,
                lossy_reverse: false,
            }
        );
    }

    /// **Negative half.** A skipped or errored row must not appear as an
    /// alignment at all — the same counters `ingest_mrconso` already
    /// proves, now proven for the function the CLI actually calls.
    #[test]
    fn mrconso_alignments_omits_skipped_and_errored_rows() {
        let mut file = synthetic_file(1);
        file.push(REAL_MRCONSO_LINE.to_string()); // skipped: SAB=MSH
        file.push("too|few|fields".to_string()); // errored

        let (alignments, progress) = mrconso_alignments(file.iter().map(String::as_str), 0);

        assert_eq!(progress.rows_processed, 3);
        assert_eq!(progress.skipped, 1);
        assert_eq!(progress.errors, 1);
        assert_eq!(alignments.len(), 1, "{alignments:#?}");
    }

    /// **`ingest_mrconso` is unchanged behaviour, now built on top of
    /// `mrconso_alignments`.** Every flake test above still has to pass
    /// unmodified — this is the refactor's own regression guard.
    #[test]
    fn ingest_mrconso_still_flakes_what_mrconso_alignments_would_yield() {
        let file = synthetic_file(5);
        let (alignments, alignment_progress) =
            mrconso_alignments(file.iter().map(String::as_str), 0);
        let (flakes, flake_progress) = ingest_mrconso(file.iter().map(String::as_str), 0, 1);

        assert_eq!(alignment_progress, flake_progress);
        let expected_flakes: Vec<Flake> = alignments
            .iter()
            .flat_map(|a| alignment_to_flakes(a, 1))
            .collect();
        assert_eq!(flakes, expected_flakes);
    }
}
