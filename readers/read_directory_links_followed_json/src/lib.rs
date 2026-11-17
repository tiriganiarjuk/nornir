//! Reader: read_directory_links_followed_json
//! Lists every entry under a root directory with its symlinks followed — its path relative
//! to the root, its kind (file, directory) and a file's size — sorted by path, as JSON
//! validated against `schemas/tools/directory-listing.schema.json`. A link to a file is
//! listed as a file, with the size of its target. A link to a directory is listed as a
//! directory, and its entries are listed beneath it under paths through the link, wherever
//! the target is: the listing may reach outside the root. No entry is of kind `symlink`.
//!
//! A link that leads to nothing is a read_error naming every such link. A link that leads
//! back into a directory being listed is a validation_error naming every such link and
//! where it leads back to. Everything else is as in `read_directory_json`, its sibling
//! that lists a link without following it: the whole tree, hidden entries left out, no
//! hashes, every path that cannot be read a read_error, and a root that is itself a
//! directory.
//!
//! Who uses it, by the three lenses (`HOW_A_NORNIR_TOOL_IS_MADE.md`): a deterministic
//! Python pipeline run from a watched main session, listing a source tree whose level
//! directories are links into a shared core. It reaches names, kinds and sizes, and reads
//! no file's content. Its refusals are in standard mode.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `read` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use directory_read_io::DirectoryReadError;
use directory_read_io::{read_directory_json as list_as_json, ListOptions};

pub const USAGE: &str = "usage: read_directory_links_followed_json <root>\n\
                         Pass the one directory to list.";

/// List the tree under root with its symlinks followed: hidden entries left out, no hashes.
pub fn read(root: &str) -> Result<Option<String>, DirectoryReadError> {
    list_as_json(root, ListOptions { follow_symlinks: true, ..ListOptions::default() }).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn read(py: Python<'_>, root: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::read(root), super::DirectoryReadError::error_type)
    }

    #[pymodule]
    fn read_directory_links_followed_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
