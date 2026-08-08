//! UMLS RRF ingestion delivery — Phase 3 item 3.13, decision 4.8 (Epic 104).
//!
//! **Decision 4.8: a CLI subcommand**, not an admin-triggered background job
//! (no async job infrastructure exists anywhere in this codebase — 4.5 found
//! the identical gap) or the generic `/connectors/*` job framework (currently
//! entangled around `run_postgres_connector` alone — item 3.12's own, larger,
//! separate scope). `MRCONSO.RRF` files are typically gigabytes; this is
//! exactly the "streaming a multi-gigabyte file" shape `20-metadata-as-code.md`
//! names as the CLI's own boundary alongside `backup`/`restore`.
//!
//! **The parsing logic below is a deliberate, independently verified
//! duplicate of `graph_owl_connectors::umls`'s**, not a shared abstraction —
//! `graph-owl-connectors` unconditionally depends on `sqlx`, `tokio`,
//! `rdkafka` and `pulsar` for its other connector modules (Kafka, Pulsar,
//! Postgres introspection), and this crate's own Cargo.toml states the rule
//! this would break: "the CLI is a thin client... depending on the facade
//! would pull the storage adapter into a binary that must be able to run
//! against a remote instance." A CLI that gained a Kafka client and a SQL
//! driver to parse a pipe-delimited text file is a worse trade than ~30 lines
//! of duplication. Verified against the same UMLS Reference Manual (NCBI
//! Bookshelf, `NBK9685`) the original was checked against.
//!
//! **One `POST /alignments` per qualifying row, not a new bulk endpoint.**
//! `upsert_alignment` checks whether an existing alignment was human-confirmed
//! before overwriting it — a real governance guarantee this project already
//! makes, and a bulk-insert fast path bypassing it would silently let a UMLS
//! reload overwrite a human's correction. Slower, correct, and matching
//! `graph_owl_connectors::umls::mrconso_alignments`'s own doc comment: the
//! caller owns persisting `skip` between calls, because this was already
//! designed for a long-running, resumable, possibly multi-session import.

use std::io::BufRead;
use std::path::Path;

use graph_owl_core::flake::namespace;

/// The fields this module needs from one `MRCONSO.RRF` row — a subset of
/// `graph_owl_connectors::umls::MrconsoAtom`'s own fields, the ones
/// `atom_to_alignment_body` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MrconsoAtom {
    cui: String,
    sab: String,
    code: String,
    suppress: String,
}

/// Parses one `MRCONSO.RRF` line. `None` for a row that does not carry
/// RRF's 18 pipe-delimited fields — reported as an error count, not a
/// per-row detail, since delivery (not diagnosis) is this module's job.
fn parse_mrconso_line(line: &str) -> Option<MrconsoAtom> {
    let trimmed = line.strip_suffix('|').unwrap_or(line);
    let fields: Vec<&str> = trimmed.split('|').collect();
    if fields.len() != 18 {
        return None;
    }
    Some(MrconsoAtom {
        cui: fields[0].to_string(),
        sab: fields[11].to_string(),
        code: fields[13].to_string(),
        suppress: fields[16].to_string(),
    })
}

/// The namespace a `SAB` maps to, for the vocabularies this system has a
/// verified IRI for — deliberately partial, matching
/// `graph_owl_connectors::umls::source_namespace` exactly.
fn source_namespace(sab: &str) -> Option<u16> {
    match sab {
        "SNOMEDCT_US" => Some(namespace::SNOMED_CT),
        "RXNORM" => Some(namespace::RXNORM),
        _ => None,
    }
}

/// The `POST /alignments` request body for one atom, or `None` if it should
/// be skipped (suppressed, or an unsupported source vocabulary) — the wire
/// shape `graph-owl-server`'s `UpsertAlignmentRequest` deserializes, built
/// directly rather than through the domain `Alignment` type this crate does
/// not depend on.
fn atom_to_alignment_body(atom: &MrconsoAtom) -> Option<serde_json::Value> {
    if atom.suppress != "N" {
        return None;
    }
    let target_namespace = source_namespace(&atom.sab)?;
    Some(serde_json::json!({
        "kind": "match",
        "left": format!("{}:{}", namespace::CUI, atom.cui),
        "right": format!("{target_namespace}:{}", atom.code),
        "predicate": "exactMatch",
        "source": { "kind": "curated", "detail": "UMLS" },
        "confidence": 1.0,
        "lossyReverse": false,
    }))
}

