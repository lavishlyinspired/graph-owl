//! Building and reading the portable archive — Epic 37b.
//!
//! `graph_owl_core::archive` defines *what* the format is; this module is
//! *how* it is written and read. Split from `Catalog`'s own export/restore
//! methods (in `lib.rs`, alongside every other facade method) because this
//! half is synchronous, blocking I/O — `tar` and `zstd` are both
//! `std::io::{Read,Write}`, not async — and keeping that boundary explicit
//! is what makes `Catalog::export_archive` safe to run the sync half inside
//! `spawn_blocking` without smuggling an async call across it.
//!
//! **Streams to a scratch directory on disk, not to memory.** `Catalog`'s
//! async half pages through storage and appends each page directly to an
//! NDJSON file, holding at most one page in RAM regardless of catalog size
//! (Slice A's own memory bound). Building a spec-correct `tar` entry needs
//! its size known before the header is written, which an unbounded stream
//! cannot promise without buffering it first — so this module buffers to
//! *disk*, not RAM, and tars the finished files in one blocking pass. The
//! acceptance criterion is a memory bound, not a disk one.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use graph_owl_core::archive::{ArchiveManifest, ArchivedEntity, ArchivedRelationship};

// The three files a scratch directory holds before they are tarred, and an
// archive holds after it is untarred. Named once so a typo cannot make the
// writer and the reader disagree about what a section is called.

/// The manifest section's filename inside the archive.
pub const MANIFEST_FILE: &str = "manifest.json";
/// The entities section's filename inside the archive.
pub const ENTITIES_FILE: &str = "entities.ndjson";
/// The relationships section's filename inside the archive.
pub const RELATIONSHIPS_FILE: &str = "relationships.ndjson";

/// What a restore actually did — Slice C's own "never a result
/// indistinguishable from a complete one". `aborted: true` means `Fail`
/// found a conflict and refused before writing a single row; every count
/// on that outcome is zero because nothing was written, not because
/// nothing was found.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    /// How many entities were written (created or, under `overwrite`,
    /// updated).
    pub entities_restored: u64,
    /// FQNs left untouched because they already existed and the policy was
    /// `skip`.
    pub entities_skipped: Vec<String>,
    /// How many relationships were written.
    pub relationships_restored: u64,
    /// Under `fail`, every FQN that already existed — populated only when
    /// [`Self::aborted`] is `true`.
    pub conflicts: Vec<String>,
    /// `true` when `fail` found a conflict and refused before writing
    /// anything.
    pub aborted: bool,
}

/// Appends one JSON value as a line — the shape every NDJSON section is
/// built from, on both the entities and relationships side.
///
/// # Errors
/// Any I/O failure opening or writing the file.
pub fn append_ndjson_line<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, value).map_err(|e| std::io::Error::other(e.to_string()))?;
    file.write_all(b"\n")
}

/// Tars `scratch_dir`'s three named files, zstd-compresses the result, and
/// writes it to `output_path`. Files are added in a fixed order —
/// manifest, entities, relationships — so two archives of the same
/// scratch-directory contents are byte-identical (Slice A's determinism
/// criterion): `tar::Builder` otherwise reflects whatever order the caller
/// appends in, and a directory walk is not a stable order across
/// filesystems.
///
/// # Errors
/// Any I/O failure reading the scratch files, writing the output, or
/// finishing either the tar or the zstd stream.
pub fn build_tar_zst(scratch_dir: &Path, output_path: &Path) -> std::io::Result<()> {
    let output = std::fs::File::create(output_path)?;
    let encoder = zstd::Encoder::new(output, 0)?.auto_finish();
    let mut tar = tar::Builder::new(encoder);
    for name in [MANIFEST_FILE, ENTITIES_FILE, RELATIONSHIPS_FILE] {
        let path = scratch_dir.join(name);
        // The relationships (and, for a scope matching nothing relational,
        // even the entities) file may never have been created — an archive
        // with a genuinely empty section is not corrupt, so its absence on
        // disk is treated as empty rather than an error.
        if path.exists() {
            tar.append_path_with_name(&path, name)?;
        }
    }
    tar.finish()
}

