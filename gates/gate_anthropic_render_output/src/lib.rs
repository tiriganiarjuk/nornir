//! Gate: gate_anthropic_render_output
//! Output gate: validates JSON against anthropic-render schema, writes TOML to disk.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use gate_io::validate_and_write;
use schemas_embedded::ANTHROPIC_RENDER;

#[pyfunction]
fn validate(py: Python<'_>, data: &str, path: &str) -> PyResult<PyObject> {
    match validate_and_write(&ANTHROPIC_RENDER, data, path, true) {
        Ok(()) => {
            let dict = PyDict::new_bound(py);
            dict.set_item("ok", true)?;
            dict.set_item("data", py.None())?;
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
fn is_valid(_py: Python<'_>, data: &str) -> PyResult<bool> {
    Ok(ANTHROPIC_RENDER.is_valid(data).unwrap_or(false))
}

#[pyfunction]
fn schema_name() -> &'static str {
    ANTHROPIC_RENDER.schema_name()
}

#[pymodule]
fn gate_anthropic_render_output(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(schema_name, m)?)?;
    Ok(())
}
