//! `Metric` as a first-class entity — Epic 24 Slices E and F.
//!
//! "Which certified revenue metric should I use?" is asked constantly and is
//! unanswerable if metrics exist only as attributes of charts. A metric with a
//! definition, an owner, and lineage to its sources is the difference between a
//! catalog that describes dashboards and one that describes the business.
//!
//! **graph-owl describes metrics; it does not compute them** (decision 3). The
//! formula is prose. Storing an evaluable AST would imply a computation engine
//! this deliberately is not, and the first person to see an AST would reasonably
//! expect a number back.

use serde::{Deserialize, Serialize};

/// How a metric is arrived at. Descriptive — nothing here is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalculationType {
    Simple,
    Ratio,
    Derived,
    Composite,
}

impl CalculationType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CalculationType::Simple => "simple",
            CalculationType::Ratio => "ratio",
            CalculationType::Derived => "derived",
            CalculationType::Composite => "composite",
        }
    }

    /// # Errors
    /// The unrecognised value, so the caller can name it.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "simple" => Ok(CalculationType::Simple),
            "ratio" => Ok(CalculationType::Ratio),
            "derived" => Ok(CalculationType::Derived),
            "composite" => Ok(CalculationType::Composite),
            other => Err(other.to_string()),
        }
    }

    pub const ALL: [CalculationType; 4] = [
        CalculationType::Simple,
        CalculationType::Ratio,
        CalculationType::Derived,
        CalculationType::Composite,
    ];
}

/// Something missing from a metric that is worth saying out loud.
///
/// **Gaps are reported, not refused.** A metric whose sources nobody recorded is
/// extremely common and is exactly the metric most worth cataloguing — refusing
/// it would keep the catalog clean by keeping the truth out of it. Epic 14's
/// `TrustSummary` is where these surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MetricGap {
    /// No `source_assets`, so its lineage cannot be derived and nobody can check
    /// where the number comes from.
    NoSources,
    /// No `defined_by`, so the metric's name is its only definition of itself.
    NoDefiningTerm,
    /// No formula, so two teams can compute it differently and both be "right".
    NoFormula,
}

impl MetricGap {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MetricGap::NoSources => "noSources",
            MetricGap::NoDefiningTerm => "noDefiningTerm",
            MetricGap::NoFormula => "noFormula",
        }
    }
}

/// What a metric declares about itself, for the parts that get judged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MetricClaims<'a> {
    pub source_assets: &'a [String],
    pub defined_by: Option<&'a str>,
    pub formula: Option<&'a str>,
}

/// Everything absent from a metric that a reader should be told about.
///
/// Ordered most- to least- consequential, so a UI that shows one shows the one
/// that matters.
#[must_use]
pub fn gaps(claims: &MetricClaims<'_>) -> Vec<MetricGap> {
    let mut found = Vec::new();
    if claims.source_assets.is_empty() {
        found.push(MetricGap::NoSources);
    }
    if claims.defined_by.is_none_or(str::is_empty) {
        found.push(MetricGap::NoDefiningTerm);
    }
    // Whitespace is absence. A formula of spaces clears the flag without
    // supplying the fact, which is worse than the gap it hides.
    if claims.formula.is_none_or(|f| f.trim().is_empty()) {
        found.push(MetricGap::NoFormula);
    }
    found
}

/// One lineage edge implied by a metric naming a source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DerivedEdge {
    /// The asset the metric derives from.
    pub from: String,
    /// The metric.
    pub to: String,
}

/// What must change so the stored edges match what the metric now claims.
///
/// **Scoped by source, not wholesale** (Slice F, the same rule as Epic 29 Slice
/// E). A metric's `source_assets` govern the edges *the metric asserted*; an
/// edge a person or a connector added is a different claim about the same pair,
/// and replacing everything would delete somebody else's fact as a side effect
/// of an edit they never saw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdgeReconciliation {
    pub to_add: Vec<DerivedEdge>,
    pub to_retract: Vec<DerivedEdge>,
}

/// An edge as stored, with the claim it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEdge {
    pub from: String,
    pub to: String,
    /// True when this edge was asserted by the metric's own `source_assets`
    /// rather than by a person or a connector.
    pub from_metric: bool,
}

