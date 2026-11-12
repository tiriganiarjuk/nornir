//! Binary reader: read_png_verify_heights
//! Reads a PNG elevation tile, verifies it is an image of exactly the stated
//! shape carrying nothing else, decodes its pixels to heights in metres, and
//! repairs bad pixels under the stated rules.
//! Returns the binary reader contract:
//! `{"ok": bool, "data": str | None, "payload": bytes | None, "error": {"type": str, "message": str} | None}`.
//! On success `payload` is the heights — 32-bit little-endian floats, row by
//! row from the top — and `data` is the JSON that describes them: `width`,
//! `height`, `payload_format`, `lowest_m`, `highest_m`, and `pixels_repaired`
//! by reason. On failure both are None. `error.type` is one of
//! `BinaryReadError::error_type()`; it never raises for a failed read,
//! verification or repair.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use binary_reader_io::{BinaryReadError, HeightEncoding, HeightsRead, PngExpectation};

/// The exact shape a tile image must have to be read. Every value is required.
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

/// How pixels encode heights, what counts as a believable height, and how far a
/// lone pixel may stand out before it is repaired. Every value is required.
#[pyclass(frozen)]
#[derive(Clone)]
struct HeightRules {
    /// The pixel encoding: "terrarium".
    #[pyo3(get)]
    encoding: String,
    #[pyo3(get)]
    minimum_m: f32,
    #[pyo3(get)]
    maximum_m: f32,
    /// True for an area with no land below sea level.
    #[pyo3(get)]
    sea_level_floor: bool,
    #[pyo3(get)]
    lone_pixel_threshold_m: f32,
}

#[pymethods]
impl HeightRules {
    #[new]
    fn new(encoding: String, minimum_m: f32, maximum_m: f32, sea_level_floor: bool, lone_pixel_threshold_m: f32) -> Self {
        HeightRules { encoding, minimum_m, maximum_m, sea_level_floor, lone_pixel_threshold_m }
    }
}

fn read_heights(file_path: &str, shape: PngShape, rules: &HeightRules) -> Result<HeightsRead, BinaryReadError> {
    let expectation = PngExpectation {
        width: shape.width,
        height: shape.height,
        channels: shape.channels,
        bit_depth: shape.bit_depth,
    };
    let height_rules = binary_reader_io::HeightRules {
        minimum_m: rules.minimum_m,
        maximum_m: rules.maximum_m,
        sea_level_floor: rules.sea_level_floor,
        lone_pixel_threshold_m: rules.lone_pixel_threshold_m,
    };
    let encoding = HeightEncoding::from_name(&rules.encoding)?;
    binary_reader_io::read_png_verify_heights(file_path, &expectation, encoding, &height_rules)
}

/// Read a PNG elevation tile of `shape`; return its repaired heights and the JSON describing them.
#[pyfunction]
fn read(py: Python<'_>, file_path: &str, shape: PngShape, rules: PyRef<'_, HeightRules>) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);
    match read_heights(file_path, shape, &rules) {
        Ok(heights_read) => {
            dict.set_item("ok", true)?;
            dict.set_item("data", &heights_read.report_json)?;
            dict.set_item("payload", PyBytes::new_bound(py, &heights_read.heights_bytes))?;
            dict.set_item("error", py.None())?;
        }
        Err(e) => {
            let err = PyDict::new_bound(py);
            err.set_item("type", e.error_type())?;
            err.set_item("message", e.to_string())?;
            dict.set_item("ok", false)?;
            dict.set_item("data", py.None())?;
            dict.set_item("payload", py.None())?;
            dict.set_item("error", err)?;
        }
    }
    Ok(dict.into())
}

#[pymodule]
fn read_png_verify_heights(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PngShape>()?;
    m.add_class::<HeightRules>()?;
    m.add_function(wrap_pyfunction!(read, m)?)?;
    Ok(())
}
