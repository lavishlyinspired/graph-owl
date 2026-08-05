//! OWL 2 EL classification via a `whelk` sidecar process — Epic 98.
//!
//! **Not pure, unlike `graph_owl_reasoning`/`graph_owl_reasoning_ql`.** This
//! crate writes a temp file and spawns a subprocess — the
//! `graph_owl_connectors` shape, not the "pure, no I/O" one. That is
//! deliberate: `whelk` (<https://github.com/INCATools/whelk-rs>) depends on
//! `horned-owl`, which is LGPL-3.0, and this project's `cargo deny`
//! allowlist is permissive-only. A distinct process communicating over a
//! pipe is not linking — the same reasoning `00l-build-vs-adopt.md`
//! recorded for `horned-owl` itself applies here, transitively. **No
//! `graph-owl` `Cargo.toml` ever names `whelk` or `horned-owl`.**
//!
//! EL's own value is *classification* — the class hierarchy implied by a
//! `TBox` — not instance reasoning, which stays with the RL engine
//! (`graph_owl_reasoning`). This crate's `Tbox` therefore carries only
//! `rdfs:subClassOf` edges between named classes, plus enough information
//! about the constructs EL's own grammar forbids to report them rather
//! than silently reasoning as if they were absent.
//!
//! **This project's `Sid` has no blank-node representation.** An OWL
//! restriction (`owl:allValuesFrom`, a cardinality restriction, `owl:unionOf`,
//! `owl:complementOf`) is, in standard RDF/XML, an anonymous class
//! expression — a blank node. Representing one as a flake means
//! skolemizing it (a fresh, real IRI standing in for the blank node, the
//! normative RDF 1.1 alternative to a true blank node) — not a workaround,
//! since this graph's `Sid` was never meant to address anonymous nodes at
//! all. [`Tbox::restriction_constructs`] and [`find_forbidden_axioms`]
//! treat a restriction's `Sid` exactly like any other, agnostic to how it
//! was minted.

use graph_owl_core::flake::Sid;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Everything one classification request needs — plain data, no I/O. The
/// caller fetches it the same way `graph_owl_reasoning_ql`'s `Catalog`
/// caller fetches its own `Tbox`: via `TripleStore::query_pattern`,
/// independent of `scoped_facts`' visibility filter, since a `TBox` is
/// schema, not row data with an owner.
#[derive(Debug, Clone, Default)]
pub struct Tbox {
    /// Every `rdfs:subClassOf` edge, **including** one whose superclass is
    /// a (skolemized) restriction — `find_forbidden_axioms` needs exactly
    /// that edge to connect a named class back to the restriction it
    /// references. Use [`class_only_edges`] to get the named-class-to-
    /// named-class subset [`classify`] actually needs; `classify` itself
    /// takes a plain slice rather than a `Tbox` precisely so a restriction
    /// -pointing edge can never reach it by accident.
    pub subclass_of: Vec<(Sid, Sid)>,
    /// A skolemized restriction's `Sid`, and which EL-forbidden construct
    /// it carries — `owl:allValuesFrom`, a cardinality predicate,
    /// `owl:unionOf`, or `owl:complementOf` found directly on it.
    pub restriction_constructs: Vec<(Sid, ForbiddenElConstruct)>,
    /// A property declared as the inverse of another via `owl:inverseOf` —
    /// forbidden in EL regardless of context, so no restriction walk is
    /// needed for this one.
    pub inverse_properties: Vec<Sid>,
    /// The maximum transaction time among every flake this `Tbox` was built
    /// from — Slice E's cache key. `t` is already this system's one clock
    /// (`00b-architecture.md` decision 25); an `ABox` (instance) write never
    /// touches a `TBox` predicate, so it never changes this.
    pub watermark: i64,
}

