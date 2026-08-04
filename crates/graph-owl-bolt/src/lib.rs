//! Bolt wire-protocol server: `PackStream` codec, handshake, connection state machine.
//!
//! **Status**: partial. Slice A (`PackStream` codec) is implemented; the rest of Epic 7d —
//! see `plans/07d-engine-bolt.md` — is not.
//!
//! Feature-gated and off by default: it opens a second listening port. Speaking Bolt makes
//! every property-graph driver and visualization tool a client without writing an adapter
//! for each.

pub mod packstream;
