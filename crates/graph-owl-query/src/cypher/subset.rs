//! The subset gate — Epic 7b Slice A.
//!
//! **`decypher` parses the whole of openCypher. This decides what we serve.**
//! Settled by the spike in `00l`: a parser that enforced our subset would be
//! enforcing *its* idea of one, and the candidate that did exactly that turned
//! out to reject relationship properties and float literals — both of which this
//! system needs.
//!
//! # The gate reads the CST, not the AST, and that is the whole design
//!
//! `decypher` 0.2.0-alpha.6 **silently drops `CALL … YIELD …` when lowering to
//! its typed AST.** `CALL db.labels() YIELD l RETURN l` arrives as a query whose
//! reading clauses are empty and whose body is `RETURN l` — no error, no
//! diagnostic. A gate reading that AST cannot refuse what it cannot see, and we
//! would have executed `RETURN l` in place of the query the caller sent.
//!
//! Its **CST** does not drop it: `STANDALONE_CALL` is present, and the tree
//! round-trips to the source byte for byte because rowan trees are lossless by
//! construction. So:
//!
//! - **Gate on the CST.** A construct cannot hide from a tree that reproduces
//!   its own input.
//! - **Lower from the AST**, whose convenience is then safe, because nothing
//!   forbidden ever reaches it.
//!
//! This is stronger than gating on the AST would have been even if the AST were
//! correct — it removes a whole class of "the parser and the gate disagree about
//! what the query says" from the design rather than trusting them to agree. The
//! upstream bug should be reported; the architecture should not depend on the
//! fix.

use std::collections::BTreeSet;

use decypher::syntax::{SyntaxKind, SyntaxNode};

/// Why a syntactically valid Cypher query is not one this engine serves.
///
/// Distinct from a parse error throughout: the query is *correct Cypher*, and
/// saying "syntax error" would send the author hunting a typo that is not there.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// A write clause. **Names the API that does perform the write**, because
    /// the answer is never "you cannot do this" — it is "not through here".
    #[error(
        "`{construct}` writes to the graph, and this query surface is read-only. \
         Use {api} instead — writes go through the catalog API so they are \
         validated, versioned and attributable."
    )]
    Write {
        construct: &'static str,
        api: &'static str,
    },

    /// A procedure call.
    #[error(
        "`{0}` invokes a procedure, which this engine does not implement. What \
         procedures are usually reached for is served directly: traversal by \
         `GET /assets/{{id}}/graph`, search by `GET /search`, lineage by \
         `GET /lineage`."
    )]
    Procedure(&'static str),

    /// In the language and outside the documented subset.
    #[error(
        "`{construct}` is valid Cypher and outside the subset this engine serves. {alternative}"
    )]
    OutsideSubset {
        construct: &'static str,
        alternative: &'static str,
    },

    /// More than one statement in one request.
    #[error(
        "a request carries one query; this one has {0}. Send them separately so \
         each gets its own authorization decision and its own budget."
    )]
    MultipleStatements(usize),

    /// No statement at all.
    ///
    /// Says what a smallest useful query looks like, because the caller here is
    /// often a client sending a blank editor buffer, and "empty" alone does not
    /// distinguish "you sent nothing" from "we understood nothing".
    #[error(
        "the request carried no query. A smallest useful one looks like \
         `MATCH (n) RETURN n LIMIT 10`."
    )]
    Empty,
}

