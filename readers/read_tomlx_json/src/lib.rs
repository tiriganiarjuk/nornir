//! Reader: read_tomlx_json
//! Reads a `.tomlx` file (TOML with semantic annotations), applies its unit, path and
//! type annotations, returns the converted data as a JSON string. A tomlx file carries
//! its own schema, so no schema parameter is taken. Environment references resolve
//! live; `base=config` references resolve against the file's directory.
//! Raises ValueError on any failure (read, parse, annotation validation).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Read a .tomlx file and return its converted data as JSON.
#[pyfunction]
fn read(file_path: &str) -> PyResult<String> {
    reader_io::read_tomlx_json(file_path).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
fn read_tomlx_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
