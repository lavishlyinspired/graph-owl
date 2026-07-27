//! Mapping between [`FlakeValue`] and the flake table's value columns.
//!
//! Pure, and deliberately separate from the adapter's I/O: the encoding is a
//! storage contract that outlives any particular query, and it is the one part
//! of the adapter that can be tested exhaustively without a database.

use graph_owl_core::flake::{FlakeValue, Sid, value_type};

/// A deterministic text encoding of a value, used by the identity index and by
/// the POST ordering.
///
/// Only needs to be injective *within* a `value_type` — every index carrying
/// it also carries the discriminant, so `Int(1)` and `String("1")` are already
/// distinguished before the key is compared.
///
/// Floats are encoded with `{:?}` rather than `{}` because it round-trips
/// every `f64` including `NaN`, `inf` and `-inf`, and because it does not
/// collapse `1.0` and `1` into the same text — two different keys must not
/// share one.
#[must_use]
pub fn value_key(value: &FlakeValue) -> String {
    match value {
        FlakeValue::Ref(sid) => format!("{}:{}", sid.namespace_code, sid.id),
        FlakeValue::String(s) | FlakeValue::Json(s) => s.clone(),
        FlakeValue::Boolean(b) => b.to_string(),
        FlakeValue::Int(i) | FlakeValue::Duration(i) => i.to_string(),
        FlakeValue::Float(f) => format!("{f:?}"),
        // RFC 3339 at fixed precision, so the key sorts chronologically and
        // two instants that differ below microseconds -- which Postgres cannot
        // store apart anyway -- produce the same key rather than two rows the
        // database will then consider identical.
        FlakeValue::Instant(dt) => dt.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string(),
        FlakeValue::Bytes(bytes) => bytes.iter().fold(String::new(), |mut acc, byte| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{byte:02x}");
            acc
        }),
        FlakeValue::Uuid(uuid) => uuid.to_string(),
    }
}

/// The typed columns for a value. Every variant writes exactly one of them,
/// which is what lets a single table hold ten value shapes without a join.
#[derive(Debug)]
pub struct ValueColumns<'a> {
    pub value_type: i16,
    pub key: String,
    pub ref_ns: Option<i32>,
    pub ref_id: Option<&'a str>,
    pub str_value: Option<&'a str>,
    pub bool_value: Option<bool>,
    pub int_value: Option<i64>,
    pub float_value: Option<f64>,
    pub instant_at: Option<chrono::DateTime<chrono::Utc>>,
    pub json_value: Option<serde_json::Value>,
    pub bytes_value: Option<&'a [u8]>,
    pub uuid_value: Option<uuid::Uuid>,
}

/// Spreads a value across its columns.
///
/// # Errors
///
/// Returns the offending text if a [`FlakeValue::Json`] does not parse. Storing
/// it in a `JSONB` column would fail at the database anyway; failing here names
/// the value instead of surfacing a driver error about a bind parameter.
pub fn columns(value: &FlakeValue) -> Result<ValueColumns<'_>, String> {
    let base = ValueColumns {
        value_type: value.value_type(),
        key: value_key(value),
        ref_ns: None,
        ref_id: None,
        str_value: None,
        bool_value: None,
        int_value: None,
        float_value: None,
        instant_at: None,
        json_value: None,
        bytes_value: None,
        uuid_value: None,
    };

    Ok(match value {
        FlakeValue::Ref(sid) => ValueColumns {
            ref_ns: Some(i32::from(sid.namespace_code)),
            ref_id: Some(sid.id.as_str()),
            ..base
        },
        FlakeValue::String(s) => ValueColumns {
            str_value: Some(s.as_str()),
            ..base
        },
        FlakeValue::Boolean(b) => ValueColumns {
            bool_value: Some(*b),
            ..base
        },
        FlakeValue::Int(i) => ValueColumns {
            int_value: Some(*i),
            ..base
        },
        FlakeValue::Float(f) => ValueColumns {
            float_value: Some(*f),
            ..base
        },
        FlakeValue::Instant(dt) => ValueColumns {
            instant_at: Some(*dt),
            ..base
        },
        FlakeValue::Json(raw) => ValueColumns {
            json_value: Some(
                serde_json::from_str(raw).map_err(|e| format!("invalid JSON value: {e}"))?,
            ),
            ..base
        },
        FlakeValue::Bytes(bytes) => ValueColumns {
            bytes_value: Some(bytes.as_slice()),
            ..base
        },
        FlakeValue::Uuid(uuid) => ValueColumns {
            uuid_value: Some(*uuid),
            ..base
        },
        // Whole seconds in a BIGINT. Postgres INTERVAL would carry months,
        // which have no fixed length -- an SLA of "30 days" and one of "1
        // month" must not compare equal.
        FlakeValue::Duration(seconds) => ValueColumns {
            int_value: Some(*seconds),
            ..base
        },
    })
}

