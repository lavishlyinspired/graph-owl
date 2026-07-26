//! Graph traversal: neighbours, shortest path, all paths, cycle detection, subgraph extraction.
//!
//! **Status**: placeholder. Implemented by Epic 7a — see `plans/07a-engine-traversal.md`.
//!
//! Separate from `graph-owl-query` because these are graph algorithms, not query-language
//! features: `shortest_path`, `all_paths`, `detect_cycles`, and `subgraph` cannot be
//! expressed as SPARQL property paths, and repeated BGP evaluation for multi-hop
//! traversal degrades to O(n^2).
//!
//! No production code lands here except through the TDD cycle defined in `CLAUDE.md`.
