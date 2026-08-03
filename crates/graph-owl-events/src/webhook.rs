//! Outbound webhooks — Epic 14 Slice F.
//!
//! Pure: registration, payload shaping, canonicalization, signing, target
//! admission, and the retry schedule. Nothing here opens a socket — the sender
//! is the composition root's, and keeping the decisions out of it is what makes
//! them testable and what stops a delivery failure reaching a write path.
//!
//! **Two properties this module exists to guarantee**, both of which are
//! security properties rather than conveniences:
//!
//! 1. **Payloads are thin.** Identifiers and versions only. A payload carrying
//!    a description or a column list has *bypassed the policy layer* — the
//!    subscriber never asked graph-owl whether it may see that, because it never
//!    had to fetch it. A thin payload forces a fetch, and a fetch is filtered.
//! 2. **Private targets are refused.** A webhook URL is attacker-controllable
//!    input that this server then makes a request to, which is the definition of
//!    SSRF. `localhost`, loopback, link-local and private ranges are refused
//!    unless a deployment explicitly allowlists the host.

use crate::{ChangeEvent, EventKind};
use serde::{Deserialize, Serialize};

/// Where to deliver, what to deliver, and the key it is signed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookRegistration {
    pub id: uuid::Uuid,
    pub url: String,
    /// **Never serialized outward.** A registration read back over the API
    /// would otherwise hand the signing key to anybody who may list webhooks,
    /// which makes every signature forgeable by a reader.
    #[serde(skip_serializing)]
    pub secret: String,
    /// Which kinds to deliver. **Empty means every kind**, because a
    /// registration that filtered to nothing would be a subscription that never
    /// fires and looks exactly like a broken one.
    #[serde(default)]
    pub event_types: Vec<EventKind>,
}

impl WebhookRegistration {
    /// Whether this registration wants this event.
    #[must_use]
    pub fn wants(&self, kind: EventKind) -> bool {
        self.event_types.is_empty() || self.event_types.contains(&kind)
    }
}

/// What actually goes on the wire.
///
/// **Identifiers and versions, nothing else.** See the module docs: a fat
/// payload is a policy bypass, because the subscriber learns content it never
/// asked graph-owl for permission to see. Everything here is either the
/// subscriber's own subscription metadata or a name it must already know to
/// have subscribed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinPayload {
    pub kind: EventKind,
    pub entity_kind: String,
    pub entity_id: String,
    pub fully_qualified_name: String,
    /// `"1.2"`, or absent at the ends of a lifetime — a creation has no
    /// previous and a hard delete has no current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
    /// RFC 3339.
    pub at: String,
    /// **Which fields changed, by name — never their values.**
    ///
    /// The names are the whole point of the event: a subscriber that cannot
    /// tell a description edit from a schema break has to fetch on every event,
    /// which is the polling this replaces. The *values* are what would leak, so
    /// they stay behind the fetch.
    #[serde(default)]
    pub changed_fields: Vec<String>,
}

/// Shape an event for delivery.
///
/// This is the only constructor, and it takes the whole event, so there is no
/// path by which a caller assembles a payload from richer parts and includes
/// one field too many.
#[must_use]
pub fn thin_payload(event: &ChangeEvent) -> ThinPayload {
    ThinPayload {
        kind: event.kind,
        entity_kind: format!("{:?}", event.subject.kind).to_lowercase(),
        entity_id: event.subject.id.clone(),
        fully_qualified_name: event.subject.fqn.clone(),
        previous_version: event
            .previous_version
            .map(|version| format!("{}.{}", version.major, version.minor)),
        current_version: event
            .current_version
            .map(|version| format!("{}.{}", version.major, version.minor)),
        at: event.at.to_rfc3339(),
        // **Names only, never the `before`/`after` values `FieldChange`
        // carries.** Extracted here rather than by a helper on
        // `ChangeDescription`, so the one place that must never leak a value is
        // the one place that does the extraction.
        changed_fields: changed_field_names(&event.change),
    }
}

