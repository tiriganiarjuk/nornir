//! Reader: read_tomlx_json
//! Reads a `.tomlx` file (TOML with semantic annotations), applies its unit, path and
//! type annotations, and converts the data to JSON. It returns the converted data;
//! `read_tomlx_validate_json` also validates it against a named schema. Environment references
//! resolve live; `base=config` references resolve against the file's directory.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `read` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use reader_io::ReaderError;

pub const USAGE: &str = "usage: read_tomlx_json <file_path>\n\
                         Pass the path of the .tomlx file.";

/// Read a .tomlx file; the result is its converted data.
pub fn read(file_path: &str) -> Result<Option<String>, ReaderError> {
    reader_io::read_tomlx_json(file_path).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn read(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::read(file_path), super::ReaderError::error_type)
    }

    #[pymodule]
    fn read_tomlx_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