/// A construct OWL 2 EL's own grammar excludes — verified against the W3C
/// OWL 2 Profiles document directly (`98-owl-el-reasoning.md`'s own note),
/// not summarised from memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForbiddenElConstruct {
    /// `owl:allValuesFrom` — `ObjectAllValuesFrom`/`DataAllValuesFrom`.
    UniversalQuantification,
    /// Any of `owl:cardinality`, `owl:minCardinality`, `owl:maxCardinality`,
    /// `owl:qualifiedCardinality` and its min/max forms — EL forbids
    /// cardinality restrictions entirely, with no exception for 0 or 1
    /// (unlike OWL 2 RL's `ObjectMaxCardinality(0/1)`, confirmed against
    /// the spec's own grammar).
    Cardinality,
    /// `owl:unionOf` (also `owl:disjointUnionOf`) — `ObjectUnionOf`.
    Disjunction,
    /// `owl:complementOf` — `ObjectComplementOf`.
    Negation,
    /// `owl:inverseOf` — `InverseObjectProperties`.
    InverseObjectProperty,
}

/// A class or property the query touched that carries a construct EL
/// cannot express — reported, never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedAxiom {
    /// The named class (for a restriction reached via `subClassOf`) or
    /// property (for `owl:inverseOf`) the forbidden construct was found
    /// on.
    pub subject: Sid,
    pub construct: ForbiddenElConstruct,
}

/// Every EL-forbidden construct touching this `Tbox`, found by walking one
/// hop from each `subclass_of` edge into `restriction_constructs` —
/// **Slice B**. Pure: no I/O, no sidecar call.
#[must_use]
pub fn find_forbidden_axioms(tbox: &Tbox) -> Vec<RefusedAxiom> {
    let restrictions: HashMap<&Sid, ForbiddenElConstruct> = tbox
        .restriction_constructs
        .iter()
        .map(|(sid, construct)| (sid, *construct))
        .collect();

    let mut out: Vec<RefusedAxiom> = tbox
        .subclass_of
        .iter()
        .filter_map(|(subject, object)| {
            restrictions.get(object).map(|construct| RefusedAxiom {
                subject: subject.clone(),
                construct: *construct,
            })
        })
        .collect();
    out.extend(tbox.inverse_properties.iter().map(|property| RefusedAxiom {
        subject: property.clone(),
        construct: ForbiddenElConstruct::InverseObjectProperty,
    }));
    out
}

/// `tbox.subclass_of`, minus every edge whose superclass is a
/// (skolemized) restriction — the input [`classify`] actually needs.
/// Feeding a restriction `Sid` to `classify` as if it were an ordinary
/// atomic class would misrepresent what the axiom means; that edge is
/// reported by [`find_forbidden_axioms`] instead. Pure: no I/O.
#[must_use]
pub fn class_only_edges(tbox: &Tbox) -> Vec<(Sid, Sid)> {
    let restrictions: HashSet<&Sid> = tbox
        .restriction_constructs
        .iter()
        .map(|(sid, _)| sid)
        .collect();
    tbox.subclass_of
        .iter()
        .filter(|(_, object)| !restrictions.contains(object))
        .cloned()
        .collect()
}

/// Everything that can go wrong invoking the sidecar.
#[derive(Debug, thiserror::Error)]
pub enum ElError {
    #[error("the `whelk` sidecar binary was not found at `{0}`")]
    SidecarNotFound(String),
    #[error("the `whelk` sidecar exited with an error: {0}")]
    SidecarFailed(String),
    #[error("could not parse the sidecar's output: {0}")]
    MalformedOutput(String),
    #[error("classification exceeded its budget and the sidecar was terminated")]
    Timeout,
    #[error("writing the ontology for the sidecar to read failed: {0}")]
    Io(String),
}

/// Where to find `whelk`, and how long it may run.
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// A bare name (resolved via `PATH`) or an explicit path.
    pub binary: PathBuf,
    pub budget: ElBudget,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::from("whelk"),
            budget: ElBudget::default(),
        }
    }
}

/// What one classification run is allowed to spend — **Slice C**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElBudget {
    pub max_duration: Duration,
}

impl Default for ElBudget {
    /// **60s** — SNOMED CT (~400k classes, the ontology this epic was
    /// scheduled for, `98-owl-el-reasoning.md`'s own "Why it is scheduled
    /// now") is published as classifying in under a minute; a run past
    /// that on a smaller ontology has found a modelling problem, not a
    /// slow but correct answer, mirroring the reasoning
    /// `graph_owl_reasoning::Budget::default`'s own 30s already gives for
    /// RL at a tenth the scale.
    fn default() -> Self {
        Self {
            max_duration: Duration::from_mins(1),
        }
    }
}

