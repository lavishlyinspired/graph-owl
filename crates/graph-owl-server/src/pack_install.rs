//! Installing a domain pack from the console, not only from the
//! `graph-owl-load-pack` CLI — a direct product decision: browsing and
//! importing already moved into `Admin > Packs`
//! (`ui/src/features/packs/PackAdminPanel.tsx`), and installation was the
//! one step still requiring a terminal.
//!
//! **Two genuinely separate concerns, kept in two functions.**
//! [`scan_available_packs`] only reads `pack.toml` headers off disk — pure,
//! synchronous, no network, no admin gate, unit-tested directly with a temp
//! directory. Installing a pack is not reimplemented here: it shells out to
//! the already-built, already-tested, already-idempotent Python loader
//! (`connectors/python/graph_owl_packs/loader.py`, `load_pack()`) exactly
//! the way an operator running `graph-owl-load-pack` by hand does today —
//! reusing that logic rather than re-deriving `pack.toml`'s full grammar
//! (documents, predicates, matching, findings, queries, glossary
//! registration, in a specific order) a second time in Rust. The same
//! "separate process, not reimplemented" boundary `agent_service` already
//! draws for the reconciliation agent (`plans/00j-language-boundaries.md`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;

/// Where this deployment's pack directories live. `GRAPH_OWL_PACKS_DIR`
/// overrides the default — every real launch of this binary so far in this
/// project (`scripts/demo.sh`, every manual restart) runs from the repo
/// root, where `packs/` sits beside `crates/`, so that is the default
/// rather than something every deployment must set.
pub fn packs_base_dir() -> PathBuf {
    std::env::var("GRAPH_OWL_PACKS_DIR").map_or_else(|_| PathBuf::from("packs"), PathBuf::from)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailablePack {
    pub id: String,
    pub description: String,
}

/// A minimal, honest slice of `pack.toml`'s `[pack]` table — just enough to
/// list a pack, not a second copy of the manifest grammar `loader.py`
/// already owns.
#[derive(Debug, serde::Deserialize)]
struct PackHeader {
    pack: PackHeaderInner,
}

#[derive(Debug, serde::Deserialize)]
struct PackHeaderInner {
    id: String,
    description: String,
}

/// Every subdirectory of `base_dir` with a readable, parseable
/// `pack.toml` whose `[pack].id` is not already in `installed` —
/// alphabetical by id, so the list does not reorder between calls for no
/// reason.
///
/// **A pack directory that fails to read or parse is skipped, not a 500.**
/// A stray non-pack directory (or one mid-edit) must not take the whole
/// listing down — the same "absent rather than broken" posture
/// `packSurfaces.ts`'s own doc comment already commits to for a pack with
/// no registered surface.
pub fn scan_available_packs<S: std::hash::BuildHasher>(
    base_dir: &Path,
    installed: &HashSet<String, S>,
) -> Vec<AvailablePack> {
    let Ok(entries) = std::fs::read_dir(base_dir) else {
        return Vec::new();
    };
    let mut packs: Vec<AvailablePack> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let manifest_path = entry.path().join("pack.toml");
            let text = std::fs::read_to_string(&manifest_path).ok()?;
            let header: PackHeader = toml::from_str(&text).ok()?;
            if installed.contains(&header.pack.id) {
                return None;
            }
            Some(AvailablePack {
                id: header.pack.id,
                description: header.pack.description,
            })
        })
        .collect();
    packs.sort_by(|a, b| a.id.cmp(&b.id));
    packs
}

/// Locates the pack loader executable. `GRAPH_OWL_LOAD_PACK_BIN`
/// overrides; otherwise the venv path every pack install in this project
/// has used so far (`connectors/python/.venv`, `pip install -e .` run
/// there per that package's own `pyproject.toml`) is tried before falling
/// back to a bare command name resolved from `PATH` — so a deployment that
/// installed the loader normally (`pip install graph-owl-packs`) still
/// works without setting anything.
fn loader_binary() -> PathBuf {
    if let Ok(path) = std::env::var("GRAPH_OWL_LOAD_PACK_BIN") {
        return PathBuf::from(path);
    }
    let venv_path = PathBuf::from("connectors/python/.venv/bin/graph-owl-load-pack");
    if venv_path.is_file() {
        return venv_path;
    }
    PathBuf::from("graph-owl-load-pack")
}

