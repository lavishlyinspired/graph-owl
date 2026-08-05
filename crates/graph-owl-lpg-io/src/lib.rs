//! Property-graph interchange at the boundary — Epic 9a.
//!
//! **Symmetric with Epic 9, same streaming discipline, different formats**
//! (the plan's own framing). `LpgNode`/`LpgEdge` already exist in
//! `graph-owl-lpg` (Epic 7c) — this crate turns them into interchange
//! bytes and back, and never becomes the graph's own model.
//!
//! **Slice A only**: streaming `GraphML` export. `LpgWriter`/`LpgReader`
//! name the full trait shape the plan specifies so later slices (bulk
//! CSV, Cypher script, JSON Graph, JSON Lines, `GraphML` import) extend
//! this crate rather than reshape it.
//!
//! ## Why `<key>` declarations do not force build-then-write
//!
//! `GraphML`'s own schema requires every `<key>` element to precede the
//! `<graph>` body — but a key's *name and type* are only known once data
//! carrying it has been seen, which looks like it forces buffering the
//! whole graph before anything can be written. It does not: node and edge
//! elements are written to two on-disk scratch files as they arrive
//! (never accumulated in a `Vec` or a `String` in memory — the same
//! "streams to disk, not RAM" pattern `graph-owl-api::archive` already
//! established for Epic 37b's export), while only the *schema* — one
//! entry per **distinct** property key encountered, bounded by the
//! catalog's own predicate vocabulary rather than by element count — is
//! held in memory. Each key's id is assigned once, the first time it is
//! seen, and never recomputed afterward: a `<data key="d3">` written
//! during `node()` must still mean the same thing when `finish()` writes
//! `<key id="d3">` later, so the id cannot depend on how many *other*
//! keys turn up afterward. [`GraphMlWriter::finish`] writes the real
//! header, the (small, bounded) `<key>` declarations, and streams the two
//! scratch files' bytes straight into the final output with
//! [`std::io::copy`], never re-reading either into memory as a whole.

use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use graph_owl_lpg::{LpgEdge, LpgNode, PropertyMap, PropertyValue};
use quick_xml::events::BytesText;
use quick_xml::writer::Writer as XmlWriter;

/// Why a graph could not cross the LPG interchange boundary.
#[derive(Debug, thiserror::Error)]
pub enum LpgIoError {
    /// A scratch file or the final output could not be read or written.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// `node()`/`edge()`/`finish()` was reached before `begin()` — a
    /// caller error, named rather than panicking on an unwrapped
    /// `Option`.
    #[error("begin() must be called before node(), edge(), or finish()")]
    NotStarted,
}

/// What one export run is named. `graph_id` becomes the `<graph id="...">`
/// attribute.
#[derive(Debug, Clone)]
pub struct ExportMeta {
    pub graph_id: String,
}

/// What a finished export actually wrote — the count a caller compares
/// against what it expected to send, so a truncated or partially-failed
/// export is detectable rather than silently accepted as complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSummary {
    pub nodes: u64,
    pub edges: u64,
}

/// Streaming write side of an interchange format — the plan's own
/// interface. `GraphML` needs nodes before edges (decision-consistent with
/// every LPG bulk-load convention this plan's own Slice C names); the
/// caller may still call `node`/`edge` in any interleaving, since
/// [`GraphMlWriter`] buffers each side to its own scratch file and orders
/// them only when [`finish`](Self::finish) assembles the final document.
pub trait LpgWriter {
    /// # Errors
    /// [`LpgIoError::Io`] if writing the scratch state fails.
    fn begin(&mut self, meta: &ExportMeta) -> Result<(), LpgIoError>;
    /// # Errors
    /// [`LpgIoError::NotStarted`] if called before [`begin`](Self::begin);
    /// [`LpgIoError::Io`] if writing fails.
    fn node(&mut self, n: &LpgNode) -> Result<(), LpgIoError>;
    /// # Errors
    /// [`LpgIoError::NotStarted`] if called before [`begin`](Self::begin);
    /// [`LpgIoError::Io`] if writing fails.
    fn edge(&mut self, e: &LpgEdge) -> Result<(), LpgIoError>;
    /// # Errors
    /// [`LpgIoError::Io`] if assembling the final output fails.
    fn finish(self) -> Result<ExportSummary, LpgIoError>;
}

