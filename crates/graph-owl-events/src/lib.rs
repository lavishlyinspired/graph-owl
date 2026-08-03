//! `EventSink` port + `ChangeEvent` — Epic 3, Slice J.
//!
//! Epic 8's search index is a **derived** store: it lives outside the write
//! transaction, so it can drift from its source and has to be told what
//! changed. This is what it subscribes to instead of polling
//! (`08-engine-search.md`).

pub mod webhook;

use chrono::{DateTime, Utc};
use graph_owl_core::AssetKind;
use graph_owl_core::envelope::{ChangeDescription, EntityVersion};
use serde::{Deserialize, Serialize};

/// What happened to the entity.
///
/// Soft delete and restore are their own kinds rather than updates carrying a
/// flag. A subscriber's reaction to them is categorically different — a search
/// index *removes* a document on one and *adds* it on the other — and a flag
/// inside an `Updated` event makes that difference something every consumer
/// must remember to look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Created,
    Updated,
    SoftDeleted,
    Restored,
    HardDeleted,
}

/// Which entity the event is about.
///
/// Carried in full so a subscriber never has to read the entity back. One that
/// did would race the next write and index a version it was never told about —
/// and it would turn every event into a database round trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSubject {
    pub kind: AssetKind,
    pub id: String,
    pub fqn: String,
}

/// One committed change.
///
/// `previous_version` and `current_version` are both optional because the ends
/// of a lifetime genuinely lack one: a creation has no previous state and a
/// hard delete has no current one. Modelling those as a sentinel version would
/// make "was it created" a value comparison rather than a shape.
///
/// `PartialEq` but not `Eq`: the diff holds `serde_json::Value`, whose floats
/// are not reflexively equal. Deriving `Eq` here would be a claim the payload
/// cannot support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEvent {
    pub kind: EventKind,
    pub subject: EventSubject,
    pub previous_version: Option<EntityVersion>,
    pub current_version: Option<EntityVersion>,
    pub change: ChangeDescription,
    pub at: DateTime<Utc>,
    pub principal_id: String,
}

impl ChangeEvent {
    fn new(
        kind: EventKind,
        subject: EventSubject,
        previous_version: Option<EntityVersion>,
        current_version: Option<EntityVersion>,
        change: ChangeDescription,
        principal_id: &str,
    ) -> Self {
        Self {
            kind,
            subject,
            previous_version,
            current_version,
            change,
            at: Utc::now(),
            principal_id: principal_id.to_string(),
        }
    }

    #[must_use]
    pub fn created(subject: EventSubject, at_version: EntityVersion, principal_id: &str) -> Self {
        Self::new(
            EventKind::Created,
            subject,
            None,
            Some(at_version),
            ChangeDescription::default(),
            principal_id,
        )
    }

    /// `None` when nothing changed.
    ///
    /// The no-op rule lives here rather than in the caller, so a facade that
    /// forgot to check still cannot emit an event with nothing to report. A
    /// version bump alone is not a change — `03-versioning.md` says a no-op
    /// update produces no version, so equal versions and an empty diff are the
    /// same statement made twice.
    #[must_use]
    pub fn updated(
        subject: EventSubject,
        from: EntityVersion,
        to: EntityVersion,
        change: ChangeDescription,
        principal_id: &str,
    ) -> Option<Self> {
        if change.is_empty() {
            return None;
        }
        Some(Self::new(
            EventKind::Updated,
            subject,
            Some(from),
            Some(to),
            change,
            principal_id,
        ))
    }

    /// Deletion and restore change no *field*, so they carry an empty diff and
    /// must not go through [`Self::updated`], which would drop them.
    #[must_use]
    pub fn soft_deleted(
        subject: EventSubject,
        from: EntityVersion,
        to: EntityVersion,
        principal_id: &str,
    ) -> Self {
        Self::new(
            EventKind::SoftDeleted,
            subject,
            Some(from),
            Some(to),
            ChangeDescription::default(),
            principal_id,
        )
    }

    #[must_use]
    pub fn restored(
        subject: EventSubject,
        from: EntityVersion,
        to: EntityVersion,
        principal_id: &str,
    ) -> Self {
        Self::new(
            EventKind::Restored,
            subject,
            Some(from),
            Some(to),
            ChangeDescription::default(),
            principal_id,
        )
    }

    #[must_use]
    pub fn hard_deleted(
        subject: EventSubject,
        last_version: EntityVersion,
        principal_id: &str,
    ) -> Self {
        Self::new(
            EventKind::HardDeleted,
            subject,
            Some(last_version),
            None,
            ChangeDescription::default(),
            principal_id,
        )
    }
}

