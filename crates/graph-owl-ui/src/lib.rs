//! Embedded web console: serves the built single-page application from the server binary.
//!
//! **Status**: placeholder. Implemented by Epics 39-41 — see `plans/00f-ui-architecture.md`.
//!
//! Feature-gated so a headless deployment compiles the assets out and keeps the binary
//! budget. Frontend sources live in `ui/`, not here; this crate only embeds and serves the
//! build output.
