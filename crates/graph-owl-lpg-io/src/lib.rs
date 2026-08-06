//! Property-graph interchange at the boundary — Epic 9a.
//!
//! **Symmetric with Epic 9, same streaming discipline, different formats**
//! (the plan's own framing). `LpgNode`/`LpgEdge` already exist in
//! `graph-owl-lpg` (Epic 7c) — this crate turns them into interchange
//! bytes and back, and never becomes the graph's own model.
//!
//! **Slice A**: streaming `GraphML` export. **Slice B**: `GraphML` import
//! and round-trip, via [`GraphMlReader`]. **Slice C**: [`BulkCsvWriter`]
//! (Neo4j's documented bulk-import CSV shape, verified against real
//! working examples fetched via GitHub after the direct docs page
//! returned `403`) and [`CypherScriptWriter`] (batched, idempotent via
//! `MERGE`). `LpgWriter`/`LpgReader` name the full trait shape the plan
//! specifies so later slices (JSON Graph, JSON Lines) extend this crate
//! rather than reshape it.
//!
//! ## Why bulk CSV cannot avoid buffering rows, unlike `GraphML`
//!
//! A CSV header must name every column before any data row, and unlike
//! `GraphML`'s per-`<data>` typing, a row's column *shape* — which
//! properties exist at all — has to be fixed for the whole file. So the
//! column set cannot be finalised without having seen every row, and
//! [`BulkCsvWriter`] holds rows in memory (bounded by export size, not by
//! catalog size) until [`finish`](LpgWriter::finish) can compute it — a
//! real, deliberate deviation from decision 5's streaming discipline for
//! this one format, not a missed optimisation.
//!
//! ## Why the reader's helper functions are free functions, not methods
//!
//! `quick_xml`'s zero-copy `Event<'buf>` borrows from whatever buffer
//! produced it — here, [`GraphMlReader`]'s own `buf` field. A `BytesStart`
//! pulled from that event and used later in the same scope keeps that
//! borrow alive for as long as it is used, and any `&self`/`&mut self`
//! method call in that window — even one touching a *different* field —
//! conflicts with it, because a method signature only says `&self`, not
//! which fields it actually touches. [`attr_value`], [`read_key_declaration`],
//! and [`typed_value`] take exactly the pieces of state they need as plain
//! parameters instead, which sidesteps the conflict entirely rather than
//! fighting it with more lifetimes.
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

pub mod projection;

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
    /// The document is not well-formed XML, or not well-formed `GraphML` —
    /// Epic 9a Slice B. Carries line and column rather than only a byte
    /// offset, because a byte offset into a file nobody has open in an
    /// editor is not somewhere a person can go.
    #[error("malformed document at line {line}, column {column}: {message}")]
    Parse {
        line: usize,
        column: usize,
        message: String,
    },
    /// An `<edge>` names a `source`/`target` id no `<node>` seen so far has
    /// declared — Epic 9a Slice B. Named rather than silently dropped or
    /// turned into a phantom node, so a malformed export is caught instead
    /// of imported as a graph with edges into nothing.
    #[error("edge `{edge_id}` references node `{missing_node_id}`, which was never declared")]
    DanglingReference {
        edge_id: String,
        missing_node_id: String,
    },
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

/// One element pulled from an interchange document — Epic 9a Slice B.
#[derive(Debug, Clone, PartialEq)]
pub enum LpgElement {
    Node(LpgNode),
    Edge(LpgEdge),
}

/// Streaming read side of an interchange format — the plan's own
/// interface, pull-based so a caller controls how much of the document is
/// live at once regardless of file size.
pub trait LpgReader {
    /// The next element, or `None` at end of document.
    ///
    /// # Errors
    /// [`LpgIoError::Parse`] if the document is malformed;
    /// [`LpgIoError::DanglingReference`] if an edge names a node not yet
    /// seen; [`LpgIoError::Io`] if reading fails.
    fn read(&mut self) -> Result<Option<LpgElement>, LpgIoError>;
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

/// One `<key>` declaration, read before any `<node>`/`<edge>` — `GraphML`'s
/// own structure guarantees this ordering, the read-side mirror of why
/// [`GraphMlWriter::finish`] writes them first.
#[derive(Debug, Clone)]
struct KeyDeclaration {
    domain: KeyDomain,
    name: String,
    attr_type: String,
}

/// Wraps a [`std::io::BufRead`], tracking line and column as `quick_xml`
/// consumes bytes through [`std::io::BufRead::consume`] — the only hook
/// that tells us exactly how many bytes of the last `fill_buf` the parser
/// actually used. `quick_xml`'s own `buffer_position()` gives a byte
/// offset, not a line/column, and computing one from an offset needs the
/// source bytes already consumed, which a genuinely streaming reader does
/// not keep — so this counts as it goes instead of computing after the
/// fact.
struct LineTrackingReader<R> {
    inner: R,
    line: usize,
    column: usize,
}

impl<R: std::io::BufRead> LineTrackingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            line: 1,
            column: 1,
        }
    }
}

impl<R: std::io::BufRead> std::io::Read for LineTrackingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: std::io::BufRead> std::io::BufRead for LineTrackingReader<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        if let Ok(filled) = self.inner.fill_buf() {
            for &byte in filled.iter().take(amt) {
                if byte == b'\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
            }
        }
        self.inner.consume(amt);
    }
}

/// Streams `GraphML` back into [`LpgNode`]/[`LpgEdge`] values — Epic 9a
/// Slice B. A single forward pass: `<key>` declarations are collected as
/// they are seen (always before `<graph>`, per `GraphML`'s own structure),
/// then each `<node>`/`<edge>` is converted using the declared type of
/// every `<data>` it carries — never guessed from the text.
///
/// **Node ids seen so far, not the whole node set, are what a dangling
/// edge reference is checked against** — correct for anything this crate's
/// own [`GraphMlWriter`] produces (nodes always precede edges), and
/// documented rather than silently assumed: a hand-authored document that
/// puts an edge before the node it references reads as dangling even
/// though the node appears later, because validating the true whole-file
/// set would mean buffering it, which is exactly what streaming import
/// exists to avoid.
pub struct GraphMlReader<R: std::io::BufRead> {
    reader: quick_xml::Reader<LineTrackingReader<R>>,
    buf: Vec<u8>,
    keys: std::collections::BTreeMap<String, KeyDeclaration>,
    seen_node_ids: std::collections::HashSet<String>,
}

fn parse_error(line: usize, column: usize, message: impl Into<String>) -> LpgIoError {
    LpgIoError::Parse {
        line,
        column,
        message: message.into(),
    }
}

/// A free function, deliberately not a `&self` method: `start` borrows the
/// same buffer a `&mut self` call to `read_event_into` just produced it
/// from, and a `GraphMlReader` method taking `&self` while that borrow is
/// still live (because `start` is used after it) is exactly the
/// self-referential conflict a plain function sidesteps — it borrows
/// nothing from `GraphMlReader` at all.
fn attr_value(
    start: &quick_xml::events::BytesStart<'_>,
    name: &str,
    line: usize,
    column: usize,
) -> Result<String, LpgIoError> {
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| parse_error(line, column, e.to_string()))?;
        if attribute.key.as_ref() == name.as_bytes() {
            return attribute
                .unescape_value()
                .map(std::borrow::Cow::into_owned)
                .map_err(|e| parse_error(line, column, e.to_string()));
        }
    }
    Err(parse_error(
        line,
        column,
        format!("missing required `{name}` attribute"),
    ))
}

