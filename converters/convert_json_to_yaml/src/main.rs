//! JSON-to-YAML converter. Reads JSON from stdin, writes YAML — nulls kept
//! (YAML represents them; a caller that wants them gone strips before piping).
//!
//! Usage:
//!     echo '{"key": "value"}' | convert_json_to_yaml output.yaml
//!     echo '{"key": "value"}' | convert_json_to_yaml              # stdout

use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

// =============================================================================
// Core logic
// =============================================================================

fn run(args: &[String]) -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("reading stdin: {e}"))?;
    let input = input.trim();

    if input.is_empty() {
        return Err("stdin is empty — pipe JSON into this command".to_string());
    }

    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| format!("invalid JSON — {e}"))?;

    let yaml_str = format_core::serialize::to_yaml(&value)
        .map_err(|e| format!("YAML conversion failed — {e}"))?;

    match args.first() {
        Some(path) => write_engine::write_file_atomic(Path::new(path), &yaml_str)?,
        None => {
            io::stdout()
                .write_all(yaml_str.as_bytes())
                .map_err(|e| format!("write stdout: {e}"))?;
        }
    }

    Ok(())
}

// =============================================================================
// Entry point
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match run(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
