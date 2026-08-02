//! Document parsing — Epic 21 Slice A, and the port every future worker
//! implements.
//!
//! **The port is the deliverable; the markdown adapter is the proof it
//! works.** Decision 4: no Python runtime is required for core operation,
//! so something has to parse *something* in-process. Markdown and plain
//! text are the right something — runbooks, decision records and notebooks
//! are already written in them, and neither needs a layout engine.
//!
//! PDF, OCR and multimodal parsing arrive later as external workers
//! (decision 0). They implement nothing in this file: they produce a
//! [`ParsedDocument`] as JSON and hand it over. That is why the port takes
//! bytes and returns a plain data type — anything richer would be a shape
//! only an in-process Rust parser could satisfy, and the boundary would
//! have to move the first time a worker appeared.

use graph_owl_core::extraction::{ParsedDocument, Section, TextSpan};

#[derive(Debug)]
pub enum ParseError {
    /// The bytes are not the media type they claimed to be.
    Malformed(String),
    /// This parser does not handle that media type — a routing answer, not
    /// a failure of the document.
    Unsupported(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Malformed(detail) => write!(f, "could not parse: {detail}"),
            ParseError::Unsupported(media_type) => {
                write!(f, "no parser for media type `{media_type}`")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Turns raw bytes into the neutral document shape.
///
/// Takes `&[u8]`, not `&str`: an OCR or PDF worker starts from bytes, and a
/// port that demanded valid UTF-8 up front would be one an image-based
/// source could not implement.
pub trait DocumentParser: Send + Sync {
    /// Media types this parser claims, for routing.
    fn handles(&self) -> &[&str];

    /// # Errors
    ///
    /// [`ParseError`] if the bytes cannot be read as this media type.
    fn parse(
        &self,
        source_id: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<ParsedDocument, ParseError>;
}

/// Markdown and plain text.
///
/// Deliberately **not** a full markdown implementation. What extraction
/// needs from markdown is the text and where the headings are — enough to
/// say "this claim came from the *Rollback* section". Rendering, links,
/// tables and emphasis are all irrelevant to a claim's meaning, and pulling
/// in a full parser to discard its output would be cost without benefit.
pub struct MarkdownParser;

impl DocumentParser for MarkdownParser {
    fn handles(&self) -> &[&str] {
        &["markdown", "text/markdown", "text", "text/plain"]
    }

    fn parse(
        &self,
        source_id: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<ParsedDocument, ParseError> {
        if !self.handles().contains(&media_type) {
            return Err(ParseError::Unsupported(media_type.to_string()));
        }
        // **Lossy, on purpose.** A runbook with one mangled byte is still a
        // runbook; refusing the whole document over an encoding slip would
        // lose every claim in it to protect nothing. The replacement
        // character is visible in the evidence span if anyone looks.
        let text = String::from_utf8_lossy(bytes).into_owned();

        Ok(ParsedDocument {
            source_id: source_id.to_string(),
            media_type: media_type.to_string(),
            sections: sections_of(&text),
            text,
        })
    }
}

/// ATX headings (`#`…`######`) and the body under each.
///
/// A section runs to the next heading of **any** level, not the next of the
/// same level. Nesting would be a tree, and the only thing downstream wants
/// is "which heading was this text under" — the nearest one, which is what
/// this gives.
fn sections_of(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut offset = 0usize;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let hashes = trimmed.chars().take_while(|c| *c == '#').count();
            // `#hashtag` is not a heading; ATX requires a space after the
            // run of hashes, and treating it as one would section a document
            // on its own prose.
            let is_heading = (1..=6).contains(&hashes)
                && trimmed.chars().nth(hashes).is_some_and(char::is_whitespace);
            if is_heading {
                if let Some(previous) = sections.last_mut() {
                    previous.span.end = offset;
                }
                sections.push(Section {
                    heading: Some(trimmed[hashes..].trim().to_string()),
                    span: TextSpan::new(offset, text.len()),
                });
            }
        }
        offset += line.len();
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNBOOK: &str = "\
# Orders service

The orders table holds one row per order.

## Rollback

Revert the migration, then restart.
";

    #[test]
    fn plain_text_parses_with_its_text_intact() {
        let parsed = MarkdownParser
            .parse("notes.txt", "text", b"just some notes")
            .expect("parses");

        assert_eq!(parsed.text, "just some notes");
        assert_eq!(parsed.source_id, "notes.txt");
        assert!(parsed.sections.is_empty(), "no headings, no sections");
    }

    #[test]
    fn headings_become_sections_covering_the_text_beneath_them() {
        let parsed = MarkdownParser
            .parse("runbook.md", "markdown", RUNBOOK.as_bytes())
            .expect("parses");

        let headings: Vec<&str> = parsed
            .sections
            .iter()
            .filter_map(|s| s.heading.as_deref())
            .collect();
        assert_eq!(headings, vec!["Orders service", "Rollback"]);

        // The first section stops where the second begins, rather than
        // running to the end of the document.
        let first = parsed.sections[0]
            .span
            .resolve(&parsed.text)
            .expect("in range");
        assert!(first.contains("one row per order"), "{first:?}");
        assert!(!first.contains("Revert the migration"), "{first:?}");
    }

    /// The last section runs to the end — there is no following heading to
    /// stop it.
    #[test]
    fn the_final_section_extends_to_the_end_of_the_document() {
        let parsed = MarkdownParser
            .parse("runbook.md", "markdown", RUNBOOK.as_bytes())
            .expect("parses");

        let last = parsed.sections.last().expect("a section");
        assert_eq!(last.span.end, parsed.text.len());
        assert!(
            last.span
                .resolve(&parsed.text)
                .expect("in range")
                .contains("restart")
        );
    }

    /// `#hashtag` is prose, not a heading. Sectioning a document on its own
    /// content would put claims under headings that do not exist.
    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        let parsed = MarkdownParser
            .parse("n.md", "markdown", b"#notaheading\nbody\n")
            .expect("parses");

        assert!(parsed.sections.is_empty(), "{:?}", parsed.sections);
    }

    /// Seven hashes is not a heading either — ATX stops at six, and a
    /// parser that accepted more would disagree with every other reader of
    /// the same file.
    #[test]
    fn more_than_six_hashes_is_not_a_heading() {
        let parsed = MarkdownParser
            .parse("n.md", "markdown", b"####### too deep\n")
            .expect("parses");

        assert!(parsed.sections.is_empty(), "{:?}", parsed.sections);
    }

    /// **Invalid UTF-8 is survivable.** A runbook with one mangled byte is
    /// still a runbook; refusing it would lose every claim inside to protect
    /// nothing.
    #[test]
    fn invalid_utf8_is_read_lossily_rather_than_refused() {
        let bytes = b"orders \xF0\x28\x8C table";

        let parsed = MarkdownParser
            .parse("broken.md", "markdown", bytes)
            .expect("a mangled byte must not lose the document");

        assert!(parsed.text.contains("orders"), "{}", parsed.text);
        assert!(parsed.text.contains("table"), "{}", parsed.text);
    }

    /// Routing is the parser's own answer, so a caller can ask rather than
    /// maintain a second table of who handles what.
    #[test]
    fn an_unhandled_media_type_is_refused_by_name() {
        let error = MarkdownParser
            .parse("scan.pdf", "application/pdf", b"%PDF-1.7")
            .expect_err("a PDF is an external worker's job");

        assert!(matches!(error, ParseError::Unsupported(ref m) if m == "application/pdf"));
        assert!(error.to_string().contains("application/pdf"));
    }

    /// Every span a parser emits must be resolvable against the text it
    /// emitted — the one invariant downstream code relies on without
    /// checking.
    #[test]
    fn every_section_span_resolves_against_the_parsed_text() {
        let parsed = MarkdownParser
            .parse("runbook.md", "markdown", RUNBOOK.as_bytes())
            .expect("parses");

        for section in &parsed.sections {
            assert!(
                section.span.resolve(&parsed.text).is_some(),
                "unresolvable span {:?} in {} bytes",
                section.span,
                parsed.text.len()
            );
        }
    }
}
