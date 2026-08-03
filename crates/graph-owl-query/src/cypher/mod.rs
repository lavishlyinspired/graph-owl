//! Cypher over the same engine — Epic 7b.
//!
//! **One query engine, two front ends.** Cypher is parsed by `decypher`, gated
//! to the documented subset here, and lowered onto the same `spargebra` algebra
//! the SPARQL front end produces. Two evaluators would mean two authorization
//! paths, two optimizers and two sets of correctness bugs — and the
//! authorization path is compiled into the plan, so the second one would be the
//! leak.
//!
//! **`decypher`'s AST reaches exactly one boundary: this module.** Its README
//! says that AST is unstable until 0.2.0, which is acceptable only because
//! nothing downstream sees it — lowering goes `decypher` → [`subset`] →
//! `spargebra`. If the crate breaks or is abandoned, this module is the blast
//! radius. The spike that chose it is recorded in `plans/00l-build-vs-adopt.md`.

pub mod subset;

pub use subset::Refusal;

/// Why a Cypher request did not produce a plan.
///
/// **Two kinds, kept apart.** A query the author wrote wrongly is theirs to fix
/// and reports a position; a query that is correct Cypher but outside what this
/// engine serves is ours to explain, and reports where to go instead. Collapsing
/// them sends somebody hunting for a typo that is not there.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CypherError {
    #[error("{message}")]
    Syntax {
        message: String,
        /// 1-based, as an editor counts.
        line: usize,
        column: usize,
    },
    #[error(transparent)]
    Refused(#[from] Refusal),
}

/// Parse a Cypher query and admit it only if it is inside the subset.
///
/// # Errors
///
/// [`CypherError::Syntax`] with a line and column for malformed input;
/// [`CypherError::Refused`] naming the construct and the API to use instead for
/// valid Cypher this engine does not serve.
pub fn parse_subset(query: &str) -> Result<decypher::ast::query::Query, CypherError> {
    // **The gate runs on the lossless CST, before the typed AST exists.** See
    // `subset`: `decypher` 0.2.0-alpha.6 drops `CALL … YIELD …` on the way to
    // its AST, so a gate reading the AST would admit a query it never saw.
    let cst = decypher::parse_cst(query);
    if let Some(error) = cst.errors.first() {
        return Err(syntax_error(query, error));
    }
    subset::admit(&cst.tree)?;

    // Only now — nothing forbidden can still be present.
    decypher::parse(query).map_err(|error| syntax_error(query, &error))
}

/// Turn a parse failure into a positioned message.
///
/// **Line and column, not a byte offset.** Slice A requires it, and the reason
/// is that an offset is useless to a human and to an editor gutter alike —
/// every caller would otherwise write this conversion, and the ones that forgot
/// would report "error at 247".
fn syntax_error(source: &str, error: &decypher::CypherError) -> CypherError {
    let offset = error.span().start;
    let (line, column) = line_and_column(source, offset);
    CypherError::Syntax {
        message: error.to_string(),
        line,
        column,
    }
}

/// A byte offset as 1-based line and column.
///
/// Counts **characters** rather than bytes for the column, so a query
/// containing a non-ASCII label reports a column an editor agrees with rather
/// than one inflated by UTF-8 continuation bytes.
fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, last)| last)
        .chars()
        .count()
        + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_in_the_subset_parses() {
        let parsed = parse_subset("MATCH (n:Person) WHERE n.age > 21 RETURN n.name");

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    /// **A write is `Refused`, not `Syntax`.** It is correct Cypher, and calling
    /// it a syntax error sends the author looking for a typo that is not there.
    #[test]
    fn a_write_is_refused_rather_than_reported_as_a_syntax_error() {
        let outcome = parse_subset("CREATE (n:Person) RETURN n");

        assert!(
            matches!(outcome, Err(CypherError::Refused(_))),
            "{outcome:?}"
        );
    }

    #[test]
    fn malformed_input_reports_a_line_and_column() {
        let outcome = parse_subset("MATCH (n:Person RETURN n");

        let Err(CypherError::Syntax { line, column, .. }) = outcome else {
            panic!("expected a positioned syntax error, got {outcome:?}");
        };
        assert_eq!(line, 1, "single-line query");
        assert!(column > 1, "the error is not at the very start: {column}");
    }

    /// A multi-line query reports the line the error is actually on — the case
    /// a byte offset makes unreadable.
    #[test]
    fn a_multi_line_query_reports_the_offending_line() {
        let outcome = parse_subset("MATCH (n)\nWHERE\nRETURN n");

        let Err(CypherError::Syntax { line, .. }) = outcome else {
            panic!("expected a syntax error, got {outcome:?}");
        };
        assert!(line >= 2, "the error is past the first line: line {line}");
    }

    #[test]
    fn line_and_column_are_one_based_and_count_characters() {
        assert_eq!(line_and_column("abc", 0), (1, 1));
        assert_eq!(line_and_column("abc", 2), (1, 3));
        assert_eq!(line_and_column("a\nbc", 2), (2, 1));
        assert_eq!(line_and_column("a\nbc", 4), (2, 3));
        // Two-byte characters must not inflate the column.
        assert_eq!(line_and_column("é é", 3), (1, 3));
        // Past the end clamps rather than panicking on a bad span.
        assert_eq!(line_and_column("ab", 99), (1, 3));
    }
}
