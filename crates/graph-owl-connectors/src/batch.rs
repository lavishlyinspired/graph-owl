//! Turning a parsed row into something the catalog can be asked to apply —
//! Epic 16 Slice C.
//!
//! [`rows`](crate::rows) knows nothing about entities: it hands back whatever
//! fields a line contained. This is where a row becomes a *claim about an
//! entity*, and where a row that does not make one is rejected with the row
//! number a client can grep their file for.
//!
//! Kept pure and kept here rather than in the API crate for two reasons: it is
//! the part with decisions in it (what a missing column means, what an empty
//! cell means, what happens to a column nobody recognises), and a leaf crate
//! mutates several times faster than `graph-owl-server` does.

use crate::rows::{Row, RowError};

/// One entity as a batch row declares it.
///
/// FQN-keyed rather than id-keyed — decision 1's out-of-process pushers do not
/// know graph-owl's UUIDs — and stringly-typed on `kind`, because parsing a kind
/// belongs to the layer that owns the vocabulary, not to a file reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowDraft {
    pub kind: String,
    pub name: String,
    pub parent_fqn: Option<String>,
    pub description: Option<String>,
    pub properties: Option<serde_json::Value>,
}

/// The columns this reader assigns a meaning to. Everything else becomes a
/// property — see [`draft_from_row`].
const RECOGNISED: &[&str] = &[
    "kind",
    "name",
    "parentFqn",
    "parent_fqn",
    "description",
    "properties",
];

/// Read one row as an entity claim.
///
/// # Errors
///
/// [`RowError::Malformed`] when the row does not name a kind and a name, naming
/// the missing column and carrying the row's own line number.
pub fn draft_from_row(row: &Row) -> Result<RowDraft, RowError> {
    let kind = required(row, "kind")?;
    let name = required(row, "name")?;

    // Both spellings. `parentFqn` is the wire contract (`00d` is camelCase
    // throughout), but CSV headers are typed by hand or produced by a SQL export,
    // and refusing `parent_fqn` would reject a file whose meaning is not in
    // doubt. Accepting a second spelling of one field is not the same as
    // guessing at an unknown one.
    let parent_fqn = optional(row, "parentFqn").or_else(|| optional(row, "parent_fqn"));

    // Columns nobody recognised are kept, not dropped. A warehouse export carries
    // whatever the warehouse had, and silently discarding data a client
    // deliberately sent is the worse of the two failures — a stray column costs a
    // property, a dropped one costs the fact.
    let mut properties = serde_json::Map::new();
    for (key, value) in &row.fields {
        if !RECOGNISED.contains(&key.as_str()) {
            properties.insert(key.clone(), value.clone());
        }
    }
    // An explicit `properties` object wins where they overlap: it is the
    // deliberate statement, and the loose columns are the leftovers.
    if let Some(serde_json::Value::Object(explicit)) = row.fields.get("properties") {
        for (key, value) in explicit {
            properties.insert(key.clone(), value.clone());
        }
    }

    Ok(RowDraft {
        kind,
        name,
        parent_fqn,
        description: optional(row, "description"),
        properties: if properties.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(properties))
        },
    })
}

/// A field that must be present and must say something.
fn required(row: &Row, column: &str) -> Result<String, RowError> {
    optional(row, column).ok_or_else(|| RowError::Malformed {
        number: row.number,
        detail: format!("no `{column}`, which every entity needs"),
    })
}

