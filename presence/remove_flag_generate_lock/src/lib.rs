//! Presence tool: remove_flag_generate_lock
//! Removes the empty `GENERATE.lock` from a directory, taking the directory back from the
//! generator. A flag already gone is nothing to do. A `GENERATE.lock` that is not an empty
//! file is refused and left exactly as it is. Its effect is the removal, so it hands back no
//! data.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `remove` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use presence_io::PresenceError;

/// The flag this tool handles, and no other.
pub const FLAG: &str = "GENERATE.lock";

pub const USAGE: &str = "usage: remove_flag_generate_lock <directory>\n\
                         Pass the one directory to take back from the generator.";

/// Remove the empty GENERATE.lock from directory.
pub fn remove(directory: &str) -> Result<Option<String>, PresenceError> {
    presence_io::remove_flag(directory, FLAG).map(|()| None)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn remove(py: Python<'_>, directory: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::remove(directory), super::PresenceError::error_type)
    }

    #[pymodule]
    fn remove_flag_generate_lock(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(remove, m)?)?;
        Ok(())
    }
}
