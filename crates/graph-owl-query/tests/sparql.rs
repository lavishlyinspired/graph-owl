//! Real SPARQL over real flakes.
//!
//! These assert the thing the adopt-the-evaluator decision rests on: that
//! `spareval` evaluating over our `FlakeDataset` answers correctly, and that a
//! caller who filtered the fact set gets a filtered answer — which is how
//! authorization and time travel survive living outside the evaluator.

use graph_owl_core::flake::{Flake, FlakeValue, Sid};
use graph_owl_query::dataset::FlakeDataset;
use spareval::{QueryEvaluator, QueryResults};
use spargebra::SparqlParser;

const DSC: &str = "https://graph-owl.dev/ns/catalog#";

fn flake(s: &str, p: &str, o: FlakeValue) -> Flake {
    Flake::assert(Sid::dsc(s), Sid::dsc(p), o, 1)
}

fn estate() -> Vec<Flake> {
    vec![
        flake("upi", "name", FlakeValue::String("upi_transactions".into())),
        flake("upi", "type", FlakeValue::String("table".into())),
        flake("upi", "parentSchema", FlakeValue::Ref(Sid::dsc("payments"))),
        flake(
            "neft",
            "name",
            FlakeValue::String("neft_transactions".into()),
        ),
        flake("neft", "type", FlakeValue::String("table".into())),
        flake(
            "neft",
            "parentSchema",
            FlakeValue::Ref(Sid::dsc("payments")),
        ),
        flake("cust", "name", FlakeValue::String("customers".into())),
        flake("cust", "type", FlakeValue::String("table".into())),
        flake(
            "cust",
            "parentSchema",
            FlakeValue::Ref(Sid::dsc("core_banking")),
        ),
        flake("payments", "name", FlakeValue::String("payments".into())),
        flake(
            "core_banking",
            "name",
            FlakeValue::String("core_banking".into()),
        ),
        flake("amount", "ordinalPosition", FlakeValue::Int(3)),
        flake("txn_id", "ordinalPosition", FlakeValue::Int(1)),
    ]
}

fn solutions(flakes: &[Flake], sparql: &str) -> Vec<Vec<(String, String)>> {
    let dataset = FlakeDataset::from_flakes(flakes).expect("dataset");
    let query = SparqlParser::new()
        .parse_query(sparql)
        .expect("the query should parse");
    let results = QueryEvaluator::new()
        .prepare(&query)
        .execute(&dataset)
        .expect("the query should evaluate");

    match results {
        QueryResults::Solutions(iter) => iter
            .map(|solution| {
                let solution = solution.expect("solution");
                solution
                    .iter()
                    .map(|(var, term)| (var.as_str().to_string(), term.to_string()))
                    .collect()
            })
            .collect(),
        _ => panic!("a SELECT must yield solutions"),
    }
}

fn values(rows: &[Vec<(String, String)>], var: &str) -> Vec<String> {
    let mut out: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            row.iter()
                .find(|(name, _)| name == var)
                .map(|(_, value)| value.trim_matches('"').to_string())
        })
        .collect();
    out.sort();
    out
}

/// The simplest thing that proves the whole stack: parse, plan, evaluate over
/// flakes, return bindings.
#[test]
fn a_basic_graph_pattern_returns_bindings() {
    let rows = solutions(
        &estate(),
        &format!("SELECT ?name WHERE {{ ?t <{DSC}type> \"table\" . ?t <{DSC}name> ?name }}"),
    );
    assert_eq!(
        values(&rows, "name"),
        vec!["customers", "neft_transactions", "upi_transactions"]
    );
}

/// **The query the REST surface cannot express**, and the reason Epic 7 exists:
/// a join across two hops in one request.
#[test]
fn a_two_hop_join_answers_what_rest_cannot() {
    let rows = solutions(
        &estate(),
        &format!(
            "SELECT ?table WHERE {{
               ?t <{DSC}parentSchema> ?s .
               ?s <{DSC}name> \"payments\" .
               ?t <{DSC}name> ?table
             }}"
        ),
    );
    assert_eq!(
        values(&rows, "table"),
        vec!["neft_transactions", "upi_transactions"],
        "customers is in core_banking and must not appear"
    );
}

/// Datatypes have to survive the term mapping or numeric comparison silently
/// becomes string comparison — where `"10" > "5"` is false.
#[test]
fn a_numeric_filter_compares_numerically() {
    let rows = solutions(
        &estate(),
        &format!("SELECT ?c WHERE {{ ?c <{DSC}ordinalPosition> ?n . FILTER(?n > 2) }}"),
    );
    assert_eq!(rows.len(), 1, "only ordinalPosition 3 is above 2: {rows:?}");
}