/// Reassembles a value from its columns.
///
/// # Errors
///
/// Returns a message if the discriminant is unknown or its column is NULL —
/// both mean the row was written by something that disagrees with this
/// encoding, which is worth failing loudly rather than defaulting.
#[allow(clippy::too_many_arguments)]
pub fn from_columns(
    value_type_code: i16,
    ref_ns: Option<i32>,
    ref_id: Option<String>,
    str_value: Option<String>,
    bool_value: Option<bool>,
    int_value: Option<i64>,
    float_value: Option<f64>,
    instant_at: Option<chrono::DateTime<chrono::Utc>>,
    json_value: Option<serde_json::Value>,
    bytes_value: Option<Vec<u8>>,
    uuid_value: Option<uuid::Uuid>,
) -> Result<FlakeValue, String> {
    fn required<T>(value: Option<T>, column: &str) -> Result<T, String> {
        value.ok_or_else(|| format!("flake row has NULL in {column} for its value_type"))
    }

    match value_type_code {
        value_type::REF => {
            let ns = required(ref_ns, "value_ref_ns")?;
            let id = required(ref_id, "value_ref_id")?;
            let ns = u16::try_from(ns).map_err(|_| format!("namespace {ns} is outside u16"))?;
            Ok(FlakeValue::Ref(Sid::new(ns, id)))
        }
        value_type::STRING => Ok(FlakeValue::String(required(str_value, "value_str")?)),
        value_type::BOOLEAN => Ok(FlakeValue::Boolean(required(bool_value, "value_bool")?)),
        value_type::INT => Ok(FlakeValue::Int(required(int_value, "value_int")?)),
        value_type::FLOAT => Ok(FlakeValue::Float(required(float_value, "value_float")?)),
        value_type::INSTANT => Ok(FlakeValue::Instant(required(instant_at, "value_inst")?)),
        value_type::JSON => Ok(FlakeValue::Json(
            required(json_value, "value_json")?.to_string(),
        )),
        value_type::BYTES => Ok(FlakeValue::Bytes(required(bytes_value, "value_bytes")?)),
        value_type::UUID => Ok(FlakeValue::Uuid(required(uuid_value, "value_uuid")?)),
        value_type::DURATION => Ok(FlakeValue::Duration(required(int_value, "value_int")?)),
        unknown => Err(format!(
            "value_type {unknown} is not in this build's vocabulary — the row \
             was written by a newer version"
        )),
    }
}

