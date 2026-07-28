//! Prints the OpenAPI document, so the committed copy is produced by the same
//! code that serves it rather than by a second implementation.
//!
//!     cargo run -p graph-owl-server --bin openapi > openapi.json
//!
//! Pretty-printed deliberately: the committed file exists to be *read in a
//! diff*, and a single-line document renders every change as one modified line.
fn main() {
    let document = graph_owl_server::openapi::document();
    println!(
        "{}",
        serde_json::to_string_pretty(&document).expect("the document serializes")
    );
}
