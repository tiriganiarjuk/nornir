//! Reader: read_tomlx_json_schema
//! Reads a `.tomlx` file: its converted data and the JSON Schema derived from the
//! file's type annotations, as one JSON object `{"data": …, "schema": …}`. Fails with
//! `schema_error` if the file has no annotations to derive a schema from. Environment
//! references resolve live; `base=config` references resolve against the file's directory.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `read` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use reader_io::ReaderError;

pub const USAGE: &str = "usage: read_tomlx_json_schema <file_path>\n\
                         Pass the path of the .tomlx file.";

/// Read a .tomlx file; the result is `{"data": …, "schema": …}` as JSON.
pub fn read(file_path: &str) -> Result<Option<String>, ReaderError> {
    reader_io::read_tomlx_json_schema(file_path).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn read(py: Python<'_>, file_path: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::read(file_path), super::ReaderError::error_type)
    }

    #[pymodule]
    fn read_tomlx_json_schema(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
