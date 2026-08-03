//! Element ids — Epic 7c Slice B.
//!
//! **A handle a client holds across restarts.** Bolt (7d), Cypher (7b), the
//! graph explorer (40) and `GraphML` export (9a) all hand these to callers and
//! accept them back. Getting the encoding wrong is a correctness bug that shows
//! up only after a restart, a replica switch, or a reindex — by which time the
//! client has stored ids that now mean something else.
//!
//! **Derived from [`Sid`], never assigned.** The conventional property-graph
//! choice is an auto-incrementing integer, which does not survive a restart, is
//! not stable across replicas, and silently renumbers after a reindex.
//! Derivation costs an encode per projection and is worth it.

use graph_owl_core::flake::Sid;

/// A stable, reversible handle for a node or an edge.
///
/// Two properties, and the second is the one that is easy to get wrong:
///
/// 1. **Reversible** — `Sid → ElementId → Sid` is exact, so a client's handle
///    resolves back to the thing it names without a lookup table.
/// 2. **Injection-proof** — no id, however punctuated, can be encoded such that
///    it decodes as a *different* `Sid`. See [`ElementId::encode`] for how, and
///    the tests for the case that motivates it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId(String);

/// Separates the namespace code from the id.
///
/// A colon, and the choice is load-bearing rather than aesthetic — see
/// [`ElementId::encode`].
const SEPARATOR: char = ':';

impl ElementId {
    /// Encode a `Sid`.
    ///
    /// **`{namespace_code}:{id}`, decoded by splitting on the *first* colon.**
    ///
    /// That is the whole trick, and it is chosen so that there is no escaping to
    /// get wrong. A namespace code is a decimal `u16`, so it cannot itself
    /// contain a colon; everything after the first one is therefore the id
    /// *verbatim*, however many colons, slashes or newlines it holds. No
    /// escaping means no unescaping, which means no escaping bug — and the
    /// classic failure here is exactly an escaping bug, because it produces
    /// **cross-entity id collisions** rather than a parse error.
    ///
    /// The tempting alternatives are both worse. Naive concatenation with a
    /// delimiter needs escaping and breaks the moment an id contains the
    /// delimiter. Hashing is not reversible, so every handle a client returns
    /// would need a lookup table that must itself survive a restart — which is
    /// the problem this is solving.
    #[must_use]
    pub fn encode(sid: &Sid) -> Self {
        Self(format!("{}{SEPARATOR}{}", sid.namespace_code, sid.id))
    }

    /// Decode back to a `Sid`.
    ///
    /// # Errors
    ///
    /// [`ElementIdError`] when the handle is not one this server issued.
    /// **Never a silent miss**: a client that mangled an id gets told, rather
    /// than getting an empty result it will read as "the thing was deleted".
    pub fn decode(&self) -> Result<Sid, ElementIdError> {
        let (namespace, id) = self
            .0
            .split_once(SEPARATOR)
            .ok_or(ElementIdError::Malformed)?;
        let namespace_code = namespace
            .parse::<u16>()
            .map_err(|_| ElementIdError::Malformed)?;
        // An empty id is refused rather than accepted as an empty-string
        // subject: nothing in the graph is named by the empty string, so
        // admitting it would produce a handle that resolves to nothing and
        // reads as a deletion.
        if id.is_empty() {
            return Err(ElementIdError::Malformed);
        }
        Ok(Sid::new(namespace_code, id))
    }

    /// The handle as a client sees it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Adopt a handle a client sent back.
    ///
    /// Deliberately does **not** validate — [`ElementId::decode`] is where a
    /// malformed handle is caught, and validating in two places means two
    /// answers to "is this valid" that can drift apart.
    #[must_use]
    pub fn from_wire(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

impl std::fmt::Display for ElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for ElementId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ElementIdError {
    #[error(
        "not an element id this server issued; element ids are \
         `<namespace>:<id>` and are only meaningful to the server that made them"
    )]
    Malformed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(namespace: u16, id: &str) -> Sid {
        Sid::new(namespace, id)
    }

    #[test]
    fn an_element_id_round_trips() {
        let original = sid(1, "asset/7f3a");

        let decoded = ElementId::encode(&original).decode().expect("decodes");

        assert_eq!(decoded, original);
    }

