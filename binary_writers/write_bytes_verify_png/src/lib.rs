//! Binary writer: write_bytes_verify_png
//! Takes PNG bytes, verifies they are an image of exactly the stated shape —
//! width, height, channel count and bit depth — carrying nothing else, then
//! writes them to `file_path` atomically. Refused bytes never reach the disk.
//! Returns the gate contract: `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.
//! A write has no data to hand back, so `data` is always None. `error.type` is
//! one of `BinaryWriteError::error_type()`; it never raises for a failed
//! verification or write.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use binary_writer_io::{verify_write_png, PngExpectation};

/// The exact shape an image must have to be written. Every value is required.
#[pyclass(frozen)]
#[derive(Clone, Copy)]
struct PngShape {
    #[pyo3(get)]
    width: u32,
    #[pyo3(get)]
    height: u32,
    /// Samples per pixel: 1 grey or palette, 2 grey+alpha, 3 RGB, 4 RGBA.
    #[pyo3(get)]
    channels: u8,
    /// Bits per sample.
    #[pyo3(get)]
    bit_depth: u8,
}

#[pymethods]
impl PngShape {
    #[new]
    fn new(width: u32, height: u32, channels: u8, bit_depth: u8) -> Self {
        PngShape { width, height, channels, bit_depth }
    }
}

/// Verify PNG bytes against `shape`, then write them.
#[pyfunction]
fn write(py: Python<'_>, data: &[u8], file_path: &str, shape: PngShape) -> PyResult<PyObject> {
    let expectation = PngExpectation {
        width: shape.width,
        height: shape.height,
        channels: shape.channels,
        bit_depth: shape.bit_depth,
    };
    let dict = PyDict::new_bound(py);
    dict.set_item("data", py.None())?;
    match verify_write_png(data, file_path, &expectation) {
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
fn write_bytes_verify_png(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PngShape>()?;
    m.add_function(wrap_pyfunction!(write, m)?)?;
    Ok(())
}
