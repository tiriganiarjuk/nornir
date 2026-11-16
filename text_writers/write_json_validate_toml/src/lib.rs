//! Text writer: write_json_validate_toml
//! Takes JSON text, validates it against `{schema_dir}/{schema_name}.schema.json`
//! (loaded at call time, compiled once per process), converts it to TOML in the
//! given key order, then writes it to `file_path` atomically. Nothing is written
//! without a schema, and nothing TOML cannot express (a null) is written.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `write` is the tool;
//! the CLI face (`main.rs`, the JSON on stdin) and the PyO3 face (below, feature
//! "python") hand its result to `tool_face_io`. A write hands no data back.

pub use text_writer_io::TextWriteError;

pub const USAGE: &str = "usage: write_json_validate_toml <file_path> <schema_name> <schema_dir>\n\
                         Pipe the JSON to write on stdin; schema_dir is an absolute path.";

/// Validate JSON text against the named schema, then write it as TOML. Pass an absolute schema_dir.
pub fn write(json_data: &str, file_path: &str, schema_name: &str, schema_dir: &str) -> Result<Option<String>, TextWriteError> {
    text_writer_io::validate_write(text_writer_io::TextFormat::Toml, json_data, file_path, schema_name, schema_dir).map(|()| None)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn write(py: Python<'_>, json_data: &str, file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::write(json_data, file_path, schema_name, schema_dir), super::TextWriteError::error_type)
    }

    #[pymodule]
    fn write_json_validate_toml(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(write, m)?)?;
        Ok(())
    }
}
