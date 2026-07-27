//! Tells cargo to rebuild when a migration is added or changed.
//!
//! `refinery::embed_migrations!` reads the directory at macro-expansion time,
//! and cargo has no way to know that happened. Without this, a new migration is
//! silently not compiled in: the crate builds, the server starts, and every
//! query against the new column fails at runtime with ColumnNotFound.
//!
//! Same class of bug as `rust-embed` and the console assets, and it presents
//! the same way — everything compiles and the failure is at runtime.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