#[cfg(test)]
mod value_key_tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn every_variant() -> Vec<FlakeValue> {
        vec![
            FlakeValue::Ref(Sid::dsc("table-1")),
            FlakeValue::String("upi_transactions".into()),
            FlakeValue::Boolean(true),
            FlakeValue::Int(42),
            FlakeValue::Float(1.5),
            FlakeValue::Instant(Utc.timestamp_opt(1, 0).unwrap()),
            FlakeValue::Json("{\"a\":1}".into()),
            FlakeValue::Bytes(vec![0xde, 0xad]),
            FlakeValue::Uuid(Uuid::nil()),
            FlakeValue::Duration(3600),
        ]
    }

    /// The key only has to be injective within a `value_type`, but distinct
    /// values of the *same* type sharing a key would make two facts one row.
    #[test]
    fn distinct_values_of_one_type_get_distinct_keys() {
        assert_ne!(
            value_key(&FlakeValue::Int(1)),
            value_key(&FlakeValue::Int(2))
        );
        assert_ne!(
            value_key(&FlakeValue::String("a".into())),
            value_key(&FlakeValue::String("b".into()))
        );
        assert_ne!(
            value_key(&FlakeValue::Ref(Sid::dsc("a"))),
            value_key(&FlakeValue::Ref(Sid::dsc("b")))
        );
    }

    /// Two references with the same local name in different vocabularies are
    /// different nodes. A key built from the name alone would merge them.
    #[test]
    fn a_reference_key_carries_its_namespace() {
        use graph_owl_core::flake::namespace;
        assert_ne!(
            value_key(&FlakeValue::Ref(Sid::dsc("type"))),
            value_key(&FlakeValue::Ref(Sid::new(namespace::RDF, "type")))
        );
    }

    #[test]
    fn the_key_is_stable_across_calls() {
        for value in every_variant() {
            assert_eq!(
                value_key(&value),
                value_key(&value),
                "{value:?} is unstable"
            );
        }
    }

    /// `{}` formatting renders 1.0 as "1", colliding with `Int`-shaped text and
    /// losing the distinction between 1.0 and 1 within the float type itself.
    #[test]
    fn float_keys_keep_their_decimal_form() {
        assert_eq!(value_key(&FlakeValue::Float(1.0)), "1.0");
    }

    /// NaN is not equal to itself, so nothing else in the system can be relied
    /// on to deduplicate it. The key must at least be stable.
    #[test]
    fn non_finite_floats_have_stable_distinct_keys() {
        let nan = value_key(&FlakeValue::Float(f64::NAN));
        let inf = value_key(&FlakeValue::Float(f64::INFINITY));
        let neg_inf = value_key(&FlakeValue::Float(f64::NEG_INFINITY));

        assert_eq!(nan, value_key(&FlakeValue::Float(f64::NAN)));
        assert_ne!(inf, neg_inf, "the two infinities are different values");
        assert_ne!(nan, inf);
    }

    #[test]
    fn byte_keys_are_hex_and_pad_each_byte() {
        // Without the 0-pad, [0x0a, 0xbc] and [0xab, 0xc0] both render "abc".
        assert_eq!(value_key(&FlakeValue::Bytes(vec![0x0a, 0xbc])), "0abc");
        assert_ne!(
            value_key(&FlakeValue::Bytes(vec![0x0a, 0xbc])),
            value_key(&FlakeValue::Bytes(vec![0xab, 0xc0]))
        );
    }

    #[test]
    fn empty_bytes_key_to_the_empty_string() {
        assert_eq!(value_key(&FlakeValue::Bytes(vec![])), "");
    }

    /// Instant keys sort chronologically as text, which is what lets the POST
    /// index answer a range query on an instant-valued object.
    #[test]
    fn instant_keys_sort_in_time_order() {
        let earlier = value_key(&FlakeValue::Instant(Utc.timestamp_opt(1, 0).unwrap()));
        let later = value_key(&FlakeValue::Instant(Utc.timestamp_opt(2, 0).unwrap()));
        assert!(earlier < later, "{earlier} should sort before {later}");
    }
}

