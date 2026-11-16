//! Gate: gate_paths_verified_input
//! Input gate: reads TOML from disk, validates it against the paths-resolved schema
//! embedded in the binary, verifies that every path it names exists, and returns the validated JSON.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `validate` is the
//! tool; the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its
//! result to `tool_face_io`.

pub use gate_io::GateError;
use schemas_embedded::PATHS_RESOLVED;

pub const USAGE: &str = "usage: gate_paths_verified_input <file_path>\n\
                         Pass the path of the paths-resolved TOML file to validate.";

/// Read the TOML file at path and validate it against the embedded paths-resolved schema.
pub fn validate(path: &str) -> Result<Option<String>, GateError> {
    gate_io::read_and_validate(&PATHS_RESOLVED, path, true).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn validate(py: Python<'_>, path: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::validate(path), super::GateError::error_type)
    }

    #[pyfunction]
    fn is_valid(_py: Python<'_>, path: &str) -> PyResult<bool> {
        Ok(super::validate(path).is_ok())
    }

    #[pyfunction]
    fn schema_name() -> &'static str {
        super::PATHS_RESOLVED.schema_name()
    }

    #[pymodule]
    fn gate_paths_verified_input(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(validate, m)?)?;
        m.add_function(wrap_pyfunction!(is_valid, m)?)?;
        m.add_function(wrap_pyfunction!(schema_name, m)?)?;
        Ok(())
    }
}