/// The inverse of [`build_tar_zst`]: decompresses and unpacks `archive_path`
/// into `scratch_dir`.
///
/// # Errors
/// Any I/O failure reading the archive, decompressing it, or writing the
/// extracted files — including a corrupt or truncated archive, which
/// `zstd`/`tar` surface as a decode error rather than partial output.
pub fn extract_tar_zst(archive_path: &Path, scratch_dir: &Path) -> std::io::Result<()> {
    let input = std::fs::File::open(archive_path)?;
    let decoder = zstd::Decoder::new(input)?;
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(scratch_dir)
}

/// SHA-256 of a scratch file's bytes, hex-encoded — the value that lands in
/// [`ArchiveManifest::section_checksums`]. `None` when the file was never
/// created (an empty section), matching [`build_tar_zst`]'s own treatment
/// of an absent file as an empty one rather than an error.
///
/// # Errors
/// Any I/O failure reading the file.
pub fn section_checksum(path: &Path) -> std::io::Result<Option<String>> {
    use sha2::{Digest, Sha256};
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(Some(format!("{:x}", hasher.finalize())))
}

/// Recomputes each section's checksum from the extracted scratch directory
/// and compares it against what the manifest claims — Slice B's "checksum
/// mismatch or truncation refuses before writing anything". Checked before
/// a single entity or relationship is restored, not interleaved with
/// writing them: validating a checksum after the fact would mean a
/// tampered archive gets partially applied before anyone finds out.
///
/// # Errors
/// An I/O failure reading a section, or a mismatch — named by which section
/// disagreed, since "the archive is corrupt" tells a restorer nothing they
/// can act on.
pub fn verify_section_checksums(
    scratch_dir: &Path,
    manifest: &ArchiveManifest,
) -> std::io::Result<()> {
    for (name, expected) in &manifest.section_checksums {
        let actual = section_checksum(&scratch_dir.join(name))?;
        if actual.as_deref() != Some(expected.as_str()) {
            return Err(std::io::Error::other(format!(
                "checksum mismatch on `{name}`: the archive is corrupt or was tampered with"
            )));
        }
    }
    Ok(())
}

/// Reads `manifest.json` out of an already-extracted scratch directory.
///
/// # Errors
/// An I/O failure, or a manifest that does not parse — both read as "not a
/// graph-owl archive" rather than a panic.
pub fn read_manifest(scratch_dir: &Path) -> std::io::Result<ArchiveManifest> {
    let bytes = std::fs::read(scratch_dir.join(MANIFEST_FILE))?;
    serde_json::from_slice(&bytes).map_err(|e| std::io::Error::other(e.to_string()))
}

/// Reads every archived entity out of an extracted scratch directory, in
/// file order — which is FQN order, since [`Catalog::export_archive`]
/// writes them sorted (parent-before-child, decision 6's restore ordering).
///
/// [`Catalog::export_archive`]: crate::Catalog::export_archive
///
/// # Errors
/// An I/O failure, or a line that does not parse as an [`ArchivedEntity`].
pub fn read_entities(scratch_dir: &Path) -> std::io::Result<Vec<ArchivedEntity>> {
    read_ndjson(&scratch_dir.join(ENTITIES_FILE))
}

/// Reads every archived relationship out of an extracted scratch directory.
///
/// # Errors
/// An I/O failure, or a line that does not parse as an
/// [`ArchivedRelationship`].
pub fn read_relationships(scratch_dir: &Path) -> std::io::Result<Vec<ArchivedRelationship>> {
    read_ndjson(&scratch_dir.join(RELATIONSHIPS_FILE))
}

fn read_ndjson<T: serde::de::DeserializeOwned>(path: &Path) -> std::io::Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)?;
    BufReader::new(file)
        .lines()
        .filter(|line| line.as_ref().is_ok_and(|l| !l.is_empty()))
        .map(|line| {
            let line = line?;
            serde_json::from_str(&line).map_err(|e| std::io::Error::other(e.to_string()))
        })
        .collect()
}