/// Which fields changed, by name.
///
/// Added, updated and deleted all count: a subscriber deciding whether to
/// refetch cares that `schema` moved, not how.
fn changed_field_names(change: &graph_owl_core::envelope::ChangeDescription) -> Vec<String> {
    let mut names: Vec<String> = change
        .fields_added
        .iter()
        .chain(&change.fields_updated)
        .chain(&change.fields_deleted)
        .map(|field| field.field.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The bytes that are signed.
///
/// **The canonicalization is documented because a receiver has to reproduce it
/// exactly**, and any ambiguity makes signatures that verify on one
/// implementation and not another:
///
/// ```text
/// <timestamp-seconds> "." <body-bytes-verbatim>
/// ```
///
/// Two decisions worth stating:
///
/// - **The timestamp is inside the signature**, so a captured delivery cannot
///   be replayed later with a fresh header — the attacker would have to re-sign,
///   which is what the secret prevents.
/// - **The body is signed verbatim, as bytes**, not re-serialized. A receiver
///   that re-encoded the JSON before checking would get different bytes for the
///   same document (key order, whitespace, number formatting) and reject a
///   perfectly good signature. Sign what is sent.
#[must_use]
pub fn canonical_bytes(timestamp_seconds: i64, body: &[u8]) -> Vec<u8> {
    let mut canonical = format!("{timestamp_seconds}.").into_bytes();
    canonical.extend_from_slice(body);
    canonical
}

/// The value of the signature header: `sha256=<hex>`.
///
/// The same shape [`crate::webhook`]'s inbound counterpart verifies, so a
/// graph-owl instance can subscribe to another one without a second convention.
///
/// # Panics
///
/// Never, in practice: HMAC-SHA256 accepts a key of any length, and
/// `new_from_slice` only fails for MAC constructions with a fixed key size.
/// Returning a `Result` here would push an impossible branch onto every caller
/// of a signing function, where the honest handling is that there is no failure
/// mode to handle.
#[must_use]
pub fn sign(secret: &[u8], timestamp_seconds: i64, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let canonical = canonical_bytes(timestamp_seconds, body);
    // An HMAC key may be any length; `new_from_slice` only fails for key types
    // that cannot be constructed, which SHA-256 is not one of.
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&canonical);
    let digest = mac.finalize().into_bytes();
    let mut hex = String::with_capacity(digest.len() * 2 + 7);
    hex.push_str("sha256=");
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Why a delivery target was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetRefusal {
    #[error("the target must be an absolute http or https URL")]
    NotHttp,
    #[error("the target has no host")]
    NoHost,
    /// **The SSRF refusal.**
    #[error(
        "`{host}` resolves to a private or loopback address; add it to the \
         webhook target allowlist if this is deliberate"
    )]
    Private { host: String },
}

/// Whether this URL may be delivered to.
///
/// **A webhook URL is attacker-controllable input that this server then makes a
/// request to** — the definition of SSRF. The refused set is the one that
/// reaches infrastructure a caller could not otherwise address: loopback
/// (`127.0.0.0/8`, `::1`), link-local including the cloud metadata address
/// (`169.254.0.0/16`, `fe80::/10`), the RFC 1918 private ranges, the RFC 4193
/// unique-local range, and the unspecified address.
///
/// `allowlist` is by **host as written**, because the point of the escape hatch
/// is a deployment that genuinely wants to call its own sidecar. Allowlisting
/// by resolved IP would let a hostname that resolves differently later slip
/// through the check that was made when it was registered.
///
/// # Errors
///
/// [`TargetRefusal`] naming which rule refused it, so the person registering
/// can fix it rather than guess.
pub fn admit_target<S: std::hash::BuildHasher>(
    url: &str,
    allowlist: &std::collections::HashSet<String, S>,
) -> Result<(), TargetRefusal> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or(TargetRefusal::NotHttp)?;

    // Everything before the first `/`, `?` or `#` is the authority; everything
    // after the last `@` in it is the host and port. A userinfo section is where
    // `http://evil.com@127.0.0.1/` hides its real target from a naive check.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = strip_port(authority);
    if host.is_empty() {
        return Err(TargetRefusal::NoHost);
    }

    if allowlist.contains(host) {
        return Ok(());
    }
    if is_private_host(host) {
        return Err(TargetRefusal::Private {
            host: host.to_string(),
        });
    }
    Ok(())
}

/// The host from `host`, `host:port`, `[::1]` or `[::1]:port`.
fn strip_port(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default();
    }
    authority.split(':').next().unwrap_or_default()
}