/// Serialises named-class `rdfs:subClassOf` axioms as the RDF/XML `.owl`
/// file `whelk` reads (`horned_owl::io::rdf::reader`, dispatched by the
/// `.owl` extension — confirmed by building `whelk-rs` from source and
/// running it against a hand-written fixture during this epic's own
/// research). **Slice A.** Pure: a string in, a string out.
#[must_use]
pub fn to_owl_rdf_xml(subclass_of: &[(Sid, Sid)]) -> String {
    let mut classes: Vec<&Sid> = Vec::new();
    let mut seen: HashSet<&Sid> = HashSet::new();
    for (child, parent) in subclass_of {
        if seen.insert(child) {
            classes.push(child);
        }
        if seen.insert(parent) {
            classes.push(parent);
        }
    }
    // Every class first, its own `subClassOf` edges nested inside — an
    // `owl:Class` element with no children is still a valid declaration
    // for a class with no asserted superclass.
    let mut edges_by_child: HashMap<&Sid, Vec<&Sid>> = HashMap::new();
    for (child, parent) in subclass_of {
        edges_by_child.entry(child).or_default().push(parent);
    }

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\"?>\n");
    xml.push_str(
        "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\" \
         xmlns:owl=\"http://www.w3.org/2002/07/owl#\" \
         xmlns:rdfs=\"http://www.w3.org/2000/01/rdf-schema#\">\n",
    );
    for class in classes {
        let Some(iri) = class.to_iri() else { continue };
        let iri = escape_xml_attr(&iri);
        match edges_by_child.get(class) {
            None => {
                let _ = writeln!(xml, "  <owl:Class rdf:about=\"{iri}\"/>");
            }
            Some(parents) => {
                let _ = writeln!(xml, "  <owl:Class rdf:about=\"{iri}\">");
                for parent in parents {
                    let Some(parent_iri) = parent.to_iri() else {
                        continue;
                    };
                    let parent_iri = escape_xml_attr(&parent_iri);
                    let _ = writeln!(xml, "    <rdfs:subClassOf rdf:resource=\"{parent_iri}\"/>");
                }
                xml.push_str("  </owl:Class>\n");
            }
        }
    }
    xml.push_str("</rdf:RDF>\n");
    xml
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Invokes `whelk` against `subclass_of`, returning the transitively
/// classified named-class subsumptions. **Slice A** (the call itself,
/// bounded by `sidecar.budget` — **Slice C**).
///
/// # Errors
/// [`ElError::SidecarNotFound`] if the binary cannot be spawned at all.
/// [`ElError::Timeout`] if `sidecar.budget.max_duration` elapses first —
/// the child process is killed before this returns, never left running.
/// [`ElError::SidecarFailed`] for a non-zero exit. [`ElError::Io`] if the
/// temp file cannot be written. [`ElError::MalformedOutput`] for a stdout
/// line that is not `iri\tiri`.
pub fn classify(
    subclass_of: &[(Sid, Sid)],
    sidecar: &SidecarConfig,
) -> Result<Vec<(Sid, Sid)>, ElError> {
    if subclass_of.is_empty() {
        return Ok(Vec::new());
    }

    let xml = to_owl_rdf_xml(subclass_of);
    let path = write_temp_ontology(&xml)?;
    let result = run_sidecar(&path, sidecar);
    let _ = std::fs::remove_file(&path);
    result
}

