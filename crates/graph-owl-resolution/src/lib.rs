//! Entity resolution, coreference, temporal resolution
//!
//! **Status**: in progress. Epic 17 — see `plans/17-entity-resolution.md`.
//!
//! No production code lands here except through the TDD cycle
//! (RED -> GREEN -> MUTATE -> KILL MUTANTS -> REFACTOR) defined in `CLAUDE.md`.

pub mod bands;
pub mod mention;
pub mod normalize;
pub mod rule_match;
pub mod score;
pub mod temporal;
