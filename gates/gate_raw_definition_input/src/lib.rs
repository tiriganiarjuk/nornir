//! Gate: gate_raw_definition_input
//! Input gate: reads TOML from disk, validates it against the raw-definition schema
//! embedded in the binary, and returns the validated JSON.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `validate` is the
//! tool; the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its
//! result to `tool_face_io`.

pub use gate_io::GateError;
use schemas_embedded::RAW_DEFINITION;

pub const USAGE: &str = "usage: gate_raw_definition_input <file_path>\n\
                         Pass the path of the raw-definition TOML file to validate.";

/// Read the TOML file at path and validate it against the embedded raw-definition schema.
pub fn validate(path: &str) -> Result<Option<String>, GateError> {
    gate_io::read_and_validate(&RAW_DEFINITION, path, false).map(Some)
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
        super::RAW_DEFINITION.schema_name()
    }

    #[pymodule]
    fn gate_raw_definition_input(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(validate, m)?)?;
        m.add_function(wrap_pyfunction!(is_valid, m)?)?;
        m.add_function(wrap_pyfunction!(schema_name, m)?)?;
        Ok(())
    }
}
