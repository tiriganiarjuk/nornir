//! Binary writer: write_bytes_verify_quantized_mesh
//! Takes the uncompressed bytes of one quantized-mesh-1.0 terrain tile,
//! verifies they are a whole tile carrying nothing else, then writes them to
//! `file_path` atomically — gzip-compressed when `gzip_compress` is true.
//! Refused bytes never reach the disk.
//! Returns the gate contract: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
//! A write has no data to hand back, so `data` is always None. `error.type` is
//! one of `BinaryWriteError::error_type()`; it never raises for a failed
//! verification, compression or write.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use binary_writer_io::verify_write_quantized_mesh;

/// Verify uncompressed quantized-mesh tile bytes, then write them, gzip-compressed on request.
#[pyfunction]
fn write(py: Python<'_>, data: &[u8], file_path: &str, gzip_compress: bool) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    dict.set_item("data", py.None())?;
    match verify_write_quantized_mesh(data, file_path, gzip_compress) {
        Ok(_) => {
            dict.set_item("ok", true)?;
            dict.set_item("error", py.None())?;
        }
        Err(e) => {
            let err = PyDict::new_bound(py);
            err.set_item("type", e.error_type())?;
            err.set_item("message", e.to_string())?;
            dict.set_item("ok", false)?;
            dict.set_item("error", err)?;
        }
    }
    Ok(dict.into())
}

#[pymodule]
fn write_bytes_verify_quantized_mesh(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(write, m)?)?;
    Ok(())
}
