//! Reader: read_toml_validate_json
//! Reads a TOML file, validates it against `{schema_dir}/{schema_name}.schema.json`
//! (loaded at call time, compiled once per process), and returns the validated JSON.
//!
//! One tool, two faces. `read` is the tool; the CLI face (`main.rs`) and the
//! PyO3 face (below, feature "python") call it and nothing else.

pub use reader_io::ReaderError;
use reader_io::{read_validate_json, SourceFormat};

/// Read a TOML file and validate it against the named schema. Pass an absolute schema_dir.
pub fn read(file_path: &str, schema_name: &str, schema_dir: &str) -> Result<String, ReaderError> {
    read_validate_json(SourceFormat::Toml, file_path, schema_name, schema_dir)
}

/// PyO3 face. Returns the gate contract:
/// `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`;
/// `error.type` is one of `ReaderError::error_type()`. It never raises for a failed read.
#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    #[pyfunction]
    fn read(py: Python<'_>, file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<PyObject> {
        let dict = PyDict::new_bound(py);
        match super::read(file_path, schema_name, schema_dir) {
            Ok(json) => {
                dict.set_item("ok", true)?;
                dict.set_item("data", &json)?;
                dict.set_item("error", py.None())?;
            }
            Err(e) => {
                let err = PyDict::new_bound(py);
                err.set_item("type", e.error_type())?;
                err.set_item("message", e.to_string())?;
                dict.set_item("ok", false)?;
                dict.set_item("data", py.None())?;
                dict.set_item("error", err)?;
            }
        }
        Ok(dict.into())
    }

    #[pymodule]
    fn read_toml_validate_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
