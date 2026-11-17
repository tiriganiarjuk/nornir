//! Writes schemas/tools/failure-class.schema.json from the error registry.
//!
//!     cargo run -p failure_mode_core --example write_failure_class_schema
//!
//! The schema's text comes from failure_modes.toml through the build script; this writes
//! it where callers outside Rust read it. `cargo test -p failure_mode_core` fails while the
//! file is behind the registry.

use std::path::Path;

fn main() -> Result<(), String> {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/tools/failure-class.schema.json");
    let shown = schema_path.display();
    let written = std::fs::read_to_string(&schema_path).unwrap_or_default();
    if written == failure_mode_core::FAILURE_CLASS_SCHEMA {
        println!("{shown}: the failure-class schema already matches the registry");
        return Ok(());
    }
    std::fs::write(&schema_path, failure_mode_core::FAILURE_CLASS_SCHEMA)
        .map_err(|io_err| format!("{shown}: {io_err}"))?;
    println!("{shown}: the failure-class schema was written from the registry");
    Ok(())
}
