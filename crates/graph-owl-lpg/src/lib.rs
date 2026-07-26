//! Labelled property graph model and the bidirectional flake <-> LPG mapping.
//!
//! **Status**: placeholder. Implemented by Epic 7c — see `plans/07c-engine-lpg.md`.
//!
//! Pure: node labels, edge types, property maps, and the mapping to and from the flake
//! model. No I/O. Reified relationships already carry edge properties, which is the
//! defining LPG feature — this crate makes that projection explicit and lossless.