#[test]
fn optional_returns_rows_that_lack_the_optional_part() {
    let rows = solutions(
        &estate(),
        &format!(
            "SELECT ?name ?schema WHERE {{
               ?t <{DSC}name> ?name .
               OPTIONAL {{ ?t <{DSC}parentSchema> ?schema }}
             }}"
        ),
    );
    // Five subjects carry a name; only three of those carry a parent schema.
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert_eq!(values(&rows, "schema").len(), 3);
}

#[test]
fn ask_answers_a_yes_no_question() {
    let dataset = FlakeDataset::from_flakes(&estate()).expect("dataset");
    let query = SparqlParser::new()
        .parse_query(&format!("ASK {{ ?t <{DSC}name> \"upi_transactions\" }}"))
        .expect("parse");
    match QueryEvaluator::new()
        .prepare(&query)
        .execute(&dataset)
        .expect("run")
    {
        QueryResults::Boolean(answer) => assert!(answer),
        _ => panic!("an ASK must yield a boolean"),
    }
}

/// **This is the test the adopt-the-evaluator decision stands on.**
///
/// Authorization and time travel are applied by the caller *before* the dataset
/// is built. The evaluator never sees the excluded facts, so it cannot leak
/// them however it optimises — the exclusion is structural rather than a filter
/// the evaluator has to be trusted to apply.
#[test]
fn a_filtered_fact_set_yields_a_filtered_answer() {
    let all = estate();
    // What a `core_banking`-denied principal's scan would have returned.
    let permitted: Vec<Flake> = all
        .into_iter()
        .filter(|f| f.s.id != "cust" && f.s.id != "core_banking")
        .collect();

    let rows = solutions(
        &permitted,
        &format!("SELECT ?name WHERE {{ ?t <{DSC}type> \"table\" . ?t <{DSC}name> ?name }}"),
    );

    assert_eq!(
        values(&rows, "name"),
        vec!["neft_transactions", "upi_transactions"],
        "the denied table must be absent from the answer"
    );
    assert!(
        !format!("{rows:?}").contains("customers"),
        "no trace of the denied subject anywhere in the result"
    );
}

/// The same property for time: a dataset built at an earlier `t` cannot return
/// a later fact, because the later fact was never in it.
#[test]
fn a_fact_set_resolved_at_an_earlier_time_cannot_return_a_later_fact() {
    let mut flakes = estate();
    flakes.push(Flake::assert(
        Sid::dsc("added_later"),
        Sid::dsc("name"),
        FlakeValue::String("added_later".into()),
        99,
    ));
    // What the scan returns for as_of = 50.
    let as_of_50: Vec<Flake> = flakes.into_iter().filter(|f| f.t <= 50).collect();

    let rows = solutions(
        &as_of_50,
        &format!("SELECT ?name WHERE {{ ?t <{DSC}name> ?name }}"),
    );
    assert!(
        !values(&rows, "name").contains(&"added_later".to_string()),
        "{rows:?}"
    );
}

/// A named-graph fact must not answer a default-graph query. This is the
/// distinction that is easy to invert, and inverting it would let unconfirmed
/// extraction facts answer a catalog question.
#[test]
fn named_graph_facts_do_not_answer_a_default_graph_query() {
    let mut flakes = estate();
    flakes.push(Flake {
        cx: Some(Sid::dsc("graph:extraction")),
        ..flake("guess", "name", FlakeValue::String("guessed_table".into()))
    });

    let rows = solutions(
        &flakes,
        &format!("SELECT ?name WHERE {{ ?t <{DSC}name> ?name }}"),
    );
    assert!(
        !values(&rows, "name").contains(&"guessed_table".to_string()),
        "an extraction-graph fact answered a default-graph query: {rows:?}"
    );
}

#[test]
fn an_unknown_predicate_returns_no_rows_rather_than_failing() {
    let rows = solutions(
        &estate(),
        &format!("SELECT ?x WHERE {{ ?x <{DSC}neverDefined> ?y }}"),
    );
    assert!(rows.is_empty());
}

#[test]
fn a_malformed_query_is_a_parse_error_not_a_panic() {
    assert!(
        SparqlParser::new()
            .parse_query("SELECT ?x WHERE { this is not sparql")
            .is_err()
    );
}

/// SQL/SPARQL aggregation — Epic 105 P9's own sizing (the platform plan's
/// §9): "aggregations are probably not work at all," since `spareval`
/// evaluates `SUM`/`COUNT`/`AVG`/`GROUP BY`/`DISTINCT` *above* the
/// [`spareval::QueryableDataset`] this crate implements. These are
/// characterization tests, not an implementation — each one either proves
/// the claim or names exactly what is missing.
mod aggregations {
    use super::*;

    /// The numeric value out of a typed literal term (`"600"^^<...#integer>`)
    /// — `values()`'s own `trim_matches('"')` only strips a bare string
    /// literal's quotes, not the `^^<datatype>` suffix a typed numeric term
    /// always carries.
    fn numeric(term: &str) -> f64 {
        term.split("\"^^")
            .next()
            .expect("a typed literal")
            .trim_start_matches('"')
            .parse()
            .unwrap_or_else(|e| panic!("{term:?} did not parse as a number: {e}"))
    }