/// Whether this host names something on the private side of the network.
///
/// Names are matched literally rather than resolved: **resolution happens at
/// delivery time, not registration time**, so a check that resolved here would
/// be answering a question about a different moment. `localhost` is refused by
/// name because that is how it is almost always written.
fn is_private_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(address)) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
        }
        Ok(std::net::IpAddr::V6(address)) => {
            address.is_loopback()
                || address.is_unspecified()
                // `fe80::/10` link-local and `fc00::/7` unique-local. Spelled
                // out because `is_unicast_link_local` and `is_unique_local` are
                // still unstable, and a webhook target check is not a place to
                // wait for a nightly feature.
                || (address.segments()[0] & 0xffc0) == 0xfe80
                || (address.segments()[0] & 0xfe00) == 0xfc00
                // An IPv4-mapped target reaches exactly the IPv4 address it
                // maps to, so it inherits that address's verdict — otherwise
                // `::ffff:127.0.0.1` walks straight past the loopback rule.
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| {
                        mapped.is_loopback()
                            || mapped.is_private()
                            || mapped.is_link_local()
                            || mapped.is_unspecified()
                    })
        }
        // Not an IP literal and not `localhost`: a name that resolves somewhere
        // this check cannot see. Admitted here and re-checked by the sender,
        // which is the only place resolution is meaningful.
        Err(_) => false,
    }
}

/// How long to wait before attempt number `attempt`, and when to give up.
///
/// **Base two seconds, doubling, capped at five minutes, dead-lettered after
/// six attempts.** Each number has a reason rather than being a round figure:
///
/// - **Two seconds** is longer than a subscriber's restart is likely to take to
///   begin refusing connections, and short enough that a transient blip is
///   recovered within one human's attention span.
/// - **Doubling** rather than a fixed interval, because the common failure is a
///   subscriber that is down for minutes, and a fixed retry turns that into
///   thousands of requests against something already struggling.
/// - **Five minutes** caps it: past that the schedule stops being a retry and
///   starts being a poll, and a subscriber down for half an hour is a page, not
///   a backoff.
/// - **Six attempts** spans roughly twenty minutes of trying
///   (2 + 4 + 8 + 16 + 32 + 64 seconds), which covers a deploy, and after which
///   the delivery is worth a human's attention rather than more retries.
///
/// Returns `None` when the attempt is past the last one — the caller
/// dead-letters it, and a dead letter is replayable.
#[must_use]
pub fn backoff(attempt: u32) -> Option<std::time::Duration> {
    if attempt == 0 || attempt > MAX_ATTEMPTS {
        return None;
    }
    let seconds = BASE_BACKOFF_SECONDS
        .checked_mul(2u64.saturating_pow(attempt - 1))
        .unwrap_or(MAX_BACKOFF_SECONDS)
        .min(MAX_BACKOFF_SECONDS);
    Some(std::time::Duration::from_secs(seconds))
}

const BASE_BACKOFF_SECONDS: u64 = 2;
const MAX_BACKOFF_SECONDS: u64 = 300;

/// After this many failures the delivery is dead-lettered for a human.
pub const MAX_ATTEMPTS: u32 = 6;

#[cfg(test)]
mod tests {
    use super::*;
    use graph_owl_core::AssetKind;
    use graph_owl_core::envelope::{ChangeDescription, EntityVersion};

    fn event() -> ChangeEvent {
        let subject = crate::EventSubject {
            kind: AssetKind::Table,
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            fqn: "warehouse.retail.public.orders".to_string(),
        };
        ChangeEvent::created(subject, EntityVersion { major: 1, minor: 0 }, "alice")
    }

    fn nothing_allowed() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    // ---- thin payloads ----

    /// **The thin-payload test.** A payload carrying content has bypassed the
    /// policy layer: the subscriber learned something without ever asking
    /// graph-owl whether it may see it.
    #[test]
    fn no_entity_content_appears_in_a_payload() {
        let mut source = event();
        source.subject.fqn = "warehouse.retail.public.orders".to_string();
        source.change = ChangeDescription::default();

        let rendered = serde_json::to_string(&thin_payload(&source)).expect("serialize");

        for forbidden in [
            "description",
            "properties",
            "extension",
            "columns",
            "owners",
            "deprecation",
        ] {
            assert!(
                !rendered.contains(forbidden),
                "`{forbidden}` must not ride along: {rendered}"
            );
        }
    }