/// Reconcile a metric's derived lineage against what is stored.
#[must_use]
pub fn reconcile_lineage(
    metric: &str,
    source_assets: &[String],
    stored: &[StoredEdge],
) -> EdgeReconciliation {
    // A metric naming itself would be its own ancestor, and Epic 29's traversal
    // has to terminate. Deduplicated because a copy-pasted row in an import
    // otherwise produces two edges that traversal then counts twice.
    let mut wanted = Vec::new();
    for source in source_assets {
        if source != metric && !wanted.contains(source) {
            wanted.push(source.clone());
        }
    }

    // **Only edges this metric asserted are ours to change.** A hand-drawn or
    // connector-observed edge is a different claim about the same pair, and
    // retracting it here would delete somebody else's fact as a side effect of an
    // edit its author never saw.
    let ours: Vec<&StoredEdge> = stored
        .iter()
        .filter(|e| e.from_metric && e.to == metric)
        .collect();

    // But an edge somebody *else* asserted still means the pair is already
    // recorded, so re-adding it would duplicate the fact under a second source.
    let already_recorded = |source: &str| stored.iter().any(|e| e.from == source && e.to == metric);

    EdgeReconciliation {
        to_add: wanted
            .iter()
            .filter(|source| !already_recorded(source))
            .map(|source| DerivedEdge {
                from: source.clone(),
                to: metric.to_string(),
            })
            .collect(),
        to_retract: ours
            .iter()
            .filter(|edge| !wanted.contains(&edge.from))
            .map(|edge| DerivedEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    // ---- gaps are reported, not refused (Slice E) ----

    // **A source-less metric is catalogued and flagged, never rejected.** It is
    // the commonest metric there is and the one most worth having a record of;
    // refusing it would keep the catalog tidy by keeping the truth out of it.
    #[test]
    fn a_metric_with_no_sources_reports_the_gap_rather_than_failing() {
        let found = gaps(&MetricClaims {
            source_assets: &[],
            defined_by: Some("term-1"),
            formula: Some("sum(amount)"),
        });

        assert_eq!(found, vec![MetricGap::NoSources]);
    }

    // The negative half. A gap detector that always reported `NoSources` would
    // pass the test above.
    #[test]
    fn a_fully_specified_metric_has_no_gaps() {
        let sources = owned(&["svc.db.public.orders"]);
        let found = gaps(&MetricClaims {
            source_assets: &sources,
            defined_by: Some("term-1"),
            formula: Some("sum(amount)"),
        });

        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn each_missing_part_is_reported_separately() {
        let sources = owned(&["a"]);
        assert_eq!(
            gaps(&MetricClaims {
                source_assets: &sources,
                defined_by: None,
                formula: Some("x"),
            }),
            vec![MetricGap::NoDefiningTerm]
        );
        assert_eq!(
            gaps(&MetricClaims {
                source_assets: &sources,
                defined_by: Some("t"),
                formula: None,
            }),
            vec![MetricGap::NoFormula]
        );
    }

    #[test]
    fn a_metric_declaring_nothing_reports_every_gap() {
        let found = gaps(&MetricClaims::default());

        assert_eq!(
            found,
            vec![
                MetricGap::NoSources,
                MetricGap::NoDefiningTerm,
                MetricGap::NoFormula
            ]
        );
    }

    // A formula of whitespace is not a formula. Treating it as present would let
    // a metric clear the gap check by declaring nothing, which is worse than the
    // gap — it silences the flag without supplying the fact.
    #[test]
    fn a_blank_formula_is_absent_rather_than_present() {
        let sources = owned(&["a"]);
        let found = gaps(&MetricClaims {
            source_assets: &sources,
            defined_by: Some("t"),
            formula: Some("   "),
        });

        assert_eq!(found, vec![MetricGap::NoFormula]);
    }

    // ---- lineage is derived from the sources (Slice F) ----

    #[test]
    fn declaring_a_source_implies_an_edge_to_the_metric() {
        let plan = reconcile_lineage("metric-1", &owned(&["orders", "refunds"]), &[]);

        assert_eq!(
            plan.to_add,
            vec![
                DerivedEdge {
                    from: "orders".into(),
                    to: "metric-1".into()
                },
                DerivedEdge {
                    from: "refunds".into(),
                    to: "metric-1".into()
                },
            ]
        );
        assert!(plan.to_retract.is_empty());
    }

    #[test]
    fn removing_a_source_retracts_its_edge() {
        let stored = vec![
            StoredEdge {
                from: "orders".into(),
                to: "metric-1".into(),
                from_metric: true,
            },
            StoredEdge {
                from: "refunds".into(),
                to: "metric-1".into(),
                from_metric: true,
            },
        ];

        let plan = reconcile_lineage("metric-1", &owned(&["orders"]), &stored);

        assert!(plan.to_add.is_empty());
        assert_eq!(
            plan.to_retract,
            vec![DerivedEdge {
                from: "refunds".into(),
                to: "metric-1".into()
            }]
        );
    }

    // An unchanged source is left alone. Retract-then-re-add would produce the
    // same final state while churning the edge's identity and its history.
    #[test]
    fn an_unchanged_source_is_neither_added_nor_retracted() {
        let stored = vec![StoredEdge {
            from: "orders".into(),
            to: "metric-1".into(),
            from_metric: true,
        }];

        let plan = reconcile_lineage("metric-1", &owned(&["orders"]), &stored);

        assert_eq!(plan, EdgeReconciliation::default());
    }

    // **The reconciliation test the plan names.** A hand-asserted lineage edge is
    // a different person's claim about the same pair, and wholesale replacement
    // would delete it as a side effect of an edit whose author never saw it.
    #[test]
    fn a_manually_asserted_edge_survives_a_source_change() {
        let stored = vec![
            StoredEdge {
                from: "orders".into(),
                to: "metric-1".into(),
                from_metric: true,
            },
            StoredEdge {
                from: "hand-drawn".into(),
                to: "metric-1".into(),
                from_metric: false,
            },
        ];

        let plan = reconcile_lineage("metric-1", &owned(&["refunds"]), &stored);

        assert_eq!(
            plan.to_retract,
            vec![DerivedEdge {
                from: "orders".into(),
                to: "metric-1".into()
            }],
            "only the metric's own edge is retracted"
        );
        assert_eq!(
            plan.to_add,
            vec![DerivedEdge {
                from: "refunds".into(),
                to: "metric-1".into()
            }]
        );
    }

    // And the other half of source-scoping: if a person asserted the same pair the
    // metric now claims, the metric does not add a duplicate.
    #[test]
    fn a_source_a_person_already_asserted_is_not_added_twice() {
        let stored = vec![StoredEdge {
            from: "orders".into(),
            to: "metric-1".into(),
            from_metric: false,
        }];

        let plan = reconcile_lineage("metric-1", &owned(&["orders"]), &stored);

        assert!(plan.to_add.is_empty(), "{plan:?}");
        assert!(plan.to_retract.is_empty(), "the person's edge is not ours");
    }

    // Edges belonging to a *different* metric must not be touched. The store is
    // shared, and a reconciler that ignored `to` would retract another metric's
    // lineage every time this one was edited.
    #[test]
    fn another_metrics_edges_are_left_alone() {
        let stored = vec![StoredEdge {
            from: "orders".into(),
            to: "metric-2".into(),
            from_metric: true,
        }];

        let plan = reconcile_lineage("metric-1", &[], &stored);

        assert_eq!(plan, EdgeReconciliation::default());
    }

    #[test]
    fn clearing_every_source_retracts_every_derived_edge() {
        let stored = vec![StoredEdge {
            from: "orders".into(),
            to: "metric-1".into(),
            from_metric: true,
        }];

        let plan = reconcile_lineage("metric-1", &[], &stored);

        assert_eq!(plan.to_retract.len(), 1);
    }

    // A source named twice is one edge. Without this a copy-pasted row in an
    // import produces two edges that traversal then counts twice.
    #[test]
    fn a_source_named_twice_produces_one_edge() {
        let plan = reconcile_lineage("metric-1", &owned(&["orders", "orders"]), &[]);

        assert_eq!(plan.to_add.len(), 1);
    }

    // A metric naming itself as a source is a self-loop that would make it its own
    // ancestor, and Epic 29's traversal has to terminate.
    #[test]
    fn a_metric_is_not_its_own_source() {
        let plan = reconcile_lineage("metric-1", &owned(&["metric-1", "orders"]), &[]);

        assert_eq!(
            plan.to_add,
            vec![DerivedEdge {
                from: "orders".into(),
                to: "metric-1".into()
            }]
        );
    }

    #[test]
    fn every_calculation_type_round_trips_through_its_wire_name() {
        for kind in CalculationType::ALL {
            assert_eq!(CalculationType::parse(kind.as_str()), Ok(kind));
        }
        assert_eq!(
            CalculationType::parse("evaluated"),
            Err("evaluated".to_string())
        );
    }

    #[test]
    fn each_gap_has_its_own_wire_name() {
        let names: std::collections::HashSet<_> = [
            MetricGap::NoSources,
            MetricGap::NoDefiningTerm,
            MetricGap::NoFormula,
        ]
        .iter()
        .map(|g| g.as_str())
        .collect();

        assert_eq!(
            names.len(),
            3,
            "a shared name makes two gaps indistinguishable"
        );
    }
}