#[cfg(test)]
mod column_tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    /// The round trip that matters: what goes into the columns comes back as
    /// the same value. A discriminant that collapsed two variants, or a
    /// variant written to the wrong column, fails here.
    #[test]
    fn every_variant_round_trips_through_its_columns() {
        let values = vec![
            FlakeValue::Ref(Sid::dsc("table-1")),
            FlakeValue::String("upi_transactions".into()),
            FlakeValue::Boolean(false),
            FlakeValue::Int(-42),
            FlakeValue::Float(1.5),
            FlakeValue::Instant(Utc.timestamp_opt(1_700_000_000, 0).unwrap()),
            FlakeValue::Json("{\"a\":1}".into()),
            FlakeValue::Bytes(vec![0, 1, 255]),
            FlakeValue::Uuid(Uuid::from_u128(7)),
            FlakeValue::Duration(3600),
        ];

        for value in values {
            let c = columns(&value).expect("columns");
            let back = from_columns(
                c.value_type,
                c.ref_ns,
                c.ref_id.map(ToString::to_string),
                c.str_value.map(ToString::to_string),
                c.bool_value,
                c.int_value,
                c.float_value,
                c.instant_at,
                c.json_value,
                c.bytes_value.map(<[u8]>::to_vec),
                c.uuid_value,
            )
            .expect("round trip");
            assert_eq!(back, value, "{value:?} did not survive its columns");
        }
    }

    /// `Int` and `Duration` deliberately share `value_int`. Only the
    /// discriminant tells them apart, so a decode that ignored it would turn
    /// every duration into an integer.
    #[test]
    fn int_and_duration_share_a_column_and_are_told_apart_by_discriminant() {
        let duration = columns(&FlakeValue::Duration(60)).expect("columns");
        let int = columns(&FlakeValue::Int(60)).expect("columns");
        assert_eq!(duration.int_value, int.int_value);
        assert_ne!(duration.value_type, int.value_type);

        let decoded = from_columns(
            duration.value_type,
            None,
            None,
            None,
            None,
            duration.int_value,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("decodes");
        assert_eq!(decoded, FlakeValue::Duration(60));
    }

    /// Each variant writes exactly one column. Writing two would make the row
    /// ambiguous; writing none would lose the value entirely.
    #[test]
    fn each_variant_populates_exactly_one_value_column() {
        let cases = [
            FlakeValue::String("x".into()),
            FlakeValue::Boolean(true),
            FlakeValue::Int(1),
            FlakeValue::Float(1.0),
            FlakeValue::Instant(Utc.timestamp_opt(1, 0).unwrap()),
            FlakeValue::Json("{}".into()),
            FlakeValue::Bytes(vec![1]),
            FlakeValue::Uuid(Uuid::nil()),
            FlakeValue::Duration(1),
        ];
        for value in cases {
            let c = columns(&value).expect("columns");
            let populated = usize::from(c.str_value.is_some())
                + usize::from(c.bool_value.is_some())
                + usize::from(c.int_value.is_some())
                + usize::from(c.float_value.is_some())
                + usize::from(c.instant_at.is_some())
                + usize::from(c.json_value.is_some())
                + usize::from(c.bytes_value.is_some())
                + usize::from(c.uuid_value.is_some());
            assert_eq!(populated, 1, "{value:?} populated {populated} columns");
            assert!(c.ref_ns.is_none(), "{value:?} is not a reference");
        }
    }

    /// A reference fills both of its columns and none of the literal ones —
    /// the OPST index reads exactly this pair.
    #[test]
    fn a_reference_populates_both_of_its_columns() {
        let reference = FlakeValue::Ref(Sid::dsc("table-1"));
        let c = columns(&reference).expect("columns");
        assert_eq!(c.ref_ns, Some(1));
        assert_eq!(c.ref_id, Some("table-1"));
        assert!(c.str_value.is_none(), "a ref must not also be a string");
    }

    #[test]
    fn malformed_json_is_named_rather_than_left_to_the_driver() {
        let error = columns(&FlakeValue::Json("{not json".into())).expect_err("must reject");
        assert!(error.contains("JSON"), "got {error}");
    }

    /// A row whose discriminant this build does not know was written by a
    /// newer version. Guessing at it would silently corrupt the read.
    #[test]
    fn an_unknown_discriminant_is_refused() {
        let error = from_columns(
            99, None, None, None, None, None, None, None, None, None, None,
        )
        .expect_err("must refuse");
        assert!(
            error.contains("99"),
            "the error must name the code: {error}"
        );
    }

    /// A NULL in the column the discriminant points at is a corrupt row.
    /// Defaulting it would invent a fact that was never asserted.
    #[test]
    fn a_null_in_the_expected_column_is_refused_not_defaulted() {
        for code in [
            value_type::REF,
            value_type::STRING,
            value_type::BOOLEAN,
            value_type::INT,
            value_type::FLOAT,
            value_type::INSTANT,
            value_type::JSON,
            value_type::BYTES,
            value_type::UUID,
            value_type::DURATION,
        ] {
            assert!(
                from_columns(
                    code, None, None, None, None, None, None, None, None, None, None
                )
                .is_err(),
                "value_type {code} accepted an all-NULL row"
            );
        }
    }

    /// Namespaces are stored as INTEGER because u16 does not fit SMALLINT. A
    /// value outside u16 means the row was not written by this encoding.
    #[test]
    fn a_reference_namespace_outside_u16_is_refused() {
        let error = from_columns(
            value_type::REF,
            Some(70_000),
            Some("x".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect_err("must refuse");
        assert!(error.contains("70000"), "got {error}");
    }
}