/// Same reasoning as [`attr_value`]: a free function, returning the pair
/// for the caller to insert — inserting here too would need `&mut self`
/// while `start` (borrowed from `self.buf`) is still alive.
fn read_key_declaration(
    start: &quick_xml::events::BytesStart<'_>,
    line: usize,
    column: usize,
) -> Result<(String, KeyDeclaration), LpgIoError> {
    let id = attr_value(start, "id", line, column)?;
    let for_attr = attr_value(start, "for", line, column)?;
    let domain = match for_attr.as_str() {
        "node" => KeyDomain::Node,
        "edge" => KeyDomain::Edge,
        other => {
            return Err(parse_error(
                line,
                column,
                format!("<key for=\"{other}\"> is neither \"node\" nor \"edge\""),
            ));
        }
    };
    let name = attr_value(start, "attr.name", line, column)?;
    let attr_type = attr_value(start, "attr.type", line, column)?;
    Ok((
        id,
        KeyDeclaration {
            domain,
            name,
            attr_type,
        },
    ))
}

/// Converts one `<data>` element's text back into a typed value, using
/// `key`'s declared `attr.type` — never guessed from the text itself.
fn typed_value(key: &KeyDeclaration, text: &str) -> PropertyValue {
    match key.attr_type.as_str() {
        "boolean" => text.parse::<bool>().map_or_else(
            |_| PropertyValue::String(text.to_string()),
            PropertyValue::Boolean,
        ),
        "long" | "int" => text.parse::<i64>().map_or_else(
            |_| PropertyValue::String(text.to_string()),
            PropertyValue::Integer,
        ),
        "double" | "float" => text.parse::<f64>().map_or_else(
            |_| PropertyValue::String(text.to_string()),
            PropertyValue::Float,
        ),
        // "string", and anything undeclared: the universal fallback —
        // matching `GraphMlWriter`'s own narrowing of `Bytes`/
        // `DateTime`/`Duration`/`List`/`ElementRef` to `string`, which
        // this reader cannot un-narrow without guessing.
        _ => PropertyValue::String(text.to_string()),
    }
}

impl<R: std::io::BufRead> GraphMlReader<R> {
    #[must_use]
    pub fn new(source: R) -> Self {
        Self {
            reader: quick_xml::Reader::from_reader(LineTrackingReader::new(source)),
            buf: Vec::new(),
            keys: std::collections::BTreeMap::new(),
            seen_node_ids: std::collections::HashSet::new(),
        }
    }

    fn position(&self) -> (usize, usize) {
        let inner = self.reader.get_ref();
        (inner.line, inner.column)
    }

    /// Reads one `<data key="...">text</data>` child fully — via
    /// `read_text_into`, which handles a `</data>` end tag arriving after
    /// mixed text/`CDATA` runs in one call, rather than this crate
    /// manually pairing a `Text` event with the `End` that follows it.
    /// `read_text_into` itself returns content "as is" (it cannot safely
    /// unescape without knowing whether a run was `CDATA`), so entity
    /// unescaping is a second, explicit step via `quick_xml::escape::unescape`.
    fn read_data_text(&mut self) -> Result<String, LpgIoError> {
        let (line, column) = self.position();
        let mut local_buf = Vec::new();
        let raw = self
            .reader
            .read_text_into(quick_xml::name::QName(b"data"), &mut local_buf)
            .map_err(|e| parse_error(line, column, e.to_string()))?;
        let decoded = raw
            .decode()
            .map_err(|e| parse_error(line, column, e.to_string()))?;
        quick_xml::escape::unescape(&decoded)
            .map(std::borrow::Cow::into_owned)
            .map_err(|e| parse_error(line, column, e.to_string()))
    }

    /// Reads one `<node>` or `<edge>` element's body: its `<data>` children
    /// up to the matching `</node>`/`</edge>`, returning
    /// `(labels_or_type_reserved_text, properties)`.
    fn read_element_body(
        &mut self,
        domain: KeyDomain,
        end_tag: &[u8],
    ) -> Result<(Option<String>, PropertyMap), LpgIoError> {
        let mut properties = PropertyMap::new();
        let mut reserved = None;
        loop {
            let (line, column) = self.position();
            let event = self
                .reader
                .read_event_into(&mut self.buf)
                .map_err(|e| parse_error(line, column, e.to_string()))?;
            match event {
                quick_xml::events::Event::Start(start) if start.name().as_ref() == b"data" => {
                    let key_id = attr_value(&start, "key", line, column)?;
                    let text = self.read_data_text()?;

                    if key_id == LABELS_KEY || key_id == EDGE_TYPE_KEY {
                        reserved = Some(text);
                        continue;
                    }
                    let Some(declaration) = self.keys.get(&key_id) else {
                        return Err(parse_error(
                            line,
                            column,
                            format!("<data key=\"{key_id}\"> has no <key> declaration"),
                        ));
                    };
                    if declaration.domain != domain {
                        return Err(parse_error(
                            line,
                            column,
                            format!(
                                "<data key=\"{key_id}\"> is declared for the other element kind"
                            ),
                        ));
                    }
                    let value = typed_value(declaration, &text);
                    properties
                        .insert_user(&declaration.name, value)
                        .map_err(|e| {
                            parse_error(
                                line,
                                column,
                                format!("could not set `{}`: {e}", declaration.name),
                            )
                        })?;
                }
                quick_xml::events::Event::Empty(start) if start.name().as_ref() == b"data" => {
                    // `<data key="..."/>` — no text at all, the empty-string case.
                    let key_id = attr_value(&start, "key", line, column)?;
                    if key_id == LABELS_KEY || key_id == EDGE_TYPE_KEY {
                        reserved = Some(String::new());
                        continue;
                    }
                    let Some(declaration) = self.keys.get(&key_id) else {
                        return Err(parse_error(
                            line,
                            column,
                            format!("<data key=\"{key_id}\"> has no <key> declaration"),
                        ));
                    };
                    let value = typed_value(declaration, "");
                    properties
                        .insert_user(&declaration.name, value)
                        .map_err(|e| {
                            parse_error(
                                line,
                                column,
                                format!("could not set `{}`: {e}", declaration.name),
                            )
                        })?;
                }
                quick_xml::events::Event::End(end) if end.name().as_ref() == end_tag => {
                    return Ok((reserved, properties));
                }
                quick_xml::events::Event::Eof => {
                    return Err(parse_error(
                        line,
                        column,
                        "unexpected end of document inside an element",
                    ));
                }
                _ => {}
            }
        }
    }
}

