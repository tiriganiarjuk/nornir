//! Reader: read_tomlx_json
//! Reads a `.tomlx` file (TOML with semantic annotations), applies its unit, path and
//! type annotations, and converts the data to JSON. A tomlx file carries its own
//! schema, so no schema parameter is taken. Environment references resolve live;
//! `base=config` references resolve against the file's directory.
//! Returns the gate contract: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
//! `error.type` is one of `ReaderError::error_type()`; it never raises for a failed read.

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Read a .tomlx file; its converted data is the result's `data`.
#[pyfunction]
fn read(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    match reader_io::read_tomlx_json(file_path) {
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
fn read_tomlx_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
