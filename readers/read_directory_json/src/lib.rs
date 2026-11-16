//! Reader: read_directory_json
//! Lists every entry under a root directory — its path relative to the root, its kind
//! (file, directory, symlink) and a file's size — sorted by path, as JSON validated against
//! `schemas/tools/directory-listing.schema.json`. It lists the whole tree, leaves hidden
//! entries out and computes no hashes; the engine, `directory_read_io`, carries those
//! choices, and a tool that makes another is a tool of its own. Nothing it could not read
//! is skipped: every unreadable path is named in one read_error. A symlink is listed and
//! never followed. `read_directory_permitted_json` is its sibling: the tree as far as the
//! caller is permitted to see it.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `read` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use directory_read_io::DirectoryReadError;
use directory_read_io::{read_directory_json as list_as_json, ListOptions};

pub const USAGE: &str = "usage: read_directory_json <root>\n\
                         Pass the one directory to list.";

/// List the whole tree under root: hidden entries left out, no hashes.
pub fn read(root: &str) -> Result<Option<String>, DirectoryReadError> {
    list_as_json(root, ListOptions::default()).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn read(py: Python<'_>, root: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::read(root), super::DirectoryReadError::error_type)
    }

    #[pymodule]
    fn read_directory_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