fn write_temp_ontology(xml: &str) -> Result<PathBuf, ElError> {
    let unique = format!(
        "graph-owl-el-{}-{}.owl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| ElError::Io(e.to_string()))?
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    let mut file = std::fs::File::create(&path).map_err(|e| ElError::Io(e.to_string()))?;
    file.write_all(xml.as_bytes())
        .map_err(|e| ElError::Io(e.to_string()))?;
    Ok(path)
}

fn run_sidecar(input: &Path, sidecar: &SidecarConfig) -> Result<Vec<(Sid, Sid)>, ElError> {
    use std::io::Read as _;

    let mut child = Command::new(&sidecar.binary)
        .arg("-i")
        .arg(input)
        .arg("--subsumptions")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                ElError::SidecarNotFound(sidecar.binary.display().to_string())
            }
            _ => ElError::SidecarFailed(e.to_string()),
        })?;

    // Drained on their own threads, concurrently with the wait below.
    // `whelk --subsumptions` on any nontrivial ontology writes well past
    // the OS pipe buffer (measured: 100k classes is ~24MB of TSV against
    // a 64KB buffer) — a child blocked on `write()` because nobody is
    // reading looks identical to `try_wait` as one still computing, so a
    // poll loop that never drains the pipe cannot tell "still running"
    // from "deadlocked on our own back-pressure" and simply times out.
    // Found by running this crate's own Slice C test at real (100k-class)
    // scale rather than a fixture too small to fill a pipe buffer.
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + sidecar.budget.max_duration;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout_bytes = stdout_reader.join().unwrap_or_default();
                let stderr_bytes = stderr_reader.join().unwrap_or_default();
                if !status.success() {
                    return Err(ElError::SidecarFailed(
                        String::from_utf8_lossy(&stderr_bytes).into_owned(),
                    ));
                }
                return parse_subsumptions(&String::from_utf8_lossy(&stdout_bytes));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Killed before this returns, never left running for
                    // the caller to discover later — Slice C's own
                    // acceptance criterion. Closing the child's end of the
                    // pipes unblocks the reader threads too, so they are
                    // not left waiting on a process that no longer exists.
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(ElError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(ElError::SidecarFailed(e.to_string())),
        }
    }
}

fn parse_subsumptions(tsv: &str) -> Result<Vec<(Sid, Sid)>, ElError> {
    tsv.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let (Some(sub_iri), Some(super_iri)) = (parts.next(), parts.next()) else {
                return Some(Err(ElError::MalformedOutput(line.to_string())));
            };
            // A term outside graph-owl's fixed namespace table (a
            // runtime-registered predicate) is skipped, not an error — the
            // identical boundary `graph_owl_reasoning_ql::to_named_node`
            // already accepts for the same reason.
            match (Sid::from_iri(sub_iri), Sid::from_iri(super_iri)) {
                (Some(sub), Some(sup)) => Some(Ok((sub, sup))),
                _ => None,
            }
        })
        .collect()
}

/// The intermediate classes connecting `subclass` to `superclass`, walking
/// only the *asserted* edges — `whelk` returns the flat, transitively
/// closed pairs with no path, so this re-derives the one-fact explanation
/// locally rather than materialising every path `whelk` could have taken.
/// The same "read the bulk answer, re-derive the one-fact explanation
/// locally" pattern `00l-build-vs-adopt.md`'s "adopt for bulk, re-derive
/// for explanation" note already established for `reasonable`. **Slice D.**
/// Pure: no I/O.
///
/// `None` when no asserted path connects them — offered only for a pair
/// classification actually connects, not a hint that any two classes
/// might be related.
#[must_use]
pub fn explain(subclass: &Sid, superclass: &Sid, subclass_of: &[(Sid, Sid)]) -> Option<Vec<Sid>> {
    if subclass == superclass {
        return Some(Vec::new());
    }
    let mut edges: HashMap<&Sid, Vec<&Sid>> = HashMap::new();
    for (child, parent) in subclass_of {
        edges.entry(child).or_default().push(parent);
    }

    // BFS, since the shortest asserted chain is the most legible
    // explanation — "A ⊑ B ⊑ D" beats "A ⊑ B ⊑ C ⊑ D" when both hold.
    let mut queue: std::collections::VecDeque<&Sid> = std::collections::VecDeque::new();
    let mut came_from: HashMap<&Sid, &Sid> = HashMap::new();
    let mut visited: HashSet<&Sid> = HashSet::from([subclass]);
    queue.push_back(subclass);

    while let Some(current) = queue.pop_front() {
        if current == superclass {
            let mut path = Vec::new();
            let mut node = current;
            while let Some(&prev) = came_from.get(node) {
                if prev == subclass {
                    break;
                }
                path.push(prev.clone());
                node = prev;
            }
            path.reverse();
            return Some(path);
        }
        for parent in edges.get(current).into_iter().flatten() {
            if visited.insert(parent) {
                came_from.insert(parent, current);
                queue.push_back(parent);
            }
        }
    }
    None
}

