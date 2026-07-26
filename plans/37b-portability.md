# Plan: Backup & Portability (Epic 37b)
**Branch**: feat/portability
**Status**: Not started
**Depends on**: Epic 3 (history is part of what must survive)
**Crates**: `graph-owl-cli` (export/restore) · `graph-owl-core` (archive format types) · `graph-owl-api` (streaming export, conflict policy) — no new crates

## Goal

Get the whole catalog out, put it back, and move it between instances — in a documented format that is not a database dump.

## Why a database dump is not enough

`pg_dump` covers disaster recovery and nothing else. It cannot move a catalog between instances running different versions, cannot extract one domain for a subsidiary, cannot seed a staging environment from production minus the sensitive parts, and cannot be inspected or diffed. Those are the operations organizations actually ask for, and each needs a format that is a contract rather than an implementation detail.

This is distinct from Epic 20's metadata-as-code export, which is deliberately lossy — it emits *declarable* state. This epic is lossless: history, versions, events, and system fields included.

## Resolved decisions

1. **The export format is a versioned contract**, documented and semver'd. An export must be restorable by a later version.
2. **Lossless by default**, unlike Epic 20's export. `--declarative-only` produces the lossy form for handoff to metadata-as-code.
3. **Streaming, not buffered.** A 100k-entity export must not require holding the catalog in memory — the same property Epic 15 required of connectors.
4. **Restore has an explicit conflict policy** — `fail`, `skip`, or `overwrite`. A silent default here loses data.
5. **Scopeable.** Export by domain, service, or entity type, so "give the subsidiary their slice" is one command.
6. **Entity ids are preserved by default**, so cross-instance relationships and lineage survive. `--regenerate-ids` supports merging two catalogs that would otherwise collide.

## Acceptance criteria (feature level)

- [ ] A full export restores into an empty instance with byte-identical entity state, including history and edges.
- [ ] Export streams without unbounded memory growth on a 100k-entity catalog.
- [ ] Restore into a populated instance honours the chosen conflict policy.
- [ ] Export is scopeable by domain, service, and entity type, with referential integrity preserved.
- [ ] The format is versioned; a newer version restores an older export.
- [ ] Sensitive fields can be redacted for non-production restores.
- [ ] A corrupt or truncated archive is detected before anything is written.

## Slices

Every slice runs RED → GREEN → MUTATE → KILL MUTANTS → REFACTOR with implementation skills loaded first.

### Slice A: The catalog exports

**Value**: The data can leave.
**Path**: `graph-owl export --out catalog.tar.zst` streaming a versioned archive.
**Acceptance criteria**:
- Archive contains a manifest (format version, source instance, timestamp, counts), entities by type, relationships, version history, and custom property definitions.
- Streams — constant memory regardless of catalog size.
- Deterministic ordering so two exports of an unchanged catalog are byte-identical.
- Compressed, with a checksum per section.
- Progress reported for long-running exports.
- Export is read-only — asserted by checking zero versions created during a run.
- An export from a catalog being written to is internally consistent (a snapshot, not a smear).
**RED**: Memory-bounded export test against Epic 37a's 100k corpus. Determinism test. A consistency test exporting while writes are in flight and asserting no dangling references. Mutator watch: a buffering implementation must fail the memory bound; a non-snapshot read must fail the consistency test.
**GREEN**: streaming exporter, manifest, checksums, snapshot isolation.
**REFACTOR**: assess sharing the streaming machinery with Epic 9's exporter — the traversal is the same, the serialization differs.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice B: The catalog restores

**Value**: The round trip closes — the only thing that makes a backup a backup.
**Path**: `graph-owl restore --in catalog.tar.zst` with a conflict policy.
**Acceptance criteria**:
- Restore into an empty instance reproduces entity state, ids, FQNs, relationships, and history exactly.
- Round-trip property: export → restore → export produces an identical second archive.
- Referential integrity: parents before children, relationships after both endpoints.
- Checksum mismatch or truncation → refuse before writing anything.
- A format version newer than the binary understands → refuse with a clear message.
- Restore is transactional per section, so a mid-restore failure does not leave a half-catalog.
- Progress reported; restore is resumable after interruption.
**RED**: The round-trip byte-identity test is the specification. A truncated-archive test asserting nothing is written. Mutator watch: validating the checksum after writing must fail the truncation test — validation must precede mutation.
**GREEN**: validating reader, ordered restore, transactional sections, resumability.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice C: Conflicts are handled explicitly

