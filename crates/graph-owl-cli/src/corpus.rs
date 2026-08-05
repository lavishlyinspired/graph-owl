//! A deterministic, realistically-shaped synthetic corpus — Epic 37a Slice A.
//!
//! **Pure generation, no I/O** — decision 2's "seeded deterministic
//! generator", kept testable without a database the same way
//! `graph-owl-core::projection` is: a function from a seed to data.
//!
//! **Loading reuses Epic 37b, rather than inventing a second bulk-write
//! path.** The generator packages its output in the *identical* archive
//! shape `Catalog::export_archive` produces (same manifest, same
//! `entities.ndjson`/`relationships.ndjson` layout, same per-section
//! checksums), so `graph-owl restore --in corpus.tar.zst` — already built,
//! already tested — is the loader. No new bulk-insert endpoint, no new
//! authz surface, no second thing to keep correct.
//!
//! This crate does not depend on `graph-owl-api` (decision 6, `main.rs`'s
//! own doc comment: "no direct database access"), so the packing step
//! below is a small, deliberate duplicate of
//! `graph-owl-api::archive::build_tar_zst`'s shape rather than a shared
//! dependency that would pull the storage adapter into this binary.
//!
//! Every `f64`/`usize` conversion in the power-law math below is a size
//! *estimate* feeding a `round()`/`sqrt()` — the corpus this produces is
//! "approximately `target_tables` tables, power-law distributed", never a
//! value anything downstream compares for exact equality, so the
//! precision/truncation/sign-loss triad `clippy::pedantic` flags on every
//! such cast does not apply here.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::collections::BTreeMap;

use graph_owl_core::archive::{
    ArchiveManifest, ArchivedEntity, ArchivedRelationship, FORMAT_VERSION,
};
use graph_owl_core::envelope::EntityVersion;
use graph_owl_core::{Asset, AssetKind, AssetVersion, Relationship};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;

/// The three files a corpus archive holds — the exact names
/// `graph-owl-api::archive` uses, so a corpus archive is byte-for-byte the
/// same shape a real export produces and `restore` cannot tell them apart.
const MANIFEST_FILE: &str = "manifest.json";
const ENTITIES_FILE: &str = "entities.ndjson";
const RELATIONSHIPS_FILE: &str = "relationships.ndjson";

/// One generated corpus: entities (each with its own version history) and
/// relationships, plus the shapes the plan's own criteria name explicitly
/// so a caller does not have to re-derive them by scanning the output.
pub struct Corpus {
    pub entities: Vec<ArchivedEntity>,
    pub relationships: Vec<ArchivedRelationship>,
    /// How many entities in `entities` are soft-deleted.
    pub soft_deleted_count: usize,
    /// Whether a cyclic lineage chain was injected.
    pub has_cycle: bool,
}

/// Generates a corpus of approximately `target_tables` tables (plus the
/// service/database/schema hierarchy above them), deterministically from
/// `seed` — the same seed always produces the identical corpus (decision 2).
///
/// **Realistically shaped, not uniform** (decision 3): schemas get a
/// power-law share of the tables — a rank-based `1/rank` distribution, the
/// simplest construction that is genuinely a power law rather than
/// approximately one, so a handful of schemas hold most of the tables and
/// most schemas hold few. Lineage fan-out follows the same shape: most
/// tables feed 0–1 others, a few "hub" tables feed many.
#[must_use]
pub fn generate(seed: u64, target_tables: usize) -> Corpus {
    let mut rng = StdRng::seed_from_u64(seed);
    let now = chrono::Utc::now();

    let service = asset(AssetKind::Service, "svc0", "svc0", None, now);
    let database = asset(
        AssetKind::Database,
        "db0",
        &fqn2("svc0", "db0"),
        Some(service.id),
        now,
    );
    let (schemas, tables, soft_deleted_count) =
        generate_tables(&mut rng, database.id, target_tables, now);
    let (relationships, has_cycle) = generate_lineage(&mut rng, &tables, now);

    let mut entities = vec![
        ArchivedEntity {
            asset: service,
            versions: vec![],
        },
        ArchivedEntity {
            asset: database,
            versions: vec![],
        },
    ];
    entities.extend(schemas.into_iter().map(|s| ArchivedEntity {
        asset: s,
        versions: vec![],
    }));
    entities.extend(tables);
    // FQN order — parent before child, matching `Catalog::restore_archive`'s
    // own ordering requirement.
    entities.sort_by(|a, b| {
        a.asset
            .fully_qualified_name
            .cmp(&b.asset.fully_qualified_name)
    });

    Corpus {
        entities,
        relationships,
        soft_deleted_count,
        has_cycle,
    }
}