#[derive(Debug)]
pub struct InstallOutcome {
    pub ok: bool,
    pub output: String,
}

/// Hand-rolled rather than `thiserror` — that crate is gated behind this
/// server's optional `bolt` feature (`Cargo.toml`'s `bolt = [...,
/// "dep:thiserror"]`), and pulling it in unconditionally for one error
/// variant would widen what a `--no-default-features` build compiles,
/// which Epic 7d Slice F specifically tests against.
#[derive(Debug)]
pub struct InstallError {
    binary: String,
    source: std::io::Error,
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "could not start the pack loader ({}): {}",
            self.binary, self.source
        )
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Runs the existing, tested loader against one pack directory —
/// `graph-owl-load-pack <dir> --server <self> --token <caller's own>`.
/// The caller's own bearer token, never a fixed service credential: every
/// route the loader drives (`/namespaces`, `/predicates`, `/graph/import/
/// rdf`, `/packs/{id}/finding-rules`, `/packs/{id}/queries`,
/// `/ontology-packs`) is already admin-gated, and this handler has already
/// checked the caller is an admin before reaching here — reusing that same
/// token is what makes the install genuinely attributed to whoever clicked
/// the button, not a shared identity.
///
/// **Not idempotence-tested here** — that property belongs to
/// `loader.py`'s own test suite (`connectors/python/tests/`), which this
/// function does not re-derive; it only proves it invoked the loader
/// correctly and surfaced what came back.
///
/// # Errors
///
/// [`InstallError`] only when the loader binary itself could not be
/// started (missing venv, not on `PATH`) — an environment problem, not a
/// judgement on the pack. The loader running and then reporting a failure
/// of its own (a bad `pack.toml`, a rejected HTTP call) is not an `Err`
/// here; it comes back as `Ok(InstallOutcome { ok: false, .. })`.
pub async fn run_pack_loader(
    pack_dir: &Path,
    server_url: &str,
    token: &str,
) -> Result<InstallOutcome, InstallError> {
    let binary = loader_binary();
    let output = tokio::process::Command::new(&binary)
        .arg(pack_dir)
        .arg("--server")
        .arg(server_url)
        .arg("--token")
        .arg(token)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|source| InstallError {
            binary: binary.display().to_string(),
            source,
        })?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(InstallOutcome {
        ok: output.status.success(),
        output: combined,
    })
}

/// A pack's own `[console]` table, verbatim, as JSON.
///
/// **Why the console needed this at all.** The reconciliation page's source
/// list, its measures and its per-finding guidance lived in TypeScript, so it
/// only ever knew how to render GST. Install a healthcare, banking or
/// automotive pack and the page would show that pack's data under GST's
/// headings, or nothing. The *shape* — sources in, rules run, a statement out,
/// exceptions to work — is genuinely domain-neutral and stays in the console;
/// everything that names a domain now comes from the pack that owns it.
///
/// **Read at request time, not stored** — the same reading
/// [`scan_available_packs`] already does of the `[pack]` header, for the same
/// reason: `pack.toml` stays the single source of truth for what a pack is,
/// and this needs no migration, no storage table and no loader change.
///
/// **Returned as untyped JSON on purpose.** Giving it a Rust struct would put
/// a second copy of the manifest's grammar in the server and make every new
/// console field a Rust change — exactly what `plans/105-domain-neutrality.md`
/// refuses. The console is the only consumer and it is the right place to know
/// the shape.
///
/// `None` for a pack with no `[console]` section, which is the ordinary case
/// (hospitality has never had one) and must render an honest empty state
/// rather than an error.
#[must_use]
pub fn read_console_config(base_dir: &Path, pack: &str) -> Option<serde_json::Value> {
    // The same path-traversal refusal installing already applies — an id is
    // joined onto a filesystem path here too.
    if !crate::pack_id_is_safe(pack) {
        return None;
    }
    let text = std::fs::read_to_string(base_dir.join(pack).join("pack.toml")).ok()?;
    let parsed: serde_json::Value = toml::from_str(&text).ok()?;
    // **camelCase the whole table, not just guidance.** A TOML author writes
    // `match_key` and `party_id`; every response on this surface is read as
    // JSON, where those are `matchKey` and `partyId`. Converting only part of
    // it is worse than converting none: `match_key` reached the console as a
    // key it never read, so the reconciliation silently fell back to matching
    // on the printed identity — a wrong answer that looks like a working page.
    let mut console = camel_case_keys(parsed.get("console").cloned()?);

    // **Guidance travels with the console config, because nothing else carries
    // it.** `[findings.guidance]` is the reviewer-facing wording for each rule
    // — what it means and what to do — and the finding-rule registry the loader
    // POSTs has no field for it. Without this the console renders a bare label
    // and the words sit unread in the manifest.
    //
    // Keys are camelCased on the way out: a TOML author writes `next_action`
    // and the console reads `nextAction`, which is the convention every other
    // response on this surface already uses.
    let mut guidance = serde_json::Map::new();
    if let Some(findings) = parsed.get("findings").and_then(|f| f.as_array()) {
        for finding in findings {
            let (Some(label), Some(declared)) = (
                finding.get("label").and_then(|l| l.as_str()),
                finding.get("guidance").and_then(|g| g.as_object()),
            ) else {
                continue;
            };
            let camel: serde_json::Map<String, serde_json::Value> = declared
                .iter()
                .map(|(key, value)| (snake_to_camel(key), value.clone()))
                .collect();
            guidance.insert(label.to_string(), serde_json::Value::Object(camel));
        }
    }
    if let Some(object) = console.as_object_mut() {
        object.insert("guidance".to_string(), serde_json::Value::Object(guidance));
    }
    Some(console)
}