/// How far one [`ingest`] call got — cumulative counts for **this call
/// only**, matching `graph_owl_connectors::umls::IngestProgress`'s own
/// convention, so a caller resuming from `last_line` can tell this call's
/// own contribution from the running total.
#[derive(Debug, Clone, Copy, Default)]
pub struct IngestSummary {
    pub rows_processed: u64,
    pub aligned: u64,
    pub skipped: u64,
    pub errors: u64,
    pub submitted: u64,
    /// The server refused the alignment — most often decision 3's own
    /// `RefusedHumanConfirmed` outcome (an existing alignment a human
    /// already vouched for), reported rather than aborting the run.
    pub refused: u64,
    /// The absolute line number this call finished at — `skip` for the
    /// next call to resume exactly where this one stopped.
    pub last_line: u64,
}

/// How often progress is reported to stderr — data goes to stdout, this is
/// a diagnostic, per this CLI's own conventions.
const PROGRESS_EVERY: u64 = 10_000;

/// Streams `input` (an `MRCONSO.RRF` file) into the catalog's alignment
/// store over `POST /alignments`, skipping the first `skip` lines.
///
/// # Errors
/// A failure opening or reading `input`, or a transport failure reaching
/// `server`. A non-success HTTP status for one alignment is **not** fatal —
/// counted in [`IngestSummary::refused`] — because one bad or protected row
/// aborting a multi-hour import would be worse than reporting it and moving
/// on.
/// 1-based `line_number` from a `BufRead::lines().enumerate()` index — the
/// arithmetic `ingest`'s loop needs, pulled out so it is checkable on its
/// own rather than only through a live file.
fn one_based_line_number(zero_based_index: usize) -> u64 {
    u64::try_from(zero_based_index).unwrap_or(u64::MAX) + 1
}

/// Whether `line_number` is at or before a resumed run's checkpoint — the
/// boundary the `--skip` flag exists to express. Pulled out of `ingest`'s
/// loop so the boundary itself is independently checkable: `ingest` also
/// makes a real network call per line, and mutation-testing that function
/// as a whole would only ever exercise this check through a live server,
/// which — matching `backup`/`restore`'s own accepted scope in this crate —
/// this module does not stand one up for. The boundary is the one piece
/// worth pulling free of that gap: get `<=` backwards here and `--skip`
/// does the opposite of what an operator resuming an interrupted import
/// asked for.
fn already_processed(line_number: u64, skip: u64) -> bool {
    line_number <= skip
}