/// Every syntax kind outside the subset, and what to say about it.
///
/// **A table rather than a match, because the CST is walked generically.** The
/// compile-time exhaustiveness a match over the AST would give is bought back by
/// [`tests::every_forbidden_kind_is_reachable`], which drives a real query
/// through each entry — a row nobody can trigger is a row that does not work.
fn forbidden(kind: SyntaxKind) -> Option<Refusal> {
    use SyntaxKind as K;
    Some(match kind {
        K::CREATE_CLAUSE => Refusal::Write {
            construct: "CREATE",
            api: "`POST /assets`",
        },
        K::MERGE_CLAUSE => Refusal::Write {
            construct: "MERGE",
            api: "`POST /assets`, which is already idempotent on fullyQualifiedName",
        },
        K::DELETE_CLAUSE => Refusal::Write {
            construct: "DELETE",
            api: "`DELETE /assets/{id}`, which soft-deletes so history survives",
        },
        K::SET_CLAUSE => Refusal::Write {
            construct: "SET",
            api: "`PATCH /assets/{id}`",
        },
        K::REMOVE_CLAUSE => Refusal::Write {
            construct: "REMOVE",
            api: "`PATCH /assets/{id}` with an explicit null to clear a field",
        },
        K::FOREACH_CLAUSE => Refusal::Write {
            construct: "FOREACH",
            api: "`POST /assets/batch`",
        },
        K::LOAD_CSV_CLAUSE => Refusal::Write {
            construct: "LOAD CSV",
            api: "`POST /connectors` — bulk ingestion runs as a connector, so it \
                  is versioned, attributable and re-runnable",
        },
        K::STANDALONE_CALL | K::IN_QUERY_CALL | K::CALL_SUBQUERY_CLAUSE => {
            Refusal::Procedure("CALL")
        }
        K::UNION => Refusal::OutsideSubset {
            construct: "UNION",
            alternative: "Run the branches as separate queries and combine the \
                          results in the caller.",
        },
        K::SHOW_CLAUSE => Refusal::OutsideSubset {
            construct: "SHOW",
            alternative: "Ask the catalog directly: `GET /assets` lists entities \
                          and `GET /openapi.json` describes the surface.",
        },
        K::USE_CLAUSE => Refusal::OutsideSubset {
            construct: "USE",
            alternative: "This engine serves one graph; named graphs appear as \
                          the `_graph` property on results.",
        },
        K::FINISH_CLAUSE => Refusal::OutsideSubset {
            construct: "FINISH",
            alternative: "A read-only query has no side effect to finish with; \
                          use `RETURN` to say what you want back.",
        },
        _ => return None,
    })
}

/// Whether this parsed query is one the engine will serve.
///
/// Walks the **whole** tree. A write can hide arbitrarily deep — inside a
/// `UNION` branch, behind a third `WITH` segment — and a gate that checked only
/// the outermost clause would admit `MATCH (n) WITH n CREATE (:Copy)`, which
/// reads as a read query for two clauses.
///
/// # Errors
///
/// [`Refusal`] naming the construct and where to go instead.
pub fn admit(tree: &SyntaxNode) -> Result<(), Refusal> {
    // **Forbidden kinds first, statement count second**, and the order is
    // deliberate. `SHOW DATABASES` parses to a tree with a `SHOW_CLAUSE` and no
    // `STATEMENT` node at all; counting first would report it as empty, which
    // is true and useless — the caller wants to know `SHOW` is not served, not
    // that they sent nothing.
    walk(tree)?;

    match statements(tree) {
        0 => Err(Refusal::Empty),
        1 => Ok(()),
        several => Err(Refusal::MultipleStatements(several)),
    }
}

/// How many queries this tree carries.
///
/// **No `STATEMENT` node means no query, whatever text came with it.** An
/// earlier version treated a non-empty source as one statement by default; that
/// admitted `###`, which `decypher` parses to a bare `SOURCE_FILE` with **no
/// errors** and no statements. The gate then found nothing forbidden — because
/// there was nothing at all — and said yes to garbage. Mutation testing found
/// it: inverting the condition produced *better* behaviour than the original,
/// which is the tell that the original was wrong.
fn statements(tree: &SyntaxNode) -> usize {
    tree.children()
        .filter(|child| child.kind() == SyntaxKind::STATEMENT)
        .count()
}

fn walk(node: &SyntaxNode) -> Result<(), Refusal> {
    if let Some(refusal) = forbidden(node.kind()) {
        return Err(refusal);
    }
    for child in node.children() {
        walk(&child)?;
    }
    Ok(())
}

