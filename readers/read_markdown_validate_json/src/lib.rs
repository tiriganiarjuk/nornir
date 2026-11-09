//! Reader: read_markdown_validate_json
//! Reads a Markdown file with optional YAML frontmatter, validates the envelope
//! `{"frontmatter": {...}, "body": "..."}` against `{schema_dir}/{schema_name}.schema.json`
//! (loaded at call time, compiled once per process), returns the validated JSON string.
//! Raises ValueError on any failure (read, parse, schema load, validation).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use reader_io::{read_validate_json, SourceFormat};

/// Read a Markdown file, validate its {frontmatter, body} envelope against the named schema, return JSON. Pass an absolute schema_dir.
#[pyfunction]
fn read(file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<String> {
    read_validate_json(SourceFormat::Markdown, file_path, schema_name, schema_dir)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
fn read_markdown_validate_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
