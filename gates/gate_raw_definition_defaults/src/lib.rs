//! Gate: gate_raw_definition_defaults
//! Passthrough gate: reads raw definition TOML, applies conditional defaults,
//! validates against raw-definition schema, writes clean TOML.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use default_apply_io::read_apply_validate_write;
use schemas_embedded::RAW_DEFINITION;

#[pyfunction]
fn validate(py: Python<'_>, input_path: &str, output_path: &str) -> PyResult<PyObject> {
    match read_apply_validate_write(&RAW_DEFINITION, input_path, output_path) {
        Ok(json) => {
            let dict = PyDict::new_bound(py);
            dict.set_item("ok", true)?;
            dict.set_item("data", &json)?;
            dict.set_item("error", py.None())?;
            Ok(dict.into())
        }
        Err(e) => {
            let dict = PyDict::new_bound(py);
            dict.set_item("ok", false)?;
            dict.set_item("data", py.None())?;
            let err = PyDict::new_bound(py);
            err.set_item("type", e.error_type())?;
            err.set_item("message", e.to_string())?;
            dict.set_item("error", err)?;
            Ok(dict.into())
        }
    }
}

#[pyfunction]
fn is_valid(_py: Python<'_>, input_path: &str) -> PyResult<bool> {
    let dir = std::env::temp_dir().join("nornir_defaults_check");
    let output = dir.join("check_output.toml");
    let output_str = output.to_str().ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err("temp path contains invalid UTF-8")
    })?;
    Ok(read_apply_validate_write(&RAW_DEFINITION, input_path, output_str).is_ok())
}

#[pyfunction]
fn schema_name() -> &'static str {
    RAW_DEFINITION.schema_name()
}

#[pymodule]
fn gate_raw_definition_defaults(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(schema_name, m)?)?;
    Ok(())
}
