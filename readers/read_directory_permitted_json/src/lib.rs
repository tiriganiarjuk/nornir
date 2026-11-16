//! Reader: read_directory_permitted_json
//! Lists every entry under a root directory that the caller is permitted to see — its path
//! relative to the root, its kind (file, directory, symlink) and a file's size — sorted by
//! path, as JSON validated against `schemas/tools/directory-permitted-listing.schema.json`.
//! A directory the caller may not look into is listed with `"permitted": false` and nothing
//! beneath it; a path named and then gone while the walk ran is left out and named in
//! `"vanished"`, which is empty when nothing vanished. Every other failure — a root that
//! cannot be listed, a disk fault, a name that is not UTF-8 — is a read_error, as in
//! `read_directory_json`, its strict sibling. It lists the whole tree, leaves hidden
//! entries out and computes no hashes. A symlink is listed and never followed.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `read` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use directory_read_io::DirectoryReadError;
use directory_read_io::{read_directory_json as list_as_json, ListOptions};

pub const USAGE: &str = "usage: read_directory_permitted_json <root>\n\
                         Pass the one directory to list.";

/// List the tree under root as far as the caller is permitted to see it: hidden entries left out, no hashes.
pub fn read(root: &str) -> Result<Option<String>, DirectoryReadError> {
    list_as_json(root, ListOptions { permitted_only: true, ..ListOptions::default() }).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn read(py: Python<'_>, root: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::read(root), super::DirectoryReadError::error_type)
    }

    #[pymodule]
    fn read_directory_permitted_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