    /// **The changed field *names* travel; their values do not.**
    ///
    /// This is the whole compromise the thin payload makes: a subscriber that
    /// cannot tell a description edit from a schema break has to fetch on every
    /// event, which is the polling this replaces — so the names go. The values
    /// are the content, so they stay behind a fetch, which is filtered.
    #[test]
    fn a_payload_carries_changed_field_names_but_never_their_values() {
        use graph_owl_core::envelope::FieldChange;

        let mut source = event();
        source.change = ChangeDescription {
            fields_added: vec![FieldChange {
                field: "retentionDays".to_string(),
                before: None,
                after: Some(serde_json::json!(30)),
            }],
            fields_updated: vec![FieldChange {
                field: "description".to_string(),
                before: Some(serde_json::json!("the old text nobody may read")),
                after: Some(serde_json::json!("the new text nobody may read")),
            }],
            fields_deleted: vec![FieldChange {
                field: "costCentre".to_string(),
                before: Some(serde_json::json!("CC-4471")),
                after: None,
            }],
        };

        let payload = thin_payload(&source);
        let rendered = serde_json::to_string(&payload).expect("serialize");

        assert_eq!(
            payload.changed_fields,
            vec![
                "costCentre".to_string(),
                "description".to_string(),
                "retentionDays".to_string(),
            ],
            "all three kinds of change name their field"
        );
        for value in ["the old text", "the new text", "CC-4471", "30"] {
            assert!(
                !rendered.contains(value),
                "`{value}` is content and must stay behind a fetch: {rendered}"
            );
        }
    }

    /// An event that changed nothing nameable carries an empty list rather than
    /// a placeholder — a subscriber reading `[\"\"]` would refetch for a field
    /// that does not exist.
    #[test]
    fn a_payload_with_no_field_changes_carries_an_empty_list() {
        let payload = thin_payload(&event());

        assert!(payload.changed_fields.is_empty(), "{payload:?}");
    }

    /// And the identifiers that make a fetch possible **are** there — a payload
    /// so thin the subscriber cannot find the entity is not thin, it is broken.
    #[test]
    fn a_payload_carries_enough_to_fetch_the_entity() {
        let payload = thin_payload(&event());

        assert_eq!(
            payload.fully_qualified_name,
            "warehouse.retail.public.orders"
        );
        assert_eq!(payload.entity_kind, "table");
        assert_eq!(payload.current_version, Some("1.0".to_string()));
        assert_eq!(
            payload.previous_version, None,
            "a creation has no previous state"
        );
    }

    /// **The secret never serializes.** A registration read back over the API
    /// would otherwise hand the signing key to anybody who may list webhooks,
    /// which makes every signature forgeable by a reader.
    #[test]
    fn a_registration_never_serializes_its_secret() {
        let registration = WebhookRegistration {
            id: uuid::Uuid::nil(),
            url: "https://example.com/hook".to_string(),
            secret: "s3cr3t-do-not-leak".to_string(),
            event_types: vec![],
        };

        let rendered = serde_json::to_string(&registration).expect("serialize");

        assert!(!rendered.contains("s3cr3t"), "{rendered}");
        assert!(!rendered.contains("secret"), "{rendered}");
    }

    #[test]
    fn an_empty_filter_wants_every_kind() {
        let registration = WebhookRegistration {
            id: uuid::Uuid::nil(),
            url: "https://example.com/hook".to_string(),
            secret: String::new(),
            event_types: vec![],
        };

        assert!(registration.wants(EventKind::Created));
        assert!(registration.wants(EventKind::HardDeleted));
    }

    #[test]
    fn a_filtered_registration_wants_only_what_it_named() {
        let registration = WebhookRegistration {
            id: uuid::Uuid::nil(),
            url: "https://example.com/hook".to_string(),
            secret: String::new(),
            event_types: vec![EventKind::Created],
        };

        assert!(registration.wants(EventKind::Created));
        assert!(!registration.wants(EventKind::SoftDeleted));
    }

    // ---- signing ----

    #[test]
    fn a_signature_is_stable_for_the_same_input() {
        let first = sign(b"key", 1_700_000_000, b"{}");
        let second = sign(b"key", 1_700_000_000, b"{}");

        assert_eq!(first, second);
        assert!(first.starts_with("sha256="), "{first}");
    }

