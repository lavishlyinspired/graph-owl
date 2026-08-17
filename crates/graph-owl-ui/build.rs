//! Tells cargo to rebuild when the console assets change.
//!
//! `rust-embed`'s derive reads the directory at macro-expansion time, and cargo
//! has no way to know that happened — so without this, a rebuilt frontend is
//! silently ignored and the binary keeps serving whatever was embedded first.
//! That failure is invisible: the server starts, returns 200, and serves a
//! stale bundle.
//!
//! Plan 122a A0: watches `graphowl-app/dist` too, for the temporary `/next/`
//! embed. Both entries are removed at A11 (`ui/`) leaving only
//! `graphowl-app/dist` once the rebuild replaces `ui/` as the sole console.
fn main() {
    println!("cargo:rerun-if-changed=../../ui/dist");
    println!("cargo:rerun-if-changed=../../ui/dist/index.html");
    println!("cargo:rerun-if-changed=../../graphowl-app/dist");
    println!("cargo:rerun-if-changed=../../graphowl-app/dist/index.html");
}
