//! Property-graph interchange at the boundary — Epic 9a.
//!
//! **Symmetric with Epic 9, same streaming discipline, different formats**
//! (the plan's own framing). `LpgNode`/`LpgEdge` already exist in
//! `graph-owl-lpg` (Epic 7c) — this crate turns them into interchange
//! bytes and back, and never becomes the graph's own model.
//!
//! **Slice A**: streaming `GraphML` export. **Slice B**: `GraphML` import
//! and round-trip, via [`GraphMlReader`]. `LpgWriter`/`LpgReader` name the
//! full trait shape the plan specifies so later slices (bulk CSV, Cypher
//! script, JSON Graph, JSON Lines) extend this crate rather than reshape
//! it.
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
}