/// Which side of the graph a property key was declared for — `GraphML`'s
/// own `for="node"` / `for="edge"` distinction. The same key name used on
/// both sides (e.g. `name`) needs two separate `<key>` declarations, one
/// per domain, since `GraphML` scopes a key's identity by domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum KeyDomain {
    Node,
    Edge,
}

/// A property key's declaration, assigned once and never changed —
/// `id` is what every `<data key="...">` this crate writes references,
/// so it must be stable for the whole export regardless of what other
/// keys are discovered afterward.
#[derive(Debug, Clone)]
struct KeyEntry {
    id: String,
    graphml_type: &'static str,
}

/// `GraphML`'s own attribute-type vocabulary this crate emits. `PropertyValue`
/// variants with no native `GraphML` equivalent (`Bytes`, `DateTime`,
/// `Duration`, `List`, `ElementRef`) render as `string` — the universal
/// fallback every `GraphML` consumer can read, at the cost of the reader
/// having to know out of band that (for example) a `string`-typed
/// `updatedAt` is really an RFC 3339 instant. That loss is Slice B's own
/// concern (typed round-trip through import); Slice A is export-only.
fn graphml_type(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Boolean(_) => "boolean",
        PropertyValue::Integer(_) => "long",
        PropertyValue::Float(_) => "double",
        PropertyValue::String(_)
        | PropertyValue::Bytes(_)
        | PropertyValue::DateTime(_)
        | PropertyValue::Duration(_)
        | PropertyValue::List(_)
        | PropertyValue::ElementRef(_) => "string",
    }
}

/// A property's value, rendered as the text a `<data>` element carries.
/// `quick_xml`'s own `BytesText::new`/attribute-`From` impls escape this
/// automatically — nothing here hand-escapes XML.
fn property_text(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Float(f) => f.to_string(),
        PropertyValue::String(s) => s.clone(),
        PropertyValue::Bytes(bytes) => bytes.iter().fold(String::new(), |mut acc, b| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{b:02x}");
            acc
        }),
        PropertyValue::DateTime(dt) => dt.to_rfc3339(),
        PropertyValue::Duration(seconds) => seconds.to_string(),
        // No native GraphML list type; joined with a separator that is
        // itself escaped if it appears inside a value, so a value
        // containing the literal separator cannot be misread as two
        // values — the exact bug Epic 9a's own Slice C names for CSV.
        PropertyValue::List(items) => items
            .iter()
            .map(|item| {
                property_text(item)
                    .replace('\\', "\\\\")
                    .replace('|', "\\|")
            })
            .collect::<Vec<_>>()
            .join("|"),
        PropertyValue::ElementRef(id) => id.as_str().to_string(),
    }
}

/// The reserved key id for a node's labels — not part of the dynamic
/// schema registry (labels are not a `PropertyMap` entry), declared
/// unconditionally in `finish` so a reader never needs to guess whether
/// it is present.
const LABELS_KEY: &str = "node-_labels";
/// The reserved key id for an edge's type, the same reasoning as
/// [`LABELS_KEY`].
const EDGE_TYPE_KEY: &str = "edge-_type";

/// The one implementation Slice A ships. Streams two scratch files
/// (nodes, edges) on disk as elements arrive, then assembles the real
/// `GraphML` document in [`finish`](LpgWriter::finish) — see the module
/// doc comment for why this does not conflict with `GraphML`'s own
/// keys-before-graph structure.
pub struct GraphMlWriter {
    output_path: PathBuf,
    scratch_dir: PathBuf,
    graph_id: String,
    node_writer: Option<BufWriter<std::fs::File>>,
    edge_writer: Option<BufWriter<std::fs::File>>,
    keys: BTreeMap<(KeyDomain, String), KeyEntry>,
    next_key_id: u64,
    node_count: u64,
    edge_count: u64,
}

