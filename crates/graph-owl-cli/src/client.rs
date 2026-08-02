//! The HTTP client — Epic 20, the piece Slices A–G deliberately left out.
//!
//! **Decision 6: a thin client over the HTTP API, never the database.** The
//! same authorization and validation apply as for any other caller, and the
//! tool works against a remote instance — which it could not if it reached
//! for `graph-owl-api` and a connection string.
//!
//! Kept behind a trait so the planning and apply logic stays testable without
//! a server. That is not indirection for its own sake: Slices A–G are pure
//! functions over `Declarations` and `Vec<LiveEntity>` precisely so their 32
//! tests need no infrastructure, and a concrete `reqwest` call in the middle
//! of `apply` would have taken that away from all of them.

use crate::plan::LiveEntity;

#[derive(Debug)]
pub enum ClientError {
    /// The catalog could not be reached, or answered in a way this client
    /// cannot use.
    Transport(String),
    /// The catalog refused the request — a validation failure, a containment
    /// violation, a permission problem. Carries the server's own message,
    /// because the server is the one that knows why.
    Refused { status: u16, detail: String },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Transport(detail) => write!(f, "could not reach the catalog: {detail}"),
            ClientError::Refused { status, detail } => {
                write!(f, "the catalog refused this ({status}): {detail}")
            }
        }
    }
}

impl std::error::Error for ClientError {}

/// What the CLI needs a catalog to do.
///
/// Three operations, not a mirror of the API surface: `plan` needs to read
/// the scope, `apply` needs to create and update, `prune` needs to delete.
/// Anything else the CLI might want belongs to the API or the console — the
/// scope boundary in `20-metadata-as-code.md` exists to stop this trait from
/// growing into the 40-subcommand surface it warns about.
pub trait Catalog {
    /// Everything live within `scope_prefixes`.
    ///
    /// **Scoped at the source, not filtered afterwards.** A client that
    /// fetched the whole catalog and filtered locally would produce a plan
    /// proposing to prune everything outside the scope the moment the filter
    /// had a bug — and decision 2 exists because that failure tombstones a
    /// catalog.
    ///
    /// # Errors
    ///
    /// [`ClientError`] if the catalog cannot be reached or refuses the read.
    fn live_within(&self, scope_prefixes: &[String]) -> Result<Vec<LiveEntity>, ClientError>;

    /// Creates or updates one entity. Idempotent by FQN, reusing the
    /// catalog's own upsert rather than a create-then-update dance that
    /// would race.
    ///
    /// # Errors
    ///
    /// [`ClientError`] if the catalog cannot be reached or refuses the write.
    fn upsert(&self, entity: &UpsertRequest) -> Result<(), ClientError>;

    /// Soft-deletes one entity — a tombstone, never a hard delete, so a
    /// mistaken prune is recoverable.
    ///
    /// # Errors
    ///
    /// [`ClientError`] if the catalog cannot be reached or refuses it.
    fn tombstone(&self, fully_qualified_name: &str) -> Result<(), ClientError>;
}

/// One entity as the CLI sends it.
///
/// `description: None` means **"not declared"** and the field is omitted from
/// the request entirely — it is not sent as null. Decision 4 lives or dies
/// here: a request that sent null for every undeclared field would reset
/// every hand-curated description on the first apply, and no amount of plan
/// review upstream would catch it because the plan never showed a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertRequest {
    pub kind: String,
    pub name: String,
    pub parent_fqn: Option<String>,
    pub description: Option<String>,
}
