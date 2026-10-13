//! Reader: read_tomlx_json_schema
//! Reads a `.tomlx` file and returns `(data_json, schema_json)`: the converted data
//! and the JSON Schema derived from the file's type annotations. Errors if the file
//! has no annotations to derive a schema from. Environment references resolve live;
//! `base=config` references resolve against the file's directory.
//! Raises ValueError on any failure (read, parse, annotation validation, no schema).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Read a .tomlx file and return (data_json, schema_json).
#[pyfunction]
fn read(file_path: &str) -> PyResult<(String, String)> {
    reader_io::read_tomlx_json_schema(file_path).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pymodule]
fn read_tomlx_json_schema(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
