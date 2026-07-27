//! Tells cargo to rebuild when a migration is added or changed.
//!
//! `refinery::embed_migrations!` reads the directory at macro-expansion time
//! and cargo has no way to know that happened. Without this a new migration is
//! silently not compiled in: the crate builds, the server starts, and the
//! first query against the new table fails at runtime.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
