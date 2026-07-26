//! Boundary validation that reports **every** problem in a request, not the first.
//!
//! `serde` stops at its first error by design, which forces a client into one
//! round trip per mistake. So a request body is parsed to a [`serde_json::Value`]
//! first, validated field-by-field with the failures accumulated, and only then
//! deserialized into its typed form.

use serde::Serialize;
use serde_json::Value;

/// Why a field failed. Machine-readable and stable — a client branches on this,
/// and the three cases need different fixes: supply it, fill it in, or fix its
/// type. Collapsing them into one code makes the response undiagnosable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldErrorCode {
    /// The field was absent and is mandatory.
    Required,
    /// The field was present but blank.
    Empty,
    /// The field was present but of the wrong JSON type.
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    /// Dotted and indexed path to the offending value, e.g. `owners[0].id`.
    pub field: String,
    pub code: FieldErrorCode,
    pub detail: String,
}

impl FieldError {
    #[must_use]
    pub fn new(field: impl Into<String>, code: FieldErrorCode, detail: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code,
            detail: detail.into(),
        }
    }
}

/// One segment of a path into a JSON document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Key(String),
    Index(usize),
}

/// Builds the dotted/indexed paths that make a nested failure addressable.
/// `owners[0].id` tells a client exactly which value to fix; `owners` does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPath(Vec<Segment>);

impl FieldPath {
    #[must_use]
    pub fn root() -> Self {
        Self(Vec::new())
    }

    #[must_use]
    pub fn key(&self, key: impl Into<String>) -> Self {
        let mut segments = self.0.clone();
        segments.push(Segment::Key(key.into()));
        Self(segments)
    }

    #[must_use]
    pub fn index(&self, index: usize) -> Self {
        let mut segments = self.0.clone();
        segments.push(Segment::Index(index));
        Self(segments)
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut rendered = String::new();
        for segment in &self.0 {
            match segment {
                // A key after anything is dot-separated; the first key is not,
                // so the root object's fields render as `name`, never `.name`.
                Segment::Key(key) => {
                    if !rendered.is_empty() {
                        rendered.push('.');
                    }
                    rendered.push_str(key);
                }
                Segment::Index(index) => {
                    rendered.push('[');
                    rendered.push_str(&index.to_string());
                    rendered.push(']');
                }
            }
        }
        rendered
    }
}

/// Validates a required, non-empty string at `path`, pushing any failure onto
/// `errors` rather than returning early.
pub fn require_non_empty_string(value: &Value, path: &FieldPath, errors: &mut Vec<FieldError>) {
    let field = path.render();
    let Some(found) = lookup(value, path) else {
        errors.push(FieldError::new(
            field.clone(),
            FieldErrorCode::Required,
            format!("`{field}` is required"),
        ));
        return;
    };
    match found {
        Value::Null => errors.push(FieldError::new(
            field.clone(),
            FieldErrorCode::Required,
            format!("`{field}` is required"),
        )),
        Value::String(text) if text.trim().is_empty() => errors.push(FieldError::new(
            field.clone(),
            FieldErrorCode::Empty,
            format!("`{field}` must not be empty"),
        )),
        Value::String(_) => {}
        other => errors.push(FieldError::new(
            field.clone(),
            FieldErrorCode::Type,
            format!("`{field}` must be a string, got {}", type_name(other)),
        )),
    }
}

/// Validates an optional string: absent and null are both fine, a non-string is not.
pub fn optional_string(value: &Value, path: &FieldPath, errors: &mut Vec<FieldError>) {
    let field = path.render();
    match lookup(value, path) {
        None | Some(Value::Null | Value::String(_)) => {}
        Some(other) => errors.push(FieldError::new(
            field.clone(),
            FieldErrorCode::Type,
            format!("`{field}` must be a string, got {}", type_name(other)),
        )),
    }
}

fn lookup<'a>(value: &'a Value, path: &FieldPath) -> Option<&'a Value> {
    let mut current = value;
    for segment in &path.0 {
        current = match segment {
            Segment::Key(key) => current.get(key)?,
            Segment::Index(index) => current.get(index)?,
        };
    }
    Some(current)
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Implemented by every request DTO that crosses the HTTP boundary.
pub trait ValidateBody {
    /// Returns **all** violations, never just the first.
    fn validate_body(value: &Value) -> Vec<FieldError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn root_field_renders_without_a_leading_dot() {
        assert_eq!(FieldPath::root().key("name").render(), "name");
    }

    #[test]
    fn nested_fields_are_dot_separated() {
        assert_eq!(
            FieldPath::root().key("owner").key("id").render(),
            "owner.id"
        );
    }

    #[test]
    fn indexed_fields_render_with_brackets_and_no_dot_before_them() {
        assert_eq!(
            FieldPath::root().key("owners").index(0).key("id").render(),
            "owners[0].id"
        );
    }

    #[test]
    fn deeply_nested_indices_chain() {
        assert_eq!(
            FieldPath::root()
                .key("columns")
                .index(2)
                .key("tags")
                .index(11)
                .render(),
            "columns[2].tags[11]"
        );
    }

    #[test]
    fn a_missing_field_reports_required() {
        let mut errors = Vec::new();
        require_non_empty_string(&json!({}), &FieldPath::root().key("name"), &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, FieldErrorCode::Required);
        assert_eq!(errors[0].field, "name");
    }

    #[test]
    fn a_blank_field_reports_empty_not_required() {
        let mut errors = Vec::new();
        require_non_empty_string(
            &json!({ "name": "   " }),
            &FieldPath::root().key("name"),
            &mut errors,
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].code,
            FieldErrorCode::Empty,
            "whitespace is blank; a client that sent spaces needs a different fix \
             from one that sent nothing"
        );
    }

    #[test]
    fn a_wrong_typed_field_reports_type_and_names_what_it_got() {
        let mut errors = Vec::new();
        require_non_empty_string(
            &json!({ "name": 42 }),
            &FieldPath::root().key("name"),
            &mut errors,
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, FieldErrorCode::Type);
        assert!(errors[0].detail.contains("number"), "{:?}", errors[0]);
    }

    #[test]
    fn a_valid_field_reports_nothing() {
        let mut errors = Vec::new();
        require_non_empty_string(
            &json!({ "name": "orders" }),
            &FieldPath::root().key("name"),
            &mut errors,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn violations_accumulate_across_fields() {
        let value = json!({ "fully_qualified_name": "" });
        let mut errors = Vec::new();
        require_non_empty_string(&value, &FieldPath::root().key("name"), &mut errors);
        require_non_empty_string(
            &value,
            &FieldPath::root().key("fully_qualified_name"),
            &mut errors,
        );
        assert_eq!(errors.len(), 2, "accumulating, not short-circuiting");
    }

    #[test]
    fn an_absent_optional_string_is_fine_but_a_wrong_typed_one_is_not() {
        let mut errors = Vec::new();
        optional_string(
            &json!({}),
            &FieldPath::root().key("description"),
            &mut errors,
        );
        optional_string(
            &json!({ "description": null }),
            &FieldPath::root().key("description"),
            &mut errors,
        );
        assert!(errors.is_empty());

        optional_string(
            &json!({ "description": true }),
            &FieldPath::root().key("description"),
            &mut errors,
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, FieldErrorCode::Type);
    }
}