pub fn ingest(
    server: &str,
    token: Option<&str>,
    input: &Path,
    skip: u64,
) -> Result<IngestSummary, String> {
    let file = std::fs::File::open(input).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);
    let client = reqwest::blocking::Client::new();
    let alignments_url = format!("{}/alignments", server.trim_end_matches('/'));

    let mut summary = IngestSummary::default();
    for (index, line) in reader.lines().enumerate() {
        let line_number = one_based_line_number(index);
        if already_processed(line_number, skip) {
            continue;
        }
        let line = line.map_err(|e| e.to_string())?;
        summary.rows_processed += 1;
        summary.last_line = line_number;

        let Some(atom) = parse_mrconso_line(&line) else {
            summary.errors += 1;
            continue;
        };
        let Some(body) = atom_to_alignment_body(&atom) else {
            summary.skipped += 1;
            continue;
        };

        let mut request = client.post(&alignments_url).json(&body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().map_err(|e| e.to_string())?;
        if response.status().is_success() {
            summary.submitted += 1;
        } else {
            summary.refused += 1;
        }

        if summary.rows_processed % PROGRESS_EVERY == 0 {
            eprintln!(
                "... line {}: {} submitted, {} refused, {} skipped, {} errors (resume with \
                 --skip {})",
                summary.last_line,
                summary.submitted,
                summary.refused,
                summary.skipped,
                summary.errors,
                summary.last_line
            );
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real line, verified against the UMLS Reference Manual's own
    /// `MRCONSO.RRF` example — matching
    /// `graph_owl_connectors::umls::tests::REAL_MRCONSO_LINE` exactly, so
    /// this module's independently-duplicated parser is checked against the
    /// identical fixture the original was.
    const REAL_MRCONSO_LINE: &str = "C0001175|ENG|P|L0001175|VO|S0010340|Y|A0019182||M0000245|D000163|MSH|PM|D000163|Acquired Immunodeficiency Syndromes|0|N||";

    fn snomed_line(cui: &str, code: &str, suppress: &str) -> String {
        format!(
            "{cui}|ENG|P|L1|PF|S1|Y|A1||{code}||SNOMEDCT_US|PT|{code}|Some SNOMED term|0|{suppress}||"
        )
    }

    #[test]
    fn the_first_line_of_a_fresh_file_is_index_zero_becoming_line_one() {
        assert_eq!(one_based_line_number(0), 1);
        assert_eq!(one_based_line_number(99), 100);
    }

    /// **The exact boundary the resume feature depends on.** `--skip 80`
    /// means "80 lines were already processed" — so line 80 itself must
    /// count as already done, and line 81 must not.
    #[test]
    fn a_line_at_the_checkpoint_counts_as_already_processed() {
        assert!(already_processed(80, 80), "the checkpoint line itself");
        assert!(already_processed(1, 80), "well before the checkpoint");
    }

    #[test]
    fn a_line_past_the_checkpoint_is_not_already_processed() {
        assert!(!already_processed(81, 80), "one past the checkpoint");
        assert!(
            !already_processed(1, 0),
            "no checkpoint at all: every line is new"
        );
    }

    #[test]
    fn a_real_mrconso_line_parses_into_its_documented_fields() {
        let atom = parse_mrconso_line(REAL_MRCONSO_LINE).expect("real line parses");
        assert_eq!(atom.cui, "C0001175");
        assert_eq!(atom.sab, "MSH");
        assert_eq!(atom.code, "D000163");
        assert_eq!(atom.suppress, "N");
    }

    #[test]
    fn a_line_with_the_wrong_field_count_does_not_parse() {
        assert_eq!(parse_mrconso_line("C0001175|ENG|only three"), None);
    }

    #[test]
    fn a_snomed_atom_becomes_a_curated_exact_match_body() {
        let atom = parse_mrconso_line(&snomed_line("C0009044", "22298006", "N")).unwrap();
        let body = atom_to_alignment_body(&atom).expect("SNOMEDCT_US is supported");
        assert_eq!(
            body,
            serde_json::json!({
                "kind": "match",
                "left": format!("{}:C0009044", namespace::CUI),
                "right": format!("{}:22298006", namespace::SNOMED_CT),
                "predicate": "exactMatch",
                "source": { "kind": "curated", "detail": "UMLS" },
                "confidence": 1.0,
                "lossyReverse": false,
            })
        );
    }

    /// **The honest gap, mirrored deliberately.** `MSH` has no verified
    /// namespace in this system (`graph_owl_connectors::umls::source_namespace`'s
    /// own doc explains why), so this must be skipped, not guessed.
    #[test]
    fn an_unsupported_source_vocabulary_produces_no_body() {
        let atom = parse_mrconso_line(REAL_MRCONSO_LINE).unwrap();
        assert_eq!(atom.sab, "MSH");
        assert_eq!(atom_to_alignment_body(&atom), None);
    }

    #[test]
    fn a_suppressed_atom_produces_no_body() {
        for suppress in ["O", "E", "Y"] {
            let atom = parse_mrconso_line(&snomed_line("C1", "1", suppress)).unwrap();
            assert_eq!(
                atom_to_alignment_body(&atom),
                None,
                "suppress={suppress} must not align"
            );
        }
    }

    /// **Mutator watch, the negative half.** A body built for the *wrong*
    /// namespace (e.g. always `SNOMED_CT` regardless of `sab`) would still
    /// pass every test above that only checks SNOMED — this proves
    /// `RxNorm` gets its own, different namespace code.
    #[test]
    fn an_rxnorm_atom_gets_the_rxnorm_namespace_not_snomeds() {
        let atom = parse_mrconso_line(
            "C0004057|ENG|P|L1|PF|S1|Y|A1||9801||RXNORM|IN|9801|Some RxNorm drug|0|N||",
        )
        .unwrap();
        let body = atom_to_alignment_body(&atom).expect("RXNORM is supported");
        assert_eq!(body["right"], format!("{}:9801", namespace::RXNORM));
        assert_ne!(
            namespace::RXNORM,
            namespace::SNOMED_CT,
            "sanity: distinct codes"
        );
    }
}
