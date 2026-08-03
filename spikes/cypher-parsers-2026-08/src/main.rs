//! Epic 7b Slice A spike: one corpus, every candidate, same assertions.
//!
//! Judged in this order, per `00l`: auditability (settled before this ran),
//! subset coverage, refusal behaviour, diagnostics, AST usability.

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    /// Inside Epic 7b's declared subset — must parse.
    InSubset,
    /// Outside it — must be *refused*, never partially accepted.
    OutOfSubset,
    /// Malformed — must be refused, ideally with a position.
    Malformed,
}
use Expect::{InSubset, Malformed, OutOfSubset};

const CORPUS: &[(&str, Expect, &str)] = &[
    ("match-basic", InSubset, "MATCH (n:Person) RETURN n"),
    ("match-props", InSubset, "MATCH (n:Person {name: 'Ada'}) RETURN n"),
    ("rel-typed", InSubset, "MATCH (a:Table)-[r:FEEDS]->(b:Table) RETURN a, b"),
    ("rel-props", InSubset, "MATCH (a)-[r:FEEDS {confidence: 0.9}]->(b) RETURN r"),
    ("rel-undirected", InSubset, "MATCH (a)-[r]-(b) RETURN a"),
    ("varlen-range", InSubset, "MATCH (a)-[r:FEEDS*1..3]->(b) RETURN b"),
    ("varlen-open", InSubset, "MATCH (a)-[r:FEEDS*]->(b) RETURN b"),
    ("varlen-upper", InSubset, "MATCH (a)-[r:FEEDS*..5]->(b) RETURN b"),
    ("optional-match", InSubset, "MATCH (a) OPTIONAL MATCH (a)-[r]->(b) RETURN a, b"),
    ("where", InSubset, "MATCH (n:Person) WHERE n.age > 21 RETURN n.name"),
    ("where-and-or", InSubset, "MATCH (n) WHERE n.a > 1 AND (n.b < 2 OR n.c = 3) RETURN n"),
    ("with", InSubset, "MATCH (n) WITH n, n.age AS age WHERE age > 21 RETURN n"),
    ("unwind", InSubset, "UNWIND [1, 2, 3] AS x RETURN x"),
    ("order-by", InSubset, "MATCH (n) RETURN n ORDER BY n.name"),
    ("order-desc", InSubset, "MATCH (n) RETURN n ORDER BY n.name DESC"),
    ("skip-limit", InSubset, "MATCH (n) RETURN n SKIP 10 LIMIT 5"),
    ("distinct", InSubset, "MATCH (n) RETURN DISTINCT n.name"),
    ("agg-count", InSubset, "MATCH (n)-[r]->(m) RETURN n, count(m)"),
    ("agg-count-distinct", InSubset, "MATCH (n)-[r]->(m) RETURN count(DISTINCT m)"),
    ("agg-collect", InSubset, "MATCH (n) RETURN collect(n.name)"),
    ("agg-mixed", InSubset, "MATCH (n) RETURN n.kind, sum(n.rows), avg(n.rows)"),
    ("plan-example", InSubset,
     "MATCH (a:Table)-[r:FEEDS*1..3]->(b) WHERE r.confidence > 0.8 \
      RETURN DISTINCT a, count(b) ORDER BY count(b) DESC LIMIT 10"),
    // Out of subset: must be refused, not partially parsed.
    ("write-create", OutOfSubset, "CREATE (n:Person {name: 'Ada'}) RETURN n"),
    ("write-merge", OutOfSubset, "MERGE (n:Person {name: 'Ada'}) RETURN n"),
    ("write-delete", OutOfSubset, "MATCH (n) DELETE n"),
    ("write-set", OutOfSubset, "MATCH (n) SET n.x = 1 RETURN n"),
    ("call-proc", OutOfSubset, "CALL db.labels() YIELD label RETURN label"),
    ("foreach", OutOfSubset, "MATCH (n) FOREACH (x IN [1] | SET n.a = x) RETURN n"),
    // Malformed: must be refused, ideally with a position.
    ("bad-unclosed", Malformed, "MATCH (n:Person RETURN n"),
    ("bad-garbage", Malformed, "MATCH ###"),
    ("bad-empty", Malformed, ""),
    ("bad-half", Malformed, "MATCH (n) WHERE RETURN n"),
];

#[derive(Default)]
struct Tally {
    in_subset_ok: usize,
    in_subset_total: usize,
    out_refused: usize,
    out_total: usize,
    malformed_refused: usize,
    malformed_total: usize,
    with_position: usize,
}