/// A pack's own `[[matching.blocking]]` declarations — Plan 111 Slice D.
///
/// **These have been declared by both shipped packs since Epic 105 and read
/// by nothing.** `graph_owl_core::blocking_strategy` is 963 lines and 38
/// tests of domain-neutral blocking with, until this, **zero callers
/// anywhere in the workspace** — the same defect as `shortest_path` one crate
/// over: a capability that stops at the engine is a tested function nobody
/// can reach.
///
/// Read at request time from `pack.toml`, exactly as
/// [`read_console_config`] reads `[console]`, and for the same reason: the
/// manifest stays the single source of truth, with no migration, no storage
/// table and no loader change.
///
/// **A malformed declaration yields nothing rather than a partial list.** A
/// pack that asked for three strategies and silently gets two blocks on
/// weaker evidence than it configured, finds fewer near-misses, and reports
/// nothing wrong — the same all-or-nothing reasoning `Strategy::key` already
/// applies to a composite's parts.
#[must_use]
pub fn read_blocking_strategies(
    base_dir: &Path,
    pack: &str,
) -> Vec<graph_owl_core::blocking_strategy::Strategy> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        matching: Option<Matching>,
    }
    #[derive(serde::Deserialize)]
    struct Matching {
        #[serde(default)]
        blocking: Vec<graph_owl_core::blocking_strategy::Strategy>,
    }

    if !crate::pack_id_is_safe(pack) {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(base_dir.join(pack).join("pack.toml")) else {
        return Vec::new();
    };
    toml::from_str::<Manifest>(&text)
        .ok()
        .and_then(|manifest| manifest.matching)
        .map(|matching| matching.blocking)
        .unwrap_or_default()
}

/// A pack's declared `prefix` and `namespace` IRI — the two halves needed to
/// turn `gst:supplierGstin` in a `[[matching.blocking]]` declaration into an
/// identifier the graph engine understands.
///
/// **The resolution happens here, not in `graph-owl-api`.** That crate does
/// not read `pack.toml` and must not learn to: a facade that could see a
/// pack's prefixed vocabulary is a facade that can name a domain.
#[must_use]
pub fn read_pack_vocabulary(base_dir: &Path, pack: &str) -> Option<(String, String)> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        pack: Vocabulary,
    }
    #[derive(serde::Deserialize)]
    struct Vocabulary {
        prefix: String,
        namespace: String,
    }

    if !crate::pack_id_is_safe(pack) {
        return None;
    }
    let text = std::fs::read_to_string(base_dir.join(pack).join("pack.toml")).ok()?;
    let manifest: Manifest = toml::from_str(&text).ok()?;
    Some((manifest.pack.prefix, manifest.pack.namespace))
}

