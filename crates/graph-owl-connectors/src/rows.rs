//! Reading a batch file one row at a time — Epic 16 Slice C.
//!
//! The criterion is "memory stays bounded on a 500k-row file", and the way to
//! meet it is structural rather than clever: **nothing here ever holds more than
//! one row.** The parser is an iterator over a `BufRead`, so a 500k-row file and
//! a 5-row file cost the same resident memory, and there is no buffer size to
//! tune wrong.
//!
//! Two formats: JSONL and CSV. **Parquet is deliberately not here** — it is
//! columnar, so a reader must materialise a row group at a time, which is exactly
//! the property this module exists to avoid. It also costs the `arrow` +
//! `parquet` dependency pair for a format any pusher can convert from in one
//! line. `00l-build-vs-adopt.md` says adopt over write; it does not say adopt
//! regardless of what it costs.

use std::io::BufRead;

/// A row as read, before anything decides whether it is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// **1-based, and the line number in the file** — not an index into the rows
    /// this parser produced. Slice C requires per-row errors to carry the row
    /// number, and a client greps their file with it; a count of successfully
    /// parsed rows would point at the wrong line the moment one was skipped.
    pub number: u64,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RowError {
    #[error("row {number}: {detail}")]
    Malformed { number: u64, detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jsonl,
    Csv,
}

impl Format {
    /// The format a content type or filename names, if this build understands it.
    ///
    /// `None` for anything else — including Parquet, which is refused by name
    /// rather than falling through to a JSONL parser that would report every row
    /// as malformed and bury the real problem.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.to_ascii_lowercase();
        if value.contains("jsonl") || value.contains("ndjson") {
            Some(Self::Jsonl)
        } else if value.contains("csv") {
            Some(Self::Csv)
        } else {
            None
        }
    }
}

/// Read rows one at a time.
///
/// Returns an iterator rather than a `Vec` — that *is* the memory bound, and
/// returning a collection would make the bound impossible to state.
pub fn rows<R: BufRead + 'static>(
    reader: R,
    format: Format,
) -> Box<dyn Iterator<Item = Result<Row, RowError>>> {
    match format {
        Format::Jsonl => Box::new(jsonl(reader)),
        Format::Csv => csv_rows(reader),
    }
}

fn jsonl<R: BufRead + 'static>(reader: R) -> impl Iterator<Item = Result<Row, RowError>> {
    // `lines()` is the whole memory argument: it hands back one line and forgets
    // it. Enumerating here rather than counting emitted rows is what keeps a row
    // number equal to the line number a client would see in an editor.
    reader.lines().enumerate().filter_map(|(offset, line)| {
        let number = u64::try_from(offset + 1).unwrap_or(u64::MAX);
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                return Some(Err(RowError::Malformed {
                    number,
                    detail: error.to_string(),
                }));
            }
        };
        // Skipped, not reported: a trailing newline is how text files end, and a
        // spurious entry in every job's report trains people to ignore the report.
        if line.trim().is_empty() {
            return None;
        }
        Some(match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(serde_json::Value::Object(fields)) => Ok(Row { number, fields }),
            Ok(other) => Err(RowError::Malformed {
                number,
                detail: format!("expected a JSON object, found {}", json_kind(&other)),
            }),
            Err(error) => Err(RowError::Malformed {
                number,
                detail: error.to_string(),
            }),
        })
    })
}

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