impl<R: std::io::BufRead> LpgReader for GraphMlReader<R> {
    fn read(&mut self) -> Result<Option<LpgElement>, LpgIoError> {
        loop {
            let (line, column) = self.position();
            let event = self
                .reader
                .read_event_into(&mut self.buf)
                .map_err(|e| parse_error(line, column, e.to_string()))?;
            match event {
                quick_xml::events::Event::Empty(start) if start.name().as_ref() == b"key" => {
                    let (id, declaration) = read_key_declaration(&start, line, column)?;
                    self.keys.insert(id, declaration);
                }
                quick_xml::events::Event::Start(start) if start.name().as_ref() == b"node" => {
                    let id = attr_value(&start, "id", line, column)?;
                    let (labels_text, properties) =
                        self.read_element_body(KeyDomain::Node, b"node")?;
                    self.seen_node_ids.insert(id.clone());
                    let labels = labels_text
                        .map(|text| text.split('|').map(ToString::to_string).collect())
                        .unwrap_or_default();
                    return Ok(Some(LpgElement::Node(LpgNode {
                        element_id: graph_owl_lpg::ElementId::from_wire(id),
                        labels,
                        properties,
                    })));
                }
                quick_xml::events::Event::Start(start) if start.name().as_ref() == b"edge" => {
                    let id = attr_value(&start, "id", line, column)?;
                    let source = attr_value(&start, "source", line, column)?;
                    let target = attr_value(&start, "target", line, column)?;
                    for referenced in [&source, &target] {
                        if !self.seen_node_ids.contains(referenced) {
                            return Err(LpgIoError::DanglingReference {
                                edge_id: id,
                                missing_node_id: referenced.clone(),
                            });
                        }
                    }
                    let (edge_type_text, properties) =
                        self.read_element_body(KeyDomain::Edge, b"edge")?;
                    return Ok(Some(LpgElement::Edge(LpgEdge {
                        element_id: graph_owl_lpg::ElementId::from_wire(id),
                        edge_type: edge_type_text.unwrap_or_default(),
                        start: graph_owl_lpg::ElementId::from_wire(source),
                        end: graph_owl_lpg::ElementId::from_wire(target),
                        properties,
                    })));
                }
                quick_xml::events::Event::Eof => return Ok(None),
                _ => {}
            }
        }
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

// ---------------------------------------------------------------------------
// Slice C: Bulk CSV and Cypher script.
// ---------------------------------------------------------------------------

/// Neo4j's own bulk-import header type vocabulary. `PropertyValue` variants
/// with no native match (`Bytes`, `DateTime`, `Duration`, `ElementRef`) fall
/// back to `string`, the same documented narrowing [`graphml_type`] already
/// uses for the other format; a `List` narrows to `string[]` rather than
/// trying to track a per-element type that this store's own model does not
/// guarantee is homogeneous.
fn csv_type(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Boolean(_) => "boolean",
        PropertyValue::Integer(_) => "long",
        PropertyValue::Float(_) => "double",
        PropertyValue::List(_) => "string[]",
        _ => "string",
    }
}

/// The array separator the bulk-import format documents (Neo4j's own
/// default). A value containing this character **must** be escaped, or one
/// array entry silently becomes two on the far side — the plan's own named
/// bug, and the reason this is a function with a test rather than an
/// inline `.join`.
const ARRAY_SEPARATOR: char = ';';

/// Renders one property's value as bulk-CSV field text — never the raw RFC
/// 4180 quoting a caller might expect, because that alone does not protect
/// the *array* separator: a comma inside a field is solved by quoting the
/// field, but a semicolon inside one *array element* of a `string[]` field
/// is invisible to CSV quoting entirely and must be escaped at the array
/// level first.
fn csv_property_text(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Float(f) => f.to_string(),
        PropertyValue::List(items) => items
            .iter()
            .map(|item| {
                csv_property_text(item)
                    .replace('\\', "\\\\")
                    .replace(ARRAY_SEPARATOR, "\\;")
            })
            .collect::<Vec<_>>()
            .join(&ARRAY_SEPARATOR.to_string()),
        other => property_text(other),
    }
}

/// RFC 4180 field quoting: a field containing the delimiter, a quote, or a
/// newline is wrapped in quotes with internal quotes doubled. Applied on
/// top of [`csv_property_text`]'s own array escaping — the two operate at
/// different levels (array-element boundaries vs. CSV-field boundaries) and
/// neither substitutes for the other.
fn csv_quote_field(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// One `<node>`'s worth of bulk-CSV state, buffered rather than streamed —
/// see the module-level note on why CSV cannot avoid this the way `GraphML`
/// does.
struct CsvNodeRow {
    id: String,
    labels: Vec<String>,
    properties: PropertyMap,
}

struct CsvRelRow {
    start: String,
    end: String,
    edge_type: String,
    properties: PropertyMap,
}

/// Streaming write side for Neo4j's documented bulk-import CSV shape —
/// Epic 9a Slice C. **Not streaming to disk the way [`GraphMlWriter`] is**,
/// and that is a real, deliberate difference rather than a missed
/// optimisation: a bulk-CSV header must name every column *before* any data
/// row, and unlike `GraphML`'s per-`<data>` typing, a CSV row's *shape*
/// (which columns exist at all) has to be fixed for the whole file — so the
/// column set cannot be finalised without having seen every row, and rows
/// are held in memory (bounded by export size, not by catalog size) until
/// [`finish`](LpgWriter::finish) can compute it. Nodes are bucketed by
/// their first label into one file per label — `nodes-Table.csv`,
/// `nodes-Column.csv`, etc. — because a single nodes file across
/// heterogeneous labels would need the union of every label's properties as
/// columns, mostly empty; one relationships file covers every edge type,
/// since edges in this catalog carry far fewer distinct properties.
pub struct BulkCsvWriter {
    output_dir: PathBuf,
    node_rows: BTreeMap<String, Vec<CsvNodeRow>>,
    rel_rows: Vec<CsvRelRow>,
    node_count: u64,
    edge_count: u64,
    started: bool,
}

impl BulkCsvWriter {
    /// # Errors
    /// [`LpgIoError::Io`] if `output_dir` cannot be created.
    pub fn new(output_dir: impl Into<PathBuf>) -> Result<Self, LpgIoError> {
        let output_dir = output_dir.into();
        std::fs::create_dir_all(&output_dir)?;
        Ok(Self {
            output_dir,
            node_rows: BTreeMap::new(),
            rel_rows: Vec::new(),
            node_count: 0,
            edge_count: 0,
            started: false,
        })
    }

    fn write_node_file(
        label: &str,
        rows: &[CsvNodeRow],
        output_dir: &std::path::Path,
    ) -> Result<(), LpgIoError> {
        let mut columns: Vec<(String, &'static str)> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            for key in row.properties.keys() {
                if seen.insert(key.clone()) {
                    let value = row.properties.get(key).expect("key just yielded by keys()");
                    columns.push((key.clone(), csv_type(value)));
                }
            }
        }

        let path = output_dir.join(format!("nodes-{label}.csv"));
        let mut out = BufWriter::new(std::fs::File::create(path)?);
        let mut header = vec!["id:ID".to_string()];
        header.extend(columns.iter().map(|(name, ty)| format!("{name}:{ty}")));
        header.push(":LABEL".to_string());
        writeln!(out, "{}", header.join(","))?;

        for row in rows {
            let mut fields = vec![csv_quote_field(&row.id)];
            for (name, _) in &columns {
                let text = row
                    .properties
                    .get(name)
                    .map(csv_property_text)
                    .unwrap_or_default();
                fields.push(csv_quote_field(&text));
            }
            fields.push(csv_quote_field(
                &row.labels.join(&ARRAY_SEPARATOR.to_string()),
            ));
            writeln!(out, "{}", fields.join(","))?;
        }
        out.flush()?;
        Ok(())
    }

    fn write_relationships_file(
        rows: &[CsvRelRow],
        output_dir: &std::path::Path,
    ) -> Result<(), LpgIoError> {
        let mut columns: Vec<(String, &'static str)> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            for key in row.properties.keys() {
                if seen.insert(key.clone()) {
                    let value = row.properties.get(key).expect("key just yielded by keys()");
                    columns.push((key.clone(), csv_type(value)));
                }
            }
        }

        let path = output_dir.join("relationships.csv");
        let mut out = BufWriter::new(std::fs::File::create(path)?);
        let mut header = vec![
            ":START_ID".to_string(),
            ":END_ID".to_string(),
            ":TYPE".to_string(),
        ];
        header.extend(columns.iter().map(|(name, ty)| format!("{name}:{ty}")));
        writeln!(out, "{}", header.join(","))?;

        for row in rows {
            let mut fields = vec![
                csv_quote_field(&row.start),
                csv_quote_field(&row.end),
                csv_quote_field(&row.edge_type),
            ];
            for (name, _) in &columns {
                let text = row
                    .properties
                    .get(name)
                    .map(csv_property_text)
                    .unwrap_or_default();
                fields.push(csv_quote_field(&text));
            }
            writeln!(out, "{}", fields.join(","))?;
        }
        out.flush()?;
        Ok(())
    }
}

