//! `PackStream`: the binary serialization format Bolt messages are framed in
//! — Epic 7d Slice A.
//!
//! **Implemented against the published spec, not a reference implementation**
//! (`00i` names it as an authorised source in its own right, the way W3C RDF
//! or ISO/IEC 39075 are for other surfaces). `PackStream`'s type system is
//! small enough that hand-rolling it is the right call rather than a
//! fallback — see `plans/00l-build-vs-adopt.md` for the crates that were
//! checked and why none replaced this.
//!
//! # Every marker picks the smallest representation that fits
//!
//! A value has exactly one canonical encoding — the smallest marker whose
//! range covers it — but [`decode`] accepts **any** width on the way in,
//! because a peer is free to (and sometimes must) use a wider one. Encoding
//! `200_i64` must never emit `INT_64` when `INT_16` fits; decoding `INT_64`
//! bytes that happen to hold `200` must still work. These are different
//! properties and the size-class boundary tests below check both
//! separately, at 15/16 and 255/256 and the equivalent signed boundaries —
//! the classic `PackStream` bug produces corrupt frames only at those specific
//! sizes, never in the middle of a class.
//!
//! # Truncation is not an error
//!
//! [`decode`] returns `Ok(None)` when the buffer does not yet hold a whole
//! value — Bolt reads off a stream, and "come back with more bytes" is a
//! normal outcome, not a malformed one. Only bytes that are actually invalid
//! (an unknown marker, a length prefix past `max_message_bytes`) return
//! `Err`.
//!
//! # The allocation guard runs before the allocation
//!
//! A length-prefixed type (string, list, map, bytes) declares its size before
//! its content arrives. Trusting that declaration to size a `Vec::with_capacity`
//! call lets two bytes on the wire claim a multi-gigabyte buffer before a
//! single content byte has been checked — the allocation itself is the
//! attack, not anything the parsed value could do afterward. So the
//! declared length is checked against `max_message_bytes` **before** any
//! allocation sized by it, for every length-prefixed variant, not only the
//! outermost one.

/// Why a buffer's bytes could not decode to a [`BoltValue`].
///
/// Never produced by ordinary truncation — see [`decode`]'s `Ok(None)` for
/// that. This is for bytes that are extra, not merely incomplete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackStreamError {
    #[error("byte 0x{0:02x} is not a PackStream marker")]
    UnknownMarker(u8),
    #[error("declared length {declared} exceeds the {limit}-byte message budget")]
    LengthExceedsBudget { declared: u64, limit: usize },
    #[error("a dictionary key must be a string, not this marker: 0x{0:02x}")]
    NonStringKey(u8),
    #[error("string bytes are not valid UTF-8")]
    InvalidUtf8,
}

/// One `PackStream` value.
///
/// **A `Dictionary` is a `Vec`, not a `HashMap`.** `PackStream` keys are always
/// strings, encoding order is not defined by the spec, but round-tripping the
/// order a peer sent is strictly more correct than reordering it, and a `Vec`
/// costs nothing extra at the sizes a Bolt message ever carries. Duplicate
/// keys are preserved as sent rather than silently merged — deciding which
/// one wins is the caller's question, not the codec's.
#[derive(Debug, Clone, PartialEq)]
pub enum BoltValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Bytes(Vec<u8>),
    String(String),
    List(Vec<BoltValue>),
    Dictionary(Vec<(String, BoltValue)>),
    /// A tagged tuple — Bolt's node, relationship, path and message types are
    /// all structures, distinguished by `signature`. This codec does not
    /// interpret the signature; that is Slice C's job, once a signature means
    /// something (a message type, a graph entity).
    Structure {
        signature: u8,
        fields: Vec<BoltValue>,
    },
}

// ---- Markers, named rather than left as magic bytes at each call site ----
//
// Every constant here is a value the spec assigns; none is chosen by this
// project, and the boundary tests below are what would fail if one were
// mistyped.

mod marker {
    pub const NULL: u8 = 0xC0;
    pub const FLOAT_64: u8 = 0xC1;
    pub const FALSE: u8 = 0xC2;
    pub const TRUE: u8 = 0xC3;
    pub const INT_8: u8 = 0xC8;
    pub const INT_16: u8 = 0xC9;
    pub const INT_32: u8 = 0xCA;
    pub const INT_64: u8 = 0xCB;
    pub const BYTES_8: u8 = 0xCC;
    pub const BYTES_16: u8 = 0xCD;
    pub const BYTES_32: u8 = 0xCE;
    pub const STRING_8: u8 = 0xD0;
    pub const STRING_16: u8 = 0xD1;
    pub const STRING_32: u8 = 0xD2;
    pub const LIST_8: u8 = 0xD4;
    pub const LIST_16: u8 = 0xD5;
    pub const LIST_32: u8 = 0xD6;
    pub const DICT_8: u8 = 0xD8;
    pub const DICT_16: u8 = 0xD9;
    pub const DICT_32: u8 = 0xDA;
    pub const STRUCT_8: u8 = 0xDC;
    pub const STRUCT_16: u8 = 0xDD;

