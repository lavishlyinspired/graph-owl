//! Closed-enum fields for Epic 34's new asset families.
//!
//! Family-specific data lives in [`crate::Asset::properties`] as free-form
//! JSON — there is no per-kind Rust struct, per the plan's own "no core
//! change" constraint (see `plans/34-entity-expansion.md`). A few fields are
//! *closed* enums the plan calls out explicitly ("chart types are a closed
//! enum"), and those still deserve the same round-trip guarantee every other
//! closed vocabulary in this crate has ([`crate::AssetKind`],
//! [`crate::lineage::LineageSource`]) rather than being validated as an
//! unstructured string wherever they happen to be read.

use serde::{Deserialize, Serialize};

/// A chart's visual form — Epic 34 Slice A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChartType {
    /// Categories on one axis, magnitude on the other.
    Bar,
    /// A trend over an ordered axis, usually time.
    Line,
    /// Parts of a whole.
    Pie,
    /// Two numeric variables against each other.
    Scatter,
    /// Rows and columns, no visual encoding.
    Table,
    /// One number, prominently.
    Number,
}

impl ChartType {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChartType::Bar => "bar",
            ChartType::Line => "line",
            ChartType::Pie => "pie",
            ChartType::Scatter => "scatter",
            ChartType::Table => "table",
            ChartType::Number => "number",
        }
    }

    /// # Errors
    /// The unrecognised value, so the caller can name it.
    pub fn parse(value: &str) -> Result<Self, String> {
        ChartType::ALL
            .into_iter()
            .find(|k| k.as_str() == value)
            .ok_or_else(|| value.to_string())
    }

    /// Every chart type.
    pub const ALL: [ChartType; 6] = [
        ChartType::Bar,
        ChartType::Line,
        ChartType::Pie,
        ChartType::Scatter,
        ChartType::Table,
        ChartType::Number,
    ];
}

/// A topic schema field's primitive type — Epic 34 Slice B.
///
/// The common ground across Avro, JSON Schema and Protobuf primitives,
/// deliberately not a superset of all three: a type only one of the three
/// serialization formats has (Avro's `fixed`, Protobuf's `sint32`) would make
/// this closed enum a lie about being closed the day a real schema needed
/// one, and a schema field's *catalog* value — is it findable, taggable,
/// classifiable as a breaking change — never depended on the distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SchemaFieldType {
    /// Text.
    String,
    /// A 32-bit signed integer.
    Int,
    /// A 64-bit signed integer.
    Long,
    /// A 32-bit floating point number.
    Float,
    /// A 64-bit floating point number.
    Double,
    /// True or false.
    Boolean,
    /// A raw byte sequence.
    Bytes,
    /// An instant in time.
    Timestamp,
}

impl SchemaFieldType {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SchemaFieldType::String => "string",
            SchemaFieldType::Int => "int",
            SchemaFieldType::Long => "long",
            SchemaFieldType::Float => "float",
            SchemaFieldType::Double => "double",
            SchemaFieldType::Boolean => "boolean",
            SchemaFieldType::Bytes => "bytes",
            SchemaFieldType::Timestamp => "timestamp",
        }
    }

    /// # Errors
    /// The unrecognised value, so the caller can name it.
    pub fn parse(value: &str) -> Result<Self, String> {
        SchemaFieldType::ALL
            .into_iter()
            .find(|t| t.as_str() == value)
            .ok_or_else(|| value.to_string())
    }

    /// Every schema field type.
    pub const ALL: [SchemaFieldType; 8] = [
        SchemaFieldType::String,
        SchemaFieldType::Int,
        SchemaFieldType::Long,
        SchemaFieldType::Float,
        SchemaFieldType::Double,
        SchemaFieldType::Boolean,
        SchemaFieldType::Bytes,
        SchemaFieldType::Timestamp,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_schema_field_type_round_trips() {
        for field_type in SchemaFieldType::ALL {
            assert_eq!(SchemaFieldType::parse(field_type.as_str()), Ok(field_type));
        }
    }

    #[test]
    fn an_unknown_schema_field_type_is_named_rather_than_defaulted() {
        assert_eq!(SchemaFieldType::parse("fixed"), Err("fixed".to_string()));
    }

    #[test]
    fn every_chart_type_round_trips() {
        for chart_type in ChartType::ALL {
            assert_eq!(ChartType::parse(chart_type.as_str()), Ok(chart_type));
        }
    }

    #[test]
    fn an_unknown_chart_type_is_named_rather_than_defaulted() {
        assert_eq!(ChartType::parse("sankey"), Err("sankey".to_string()));
    }

    /// Defaulting an unrecognised type to `Table` (the least visual of the
    /// six) would hide a typo behind a chart that renders, just wrong.
    #[test]
    fn chart_types_have_distinct_wire_forms() {
        let mut forms: Vec<&str> = ChartType::ALL.iter().map(|c| c.as_str()).collect();
        forms.sort_unstable();
        let before = forms.len();
        forms.dedup();
        assert_eq!(forms.len(), before);
    }
}
