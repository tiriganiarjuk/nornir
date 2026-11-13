//! Text writer: write_json_validate_xml
//! Takes JSON text in the `{"root": {"@attr": …, "#text": …}}` convention the XML
//! reader returns, validates it against `{schema_dir}/{schema_name}.schema.json`
//! (loaded at call time, compiled once per process), converts it to XML, then
//! writes it to `file_path` atomically. Nothing is written without a schema, and
//! nothing without its one root key.
//! Returns the gate contract: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
//! A write has no data to hand back, so `data` is always None. `error.type` is
//! one of `TextWriteError::error_type()`; it never raises for a failed
//! validation, conversion or write.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use text_writer_io::{validate_write, TextFormat};

/// Validate JSON text against the named schema, then write it as XML. Pass an absolute schema_dir.
#[pyfunction]
fn write(py: Python<'_>, json_data: &str, file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("data", py.None())?;
    match validate_write(TextFormat::Xml, json_data, file_path, schema_name, schema_dir) {
        Ok(()) => {
            dict.set_item("ok", true)?;
            dict.set_item("error", py.None())?;
        }
        Err(e) => {
            let err = PyDict::new_bound(py);
            err.set_item("type", e.error_type())?;
            err.set_item("message", e.to_string())?;
            dict.set_item("ok", false)?;
            dict.set_item("error", err)?;
        }
    }
    Ok(dict.into())
}

#[pymodule]
fn write_json_validate_xml(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(write, m)?)?;
    Ok(())
}
