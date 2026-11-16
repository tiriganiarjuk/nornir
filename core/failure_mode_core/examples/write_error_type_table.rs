//! Rewrites the `error.type` table of MUST_READ_BEFORE_BUILDING.md from the error registry.
//!
//!     cargo run -p failure_mode_core --example write_error_type_table
//!
//! The table's text comes from failure_modes.toml through the build script; this
//! places it in the document. `cargo test -p failure_mode_core` fails while the
//! document is behind the registry.

use std::path::Path;

fn main() -> Result<(), String> {
    let document_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../MUST_READ_BEFORE_BUILDING.md");
    let shown = document_path.display();
    let document = std::fs::read_to_string(&document_path).map_err(|io_err| format!("{shown}: {io_err}"))?;
    let current = failure_mode_core::with_error_type_table(&document).map_err(|reason| format!("{shown}: {reason}"))?;
    if current == document {
        println!("{shown}: the error-type table already matches the registry");
        return Ok(());
    }
    std::fs::write(&document_path, current).map_err(|io_err| format!("{shown}: {io_err}"))?;
    println!("{shown}: the error-type table was rewritten from the registry");
    Ok(())
}
