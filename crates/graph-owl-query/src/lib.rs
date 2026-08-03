//! SPARQL over the flake store.
//!
//! **Most of SPARQL is adopted, not built** (`plans/00l-build-vs-adopt.md`):
//! `spargebra` parses, `sparopt` optimises, `spareval` evaluates, `sparesults`
//! serialises — all permissively licensed.
//!
//! What lives here is the one thing no library could supply: a
//! [`spareval::QueryableDataset`] over flakes. That is where index selection,
//! `as_of` resolution and the compiled access predicate live, and it is why
//! adopting an evaluator costs none of this project's differentiators — the
//! evaluator only ever sees rows the scan already permitted, at the one
//! transaction time the scan already resolved.

pub mod cypher;
pub mod dataset;
pub mod pushdown;
pub mod term;