impl GraphMlWriter {
    /// # Errors
    /// [`LpgIoError::Io`] if the scratch directory cannot be created.
    pub fn new(output_path: impl Into<PathBuf>) -> Result<Self, LpgIoError> {
        let scratch_dir =
            std::env::temp_dir().join(format!("graph-owl-graphml-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&scratch_dir)?;
        Ok(Self {
            output_path: output_path.into(),
            scratch_dir,
            graph_id: String::new(),
            node_writer: None,
            edge_writer: None,
            keys: BTreeMap::new(),
            next_key_id: 0,
            node_count: 0,
            edge_count: 0,
        })
    }

    /// The key's stable id, assigning one the first time this `(domain,
    /// name)` pair is seen. Every later call for the same pair returns
    /// the identical id — the property this whole module exists to
    /// guarantee, since a `<data key="...">` already written to a
    /// scratch file cannot be revised once the bytes have left this
    /// process.
    fn key_id(&mut self, domain: KeyDomain, name: &str, value: &PropertyValue) -> String {
        if let Some(entry) = self.keys.get(&(domain, name.to_string())) {
            return entry.id.clone();
        }
        let id = format!("d{}", self.next_key_id);
        self.next_key_id += 1;
        self.keys.insert(
            (domain, name.to_string()),
            KeyEntry {
                id: id.clone(),
                graphml_type: graphml_type(value),
            },
        );
        id
    }

    /// Every property's key id and rendered text, assigned *before* the
    /// XML element is opened — `key_id` needs `&mut self`, and once the
    /// element itself is being written through a borrowed `XmlWriter`,
    /// `self` is no longer free to borrow again. Resolving ids up front
    /// avoids that conflict entirely rather than working around it with a
    /// second buffer.
    fn rendered_properties(
        &mut self,
        domain: KeyDomain,
        properties: &PropertyMap,
    ) -> Vec<(String, String)> {
        properties
            .keys()
            .filter_map(|key| {
                let value = properties.get(key)?;
                Some((self.key_id(domain, key, value), property_text(value)))
            })
            .collect()
    }
}

impl LpgWriter for GraphMlWriter {
    fn begin(&mut self, meta: &ExportMeta) -> Result<(), LpgIoError> {
        self.graph_id.clone_from(&meta.graph_id);
        self.node_writer = Some(BufWriter::new(std::fs::File::create(
            self.scratch_dir.join("nodes.xml"),
        )?));
        self.edge_writer = Some(BufWriter::new(std::fs::File::create(
            self.scratch_dir.join("edges.xml"),
        )?));
        Ok(())
    }

    fn node(&mut self, n: &LpgNode) -> Result<(), LpgIoError> {
        if self.node_writer.is_none() {
            return Err(LpgIoError::NotStarted);
        }
        let data = self.rendered_properties(KeyDomain::Node, &n.properties);

        // One element, built into its own small buffer — never the whole
        // export — then written straight through to the scratch file.
        let mut element = Vec::new();
        let mut xml = XmlWriter::new(&mut element);
        xml.create_element("node")
            .with_attribute(("id", n.element_id.as_str()))
            .write_inner_content(|xml| {
                if !n.labels.is_empty() {
                    xml.create_element("data")
                        .with_attribute(("key", LABELS_KEY))
                        .write_text_content(BytesText::new(&n.labels.join("|")))?;
                }
                for (key_id, text) in &data {
                    xml.create_element("data")
                        .with_attribute(("key", key_id.as_str()))
                        .write_text_content(BytesText::new(text))?;
                }
                Ok(())
            })?;

        let writer = self.node_writer.as_mut().ok_or(LpgIoError::NotStarted)?;
        writer.write_all(&element)?;
        self.node_count += 1;
        Ok(())
    }

    fn edge(&mut self, e: &LpgEdge) -> Result<(), LpgIoError> {
        if self.edge_writer.is_none() {
            return Err(LpgIoError::NotStarted);
        }
        let data = self.rendered_properties(KeyDomain::Edge, &e.properties);

        let mut element = Vec::new();
        let mut xml = XmlWriter::new(&mut element);
        xml.create_element("edge")
            .with_attribute(("id", e.element_id.as_str()))
            .with_attribute(("source", e.start.as_str()))
            .with_attribute(("target", e.end.as_str()))
            .write_inner_content(|xml| {
                xml.create_element("data")
                    .with_attribute(("key", EDGE_TYPE_KEY))
                    .write_text_content(BytesText::new(&e.edge_type))?;
                for (key_id, text) in &data {
                    xml.create_element("data")
                        .with_attribute(("key", key_id.as_str()))
                        .write_text_content(BytesText::new(text))?;
                }
                Ok(())
            })?;

        let writer = self.edge_writer.as_mut().ok_or(LpgIoError::NotStarted)?;
        writer.write_all(&element)?;
        self.edge_count += 1;
        Ok(())
    }

    fn finish(mut self) -> Result<ExportSummary, LpgIoError> {
        if let Some(mut w) = self.node_writer.take() {
            w.flush()?;
        }
        if let Some(mut w) = self.edge_writer.take() {
            w.flush()?;
        }

        let output = std::fs::File::create(&self.output_path)?;
        let mut out = BufWriter::new(output);
        writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
        writeln!(
            out,
            "<graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\">"
        )?;
        writeln!(
            out,
            "  <key id=\"{LABELS_KEY}\" for=\"node\" attr.name=\"_labels\" attr.type=\"string\"/>"
        )?;
        writeln!(
            out,
            "  <key id=\"{EDGE_TYPE_KEY}\" for=\"edge\" attr.name=\"_type\" attr.type=\"string\"/>"
        )?;
        for ((domain, name), entry) in &self.keys {
            let for_attr = match domain {
                KeyDomain::Node => "node",
                KeyDomain::Edge => "edge",
            };
            writeln!(
                out,
                "  <key id=\"{}\" for=\"{for_attr}\" attr.name=\"{}\" attr.type=\"{}\"/>",
                entry.id,
                xml_escape_attr(name),
                entry.graphml_type
            )?;
        }
        writeln!(
            out,
            "  <graph id=\"{}\" edgedefault=\"directed\">",
            xml_escape_attr(&self.graph_id)
        )?;
        out.flush()?;

        // Nodes before edges — streamed straight from the scratch files,
        // never re-read into memory as a whole.
        let mut node_scratch = std::fs::File::open(self.scratch_dir.join("nodes.xml"))?;
        std::io::copy(&mut node_scratch, &mut out)?;
        let mut edge_scratch = std::fs::File::open(self.scratch_dir.join("edges.xml"))?;
        std::io::copy(&mut edge_scratch, &mut out)?;

        writeln!(out, "  </graph>")?;
        writeln!(out, "</graphml>")?;
        out.flush()?;

        std::fs::remove_dir_all(&self.scratch_dir).ok();

        Ok(ExportSummary {
            nodes: self.node_count,
            edges: self.edge_count,
        })
    }
}

/// The five XML-significant characters in attribute *values* this crate
/// writes with `write!`/`writeln!` directly (the manifest header and
/// `<key>` declarations, not through `quick_xml`'s own escaping element
/// API — property and label data go through `BytesText`/`Attribute`'s
/// `From` impls instead, which escape automatically, confirmed by this
/// crate's own byte-level escaping test).
fn xml_escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::Sid;
    use graph_owl_lpg::ElementId;

    fn node(id: &str, labels: &[&str], properties: PropertyMap) -> LpgNode {
        LpgNode {
            element_id: ElementId::encode(&Sid::dsc(id)),
            labels: labels.iter().map(ToString::to_string).collect(),
            properties,
        }
    }

    fn edge(id: &str, from: &str, to: &str, edge_type: &str, properties: PropertyMap) -> LpgEdge {
        LpgEdge {
            element_id: ElementId::encode(&Sid::dsc(id)),
            edge_type: edge_type.to_string(),
            start: ElementId::encode(&Sid::dsc(from)),
            end: ElementId::encode(&Sid::dsc(to)),
            properties,
        }
    }

    fn output_path() -> PathBuf {
        std::env::temp_dir().join(format!("graphml-test-{}.xml", uuid::Uuid::new_v4()))
    }

    #[test]
    fn nodes_and_edges_export_with_key_declarations_preceding_the_graph() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "catalog".to_string(),
            })
            .expect("begin");

        let mut a_props = PropertyMap::new();
        a_props
            .insert_user("name", PropertyValue::String("orders".into()))
            .expect("insert");
        writer
            .node(&node("a", &["Table"], a_props))
            .expect("node a");

        let mut b_props = PropertyMap::new();
        b_props
            .insert_user("name", PropertyValue::String("shipments".into()))
            .expect("insert");
        writer
            .node(&node("b", &["Table"], b_props))
            .expect("node b");

        writer
            .edge(&edge("r1", "a", "b", "feeds", PropertyMap::new()))
            .expect("edge");

        let summary = writer.finish().expect("finish");
        assert_eq!(summary, ExportSummary { nodes: 2, edges: 1 });

        let xml = std::fs::read_to_string(&path).expect("read output");
        let graph_pos = xml.find("<graph ").expect("graph element");
        let last_key_pos = xml.rfind("<key ").expect("key element");
        assert!(
            last_key_pos < graph_pos,
            "every <key> must precede <graph>: {xml}"
        );
        let first_node_pos = xml.find("<node ").expect("node element");
        assert!(first_node_pos > graph_pos, "nodes must be inside <graph>");
        let first_edge_pos = xml.find("<edge ").expect("edge element");
        assert!(
            first_node_pos < first_edge_pos,
            "nodes must precede edges: {xml}"
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_real_edge_carries_source_target_and_type() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        writer
            .node(&node("a", &["Table"], PropertyMap::new()))
            .expect("node a");
        writer
            .node(&node("b", &["Table"], PropertyMap::new()))
            .expect("node b");
        writer
            .edge(&edge("r1", "a", "b", "feeds", PropertyMap::new()))
            .expect("edge");
        let summary = writer.finish().expect("finish");
        assert_eq!(summary.edges, 1);

        let xml = std::fs::read_to_string(&path).expect("read");
        let source_id = ElementId::encode(&Sid::dsc("a"));
        let target_id = ElementId::encode(&Sid::dsc("b"));
        assert!(xml.contains(&format!("source=\"{}\"", source_id.as_str())));
        assert!(xml.contains(&format!("target=\"{}\"", target_id.as_str())));
        assert!(xml.contains("feeds"));

        std::fs::remove_file(&path).ok();
    }

    /// **A key id, once written into a `<data>` element, must still name
    /// the same key when `<key>` is declared later** — the bug a
    /// position-recomputed id would introduce the moment a later element
    /// discovers a new property that sorts earlier than an existing one.
    #[test]
    fn a_key_id_stays_stable_even_when_a_later_element_introduces_an_earlier_sorting_key() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");

        let mut first = PropertyMap::new();
        first
            .insert_user("zzz_last", PropertyValue::String("z".into()))
            .expect("insert");
        writer.node(&node("a", &[], first)).expect("node a");

        // "aaa_first" sorts before "zzz_last" — if key ids were derived
        // from sorted position at write time, "zzz_last"'s id would shift
        // once this element is processed, disagreeing with what was
        // already written for node "a".
        let mut second = PropertyMap::new();
        second
            .insert_user("aaa_first", PropertyValue::String("a".into()))
            .expect("insert");
        writer.node(&node("b", &[], second)).expect("node b");

        writer.finish().expect("finish");

        let xml = std::fs::read_to_string(&path).expect("read");
        // Each <key id="dN" ... attr.name="X"/> must be the *only*
        // declaration referencing whatever id node "a"'s <data> element
        // used for zzz_last — found by cross-referencing rather than
        // asserting a specific id, so the test does not encode the
        // assignment order as part of its own expectation.
        let key_line = xml
            .lines()
            .find(|line| line.contains("attr.name=\"zzz_last\""))
            .expect("a <key> for zzz_last must exist");
        let id_start = key_line.find("id=\"").expect("id attribute") + 4;
        let id = &key_line[id_start..key_line[id_start..].find('"').unwrap() + id_start];
        assert!(
            xml.contains(&format!("<data key=\"{id}\">z</data>")),
            "node a's own <data> must reference the same id its <key> declares: {xml}"
        );

        std::fs::remove_file(&path).ok();
    }

    /// **The classic silent-corruption case.** `<`, `&`, quotes, and a
    /// value containing markup — completely ordinary in a metadata
    /// catalog description — must not produce a file that fails to parse
    /// at the far end.
    #[test]
    fn xml_special_characters_are_escaped_in_property_values() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        let mut props = PropertyMap::new();
        props
            .insert_user(
                "description",
                PropertyValue::String("Tom & Jerry <html> \"quoted\" 'apos'".into()),
            )
            .expect("insert");
        writer.node(&node("a", &["Table"], props)).expect("node");
        writer.finish().expect("finish");

        let bytes = std::fs::read(&path).expect("read output bytes");
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("Tom & Jerry"),
            "raw & must not survive: {text}"
        );
        assert!(!text.contains("<html>"), "raw < must not survive: {text}");
        assert!(text.contains("&amp;"), "& must be escaped");
        assert!(text.contains("&lt;html&gt;"), "< and > must be escaped");

        // And the file must actually be well-formed XML — a real parser
        // must accept it without erroring.
        let mut reader = quick_xml::Reader::from_str(&text);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Eof) => break,
                Err(e) => panic!("output is not well-formed XML: {e}\n{text}"),
                _ => {}
            }
            buf.clear();
        }

        std::fs::remove_file(&path).ok();
    }

    /// **Streaming, not build-then-write** (decision 5). A structural
    /// proof rather than an OS-level RSS measurement: the writer's own
    /// in-memory schema state stays bounded to the number of *distinct*
    /// property keys regardless of how many elements carry them, which is
    /// only possible if elements themselves are written through to disk
    /// as they arrive rather than accumulated.
    #[test]
    fn writing_many_elements_with_one_shared_key_keeps_the_schema_bounded() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");

        for i in 0..5_000 {
            let mut props = PropertyMap::new();
            props
                .insert_user("name", PropertyValue::String(format!("n{i}")))
                .expect("insert");
            writer
                .node(&node(&format!("n{i}"), &["Table"], props))
                .expect("node");
        }

        assert_eq!(
            writer.keys.len(),
            1,
            "5000 elements sharing one property key must yield exactly one \
             schema entry, not one per element"
        );

        let summary = writer.finish().expect("finish");
        assert_eq!(summary.nodes, 5_000);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_repeated_property_value_uses_the_documented_list_separator() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        let mut props = PropertyMap::new();
        props
            .insert_user("tag", PropertyValue::String("pii".into()))
            .expect("insert");
        props
            .insert_user("tag", PropertyValue::String("financial".into()))
            .expect("insert");
        writer.node(&node("a", &["Table"], props)).expect("node");
        writer.finish().expect("finish");

        let xml = std::fs::read_to_string(&path).expect("read");
        assert!(xml.contains("pii|financial"), "{xml}");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn calling_node_before_begin_is_a_named_error_not_a_panic() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        let outcome = writer.node(&node("a", &[], PropertyMap::new()));
        assert!(matches!(outcome, Err(LpgIoError::NotStarted)));
    }
}
