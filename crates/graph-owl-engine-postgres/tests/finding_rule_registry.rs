//! Epic 105 P5b: finding rules definable at runtime, so a pack's
//! reconciliation rules are declared once and evaluated by the native
//! reconcile engine — `plans/105b-native-reconcile-engine.md`.
//!
//! These prove the half only a real database can: that a declaration
//! persists, that it is scoped correctly per pack, and — the design's own
//! deliberate departure from `namespaces`/`predicates` — that redeclaring
//! the same `(pack, label)` **replaces** the row rather than conflicting,
//! because a finding rule carries no stored artifact a changed query would
//! invalidate.

mod common;

use graph_owl_engine::{EvidenceBinding, FindingRuleDef, FindingRuleRegistry};
use graph_owl_engine_postgres::PostgresTripleStore;

async fn store() -> (PostgresTripleStore, common::TestDb, String) {
    let (database, connection_string) = common::fresh_database().await;
    let store = PostgresTripleStore::connect(&connection_string)
        .await
        .expect("engine should connect and migrate");
    (store, database, connection_string)
}

fn rule(pack: &str, label: &str, query: &str) -> FindingRuleDef {
    FindingRuleDef {
        pack: pack.to_string(),
        label: label.to_string(),
        summary: "an invoice claimed but never filed".to_string(),
        governed_by: "gst:Section16-2-aa".to_string(),
        query: query.to_string(),
        subject_var: "invoice".to_string(),
        evidence: vec![
            EvidenceBinding {
                predicate: "gst:supplierGstin".to_string(),
                var: "gstin".to_string(),
            },
            EvidenceBinding {
                predicate: "gst:invoiceNumber".to_string(),
                var: "number".to_string(),
            },
        ],
        similarity: None,
        span: None,
    }
}

#[tokio::test]
async fn a_declared_rule_is_read_back() {
    let (store, _db, _url) = store().await;

    store
        .declare(&rule("gst", "gst:PotentialMismatch", "SELECT ?invoice {}"))
        .await
        .expect("declare");

    let rules = store.for_pack("gst").await.expect("list");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].label, "gst:PotentialMismatch");
    assert_eq!(rules[0].evidence.len(), 2);
    assert_eq!(rules[0].evidence[0].predicate, "gst:supplierGstin");
}

#[tokio::test]
async fn rules_are_scoped_per_pack() {
    let (store, _db, _url) = store().await;

    store
        .declare(&rule("gst", "gst:PotentialMismatch", "SELECT ?invoice {}"))
        .await
        .expect("declare gst");
    store
        .declare(&rule(
            "hospitality",
            "hosp:DuplicateGuest",
            "SELECT ?guest {}",
        ))
        .await
        .expect("declare hospitality");

    let gst_rules = store.for_pack("gst").await.expect("list");
    assert_eq!(gst_rules.len(), 1);
    assert_eq!(gst_rules[0].pack, "gst");

    let hosp_rules = store.for_pack("hospitality").await.expect("list");
    assert_eq!(hosp_rules.len(), 1);
    assert_eq!(hosp_rules[0].label, "hosp:DuplicateGuest");
}

/// **The load-bearing difference from `namespaces`/`predicates`.** A code
/// once assigned is permanent, and a predicate's value type is permanent,
/// because flakes are already stored against them. A finding rule's query
/// text has nothing stored against it — so redeclaring must replace, not
/// refuse, or every second `demo.sh` run (which reloads the pack) would
/// either fail outright or leave a stale query silently in force.
#[tokio::test]
async fn redeclaring_the_same_label_replaces_the_query_rather_than_duplicating() {
    let (store, _db, _url) = store().await;

    store
        .declare(&rule("gst", "gst:PotentialMismatch", "SELECT ?invoice {}"))
        .await
        .expect("first declare");
    store
        .declare(&rule(
            "gst",
            "gst:PotentialMismatch",
            "SELECT ?invoice { ?invoice a gst:PurchaseInvoice }",
        ))
        .await
        .expect("redeclare with an edited query");

    let rules = store.for_pack("gst").await.expect("list");
    assert_eq!(rules.len(), 1, "must replace, not duplicate: {rules:?}");
    assert!(rules[0].query.contains("PurchaseInvoice"));
}

/// A pack that declares no rules is a legitimate, half-built state — not an
/// error. The reconcile endpoint reads this as "nothing to evaluate", the
/// same reading `run_findings` in the Python predecessor already gave it.
#[tokio::test]
async fn a_pack_with_no_declared_rules_returns_an_empty_list() {
    let (store, _db, _url) = store().await;
    assert_eq!(store.for_pack("nonexistent").await.expect("list"), vec![]);
}

/// The similarity/span bands round-trip as opaque JSON — proving the
/// registry never has to know their shape, only store and return it, exactly
/// as `FindingRuleDef`'s own doc comment states.
#[tokio::test]
async fn similarity_and_span_bands_round_trip_as_opaque_json() {
    let (store, _db, _url) = store().await;

    let mut with_bands = rule("gst", "gst:GstinTransposition", "SELECT ?purchase {}");
    with_bands.similarity = Some(serde_json::json!({
        "strategy": "ngram", "n": 3, "left": "claimedGstin", "right": "filedGstin",
        "atLeast": 0.40, "atMost": 0.999
    }));
    with_bands.span = Some(serde_json::json!({
        "from": "purchasedAt", "to": "paidAt", "exceedsDays": 180,
        "whenMissing": "elapsed", "asOf": "2026-08-01"
    }));

    store.declare(&with_bands).await.expect("declare");

    let rules = store.for_pack("gst").await.expect("list");
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].similarity.as_ref().and_then(|v| v.get("atLeast")),
        Some(&serde_json::json!(0.40))
    );
    assert_eq!(
        rules[0].span.as_ref().and_then(|v| v.get("exceedsDays")),
        Some(&serde_json::json!(180))
    );
}

/// A rule with neither band (the four rules that are pure joins, no fuzzy
/// matching and no time arithmetic) must round-trip as `None`, not as a
/// JSON `null` sitting where `Option::None` should be — the two are
/// indistinguishable to a careless `Option<serde_json::Value>` mapping and
/// would make a rule's own presence check ambiguous.
#[tokio::test]
async fn a_rule_with_no_bands_round_trips_as_none_not_json_null() {
    let (store, _db, _url) = store().await;

    store
        .declare(&rule("gst", "gst:ITCNotAvailable", "SELECT ?purchase {}"))
        .await
        .expect("declare");

    let rules = store.for_pack("gst").await.expect("list");
    assert_eq!(rules[0].similarity, None);
    assert_eq!(rules[0].span, None);
}
