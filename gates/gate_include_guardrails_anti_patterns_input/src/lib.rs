//! Gate: gate_include_guardrails_anti_patterns_input
//! Input gate: reads TOML from disk, validates against include-guardrails-anti-patterns schema, returns JSON.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use gate_io::read_and_validate;
use schemas_embedded::INCLUDE_GUARDRAILS_ANTI_PATTERNS;

#[pyfunction]
fn validate(py: Python<'_>, path: &str) -> PyResult<PyObject> {
    match read_and_validate(&INCLUDE_GUARDRAILS_ANTI_PATTERNS, path, false) {
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
fn is_valid(_py: Python<'_>, path: &str) -> PyResult<bool> {
    Ok(read_and_validate(&INCLUDE_GUARDRAILS_ANTI_PATTERNS, path, false).is_ok())
}

#[pyfunction]
fn schema_name() -> &'static str {
    INCLUDE_GUARDRAILS_ANTI_PATTERNS.schema_name()
}

#[pymodule]
fn gate_include_guardrails_anti_patterns_input(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(schema_name, m)?)?;
    Ok(())
}
