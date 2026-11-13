//! Text writer: write_json_validate_zone
//! Replaces a generated zone whole. Takes a request, `{"files": [...]}` per
//! `schemas/tools/zone-request.schema.json`: each text entry is validated against
//! its named schema and written in its format, each copy entry is an existing file's
//! bytes. Everything is checked before anything is written; the new zone is built
//! beside the old and swapped in; a byte-identical file is kept with its time. A
//! folder without the zone's `.zone.json` marker is refused.
//! Returns the gate contract with an approved variation: `data` is the run's
//! accounting, `{"written": int, "unchanged": int, "removed": int}`, validated
//! against `schemas/tools/zone-report.schema.json`. `error.type` is one of
//! `ZoneWriteError::error_type()`; it never raises for a refused or failed write.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use zone_write_io::write_zone_json;

/// Replace the zone at zone_root with exactly the files the request lists. Pass an absolute schema_dir.
#[pyfunction]
fn write(py: Python<'_>, zone_root: &str, request_json: &str, schema_dir: &str) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    match write_zone_json(zone_root, request_json, schema_dir) {
        Ok(report) => {
            dict.set_item("ok", true)?;
            dict.set_item("data", &report)?;
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
fn write_json_validate_zone(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(write, m)?)?;
    Ok(())
}