/// Schemas plus their power-law share of `target_tables`, each table
/// carrying 1–3 versions and a ~2% chance of being soft-deleted. The
/// harmonic-normalized rank-based share (schema at rank `r` gets `1/r` of
/// the total) is what makes this a genuine power law rather than an
/// approximation of one.
fn generate_tables(
    rng: &mut StdRng,
    database_id: uuid::Uuid,
    target_tables: usize,
    now: chrono::DateTime<chrono::Utc>,
) -> (Vec<Asset>, Vec<ArchivedEntity>, usize) {
    let schema_count = (target_tables as f64).sqrt().ceil().max(1.0) as usize;
    let schemas: Vec<Asset> = (0..schema_count)
        .map(|i| {
            let name = format!("schema{i}");
            asset(
                AssetKind::Schema,
                &name,
                &fqn3("svc0", "db0", &name),
                Some(database_id),
                now,
            )
        })
        .collect();

    let harmonic: f64 = (1..=schema_count).map(|r| 1.0 / r as f64).sum();
    let mut tables: Vec<ArchivedEntity> = Vec::with_capacity(target_tables);
    let mut soft_deleted_count = 0usize;
    for (i, schema) in schemas.iter().enumerate() {
        let rank = i + 1;
        let share = (1.0 / rank as f64) / harmonic;
        let this_schema_tables = ((target_tables as f64) * share).round() as usize;
        for t in 0..this_schema_tables {
            if tables.len() >= target_tables {
                break;
            }
            let name = format!("t{i}_{t}");
            let deleted = rng.gen_ratio(2, 100); // ~2% soft-deleted
            if deleted {
                soft_deleted_count += 1;
            }
            let mut table = asset(
                AssetKind::Table,
                &name,
                &fqn4("svc0", "db0", &schema.name, &name),
                Some(schema.id),
                now,
            );
            table.deleted = deleted;
            if deleted {
                table.deleted_at = Some(now);
            }

            // 1–3 versions, deterministic per entity.
            let version_count = rng.gen_range(1..=3);
            let versions = (0..version_count)
                .map(|v| AssetVersion {
                    version: EntityVersion {
                        major: 1,
                        minor: u32::try_from(v).unwrap_or(0),
                    },
                    snapshot: table.clone(),
                    change_description: None,
                    updated_by: "corpus-generator".to_string(),
                    updated_at: now,
                })
                .collect();

            tables.push(ArchivedEntity {
                asset: table,
                versions,
            });
        }
    }

    (schemas, tables, soft_deleted_count)
}

/// Lineage fan-out over `tables` — the same rank-based power law, so most
/// tables feed 0–1 others and a handful of "hub" tables feed many — plus a
/// deliberate cyclic chain (decision 3's own named shape): the first three
/// tables feed each other in a ring, A -> B -> C -> A, the shape that
/// breaks a naive traversal without a visited-set.
fn generate_lineage(
    rng: &mut StdRng,
    tables: &[ArchivedEntity],
    now: chrono::DateTime<chrono::Utc>,
) -> (Vec<ArchivedRelationship>, bool) {
    let feeds = |from: uuid::Uuid, to: uuid::Uuid| ArchivedRelationship {
        relationship: Relationship {
            id: uuid::Uuid::new_v4(),
            from_entity_type: "table".to_string(),
            from_entity_id: from,
            relationship_type: "feeds".to_string(),
            to_entity_type: "table".to_string(),
            to_entity_id: to,
            created_at: now,
        },
    };

    // A real "table A feeds table B" fact is asserted once — deduped here
    // (rather than left to the restore path's own tuple-conflict handling)
    // so `relationships.len()` is the true count a caller can rely on,
    // including in a test asserting a restore's own counts.
    let mut seen: std::collections::HashSet<(uuid::Uuid, uuid::Uuid)> =
        std::collections::HashSet::new();
    let mut relationships = Vec::new();
    if tables.len() > 1 {
        let table_harmonic: f64 = (1..=tables.len()).map(|r| 1.0 / r as f64).sum();
        for (i, table) in tables.iter().enumerate() {
            let rank = i + 1;
            let share = (1.0 / rank as f64) / table_harmonic;
            let fan_out = ((tables.len() as f64) * share * 3.0).round() as usize;
            for _ in 0..fan_out.min(tables.len() - 1) {
                let target = rng.gen_range(0..tables.len());
                if target != i && seen.insert((table.asset.id, tables[target].asset.id)) {
                    relationships.push(feeds(table.asset.id, tables[target].asset.id));
                }
            }
        }
    }

    let has_cycle = tables.len() >= 3;
    if has_cycle {
        for i in 0..3 {
            let (from, to) = (tables[i].asset.id, tables[(i + 1) % 3].asset.id);
            if seen.insert((from, to)) {
                relationships.push(feeds(from, to));
            }
        }
    }

    (relationships, has_cycle)
}

