//! Bolt wire-protocol server: `PackStream` codec, handshake, connection state machine.
//!
//! See `plans/07d-engine-bolt.md` for the epic this implements and its slices.
//!
//! Feature-gated and off by default: it opens a second listening port. Speaking Bolt makes
//! every property-graph driver and visualization tool a client without writing an adapter
//! for each.

pub mod auth;
pub mod chunking;
pub mod handshake;
pub mod limits;
pub mod messages;
pub mod packstream;
pub mod query;
pub mod server;
pub mod state;

pub use auth::{AuthError, Authenticator, Credentials};
pub use limits::BoltLimits;
pub use query::{BoltRow, QueryEngine, QueryError, RecordReceiver, RecordValue, RunOutcome};
pub use server::BoltServer;