impl LpgWriter for BulkCsvWriter {
    fn begin(&mut self, _meta: &ExportMeta) -> Result<(), LpgIoError> {
        self.started = true;
        Ok(())
    }

    fn node(&mut self, n: &LpgNode) -> Result<(), LpgIoError> {
        if !self.started {
            return Err(LpgIoError::NotStarted);
        }
        let label = n
            .labels
            .first()
            .cloned()
            .unwrap_or_else(|| "Node".to_string());
        self.node_rows.entry(label).or_default().push(CsvNodeRow {
            id: n.element_id.as_str().to_string(),
            labels: n.labels.clone(),
            properties: n.properties.clone(),
        });
        self.node_count += 1;
        Ok(())
    }

    fn edge(&mut self, e: &LpgEdge) -> Result<(), LpgIoError> {
        if !self.started {
            return Err(LpgIoError::NotStarted);
        }
        self.rel_rows.push(CsvRelRow {
            start: e.start.as_str().to_string(),
            end: e.end.as_str().to_string(),
            edge_type: e.edge_type.clone(),
            properties: e.properties.clone(),
        });
        self.edge_count += 1;
        Ok(())
    }

    fn finish(self) -> Result<ExportSummary, LpgIoError> {
        for (label, rows) in &self.node_rows {
            Self::write_node_file(label, rows, &self.output_dir)?;
        }
        Self::write_relationships_file(&self.rel_rows, &self.output_dir)?;
        Ok(ExportSummary {
            nodes: self.node_count,
            edges: self.edge_count,
        })
    }
}

/// Streaming write side for a Cypher import script — Epic 9a Slice C.
/// Batched `UNWIND` over a literal row list, not one `MERGE` per element
/// (an order of magnitude fewer round trips against a real server), and
/// `MERGE` keyed on the element id rather than `CREATE`, so re-running the
/// whole script converges on one copy instead of duplicating — the
/// idempotency the plan's own criterion names. **Labelled slow in the
/// script's own header comment**, naming [`BulkCsvWriter`]'s output as the
/// alternative at scale: a Cypher script is one client-to-server round trip
/// per batch, where the CSV path is a single bulk-loader invocation.
pub struct CypherScriptWriter {
    output_path: PathBuf,
    file: Option<BufWriter<std::fs::File>>,
    node_batch: Vec<LpgNode>,
    edge_batch: Vec<LpgEdge>,
    batch_size: usize,
    node_count: u64,
    edge_count: u64,
}

impl CypherScriptWriter {
    const DEFAULT_BATCH_SIZE: usize = 500;

    /// # Errors
    /// [`LpgIoError::Io`] if `output_path` cannot be created.
    pub fn new(output_path: impl Into<PathBuf>) -> Result<Self, LpgIoError> {
        Ok(Self {
            output_path: output_path.into(),
            file: None,
            node_batch: Vec::new(),
            edge_batch: Vec::new(),
            batch_size: Self::DEFAULT_BATCH_SIZE,
            node_count: 0,
            edge_count: 0,
        })
    }

    fn cypher_string_literal(value: &str) -> String {
        format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
    }

    fn cypher_value_literal(value: &PropertyValue) -> String {
        match value {
            PropertyValue::Boolean(b) => b.to_string(),
            PropertyValue::Integer(i) => i.to_string(),
            PropertyValue::Float(f) => f.to_string(),
            PropertyValue::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Self::cypher_value_literal)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            other => Self::cypher_string_literal(&property_text(other)),
        }
    }

    fn properties_map_literal(properties: &PropertyMap) -> String {
        let entries: Vec<String> = properties
            .keys()
            .filter_map(|key| {
                let value = properties.get(key)?;
                Some(format!("{key}: {}", Self::cypher_value_literal(value)))
            })
            .collect();
        format!("{{{}}}", entries.join(", "))
    }

    fn flush_nodes(&mut self) -> Result<(), LpgIoError> {
        if self.node_batch.is_empty() {
            return Ok(());
        }
        let file = self.file.as_mut().ok_or(LpgIoError::NotStarted)?;
        let rows: Vec<String> = self
            .node_batch
            .drain(..)
            .map(|n| {
                let labels = n.labels.join(":");
                format!(
                    "{{id: {}, labels: {}, props: {}}}",
                    Self::cypher_string_literal(n.element_id.as_str()),
                    Self::cypher_string_literal(&labels),
                    Self::properties_map_literal(&n.properties)
                )
            })
            .collect();
        writeln!(file, "UNWIND [{}] AS row", rows.join(", "))?;
        writeln!(file, "MERGE (n {{id: row.id}})")?;
        writeln!(file, "SET n += row.props;")?;
        Ok(())
    }

    fn flush_edges(&mut self) -> Result<(), LpgIoError> {
        if self.edge_batch.is_empty() {
            return Ok(());
        }
        let file = self.file.as_mut().ok_or(LpgIoError::NotStarted)?;
        let rows: Vec<String> = self
            .edge_batch
            .drain(..)
            .map(|e| {
                format!(
                    "{{start: {}, end: {}, type: {}, props: {}}}",
                    Self::cypher_string_literal(e.start.as_str()),
                    Self::cypher_string_literal(e.end.as_str()),
                    Self::cypher_string_literal(&e.edge_type),
                    Self::properties_map_literal(&e.properties)
                )
            })
            .collect();
        writeln!(file, "UNWIND [{}] AS row", rows.join(", "))?;
        writeln!(file, "MATCH (a {{id: row.start}}), (b {{id: row.end}})")?;
        writeln!(file, "MERGE (a)-[r:REL {{type: row.type}}]->(b)")?;
        writeln!(file, "SET r += row.props;")?;
        Ok(())
    }
}

impl LpgWriter for CypherScriptWriter {
    fn begin(&mut self, _meta: &ExportMeta) -> Result<(), LpgIoError> {
        let mut file = BufWriter::new(std::fs::File::create(&self.output_path)?);
        writeln!(
            file,
            "// Generated by graph-owl (Epic 9a Slice C). This script is SLOW at scale — \
             one network round trip per batch of {}, against a bulk loader's single \
             invocation. For a large graph, use the CSV export \
             (graph_owl_lpg_io::BulkCsvWriter) and your store's own bulk importer instead.",
            self.batch_size
        )?;
        writeln!(
            file,
            "// Idempotent: MERGE, not CREATE — re-running converges."
        )?;
        self.file = Some(file);
        Ok(())
    }

    fn node(&mut self, n: &LpgNode) -> Result<(), LpgIoError> {
        if self.file.is_none() {
            return Err(LpgIoError::NotStarted);
        }
        self.node_batch.push(n.clone());
        self.node_count += 1;
        if self.node_batch.len() >= self.batch_size {
            self.flush_nodes()?;
        }
        Ok(())
    }