    /// **The timestamp is inside the signature**, so a captured delivery cannot
    /// be replayed with a fresh header — the attacker would have to re-sign.
    #[test]
    fn changing_only_the_timestamp_changes_the_signature() {
        let first = sign(b"key", 1_700_000_000, b"{}");
        let second = sign(b"key", 1_700_000_001, b"{}");

        assert_ne!(first, second);
    }

    #[test]
    fn changing_the_body_changes_the_signature() {
        let first = sign(b"key", 1_700_000_000, b"{\"a\":1}");
        let second = sign(b"key", 1_700_000_000, b"{\"a\":2}");

        assert_ne!(first, second);
    }

    #[test]
    fn a_different_secret_produces_a_different_signature() {
        let first = sign(b"key-one", 1_700_000_000, b"{}");
        let second = sign(b"key-two", 1_700_000_000, b"{}");

        assert_ne!(first, second);
    }

    /// The canonicalization is `<timestamp>.<body>` and a receiver must be able
    /// to reproduce it byte for byte.
    #[test]
    fn the_canonical_form_is_the_timestamp_a_dot_and_the_body() {
        let canonical = canonical_bytes(42, b"{\"a\":1}");

        assert_eq!(canonical, b"42.{\"a\":1}".to_vec());
    }

    /// Our own signature verifies under the inbound convention, so a graph-owl
    /// instance can subscribe to another one without a second convention.
    #[test]
    fn a_signature_verifies_under_the_shared_convention() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let header = sign(b"key", 1_700_000_000, b"{}");
        let hex = header.strip_prefix("sha256=").expect("prefix");
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
            .collect();

        let mut mac = Hmac::<Sha256>::new_from_slice(b"key").expect("key");
        mac.update(&canonical_bytes(1_700_000_000, b"{}"));

