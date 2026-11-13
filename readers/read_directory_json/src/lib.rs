//! Reader: read_directory_json
//! Lists every entry under a root directory — its path relative to the root, its
//! kind (file, directory, symlink), a file's size and, when asked, its sha256 —
//! sorted by path, as JSON validated against `schemas/tools/directory-listing.schema.json`.
//! A directory carries its own shape, so no schema parameter is taken.
//! Nothing it could not read is skipped: every unreadable path is named in one
//! read_error. A symlink is listed and never followed. The walk applies no skip
//! list; what to keep is the caller's to decide.
//! Returns the gate contract: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
//! `error.type` is one of `DirectoryReadError::error_type()`; it never raises for a failed read.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use directory_read_io::{read_directory_json as list_as_json, ListOptions};

/// List the tree under root. max_depth None lists it whole; 1 lists the root's own entries.
#[pyfunction]
#[pyo3(signature = (root, max_depth=None, include_hidden=false, with_hashes=false))]
fn read(py: Python<'_>, root: &str, max_depth: Option<u32>, include_hidden: bool, with_hashes: bool) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    let options = ListOptions { max_depth, include_hidden, with_hashes };
    match list_as_json(root, options) {
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
fn read_directory_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