    fn invoices() -> Vec<Flake> {
        vec![
            flake("inv1", "type", FlakeValue::String("invoice".into())),
            flake("inv1", "amount", FlakeValue::Int(100)),
            flake("inv1", "vendor", FlakeValue::Ref(Sid::dsc("acme"))),
            flake("inv2", "type", FlakeValue::String("invoice".into())),
            flake("inv2", "amount", FlakeValue::Int(200)),
            flake("inv2", "vendor", FlakeValue::Ref(Sid::dsc("acme"))),
            flake("inv3", "type", FlakeValue::String("invoice".into())),
            flake("inv3", "amount", FlakeValue::Int(300)),
            flake("inv3", "vendor", FlakeValue::Ref(Sid::dsc("globex"))),
        ]
    }

    /// The platform plan's own worked example, verbatim: "three invoices at
    /// ₹100/₹200/₹300 must `SUM` to ₹600."
    #[test]
    fn sum_totals_across_matched_rows() {
        let rows = solutions(
            &invoices(),
            &format!(
                "SELECT (SUM(?amount) AS ?total) WHERE {{
                   ?i <{DSC}type> \"invoice\" .
                   ?i <{DSC}amount> ?amount
                 }}"
            ),
        );
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            numeric(&values(&rows, "total")[0]),
            600.0,
            "100 + 200 + 300, over xsd:integer terms, not string concatenation: {rows:?}"
        );
    }

    #[test]
    fn count_answers_how_many_not_the_rows_themselves() {
        let rows = solutions(
            &invoices(),
            &format!("SELECT (COUNT(?i) AS ?n) WHERE {{ ?i <{DSC}type> \"invoice\" }}"),
        );
        assert_eq!(numeric(&values(&rows, "n")[0]), 3.0, "{rows:?}");
    }

    #[test]
    fn avg_divides_the_sum_by_the_count() {
        let rows = solutions(
            &invoices(),
            &format!(
                "SELECT (AVG(?amount) AS ?mean) WHERE {{
                   ?i <{DSC}type> \"invoice\" .
                   ?i <{DSC}amount> ?amount
                 }}"
            ),
        );
        assert_eq!(rows.len(), 1, "{rows:?}");
        let mean = numeric(&values(&rows, "mean")[0]);
        assert!((mean - 200.0).abs() < f64::EPSILON, "{mean}");
    }

    /// One `SUM` per vendor, not one `SUM` over everything — the case a
    /// non-grouped aggregate cannot express and the reason `GROUP BY`
    /// matters on its own, not merely as `SUM`'s syntax.
    #[test]
    fn group_by_partitions_the_aggregate_per_key() {
        let rows = solutions(
            &invoices(),
            &format!(
                "SELECT ?vendor (SUM(?amount) AS ?total) WHERE {{
                   ?i <{DSC}type> \"invoice\" .
                   ?i <{DSC}amount> ?amount .
                   ?i <{DSC}vendor> ?vendor
                 }}
                 GROUP BY ?vendor"
            ),
        );
        assert_eq!(rows.len(), 2, "one row per vendor: {rows:?}");
        let by_vendor: std::collections::BTreeMap<String, f64> = rows
            .iter()
            .map(|row| {
                let vendor = row
                    .iter()
                    .find(|(name, _)| name == "vendor")
                    .map(|(_, v)| v.clone())
                    .expect("vendor bound");
                let total = row
                    .iter()
                    .find(|(name, _)| name == "total")
                    .map(|(_, v)| numeric(v))
                    .expect("total bound");
                (vendor, total)
            })
            .collect();
        // Both vendors happen to total 300 — 100+200 for acme, the lone 300
        // for globex — which would also be true of a `GROUP BY` that
        // silently ignored the grouping key and summed everything into one
        // bucket, *if* that bucket then leaked into two identical rows. The
        // row count assertion above is what actually rules that out; this
        // pins the per-vendor values so a regression that changes either
        // sum is still caught.
        assert_eq!(
            by_vendor.get(&format!("<{DSC}acme>")),
            Some(&300.0),
            "acme's two invoices: 100 + 200: {by_vendor:?}"
        );
        assert_eq!(
            by_vendor.get(&format!("<{DSC}globex>")),
            Some(&300.0),
            "globex's one invoice: {by_vendor:?}"
        );
    }

    #[test]
    fn distinct_collapses_duplicate_rows() {
        let rows = solutions(
            &invoices(),
            &format!(
                "SELECT DISTINCT ?vendor WHERE {{
                   ?i <{DSC}type> \"invoice\" .
                   ?i <{DSC}vendor> ?vendor
                 }}"
            ),
        );
        assert_eq!(
            rows.len(),
            2,
            "three invoices, two vendors, DISTINCT must not return three rows: {rows:?}"
        );
    }
}