fn csv_rows<R: BufRead + 'static>(reader: R) -> Box<dyn Iterator<Item = Result<Row, RowError>>> {
    // `flexible(true)` so the reader hands back a short row instead of its own
    // error: the width check below owns that message, and the reader's version
    // does not say which columns the header declared.
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(reader);
    // Reads exactly the first line. Cloned because the record iterator takes the
    // reader by value, and the header names are needed for every row after it.
    let headers = reader.headers().cloned().unwrap_or_default();
    let width = headers.len();

    Box::new(reader.into_records().map(move |record| {
        let record = record.map_err(|error| RowError::Malformed {
            number: error.position().map_or(0, csv::Position::line),
            detail: error.to_string(),
        })?;
        let number = record.position().map_or(0, csv::Position::line);
        // Not padded and not truncated. Padding invents empty values the file
        // never contained — an entity with a silently-empty name is worse than a
        // rejected row — and extra columns mean the file disagrees with its own
        // header, which is a fact worth surfacing rather than dropping.
        if record.len() != width {
            return Err(RowError::Malformed {
                number,
                detail: format!("expected {width} columns, found {}", record.len()),
            });
        }
        Ok(Row {
            number,
            fields: headers
                .iter()
                .zip(record.iter())
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        serde_json::Value::String(value.to_string()),
                    )
                })
                .collect(),
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn read(text: &str, format: Format) -> Vec<Result<Row, RowError>> {
        rows(Cursor::new(text.to_string()), format).collect()
    }

    fn field(row: &Row, key: &str) -> String {
        row.fields
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn jsonl_yields_one_row_per_line() {
        let out = read("{\"name\":\"a\"}\n{\"name\":\"b\"}\n", Format::Jsonl);

        assert_eq!(out.len(), 2);
        assert_eq!(field(out[0].as_ref().expect("row"), "name"), "a");
        assert_eq!(field(out[1].as_ref().expect("row"), "name"), "b");
    }

    // **Row numbers are file line numbers.** A client greps their file with this,
    // so counting parsed rows instead would point at the wrong line as soon as one
    // was skipped.
    #[test]
    fn a_malformed_line_does_not_shift_the_numbers_after_it() {
        let out = read(
            "{\"name\":\"a\"}\nnot json\n{\"name\":\"c\"}\n",
            Format::Jsonl,
        );

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].as_ref().expect("row").number, 1);
        assert!(matches!(out[1], Err(RowError::Malformed { number: 2, .. })));
        assert_eq!(out[2].as_ref().expect("row").number, 3);
    }

    // A bad line is reported and reading continues — one typo must not cost the
    // other 499,999 rows, which is the same partial-success principle as the
    // synchronous push.
    #[test]
    fn reading_continues_past_a_malformed_line() {
        let out = read("bad\n{\"name\":\"good\"}\n", Format::Jsonl);

        assert!(out[0].is_err());
        assert_eq!(field(out[1].as_ref().expect("row"), "name"), "good");
    }

    // Blank lines are skipped rather than reported: a trailing newline is how text
    // files end, and reporting it as an error would put a spurious entry in every
    // job's report.
    #[test]
    fn blank_lines_are_skipped_not_reported() {
        let out = read("{\"name\":\"a\"}\n\n\n{\"name\":\"b\"}\n", Format::Jsonl);

        assert_eq!(out.len(), 2);
    }

    // A JSON array or string is valid JSON but not a row. Reported as malformed
    // rather than silently producing an empty row, which would land an entity with
    // no fields.
    #[test]
    fn a_json_value_that_is_not_an_object_is_malformed() {
        let out = read("[1,2,3]\n\"just a string\"\n", Format::Jsonl);

        assert!(out.iter().all(std::result::Result::is_err), "{out:?}");
        // The detail has to name what was actually found. "malformed" alone sends
        // somebody looking for a typo in a file whose real problem is that it is a
        // JSON array rather than JSONL — a different fix entirely.
        let detail = out[0].as_ref().expect_err("array is not a row").to_string();
        assert!(detail.contains("array"), "{detail}");
        let detail = out[1]
            .as_ref()
            .expect_err("string is not a row")
            .to_string();
        assert!(detail.contains("string"), "{detail}");
    }

    #[test]
    fn csv_uses_the_header_row_for_field_names() {
        let out = read("kind,name\nservice,orders\n", Format::Csv);

        assert_eq!(out.len(), 1);
        let row = out[0].as_ref().expect("row");
        assert_eq!(field(row, "kind"), "service");
        assert_eq!(field(row, "name"), "orders");
        // Line 2, because the header is line 1 — a client counting from the file
        // needs the number they would see in an editor.
        assert_eq!(row.number, 2);
    }

    // A short row is malformed rather than padded: padding invents empty values
    // the file never contained, and an entity with a silently-empty name is worse
    // than a rejected row.
    #[test]
    fn a_csv_row_with_too_few_columns_is_malformed() {
        let out = read("kind,name\nservice\n", Format::Csv);

        assert!(matches!(out[0], Err(RowError::Malformed { number: 2, .. })));
    }

    // And a long row is malformed too, rather than truncated. The file disagrees
    // with its own header — usually a stray comma inside an unquoted value — and
    // dropping the surplus silently would shift every field after it.
    #[test]
    fn a_csv_row_with_too_many_columns_is_malformed() {
        let out = read("kind,name\nservice,orders,extra\n", Format::Csv);

        assert!(matches!(out[0], Err(RowError::Malformed { number: 2, .. })));
    }

    #[test]
    fn an_empty_csv_yields_nothing_rather_than_failing() {
        assert!(read("", Format::Csv).is_empty());
        assert!(read("kind,name\n", Format::Csv).is_empty());
    }

    #[test]
    fn an_empty_jsonl_yields_nothing() {
        assert!(read("", Format::Jsonl).is_empty());
    }

    // ---- format detection ----

    #[test]
    fn the_formats_this_build_understands_are_recognised() {
        assert_eq!(Format::parse("application/x-ndjson"), Some(Format::Jsonl));
        assert_eq!(Format::parse("data.jsonl"), Some(Format::Jsonl));
        assert_eq!(Format::parse("text/csv"), Some(Format::Csv));
        assert_eq!(Format::parse("EXPORT.CSV"), Some(Format::Csv));
    }

    // **Parquet is refused by name, not fallen through to JSONL.** A columnar file
    // fed to a line parser reports every row as malformed, which buries "this
    // build does not read Parquet" under 500k parse errors.
    #[test]
    fn a_format_this_build_cannot_read_is_refused_rather_than_guessed() {
        assert_eq!(Format::parse("application/vnd.apache.parquet"), None);
        assert_eq!(Format::parse("data.parquet"), None);
        assert_eq!(Format::parse("application/octet-stream"), None);
    }

    // ---- the memory bound ----

    /// A file that never ends. Reading it produces the same row forever, so it
    /// cannot be materialised — which is the point.
    struct Endless {
        line: Vec<u8>,
        offset: usize,
        served: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl std::io::Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut written = 0;
            while written < buf.len() {
                if self.offset == self.line.len() {
                    self.offset = 0;
                }
                let take = (self.line.len() - self.offset).min(buf.len() - written);
                buf[written..written + take]
                    .copy_from_slice(&self.line[self.offset..self.offset + take]);
                self.offset += take;
                written += take;
            }
            self.served.set(self.served.get() + written);
            Ok(written)
        }
    }

    /// **The memory-bound criterion, asserted rather than measured.**
    ///
    /// Peak RSS is the obvious instrument and the wrong one: it is noisy, it
    /// measures the allocator as much as the parser, and a test that merely
    /// finishes on a large file passes just as well when the whole file was
    /// buffered. An *endless* file cannot be buffered at all — an implementation
    /// that collected before yielding would never return — and the bytes-consumed
    /// assertion pins the bound to a constant instead of a ratio.
    #[test]
    fn reading_a_few_rows_consumes_a_bounded_prefix_of_an_endless_file() {
        for format in [Format::Jsonl, Format::Csv] {
            let served = std::rc::Rc::new(std::cell::Cell::new(0));
            let source = Endless {
                line: b"name,other\n".to_vec(),
                offset: 0,
                served: std::rc::Rc::clone(&served),
            };

            let out: Vec<_> = rows(std::io::BufReader::new(source), format)
                .take(3)
                .collect();

            assert_eq!(out.len(), 3, "{format:?} should keep yielding");
            // One `BufReader` fill is 8 KiB; the csv reader has a buffer of its
            // own. A megabyte is far above either and far below any file worth
            // sending, so this fails loudly for a buffering implementation and
            // never for a streaming one.
            assert!(
                served.get() < 1_000_000,
                "{format:?} consumed {} bytes to read three rows",
                served.get()
            );
        }
    }
}