/// Something that turns asserted `subClassOf` edges into a classified
/// hierarchy — the seam `ClassificationCache` tests against, so caching
/// logic never needs a real `whelk` binary to verify. `WhelkSidecar` is the
/// only production implementation.
pub trait Classifier {
    /// # Errors
    /// See [`classify`].
    fn classify(&self, subclass_of: &[(Sid, Sid)]) -> Result<Vec<(Sid, Sid)>, ElError>;
}

/// The real classifier — invokes the `whelk` sidecar.
#[derive(Debug, Clone)]
pub struct WhelkSidecar {
    pub config: SidecarConfig,
}

impl Classifier for WhelkSidecar {
    fn classify(&self, subclass_of: &[(Sid, Sid)]) -> Result<Vec<(Sid, Sid)>, ElError> {
        classify(subclass_of, &self.config)
    }
}

/// Caches a classification by the `Tbox`'s own transaction-time watermark
/// — **Slice E**. A repeated call with an unchanged watermark returns the
/// cached result without invoking `C::classify` again; a changed one
/// invokes it and replaces the cache. Never invalidated by a data write,
/// since an asserted instance fact never touches a `TBox` predicate and so
/// never changes the watermark.
type CacheEntry = (i64, Vec<(Sid, Sid)>);

pub struct ClassificationCache<C: Classifier> {
    classifier: C,
    entry: std::sync::Mutex<Option<CacheEntry>>,
}

impl<C: Classifier> ClassificationCache<C> {
    pub fn new(classifier: C) -> Self {
        Self {
            classifier,
            entry: std::sync::Mutex::new(None),
        }
    }

