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
}
