//! Presence tool: create_flag_generate_lock
//! Creates the empty `GENERATE.lock` in a directory — the flag that releases a directory to
//! a generator. A flag already there is nothing to do. A `GENERATE.lock` that is not an empty
//! file is refused, and so is a directory that does not exist: the tool creates no folders.
//! Its effect is the file, so it hands back no data.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `create` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use presence_io::PresenceError;

/// The flag this tool handles, and no other.
pub const FLAG: &str = "GENERATE.lock";

pub const USAGE: &str = "usage: create_flag_generate_lock <directory>\n\
                         Pass the one directory to release.";

/// Create the empty GENERATE.lock in directory.
pub fn create(directory: &str) -> Result<Option<String>, PresenceError> {
    presence_io::create_flag(directory, FLAG).map(|()| None)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn create(py: Python<'_>, directory: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::create(directory), super::PresenceError::error_type)
    }

    #[pymodule]
    fn create_flag_generate_lock(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(create, m)?)?;
        Ok(())
    }
}
