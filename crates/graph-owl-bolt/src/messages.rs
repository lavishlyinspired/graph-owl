//! Bolt's message set, and the graph-structure encoding `RECORD` carries
//! them in — targeting protocol version **5.0** exactly.
//!
//! **One version, deliberately, not the declared range the plan sketches.**
//! Each Bolt minor version after 5.0 changes a message or structure shape —
//! `LOGON`/`LOGOFF` split authentication out of `HELLO` at 5.1, notification
//! filtering fields land at 5.2, `bolt_agent` becomes mandatory at 5.3, and
//! `FAILURE`'s metadata shape changes at 5.7. Supporting a *range* would mean
//! branching every message's shape on the negotiated version; supporting
//! *one* keeps this module's encode/decode a single, exhaustively-testable
//! pair of functions. 5.0 is the newest version where that is still true —
//! it already carries `element_id` (added at 5.0) — so [`crate::handshake`]
//! offers exactly `(5, 0)` today. Widening the supported set is a matter of
//! adding another `(major, minor)` and, if the new version changed a shape
//! this module relies on, a version-gated branch at exactly that point —
//! not a rewrite.
//!
//! Signature bytes and field orders are taken from the published Bolt
//! protocol specification (`00i` rule 2), not from any reference server or
//! driver implementation.

use graph_owl_lpg::{LossyMapping, LpgEdge, LpgNode, PropertyMap, PropertyValue};

use crate::auth::Credentials;
use crate::packstream::BoltValue;

pub mod signature {
    // Client request messages.
    pub const HELLO: u8 = 0x01;
    pub const GOODBYE: u8 = 0x02;
    pub const RESET: u8 = 0x0F;
    pub const RUN: u8 = 0x10;
    pub const BEGIN: u8 = 0x11;
    pub const COMMIT: u8 = 0x12;
    pub const ROLLBACK: u8 = 0x13;
    pub const DISCARD: u8 = 0x2F;
    pub const PULL: u8 = 0x3F;

    // Server summary/detail messages.
    pub const SUCCESS: u8 = 0x70;
    pub const RECORD: u8 = 0x71;
    pub const IGNORED: u8 = 0x7E;
    pub const FAILURE: u8 = 0x7F;

    // Graph structures (`structure-semantics.adoc`).
    pub const NODE: u8 = 0x4E;
    pub const RELATIONSHIP: u8 = 0x52;
    pub const PATH: u8 = 0x50;
}

/// A message the server may receive, decoded from a top-level
/// [`BoltValue::Structure`].
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Hello {
        credentials: Credentials,
        user_agent: String,
    },
    Run {
        query: String,
    },
    Pull {
        n: i64,
    },
    Discard {
        n: i64,
    },
    Begin,
    Commit,
    Rollback,
    Reset,
    Goodbye,
}

/// Which [`ClientMessage`] a byte decoded to, without needing to decode its
/// fields — the state machine (Slice C) checks this against what is legal in
/// the current state before spending any effort on the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Hello,
    Run,
    Pull,
    Discard,
    Begin,
    Commit,
    Rollback,
    Reset,
    Goodbye,
}

impl ClientMessage {
    #[must_use]
    pub fn kind(&self) -> MessageKind {
        match self {
            ClientMessage::Hello { .. } => MessageKind::Hello,
            ClientMessage::Run { .. } => MessageKind::Run,
            ClientMessage::Pull { .. } => MessageKind::Pull,
            ClientMessage::Discard { .. } => MessageKind::Discard,
            ClientMessage::Begin => MessageKind::Begin,
            ClientMessage::Commit => MessageKind::Commit,
            ClientMessage::Rollback => MessageKind::Rollback,
            ClientMessage::Reset => MessageKind::Reset,
            ClientMessage::Goodbye => MessageKind::Goodbye,
        }
    }
}

/// A response the server may send.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    Success(Vec<(String, BoltValue)>),
    Record(Vec<BoltValue>),
    Ignored,
    /// `{"code": ..., "message": ...}` — the shape valid through Bolt 5.6;
    /// 5.7 replaces it with `gql_status`, out of scope at 5.0.
    Failure {
        code: String,
        message: String,
    },
}

