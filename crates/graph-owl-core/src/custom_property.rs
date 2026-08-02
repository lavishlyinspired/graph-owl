//! Organization-defined fields on entity types — Epic 22.
//!
//! Every organization has fields the catalog's authors did not anticipate:
//! `costCenter`, `retentionDays`, `sourceOfTruth`. Without a supported
//! mechanism they end up written into the description as prose, which is
//! unsearchable, unvalidatable, and impossible to report on. This module is
//! the vocabulary and the validator; nothing here does I/O.
//!
//! **`extension` is a different field from `properties`, and the separation is
//! load-bearing.** `properties` is what the *source system* reported — a
//! column's data type, a service's engine — and a connector run replaces it
//! wholesale. `extension` is what the *organization* added. Putting custom
//! properties in `properties` would mean the next connector run silently wiped
//! every hand-curated `costCenter`, which is exactly the class of silent data
//! loss this codebase refuses everywhere else.
//!
//! **A supported type set, not arbitrary JSON Schema** (decision 4). Arbitrary
//! schema makes validation, indexing and filtering unbounded problems, and the
//! set below can grow additively when something real needs it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The types a custom property may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropertyType {
    String,
    Integer,
    Number,
    Boolean,
    Date,
    Timestamp,
    Enum,
    /// An FQN naming another catalog entity. Validated for *existence* by the
    /// facade — this module has no catalog to ask.
    EntityReference,
}

impl PropertyType {
    /// The wire spelling, used in error messages so a client is told the same
    /// name it would send.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PropertyType::String => "string",
            PropertyType::Integer => "integer",
            PropertyType::Number => "number",
            PropertyType::Boolean => "boolean",
            PropertyType::Date => "date",
            PropertyType::Timestamp => "timestamp",
            PropertyType::Enum => "enum",
            PropertyType::EntityReference => "entityReference",
        }
    }

    /// Every supported type, for the "unsupported type" error to list.
    ///
    /// A client told only "unsupported" has to go and find the documentation;
    /// one told what *is* supported can fix the request from the response.
    #[must_use]
    pub const fn all() -> &'static [PropertyType] {
        &[
            PropertyType::String,
            PropertyType::Integer,
            PropertyType::Number,
            PropertyType::Boolean,
            PropertyType::Date,
            PropertyType::Timestamp,
            PropertyType::Enum,
            PropertyType::EntityReference,
        ]
    }

    /// Parses a wire name.
    ///
    /// # Errors
    ///
    /// The unrecognised name, so the caller can name it back.
    pub fn parse(name: &str) -> Result<Self, String> {
        PropertyType::all()
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == name)
            .ok_or_else(|| name.to_string())
    }
}

/// Bounds on a property's values.
///
/// All optional: a property with no constraints is a perfectly ordinary
/// property, and requiring bounds would make the common case ceremonial.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constraints {
    /// Inclusive lower bound for `Integer` and `Number`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    /// Inclusive upper bound for `Integer` and `Number`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    /// The permitted values for an `Enum`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// The entity kind an `EntityReference` must point at, if it is restricted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
}

/// One organization-defined field on one entity type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProperty {
    pub name: String,
    /// Which entity type it applies to. **Per type** (decision 2):
    /// `costCenter` on a table need not exist on a user, and a globally-scoped
    /// vocabulary would force every organization's fields onto every entity.
    pub entity_type: String,
    pub property_type: PropertyType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub constraints: Constraints,
}

/// Field names a custom property may not take.
///
/// **A custom `description` would shadow the real field**, and every client
/// reading `description` would then get one of two different values depending
/// on which layer answered. Rejecting the definition is the only point at which
/// this is cheap — once values exist, every fix is a migration.
pub const RESERVED_NAMES: &[&str] = &[
    "id",
    "kind",
    "name",
    "fullyQualifiedName",
    "parentId",
    "description",
    "properties",
    "owners",
    "version",
    "updatedBy",
    "changeDescription",
    "deleted",
    "deletedAt",
    "createdAt",
    "updatedAt",
    "tags",
    "extension",
];

/// Why a definition was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionError {
    /// The name would shadow a built-in envelope field.
    Reserved(String),
    /// The name is empty or not a usable identifier.
    Name(String),
    /// An enum with no values can never be satisfied.
    EnumWithoutValues,
    /// Constraints that no value can satisfy.
    Impossible(String),
}