/// Where committed changes go.
///
/// **`emit` returns nothing, deliberately.** "Emission failure does not fail
/// the request" (`03-versioning.md` Slice J) is then a property of the
/// signature rather than a discipline every call site has to remember: there is
/// no error for a caller to propagate by accident. A sink that cannot deliver
/// reports it the only way it can — by logging — because a catalog write must
/// not fail because a search index was unreachable.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &ChangeEvent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::AssetKind;
    use graph_owl_core::envelope::{ChangeDescription, EntityVersion};

    fn changed() -> ChangeDescription {
        ChangeDescription::between(
            &serde_json::json!({ "description": "before" }),
            &serde_json::json!({ "description": "after" }),
        )
    }

    fn subject() -> EventSubject {
        EventSubject {
            kind: AssetKind::Table,
            id: "asset-1".to_string(),
            fqn: "svc.db.schema.customers".to_string(),
        }
    }

    mod what_an_event_carries {
        use super::*;

        #[test]
        fn a_creation_has_a_current_version_and_no_previous_one() {
            let event = ChangeEvent::created(subject(), EntityVersion::initial(), "alice");

            assert_eq!(event.kind, EventKind::Created);
            assert_eq!(event.previous_version, None);
            assert_eq!(event.current_version, Some(EntityVersion::initial()));
            assert_eq!(event.principal_id, "alice");
        }

        #[test]
        fn a_hard_deletion_has_a_previous_version_and_no_current_one() {
            let was = EntityVersion { major: 1, minor: 2 };
            let event = ChangeEvent::hard_deleted(subject(), was, "alice");

            assert_eq!(event.kind, EventKind::HardDeleted);
            assert_eq!(event.previous_version, Some(was));
            assert_eq!(event.current_version, None);
        }

        #[test]
        fn an_update_carries_both_versions_and_the_field_diff() {
            let before = EntityVersion::initial();
            let after = before.next_minor();
            let event = ChangeEvent::updated(subject(), before, after, changed(), "alice")
                .expect("a real change produces an event");

            assert_eq!(event.previous_version, Some(before));
            assert_eq!(event.current_version, Some(after));
            assert!(!event.change.is_empty());
        }

        #[test]
        fn the_subject_travels_with_the_event_so_a_consumer_need_not_read_back() {
            // A subscriber that had to fetch the entity to learn its FQN would
            // race the next write and index a version it was never told about.
            let event = ChangeEvent::created(subject(), EntityVersion::initial(), "alice");

            assert_eq!(event.subject.kind, AssetKind::Table);
            assert_eq!(event.subject.id, "asset-1");
            assert_eq!(event.subject.fqn, "svc.db.schema.customers");
        }
    }

    mod a_no_op_is_not_a_change {
        use super::*;

        #[test]
        fn an_update_with_an_empty_diff_produces_no_event() {
            // The rule is structural rather than a caller's duty: `updated`
            // cannot return an event there is nothing to say. A facade that
            // forgot to check would still not emit one.
            let v = EntityVersion::initial();
            let nothing = ChangeDescription::between(
                &serde_json::json!({ "description": "same" }),
                &serde_json::json!({ "description": "same" }),
            );

            assert_eq!(
                ChangeEvent::updated(subject(), v, v, nothing, "alice"),
                None
            );
        }

        #[test]
        fn a_soft_delete_is_a_change_even_though_no_field_moved() {
            // Deletion changes no *field*, so a diff-driven rule would drop it.
            // It is the event a search index most needs.
            let before = EntityVersion::initial();
            let event = ChangeEvent::soft_deleted(subject(), before, before.next_minor(), "alice");

            assert_eq!(event.kind, EventKind::SoftDeleted);
            assert!(event.change.is_empty());
        }

        #[test]
        fn a_restore_is_its_own_kind_not_an_update() {
            let before = EntityVersion::initial();
            let event = ChangeEvent::restored(subject(), before, before.next_minor(), "alice");

            assert_eq!(event.kind, EventKind::Restored);
        }
    }

    mod emission_cannot_fail_the_request {
        use super::*;
        use std::sync::Mutex;

        struct Recording(Mutex<Vec<ChangeEvent>>);

        impl EventSink for Recording {
            fn emit(&self, event: &ChangeEvent) {
                self.0.lock().expect("lock").push(event.clone());
            }
        }

        struct Broken;

        impl EventSink for Broken {
            fn emit(&self, _: &ChangeEvent) {
                // A sink whose transport is down. It has nowhere to report
                // that, which is the point.
            }
        }

        #[test]
        fn a_sink_records_what_it_is_given() {
            let sink = Recording(Mutex::new(Vec::new()));
            sink.emit(&ChangeEvent::created(
                subject(),
                EntityVersion::initial(),
                "alice",
            ));

            assert_eq!(sink.0.lock().expect("lock").len(), 1);
        }

        #[test]
        fn a_broken_sink_returns_normally_because_emit_yields_nothing_to_propagate() {
            // `emit` returns `()`, so "emission failure does not fail the
            // request" is enforced by the signature rather than by every
            // caller remembering to swallow an error.
            let sink = Broken;
            sink.emit(&ChangeEvent::created(
                subject(),
                EntityVersion::initial(),
                "alice",
            ));
        }
    }
}
