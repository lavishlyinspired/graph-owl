//! Declarative payload-to-draft mapping — Epic 18 Slice C.
//!
//! Turns a webhook payload into a [`RowDraft`], the same claim-about-an-entity
//! shape [`crate::batch::draft_from_row`] produces from a batch file — one
//! entity-draft concept, two sources. `Expression`'s five variants
//! ([`graph_owl_storage::Expression`]) are the whole vocabulary a mapping can
//! use, deliberately closed: a general scripting language can hang the
//! receiver forever, and every variant here recurses into a strictly smaller,
//! owned sub-expression, so evaluation terminates by construction.

use crate::batch::RowDraft;
use graph_owl_storage::{Expression, Mapping};
use std::collections::BTreeMap;

/// A mapping's required field resolved to nothing.
///
/// Names the mapping **and** the field, because "invalid draft" alone gives
/// a pusher nothing to fix — Slice C's own stated reason for existing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingError {
    pub mapping: String,
    pub field: &'static str,
}

/// Resolves one expression against a payload to a string, or `None` if a
/// [`Expression::Path`] inside it finds nothing.
#[must_use]
pub fn evaluate(expression: &Expression, payload: &serde_json::Value) -> Option<String> {
    match expression {
        Expression::Path { pointer } => payload.pointer(pointer).map(render),
        Expression::Literal { value } => Some(value.clone()),
        Expression::Concat { parts } => {
            let mut out = String::new();
            for part in parts {
                out.push_str(&evaluate(part, payload)?);
            }
            Some(out)
        }
        Expression::Lowercase { of } => evaluate(of, payload).map(|s| s.to_lowercase()),
        Expression::Template { pattern, bindings } => evaluate_template(pattern, bindings, payload),
    }
}