/// Bytes that did not decode to a legal `ClientMessage`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("byte 0x{0:02x} is not a message signature this server understands")]
    UnknownSignature(u8),
    #[error("{signature} message is missing its {field} field")]
    MissingField {
        signature: &'static str,
        field: &'static str,
    },
    #[error("{signature} message's {field} field has the wrong type")]
    WrongType {
        signature: &'static str,
        field: &'static str,
    },
}

fn dict_field<'a>(entries: &'a [(String, BoltValue)], key: &str) -> Option<&'a BoltValue> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn dict_string(entries: &[(String, BoltValue)], key: &str) -> Option<String> {
    match dict_field(entries, key) {
        Some(BoltValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Decode one message from a top-level [`BoltValue`] — the value
/// [`crate::packstream::decode`] produced from one chunked-and-reassembled
/// message.
///
/// # Errors
///
/// [`ProtocolError`] when the signature is not one this server's message set
/// includes, or a required field is absent or the wrong PackStream type —
/// both distinct from truncation, which [`crate::packstream::decode`] and
/// [`crate::chunking::Decoder`] already resolved before this function runs.
pub fn decode_client_message(value: BoltValue) -> Result<ClientMessage, ProtocolError> {
    let BoltValue::Structure { signature, fields } = value else {
        return Err(ProtocolError::UnknownSignature(0));
    };

    match signature {
        signature::HELLO => {
            let Some(BoltValue::Dictionary(extra)) = fields.into_iter().next() else {
                return Err(ProtocolError::MissingField {
                    signature: "HELLO",
                    field: "extra",
                });
            };
            let scheme = dict_string(&extra, "scheme").unwrap_or_else(|| "none".to_string());
            let principal = dict_string(&extra, "principal");
            let credentials = dict_string(&extra, "credentials");
            let user_agent = dict_string(&extra, "user_agent").unwrap_or_default();
            Ok(ClientMessage::Hello {
                credentials: Credentials {
                    scheme,
                    principal,
                    credentials,
                },
                user_agent,
            })
        }
        signature::RUN => {
            let Some(BoltValue::String(query)) = fields.into_iter().next() else {
                return Err(ProtocolError::MissingField {
                    signature: "RUN",
                    field: "query",
                });
            };
            // `parameters` and `extra` (bookmarks, tx_timeout, mode, ...) are
            // present on the wire and consumed by the caller reading past
            // this field, but not interpreted: Epic 7b's Cypher subset has
            // no `$parameter` substitution to feed them into, and this
            // server has no bookmark/routing state to honour. A query that
            // does not use parameters — which is everything Slice F's own
            // driver test runs — is unaffected.
            Ok(ClientMessage::Run { query })
        }
        signature::PULL => Ok(ClientMessage::Pull {
            n: extra_n(fields, "PULL")?,
        }),
        signature::DISCARD => Ok(ClientMessage::Discard {
            n: extra_n(fields, "DISCARD")?,
        }),
        signature::BEGIN => Ok(ClientMessage::Begin),
        signature::COMMIT => Ok(ClientMessage::Commit),
        signature::ROLLBACK => Ok(ClientMessage::Rollback),
        signature::RESET => Ok(ClientMessage::Reset),
        signature::GOODBYE => Ok(ClientMessage::Goodbye),
        other => Err(ProtocolError::UnknownSignature(other)),
    }
}

/// `PULL`/`DISCARD` share this exact shape: one `extra::Dictionary` field
/// carrying `n` (required) and `qid` (present on the wire, not tracked here —
/// see the module doc's scope note on single-statement-per-connection).
fn extra_n(fields: Vec<BoltValue>, name: &'static str) -> Result<i64, ProtocolError> {
    let Some(BoltValue::Dictionary(extra)) = fields.into_iter().next() else {
        return Err(ProtocolError::MissingField {
            signature: name,
            field: "extra",
        });
    };
    match dict_field(&extra, "n") {
        Some(BoltValue::Integer(n)) => Ok(*n),
        Some(_) => Err(ProtocolError::WrongType {
            signature: name,
            field: "n",
        }),
        None => Err(ProtocolError::MissingField {
            signature: name,
            field: "n",
        }),
    }
}

/// Encode a response as the [`BoltValue::Structure`]
/// [`crate::packstream::encode`] then [`crate::chunking::encode`] carry to
/// the wire.
#[must_use]
pub fn encode_server_message(message: &ServerMessage) -> BoltValue {
    match message {
        ServerMessage::Success(metadata) => BoltValue::Structure {
            signature: signature::SUCCESS,
            fields: vec![BoltValue::Dictionary(metadata.clone())],
        },
        ServerMessage::Record(values) => BoltValue::Structure {
            signature: signature::RECORD,
            fields: vec![BoltValue::List(values.clone())],
        },
        ServerMessage::Ignored => BoltValue::Structure {
            signature: signature::IGNORED,
            fields: vec![],
        },
        ServerMessage::Failure { code, message } => BoltValue::Structure {
            signature: signature::FAILURE,
            fields: vec![BoltValue::Dictionary(vec![
                ("code".to_string(), BoltValue::String(code.clone())),
                ("message".to_string(), BoltValue::String(message.clone())),
            ])],
        },
    }
}

/// A stable, deterministic stand-in for the legacy integer node/relationship
/// id every structure still carries a field for even at protocol 5.0.
///
/// **Not an identity a client should key anything on** — `element_id` is.
/// The legacy field exists only because the structure's field *count* is
/// part of the wire format; a driver that still reads it (most no longer
/// do, past `element_id`'s introduction) gets a value that is at least
/// stable across repeated reads of the same element within one process,
/// which is all the legacy contract ever guaranteed once ids stopped being
/// assigned in order.
fn legacy_id(element_id: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    element_id.hash(&mut hasher);
    #[allow(clippy::cast_possible_wrap)] // a stand-in id, not a value anything decodes back
    {
        hasher.finish() as i64
    }
}

fn bolt_property_value(value: &PropertyValue) -> BoltValue {
    match value {
        PropertyValue::Boolean(b) => BoltValue::Boolean(*b),
        PropertyValue::Integer(n) => BoltValue::Integer(*n),
        PropertyValue::Float(f) => BoltValue::Float(*f),
        PropertyValue::String(s) => BoltValue::String(s.clone()),
        PropertyValue::Bytes(b) => BoltValue::Bytes(b.clone()),
        // Bolt has a native `DateTime` structure (`structure-semantics.adoc`);
        // encoding one exactly needs the tz-offset/seconds/nanoseconds split
        // that structure defines. Deferred to the slice that adds Bolt's
        // temporal structures generally — see Slice D's acceptance criteria
        // — rather than half-implemented here as a string that a driver's
        // typed temporal API would not recognise anyway.
        PropertyValue::DateTime(dt) => BoltValue::String(dt.to_rfc3339()),
        PropertyValue::Duration(seconds) => BoltValue::Integer(*seconds),
        PropertyValue::List(items) => {
            BoltValue::List(items.iter().map(bolt_property_value).collect())
        }
        PropertyValue::ElementRef(id) => BoltValue::String(id.to_string()),
    }
}

fn bolt_properties(properties: &PropertyMap) -> BoltValue {
    BoltValue::Dictionary(
        properties
            .keys()
            .map(|key| {
                let value = properties
                    .get(key)
                    .expect("key came from this map's own keys()");
                (key.clone(), bolt_property_value(value))
            })
            .collect(),
    )
}

/// Project an [`LpgNode`] into Bolt's `Node` structure (tag `4E`, 4 fields —
/// the 5.0+ shape, `element_id` included).
#[must_use]
pub fn bolt_node(node: &LpgNode) -> BoltValue {
    let element_id = node.element_id.to_string();
    BoltValue::Structure {
        signature: signature::NODE,
        fields: vec![
            BoltValue::Integer(legacy_id(&element_id)),
            BoltValue::List(node.labels.iter().cloned().map(BoltValue::String).collect()),
            bolt_properties(&node.properties),
            BoltValue::String(element_id),
        ],
    }
}

/// Project an [`LpgEdge`] into Bolt's `Relationship` structure (tag `52`, 8
/// fields — the 5.0+ shape).
#[must_use]
pub fn bolt_relationship(edge: &LpgEdge) -> BoltValue {
    let element_id = edge.element_id.to_string();
    let start_id = edge.start.to_string();
    let end_id = edge.end.to_string();
    BoltValue::Structure {
        signature: signature::RELATIONSHIP,
        fields: vec![
            BoltValue::Integer(legacy_id(&element_id)),
            BoltValue::Integer(legacy_id(&start_id)),
            BoltValue::Integer(legacy_id(&end_id)),
            BoltValue::String(edge.edge_type.clone()),
            bolt_properties(&edge.properties),
            BoltValue::String(element_id),
            BoltValue::String(start_id),
            BoltValue::String(end_id),
        ],
    }
}

/// Render one [`LossyMapping`] as a Bolt notification dictionary — the
/// `SUCCESS`-summary vocabulary the published protocol defines for a
/// server to warn a driver about something short of a failure, reused here
/// (not any particular server's own notification codes or wording, which
/// `00i-licensing.md` rule 3 forbids copying) for Epic 7c decision 2: a
/// lossy projection is reported, never dropped silently, and `PULL`
/// (`crate::server::drain`) folds one of these into its own summary per
/// entry accumulated across the rows it drained.
#[must_use]
pub fn bolt_notification(loss: &LossyMapping) -> BoltValue {
    let (code, description) = match loss {
        LossyMapping::RefInProperty { subject, predicate } => (
            "RefInProperty",
            format!(
                "`{subject}.{predicate}` is a reference in property position; it survives as \
                 a handle string, not a typed reference."
            ),
        ),
        LossyMapping::NamedGraphCollapse { subject, graphs } => (
            "NamedGraphCollapse",
            format!(
                "`{subject}` is asserted in {} named graphs ({}); only one survives on `_graph`.",
                graphs.len(),
                graphs.join(", ")
            ),
        ),
        LossyMapping::TypeNarrowed {
            subject,
            predicate,
            from,
        } => (
            "TypeNarrowed",
            format!(
                "`{subject}.{predicate}` was a {from} value; it projects as a string and the \
                 type tag does not survive a round trip."
            ),
        ),
    };
    BoltValue::Dictionary(vec![
        (
            "code".to_string(),
            BoltValue::String(format!("GraphOwl.LossyMapping.{code}")),
        ),
        ("description".to_string(), BoltValue::String(description)),
        (
            "severity".to_string(),
            BoltValue::String("WARNING".to_string()),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::flake::Sid;
    use graph_owl_lpg::ElementId;

    fn structure(signature: u8, fields: Vec<BoltValue>) -> BoltValue {
        BoltValue::Structure { signature, fields }
    }

    fn dict(entries: &[(&str, BoltValue)]) -> BoltValue {
        BoltValue::Dictionary(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn hello_decodes_bearer_credentials() {
        let value = structure(
            signature::HELLO,
            vec![dict(&[
                ("scheme", BoltValue::String("bearer".to_string())),
                ("credentials", BoltValue::String("token-123".to_string())),
                (
                    "user_agent",
                    BoltValue::String("test-driver/1.0".to_string()),
                ),
            ])],
        );
        assert_eq!(
            decode_client_message(value),
            Ok(ClientMessage::Hello {
                credentials: Credentials {
                    scheme: "bearer".to_string(),
                    principal: None,
                    credentials: Some("token-123".to_string()),
                },
                user_agent: "test-driver/1.0".to_string(),
            })
        );
    }

    #[test]
    fn hello_with_no_scheme_defaults_to_none() {
        let value = structure(signature::HELLO, vec![dict(&[])]);
        let ClientMessage::Hello { credentials, .. } = decode_client_message(value).unwrap() else {
            panic!("expected Hello");
        };
        assert_eq!(credentials.scheme, "none");
    }

    #[test]
    fn hello_missing_extra_is_an_error() {
        assert_eq!(
            decode_client_message(structure(signature::HELLO, vec![])),
            Err(ProtocolError::MissingField {
                signature: "HELLO",
                field: "extra"
            })
        );
    }

    #[test]
    fn run_decodes_the_query_string() {
        let value = structure(
            signature::RUN,
            vec![
                BoltValue::String("RETURN 1".to_string()),
                BoltValue::Dictionary(vec![]),
                BoltValue::Dictionary(vec![]),
            ],
        );
        assert_eq!(
            decode_client_message(value),
            Ok(ClientMessage::Run {
                query: "RETURN 1".to_string()
            })
        );
    }

    #[test]
    fn pull_decodes_n() {
        let value = structure(
            signature::PULL,
            vec![dict(&[("n", BoltValue::Integer(1000))])],
        );
        assert_eq!(
            decode_client_message(value),
            Ok(ClientMessage::Pull { n: 1000 })
        );
    }

    #[test]
    fn pull_with_n_negative_one_means_drain_all() {
        let value = structure(
            signature::PULL,
            vec![dict(&[("n", BoltValue::Integer(-1))])],
        );
        assert_eq!(
            decode_client_message(value),
            Ok(ClientMessage::Pull { n: -1 })
        );
    }

    #[test]
    fn pull_missing_n_is_an_error() {
        assert_eq!(
            decode_client_message(structure(signature::PULL, vec![dict(&[])])),
            Err(ProtocolError::MissingField {
                signature: "PULL",
                field: "n"
            })
        );
    }

    #[test]
    fn discard_decodes_n() {
        let value = structure(
            signature::DISCARD,
            vec![dict(&[("n", BoltValue::Integer(-1))])],
        );
        assert_eq!(
            decode_client_message(value),
            Ok(ClientMessage::Discard { n: -1 })
        );
    }

    #[test]
    fn no_field_messages_decode_from_an_empty_structure() {
        assert_eq!(
            decode_client_message(structure(signature::BEGIN, vec![])),
            Ok(ClientMessage::Begin)
        );
        assert_eq!(
            decode_client_message(structure(signature::COMMIT, vec![])),
            Ok(ClientMessage::Commit)
        );
        assert_eq!(
            decode_client_message(structure(signature::ROLLBACK, vec![])),
            Ok(ClientMessage::Rollback)
        );
        assert_eq!(
            decode_client_message(structure(signature::RESET, vec![])),
            Ok(ClientMessage::Reset)
        );
        assert_eq!(
            decode_client_message(structure(signature::GOODBYE, vec![])),
            Ok(ClientMessage::Goodbye)
        );
    }

    #[test]
    fn an_unknown_signature_is_an_error() {
        assert_eq!(
            decode_client_message(structure(0x99, vec![])),
            Err(ProtocolError::UnknownSignature(0x99))
        );
    }

    #[test]
    fn success_encodes_signature_and_metadata() {
        let encoded = encode_server_message(&ServerMessage::Success(vec![(
            "fields".to_string(),
            BoltValue::List(vec![BoltValue::String("x".to_string())]),
        )]));
        assert_eq!(
            encoded,
            structure(
                signature::SUCCESS,
                vec![BoltValue::Dictionary(vec![(
                    "fields".to_string(),
                    BoltValue::List(vec![BoltValue::String("x".to_string())])
                )])]
            )
        );
    }

    #[test]
    fn record_wraps_its_values_in_one_list_field() {
        let encoded = encode_server_message(&ServerMessage::Record(vec![
            BoltValue::Integer(1),
            BoltValue::Integer(2),
        ]));
        assert_eq!(
            encoded,
            structure(
                signature::RECORD,
                vec![BoltValue::List(vec![
                    BoltValue::Integer(1),
                    BoltValue::Integer(2)
                ])]
            )
        );
    }

    #[test]
    fn ignored_has_no_fields() {
        assert_eq!(
            encode_server_message(&ServerMessage::Ignored),
            structure(signature::IGNORED, vec![])
        );
    }

    #[test]
    fn failure_carries_code_and_message() {
        let encoded = encode_server_message(&ServerMessage::Failure {
            code: "Example.Failure.Code".to_string(),
            message: "boom".to_string(),
        });
        assert_eq!(
            encoded,
            structure(
                signature::FAILURE,
                vec![BoltValue::Dictionary(vec![
                    (
                        "code".to_string(),
                        BoltValue::String("Example.Failure.Code".to_string())
                    ),
                    ("message".to_string(), BoltValue::String("boom".to_string())),
                ])]
            )
        );
    }

    fn a_node() -> LpgNode {
        let mut properties = PropertyMap::new();
        properties
            .insert_user("name", PropertyValue::String("Ada".to_string()))
            .unwrap();
        LpgNode {
            element_id: ElementId::encode(&Sid::dsc("table-1")),
            labels: vec!["Table".to_string()],
            properties,
        }
    }

    #[test]
    fn a_node_encodes_as_a_4_field_structure_with_the_node_signature() {
        let encoded = bolt_node(&a_node());
        let BoltValue::Structure { signature, fields } = encoded else {
            panic!("expected a structure");
        };
        assert_eq!(signature, signature::NODE);
        assert_eq!(fields.len(), 4, "id, labels, properties, element_id");
        assert_eq!(
            fields[1],
            BoltValue::List(vec![BoltValue::String("Table".to_string())])
        );
        assert_eq!(
            fields[3],
            BoltValue::String(ElementId::encode(&Sid::dsc("table-1")).to_string())
        );
    }

    #[test]
    fn a_relationship_encodes_as_an_8_field_structure_with_the_relationship_signature() {
        let mut properties = PropertyMap::new();
        properties
            .insert_user("confidence", PropertyValue::Float(0.9))
            .unwrap();
        let edge = LpgEdge {
            element_id: ElementId::encode(&Sid::dsc("rel-1")),
            edge_type: "FEEDS".to_string(),
            start: ElementId::encode(&Sid::dsc("table-1")),
            end: ElementId::encode(&Sid::dsc("table-2")),
            properties,
        };
        let encoded = bolt_relationship(&edge);
        let BoltValue::Structure { signature, fields } = encoded else {
            panic!("expected a structure");
        };
        assert_eq!(signature, signature::RELATIONSHIP);
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[3], BoltValue::String("FEEDS".to_string()));
        assert_eq!(
            fields[5],
            BoltValue::String(ElementId::encode(&Sid::dsc("rel-1")).to_string())
        );
        assert_eq!(
            fields[6],
            BoltValue::String(ElementId::encode(&Sid::dsc("table-1")).to_string())
        );
        assert_eq!(
            fields[7],
            BoltValue::String(ElementId::encode(&Sid::dsc("table-2")).to_string())
        );
    }

    #[test]
    fn the_same_element_id_always_produces_the_same_legacy_id() {
        assert_eq!(legacy_id("1:table-1"), legacy_id("1:table-1"));
    }

    #[test]
    fn different_element_ids_produce_different_legacy_ids() {
        assert_ne!(legacy_id("1:table-1"), legacy_id("1:table-2"));
    }

    fn dict_entries(value: &BoltValue) -> &[(String, BoltValue)] {
        match value {
            BoltValue::Dictionary(entries) => entries,
            other => panic!("expected a dictionary, got {other:?}"),
        }
    }

    #[test]
    fn a_notification_names_its_code_description_and_severity() {
        let loss = LossyMapping::TypeNarrowed {
            subject: "table-1".to_string(),
            predicate: "properties".to_string(),
            from: "json",
        };
        let notification = bolt_notification(&loss);
        let entries = dict_entries(&notification);
        assert_eq!(
            entries.iter().find(|(k, _)| k == "code").map(|(_, v)| v),
            Some(&BoltValue::String(
                "GraphOwl.LossyMapping.TypeNarrowed".to_string()
            ))
        );
        assert_eq!(
            entries
                .iter()
                .find(|(k, _)| k == "severity")
                .map(|(_, v)| v),
            Some(&BoltValue::String("WARNING".to_string()))
        );
        let BoltValue::String(description) = entries
            .iter()
            .find(|(k, _)| k == "description")
            .map(|(_, v)| v)
            .expect("a description")
        else {
            panic!("description is not a string");
        };
        assert!(description.contains("table-1"), "{description}");
        assert!(description.contains("properties"), "{description}");
    }

    /// Each variant gets its **own** code — a mutation collapsing two match
    /// arms to the same string still produces a dictionary, so only
    /// distinctness across variants catches it.
    #[test]
    fn different_lossy_variants_produce_different_codes() {
        let code_of = |loss: &LossyMapping| {
            let notification = bolt_notification(loss);
            let entries = dict_entries(&notification);
            entries
                .iter()
                .find(|(k, _)| k == "code")
                .map(|(_, v)| v.clone())
        };
        let ref_in_property = code_of(&LossyMapping::RefInProperty {
            subject: "s".to_string(),
            predicate: "p".to_string(),
        });
        let named_graph_collapse = code_of(&LossyMapping::NamedGraphCollapse {
            subject: "s".to_string(),
            graphs: vec!["a".to_string(), "b".to_string()],
        });
        let type_narrowed = code_of(&LossyMapping::TypeNarrowed {
            subject: "s".to_string(),
            predicate: "p".to_string(),
            from: "uuid",
        });
        assert_ne!(ref_in_property, named_graph_collapse);
        assert_ne!(named_graph_collapse, type_narrowed);
        assert_ne!(ref_in_property, type_narrowed);
    }
}