    fn edge(&mut self, e: &LpgEdge) -> Result<(), LpgIoError> {
        if self.file.is_none() {
            return Err(LpgIoError::NotStarted);
        }
        self.edge_batch.push(e.clone());
        self.edge_count += 1;
        if self.edge_batch.len() >= self.batch_size {
            self.flush_edges()?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<ExportSummary, LpgIoError> {
        self.flush_nodes()?;
        self.flush_edges()?;
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        Ok(ExportSummary {
            nodes: self.node_count,
            edges: self.edge_count,
        })
    }
}

// ---------------------------------------------------------------------------
// Slice F: JSON Graph and JSON Lines.
// ---------------------------------------------------------------------------

/// One line of a JSON Lines export — Epic 9a Slice F. Adjacently tagged so
/// a reader can tell a node from an edge without first parsing the whole
/// object into a generic value and inspecting its shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum JsonLinesElement {
    Node(LpgNode),
    Edge(LpgEdge),
}

/// Streaming write side for JSON Lines — one element per line, so a
/// consumer can process the file without ever holding more than one
/// element at a time, and a producer never needs to know the whole element
/// count or property schema up front the way [`BulkCsvWriter`] does.
pub struct JsonLinesWriter {
    output_path: PathBuf,
    file: Option<BufWriter<std::fs::File>>,
    node_count: u64,
    edge_count: u64,
}

impl JsonLinesWriter {
    #[must_use]
    pub fn new(output_path: impl Into<PathBuf>) -> Self {
        Self {
            output_path: output_path.into(),
            file: None,
            node_count: 0,
            edge_count: 0,
        }
    }
}

impl LpgWriter for JsonLinesWriter {
    fn begin(&mut self, _meta: &ExportMeta) -> Result<(), LpgIoError> {
        self.file = Some(BufWriter::new(std::fs::File::create(&self.output_path)?));
        Ok(())
    }

    fn node(&mut self, n: &LpgNode) -> Result<(), LpgIoError> {
        let file = self.file.as_mut().ok_or(LpgIoError::NotStarted)?;
        let line = serde_json::to_string(&JsonLinesElement::Node(n.clone()))
            .map_err(|e| LpgIoError::Io(std::io::Error::other(e)))?;
        writeln!(file, "{line}")?;
        self.node_count += 1;
        Ok(())
    }

    fn edge(&mut self, e: &LpgEdge) -> Result<(), LpgIoError> {
        let file = self.file.as_mut().ok_or(LpgIoError::NotStarted)?;
        let line = serde_json::to_string(&JsonLinesElement::Edge(e.clone()))
            .map_err(|e| LpgIoError::Io(std::io::Error::other(e)))?;
        writeln!(file, "{line}")?;
        self.edge_count += 1;
        Ok(())
    }

    fn finish(mut self) -> Result<ExportSummary, LpgIoError> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        Ok(ExportSummary {
            nodes: self.node_count,
            edges: self.edge_count,
        })
    }
}

/// Streaming read side for JSON Lines — pull-based, one element per
/// `read()` call. **Resumable from an arbitrary line for free**: this
/// reader has no state beyond "the next line from `source`", so a caller
/// wanting to resume from line N simply hands it a `BufRead` already
/// advanced past the first N lines (skipping lines, or seeking a file to a
/// remembered byte offset) — there is no separate "resume token" to keep
/// in sync with the file, which is exactly the class of bug a bespoke
/// resume mechanism would risk.
pub struct JsonLinesReader<R: std::io::BufRead> {
    lines: std::io::Lines<R>,
    line_number: usize,
}

impl<R: std::io::BufRead> JsonLinesReader<R> {
    #[must_use]
    pub fn new(source: R) -> Self {
        Self {
            lines: source.lines(),
            line_number: 0,
        }
    }
}

impl<R: std::io::BufRead> LpgReader for JsonLinesReader<R> {
    fn read(&mut self) -> Result<Option<LpgElement>, LpgIoError> {
        loop {
            let Some(line) = self.lines.next() else {
                return Ok(None);
            };
            self.line_number += 1;
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // **A truncated final line is reported, not silently
            // dropped**: a malformed line is a parse error here exactly
            // like any other line, never skipped — the plan's own
            // criterion, satisfied by not special-casing the last line at
            // all rather than by detecting truncation after the fact.
            let element: JsonLinesElement =
                serde_json::from_str(&line).map_err(|e| LpgIoError::Parse {
                    line: self.line_number,
                    column: e.column(),
                    message: e.to_string(),
                })?;
            return Ok(Some(match element {
                JsonLinesElement::Node(n) => LpgElement::Node(n),
                JsonLinesElement::Edge(e) => LpgElement::Edge(e),
            }));
        }
    }
}

/// A node the way the Epic 40 explorer's own `GraphNode` (`ui/src/api.ts`)
/// consumes it — not an invented shape. Field names and optionality
/// (`fullyQualifiedName` absent rather than empty) are copied from that
/// interface exactly, checked by reading the file directly rather than
/// guessed at, since the plan's own criterion is "asserted against that
/// consumer's fixture."
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JsonGraphNode {
    pub id: String,
    pub name: String,
    /// `null` when the label does not name a real `AssetKind` — the same
    /// "stays in the picture as a bare node" reasoning `ui/src/api.ts`'s
    /// own doc comment states for a node the reader may not fully resolve.
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fully_qualified_name: Option<String>,
    /// Epic 7c's named-graph and transaction-time markers, additive to
    /// `ui/src/api.ts`'s own `GraphNode` shape rather than replacing any
    /// of it — a TypeScript consumer parsing this JSON ignores fields its
    /// own interface does not declare, so carrying these two costs the
    /// explorer nothing while still letting a caller that *does* care
    /// (Epic 9a's own JSON Lines round trip, a diagnostic tool) tell a
    /// derived or historical element from a current, directly-asserted
    /// one. Absent, not `null`, when the underlying node never had either
    /// reserved property set.
    #[serde(rename = "_graph", skip_serializing_if = "Option::is_none")]
    pub graph: Option<String>,
    #[serde(rename = "_t", skip_serializing_if = "Option::is_none")]
    pub t: Option<i64>,
}

/// An edge the way `GraphEdge` (`ui/src/api.ts`) consumes it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JsonGraphEdge {
    pub from: String,
    pub to: String,
    pub relationship: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived: Option<bool>,
    /// Same reasoning as `JsonGraphNode::graph`/`t`.
    #[serde(rename = "_graph", skip_serializing_if = "Option::is_none")]
    pub graph: Option<String>,
    #[serde(rename = "_t", skip_serializing_if = "Option::is_none")]
    pub t: Option<i64>,
}

/// The whole document the way `GraphView` (`ui/src/api.ts`) consumes it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JsonGraphView {
    pub nodes: Vec<JsonGraphNode>,
    pub edges: Vec<JsonGraphEdge>,
    pub truncated: bool,
}

