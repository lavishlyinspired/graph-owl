//! Tells cargo to rebuild when the console assets change.
//!
//! `rust-embed`'s derive reads the directory at macro-expansion time, and cargo
//! has no way to know that happened — so without this, a rebuilt frontend is
//! silently ignored and the binary keeps serving whatever was embedded first.
//! That failure is invisible: the server starts, returns 200, and serves a
//! stale bundle.
//!
//! Plan 122a A11: `graphowl-app/dist` is now the only embed — `ui/dist` was
//! watched here through A0–A10's dual-embed migration and removed once
//! `ui/` moved to `_archived/ui/`.
fn main() {
    println!("cargo:rerun-if-changed=../../graphowl-app/dist");
    println!("cargo:rerun-if-changed=../../graphowl-app/dist/index.html");
}