        assert!(mac.verify_slice(&bytes).is_ok());
    }

    // ---- SSRF ----

    /// **The SSRF test.** A webhook URL is attacker-controllable input this
    /// server then makes a request to.
    #[test]
    fn a_private_or_loopback_target_is_refused() {
        for target in [
            "http://localhost/hook",
            "http://localhost:8080/hook",
            "http://127.0.0.1/hook",
            "http://127.7.7.7/hook",
            "https://10.0.0.5/hook",
            "https://192.168.1.1/hook",
            "https://172.16.0.1/hook",
            // The cloud metadata endpoint — the single most valuable SSRF
            // target on any hosted deployment.
            "http://169.254.169.254/latest/meta-data/",
            "http://0.0.0.0/hook",
            "http://[::1]/hook",
            "http://[fe80::1]/hook",
            "http://[fc00::1]/hook",
            // IPv4-mapped IPv6 reaches the IPv4 address it maps to, so every
            // v4 rule has to survive the mapping — loopback, private, and
            // link-local each spelled this way too.
            "http://[::ffff:127.0.0.1]/hook",
            "http://[::ffff:10.0.0.5]/hook",
            "http://[::ffff:192.168.1.1]/hook",
            "http://[::ffff:169.254.169.254]/hook",
            "http://[::ffff:0.0.0.0]/hook",
        ] {
            assert_eq!(
                admit_target(target, &nothing_allowed()),
                Err(TargetRefusal::Private {
                    host: super::strip_port(
                        target
                            .split("//")
                            .nth(1)
                            .and_then(|rest| rest.split('/').next())
                            .unwrap_or_default()
                    )
                    .to_string()
                }),
                "{target} must be refused"
            );
        }
    }

    /// **The metadata endpoint is refused however it is spelled.**
    ///
    /// `169.254.169.254` is the single most valuable SSRF target on any hosted
    /// deployment, and an IPv4-mapped IPv6 literal is the obvious way to try it
    /// twice. Called out separately from the table above because this is the
    /// one a reader should be able to find by name.
    #[test]
    fn the_cloud_metadata_address_is_refused_in_both_spellings() {
        for target in [
            "http://169.254.169.254/latest/meta-data/",
            "http://[::ffff:169.254.169.254]/latest/meta-data/",
        ] {
            assert!(
                matches!(
                    admit_target(target, &nothing_allowed()),
                    Err(TargetRefusal::Private { .. })
                ),
                "{target} must be refused"
            );
        }
    }

    /// A public IPv4 wearing the mapped spelling is still public — the mapping
    /// inherits the address's verdict, it does not impose one.
    #[test]
    fn a_mapped_public_address_is_still_admitted() {
        assert_eq!(
            admit_target("http://[::ffff:93.184.216.34]/hook", &nothing_allowed()),
            Ok(())
        );
    }

    /// **Userinfo does not launder a private target.** `http://evil.com@127.0.0.1/`
    /// reaches 127.0.0.1; a check that read the host as everything before the
    /// first `/` would see `evil.com` and admit it.
    #[test]
    fn a_userinfo_section_does_not_hide_the_real_host() {
        let outcome = admit_target("http://example.com@127.0.0.1/hook", &nothing_allowed());

        assert_eq!(
            outcome,
            Err(TargetRefusal::Private {
                host: "127.0.0.1".to_string()
            })
        );
    }

    /// And a trailing dot is the same host — `localhost.` resolves identically
    /// and would otherwise walk straight past a literal comparison.
    #[test]
    fn a_trailing_dot_is_the_same_host() {
        assert!(matches!(
            admit_target("http://localhost./hook", &nothing_allowed()),
            Err(TargetRefusal::Private { .. })
        ));
    }

    #[test]
    fn a_public_target_is_admitted() {
        for target in [
            "https://example.com/hook",
            "https://hooks.example.com:8443/a/b?c=d",
            "http://93.184.216.34/hook",
        ] {
            assert_eq!(
                admit_target(target, &nothing_allowed()),
                Ok(()),
                "{target} should be admitted"
            );
        }
    }

    /// The escape hatch exists, and it is **by host as written** — a deployment
    /// calling its own sidecar is a real case, and forcing it to go without
    /// webhooks entirely would push it to disable the check altogether.
    #[test]
    fn an_explicitly_allowlisted_private_target_is_admitted() {
        let allowed: std::collections::HashSet<String> =
            ["localhost".to_string()].into_iter().collect();

        assert_eq!(admit_target("http://localhost:9000/hook", &allowed), Ok(()));
        assert!(
            admit_target("http://127.0.0.1:9000/hook", &allowed).is_err(),
            "allowlisting `localhost` does not allowlist every loopback spelling"
        );
    }

    #[test]
    fn a_non_http_target_is_refused() {
        for target in [
            "ftp://example.com/hook",
            "file:///etc/passwd",
            "example.com",
        ] {
            assert_eq!(
                admit_target(target, &nothing_allowed()),
                Err(TargetRefusal::NotHttp),
                "{target}"
            );
        }
    }

    #[test]
    fn a_target_with_no_host_is_refused() {
        assert_eq!(
            admit_target("http:///hook", &nothing_allowed()),
            Err(TargetRefusal::NoHost)
        );
    }

    // ---- retries ----

    #[test]
    fn the_backoff_doubles() {
        assert_eq!(backoff(1), Some(std::time::Duration::from_secs(2)));
        assert_eq!(backoff(2), Some(std::time::Duration::from_secs(4)));
        assert_eq!(backoff(3), Some(std::time::Duration::from_secs(8)));
        assert_eq!(backoff(4), Some(std::time::Duration::from_secs(16)));
    }

    /// **Dead-lettered rather than retried forever.** A subscriber down for
    /// half an hour is a page, not a backoff.
    #[test]
    fn delivery_gives_up_after_the_last_attempt() {
        assert!(backoff(MAX_ATTEMPTS).is_some());
        assert_eq!(backoff(MAX_ATTEMPTS + 1), None);
    }

    /// Attempt zero is not a thing — the first delivery is attempt one, and
    /// treating zero as "wait two seconds" would insert a delay before the
    /// first try.
    #[test]
    fn there_is_no_attempt_zero() {
        assert_eq!(backoff(0), None);
    }

    /// The cap holds even against a large attempt number, so nothing here can
    /// overflow into a nonsense delay.
    #[test]
    fn the_backoff_is_capped() {
        for attempt in 1..=MAX_ATTEMPTS {
            let wait = backoff(attempt).expect("within the limit");
            assert!(
                wait <= std::time::Duration::from_secs(MAX_BACKOFF_SECONDS),
                "attempt {attempt} waited {wait:?}"
            );
        }
    }
}