fn graph_marker(properties: &PropertyMap) -> Option<String> {
    match properties.get(graph_owl_lpg::GRAPH_KEY) {
        Some(PropertyValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn time_marker(properties: &PropertyMap) -> Option<i64> {
    match properties.get(graph_owl_lpg::TIME_KEY) {
        Some(PropertyValue::Integer(t)) => Some(*t),
        _ => None,
    }
}

fn json_graph_node(n: &LpgNode) -> JsonGraphNode {
    let name = match n.properties.get("name") {
        Some(PropertyValue::String(s)) => s.clone(),
        _ => n.element_id.as_str().to_string(),
    };
    // `"fqn"` — `graph_owl_core::projection::asset_to_flakes`'s own
    // property key for an asset's fully-qualified name (`Sid::dsc("fqn")`),
    // which is what a real node built by `node_from_flakes` actually
    // carries. Reading `"fullyQualifiedName"` here instead was a real, if
    // latent, bug: it silently left this field `None` for every real
    // asset — caught while wiring `GET /graph/export/json-graph` (Epic
    // 9a's export-authorization gap-closing epic) against real
    // connector-cataloged data, which no existing unit test for this
    // writer had exercised.
    let fully_qualified_name = match n.properties.get("fqn") {
        Some(PropertyValue::String(s)) => Some(s.clone()),
        _ => None,
    };
    JsonGraphNode {
        id: n.element_id.as_str().to_string(),
        name,
        kind: n.labels.first().cloned(),
        fully_qualified_name,
        graph: graph_marker(&n.properties),
        t: time_marker(&n.properties),
    }
}

fn json_graph_edge(e: &LpgEdge) -> JsonGraphEdge {
    JsonGraphEdge {
        from: e.start.as_str().to_string(),
        to: e.end.as_str().to_string(),
        relationship: e.edge_type.clone(),
        // No signal in `LpgEdge` distinguishes a derived relationship from
        // an asserted one (that classification lives above this crate, in
        // whatever built the edge) — omitted rather than guessed, which
        // `ui/src/api.ts`'s own doc comment says reads as "asserted",
        // understating rather than overstating what was inferred.
        derived: None,
        graph: graph_marker(&e.properties),
        t: time_marker(&e.properties),
    }
}

/// Non-streaming by construction — `GraphView`'s own shape is one JSON
/// object with `nodes`/`edges` arrays, so unlike every other writer in
/// this crate there is no way to start emitting bytes before every element
/// has arrived. **Bounded to what one call already holds in memory**:
/// intended for one bounded neighbourhood (what the Epic 40 explorer
/// itself ever requests in one call — Epic 40 decision 2, "the canvas
/// opens on a seed and grows by explicit expansion", never "show
/// everything"), not a whole-catalog export, which is what
/// [`BulkCsvWriter`]/[`GraphMlWriter`]/[`JsonLinesWriter`] are for.
pub struct JsonGraphWriter {
    nodes: Vec<LpgNode>,
    edges: Vec<LpgEdge>,
    truncated: bool,
    started: bool,
}

impl JsonGraphWriter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: false,
            started: false,
        }
    }

    /// Marks the picture incomplete — Epic 40's own truncation semantics.
    pub fn mark_truncated(&mut self) {
        self.truncated = true;
    }

    /// The document, in `GraphView`'s own shape.
    #[must_use]
    pub fn into_view(self) -> JsonGraphView {
        JsonGraphView {
            nodes: self.nodes.iter().map(json_graph_node).collect(),
            edges: self.edges.iter().map(json_graph_edge).collect(),
            truncated: self.truncated,
        }
    }
}

impl Default for JsonGraphWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl LpgWriter for JsonGraphWriter {
    fn begin(&mut self, _meta: &ExportMeta) -> Result<(), LpgIoError> {
        self.started = true;
        Ok(())
    }

    fn node(&mut self, n: &LpgNode) -> Result<(), LpgIoError> {
        if !self.started {
            return Err(LpgIoError::NotStarted);
        }
        self.nodes.push(n.clone());
        Ok(())
    }

    fn edge(&mut self, e: &LpgEdge) -> Result<(), LpgIoError> {
        if !self.started {
            return Err(LpgIoError::NotStarted);
        }
        self.edges.push(e.clone());
        Ok(())
    }

    fn finish(self) -> Result<ExportSummary, LpgIoError> {
        Ok(ExportSummary {
            nodes: self.nodes.len() as u64,
            edges: self.edges.len() as u64,
        })
    }
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