**Value**: Restoring into a live catalog is predictable rather than destructive.
**Path**: `--on-conflict fail|skip|overwrite`, matched by entity id and by FQN.
**Acceptance criteria**:
- `fail` (default): any conflict aborts before writing, listing conflicts.
- `skip`: existing entities untouched; the report names what was skipped.
- `overwrite`: existing entities replaced, bumping versions, with history preserved rather than discarded.
- Conflict detection covers both id collision and FQN collision — these are different conflicts and are reported distinctly.
- A dry-run reports conflicts without writing.
- `--regenerate-ids` avoids id collisions when merging two catalogs, rewriting relationship references consistently.
**RED**: Tests per policy. The `--regenerate-ids` test is the subtle one: assert every relationship still points at the right entity after rewriting. Mutator watch: rewriting ids without rewriting references must fail it — a corruption that would be invisible until traversal.
**GREEN**: conflict detection, per-policy handling, consistent id rewriting.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice D: Exports are scopeable

**Value**: "Give the subsidiary their slice" and "seed staging from a subset" become one command.
**Path**: `--scope domain:payments` / `service:snowflake_prod` / `entity-type:table`.
**Acceptance criteria**:
- Scoped export contains only in-scope entities.
- Referential integrity is preserved: a relationship whose other endpoint is out of scope is either excluded or included as a stub, per `--include-references`.
- Ancestors required for FQN derivation are always included, even if out of scope — otherwise the restore produces orphans.
- Scoped export restores standalone into an empty instance.
- The manifest records the scope so a restore knows the archive is partial.
- Combining scopes is a union.
**RED**: Test asserting a scoped export restores without dangling references. Test asserting required ancestors are pulled in despite being out of scope. Mutator watch: omitting ancestors must fail the restore — the failure mode that produces an unusable partial archive.
**GREEN**: scope resolution, ancestor closure, reference policy.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice E: Sensitive data can be redacted

**Value**: Production data can seed a development environment without carrying what it should not.
**Path**: `--redact` rules over fields and custom properties.
**Acceptance criteria**:
- Redact named custom properties, descriptions, or user emails.
- Redaction is applied at export, so the archive never contains the data — not at restore.
- Redacted fields are marked as redacted, not silently emptied, so a consumer knows the difference from a genuinely empty field.
- Redaction rules are recorded in the manifest.
- A redacted export still restores and round-trips.
- Entity ids and structure are preserved so lineage and relationships stay meaningful.
**RED**: Test asserting a redacted value appears nowhere in the archive bytes — a grep over the decompressed archive, not just an API assertion. Mutator watch: redacting on read rather than on write must fail the byte-level assertion.
**GREEN**: export-time redaction, manifest recording, marker fields.
**Done when**: criteria met, mutation report reviewed, commit approved.

### Slice F: The format is a contract

**Value**: An archive taken today restores in two years.
**Path**: documented, versioned format with compatibility tests.
**Acceptance criteria**:
- The format is documented field by field in `plans/37b-portability.md` (this plan, Slice F).
- Format version is semver; the manifest carries it.
- A newer binary restores an older archive — verified by committed golden archives from each released format version.
- An older binary refuses a newer archive with a clear message rather than corrupting.
- A compatibility test suite runs against every golden archive on every build.
- A documented policy for what constitutes a breaking format change.
**RED**: Golden-archive restore tests. A test asserting an older binary refuses a newer version cleanly. Mutator watch: a version check that accepts anything must fail the refusal test.
**GREEN**: documentation, version negotiation, golden archives, compatibility suite.
**Done when**: criteria met, compatibility suite green, commit approved.

## Explicitly deferred (with destination)

- **Continuous replication / streaming backup** → an operational concern owned by the database; this epic covers logical, portable export.
- **Point-in-time restore of the whole catalog** → Epic absorbed into 4's time-travel answers the read case; whole-catalog rollback is a database-level operation.
- **Incremental / differential export** → full export is sufficient at the target scale; add if export duration becomes a problem, and Epic absorbed into 4's range diff is the natural basis.
- **Cross-format import** (from another catalog product) → each source would need its own mapper; metadata-as-code (Epic 15) is the general-purpose ingestion path.
- **Encryption at rest for archives** → delegate to the storage layer or an external tool rather than inventing key management.

## Pre-PR quality gate

1. `cargo mutants` on every changed file — 0 missed.
2. Refactoring assessment.
3. `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check`.
4. Round-trip verified against Epic 37's 100k corpus, not only a small fixture.
5. Redaction verified at the byte level (Slice E) — an API-level assertion is insufficient.