/// Redacts named fields on an asset in place — Slice E. Applied at export
/// time, before a line is ever written, so a redacted field never exists in
/// the archive's bytes to begin with (the byte-level guarantee the plan's
/// own RED test asks for — redacting on read would still leave it
/// recoverable from the file).
///
/// Only `description` is redactable today: it is the one free-text field an
/// [`ArchivedEntity`] carries. `fields` naming anything else is a no-op
/// rather than an error — an unknown field name in a redaction list is
/// almost always a typo the operator would want surfaced, but refusing the
/// whole export over it would be a worse failure than over-redacting
/// nothing.
pub fn redact_entity(entity: &mut ArchivedEntity, fields: &[String]) {
    if fields.iter().any(|f| f == "description") {
        entity.asset.description = None;
        for version in &mut entity.versions {
            version.snapshot.description = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::{Asset, AssetKind};

    fn scratch() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("graph-owl-archive-test-{}", uuid::Uuid::new_v4()))
    }

    fn asset(fqn: &str) -> Asset {
        let now = chrono::Utc::now();
        Asset {
            id: uuid::Uuid::new_v4(),
            kind: AssetKind::Table,
            name: fqn.to_string(),
            fully_qualified_name: fqn.to_string(),
            parent_id: None,
            description: Some("a secret detail".to_string()),
            properties: None,
            owners: Vec::new(),
            version: graph_owl_core::envelope::EntityVersion::initial(),
            updated_by: "test".to_string(),
            change_description: None,
            deleted: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
            extension: None,
            lifecycle: graph_owl_core::lifecycle::LifecycleState::default(),
            deprecation: None,
        }
    }

    #[test]
    fn a_tar_zst_archive_round_trips_every_section() {
        let dir = scratch();
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let manifest = ArchiveManifest {
            format_version: graph_owl_core::archive::FORMAT_VERSION,
            source_instance: "test".to_string(),
            created_at: chrono::Utc::now(),
            entity_count: 1,
            relationship_count: 0,
            scope: None,
            redacted_fields: vec![],
            section_checksums: std::collections::BTreeMap::new(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_vec(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");
        let entity = ArchivedEntity {
            asset: asset("a.b"),
            versions: vec![],
        };
        append_ndjson_line(&dir.join(ENTITIES_FILE), &entity).expect("write entity");

        let archive_path = dir.with_extension("tar.zst");
        build_tar_zst(&dir, &archive_path).expect("build archive");

        let extract_dir = scratch();
        extract_tar_zst(&archive_path, &extract_dir).expect("extract archive");

        let read_back = read_manifest(&extract_dir).expect("read manifest");
        assert_eq!(read_back, manifest);
        let entities = read_entities(&extract_dir).expect("read entities");
        assert_eq!(entities, vec![entity]);
        let relationships = read_relationships(&extract_dir).expect("read relationships");
        assert!(relationships.is_empty(), "no relationships were written");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&archive_path).ok();
        std::fs::remove_dir_all(&extract_dir).ok();
    }

    /// **A truncated archive is detected, not silently half-read.** `zstd`
    /// refuses a stream that stops mid-frame; asserting it here is what
    /// backs Slice B's "checksum mismatch or truncation refuses before
    /// writing anything" — the failure has to originate somewhere, and this
    /// is where.
    #[test]
    fn a_truncated_archive_is_refused_not_silently_read() {
        let dir = scratch();
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let manifest = ArchiveManifest {
            format_version: graph_owl_core::archive::FORMAT_VERSION,
            source_instance: "test".to_string(),
            created_at: chrono::Utc::now(),
            entity_count: 0,
            relationship_count: 0,
            scope: None,
            redacted_fields: vec![],
            section_checksums: std::collections::BTreeMap::new(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_vec(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");
        let archive_path = dir.with_extension("tar.zst");
        build_tar_zst(&dir, &archive_path).expect("build archive");

        let full = std::fs::read(&archive_path).expect("read archive bytes");
        let truncated_path = dir.with_extension("truncated.tar.zst");
        std::fs::write(&truncated_path, &full[..full.len() / 2]).expect("write truncated");

        let extract_dir = scratch();
        let outcome = extract_tar_zst(&truncated_path, &extract_dir);
        assert!(
            outcome.is_err(),
            "a truncated archive must not extract cleanly"
        );

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_file(&archive_path).ok();
        std::fs::remove_file(&truncated_path).ok();
        std::fs::remove_dir_all(&extract_dir).ok();
    }

    #[test]
    fn a_section_whose_bytes_changed_since_the_manifest_was_written_is_refused() {
        let dir = scratch();
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let entity = ArchivedEntity {
            asset: asset("a.b"),
            versions: vec![],
        };
        append_ndjson_line(&dir.join(ENTITIES_FILE), &entity).expect("write entity");
        let real_checksum = section_checksum(&dir.join(ENTITIES_FILE))
            .expect("compute checksum")
            .expect("file exists");

        let mut checksums = std::collections::BTreeMap::new();
        // A checksum that does not match what was actually written — the
        // shape a tampered or corrupted archive would present.
        checksums.insert(ENTITIES_FILE.to_string(), "0".repeat(real_checksum.len()));
        let manifest = ArchiveManifest {
            format_version: graph_owl_core::archive::FORMAT_VERSION,
            source_instance: "test".to_string(),
            created_at: chrono::Utc::now(),
            entity_count: 1,
            relationship_count: 0,
            scope: None,
            redacted_fields: vec![],
            section_checksums: checksums,
        };

        let outcome = verify_section_checksums(&dir, &manifest);
        assert!(outcome.is_err(), "a mismatched checksum must be refused");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_correct_checksum_verifies() {
        let dir = scratch();
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let entity = ArchivedEntity {
            asset: asset("a.b"),
            versions: vec![],
        };
        append_ndjson_line(&dir.join(ENTITIES_FILE), &entity).expect("write entity");
        let checksum = section_checksum(&dir.join(ENTITIES_FILE))
            .expect("compute checksum")
            .expect("file exists");

        let mut checksums = std::collections::BTreeMap::new();
        checksums.insert(ENTITIES_FILE.to_string(), checksum);
        let manifest = ArchiveManifest {
            format_version: graph_owl_core::archive::FORMAT_VERSION,
            source_instance: "test".to_string(),
            created_at: chrono::Utc::now(),
            entity_count: 1,
            relationship_count: 0,
            scope: None,
            redacted_fields: vec![],
            section_checksums: checksums,
        };

        verify_section_checksums(&dir, &manifest).expect("a correct checksum must verify");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn redacting_description_clears_it_on_the_entity_and_every_version() {
        let mut entity = ArchivedEntity {
            asset: asset("a.b"),
            versions: vec![graph_owl_core::AssetVersion {
                version: graph_owl_core::envelope::EntityVersion::initial(),
                snapshot: asset("a.b"),
                change_description: None,
                updated_by: "test".to_string(),
                updated_at: chrono::Utc::now(),
            }],
        };
        assert!(entity.asset.description.is_some());

        redact_entity(&mut entity, &["description".to_string()]);

        assert_eq!(entity.asset.description, None);
        assert_eq!(entity.versions[0].snapshot.description, None);
    }

    /// A redaction rule naming a field this archive does not carry a value
    /// for must not touch anything else — over-redacting silently is as
    /// wrong as under-redacting.
    #[test]
    fn an_unrecognised_redaction_field_redacts_nothing() {
        let mut entity = ArchivedEntity {
            asset: asset("a.b"),
            versions: vec![],
        };
        let before = entity.clone();

        redact_entity(&mut entity, &["ownerEmail".to_string()]);

        assert_eq!(entity, before);
    }
}
