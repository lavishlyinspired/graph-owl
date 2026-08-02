//! metadata-as-code apply/plan/drift, admin, DevOps tooling
//!
//! **Status**: placeholder. Implemented by Epic 20 — see `plans/`.
//!
//! No production code lands here except through the TDD cycle
//! (RED -> GREEN -> MUTATE -> KILL MUTANTS -> REFACTOR) defined in `CLAUDE.md`.

pub mod apply;
pub mod declaration;
pub mod drift;
pub mod exit;
pub mod export;
pub mod plan;
pub mod prune;
pub mod validate;