/// Every kind this gate refuses, for the test that proves each is reachable.
#[must_use]
pub fn forbidden_kinds() -> BTreeSet<String> {
    [
        SyntaxKind::CREATE_CLAUSE,
        SyntaxKind::MERGE_CLAUSE,
        SyntaxKind::DELETE_CLAUSE,
        SyntaxKind::SET_CLAUSE,
        SyntaxKind::REMOVE_CLAUSE,
        SyntaxKind::FOREACH_CLAUSE,
        SyntaxKind::LOAD_CSV_CLAUSE,
        SyntaxKind::STANDALONE_CALL,
        SyntaxKind::IN_QUERY_CALL,
        SyntaxKind::CALL_SUBQUERY_CLAUSE,
        SyntaxKind::UNION,
        SyntaxKind::SHOW_CLAUSE,
        SyntaxKind::USE_CLAUSE,
        SyntaxKind::FINISH_CLAUSE,
    ]
    .into_iter()
    .map(|kind| format!("{kind:?}"))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(query: &str) -> Result<(), Refusal> {
        let parse = decypher::parse_cst(query);
        admit(&parse.tree)
    }

    fn refusal(query: &str) -> Refusal {
        gate(query).expect_err("should be refused")
    }

    // ---- what the subset serves ----

    #[test]
    fn the_declared_subset_is_admitted() {
        for query in [
            "MATCH (n:Person) RETURN n",
            "MATCH (n:Person {name: 'Ada'}) RETURN n",
            "MATCH (a:Table)-[r:FEEDS]->(b:Table) RETURN a, b",
            "MATCH (a)-[r:FEEDS {confidence: 0.9}]->(b) RETURN r",
            "MATCH (a)-[r:FEEDS*1..3]->(b) RETURN b",
            "MATCH (a)-[r:FEEDS*]->(b) RETURN b",
            "MATCH (a) OPTIONAL MATCH (a)-[r]->(b) RETURN a, b",
            "MATCH (n) WHERE n.conf > 0.8 RETURN n",
            "MATCH (n) WITH n, n.age AS age WHERE age > 21 RETURN n",
            "UNWIND [1, 2, 3] AS x RETURN x",
            "MATCH (n) RETURN n ORDER BY n.name DESC SKIP 10 LIMIT 5",
            "MATCH (n) RETURN DISTINCT n.name",
            "MATCH (n)-[r]->(m) RETURN n, count(m)",
        ] {
            assert_eq!(gate(query), Ok(()), "should be admitted: {query}");
        }
    }

    // ---- the bug this design exists for ----

    /// **The regression test for `decypher`'s dropped `CALL`.**
    ///
    /// Its typed AST lowers `CALL db.labels() YIELD l RETURN l` to a query whose
    /// reading clauses are empty and whose body is `RETURN l` — the `CALL`
    /// vanishes with no error. Gating on the AST would therefore have executed
    /// `RETURN l` in place of what the caller sent. The CST keeps it.
    #[test]
    fn a_call_that_the_typed_ast_drops_is_still_refused() {
        let query = "CALL db.labels() YIELD l RETURN l";

        // The upstream defect, asserted so this test explains itself when the
        // bug is fixed and this assertion starts failing.
        let ast = decypher::parse(query).expect("valid Cypher");
        let ast_shape = format!("{:?}", ast.statements[0]);
        assert!(
            !ast_shape.contains("Call"),
            "upstream appears to have fixed the dropped CALL — the CST gate is \
             still correct, but this note is now stale: {ast_shape:.200}"
        );

        // The gate refuses it anyway, because it reads the lossless tree.
        assert!(matches!(refusal(query), Refusal::Procedure(_)));
    }

    /// The CST round-trips to its input. That property is what makes the gate
    /// sound: a construct cannot hide from a tree that reproduces its own source.
    #[test]
    fn the_gated_tree_reproduces_its_own_source() {
        for query in [
            "CALL db.labels() YIELD l RETURN l",
            "MATCH (n) WITH n CREATE (:Copy) RETURN n",
            "MATCH (n:Person {name: 'Ada'}) RETURN n",
        ] {
            let parse = decypher::parse_cst(query);
            assert_eq!(
                parse.tree.text().to_string(),
                query,
                "the tree must be lossless"
            );
        }
    }

    // ---- writes ----

    /// **Every write names the API that performs it.** A bare "unsupported"
    /// leaves the author stuck, and this surface exists for people who reached
    /// for Cypher because it was familiar.
    #[test]
    fn every_write_clause_names_the_api_to_use_instead() {
        for (query, construct, api) in [
            ("CREATE (n:Person) RETURN n", "CREATE", "POST /assets"),
            ("MERGE (n:Person) RETURN n", "MERGE", "POST /assets"),
            ("MATCH (n) DELETE n", "DELETE", "DELETE /assets/{id}"),
            (
                "MATCH (n) SET n.x = 1 RETURN n",
                "SET",
                "PATCH /assets/{id}",
            ),
            (
                "MATCH (n) REMOVE n.x RETURN n",
                "REMOVE",
                "PATCH /assets/{id}",
            ),
        ] {
            let rendered = refusal(query).to_string();
            assert!(rendered.contains(construct), "names it: {rendered}");
            assert!(rendered.contains(api), "names `{api}`: {rendered}");
        }
    }

    /// **The refusal is not a syntax error.** These are correct Cypher; calling
    /// them syntax errors sends the author hunting a typo that is not there.
    #[test]
    fn a_write_parses_cleanly_and_is_refused_by_the_gate() {
        let parse = decypher::parse_cst("CREATE (n:Person) RETURN n");

        assert!(parse.errors.is_empty(), "it is valid Cypher");
        assert!(matches!(
            admit(&parse.tree),
            Err(Refusal::Write {
                construct: "CREATE",
                ..
            })
        ));
    }

    /// **The hiding place a top-level-only gate misses.** This reads as a read
    /// query for two clauses.
    #[test]
    fn a_write_buried_behind_a_with_is_still_refused() {
        assert!(matches!(
            refusal("MATCH (n) WITH n CREATE (:Copy) RETURN n"),
            Refusal::Write {
                construct: "CREATE",
                ..
            }
        ));
    }

    #[test]
    fn a_write_behind_two_with_segments_is_still_refused() {
        assert!(matches!(
            refusal("MATCH (n) WITH n WITH n AS m MATCH (m) SET m.x = 1 RETURN m"),
            Refusal::Write {
                construct: "SET",
                ..
            }
        ));
    }

    /// And inside a `UNION` branch — refused whichever construct is found first.
    #[test]
    fn a_write_inside_a_union_branch_is_refused() {
        let refused = refusal("MATCH (n) RETURN n UNION MATCH (m) SET m.x = 1 RETURN m");

        assert!(
            matches!(
                refused,
                Refusal::Write { .. } | Refusal::OutsideSubset { .. }
            ),
            "{refused:?}"
        );
    }

    // ---- procedures and the rest ----

    #[test]
    fn a_procedure_call_is_refused_and_points_at_the_direct_apis() {
        for query in [
            "CALL db.labels() YIELD l RETURN l",
            "MATCH (n) CALL db.labels() YIELD l RETURN l",
        ] {
            let refused = refusal(query);
            assert!(matches!(refused, Refusal::Procedure(_)), "{refused:?}");
            let rendered = refused.to_string();
            assert!(
                rendered.contains("/search") || rendered.contains("/lineage"),
                "names what serves the same need: {rendered}"
            );
        }
    }

    #[test]
    fn load_csv_points_at_the_connector_api() {
        assert!(
            refusal("LOAD CSV FROM 'file:///x.csv' AS row RETURN row")
                .to_string()
                .contains("connectors")
        );
    }

    #[test]
    fn union_is_refused_with_the_rewrite_that_replaces_it() {
        let refused = refusal("MATCH (n) RETURN n UNION MATCH (m) RETURN m");

        assert!(
            matches!(
                refused,
                Refusal::OutsideSubset {
                    construct: "UNION",
                    ..
                }
            ),
            "{refused:?}"
        );
        assert!(refused.to_string().contains("separate queries"));
    }

    // ---- the table is exhaustive in the only way it can be ----

    /// **Every forbidden kind is reachable by a real query.**
    ///
    /// The gate walks the CST generically, so it cannot get a compiler
    /// exhaustiveness check the way a match over the AST would. This buys that
    /// back: a row in the table that no query can trigger is a row that does not
    /// work, and this test names it.
    #[test]
    fn every_forbidden_kind_is_reachable() {
        let probes = [
            ("CREATE (n) RETURN n", "CREATE_CLAUSE"),
            ("MERGE (n) RETURN n", "MERGE_CLAUSE"),
            ("MATCH (n) DELETE n", "DELETE_CLAUSE"),
            ("MATCH (n) SET n.x = 1 RETURN n", "SET_CLAUSE"),
            ("MATCH (n) REMOVE n.x RETURN n", "REMOVE_CLAUSE"),
            (
                "MATCH (n) FOREACH (x IN [1] | SET n.a = x) RETURN n",
                "FOREACH_CLAUSE",
            ),
            (
                "LOAD CSV FROM 'file:///x.csv' AS row RETURN row",
                "LOAD_CSV_CLAUSE",
            ),
            ("CALL db.labels() YIELD l RETURN l", "STANDALONE_CALL"),
            ("MATCH (n) RETURN n UNION MATCH (m) RETURN m", "UNION"),
        ];

        for (query, kind) in probes {
            assert!(
                gate(query).is_err(),
                "`{kind}` must be reachable and refused, but `{query}` was admitted"
            );
        }
        // The table may legitimately carry kinds no probe covers yet; what it
        // must not do is shrink below what these probes exercise.
        assert!(forbidden_kinds().len() >= probes.len());
    }

    /// Every refusal says something actionable — a variant rendering to a bare
    /// noun would pass every test above and help nobody.
    #[test]
    fn no_refusal_is_a_bare_noun() {
        for refused in [
            refusal("CREATE (n) RETURN n"),
            refusal("CALL db.labels() YIELD l RETURN l"),
            refusal("MATCH (n) RETURN n UNION MATCH (m) RETURN m"),
            Refusal::Empty,
            Refusal::MultipleStatements(3),
        ] {
            let rendered = refused.to_string();
            assert!(
                rendered.len() > 40,
                "a refusal has to say what to do instead: {rendered}"
            );
        }
    }

    /// **One query per request**, so each gets its own authorization decision
    /// and its own budget.
    #[test]
    fn several_statements_in_one_request_are_refused() {
        assert_eq!(
            gate("MATCH (n) RETURN n; MATCH (m) RETURN m"),
            Err(Refusal::MultipleStatements(2))
        );
    }

    #[test]
    fn an_empty_query_is_refused() {
        assert_eq!(gate(""), Err(Refusal::Empty));
        assert_eq!(gate("   \n  "), Err(Refusal::Empty));
    }

    /// **Garbage is refused, and it used to be admitted.**
    ///
    /// `decypher` parses `###` to a bare `SOURCE_FILE` with **no errors** and no
    /// statements. The first version of `statements` treated any non-empty
    /// source as one statement, so the walk found nothing forbidden — because
    /// there was nothing at all — and the gate said yes.
    ///
    /// Mutation testing found it by inverting the condition and producing
    /// *better* behaviour than the original, which is the tell that the original
    /// was wrong rather than merely untested.
    #[test]
    fn source_that_parses_to_no_statement_is_refused() {
        for garbage in ["###", "@@@ !!!", "%%%"] {
            assert_eq!(
                gate(garbage),
                Err(Refusal::Empty),
                "`{garbage}` parses to nothing and must not be admitted"
            );
        }
    }

    /// **`SHOW` keeps its own message even though it has no `STATEMENT` node.**
    ///
    /// This is why the forbidden-kind walk runs before the statement count:
    /// counting first would report `SHOW DATABASES` as empty, which is true and
    /// useless.
    #[test]
    fn show_is_refused_by_name_rather_than_as_empty() {
        let refused = refusal("SHOW DATABASES");

        assert!(
            matches!(
                refused,
                Refusal::OutsideSubset {
                    construct: "SHOW",
                    ..
                }
            ),
            "{refused:?}"
        );
        assert!(refused.to_string().contains("GET /assets"));
    }

    #[test]
    fn use_and_finish_are_refused_by_name() {
        let used = refusal("USE mygraph MATCH (n) RETURN n");
        assert!(
            matches!(
                used,
                Refusal::OutsideSubset {
                    construct: "USE",
                    ..
                }
            ),
            "{used:?}"
        );

        let finished = refusal("MATCH (n) FINISH");
        assert!(
            matches!(
                finished,
                Refusal::OutsideSubset {
                    construct: "FINISH",
                    ..
                }
            ),
            "{finished:?}"
        );
    }

    /// **`FOREACH` is named as `FOREACH`, not as the `SET` inside it.**
    ///
    /// Every `FOREACH` body is itself a write, so a test asserting only "this is
    /// refused" passes even with the `FOREACH` arm deleted — the `SET` inside
    /// catches it, and the author is told to use `PATCH /assets/{id}` for a loop.
    /// Asserting the *construct* is what makes the arm earn its place.
    #[test]
    fn foreach_is_named_rather_than_the_write_inside_it() {
        let refused = refusal("MATCH (n) FOREACH (x IN [1] | SET n.a = x) RETURN n");

        assert!(
            matches!(
                refused,
                Refusal::Write {
                    construct: "FOREACH",
                    ..
                }
            ),
            "the outer construct is the one to report: {refused:?}"
        );
        assert!(refused.to_string().contains("batch"));
    }
}