impl std::fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefinitionError::Reserved(name) => write!(
                f,
                "`{name}` is a built-in field; a custom property with that name would shadow it"
            ),
            DefinitionError::EnumWithoutValues => write!(
                f,
                "an enum property needs at least one permitted value; \
                 with none, no value could ever be valid"
            ),
            // Two variants, one rendering: both already carry a full sentence.
            // They stay separate variants because a caller may want to branch
            // on "the name is wrong" versus "the bounds are wrong", which the
            // rendered string cannot support.
            DefinitionError::Name(detail) | DefinitionError::Impossible(detail) => {
                write!(f, "{detail}")
            }
        }
    }
}

impl CustomProperty {
    /// Whether this definition can exist at all.
    ///
    /// Separate from value validation because the two fail at different times
    /// and for different people: this is the metadata modeller's mistake, and
    /// catching it here means no value ever has to be migrated out of it.
    ///
    /// # Errors
    ///
    /// [`DefinitionError`] naming what is wrong with the definition.
    pub fn validate(&self) -> Result<(), DefinitionError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(DefinitionError::Name(
                "a custom property needs a name".to_string(),
            ));
        }
        // Compared case-insensitively: `Description` shadows `description` just
        // as effectively, and a client that got one past this check would have
        // found a way to break every reader of the field.
        if RESERVED_NAMES
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(name))
        {
            return Err(DefinitionError::Reserved(name.to_string()));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(DefinitionError::Name(format!(
                "`{name}` must be letters, digits, `_` or `-` — it becomes a \
                 query parameter and a JSON key"
            )));
        }

        if self.property_type == PropertyType::Enum && self.constraints.values.is_empty() {
            return Err(DefinitionError::EnumWithoutValues);
        }

        if let (Some(min), Some(max)) = (self.constraints.minimum, self.constraints.maximum)
            && min > max
        {
            return Err(DefinitionError::Impossible(format!(
                "minimum {min} is above maximum {max}, so no value could satisfy both"
            )));
        }
        if let (Some(min), Some(max)) = (self.constraints.min_length, self.constraints.max_length)
            && min > max
        {
            return Err(DefinitionError::Impossible(format!(
                "minLength {min} is above maxLength {max}, so no value could satisfy both"
            )));
        }

        Ok(())
    }
}

/// Why a value was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// No definition by that name on this entity type.
    Undefined { name: String, entity_type: String },
    /// The value is not of the declared type.
    WrongType {
        name: String,
        expected: String,
        found: String,
    },
    /// The value is of the right type but outside its bounds.
    Constraint { name: String, detail: String },
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValueError::Undefined { name, entity_type } => write!(
                f,
                "`{name}` is not a custom property defined on `{entity_type}`"
            ),
            // **Both types named.** "wrong type" alone makes a client guess
            // which of the two it got wrong.
            ValueError::WrongType {
                name,
                expected,
                found,
            } => write!(f, "`{name}` expects {expected}, got {found}"),
            ValueError::Constraint { name, detail } => write!(f, "`{name}`: {detail}"),
        }
    }
}