    /// **The separator-injection test, and it is the point of the slice.**
    ///
    /// An id containing the delimiter is the classic encoding bug, and its
    /// failure mode is the worst available here: not a parse error, but two
    /// different entities decoding to the same handle.
    #[test]
    fn an_id_containing_the_separator_still_round_trips() {
        for punctuated in [
            "a:b",
            "a:b:c",
            ":leading",
            "trailing:",
            "::",
            "urn:uuid:9f8e:7d6c",
        ] {
            let original = sid(1, punctuated);

            let decoded = ElementId::encode(&original).decode().expect("decodes");

            assert_eq!(decoded, original, "`{punctuated}` did not survive");
        }
    }

    /// And the collision that injection would cause is impossible: two `Sid`s
    /// that differ at all encode differently.
    #[test]
    fn two_different_sids_never_share_a_handle() {
        // `(1, "2:x")` and `(12, ":x")` are the adversarial pair — a naive
        // encoding that split on the *last* separator, or that concatenated
        // without one, would map both to the same string.
        let pairs = [
            (sid(1, "2:x"), sid(12, ":x")),
            (sid(1, "a"), sid(1, "a ")),
            (sid(1, "a"), sid(2, "a")),
            (sid(0, "1:1"), sid(1, "1")),
        ];

        for (left, right) in pairs {
            assert_ne!(left, right, "the fixture pair must differ");
            assert_ne!(
                ElementId::encode(&left),
                ElementId::encode(&right),
                "{left:?} and {right:?} collided"
            );
        }
    }

    /// **Stable across restarts**, asserted against a literal rather than
    /// against state — an id derived from anything process-local would pass a
    /// round-trip test and fail in production after a deploy.
    #[test]
    fn an_element_id_is_derived_only_from_the_sid() {
        assert_eq!(
            ElementId::encode(&sid(1, "asset/7f3a")).as_str(),
            "1:asset/7f3a"
        );
        assert_eq!(ElementId::encode(&sid(0, "x")).as_str(), "0:x");
        assert_eq!(ElementId::encode(&sid(65535, "x")).as_str(), "65535:x");
    }

    /// Encoding the same `Sid` twice gives the same bytes — required by every
    /// serialization downstream, and the cheapest possible guard against
    /// somebody reaching for a counter.
    #[test]
    fn encoding_is_deterministic() {
        let original = sid(1, "asset/7f3a");

        assert_eq!(ElementId::encode(&original), ElementId::encode(&original));
    }

    /// **A mangled handle is an error, never a silent miss.** An empty result
    /// would be read as "the thing was deleted", which is a different and much
    /// more alarming statement than "that is not a valid id".
    #[test]
    fn a_malformed_handle_is_refused_rather_than_missed() {
        for bad in [
            "no-separator",
            "",
            "notanumber:x",
            // A namespace code past `u16`.
            "70000:x",
            // Present separator, absent id.
            "1:",
            "-1:x",
        ] {
            assert_eq!(
                ElementId::from_wire(bad).decode(),
                Err(ElementIdError::Malformed),
                "`{bad}` should be refused"
            );
        }
    }

    /// **`Display` renders the handle**, not an empty string. Nothing else
    /// asserted its output, so a `fmt` that wrote nothing passed the whole file
    /// — and this is the form that reaches a log line or an error message,
    /// where an empty id is indistinguishable from a missing one.
    #[test]
    fn display_renders_the_handle() {
        let rendered = ElementId::encode(&sid(1, "asset/7f3a")).to_string();

        assert_eq!(rendered, "1:asset/7f3a");
        assert_eq!(
            rendered,
            ElementId::encode(&sid(1, "asset/7f3a")).as_str(),
            "`Display` and `as_str` must not diverge"
        );
    }

    /// The wire form is a plain string, because every consumer — Bolt, JSON,
    /// `GraphML` — has strings and none of them share a richer type.
    #[test]
    fn an_element_id_serializes_as_a_string() {
        let json = serde_json::to_value(ElementId::encode(&sid(1, "a:b"))).expect("serialize");

        assert_eq!(json, serde_json::Value::String("1:a:b".to_string()));
    }
}