/// Rewrite a strategy's field names from a pack's prefixed vocabulary into
/// rendered `Sid`s (`code:local`).
///
/// **A field this cannot resolve is dropped, and `values`' all-or-nothing
/// rule then makes the whole strategy unkeyable.** That is the safe
/// direction: a strategy silently keyed on a subset of the fields the pack
/// named would block records together on weaker evidence than the
/// configuration asked for, invisibly.
#[must_use]
pub fn resolve_strategy_fields(
    strategy: &graph_owl_core::blocking_strategy::Strategy,
    prefix: &str,
    code: u16,
) -> graph_owl_core::blocking_strategy::Strategy {
    use graph_owl_core::blocking_strategy::Strategy;

    let rewrite = |fields: &Vec<String>| -> Vec<String> {
        fields
            .iter()
            .filter_map(|field| {
                let local = field.strip_prefix(prefix)?.strip_prefix(':')?;
                Some(format!("{code}:{local}"))
            })
            .collect()
    };

    match strategy {
        Strategy::Exact { fields } => Strategy::Exact {
            fields: rewrite(fields),
        },
        Strategy::Normalized { fields } => Strategy::Normalized {
            fields: rewrite(fields),
        },
        Strategy::Phonetic { fields } => Strategy::Phonetic {
            fields: rewrite(fields),
        },
        Strategy::NGram { fields, n } => Strategy::NGram {
            fields: rewrite(fields),
            n: *n,
        },
        Strategy::NumericBucket { fields, width } => Strategy::NumericBucket {
            fields: rewrite(fields),
            width: *width,
        },
        Strategy::DateWindow { fields, days } => Strategy::DateWindow {
            fields: rewrite(fields),
            days: *days,
        },
        Strategy::Composite { of } => Strategy::Composite {
            of: of
                .iter()
                .map(|part| resolve_strategy_fields(part, prefix, code))
                .collect(),
        },
    }
}

/// Every key in a value tree, camelCased, recursively.
fn camel_case_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, inner)| (snake_to_camel(&key), camel_case_keys(inner)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(camel_case_keys).collect())
        }
        other => other,
    }
}