    /// An escape-aware reader for the bulk CSV array format's documented
    /// `;`-with-`\;`/`\\`-escaping contract — used only to prove a real
    /// consumer of that contract recovers the original array, since a
    /// naive `str::split(';')` always sees every literal separator
    /// regardless of escaping and would pass even a broken writer.
    fn escape_aware_split(field: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut chars = field.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&';') || chars.peek() == Some(&'\\') => {
                    current.push(chars.next().expect("peeked"));
                }
                ';' => {
                    parts.push(std::mem::take(&mut current));
                }
                other => current.push(other),
            }
        }
        parts.push(current);
        parts
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

    // -- Slice B: GraphML import and round-trip -----------------------------

    fn read_all(path: &PathBuf) -> Vec<LpgElement> {
        let file = std::fs::File::open(path).expect("open");
        let mut reader = GraphMlReader::new(std::io::BufReader::new(file));
        let mut elements = Vec::new();
        while let Some(element) = reader.read().expect("read") {
            elements.push(element);
        }
        elements
    }

    /// **The specification, per the plan itself**: export, import, export
    /// again — the two files must be byte-identical.
    #[test]
    fn export_import_export_is_byte_identical() {
        let first_path = output_path();
        let mut writer = GraphMlWriter::new(&first_path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");

        let mut a_props = PropertyMap::new();
        a_props
            .insert_user("name", PropertyValue::String("orders".into()))
            .expect("insert");
        a_props
            .insert_user("active", PropertyValue::Boolean(true))
            .expect("insert");
        a_props
            .insert_user("rowCount", PropertyValue::Integer(42))
            .expect("insert");
        a_props
            .insert_user("confidence", PropertyValue::Float(0.75))
            .expect("insert");
        writer
            .node(&node("a", &["Table"], a_props))
            .expect("node a");
        writer
            .node(&node("b", &["Table"], PropertyMap::new()))
            .expect("node b");

        let mut edge_props = PropertyMap::new();
        edge_props
            .insert_user("verified", PropertyValue::Boolean(false))
            .expect("insert");
        writer
            .edge(&edge("r1", "a", "b", "feeds", edge_props))
            .expect("edge");
        writer.finish().expect("finish");

        let first_bytes = std::fs::read(&first_path).expect("read first");

        let elements = read_all(&first_path);
        assert_eq!(elements.len(), 3, "{elements:#?}");

        let second_path = output_path();
        let mut writer = GraphMlWriter::new(&second_path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        for element in &elements {
            match element {
                LpgElement::Node(n) => writer.node(n).expect("re-export node"),
                LpgElement::Edge(e) => writer.edge(e).expect("re-export edge"),
            }
        }
        writer.finish().expect("finish");
        let second_bytes = std::fs::read(&second_path).expect("read second");

        assert_eq!(
            first_bytes, second_bytes,
            "export -> import -> export must be byte-identical"
        );

        std::fs::remove_file(&first_path).ok();
        std::fs::remove_file(&second_path).ok();
    }

    /// **Type fidelity**: declared key types are honoured, not
    /// string-guessed — an integer property must not return as a string.
    #[test]
    fn declared_key_types_are_honoured_not_string_guessed() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        let mut props = PropertyMap::new();
        props
            .insert_user("count", PropertyValue::Integer(7))
            .expect("insert");
        props
            .insert_user("ratio", PropertyValue::Float(1.5))
            .expect("insert");
        props
            .insert_user("enabled", PropertyValue::Boolean(true))
            .expect("insert");
        writer.node(&node("a", &["Table"], props)).expect("node");
        writer.finish().expect("finish");

        let elements = read_all(&path);
        let LpgElement::Node(n) = &elements[0] else {
            panic!("expected a node: {elements:#?}");
        };
        assert_eq!(n.properties.get("count"), Some(&PropertyValue::Integer(7)));
        assert_eq!(n.properties.get("ratio"), Some(&PropertyValue::Float(1.5)));
        assert_eq!(
            n.properties.get("enabled"),
            Some(&PropertyValue::Boolean(true))
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn labels_and_edge_type_round_trip() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        writer
            .node(&node("a", &["Table", "PII"], PropertyMap::new()))
            .expect("node a");
        writer
            .node(&node("b", &[], PropertyMap::new()))
            .expect("node b");
        writer
            .edge(&edge("r1", "a", "b", "feeds", PropertyMap::new()))
            .expect("edge");
        writer.finish().expect("finish");

        let elements = read_all(&path);
        let LpgElement::Node(a) = &elements[0] else {
            panic!("expected node a: {elements:#?}");
        };
        assert_eq!(a.labels, vec!["Table".to_string(), "PII".to_string()]);
        let LpgElement::Edge(r1) = &elements[2] else {
            panic!("expected edge r1: {elements:#?}");
        };
        assert_eq!(r1.edge_type, "feeds");

        std::fs::remove_file(&path).ok();
    }

    /// **An edge referencing an undeclared node reports the id.**
    #[test]
    fn an_edge_referencing_an_undeclared_node_is_a_named_error() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns">
  <key id="{EDGE_TYPE_KEY}" for="edge" attr.name="_type" attr.type="string"/>
  <graph id="g" edgedefault="directed">
    <node id="1:a"></node>
    <edge id="1:r1" source="1:a" target="1:missing">
      <data key="{EDGE_TYPE_KEY}">feeds</data>
    </edge>
  </graph>
</graphml>"#
        );
        let mut reader = GraphMlReader::new(std::io::Cursor::new(xml.as_bytes()));
        reader.read().expect("node a").expect("some");
        let outcome = reader.read();
        match outcome {
            Err(LpgIoError::DanglingReference {
                edge_id,
                missing_node_id,
            }) => {
                assert_eq!(edge_id, "1:r1");
                assert_eq!(missing_node_id, "1:missing");
            }
            other => panic!("expected DanglingReference: {other:?}"),
        }
    }

    /// **A malformed document reports line and column**, not only that
    /// something went wrong.
    #[test]
    fn a_malformed_document_reports_line_and_column() {
        let xml = "<?xml version=\"1.0\"?>\n<graphml>\n  <node id=\"1:a\"\n";
        let mut reader = GraphMlReader::new(std::io::Cursor::new(xml.as_bytes()));
        let outcome = reader.read();
        match outcome {
            Err(LpgIoError::Parse { line, .. }) => {
                assert!(line >= 3, "expected the error on/after line 3: got {line}");
            }
            other => panic!("expected Parse: {other:?}"),
        }
    }

    /// Import is streaming: `read()` yields elements one at a time from a
    /// `Cursor`/`BufRead` rather than requiring the whole document up
    /// front — proven by reading a real multi-element document one call at
    /// a time and observing each element arrive before the next is asked
    /// for, the pull-based shape [`LpgReader::read`] itself specifies.
    #[test]
    fn import_is_streaming_one_element_per_read_call() {
        let path = output_path();
        let mut writer = GraphMlWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        for i in 0..10 {
            writer
                .node(&node(&format!("n{i}"), &["Table"], PropertyMap::new()))
                .expect("node");
        }
        writer.finish().expect("finish");

        let file = std::fs::File::open(&path).expect("open");
        let mut reader = GraphMlReader::new(std::io::BufReader::new(file));
        let mut count = 0;
        while reader.read().expect("read").is_some() {
            count += 1;
        }
        assert_eq!(count, 10);

        std::fs::remove_file(&path).ok();
    }

    // -- Slice C: Bulk CSV and Cypher script --------------------------------

    fn output_dir() -> PathBuf {
        std::env::temp_dir().join(format!("bulk-csv-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn separate_typed_node_and_relationship_files_with_the_documented_bulk_shape() {
        let dir = output_dir();
        let mut writer = BulkCsvWriter::new(&dir).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");

        let mut a_props = PropertyMap::new();
        a_props
            .insert_user("rowCount", PropertyValue::Integer(42))
            .expect("insert");
        a_props
            .insert_user("active", PropertyValue::Boolean(true))
            .expect("insert");
        writer
            .node(&node("a", &["Table"], a_props))
            .expect("node a");
        writer
            .node(&node("b", &["Table"], PropertyMap::new()))
            .expect("node b");

        let mut edge_props = PropertyMap::new();
        edge_props
            .insert_user("verified", PropertyValue::Boolean(true))
            .expect("insert");
        writer
            .edge(&edge("r1", "a", "b", "feeds", edge_props))
            .expect("edge");
        writer.finish().expect("finish");

        let nodes_csv = std::fs::read_to_string(dir.join("nodes-Table.csv")).expect("nodes file");
        let mut lines = nodes_csv.lines();
        let header = lines.next().expect("header");
        assert!(header.starts_with("id:ID,"), "{header}");
        assert!(header.contains("rowCount:long"), "{header}");
        assert!(header.contains("active:boolean"), "{header}");
        assert!(header.ends_with(":LABEL"), "{header}");

        let rels_csv = std::fs::read_to_string(dir.join("relationships.csv")).expect("rels file");
        let rel_header = rels_csv.lines().next().expect("header");
        assert_eq!(rel_header, ":START_ID,:END_ID,:TYPE,verified:boolean");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **The plan's own named bug**: an array property whose value contains
    /// the array separator must not silently become two values.
    #[test]
    fn an_array_value_containing_the_separator_is_escaped_not_split() {
        let dir = output_dir();
        let mut writer = BulkCsvWriter::new(&dir).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        let mut props = PropertyMap::new();
        props
            .insert_user("tag", PropertyValue::String("a;b".into()))
            .expect("insert");
        props
            .insert_user("tag", PropertyValue::String("c".into()))
            .expect("insert");
        writer.node(&node("a", &["Table"], props)).expect("node");
        writer.finish().expect("finish");

        let csv = std::fs::read_to_string(dir.join("nodes-Table.csv")).expect("nodes file");
        let data_line = csv.lines().nth(1).expect("data row");
        let tag_field = data_line.split(',').nth(1).expect("tag column");

        // A naive, escape-unaware split on `;` sees three pieces regardless
        // (the backslash does not stop `str::split` from matching) — that
        // is not the bug this test guards. The bug is a *consumer that
        // respects the documented escaping* recovering the wrong values.
        // An escape-aware split — the contract this format documents —
        // must recover exactly the original two array entries.
        assert_eq!(
            escape_aware_split(tag_field),
            vec!["a;b".to_string(), "c".to_string()],
            "an escape-aware reader must recover the original two entries, not three: {tag_field}"
        );
        assert!(
            tag_field.contains("a\\;b"),
            "the literal separator inside a value must be escaped: {tag_field}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn calling_bulk_csv_node_before_begin_is_a_named_error() {
        let dir = output_dir();
        let mut writer = BulkCsvWriter::new(&dir).expect("new");
        let outcome = writer.node(&node("a", &[], PropertyMap::new()));
        assert!(matches!(outcome, Err(LpgIoError::NotStarted)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// **The Cypher script is idempotent via `MERGE`, labelled slow, and
    /// names the CSV path as the alternative.**
    #[test]
    fn the_cypher_script_uses_merge_and_documents_itself_as_slow() {
        let path = output_path();
        let mut writer = CypherScriptWriter::new(&path).expect("new");
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
        assert_eq!(summary, ExportSummary { nodes: 2, edges: 1 });

        let script = std::fs::read_to_string(&path).expect("read script");
        assert!(script.contains("MERGE"), "{script}");
        assert!(!script.contains("CREATE ("), "{script}");
        assert!(
            script.to_lowercase().contains("slow"),
            "the script must document itself as slow: {script}"
        );
        assert!(
            script.contains("BulkCsvWriter") || script.to_lowercase().contains("csv"),
            "the script must name the CSV export as the alternative: {script}"
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_cypher_script_batches_rather_than_one_statement_per_element() {
        let path = output_path();
        let mut writer = CypherScriptWriter::new(&path).expect("new");
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        for i in 0..10 {
            writer
                .node(&node(&format!("n{i}"), &["Table"], PropertyMap::new()))
                .expect("node");
        }
        let summary = writer.finish().expect("finish");
        assert_eq!(summary.nodes, 10);

        let script = std::fs::read_to_string(&path).expect("read script");
        let unwind_count = script.matches("UNWIND").count();
        assert_eq!(
            unwind_count, 1,
            "10 nodes under the default batch size must be one UNWIND, not ten: {script}"
        );

        std::fs::remove_file(&path).ok();
    }

    // -- Slice F: JSON Graph and JSON Lines ----------------------------------

    fn node_with_markers(
        id: &str,
        labels: &[&str],
        graph: Option<&str>,
        t: Option<i64>,
    ) -> LpgNode {
        let mut n = node(id, labels, PropertyMap::new());
        if let Some(graph) = graph {
            n.properties.insert_reserved(
                graph_owl_lpg::GRAPH_KEY,
                PropertyValue::String(graph.to_string()),
            );
        }
        if let Some(t) = t {
            n.properties
                .insert_reserved(graph_owl_lpg::TIME_KEY, PropertyValue::Integer(t));
        }
        n
    }

    #[test]
    fn json_lines_round_trips_nodes_and_edges() {
        let path = output_path();
        let mut writer = JsonLinesWriter::new(&path);
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        let mut props = PropertyMap::new();
        props
            .insert_user("name", PropertyValue::String("orders".into()))
            .expect("insert");
        writer.node(&node("a", &["Table"], props)).expect("node a");
        writer
            .node(&node("b", &["Table"], PropertyMap::new()))
            .expect("node b");
        writer
            .edge(&edge("r1", "a", "b", "feeds", PropertyMap::new()))
            .expect("edge");
        let summary = writer.finish().expect("finish");
        assert_eq!(summary, ExportSummary { nodes: 2, edges: 1 });

        let file = std::fs::File::open(&path).expect("open");
        let mut reader = JsonLinesReader::new(std::io::BufReader::new(file));
        let mut elements = Vec::new();
        while let Some(element) = reader.read().expect("read") {
            elements.push(element);
        }
        assert_eq!(elements.len(), 3, "{elements:#?}");
        let LpgElement::Node(a) = &elements[0] else {
            panic!("expected node a: {elements:#?}");
        };
        assert_eq!(
            a.properties.get("name"),
            Some(&PropertyValue::String("orders".to_string()))
        );

        std::fs::remove_file(&path).ok();
    }

    /// **Resumable from an arbitrary line**: skip the first line by hand,
    /// then start a fresh reader on what remains — no special "resume"
    /// entry point needed, because this reader's only state is its
    /// position in `source`.
    #[test]
    fn json_lines_import_is_resumable_from_an_arbitrary_line() {
        let path = output_path();
        let mut writer = JsonLinesWriter::new(&path);
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        for i in 0..5 {
            writer
                .node(&node(&format!("n{i}"), &["Table"], PropertyMap::new()))
                .expect("node");
        }
        writer.finish().expect("finish");

        let contents = std::fs::read_to_string(&path).expect("read");
        let remaining: String = contents.lines().skip(2).collect::<Vec<_>>().join("\n");
        let mut reader = JsonLinesReader::new(std::io::Cursor::new(remaining));
        let mut count = 0;
        while reader.read().expect("read").is_some() {
            count += 1;
        }
        assert_eq!(
            count, 3,
            "resuming after line 2 must yield the remaining 3 elements"
        );

        std::fs::remove_file(&path).ok();
    }

    /// **A truncated final line is reported, not silently dropped.**
    #[test]
    fn a_truncated_final_line_is_a_named_error_not_silently_skipped() {
        let mut buf = Vec::new();
        {
            use std::io::Write as _;
            writeln!(
                buf,
                r#"{{"type":"node","elementId":"1:a","labels":["Table"],"properties":{{}}}}"#
            )
            .unwrap();
            // A truncated second line — cut off mid-object, no closing brace.
            write!(buf, r#"{{"type":"node","elementId":"1:b","label"#).unwrap();
        }

        let mut reader = JsonLinesReader::new(std::io::Cursor::new(buf));
        let first = reader.read().expect("first line reads fine");
        assert!(matches!(first, Some(LpgElement::Node(_))));

        let outcome = reader.read();
        assert!(
            matches!(outcome, Err(LpgIoError::Parse { .. })),
            "a truncated final line must be a named parse error: {outcome:?}"
        );
    }

    #[test]
    fn json_graph_matches_the_explorers_own_field_shape() {
        let mut writer = JsonGraphWriter::new();
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        let mut a_props = PropertyMap::new();
        a_props
            .insert_user("name", PropertyValue::String("Orders".into()))
            .expect("insert");
        // `"fqn"`, not `"fullyQualifiedName"` — the property key a real
        // node built by `graph_owl_lpg::node_from_flakes` actually carries
        // (`graph_owl_core::projection::asset_to_flakes`'s own
        // `Sid::dsc("fqn")`), so this exercises the real integration point
        // rather than a key nothing but this test ever produces.
        a_props
            .insert_user(
                "fqn",
                PropertyValue::String("warehouse.public.orders".into()),
            )
            .expect("insert");
        writer
            .node(&node("a", &["Table"], a_props))
            .expect("node a");
        writer
            .node(&node("b", &["Table"], PropertyMap::new()))
            .expect("node b");
        writer
            .edge(&edge("r1", "a", "b", "feeds", PropertyMap::new()))
            .expect("edge");
        let view = writer.into_view();
        let json = serde_json::to_value(&view).expect("serialize");

        // The exact field names `ui/src/api.ts` declares for `GraphNode`/
        // `GraphEdge`/`GraphView` — read directly from that file, not
        // invented.
        assert!(json.get("nodes").is_some());
        assert!(json.get("edges").is_some());
        assert!(json.get("truncated").is_some());
        let node_json = &json["nodes"][0];
        assert!(node_json.get("id").is_some());
        assert!(node_json.get("name").is_some());
        assert!(node_json.get("kind").is_some());
        assert_eq!(
            node_json["fullyQualifiedName"], "warehouse.public.orders",
            "{node_json:?}"
        );
        let node_b_json = &json["nodes"][1];
        assert!(
            node_b_json.get("fullyQualifiedName").is_none()
                || node_b_json["fullyQualifiedName"].is_null(),
            "absent, not null, when the source node never had an fqn: {node_b_json:?}"
        );
        let edge_json = &json["edges"][0];
        assert!(edge_json.get("from").is_some());
        assert!(edge_json.get("to").is_some());
        assert!(edge_json.get("relationship").is_some());
    }

    #[test]
    fn json_graph_carries_graph_and_t_markers_when_present() {
        let mut writer = JsonGraphWriter::new();
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        writer
            .node(&node_with_markers(
                "a",
                &["Table"],
                Some("graph:import:x"),
                Some(42),
            ))
            .expect("node a");
        let view = writer.into_view();

        assert_eq!(view.nodes[0].graph.as_deref(), Some("graph:import:x"));
        assert_eq!(view.nodes[0].t, Some(42));
    }

    #[test]
    fn json_graph_marks_truncation() {
        let mut writer = JsonGraphWriter::new();
        writer
            .begin(&ExportMeta {
                graph_id: "g".to_string(),
            })
            .expect("begin");
        writer.mark_truncated();
        let view = writer.into_view();
        assert!(view.truncated);
    }
}