/// A non-string JSON value is rendered rather than refused — the same
/// reasoning [`crate::batch::draft_from_row`] applies to a JSONL row: a
/// number or a bool still names something.
fn render(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Substitutes each `{name}` in `pattern` from `bindings`'s evaluated
/// results, **in a single left-to-right pass over `pattern` only**. The
/// output is never re-scanned for more placeholders, so a bound value that
/// itself contains `{...}` cannot trigger a second substitution — the loop
/// risk an unbounded template evaluator would have.
fn evaluate_template(
    pattern: &str,
    bindings: &BTreeMap<String, Expression>,
    payload: &serde_json::Value,
) -> Option<String> {
    let mut resolved = BTreeMap::new();
    for (key, expr) in bindings {
        resolved.insert(key.as_str(), evaluate(expr, payload)?);
    }

    let mut out = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else {
            // No closing brace: the rest is literal text, not a placeholder.
            out.push_str(rest);
            rest = "";
            break;
        };
        let end = start + len;
        out.push_str(&rest[..start]);
        out.push_str(resolved.get(&rest[start + 1..end])?);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Applies a mapping to a payload, producing the draft it claims.
///
/// # Errors
///
/// [`MappingError`] naming the mapping and whichever of `kind`/`entity_name`
/// — the two fields every entity-draft path in this codebase requires —
/// resolved to nothing.
pub fn apply_mapping(
    mapping: &Mapping,
    payload: &serde_json::Value,
) -> Result<RowDraft, MappingError> {
    let kind = evaluate(&mapping.kind, payload).ok_or_else(|| MappingError {
        mapping: mapping.name.clone(),
        field: "kind",
    })?;
    let name = evaluate(&mapping.entity_name, payload).ok_or_else(|| MappingError {
        mapping: mapping.name.clone(),
        field: "name",
    })?;
    let parent_fqn = mapping
        .parent_fqn
        .as_ref()
        .and_then(|expr| evaluate(expr, payload));
    let description = mapping
        .description
        .as_ref()
        .and_then(|expr| evaluate(expr, payload));

    let mut properties = serde_json::Map::new();
    for (key, expr) in &mapping.properties {
        if let Some(value) = evaluate(expr, payload) {
            properties.insert(key.clone(), serde_json::Value::String(value));
        }
    }

    Ok(RowDraft {
        kind,
        name,
        parent_fqn,
        description,
        properties: if properties.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(properties))
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn path(pointer: &str) -> Expression {
        Expression::Path {
            pointer: pointer.to_string(),
        }
    }

    fn literal(value: &str) -> Expression {
        Expression::Literal {
            value: value.to_string(),
        }
    }

    #[test]
    fn a_path_resolves_a_string_value() {
        let payload = json!({"run": {"status": "table"}});
        assert_eq!(
            evaluate(&path("/run/status"), &payload),
            Some("table".to_string())
        );
    }

    #[test]
    fn a_path_to_nothing_is_none() {
        let payload = json!({"run": {}});
        assert_eq!(evaluate(&path("/run/missing"), &payload), None);
    }

    #[test]
    fn a_path_to_a_number_renders_it() {
        let payload = json!({"row_count": 41000});
        assert_eq!(
            evaluate(&path("/row_count"), &payload),
            Some("41000".to_string())
        );
    }

    #[test]
    fn a_path_to_null_renders_the_empty_string() {
        let payload = json!({"description": null});
        assert_eq!(
            evaluate(&path("/description"), &payload),
            Some(String::new())
        );
    }

    #[test]
    fn a_literal_ignores_the_payload_entirely() {
        let payload = json!({});
        assert_eq!(evaluate(&literal("dbt"), &payload), Some("dbt".to_string()));
    }

    #[test]
    fn concat_joins_parts_with_nothing_between() {
        let payload = json!({"schema": "public", "table": "orders"});
        let expr = Expression::Concat {
            parts: vec![path("/schema"), literal("."), path("/table")],
        };
        assert_eq!(evaluate(&expr, &payload), Some("public.orders".to_string()));
    }

    #[test]
    fn concat_fails_if_any_part_fails() {
        let payload = json!({"schema": "public"});
        let expr = Expression::Concat {
            parts: vec![path("/schema"), literal("."), path("/missing")],
        };
        assert_eq!(evaluate(&expr, &payload), None);
    }

    #[test]
    fn lowercase_transforms_its_sub_expression() {
        let payload = json!({"name": "ORDERS"});
        let expr = Expression::Lowercase {
            of: Box::new(path("/name")),
        };
        assert_eq!(evaluate(&expr, &payload), Some("orders".to_string()));
    }

    #[test]
    fn lowercase_fails_if_its_sub_expression_fails() {
        let payload = json!({});
        let expr = Expression::Lowercase {
            of: Box::new(path("/missing")),
        };
        assert_eq!(evaluate(&expr, &payload), None);
    }

    #[test]
    fn template_substitutes_each_named_binding() {
        let payload = json!({"schema": "public", "table": "orders"});
        let expr = Expression::Template {
            pattern: "{schema}.{table}".to_string(),
            bindings: BTreeMap::from([
                ("schema".to_string(), path("/schema")),
                ("table".to_string(), path("/table")),
            ]),
        };
        assert_eq!(evaluate(&expr, &payload), Some("public.orders".to_string()));
    }

    #[test]
    fn template_keeps_literal_text_around_placeholders() {
        let payload = json!({"id": "42"});
        let expr = Expression::Template {
            pattern: "run-{id}-completed".to_string(),
            bindings: BTreeMap::from([("id".to_string(), path("/id"))]),
        };
        assert_eq!(
            evaluate(&expr, &payload),
            Some("run-42-completed".to_string())
        );
    }

    #[test]
    fn template_fails_if_a_binding_fails() {
        let payload = json!({});
        let expr = Expression::Template {
            pattern: "{missing}".to_string(),
            bindings: BTreeMap::from([("missing".to_string(), path("/nothing"))]),
        };
        assert_eq!(evaluate(&expr, &payload), None);
    }

    /// **The mutator-watch case.** A naive template evaluator that re-scans
    /// its *output* for more `{...}` placeholders would try to substitute
    /// again here, and if a binding's own value could reference a binding,
    /// that is an unbounded loop. This binds a value that *looks like* a
    /// placeholder and asserts it comes through as literal text — proving
    /// substitution is exactly one pass over the pattern, never the output.
    #[test]
    fn a_bound_value_containing_braces_is_not_re_substituted() {
        let payload = json!({"weird": "{table}"});
        let expr = Expression::Template {
            pattern: "prefix-{weird}-suffix".to_string(),
            bindings: BTreeMap::from([("weird".to_string(), path("/weird"))]),
        };
        assert_eq!(
            evaluate(&expr, &payload),
            Some("prefix-{table}-suffix".to_string()),
            "the bound value's own braces must not be treated as a second placeholder"
        );
    }

    #[test]
    fn deeply_nested_expressions_still_terminate() {
        // A positive control alongside the loop-risk test above: nesting is
        // finite and owned, so even a chain this deep evaluates once and
        // returns — there is no runaway recursion to guard against here.
        let payload = json!({"v": "x"});
        let mut expr = path("/v");
        for _ in 0..200 {
            expr = Expression::Concat {
                parts: vec![expr, literal("")],
            };
        }
        assert_eq!(evaluate(&expr, &payload), Some("x".to_string()));
    }

    fn mapping(kind: Expression, entity_name: Expression) -> Mapping {
        Mapping {
            name: "dbt-run-completed".to_string(),
            version: 1,
            kind,
            entity_name,
            parent_fqn: None,
            description: None,
            properties: BTreeMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_complete_mapping_produces_a_draft() {
        let payload = json!({"kind": "table", "table_name": "orders"});
        let m = mapping(path("/kind"), path("/table_name"));

        let draft = apply_mapping(&m, &payload).expect("draft");
        assert_eq!(draft.kind, "table");
        assert_eq!(draft.name, "orders");
    }

    #[test]
    fn a_missing_kind_path_names_the_mapping_and_the_field() {
        let payload = json!({"table_name": "orders"});
        let m = mapping(path("/kind"), path("/table_name"));

        let error = apply_mapping(&m, &payload).expect_err("mapping failure");
        assert_eq!(error.mapping, "dbt-run-completed");
        assert_eq!(error.field, "kind");
    }

    #[test]
    fn a_missing_name_path_names_the_mapping_and_the_field() {
        let payload = json!({"kind": "table"});
        let m = mapping(path("/kind"), path("/table_name"));

        let error = apply_mapping(&m, &payload).expect_err("mapping failure");
        assert_eq!(error.field, "name");
    }

    #[test]
    fn an_optional_field_missing_from_the_payload_is_absent_not_an_error() {
        let payload = json!({"kind": "table", "table_name": "orders"});
        let mut m = mapping(path("/kind"), path("/table_name"));
        m.description = Some(path("/missing"));

        let draft = apply_mapping(&m, &payload).expect("draft");
        assert_eq!(draft.description, None);
    }

    #[test]
    fn properties_are_built_from_the_declared_expressions() {
        let payload = json!({"kind": "table", "table_name": "orders", "rows": 41000});
        let mut m = mapping(path("/kind"), path("/table_name"));
        m.properties = BTreeMap::from([("rowCount".to_string(), path("/rows"))]);

        let draft = apply_mapping(&m, &payload).expect("draft");
        assert_eq!(draft.properties, Some(json!({"rowCount": "41000"})));
    }
}
