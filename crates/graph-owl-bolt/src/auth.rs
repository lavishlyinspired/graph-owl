//! The authentication port `HELLO` calls through — Epic 12, reused rather
//! than a parallel credential path.
//!
//! **A divergent identity path is the one nobody audits** (`07d-engine-bolt.md`
//! decision 4). This crate never verifies a credential itself; it hands the
//! scheme this connection's `HELLO` carried to whatever [`Authenticator`] the
//! composition root wired up, which is expected to be the identical
//! verification logic the HTTP surface calls — see
//! `crates/graph-owl-server`'s adapter.

use graph_owl_core::Principal;

/// What `HELLO`'s `extra` map carried for authentication, at the protocol
/// version this server speaks (< 5.1, so credentials arrive on `HELLO`
/// itself rather than a separate `LOGON`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// `"none"`, `"basic"`, or `"bearer"` — this server only ever resolves
    /// `"bearer"` to something, since the catalog has no password store.
    pub scheme: String,
    /// The `basic` scheme's username. Unused by `bearer`.
    pub principal: Option<String>,
    /// The `basic` scheme's password, or the `bearer` scheme's token.
    pub credentials: Option<String>,
}

/// Why authentication was refused. Carries a message rather than structured
/// detail because the only consumer is `HELLO`'s `FAILURE` response, which
/// is a string field on the wire regardless.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct AuthError(pub String);

impl AuthError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Resolves `HELLO`'s credentials to a [`Principal`] — the one and only
/// place a Bolt connection's identity is decided, mirroring
/// `crates/graph-owl-server`'s `Auth` extractor's own doc comment for the
/// HTTP side of the same rule.
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    async fn authenticate(&self, credentials: &Credentials) -> Result<Principal, AuthError>;
}