    // The tiny forms encode their length or value in the low nibble of the
    // marker itself, so they are ranges rather than single bytes.
    pub const TINY_STRING_BASE: u8 = 0x80;
    pub const TINY_LIST_BASE: u8 = 0x90;
    pub const TINY_DICT_BASE: u8 = 0xA0;
    pub const TINY_STRUCT_BASE: u8 = 0xB0;
    // Positive TINY_INT is the marker byte itself, 0x00..=0x7F.
    pub const TINY_INT_POSITIVE_MAX: u8 = 0x7F;
    // Negative TINY_INT: 0xF0..=0xFF encodes -16..=-1 as a signed i8.
    pub const TINY_INT_NEGATIVE_MIN: u8 = 0xF0;
}

/// Encode one value.
///
/// Infallible: every [`BoltValue`] this codec can hold has a representation,
/// and the smallest one is always chosen — see the module docs.
#[must_use]
pub fn encode(value: &BoltValue) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    out
}

fn encode_into(value: &BoltValue, out: &mut Vec<u8>) {
    match value {
        BoltValue::Null => out.push(marker::NULL),
        BoltValue::Boolean(false) => out.push(marker::FALSE),
        BoltValue::Boolean(true) => out.push(marker::TRUE),
        BoltValue::Integer(n) => encode_integer(*n, out),
        BoltValue::Float(f) => {
            out.push(marker::FLOAT_64);
            out.extend_from_slice(&f.to_be_bytes());
        }
        BoltValue::Bytes(bytes) => {
            encode_sized(
                bytes.len(),
                out,
                [
                    (0xFF, marker::BYTES_8, 1),
                    (0xFFFF, marker::BYTES_16, 2),
                    (u64::from(u32::MAX), marker::BYTES_32, 4),
                ],
            );
            out.extend_from_slice(bytes);
        }
        BoltValue::String(s) => encode_string(s, out),
        BoltValue::List(items) => {
            encode_collection_header(
                items.len(),
                out,
                marker::TINY_LIST_BASE,
                [
                    (0xFF, marker::LIST_8, 1),
                    (0xFFFF, marker::LIST_16, 2),
                    (u64::from(u32::MAX), marker::LIST_32, 4),
                ],
            );
            for item in items {
                encode_into(item, out);
            }
        }
        BoltValue::Dictionary(entries) => {
            encode_collection_header(
                entries.len(),
                out,
                marker::TINY_DICT_BASE,
                [
                    (0xFF, marker::DICT_8, 1),
                    (0xFFFF, marker::DICT_16, 2),
                    (u64::from(u32::MAX), marker::DICT_32, 4),
                ],
            );
            for (key, val) in entries {
                encode_string(key, out);
                encode_into(val, out);
            }
        }
        BoltValue::Structure { signature, fields } => {
            if fields.len() <= 0x0F {
                #[allow(clippy::cast_possible_truncation)]
                out.push(marker::TINY_STRUCT_BASE | (fields.len() as u8));
            } else if fields.len() <= 0xFF {
                out.push(marker::STRUCT_8);
                #[allow(clippy::cast_possible_truncation)]
                out.push(fields.len() as u8);
            } else {
                out.push(marker::STRUCT_16);
                #[allow(clippy::cast_possible_truncation)]
                out.extend_from_slice(&(fields.len() as u16).to_be_bytes());
            }
            out.push(*signature);
            for field in fields {
                encode_into(field, out);
            }
        }
    }
}