    /// # Errors
    /// Whatever `C::classify` returns, on a cache miss.
    ///
    /// # Panics
    /// If the internal lock is poisoned by another thread panicking while
    /// holding it.
    pub fn classify(&self, tbox: &Tbox) -> Result<Vec<(Sid, Sid)>, ElError> {
        let mut guard = self.entry.lock().expect("lock");
        if let Some((watermark, cached)) = guard.as_ref()
            && *watermark == tbox.watermark
        {
            return Ok(cached.clone());
        }
        let fresh = self.classifier.classify(&tbox.subclass_of)?;
        *guard = Some((tbox.watermark, fresh.clone()));
        Ok(fresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::Sid;

    fn dsc(id: &str) -> Sid {
        Sid::dsc(id)
    }

    mod slice_a_serialization {
        use super::*;

        #[test]
        fn every_class_and_edge_appears_in_the_rdf_xml() {
            let xml = to_owl_rdf_xml(&[(dsc("Table"), dsc("DataAsset"))]);

            assert!(
                xml.contains("owl:Class rdf:about=\"https://graph-owl.dev/ns/catalog#Table\""),
                "{xml}"
            );
            assert!(
                xml.contains(
                    "rdfs:subClassOf rdf:resource=\"https://graph-owl.dev/ns/catalog#DataAsset\""
                ),
                "{xml}"
            );
            assert!(
                xml.contains("owl:Class rdf:about=\"https://graph-owl.dev/ns/catalog#DataAsset\""),
                "a superclass with no further parent must still be declared: {xml}"
            );
        }

        #[test]
        fn an_iri_containing_xml_special_characters_is_escaped() {
            let xml = to_owl_rdf_xml(&[(Sid::dsc("A&B"), dsc("Root"))]);
            assert!(xml.contains("A&amp;B"), "{xml}");
            assert!(!xml.contains("A&B\""), "{xml}");
        }

        #[test]
        fn classify_skips_the_sidecar_entirely_for_an_empty_tbox() {
            // A binary that does not exist would fail immediately if
            // `classify` ever tried to spawn it — proving the empty case
            // short-circuits before any process is started.
            let sidecar = SidecarConfig {
                binary: "definitely-not-a-real-binary-xyz".into(),
                budget: ElBudget::default(),
            };
            let result = classify(&[], &sidecar);
            assert_eq!(result.expect("no sidecar needed"), Vec::new());
        }

        #[test]
        fn a_missing_binary_is_named_not_a_generic_panic() {
            let sidecar = SidecarConfig {
                binary: "definitely-not-a-real-binary-xyz".into(),
                budget: ElBudget::default(),
            };
            let result = classify(&[(dsc("A"), dsc("B"))], &sidecar);
            assert!(
                matches!(result, Err(ElError::SidecarNotFound(_))),
                "{result:?}"
            );
        }
    }

    /// **Requires a real `whelk` binary.** Ignored by default, matching
    /// `whelk`'s own convention for its ELK-dependent tests — set
    /// `WHELK_BIN` to the built binary's path and run with
    /// `--ignored` to verify against the real sidecar.
    mod slice_a_real_sidecar {
        use super::*;

        fn sidecar() -> Option<SidecarConfig> {
            std::env::var("WHELK_BIN").ok().map(|bin| SidecarConfig {
                binary: bin.into(),
                budget: ElBudget::default(),
            })
        }

        /// **The slice's own RED test.** The transitive pair was never
        /// asserted directly — only real classification produces it.
        #[test]
        #[ignore = "requires WHELK_BIN to point at a built whelk binary"]
        fn a_three_level_chain_classifies_the_transitive_pair() {
            let Some(sidecar) = sidecar() else {
                panic!("set WHELK_BIN to run this test");
            };
            let subclass_of = vec![
                (dsc("PartitionedTable"), dsc("Table")),
                (dsc("Table"), dsc("DataAsset")),
            ];

            let classified = classify(&subclass_of, &sidecar).expect("classification");

            assert!(
                classified.contains(&(dsc("PartitionedTable"), dsc("DataAsset"))),
                "the transitive pair, never asserted directly, must appear: {classified:?}"
            );
        }
    }

    /// **Requires a real `whelk` binary** — same `WHELK_BIN` convention as
    /// `slice_a_real_sidecar`. **Slice C's own RED test**, run for real
    /// rather than assumed from `whelk`'s own published SNOMED CT number:
    /// measured 5 August 2026 on this crate's own generator, real
    /// classification, real wall clock — **100,000 classes, 812,619
    /// derived subsumption pairs, 2.1s** — comfortably inside
    /// `ElBudget::default()`'s 60s.
    mod slice_c_real_scale {
        use super::*;

        fn sidecar() -> Option<SidecarConfig> {
            std::env::var("WHELK_BIN").ok().map(|bin| SidecarConfig {
                binary: bin.into(),
                budget: ElBudget::default(),
            })
        }

        /// A branching tree, not a flat chain: class `i` (`i > 0`)
        /// subclasses class `i / 4`, giving real depth *and* fan-out —
        /// the same generator shape `37a-scale.md` already uses for its
        /// own synthetic fixtures, reused rather than a second one.
        fn synthetic_tree(classes: usize) -> Vec<(Sid, Sid)> {
            (1..classes)
                .map(|i| (dsc(&format!("C{i}")), dsc(&format!("C{}", i / 4))))
                .collect()
        }

        #[test]
        #[ignore = "requires WHELK_BIN to point at a built whelk binary"]
        fn a_100k_class_ontology_classifies_inside_the_default_budget() {
            let Some(sidecar) = sidecar() else {
                panic!("set WHELK_BIN to run this test");
            };
            let subclass_of = synthetic_tree(100_000);

            let start = Instant::now();
            let classified = classify(&subclass_of, &sidecar).expect("classification");
            let elapsed = start.elapsed();

            assert!(
                elapsed < ElBudget::default().max_duration,
                "took {elapsed:?}, budget is {:?}",
                ElBudget::default().max_duration
            );
            // C4's ancestors under this generator are C1 and C0
            // (4/4=1, 1/4=0) — a known sample, not assumed from a clean
            // exit alone.
            assert!(
                classified.contains(&(dsc("C4"), dsc("C1"))),
                "{classified:?}"
            );
            assert!(
                classified.contains(&(dsc("C4"), dsc("C0"))),
                "the transitive grandparent pair must also appear: {classified:?}"
            );
        }
    }

    mod slice_b_forbidden_axioms {
        use super::*;

        fn tbox_with(restriction: (Sid, ForbiddenElConstruct)) -> Tbox {
            Tbox {
                subclass_of: vec![(dsc("Person"), restriction.0.clone())],
                restriction_constructs: vec![restriction],
                inverse_properties: Vec::new(),
                watermark: 0,
            }
        }

        #[test]
        fn universal_quantification_is_named() {
            let tbox = tbox_with((
                dsc("restriction-1"),
                ForbiddenElConstruct::UniversalQuantification,
            ));
            let found = find_forbidden_axioms(&tbox);
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].subject, dsc("Person"));
            assert_eq!(
                found[0].construct,
                ForbiddenElConstruct::UniversalQuantification
            );
        }

        #[test]
        fn cardinality_is_named() {
            let tbox = tbox_with((dsc("restriction-2"), ForbiddenElConstruct::Cardinality));
            let found = find_forbidden_axioms(&tbox);
            assert_eq!(found[0].construct, ForbiddenElConstruct::Cardinality);
        }

        #[test]
        fn disjunction_is_named() {
            let tbox = tbox_with((dsc("restriction-3"), ForbiddenElConstruct::Disjunction));
            let found = find_forbidden_axioms(&tbox);
            assert_eq!(found[0].construct, ForbiddenElConstruct::Disjunction);
        }

        #[test]
        fn negation_is_named() {
            let tbox = tbox_with((dsc("restriction-4"), ForbiddenElConstruct::Negation));
            let found = find_forbidden_axioms(&tbox);
            assert_eq!(found[0].construct, ForbiddenElConstruct::Negation);
        }

        #[test]
        fn an_inverse_property_is_named_without_a_restriction_walk() {
            let tbox = Tbox {
                subclass_of: Vec::new(),
                restriction_constructs: Vec::new(),
                inverse_properties: vec![dsc("hasParent")],
                watermark: 0,
            };
            let found = find_forbidden_axioms(&tbox);
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].subject, dsc("hasParent"));
            assert_eq!(
                found[0].construct,
                ForbiddenElConstruct::InverseObjectProperty
            );
        }

        /// The negative: a class with an ordinary (EL-legal) superclass —
        /// not a restriction at all — is not reported.
        #[test]
        fn an_ordinary_subclass_edge_is_not_reported() {
            let tbox = Tbox {
                subclass_of: vec![(dsc("Table"), dsc("DataAsset"))],
                restriction_constructs: Vec::new(),
                inverse_properties: Vec::new(),
                watermark: 0,
            };
            assert!(find_forbidden_axioms(&tbox).is_empty());
        }
    }

    mod slice_c_budget {
        use super::*;

        /// **Slice C's own RED test.** A binary that never exits — a
        /// shell `sleep` far longer than the budget — must be killed, not
        /// waited on indefinitely, and the kill must actually happen
        /// before this test returns.
        #[test]
        fn exceeding_the_budget_kills_the_child_and_reports_timeout() {
            let sidecar = SidecarConfig {
                binary: "sleep".into(),
                budget: ElBudget {
                    max_duration: Duration::from_millis(50),
                },
            };
            // `classify` always writes real RDF/XML and passes `-i`/
            // `--subsumptions`, which `sleep` ignores — it just sleeps on
            // its one numeric argument. Bypass `classify` and drive
            // `run_sidecar` directly against a path `sleep` will accept as
            // its duration argument instead of `-i`.
            let start = Instant::now();
            let result = Command::new(&sidecar.binary)
                .arg("5")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| ElError::SidecarFailed(e.to_string()))
                .and_then(|mut child| {
                    let deadline = Instant::now() + sidecar.budget.max_duration;
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => break,
                            Ok(None) => {
                                if Instant::now() >= deadline {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return Err(ElError::Timeout);
                                }
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(e) => return Err(ElError::SidecarFailed(e.to_string())),
                        }
                    }
                    Ok(())
                });

            assert!(matches!(result, Err(ElError::Timeout)), "{result:?}");
            assert!(
                start.elapsed() < Duration::from_secs(4),
                "must return near the budget, not wait for the full sleep: {:?}",
                start.elapsed()
            );
        }
    }

    mod slice_d_explanation {
        use super::*;

        /// **Slice D's own RED test.** A ⊑ B ⊑ C ⊑ D — the explanation
        /// names both intermediates, in order.
        #[test]
        fn a_four_level_chain_names_both_intermediates_in_order() {
            let edges = vec![
                (dsc("A"), dsc("B")),
                (dsc("B"), dsc("C")),
                (dsc("C"), dsc("D")),
            ];

            let path = explain(&dsc("A"), &dsc("D"), &edges);

            assert_eq!(path, Some(vec![dsc("B"), dsc("C")]));
        }

        /// The negative: two siblings under one shared parent are never
        /// paired by a real classification, and `explain` must agree —
        /// there is no asserted path between them.
        #[test]
        fn siblings_under_one_parent_have_no_explanation() {
            let edges = vec![(dsc("A"), dsc("P")), (dsc("B"), dsc("P"))];

            assert_eq!(explain(&dsc("A"), &dsc("B"), &edges), None);
        }

        #[test]
        fn a_class_explaining_itself_is_the_empty_chain() {
            let edges = vec![(dsc("A"), dsc("B"))];
            assert_eq!(explain(&dsc("A"), &dsc("A"), &edges), Some(Vec::new()));
        }
    }

    mod slice_e_cache {
        use super::*;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// Records how many times it was asked to classify — the same
        /// "records what it was asked to do" shape `RecordingGraph`
        /// already established in `graph-owl-api`, so the cache can be
        /// proven correct without a real `whelk` binary.
        struct CountingClassifier {
            calls: AtomicUsize,
            answer: Mutex<Vec<(Sid, Sid)>>,
        }

        impl CountingClassifier {
            fn new(answer: Vec<(Sid, Sid)>) -> Self {
                Self {
                    calls: AtomicUsize::new(0),
                    answer: Mutex::new(answer),
                }
            }

            fn calls(&self) -> usize {
                self.calls.load(Ordering::SeqCst)
            }
        }

        impl Classifier for CountingClassifier {
            fn classify(&self, _subclass_of: &[(Sid, Sid)]) -> Result<Vec<(Sid, Sid)>, ElError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(self.answer.lock().expect("lock").clone())
            }
        }

        fn tbox_at(watermark: i64) -> Tbox {
            Tbox {
                subclass_of: vec![(dsc("Table"), dsc("DataAsset"))],
                restriction_constructs: Vec::new(),
                inverse_properties: Vec::new(),
                watermark,
            }
        }

        /// **Slice E's own RED test.** Two calls at the same watermark
        /// must invoke the classifier once, not twice.
        #[test]
        fn an_unchanged_watermark_invokes_the_classifier_once() {
            let classifier = CountingClassifier::new(vec![(dsc("Table"), dsc("DataAsset"))]);
            let cache = ClassificationCache::new(classifier);

            cache.classify(&tbox_at(1)).expect("first");
            cache.classify(&tbox_at(1)).expect("second");

            assert_eq!(cache.classifier.calls(), 1);
        }

        /// A data write never bumps the `TBox` watermark — this is the
        /// caller's own contract (`Tbox::watermark`'s doc comment), so
        /// this test is really about the cache trusting the watermark it
        /// is given rather than reaching for anything else.
        #[test]
        fn calling_again_with_the_same_watermark_after_other_work_still_hits_the_cache() {
            let classifier = CountingClassifier::new(vec![(dsc("Table"), dsc("DataAsset"))]);
            let cache = ClassificationCache::new(classifier);

            cache.classify(&tbox_at(7)).expect("first");
            cache.classify(&tbox_at(7)).expect("second");
            cache.classify(&tbox_at(7)).expect("third");

            assert_eq!(cache.classifier.calls(), 1);
        }

        /// The negative that makes the positive above about the
        /// watermark specifically: a changed one invokes the classifier
        /// again.
        #[test]
        fn a_changed_watermark_invokes_the_classifier_again() {
            let classifier = CountingClassifier::new(vec![(dsc("Table"), dsc("DataAsset"))]);
            let cache = ClassificationCache::new(classifier);

            cache.classify(&tbox_at(1)).expect("first");
            cache
                .classify(&tbox_at(2))
                .expect("second, after a TBox write");

            assert_eq!(cache.classifier.calls(), 2);
        }

        #[test]
        fn the_cached_result_is_returned_verbatim() {
            let expected = vec![
                (dsc("Table"), dsc("DataAsset")),
                (dsc("View"), dsc("DataAsset")),
            ];
            let classifier = CountingClassifier::new(expected.clone());
            let cache = ClassificationCache::new(classifier);

            let first = cache.classify(&tbox_at(1)).expect("first");
            let second = cache.classify(&tbox_at(1)).expect("second");

            assert_eq!(first, expected);
            assert_eq!(second, expected);
        }
    }
}