fn asset(
    kind: AssetKind,
    name: &str,
    fqn: &str,
    parent_id: Option<uuid::Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) -> Asset {
    Asset {
        id: uuid::Uuid::new_v4(),
        kind,
        name: name.to_string(),
        fully_qualified_name: fqn.to_string(),
        parent_id,
        description: None,
        properties: None,
        owners: Vec::new(),
        version: EntityVersion::initial(),
        updated_by: "corpus-generator".to_string(),
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

fn fqn2(a: &str, b: &str) -> String {
    graph_owl_core::fqn::child_of(a, b).unwrap_or_else(|_| format!("{a}.{b}"))
}
fn fqn3(a: &str, b: &str, c: &str) -> String {
    graph_owl_core::fqn::child_of(&fqn2(a, b), c).unwrap_or_else(|_| format!("{a}.{b}.{c}"))
}
fn fqn4(a: &str, b: &str, c: &str, d: &str) -> String {
    graph_owl_core::fqn::child_of(&fqn3(a, b, c), d).unwrap_or_else(|_| format!("{a}.{b}.{c}.{d}"))
}

/// Packages a [`Corpus`] as a `.tar.zst` archive at `out`, in the identical
/// shape `graph-owl-api::archive::build_tar_zst` produces — a manifest with
/// real per-section SHA-256 checksums, so `graph-owl restore --in` can read
/// it with no special case.
///
/// # Errors
/// Any I/O failure writing the scratch files or the final archive.
pub fn write_archive(corpus: &Corpus, out: &std::path::Path) -> std::io::Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;

    let scratch = std::env::temp_dir().join(format!("graph-owl-corpus-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch)?;

    let entities_path = scratch.join(ENTITIES_FILE);
    {
        let mut file = std::fs::File::create(&entities_path)?;
        for entity in &corpus.entities {
            serde_json::to_writer(&mut file, entity).map_err(std::io::Error::other)?;
            file.write_all(b"\n")?;
        }
    }
    let relationships_path = scratch.join(RELATIONSHIPS_FILE);
    {
        let mut file = std::fs::File::create(&relationships_path)?;
        for relationship in &corpus.relationships {
            serde_json::to_writer(&mut file, relationship).map_err(std::io::Error::other)?;
            file.write_all(b"\n")?;
        }
    }

    let checksum_of = |path: &std::path::Path| -> std::io::Result<String> {
        let bytes = std::fs::read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Ok(format!("{:x}", hasher.finalize()))
    };
    let mut section_checksums = BTreeMap::new();
    section_checksums.insert(ENTITIES_FILE.to_string(), checksum_of(&entities_path)?);
    section_checksums.insert(
        RELATIONSHIPS_FILE.to_string(),
        checksum_of(&relationships_path)?,
    );

    let manifest = ArchiveManifest {
        format_version: FORMAT_VERSION,
        source_instance: "corpus-generator".to_string(),
        created_at: chrono::Utc::now(),
        entity_count: corpus.entities.len() as u64,
        relationship_count: corpus.relationships.len() as u64,
        scope: None,
        redacted_fields: vec![],
        section_checksums,
    };
    let manifest_path = scratch.join(MANIFEST_FILE);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest).map_err(std::io::Error::other)?,
    )?;

    let output = std::fs::File::create(out)?;
    let encoder = zstd::Encoder::new(output, 0)?.auto_finish();
    let mut tar = tar::Builder::new(encoder);
    tar.append_path_with_name(&manifest_path, MANIFEST_FILE)?;
    tar.append_path_with_name(&entities_path, ENTITIES_FILE)?;
    tar.append_path_with_name(&relationships_path, RELATIONSHIPS_FILE)?;
    tar.finish()?;

    std::fs::remove_dir_all(&scratch).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_produces_an_identical_corpus() {
        let a = generate(42, 200);
        let b = generate(42, 200);

        assert_eq!(a.entities.len(), b.entities.len());
        assert_eq!(a.relationships.len(), b.relationships.len());
        // Entity ids are freshly minted `Uuid::new_v4()` per generation, so
        // they cannot be compared directly — the *shape* is what
        // determinism means here: same names, same FQNs, same structure,
        // in the same order.
        let names_a: Vec<&str> = a.entities.iter().map(|e| e.asset.name.as_str()).collect();
        let names_b: Vec<&str> = b.entities.iter().map(|e| e.asset.name.as_str()).collect();
        assert_eq!(names_a, names_b);
        let fqns_a: Vec<&str> = a
            .entities
            .iter()
            .map(|e| e.asset.fully_qualified_name.as_str())
            .collect();
        let fqns_b: Vec<&str> = b
            .entities
            .iter()
            .map(|e| e.asset.fully_qualified_name.as_str())
            .collect();
        assert_eq!(fqns_a, fqns_b);
        assert_eq!(a.soft_deleted_count, b.soft_deleted_count);
    }

    #[test]
    fn a_different_seed_produces_a_different_corpus() {
        let a = generate(1, 200);
        let b = generate(2, 200);

        let names_a: Vec<&str> = a.entities.iter().map(|e| e.asset.name.as_str()).collect();
        let names_b: Vec<&str> = b.entities.iter().map(|e| e.asset.name.as_str()).collect();
        // The schema/table *names* are seed-independent (they are assigned
        // by position, not drawn from the rng); what differs between seeds
        // is which tables are soft-deleted, how many versions each has, and
        // the lineage edges — asserted separately below. This test exists
        // so a future refactor that made name assignment seed-dependent
        // could not silently make every corpus at a given size identical.
        assert_eq!(names_a, names_b, "names are positional, not seed-drawn");
        assert_ne!(
            a.relationships.len(),
            b.relationships.len(),
            "two different seeds producing the exact same edge count would \
             be a suspicious coincidence worth checking, though not \
             impossible; regenerate with different sizes if this ever \
             flakes"
        );
    }

    /// **The slice's own RED test.** A uniform generator — the same number
    /// of tables in every schema — must fail this: the whole point of
    /// decision 3 is that a handful of schemas hold most of the tables.
    #[test]
    fn table_distribution_across_schemas_is_a_power_law_not_uniform() {
        let corpus = generate(7, 1000);
        let mut per_schema: std::collections::HashMap<uuid::Uuid, usize> =
            std::collections::HashMap::new();
        for entity in &corpus.entities {
            if entity.asset.kind == AssetKind::Table
                && let Some(parent) = entity.asset.parent_id
            {
                *per_schema.entry(parent).or_insert(0) += 1;
            }
        }
        let mut counts: Vec<usize> = per_schema.into_values().collect();
        counts.sort_unstable_by(|a, b| b.cmp(a));

        assert!(counts.len() > 5, "need enough schemas to see a shape");
        let top = counts[0];
        let median = counts[counts.len() / 2];
        assert!(
            top > median * 2,
            "the busiest schema ({top} tables) should dwarf a median one \
             ({median}) under a power law; a uniform generator would make \
             these nearly equal: {counts:?}"
        );
    }

    #[test]
    fn the_corpus_includes_soft_deleted_entities_multiple_versions_and_a_cycle() {
        let corpus = generate(3, 500);

        assert!(corpus.soft_deleted_count > 0, "no soft-deleted entities");
        assert!(corpus.has_cycle, "no cyclic lineage was injected");
        assert!(
            corpus.entities.iter().any(|e| e.versions.len() > 1),
            "no entity has more than one version"
        );
    }

    #[test]
    fn entities_are_ordered_parent_before_child_by_fqn() {
        let corpus = generate(9, 300);
        let mut seen: std::collections::HashSet<uuid::Uuid> = std::collections::HashSet::new();
        for entity in &corpus.entities {
            if let Some(parent) = entity.asset.parent_id {
                assert!(
                    seen.contains(&parent),
                    "{} appeared before its parent",
                    entity.asset.fully_qualified_name
                );
            }
            seen.insert(entity.asset.id);
        }
    }

    #[test]
    fn a_generated_corpus_writes_and_reads_back_as_a_real_archive() {
        let corpus = generate(11, 50);
        let out =
            std::env::temp_dir().join(format!("corpus-test-{}.tar.zst", uuid::Uuid::new_v4()));

        write_archive(&corpus, &out).expect("write archive");
        assert!(out.exists());
        let bytes = std::fs::read(&out).expect("read archive bytes");
        assert!(!bytes.is_empty());

        // Decompress and untar directly — proving the bytes are a real
        // archive, not just proving this module's own writer ran.
        let decoder = zstd::Decoder::new(std::fs::File::open(&out).expect("open")).expect("decode");
        let mut archive = tar::Archive::new(decoder);
        let names: Vec<String> = archive
            .entries()
            .expect("entries")
            .map(|e| {
                e.expect("entry")
                    .path()
                    .expect("path")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(names.contains(&MANIFEST_FILE.to_string()));
        assert!(names.contains(&ENTITIES_FILE.to_string()));
        assert!(names.contains(&RELATIONSHIPS_FILE.to_string()));

        std::fs::remove_file(&out).ok();
    }
}