/// Smallest-representation integer encoding — the property size-class
/// boundary tests check directly.
fn encode_integer(n: i64, out: &mut Vec<u8>) {
    if (-16..=i64::from(marker::TINY_INT_POSITIVE_MAX)).contains(&n) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        out.push(n as i8 as u8);
    } else if let Ok(n) = i8::try_from(n) {
        out.push(marker::INT_8);
        #[allow(clippy::cast_sign_loss)]
        out.push(n as u8);
    } else if let Ok(n) = i16::try_from(n) {
        out.push(marker::INT_16);
        out.extend_from_slice(&n.to_be_bytes());
    } else if let Ok(n) = i32::try_from(n) {
        out.push(marker::INT_32);
        out.extend_from_slice(&n.to_be_bytes());
    } else {
        out.push(marker::INT_64);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn encode_string(s: &str, out: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    encode_collection_header(
        bytes.len(),
        out,
        marker::TINY_STRING_BASE,
        [
            (0xFF, marker::STRING_8, 1),
            (0xFFFF, marker::STRING_16, 2),
            (u64::from(u32::MAX), marker::STRING_32, 4),
        ],
    );
    out.extend_from_slice(bytes);
}

/// Shared shape for `Bytes`' three size classes — kept separate from
/// [`encode_collection_header`] because bytes have no tiny form (there is no
/// `TINY_BYTES` marker in the spec, only 8/16/32).
fn encode_sized(len: usize, out: &mut Vec<u8>, classes: [(u64, u8, u8); 3]) {
    let len_u64 = len as u64;
    for (max, marker, width) in classes {
        if len_u64 <= max {
            out.push(marker);
            push_length(out, len, width);
            return;
        }
    }
    // Unreachable for any length that fits in a `usize` on a 64-bit target,
    // since the last class covers up to `u32::MAX`; kept as a defensive
    // encode of the widest class rather than a panic, so a future caller on
    // an exotic target degrades rather than crashes.
    let (_, marker, width) = classes[2];
    out.push(marker);
    push_length(out, len, width);
}

/// The tiny/8/16/32 size-class marker a length falls into, plus the length
/// prefix itself when it does not fit in the marker's own nibble.
///
/// `classes` is `[(max_for_8, marker_8, width_8), (max_for_16, ...), (max_for_32, ...)]`.
/// One function serving strings, lists and dictionaries alike is what makes
/// the boundary property — smallest representation, always — a single thing
/// to get right rather than three independent copies of the same off-by-one
/// risk.
fn encode_collection_header(
    len: usize,
    out: &mut Vec<u8>,
    tiny_base: u8,
    classes: [(u64, u8, u8); 3],
) {
    if len <= 0x0F {
        #[allow(clippy::cast_possible_truncation)]
        out.push(tiny_base | (len as u8));
        return;
    }
    encode_sized(len, out, classes);
}

fn push_length(out: &mut Vec<u8>, len: usize, width: u8) {
    match width {
        1 => {
            #[allow(clippy::cast_possible_truncation)]
            out.push(len as u8);
        }
        2 => {
            #[allow(clippy::cast_possible_truncation)]
            out.extend_from_slice(&(len as u16).to_be_bytes());
        }
        _ => {
            #[allow(clippy::cast_possible_truncation)]
            out.extend_from_slice(&(len as u32).to_be_bytes());
        }
    }
}

/// Decode one value from the front of `buf`.
///
/// `max_message_bytes` bounds every length-prefixed type this call decodes,
/// checked **before** any allocation that length would size — see the module
/// docs.
///
/// Returns `Ok(None)` when `buf` does not yet hold a complete value — this is
/// the normal "read more from the stream" outcome, not an error.
///
/// # Errors
///
/// [`PackStreamError`] for bytes that are present and invalid: an unknown
/// marker, a length past `max_message_bytes`, a non-string dictionary key, or
/// a string that is not valid UTF-8.
///
/// # Panics
///
/// Never, on any input. The fixed-width float/integer paths hold a
/// slice-to-array conversion behind an `expect`, but only after checking
/// the slice is exactly that width, so the conversion cannot fail.
pub fn decode(
    buf: &[u8],
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(&marker) = buf.first() else {
        return Ok(None);
    };
    let rest = &buf[1..];

    match marker {
        marker::NULL => Ok(Some((BoltValue::Null, 1))),
        marker::FALSE => Ok(Some((BoltValue::Boolean(false), 1))),
        marker::TRUE => Ok(Some((BoltValue::Boolean(true), 1))),
        marker::FLOAT_64 => Ok(decode_fixed(rest, 8, |b| {
            BoltValue::Float(f64::from_be_bytes(b.try_into().expect("8 bytes")))
        })),
        marker::INT_8 => Ok(decode_fixed(rest, 1, |b| {
            BoltValue::Integer(i64::from(b[0].cast_signed()))
        })),
        marker::INT_16 => Ok(decode_fixed(rest, 2, |b| {
            BoltValue::Integer(i64::from(i16::from_be_bytes(
                b.try_into().expect("2 bytes"),
            )))
        })),
        marker::INT_32 => Ok(decode_fixed(rest, 4, |b| {
            BoltValue::Integer(i64::from(i32::from_be_bytes(
                b.try_into().expect("4 bytes"),
            )))
        })),
        marker::INT_64 => Ok(decode_fixed(rest, 8, |b| {
            BoltValue::Integer(i64::from_be_bytes(b.try_into().expect("8 bytes")))
        })),
        b if b <= marker::TINY_INT_POSITIVE_MAX => Ok(Some((BoltValue::Integer(i64::from(b)), 1))),
        b if b >= marker::TINY_INT_NEGATIVE_MIN => {
            Ok(Some((BoltValue::Integer(i64::from(b.cast_signed())), 1)))
        }
        marker::BYTES_8 => decode_bytes(rest, 1, max_message_bytes),
        marker::BYTES_16 => decode_bytes(rest, 2, max_message_bytes),
        marker::BYTES_32 => decode_bytes(rest, 4, max_message_bytes),
        marker::STRING_8 => decode_string(rest, 1, max_message_bytes),
        marker::STRING_16 => decode_string(rest, 2, max_message_bytes),
        marker::STRING_32 => decode_string(rest, 4, max_message_bytes),
        b if (marker::TINY_STRING_BASE..marker::TINY_LIST_BASE).contains(&b) => {
            decode_string_from(rest, usize::from(b & 0x0F), 1)
        }
        marker::LIST_8 => decode_list(rest, 1, max_message_bytes),
        marker::LIST_16 => decode_list(rest, 2, max_message_bytes),
        marker::LIST_32 => decode_list(rest, 4, max_message_bytes),
        b if (marker::TINY_LIST_BASE..marker::TINY_DICT_BASE).contains(&b) => {
            decode_list_from(rest, usize::from(b & 0x0F), 1, max_message_bytes)
        }
        marker::DICT_8 => decode_dict(rest, 1, max_message_bytes),
        marker::DICT_16 => decode_dict(rest, 2, max_message_bytes),
        marker::DICT_32 => decode_dict(rest, 4, max_message_bytes),
        b if (marker::TINY_DICT_BASE..marker::TINY_STRUCT_BASE).contains(&b) => {
            decode_dict_from(rest, usize::from(b & 0x0F), 1, max_message_bytes)
        }
        marker::STRUCT_8 => decode_struct(rest, 1, max_message_bytes),
        marker::STRUCT_16 => decode_struct(rest, 2, max_message_bytes),
        // Bounded above by `marker::NULL` (0xC0), not left open-ended: bytes
        // like 0xC4 or 0xD3 are undefined by the spec and must fall through
        // to `UnknownMarker` rather than be misread as a tiny struct whose
        // field count happens to share their low nibble.
        b if (marker::TINY_STRUCT_BASE..marker::NULL).contains(&b) => {
            decode_struct_from(rest, usize::from(b & 0x0F), 1, max_message_bytes)
        }
        other => Err(PackStreamError::UnknownMarker(other)),
    }
}

/// A fixed-width payload with no length prefix: floats and the wider
/// integers. One function for all of them, the same way
/// [`encode_collection_header`] is one function for every size-classed
/// collection — the four call sites each fix their own `width` and `build`,
/// so the boundary a reviewer checks is stated once, not four times.
fn decode_fixed(
    rest: &[u8],
    width: usize,
    build: impl FnOnce(&[u8]) -> BoltValue,
) -> Option<(BoltValue, usize)> {
    if rest.len() < width {
        return None;
    }
    Some((build(&rest[..width]), 1 + width))
}

/// Reads a big-endian length prefix of `width` bytes from the front of
/// `buf`, validating it against `max_message_bytes` **before** any caller
/// can allocate from it. `Ok(None)` means `buf` does not yet hold the whole
/// prefix — truncation, not a malformed message.
fn read_length(
    buf: &[u8],
    width: usize,
    max_message_bytes: usize,
) -> Result<Option<usize>, PackStreamError> {
    if buf.len() < width {
        return Ok(None);
    }
    let declared: u64 = match width {
        1 => u64::from(buf[0]),
        2 => u64::from(u16::from_be_bytes(buf[..2].try_into().expect("2 bytes"))),
        _ => u64::from(u32::from_be_bytes(buf[..4].try_into().expect("4 bytes"))),
    };
    if declared > max_message_bytes as u64 {
        return Err(PackStreamError::LengthExceedsBudget {
            declared,
            limit: max_message_bytes,
        });
    }
    // Safe: `declared <= max_message_bytes`, which is itself a `usize`.
    #[allow(clippy::cast_possible_truncation)]
    Ok(Some(declared as usize))
}

fn decode_bytes(
    rest: &[u8],
    width: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(len) = read_length(rest, width, max_message_bytes)? else {
        return Ok(None);
    };
    let body = &rest[width..];
    if body.len() < len {
        return Ok(None);
    }
    Ok(Some((
        BoltValue::Bytes(body[..len].to_vec()),
        1 + width + len,
    )))
}

fn decode_string(
    rest: &[u8],
    width: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(len) = read_length(rest, width, max_message_bytes)? else {
        return Ok(None);
    };
    decode_string_from(&rest[width..], len, 1 + width)
}

/// `body` starts right after any length prefix (or right after the marker,
/// for a tiny string); `prefix` is how many bytes were already consumed
/// getting there, so the caller's total is `prefix + len`.
fn decode_string_from(
    body: &[u8],
    len: usize,
    prefix: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    if body.len() < len {
        return Ok(None);
    }
    let s = std::str::from_utf8(&body[..len]).map_err(|_| PackStreamError::InvalidUtf8)?;
    Ok(Some((BoltValue::String(s.to_owned()), prefix + len)))
}

fn decode_list(
    rest: &[u8],
    width: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(count) = read_length(rest, width, max_message_bytes)? else {
        return Ok(None);
    };
    decode_list_from(&rest[width..], count, 1 + width, max_message_bytes)
}

fn decode_list_from(
    body: &[u8],
    count: usize,
    prefix: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let mut items = Vec::new();
    let mut offset = 0;
    for _ in 0..count {
        let Some((value, used)) = decode(&body[offset..], max_message_bytes)? else {
            return Ok(None);
        };
        items.push(value);
        offset += used;
    }
    Ok(Some((BoltValue::List(items), prefix + offset)))
}

fn decode_dict(
    rest: &[u8],
    width: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(count) = read_length(rest, width, max_message_bytes)? else {
        return Ok(None);
    };
    decode_dict_from(&rest[width..], count, 1 + width, max_message_bytes)
}

/// Keys are decoded through the same [`decode`] as any other value, then
/// checked for `String` — rather than duplicating the string dispatch — so a
/// non-string key is reported using the marker it actually arrived with.
fn decode_dict_from(
    body: &[u8],
    count: usize,
    prefix: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let mut entries = Vec::new();
    let mut offset = 0;
    for _ in 0..count {
        let Some(&key_marker) = body.get(offset) else {
            return Ok(None);
        };
        let Some((key_value, key_used)) = decode(&body[offset..], max_message_bytes)? else {
            return Ok(None);
        };
        let BoltValue::String(key) = key_value else {
            return Err(PackStreamError::NonStringKey(key_marker));
        };
        offset += key_used;
        let Some((value, value_used)) = decode(&body[offset..], max_message_bytes)? else {
            return Ok(None);
        };
        offset += value_used;
        entries.push((key, value));
    }
    Ok(Some((BoltValue::Dictionary(entries), prefix + offset)))
}

fn decode_struct(
    rest: &[u8],
    width: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(field_count) = read_length(rest, width, max_message_bytes)? else {
        return Ok(None);
    };
    decode_struct_from(&rest[width..], field_count, 1 + width, max_message_bytes)
}

fn decode_struct_from(
    body: &[u8],
    field_count: usize,
    prefix: usize,
    max_message_bytes: usize,
) -> Result<Option<(BoltValue, usize)>, PackStreamError> {
    let Some(&signature) = body.first() else {
        return Ok(None);
    };
    let mut offset = 1;
    let mut fields = Vec::new();
    for _ in 0..field_count {
        let Some((value, used)) = decode(&body[offset..], max_message_bytes)? else {
            return Ok(None);
        };
        fields.push(value);
        offset += used;
    }
    Ok(Some((
        BoltValue::Structure { signature, fields },
        prefix + offset,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_round_trips(value: &BoltValue) {
        let encoded = encode(value);
        let (decoded, consumed) = decode(&encoded, encoded.len())
            .expect("well-formed bytes must decode")
            .expect("a full encoding must not be reported as truncated");
        assert_eq!(
            &decoded, value,
            "round-trip must reproduce the original value"
        );
        assert_eq!(
            consumed,
            encoded.len(),
            "decode must consume exactly the bytes encode produced, no more and no less"
        );
    }

    // ---- Every variant round-trips ----

    #[test]
    fn null_round_trips() {
        assert_round_trips(&BoltValue::Null);
    }

    #[test]
    fn both_booleans_round_trip() {
        assert_round_trips(&BoltValue::Boolean(true));
        assert_round_trips(&BoltValue::Boolean(false));
    }

    #[test]
    fn a_float_round_trips() {
        assert_round_trips(&BoltValue::Float(std::f64::consts::PI));
    }

    #[test]
    fn negative_and_fractional_floats_round_trip() {
        assert_round_trips(&BoltValue::Float(-0.5));
        assert_round_trips(&BoltValue::Float(0.0));
    }

    #[test]
    fn bytes_round_trip() {
        assert_round_trips(&BoltValue::Bytes(vec![0x00, 0xFF, 0x10, 0x20]));
    }

    #[test]
    fn empty_bytes_round_trip() {
        assert_round_trips(&BoltValue::Bytes(vec![]));
    }

    fn bytes_of_len(n: usize) -> BoltValue {
        BoltValue::Bytes(vec![0xAB; n])
    }

    #[test]
    fn bytes_has_no_tiny_form_even_a_single_byte_uses_bytes8() {
        // Unlike string/list/dict, PackStream defines no TINY_BYTES marker.
        assert_eq!(first_byte(&bytes_of_len(0)), marker::BYTES_8);
        assert_eq!(first_byte(&bytes_of_len(1)), marker::BYTES_8);
    }

    #[test]
    fn bytes_of_255_is_bytes8_and_256_is_bytes16() {
        assert_eq!(first_byte(&bytes_of_len(255)), marker::BYTES_8);
        assert_eq!(first_byte(&bytes_of_len(256)), marker::BYTES_16);
    }

    #[test]
    fn bytes_of_65535_is_bytes16_and_65536_is_bytes32() {
        assert_eq!(first_byte(&bytes_of_len(65535)), marker::BYTES_16);
        assert_eq!(first_byte(&bytes_of_len(65536)), marker::BYTES_32);
    }

    #[test]
    fn bytes_at_every_size_class_boundary_round_trip() {
        for n in [0, 1, 255, 256, 65535, 65536] {
            assert_round_trips(&bytes_of_len(n));
        }
    }

    #[test]
    fn a_string_round_trips() {
        assert_round_trips(&BoltValue::String("hello, graph-owl".to_owned()));
    }

    #[test]
    fn an_empty_string_round_trips() {
        assert_round_trips(&BoltValue::String(String::new()));
    }

    #[test]
    fn a_multibyte_utf8_string_round_trips() {
        // Multi-byte UTF-8 means byte length and character count diverge —
        // the length prefix must be in bytes, not chars.
        assert_round_trips(&BoltValue::String("caf\u{e9} \u{1f9ee}".to_owned()));
    }

    #[test]
    fn a_list_round_trips() {
        assert_round_trips(&BoltValue::List(vec![
            BoltValue::Integer(1),
            BoltValue::String("two".to_owned()),
            BoltValue::Boolean(true),
        ]));
    }

    #[test]
    fn an_empty_list_round_trips() {
        assert_round_trips(&BoltValue::List(vec![]));
    }

    #[test]
    fn a_dictionary_round_trips() {
        assert_round_trips(&BoltValue::Dictionary(vec![
            ("name".to_owned(), BoltValue::String("Alice".to_owned())),
            ("age".to_owned(), BoltValue::Integer(30)),
        ]));
    }

    #[test]
    fn an_empty_dictionary_round_trips() {
        assert_round_trips(&BoltValue::Dictionary(vec![]));
    }

    #[test]
    fn dictionary_key_order_is_preserved_not_reordered() {
        let value = BoltValue::Dictionary(vec![
            ("z".to_owned(), BoltValue::Integer(1)),
            ("a".to_owned(), BoltValue::Integer(2)),
        ]);
        let encoded = encode(&value);
        let (decoded, _) = decode(&encoded, encoded.len()).unwrap().unwrap();
        let BoltValue::Dictionary(entries) = decoded else {
            panic!("expected a dictionary");
        };
        assert_eq!(
            entries[0].0, "z",
            "encoding order must survive, not be sorted"
        );
        assert_eq!(entries[1].0, "a");
    }

    #[test]
    fn a_structure_round_trips() {
        assert_round_trips(&BoltValue::Structure {
            signature: 0x4E,
            fields: vec![
                BoltValue::Integer(7),
                BoltValue::String("Person".to_owned()),
            ],
        });
    }

    #[test]
    fn a_structure_with_no_fields_round_trips() {
        assert_round_trips(&BoltValue::Structure {
            signature: 0x01,
            fields: vec![],
        });
    }

    #[test]
    fn a_nested_structure_of_list_dict_and_structure_round_trips() {
        assert_round_trips(&BoltValue::List(vec![BoltValue::Dictionary(vec![(
            "node".to_owned(),
            BoltValue::Structure {
                signature: 0x4E,
                fields: vec![
                    BoltValue::Integer(1),
                    BoltValue::List(vec![BoltValue::String("Label".to_owned())]),
                ],
            },
        )])]));
    }

    // ---- Integer smallest-representation boundaries ----
    //
    // Each case names the boundary value, the marker its encoding must start
    // with, and its total encoded length (marker + payload). A wrong
    // boundary constant shifts one of these to the neighbouring class.

    fn first_byte(value: &BoltValue) -> u8 {
        encode(value)[0]
    }

    #[test]
    fn the_tiny_positive_boundary_is_127_not_128() {
        assert_eq!(
            encode(&BoltValue::Integer(127)).len(),
            1,
            "127 fits in the marker byte itself"
        );
        assert_eq!(first_byte(&BoltValue::Integer(127)), 0x7F);
        assert_eq!(
            first_byte(&BoltValue::Integer(128)),
            marker::INT_16,
            "128 exceeds i8::MAX, so INT_8 cannot hold it either — the smallest fit is INT_16"
        );
    }

    #[test]
    fn the_tiny_negative_boundary_is_negative_16_not_negative_17() {
        assert_eq!(encode(&BoltValue::Integer(-16)).len(), 1);
        assert_eq!(first_byte(&BoltValue::Integer(-16)), 0xF0);
        assert_eq!(first_byte(&BoltValue::Integer(-17)), marker::INT_8);
    }

    #[test]
    fn the_int8_negative_boundary_is_negative_128() {
        assert_eq!(first_byte(&BoltValue::Integer(-128)), marker::INT_8);
        assert_eq!(
            first_byte(&BoltValue::Integer(-129)),
            marker::INT_16,
            "-129 is below i8::MIN"
        );
    }

    #[test]
    fn the_int16_boundaries_are_i16_min_and_max() {
        assert_eq!(first_byte(&BoltValue::Integer(32767)), marker::INT_16);
        assert_eq!(first_byte(&BoltValue::Integer(32768)), marker::INT_32);
        assert_eq!(first_byte(&BoltValue::Integer(-32768)), marker::INT_16);
        assert_eq!(first_byte(&BoltValue::Integer(-32769)), marker::INT_32);
    }

    #[test]
    fn the_int32_boundaries_are_i32_min_and_max() {
        assert_eq!(
            first_byte(&BoltValue::Integer(2_147_483_647)),
            marker::INT_32
        );
        assert_eq!(
            first_byte(&BoltValue::Integer(2_147_483_648)),
            marker::INT_64
        );
        assert_eq!(
            first_byte(&BoltValue::Integer(-2_147_483_648)),
            marker::INT_32
        );
        assert_eq!(
            first_byte(&BoltValue::Integer(-2_147_483_649)),
            marker::INT_64
        );
    }

    #[test]
    fn the_widest_integers_round_trip() {
        assert_round_trips(&BoltValue::Integer(i64::MAX));
        assert_round_trips(&BoltValue::Integer(i64::MIN));
    }

    #[test]
    fn every_integer_boundary_round_trips() {
        for n in [
            -16,
            -17,
            -128,
            -129,
            -32768,
            -32769,
            -2_147_483_648,
            -2_147_483_649,
            0,
            127,
            128,
            32767,
            32768,
            2_147_483_647,
            2_147_483_648,
        ] {
            assert_round_trips(&BoltValue::Integer(n));
        }
    }

    #[test]
    fn a_value_decodes_correctly_from_a_wider_marker_than_encode_would_choose() {
        // encode(200) picks INT_16; a peer sending the same value as INT_64
        // must still be read back as 200, not rejected or misread.
        let mut wide = vec![marker::INT_64];
        wide.extend_from_slice(&200_i64.to_be_bytes());
        let (decoded, consumed) = decode(&wide, wide.len()).unwrap().unwrap();
        assert_eq!(decoded, BoltValue::Integer(200));
        assert_eq!(consumed, 9);
    }

    #[test]
    fn a_small_value_decodes_correctly_from_int32() {
        let mut wide = vec![marker::INT_32];
        wide.extend_from_slice(&5_i32.to_be_bytes());
        let (decoded, _) = decode(&wide, wide.len()).unwrap().unwrap();
        assert_eq!(decoded, BoltValue::Integer(5));
    }

    // ---- String / list / dictionary size-class boundaries: 15/16, 255/256, 65535/65536 ----

    fn string_of_len(n: usize) -> BoltValue {
        BoltValue::String("a".repeat(n))
    }

    fn list_of_len(n: usize) -> BoltValue {
        BoltValue::List((0..n).map(|_| BoltValue::Boolean(true)).collect())
    }

    fn dict_of_len(n: usize) -> BoltValue {
        BoltValue::Dictionary(
            (0..n)
                .map(|i| (format!("k{i}"), BoltValue::Integer(1)))
                .collect(),
        )
    }

    #[test]
    fn a_string_of_15_bytes_is_tiny_and_16_is_not() {
        assert_eq!(
            first_byte(&string_of_len(15)),
            marker::TINY_STRING_BASE | 15
        );
        assert_eq!(first_byte(&string_of_len(16)), marker::STRING_8);
    }

    #[test]
    fn a_string_of_255_bytes_is_string8_and_256_is_string16() {
        assert_eq!(first_byte(&string_of_len(255)), marker::STRING_8);
        assert_eq!(first_byte(&string_of_len(256)), marker::STRING_16);
    }

    #[test]
    fn a_string_of_65535_bytes_is_string16_and_65536_is_string32() {
        assert_eq!(first_byte(&string_of_len(65535)), marker::STRING_16);
        assert_eq!(first_byte(&string_of_len(65536)), marker::STRING_32);
    }

    #[test]
    fn strings_at_every_size_class_boundary_round_trip() {
        for n in [0, 15, 16, 255, 256, 65535, 65536] {
            assert_round_trips(&string_of_len(n));
        }
    }

    #[test]
    fn a_list_of_15_items_is_tiny_and_16_is_not() {
        assert_eq!(first_byte(&list_of_len(15)), marker::TINY_LIST_BASE | 15);
        assert_eq!(first_byte(&list_of_len(16)), marker::LIST_8);
    }

    #[test]
    fn a_list_of_255_items_is_list8_and_256_is_list16() {
        assert_eq!(first_byte(&list_of_len(255)), marker::LIST_8);
        assert_eq!(first_byte(&list_of_len(256)), marker::LIST_16);
    }

    #[test]
    fn a_list_of_65535_items_is_list16_and_65536_is_list32() {
        assert_eq!(first_byte(&list_of_len(65535)), marker::LIST_16);
        assert_eq!(first_byte(&list_of_len(65536)), marker::LIST_32);
    }

    #[test]
    fn lists_at_size_class_boundaries_round_trip() {
        for n in [0, 15, 16, 255, 256, 65535, 65536] {
            assert_round_trips(&list_of_len(n));
        }
    }

    #[test]
    fn a_dict_of_15_entries_is_tiny_and_16_is_not() {
        assert_eq!(first_byte(&dict_of_len(15)), marker::TINY_DICT_BASE | 15);
        assert_eq!(first_byte(&dict_of_len(16)), marker::DICT_8);
    }

    #[test]
    fn a_dict_of_255_entries_is_dict8_and_256_is_dict16() {
        assert_eq!(first_byte(&dict_of_len(255)), marker::DICT_8);
        assert_eq!(first_byte(&dict_of_len(256)), marker::DICT_16);
    }

    #[test]
    fn a_dict_of_65535_entries_is_dict16_and_65536_is_dict32() {
        assert_eq!(first_byte(&dict_of_len(65535)), marker::DICT_16);
        assert_eq!(first_byte(&dict_of_len(65536)), marker::DICT_32);
    }

    #[test]
    fn dicts_at_size_class_boundaries_round_trip() {
        for n in [0, 15, 16, 255, 256, 65535, 65536] {
            assert_round_trips(&dict_of_len(n));
        }
    }

    // ---- Structures: signature and field count ----

    fn structure_with_fields(n: usize) -> BoltValue {
        BoltValue::Structure {
            signature: 0x2A,
            fields: (0..n).map(|_| BoltValue::Integer(1)).collect(),
        }
    }

    #[test]
    fn a_structure_of_15_fields_is_tiny_and_16_is_not() {
        assert_eq!(
            first_byte(&structure_with_fields(15)),
            marker::TINY_STRUCT_BASE | 15
        );
        assert_eq!(first_byte(&structure_with_fields(16)), marker::STRUCT_8);
    }

    #[test]
    fn a_structure_of_255_fields_is_struct8_and_256_is_struct16() {
        assert_eq!(first_byte(&structure_with_fields(255)), marker::STRUCT_8);
        assert_eq!(first_byte(&structure_with_fields(256)), marker::STRUCT_16);
    }

    #[test]
    fn structures_at_field_count_boundaries_round_trip() {
        for n in [0, 15, 16, 255, 256] {
            assert_round_trips(&structure_with_fields(n));
        }
    }

    #[test]
    fn the_signature_byte_survives_the_round_trip_distinct_from_the_field_count() {
        let value = BoltValue::Structure {
            signature: 0x99,
            fields: vec![BoltValue::Null],
        };
        let encoded = encode(&value);
        let (decoded, _) = decode(&encoded, encoded.len()).unwrap().unwrap();
        let BoltValue::Structure { signature, .. } = decoded else {
            panic!("expected a structure");
        };
        assert_eq!(signature, 0x99);
    }

    // ---- Truncation returns Ok(None), never an error ----

    #[test]
    fn an_empty_buffer_is_truncated_not_an_error() {
        assert_eq!(decode(&[], 1024), Ok(None));
    }

    #[test]
    fn a_float_missing_payload_bytes_is_truncated() {
        assert_eq!(decode(&[marker::FLOAT_64, 0x00, 0x00], 1024), Ok(None));
    }

    #[test]
    fn an_int16_missing_one_payload_byte_is_truncated() {
        assert_eq!(decode(&[marker::INT_16, 0x00], 1024), Ok(None));
    }

    #[test]
    fn a_string8_missing_the_length_byte_is_truncated() {
        assert_eq!(decode(&[marker::STRING_8], 1024), Ok(None));
    }

    #[test]
    fn a_string8_with_the_length_but_not_the_body_is_truncated() {
        assert_eq!(decode(&[marker::STRING_8, 5, b'h', b'i'], 1024), Ok(None));
    }

    #[test]
    fn a_list_missing_a_declared_element_is_truncated() {
        // TINY_LIST of 2, only one element present.
        let buf = [marker::TINY_LIST_BASE | 2, 0x01];
        assert_eq!(decode(&buf, 1024), Ok(None));
    }

    #[test]
    fn a_dict_missing_a_declared_value_is_truncated() {
        // TINY_DICT of 1, key present, value absent.
        let mut buf = vec![marker::TINY_DICT_BASE | 1];
        buf.extend(encode(&BoltValue::String("k".to_owned())));
        assert_eq!(decode(&buf, 1024), Ok(None));
    }

    #[test]
    fn a_structure_missing_the_signature_byte_is_truncated() {
        assert_eq!(decode(&[marker::TINY_STRUCT_BASE | 1], 1024), Ok(None));
    }

    // ---- Allocation guard: the check runs before any allocation it bounds ----

    #[test]
    fn an_oversized_declared_bytes_length_is_refused_before_allocating() {
        let mut buf = vec![marker::BYTES_32];
        buf.extend_from_slice(&0x8000_0000_u32.to_be_bytes()); // ~2^31, only 2 bytes follow
        buf.extend_from_slice(&[0x00, 0x00]);
        assert_eq!(
            decode(&buf, 1024),
            Err(PackStreamError::LengthExceedsBudget {
                declared: 0x8000_0000,
                limit: 1024
            })
        );
    }

    #[test]
    fn an_oversized_declared_string_length_is_refused_before_allocating() {
        let mut buf = vec![marker::STRING_32];
        buf.extend_from_slice(&0x8000_0000_u32.to_be_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        assert_eq!(
            decode(&buf, 1024),
            Err(PackStreamError::LengthExceedsBudget {
                declared: 0x8000_0000,
                limit: 1024
            })
        );
    }

    #[test]
    fn an_oversized_declared_list_length_is_refused_before_allocating() {
        let mut buf = vec![marker::LIST_32];
        buf.extend_from_slice(&0x8000_0000_u32.to_be_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        assert_eq!(
            decode(&buf, 1024),
            Err(PackStreamError::LengthExceedsBudget {
                declared: 0x8000_0000,
                limit: 1024
            })
        );
    }

    #[test]
    fn an_oversized_declared_dict_length_is_refused_before_allocating() {
        let mut buf = vec![marker::DICT_32];
        buf.extend_from_slice(&0x8000_0000_u32.to_be_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        assert_eq!(
            decode(&buf, 1024),
            Err(PackStreamError::LengthExceedsBudget {
                declared: 0x8000_0000,
                limit: 1024
            })
        );
    }

    #[test]
    fn the_length_at_exactly_the_budget_is_accepted_not_refused() {
        // The boundary itself must not be swept into the rejection by an
        // off-by-one — only *exceeding* the budget is refused.
        let mut buf = vec![marker::BYTES_8];
        buf.push(4);
        buf.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            decode(&buf, 4),
            Ok(Some((BoltValue::Bytes(vec![1, 2, 3, 4]), 6)))
        );
    }

    // ---- Malformed content that is present, not merely truncated ----

    #[test]
    fn an_unknown_marker_byte_is_an_error() {
        assert_eq!(
            decode(&[0xC4], 1024),
            Err(PackStreamError::UnknownMarker(0xC4))
        );
    }

    #[test]
    fn every_reserved_marker_byte_is_rejected_not_misread_as_a_tiny_struct() {
        // These sit inside 0xB0..=0xFF, the same byte range a naive
        // `b >= TINY_STRUCT_BASE` catch-all would wrongly swallow — each
        // must still be reported as unknown rather than decoded as a
        // struct whose field count happens to match its low nibble.
        for b in [0xC4, 0xC5, 0xC6, 0xC7, 0xCF, 0xD3, 0xD7, 0xDB, 0xDE, 0xDF] {
            assert_eq!(decode(&[b], 1024), Err(PackStreamError::UnknownMarker(b)));
        }
    }

    #[test]
    fn a_non_string_dictionary_key_is_rejected() {
        // TINY_DICT of 1, key is an integer (marker 0x01), not a string.
        let buf = [marker::TINY_DICT_BASE | 1, 0x01, 0x01];
        assert_eq!(decode(&buf, 1024), Err(PackStreamError::NonStringKey(0x01)));
    }

    #[test]
    fn invalid_utf8_in_a_string_is_rejected() {
        let buf = [marker::TINY_STRING_BASE | 1, 0xFF];
        assert_eq!(decode(&buf, 1024), Err(PackStreamError::InvalidUtf8));
    }

    // ---- Fuzz-style corpus: arbitrary bytes must never panic ----

    #[test]
    #[allow(clippy::cast_possible_truncation)] // deliberately taking the low byte/bits of a PRNG word
    fn decoding_arbitrary_bytes_never_panics() {
        // A small deterministic PRNG (xorshift32) avoids adding a fuzzing
        // dependency for what the acceptance criterion actually asks for:
        // no panic across a wide spread of inputs, not statistical coverage.
        let mut state: u32 = 0x9E37_79B9;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..2000 {
            let len = (next() % 40) as usize;
            let buf: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            // Every outcome is acceptable except a panic — Ok or Err both are.
            let _ = decode(&buf, 4096);
        }
    }

    #[test]
    fn decoding_every_single_marker_byte_alone_never_panics() {
        for b in 0u8..=255 {
            let _ = decode(&[b], 4096);
        }
    }
}
