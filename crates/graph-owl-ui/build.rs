//! Tells cargo to rebuild when the console assets change.
//!
//! `rust-embed`'s derive reads the directory at macro-expansion time, and cargo
//! has no way to know that happened — so without this, a rebuilt frontend is
//! silently ignored and the binary keeps serving whatever was embedded first.
//! That failure is invisible: the server starts, returns 200, and serves a
//! stale bundle.
fn main() {
    println!("cargo:rerun-if-changed=../../ui/dist");
    println!("cargo:rerun-if-changed=../../ui/dist/index.html");
}
