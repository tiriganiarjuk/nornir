//! Reader: read_xml_validate_json
//! Reads an XML file, converts it to JSON using the `{"root": {"@attr": …, "#text": …}}`
//! convention, validates against `{schema_dir}/{schema_name}.schema.json` (loaded at
//! call time, compiled once per process).
//! Returns the gate contract: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
//! `error.type` is one of `ReaderError::error_type()`; it never raises for a failed read.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use reader_io::{read_validate_json, SourceFormat};

/// Read an XML file, validate against the named schema. Pass an absolute schema_dir.
#[pyfunction]
fn read(py: Python<'_>, file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    match read_validate_json(SourceFormat::Xml, file_path, schema_name, schema_dir) {
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
fn read_xml_validate_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