/// The JSON type name of a value, for the "got" half of a type error.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) => {
            if number.is_f64() {
                "number"
            } else {
                "integer"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Whether one value satisfies one definition.
///
/// **A pure function of `(definition, value)`**, which is why it lives in
/// `core` rather than in the facade: it is domain knowledge, it is exhaustively
/// testable without I/O, and it mutates several times faster here than it would
/// in the server crate.
///
/// `null` is *not* checked here — clearing a value is a legitimate operation
/// and the caller decides whether it is allowed, exactly as `Option` fields
/// work elsewhere in the envelope.
///
/// # Errors
///
/// [`ValueError`] naming the property and what was wrong with the value.
pub fn validate_value(definition: &CustomProperty, value: &Value) -> Result<(), ValueError> {
    let name = definition.name.clone();
    let wrong = |found: &Value| ValueError::WrongType {
        name: name.clone(),
        expected: definition.property_type.as_str().to_string(),
        found: type_name(found).to_string(),
    };

    match definition.property_type {
        PropertyType::String => {
            let text = value.as_str().ok_or_else(|| wrong(value))?;
            check_length(definition, text)?;
        }
        PropertyType::Integer => {
            // `as_i64` rather than `as_f64`: 1.5 into an integer property is a
            // type error, and a float that happens to be whole is accepted
            // because JSON has no way to write `1` as anything else.
            let number = value
                .as_i64()
                .map(|n| {
                    #[allow(clippy::cast_precision_loss)]
                    {
                        n as f64
                    }
                })
                .or_else(|| value.as_f64().filter(|n| n.fract() == 0.0))
                .ok_or_else(|| wrong(value))?;
            check_range(definition, number)?;
        }
        PropertyType::Number => {
            let number = value.as_f64().ok_or_else(|| wrong(value))?;
            check_range(definition, number)?;
        }
        PropertyType::Boolean => {
            if !value.is_boolean() {
                return Err(wrong(value));
            }
        }
        PropertyType::Date => {
            let text = value.as_str().ok_or_else(|| wrong(value))?;
            if chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_err() {
                return Err(ValueError::Constraint {
                    name,
                    detail: format!("`{text}` is not a date in YYYY-MM-DD form"),
                });
            }
        }
        PropertyType::Timestamp => {
            let text = value.as_str().ok_or_else(|| wrong(value))?;
            if chrono::DateTime::parse_from_rfc3339(text).is_err() {
                return Err(ValueError::Constraint {
                    name,
                    detail: format!("`{text}` is not an RFC 3339 timestamp"),
                });
            }
        }
        PropertyType::Enum => {
            let text = value.as_str().ok_or_else(|| wrong(value))?;
            if !definition
                .constraints
                .values
                .iter()
                .any(|allowed| allowed == text)
            {
                // **The valid values are listed.** A client told only
                // "invalid" has to go and read the definition; one told the
                // options can fix the request from the response.
                return Err(ValueError::Constraint {
                    name,
                    detail: format!(
                        "`{text}` is not one of: {}",
                        definition.constraints.values.join(", ")
                    ),
                });
            }
        }
        PropertyType::EntityReference => {
            let text = value.as_str().ok_or_else(|| wrong(value))?;
            if text.trim().is_empty() {
                return Err(ValueError::Constraint {
                    name,
                    detail: "an entity reference needs a fully qualified name".to_string(),
                });
            }
        }
    }

    Ok(())
}

fn check_range(definition: &CustomProperty, number: f64) -> Result<(), ValueError> {
    if let Some(minimum) = definition.constraints.minimum
        && number < minimum
    {
        return Err(ValueError::Constraint {
            name: definition.name.clone(),
            detail: format!("{number} is below the minimum of {minimum}"),
        });
    }
    if let Some(maximum) = definition.constraints.maximum
        && number > maximum
    {
        return Err(ValueError::Constraint {
            name: definition.name.clone(),
            detail: format!("{number} is above the maximum of {maximum}"),
        });
    }
    Ok(())
}

fn check_length(definition: &CustomProperty, text: &str) -> Result<(), ValueError> {
    // **Characters, not bytes.** A `maxLength` of 10 that rejected a
    // ten-character name containing an accent would be enforcing a rule nobody
    // wrote, and only for some organizations' data.
    let length = text.chars().count();
    if let Some(minimum) = definition.constraints.min_length
        && length < minimum
    {
        return Err(ValueError::Constraint {
            name: definition.name.clone(),
            detail: format!("is {length} characters, below the minimum of {minimum}"),
        });
    }
    if let Some(maximum) = definition.constraints.max_length
        && length > maximum
    {
        return Err(ValueError::Constraint {
            name: definition.name.clone(),
            detail: format!("is {length} characters, above the maximum of {maximum}"),
        });
    }
    Ok(())
}

/// Validate a whole `extension` bag against the definitions for its type.
///
/// Returns **every** failure, not the first: a client fixing one field per
/// round trip is the cost this codebase's accumulating validators exist to
/// avoid, and a bag with four bad values is a realistic first attempt.
///
/// A `null` clears the property and is always allowed — consistent with Epic
/// 3's PATCH semantics, where omitting leaves a field unchanged and null
/// clears it.
///
/// # Errors
///
/// Every [`ValueError`] the bag produced.
pub fn validate_extension(
    definitions: &[CustomProperty],
    entity_type: &str,
    extension: &serde_json::Map<String, Value>,
) -> Result<(), Vec<ValueError>> {
    let mut errors = Vec::new();

    for (name, value) in extension {
        if value.is_null() {
            continue;
        }
        match definitions
            .iter()
            .find(|definition| &definition.name == name && definition.entity_type == entity_type)
        {
            // **Refused, never stored untyped.** An undefined name silently
            // accepted is the whole failure this epic exists to prevent — it is
            // the description field again, with extra steps.
            None => errors.push(ValueError::Undefined {
                name: name.clone(),
                entity_type: entity_type.to_string(),
            }),
            Some(definition) => {
                if let Err(error) = validate_value(definition, value) {
                    errors.push(error);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Whether a definition change can strand a value that already exists — Epic
/// 22 Slice C.
///
/// **A change to the description or the display name cannot**, so saying so is
/// what keeps editing help text from becoming an O(estate) read. Everything
/// that moves what a value must *satisfy* can, and is checked against the data
/// rather than reasoned about.
#[must_use]
pub fn constrains_differently(before: &CustomProperty, after: &CustomProperty) -> bool {
    before.property_type != after.property_type || before.constraints != after.constraints
}

/// How many of `values` the changed definition would no longer accept.
///
/// **One rule, not a classification table.** The four cases the plan lists —
/// type change, constraint narrowing, enum-member removal, and the widenings
/// that are always fine — are tempting to encode as predicates over the *shape*
/// of the change. That table has to be right for every combination of bound,
/// type and enum member, and the first combination it gets wrong silently
/// orphans data.
///
/// So the change is applied and [`validate_value`] — the **write path's own
/// validator** — is re-run over what exists. A widening admits everything it
/// did before and strands nothing; a narrowing that orphans values reports how
/// many. It cannot disagree with what a write would do, because it is the same
/// function, and no case can be forgotten because there are no cases.
#[must_use]
pub fn stranded_by(after: &CustomProperty, values: &[Value]) -> usize {
    values
        .iter()
        .filter(|value| validate_value(after, value).is_err())
        .count()
}

#[cfg(test)]
mod change_safety_tests {
    use super::*;

    fn integer(minimum: f64, maximum: f64) -> CustomProperty {
        CustomProperty {
            name: "retentionDays".to_string(),
            entity_type: "table".to_string(),
            property_type: PropertyType::Integer,
            description: None,
            constraints: Constraints {
                minimum: Some(minimum),
                maximum: Some(maximum),
                ..Constraints::default()
            },
        }
    }

    fn choices(values: &[&str]) -> CustomProperty {
        CustomProperty {
            name: "tier".to_string(),
            entity_type: "table".to_string(),
            property_type: PropertyType::Enum,
            description: None,
            constraints: Constraints {
                values: values.iter().map(|v| (*v).to_string()).collect(),
                ..Constraints::default()
            },
        }
    }

    /// Editing help text is not a schema change, and proving anything about the
    /// values for it would make renaming a description cost a scan.
    #[test]
    fn a_description_edit_does_not_constrain_differently() {
        let before = integer(1.0, 90.0);
        let mut after = before.clone();
        after.description = Some("how long we keep it".to_string());

        assert!(!constrains_differently(&before, &after));
    }

    /// Both halves of the condition, separately — an `&&` in place of the `||`
    /// would let a type change through unchecked whenever the constraints
    /// happened to match.
    #[test]
    fn a_type_change_and_a_constraint_change_each_constrain_differently() {
        let before = integer(1.0, 90.0);

        let mut retyped = before.clone();
        retyped.property_type = PropertyType::String;
        assert!(
            constrains_differently(&before, &retyped),
            "a type change alone must be checked"
        );

        assert!(
            constrains_differently(&before, &integer(1.0, 30.0)),
            "a constraint change alone must be checked"
        );
    }

    /// **Widening strands nothing**, which is the half that proves the check is
    /// looking at the data rather than at the diff.
    #[test]
    fn widening_a_bound_strands_nothing() {
        let values = vec![json_int(30), json_int(80)];

        assert_eq!(stranded_by(&integer(1.0, 365.0), &values), 0);
    }

    /// And narrowing strands exactly the values past the new bound — not all of
    /// them, and not none.
    #[test]
    fn narrowing_a_bound_strands_only_the_values_past_it() {
        let values = vec![json_int(30), json_int(200), json_int(400)];

        assert_eq!(stranded_by(&integer(1.0, 90.0), &values), 2);
    }

    /// Removing an enum member in use strands its holders; removing an unused
    /// one strands nobody. The pair is the case a shape-based rule cannot tell
    /// apart at all.
    #[test]
    fn removing_an_enum_value_strands_only_its_holders() {
        let values = vec![
            Value::String("gold".to_string()),
            Value::String("gold".to_string()),
            Value::String("silver".to_string()),
        ];

        assert_eq!(
            stranded_by(&choices(&["silver"]), &values),
            2,
            "both gold holders are stranded"
        );
        assert_eq!(
            stranded_by(&choices(&["gold", "silver"]), &values),
            0,
            "removing an unused member strands nobody"
        );
    }

    /// Retyping strands every value that is not of the new type — the case the
    /// `409` reports a count for.
    #[test]
    fn retyping_strands_the_values_that_are_not_of_the_new_type() {
        let mut retyped = integer(1.0, 365.0);
        retyped.property_type = PropertyType::Boolean;
        retyped.constraints = Constraints::default();

        assert_eq!(stranded_by(&retyped, &[json_int(30), json_int(80)]), 2);
    }

    #[test]
    fn no_values_strand_nothing() {
        assert_eq!(stranded_by(&integer(1.0, 1.0), &[]), 0);
    }

    fn json_int(value: i64) -> Value {
        Value::Number(value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn definition(property_type: PropertyType) -> CustomProperty {
        CustomProperty {
            name: "costCenter".to_string(),
            entity_type: "table".to_string(),
            property_type,
            description: None,
            constraints: Constraints::default(),
        }
    }

    // ── the definition itself ──────────────────────────────────────────

    /// **The important one.** A custom `description` would shadow the real
    /// field, and every reader would get one of two values depending on which
    /// layer answered.
    #[test]
    fn a_name_colliding_with_a_built_in_field_is_refused() {
        for reserved in ["description", "name", "owners", "id"] {
            let mut property = definition(PropertyType::String);
            property.name = reserved.to_string();

            assert_eq!(
                property.validate(),
                Err(DefinitionError::Reserved(reserved.to_string())),
                "`{reserved}` must not be definable"
            );
        }
    }

    /// `Description` shadows `description` just as effectively.
    #[test]
    fn the_reserved_check_is_case_insensitive() {
        let mut property = definition(PropertyType::String);
        property.name = "Description".to_string();

        assert!(property.validate().is_err());
    }

    /// **The negative half.** A check that refused everything would pass the
    /// test above and make the feature unusable.
    #[test]
    fn an_ordinary_name_is_accepted() {
        assert_eq!(definition(PropertyType::String).validate(), Ok(()));
    }

    #[test]
    fn an_enum_without_values_is_refused() {
        assert_eq!(
            definition(PropertyType::Enum).validate(),
            Err(DefinitionError::EnumWithoutValues),
            "no value could ever satisfy it"
        );
    }

    #[test]
    fn an_enum_with_values_is_accepted() {
        let mut property = definition(PropertyType::Enum);
        property.constraints.values = vec!["gold".to_string(), "silver".to_string()];

        assert_eq!(property.validate(), Ok(()));
    }

    /// Bounds that cross admit no value at all, and the failure would show up
    /// as every write being rejected for a reason naming the value rather than
    /// the definition.
    #[test]
    fn constraints_that_no_value_could_satisfy_are_refused() {
        let mut property = definition(PropertyType::Integer);
        property.constraints.minimum = Some(10.0);
        property.constraints.maximum = Some(5.0);

        assert!(matches!(
            property.validate(),
            Err(DefinitionError::Impossible(_))
        ));
    }

    #[test]
    fn a_min_equal_to_its_max_is_allowed() {
        let mut property = definition(PropertyType::Integer);
        property.constraints.minimum = Some(5.0);
        property.constraints.maximum = Some(5.0);

        assert_eq!(property.validate(), Ok(()), "exactly 5 is satisfiable");
    }

    #[test]
    fn a_name_that_is_not_a_usable_key_is_refused() {
        let mut property = definition(PropertyType::String);
        property.name = "cost center!".to_string();

        assert!(matches!(property.validate(), Err(DefinitionError::Name(_))));
    }

    #[test]
    fn an_empty_name_is_refused() {
        let mut property = definition(PropertyType::String);
        property.name = "   ".to_string();

        assert!(matches!(property.validate(), Err(DefinitionError::Name(_))));
    }

    // ── types ──────────────────────────────────────────────────────────

    #[test]
    fn every_supported_type_round_trips_through_its_wire_name() {
        for property_type in PropertyType::all() {
            assert_eq!(
                PropertyType::parse(property_type.as_str()),
                Ok(*property_type)
            );
        }
    }

    #[test]
    fn an_unsupported_type_is_refused_by_name() {
        assert_eq!(
            PropertyType::parse("geolocation"),
            Err("geolocation".into())
        );
    }

    // ── values ─────────────────────────────────────────────────────────

    /// Table-driven over every type, both directions. **The negative rows are
    /// what a mutation survives without** — validation that checks presence but
    /// not type passes every positive row.
    #[test]
    fn each_type_accepts_its_own_values_and_refuses_others() {
        let cases: &[(PropertyType, Value, Value)] = &[
            (PropertyType::String, json!("CC-1"), json!(7)),
            (PropertyType::Integer, json!(30), json!("thirty")),
            (PropertyType::Number, json!(1.5), json!("1.5")),
            (PropertyType::Boolean, json!(true), json!("true")),
            (PropertyType::Date, json!("2026-08-02"), json!(20_260_802)),
            (
                PropertyType::Timestamp,
                json!("2026-08-02T00:00:00Z"),
                json!(true),
            ),
            (
                PropertyType::EntityReference,
                json!("warehouse.public.orders"),
                json!(42),
            ),
        ];

        for (property_type, good, bad) in cases {
            let property = definition(*property_type);
            assert_eq!(
                validate_value(&property, good),
                Ok(()),
                "{property_type:?} should accept {good}"
            );
            assert!(
                validate_value(&property, bad).is_err(),
                "{property_type:?} should refuse {bad}"
            );
        }
    }

    /// **Both types named**, because "wrong type" alone makes a client guess
    /// which of the two it got wrong.
    #[test]
    fn a_type_error_names_the_property_and_both_types() {
        let error = validate_value(&definition(PropertyType::Integer), &json!("thirty"))
            .expect_err("a string is not an integer");

        let rendered = error.to_string();
        assert!(rendered.contains("costCenter"), "{rendered}");
        assert!(rendered.contains("integer"), "{rendered}");
        assert!(rendered.contains("string"), "{rendered}");
    }

    /// A float into an integer property is a type error. Accepting it would
    /// store 1.5 in a field every reader treats as whole.
    #[test]
    fn a_fractional_number_is_not_an_integer() {
        assert!(validate_value(&definition(PropertyType::Integer), &json!(1.5)).is_err());
    }

    /// But `30.0` is how JSON may spell 30, and refusing it would reject a
    /// value the client had no other way to send.
    #[test]
    fn a_whole_float_is_accepted_as_an_integer() {
        assert_eq!(
            validate_value(&definition(PropertyType::Integer), &json!(30.0)),
            Ok(())
        );
    }

    #[test]
    fn an_enum_value_outside_the_list_is_refused_and_the_options_are_listed() {
        let mut property = definition(PropertyType::Enum);
        property.constraints.values = vec!["gold".to_string(), "silver".to_string()];

        let error = validate_value(&property, &json!("bronze")).expect_err("not permitted");

        let rendered = error.to_string();
        assert!(rendered.contains("gold"), "{rendered}");
        assert!(rendered.contains("silver"), "{rendered}");
    }

    #[test]
    fn a_value_below_the_minimum_is_refused_and_the_bound_is_named() {
        let mut property = definition(PropertyType::Integer);
        property.constraints.minimum = Some(1.0);

        let error = validate_value(&property, &json!(0)).expect_err("below the minimum");

        assert!(error.to_string().contains('1'), "{error}");
    }

    /// **Boundary, both sides.** `>` for `>=` is the classic mutation, and only
    /// a test at exactly the bound catches it.
    #[test]
    fn a_value_exactly_on_the_bound_is_accepted() {
        let mut property = definition(PropertyType::Integer);
        property.constraints.minimum = Some(1.0);
        property.constraints.maximum = Some(10.0);

        assert_eq!(validate_value(&property, &json!(1)), Ok(()), "the minimum");
        assert_eq!(validate_value(&property, &json!(10)), Ok(()), "the maximum");
        assert!(validate_value(&property, &json!(0)).is_err(), "just below");
        assert!(validate_value(&property, &json!(11)).is_err(), "just above");
    }

    /// Length is in characters. A `maxLength` of 4 rejecting `café` would be
    /// enforcing a rule nobody wrote, and only for some organizations' data.
    #[test]
    fn length_is_counted_in_characters_not_bytes() {
        let mut property = definition(PropertyType::String);
        property.constraints.max_length = Some(4);

        assert_eq!(
            validate_value(&property, &json!("café")),
            Ok(()),
            "four characters, five bytes"
        );
        assert!(validate_value(&property, &json!("cafés")).is_err());
    }

    #[test]
    fn a_malformed_date_is_refused() {
        assert!(validate_value(&definition(PropertyType::Date), &json!("02-08-2026")).is_err());
        assert!(validate_value(&definition(PropertyType::Date), &json!("2026-13-01")).is_err());
    }

    #[test]
    fn a_timestamp_without_an_offset_is_refused() {
        // RFC 3339 requires one, and a timestamp whose zone is a guess is worse
        // than one that was rejected.
        assert!(
            validate_value(
                &definition(PropertyType::Timestamp),
                &json!("2026-08-02T00:00:00")
            )
            .is_err()
        );
    }

    // ── the whole bag ──────────────────────────────────────────────────

    #[test]
    fn an_undefined_property_name_is_refused_rather_than_stored() {
        let bag = json!({ "notAThing": "value" });

        let errors = validate_extension(
            &[definition(PropertyType::String)],
            "table",
            bag.as_object().expect("an object"),
        )
        .expect_err("undefined");

        assert!(matches!(errors[0], ValueError::Undefined { .. }));
    }

    /// **Definitions are per entity type** (decision 2). A property defined on
    /// `table` is undefined on `user`, and accepting it there would make the
    /// scoping decorative.
    #[test]
    fn a_property_defined_on_another_entity_type_is_undefined_here() {
        let bag = json!({ "costCenter": "CC-1" });

        let errors = validate_extension(
            &[definition(PropertyType::String)],
            "user",
            bag.as_object().expect("an object"),
        )
        .expect_err("not defined on user");

        assert!(matches!(errors[0], ValueError::Undefined { .. }));
    }

    /// **Every failure, not the first.** A bag with four bad values is a
    /// realistic first attempt, and one fix per round trip is the cost this
    /// codebase's accumulating validators exist to avoid.
    #[test]
    fn every_bad_value_in_a_bag_is_reported_at_once() {
        let definitions = vec![
            definition(PropertyType::Integer),
            CustomProperty {
                name: "tier".to_string(),
                ..definition(PropertyType::Boolean)
            },
        ];
        let bag = json!({ "costCenter": "not a number", "tier": 7, "unknown": 1 });

        let errors = validate_extension(&definitions, "table", bag.as_object().expect("object"))
            .expect_err("three problems");

        assert_eq!(errors.len(), 3, "{errors:?}");
    }

    /// `null` clears a property and is always allowed — the same rule the rest
    /// of the envelope's PATCH semantics follow.
    #[test]
    fn a_null_clears_a_property_rather_than_failing_validation() {
        let bag = json!({ "costCenter": null });

        assert_eq!(
            validate_extension(
                &[definition(PropertyType::Integer)],
                "table",
                bag.as_object().expect("an object")
            ),
            Ok(())
        );
    }

    /// A null for a name nobody defined is still allowed: clearing something
    /// that is not there is a no-op, not an error, and rejecting it would make
    /// removing a property harder than adding one.
    #[test]
    fn a_null_for_an_undefined_property_is_a_no_op() {
        let bag = json!({ "neverDefined": null });

        assert_eq!(
            validate_extension(&[], "table", bag.as_object().expect("obj")),
            Ok(())
        );
    }

    #[test]
    fn an_empty_bag_is_valid() {
        assert_eq!(
            validate_extension(&[], "table", &serde_json::Map::new()),
            Ok(())
        );
    }

    #[test]
    fn a_definition_round_trips_through_json() {
        let mut original = definition(PropertyType::Enum);
        original.constraints.values = vec!["gold".to_string()];

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: CustomProperty = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, original);
    }

    /// The wire shape is camelCase like everything else on the envelope.
    #[test]
    fn the_wire_shape_is_camel_case() {
        let value =
            serde_json::to_value(definition(PropertyType::EntityReference)).expect("serialize");

        assert!(value.get("entityType").is_some(), "{value}");
        assert!(value.get("propertyType").is_some(), "{value}");
        assert_eq!(value["propertyType"], "entityReference");
        assert!(value.get("entity_type").is_none(), "{value}");
    }
}