/// `next_action` → `nextAction`. The manifest is written in TOML's idiom and
/// every response on this surface is read in JSON's; converting here keeps a
/// pack author from having to know which side they are writing for.
fn snake_to_camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for ch in key.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fresh, uniquely-named temp directory per test — no shared state,
    /// so these are safe under `cargo test`'s default concurrent execution
    /// (unlike mutating `GRAPH_OWL_PACKS_DIR`, which every test in this
    /// process would race over).
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "graph-owl-pack-scan-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_pack(base: &Path, id: &str, description: &str) {
        let dir = base.join(id);
        fs::create_dir_all(&dir).expect("create pack dir");
        fs::write(
            dir.join("pack.toml"),
            format!("[pack]\nid = \"{id}\"\nnamespace = \"https://example.dev/{id}#\"\nprefix = \"{id}\"\ndescription = \"{description}\"\n"),
        )
        .expect("write pack.toml");
    }

    #[test]
    fn lists_a_pack_whose_manifest_is_readable() {
        let base = temp_dir("lists-one");
        write_pack(&base, "gst", "Reconcile purchase register against GSTR-2B.");

        let found = scan_available_packs(&base, &HashSet::new());

        assert_eq!(
            found,
            vec![AvailablePack {
                id: "gst".to_string(),
                description: "Reconcile purchase register against GSTR-2B.".to_string(),
            }]
        );
    }

    #[test]
    fn omits_a_pack_already_installed() {
        let base = temp_dir("omits-installed");
        write_pack(&base, "gst", "Reconcile purchase register against GSTR-2B.");
        write_pack(
            &base,
            "hospitality",
            "Reconcile PMS bookings against GST invoices.",
        );

        let installed: HashSet<String> = ["gst".to_string()].into_iter().collect();
        let found = scan_available_packs(&base, &installed);

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].id, "hospitality");
    }

    #[test]
    fn a_directory_with_no_pack_toml_is_skipped_not_an_error() {
        let base = temp_dir("skips-non-pack-dir");
        write_pack(&base, "gst", "Reconcile purchase register against GSTR-2B.");
        fs::create_dir_all(base.join("not-a-pack")).expect("create stray dir");

        let found = scan_available_packs(&base, &HashSet::new());

        assert_eq!(
            found.len(),
            1,
            "the stray directory must not appear: {found:?}"
        );
        assert_eq!(found[0].id, "gst");
    }

    #[test]
    fn a_pack_toml_that_fails_to_parse_is_skipped_not_a_panic() {
        let base = temp_dir("skips-malformed");
        write_pack(&base, "gst", "Reconcile purchase register against GSTR-2B.");
        let broken = base.join("broken");
        fs::create_dir_all(&broken).expect("create broken pack dir");
        fs::write(broken.join("pack.toml"), "this is not valid toml [[[").expect("write");

        let found = scan_available_packs(&base, &HashSet::new());

        assert_eq!(
            found.len(),
            1,
            "the malformed pack must not appear: {found:?}"
        );
        assert_eq!(found[0].id, "gst");
    }

    #[test]
    fn a_missing_base_directory_returns_empty_not_an_error() {
        let missing = std::env::temp_dir().join("graph-owl-pack-scan-does-not-exist-at-all");
        let _ = fs::remove_dir_all(&missing);

        let found = scan_available_packs(&missing, &HashSet::new());

        assert_eq!(found, Vec::new());
    }

    #[test]
    fn results_are_sorted_by_id() {
        let base = temp_dir("sorted");
        write_pack(
            &base,
            "hospitality",
            "Reconcile PMS bookings against GST invoices.",
        );
        write_pack(&base, "gst", "Reconcile purchase register against GSTR-2B.");

        let found = scan_available_packs(&base, &HashSet::new());

        assert_eq!(
            found.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["gst", "hospitality"]
        );
    }

    /// **The reconciliation page was GST-shaped, and a second domain made that
    /// a defect rather than a shortcut.** Its source list, its measures and its
    /// per-finding guidance all lived in TypeScript, so installing a
    /// healthcare or banking pack would render that pack's data under GST's
    /// headings — or nothing at all. The shape of the page (sources in, rules
    /// run, a statement out, exceptions to work) is genuinely domain-neutral;
    /// everything that names a domain now comes from the pack that owns it.
    ///
    /// Read from `pack.toml` at request time rather than stored, exactly as
    /// [`scan_available_packs`] already reads the `[pack]` header — no
    /// migration, no loader change, and the manifest stays the single source
    /// of truth for what a pack is.
    #[test]
    fn reads_a_packs_declared_console_configuration() {
        let base = temp_dir("console-config");
        let pack = base.join("gst");
        fs::create_dir_all(&pack).expect("mkdir");
        fs::write(
            pack.join("pack.toml"),
            r#"
[pack]
id = "gst"
description = "d"

[console]
[console.reconciliation]
label = "GST reconciliation"
currency = "INR"
locale = "en-IN"

[[console.reconciliation.sources]]
key = "books"
class = "PurchaseInvoice"
label = "Your books"
"#,
        )
        .expect("write");

        let found = read_console_config(&base, "gst").expect("console section");

        assert_eq!(found["reconciliation"]["label"], "GST reconciliation");
        assert_eq!(
            found["reconciliation"]["sources"][0]["class"],
            "PurchaseInvoice"
        );
    }

    /// **Guidance travels with the console config, because nothing else
    /// carries it.** `[findings.guidance]` is the CA-facing wording for each
    /// rule — what it means and what to do about it — and it lives on the rule
    /// in `pack.toml`. The finding-rule registry the loader POSTs has no field
    /// for it, so without this the console falls back to rendering a bare
    /// label and the words sit in the manifest unread.
    #[test]
    fn carries_each_rules_declared_guidance() {
        let base = temp_dir("console-guidance");
        let pack = base.join("gst");
        fs::create_dir_all(&pack).expect("mkdir");
        fs::write(
            pack.join("pack.toml"),
            r#"
[pack]
id = "gst"
description = "d"

[[findings]]
label = "gst:SupplierNotFiled"
summary = "written for a reviewer of the rule"

[findings.guidance]
title = "Supplier has not filed"
meaning = "You booked it and nobody declared it."
next_action = "Chase the supplier."
tone = "danger"

[console]
[console.reconciliation]
label = "GST reconciliation"
"#,
        )
        .expect("write");

        let found = read_console_config(&base, "gst").expect("console section");

        assert_eq!(found["reconciliation"]["label"], "GST reconciliation");
        assert_eq!(
            found["guidance"]["gst:SupplierNotFiled"]["title"],
            "Supplier has not filed"
        );
        // camelCase on the wire, snake_case in the manifest — the console reads
        // one convention and a TOML author writes the other.
        assert_eq!(
            found["guidance"]["gst:SupplierNotFiled"]["nextAction"],
            "Chase the supplier."
        );
    }

    /// **Converting only part of the table is worse than converting none.**
    /// A TOML author writes `match_key`; the console reads `matchKey`. When
    /// only guidance was camelCased, the reconciliation's own key arrived under
    /// a name nothing read and the page silently fell back to matching on the
    /// printed identity — the exact bug the key exists to prevent, reintroduced
    /// by a serialization detail.
    #[test]
    fn camel_cases_every_key_not_only_guidance() {
        let base = temp_dir("console-camel");
        let pack = base.join("gst");
        fs::create_dir_all(&pack).expect("mkdir");
        fs::write(
            pack.join("pack.toml"),
            r#"
[pack]
id = "gst"
description = "d"

[console]
[console.reconciliation]
match_key = "invoiceKey"
record_noun = "invoice"

[console.reconciliation.fields]
party_id = "supplierGstin"
"#,
        )
        .expect("write");

        let found = read_console_config(&base, "gst").expect("console section");

        assert_eq!(found["reconciliation"]["matchKey"], "invoiceKey");
        assert_eq!(found["reconciliation"]["recordNoun"], "invoice");
        assert_eq!(
            found["reconciliation"]["fields"]["partyId"],
            "supplierGstin"
        );
    }

    /// **Against the pack that actually ships.** Plan 111 Slice E moved the
    /// console's import surfaces into `[[console.imports]]`, and the console
    /// reads them as JSON — so `how_to_obtain` has to arrive as
    /// `howToObtain`. A key that camelCases wrong reaches the console under a
    /// name nothing reads, which is the exact failure `match_key` already had
    /// once: a working-looking page silently missing a field.
    #[test]
    fn a_shipped_packs_import_surfaces_arrive_camel_cased() {
        let packs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let console = read_console_config(&packs, "gst").expect("gst declares a console");

        let imports = console["imports"].as_array().expect("declared imports");
        assert!(!imports.is_empty(), "gst declares its own upload surfaces");
        for surface in imports {
            assert!(surface["key"].is_string(), "{surface}");
            assert!(surface["format"].is_string(), "{surface}");
            assert!(
                surface["howToObtain"].is_string(),
                "`how_to_obtain` must reach the console camelCased: {surface}",
            );
        }
    }

    /// A pack that declares no console section is the ordinary case, not a
    /// failure — hospitality has never had one. The console must render an
    /// honest empty state for it rather than an error.
    #[test]
    fn a_pack_with_no_console_section_reports_none() {
        let base = temp_dir("console-none");
        write_pack(&base, "hospitality", "d");

        assert!(read_console_config(&base, "hospitality").is_none());
    }

    /// The same path-traversal refusal `pack_id_is_safe` already applies to
    /// installing — an id is joined onto a filesystem path here too.
    #[test]
    fn refuses_a_path_traversal_shaped_id() {
        let base = temp_dir("console-traversal");

        assert!(read_console_config(&base, "../../etc").is_none());
    }

    /// Plan 111 Slice D — the pack's blocking strategies stop being
    /// configuration nothing reads.
    mod blocking_strategies_are_read {
        use super::*;

        /// **Against the packs that actually ship, not a fixture.** A
        /// hand-written fixture is a second opinion about what a pack looks
        /// like, and the two drift — this is the assertion that would have
        /// caught `strategy = "ngram"` against a derive that produced
        /// `n_gram`, which is exactly what it did catch one crate over.
        #[test]
        fn every_shipped_pack_s_declarations_parse() {
            let packs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
            let mut ids: Vec<String> = std::fs::read_dir(&packs)
                .expect("the packs directory")
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    entry
                        .path()
                        .join("pack.toml")
                        .exists()
                        .then(|| entry.file_name().to_string_lossy().into_owned())
                })
                .collect();
            ids.sort();
            assert!(
                ids.len() >= 2,
                "expected at least two shipped packs: {ids:?}"
            );

            for id in &ids {
                let declared = read_blocking_strategies(&packs, id);
                // Every pack in the tree today declares blocking. If one
                // stops, this should be relaxed deliberately rather than by
                // an empty vector quietly passing.
                assert!(
                    !declared.is_empty(),
                    "`{id}` declares `[[matching.blocking]]` and it did not parse",
                );
            }
        }

        /// The composite is the one worth asserting whole: its parts are
        /// strategies in their own right, so a tagging scheme that works at
        /// the top level and not inside `of` passes every other check.
        #[test]
        fn a_composite_declaration_keeps_its_parts() {
            use graph_owl_core::blocking_strategy::Strategy;
            let packs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");

            let composites: Vec<_> = read_blocking_strategies(&packs, "gst")
                .into_iter()
                .filter_map(|strategy| match strategy {
                    Strategy::Composite { of } => Some(of),
                    _ => None,
                })
                .collect();

            assert_eq!(composites.len(), 1, "gst declares one composite");
            assert_eq!(
                composites[0].len(),
                2,
                "with two parts: {:?}",
                composites[0]
            );
            assert!(
                matches!(composites[0][1], Strategy::DateWindow { days: 7, .. }),
                "the second part is a 7-day window: {:?}",
                composites[0][1],
            );
        }

        /// A pack that declares no matching table is not an error — it simply
        /// blocks on nothing, which is the honest reading of saying nothing.
        #[test]
        fn a_pack_with_no_matching_table_declares_nothing() {
            let base = temp_dir("blocking-none");
            write_pack(&base, "quiet", "d");

            assert!(read_blocking_strategies(&base, "quiet").is_empty());
        }

        #[test]
        fn refuses_a_path_traversal_shaped_id() {
            let base = temp_dir("blocking-traversal");

            assert!(read_blocking_strategies(&base, "../../etc").is_empty());
        }

        /// Both shipped packs declare a prefix and a namespace, and the
        /// blocking fields are written in that prefix — the two have to be
        /// read from the same file or a rename in one desynchronises the
        /// other silently.
        #[test]
        fn a_packs_own_prefix_and_namespace_are_readable() {
            let packs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");

            let (prefix, namespace) =
                read_pack_vocabulary(&packs, "hospitality").expect("hospitality declares both");
            assert_eq!(prefix, "hosp");
            assert!(namespace.ends_with('#'), "{namespace}");
        }

        /// **Resolution is what keeps `graph-owl-api` unable to name a
        /// domain.** The facade sees `1024:supplierGstin`; only this layer
        /// ever sees `gst:`.
        #[test]
        fn fields_are_rewritten_from_the_packs_prefix_into_rendered_ids() {
            use graph_owl_core::blocking_strategy::Strategy;

            let resolved = resolve_strategy_fields(
                &Strategy::Normalized {
                    fields: vec!["ns:partyId".to_string(), "ns:documentNumber".to_string()],
                },
                "ns",
                1024,
            );

            assert_eq!(
                resolved,
                Strategy::Normalized {
                    fields: vec![
                        "1024:partyId".to_string(),
                        "1024:documentNumber".to_string()
                    ],
                },
            );
        }

        /// A composite's parts are strategies in their own right, so a
        /// rewrite that stops at the top level leaves the part that does the
        /// real narrowing pointing at names nothing resolves.
        #[test]
        fn a_composites_parts_are_rewritten_too() {
            use graph_owl_core::blocking_strategy::Strategy;

            let resolved = resolve_strategy_fields(
                &Strategy::Composite {
                    of: vec![Strategy::DateWindow {
                        fields: vec!["ns:documentDate".to_string()],
                        days: 7,
                    }],
                },
                "ns",
                1024,
            );

            assert_eq!(
                resolved,
                Strategy::Composite {
                    of: vec![Strategy::DateWindow {
                        fields: vec!["1024:documentDate".to_string()],
                        days: 7,
                    }],
                },
            );
        }

        /// **A field in someone else's prefix is dropped, not guessed at.**
        /// `values`' all-or-nothing rule then makes the strategy unkeyable,
        /// which is the safe direction: a key built from a subset of the
        /// named fields blocks on weaker evidence than the pack asked for,
        /// and reports nothing wrong.
        #[test]
        fn a_field_in_another_prefix_is_dropped_rather_than_assumed() {
            use graph_owl_core::blocking_strategy::Strategy;

            let resolved = resolve_strategy_fields(
                &Strategy::Normalized {
                    fields: vec!["ns:partyId".to_string(), "other:thing".to_string()],
                },
                "ns",
                1024,
            );

            assert_eq!(
                resolved,
                Strategy::Normalized {
                    fields: vec!["1024:partyId".to_string()],
                },
            );
        }
    }
}