/// A field as a string, treating absent and blank alike.
///
/// **A CSV cannot express null.** An empty cell means "this file has nothing to
/// say about that", and reading it as the empty string would set a description to
/// `""` — overwriting a real one with nothing, which is a write no file asked for.
fn optional(row: &Row, column: &str) -> Option<String> {
    let text = match row.fields.get(column)? {
        serde_json::Value::String(text) => text.trim().to_string(),
        // A non-string is rendered rather than refused: a JSONL row may
        // legitimately carry a number, and `{"name": 42}` names an entity.
        serde_json::Value::Null => return None,
        other => other.to_string(),
    };
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(number: u64, pairs: &[(&str, serde_json::Value)]) -> Row {
        Row {
            number,
            fields: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        }
    }

    fn text(value: &str) -> serde_json::Value {
        serde_json::Value::String(value.to_string())
    }

    #[test]
    fn a_row_naming_a_kind_and_a_name_is_a_draft() {
        let draft = draft_from_row(&row(
            2,
            &[
                ("kind", text("table")),
                ("name", text("orders")),
                ("parentFqn", text("svc.db.public")),
                ("description", text("one row per order")),
            ],
        ))
        .expect("draft");

        assert_eq!(draft.kind, "table");
        assert_eq!(draft.name, "orders");
        assert_eq!(draft.parent_fqn.as_deref(), Some("svc.db.public"));
        assert_eq!(draft.description.as_deref(), Some("one row per order"));
    }

    // The error carries the **row's own number**, because that is what a client
    // greps their file with. A generic "some row was bad" is unactionable at
    // 500k rows.
    #[test]
    fn a_row_with_no_kind_is_rejected_naming_the_column_and_the_row() {
        let error = draft_from_row(&row(97, &[("name", text("orders"))])).expect_err("rejected");

        let RowError::Malformed { number, detail } = error;
        assert_eq!(number, 97);
        assert!(detail.contains("kind"), "{detail}");
    }

    #[test]
    fn a_row_with_no_name_is_rejected_naming_the_column() {
        let error = draft_from_row(&row(3, &[("kind", text("table"))])).expect_err("rejected");

        let RowError::Malformed { detail, .. } = error;
        assert!(detail.contains("name"), "{detail}");
    }

    // **An empty cell is absence, not the empty string.** CSV has no null, and a
    // description read as `""` would overwrite a real one with nothing — a write
    // the file never asked for.
    #[test]
    fn an_empty_cell_is_absent_rather_than_empty() {
        let draft = draft_from_row(&row(
            2,
            &[
                ("kind", text("table")),
                ("name", text("orders")),
                ("description", text("   ")),
                ("parentFqn", text("")),
            ],
        ))
        .expect("draft");

        assert_eq!(draft.description, None);
        assert_eq!(draft.parent_fqn, None);
    }

    // And an empty *required* cell is a rejection, for the same reason: a row
    // with a blank name has not named anything.
    #[test]
    fn a_blank_required_cell_is_rejected_not_accepted_as_empty() {
        assert!(draft_from_row(&row(2, &[("kind", text("table")), ("name", text(" "))])).is_err());
    }

    // Snake case is accepted for the one field a CSV author types by hand, since
    // a SQL export produces it and the meaning is not in doubt.
    #[test]
    fn the_parent_column_is_accepted_in_either_spelling() {
        let draft = draft_from_row(&row(
            2,
            &[
                ("kind", text("table")),
                ("name", text("orders")),
                ("parent_fqn", text("svc.db.public")),
            ],
        ))
        .expect("draft");

        assert_eq!(draft.parent_fqn.as_deref(), Some("svc.db.public"));
    }

    // **Columns nobody recognised are kept.** A stray column costs a property; a
    // dropped one costs the fact, and a warehouse export carries whatever the
    // warehouse had.
    #[test]
    fn unrecognised_columns_become_properties() {
        let draft = draft_from_row(&row(
            2,
            &[
                ("kind", text("table")),
                ("name", text("orders")),
                ("row_count", text("41000")),
            ],
        ))
        .expect("draft");

        assert_eq!(
            draft.properties,
            Some(serde_json::json!({ "row_count": "41000" }))
        );
    }

    // A recognised column is *not* also a property — otherwise every row would
    // carry a duplicate of its own name.
    #[test]
    fn recognised_columns_are_not_repeated_as_properties() {
        let draft = draft_from_row(&row(
            2,
            &[("kind", text("table")), ("name", text("orders"))],
        ))
        .expect("draft");

        assert_eq!(draft.properties, None);
    }

    // An explicit `properties` object is the deliberate statement and wins over a
    // loose column of the same name.
    #[test]
    fn an_explicit_properties_object_beats_a_loose_column() {
        let draft = draft_from_row(&row(
            2,
            &[
                ("kind", text("table")),
                ("name", text("orders")),
                ("owner", text("loose")),
                ("properties", serde_json::json!({ "owner": "explicit" })),
            ],
        ))
        .expect("draft");

        assert_eq!(
            draft.properties,
            Some(serde_json::json!({ "owner": "explicit" }))
        );
    }

    // A JSONL row may legitimately carry a non-string, and `{"name": 42}` names
    // an entity — rendering beats refusing.
    #[test]
    fn a_non_string_value_is_rendered_rather_than_refused() {
        let draft = draft_from_row(&row(
            2,
            &[
                ("kind", text("table")),
                ("name", serde_json::json!(42)),
                ("description", serde_json::Value::Null),
            ],
        ))
        .expect("draft");

        assert_eq!(draft.name, "42");
        assert_eq!(draft.description, None);
    }
}