impl Tally {
    fn record(&mut self, expect: Expect, accepted: bool, positioned: bool) {
        match expect {
            InSubset => {
                self.in_subset_total += 1;
                if accepted {
                    self.in_subset_ok += 1;
                }
            }
            OutOfSubset => {
                self.out_total += 1;
                if !accepted {
                    self.out_refused += 1;
                }
            }
            Malformed => {
                self.malformed_total += 1;
                if !accepted {
                    self.malformed_refused += 1;
                    if positioned {
                        self.with_position += 1;
                    }
                }
            }
        }
    }

    fn report(&self, name: &str) {
        println!(
            "\n{name}\n  in-subset parsed : {}/{}\n  out-of-subset refused: {}/{}\n  \
             malformed refused: {}/{}  (with a position: {})",
            self.in_subset_ok,
            self.in_subset_total,
            self.out_refused,
            self.out_total,
            self.malformed_refused,
            self.malformed_total,
            self.with_position
        );
    }
}

fn run_cypher_parser() {
    let mut tally = Tally::default();
    println!("\n================ cypher-parser 0.8.1 ================");
    for (name, expect, query) in CORPUS {
        match cypher_parser::parse(query) {
            Ok(_) => {
                tally.record(*expect, true, false);
                if *expect != InSubset {
                    println!("  ACCEPTED (should refuse): {name}");
                }
            }
            Err(error) => {
                let positioned =
                    matches!(error, cypher_parser::CypherError::Syntax { .. });
                tally.record(*expect, false, positioned);
                if *expect == InSubset {
                    println!("  REFUSED (should parse): {name} -> {error}");
                }
            }
        }
    }
    tally.report("cypher-parser");
}

fn run_decypher() {
    let mut tally = Tally::default();
    println!("\n================ decypher 0.2.0-alpha.6 ================");
    for (name, expect, query) in CORPUS {
        match decypher::parse(*query) {
            Ok(_) => {
                tally.record(*expect, true, false);
                if *expect != InSubset {
                    println!("  ACCEPTED (should refuse): {name}");
                }
            }
            Err(error) => {
                let rendered = format!("{error}");
                // Any span/line information counts as positioned.
                let positioned = rendered.contains(':') || rendered.contains("at ");
                tally.record(*expect, false, positioned);
                if *expect == InSubset {
                    println!("  REFUSED (should parse): {name} -> {rendered}");
                }
            }
        }
    }
    tally.report("decypher");
}

/// Whether the tree carries a locatable error. tree-sitter reports row and
/// column on every node, so this is about *finding* the error node rather than
/// about whether position information exists.
fn positioned_error(tree: &tree_sitter::Tree) -> bool {
    fn walk(node: tree_sitter::Node, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        if node.is_error() || node.is_missing() {
            return true;
        }
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|child| walk(child, depth + 1))
    }
    walk(tree.root_node(), 0)
}

fn run_tree_sitter() {
    let mut tally = Tally::default();
    println!("\n================ tree-sitter-cypher 0.2.6 ================");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cypher::LANGUAGE.into())
        .expect("language loads");

    for (name, expect, query) in CORPUS {
        let tree = parser.parse(query, None);
        // **The crux for the engine path.** tree-sitter always returns a tree;
        // "did it parse" means "does the tree contain no ERROR or MISSING node",
        // which the caller must check by walking. A parser that recovers rather
        // than refusing makes the subset decision by omission.
        let accepted = tree.as_ref().is_some_and(|tree| {
            let root = tree.root_node();
            !root.has_error() && root.child_count() > 0
        });
        // **Corrected after the first run.** This originally passed `accepted`
        // as the `positioned` flag, so a refusal always recorded "no position"
        // and the report libelled tree-sitter as giving none. It gives row and
        // column on every `ERROR`/`MISSING` node — see `positioned_error`.
        let positioned = tree.as_ref().is_some_and(positioned_error);
        tally.record(*expect, accepted, positioned);
        if *expect == InSubset && !accepted {
            println!("  REFUSED (should parse): {name}");
        }
        if *expect != InSubset && accepted {
            println!("  ACCEPTED (should refuse): {name}");
        }
    }
    tally.report("tree-sitter-cypher");
}

fn main() {
    println!("Epic 7b Slice A spike — {} corpus entries", CORPUS.len());
    run_cypher_parser();
    run_decypher();
    run_tree_sitter();
}
