//! Shared binary-reader IO — read a binary file, verify it, hand back typed content.
//!
//! The binary sibling of `reader_io`. A binary reader reads one file from disk,
//! has `binary_format_core` verify that it is a file of one named format, of
//! the exact shape the caller states, carrying nothing else, and only then
//! decodes it. Each `binary_readers/read_{format}_verify_{content}` crate is a
//! thin PyO3 wrapper over one function here.
//!
//! A binary reader hands back two things that belong together: a payload of
//! bytes, and JSON that describes the payload — its dimensions, its number
//! format, and what was found in it. The caller validates the JSON into a model
//! and reads the bytes by what the model says. Failures are a typed error:
//! `BinaryReadError::error_type()` names the class.
//!
//! `read_png_verify_heights` reads an elevation tile: a PNG whose pixels encode
//! heights. Flow: read file → verify and decode the PNG → decode heights →
//! repair bad pixels (`elevation_core`) → the heights as the payload, plus the
//! JSON describing them and what was repaired.

use std::fs;

use serde_json::json;

use binary_format_core::png::{decode_png, PngError};
use elevation_core::{decode_terrarium, repair_heights, ElevationError, HeightGrid};

pub use binary_format_core::png::PngExpectation;
pub use elevation_core::HeightRules;

/// The height encodings a PNG elevation tile may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightEncoding {
    /// red × 256 + green + blue ÷ 256 − 32768 metres, in 8-bit RGB.
    Terrarium,
}

impl HeightEncoding {
    /// The encoding with this name, as callers spell it.
    pub fn from_name(name: &str) -> Result<Self, BinaryReadError> {
        match name {
            "terrarium" => Ok(HeightEncoding::Terrarium),
            other => Err(BinaryReadError::EncodingUnknown { name: other.to_string() }),
        }
    }
}

/// Every failure a binary reader can produce. Display strings are what the
/// Python caller sees as the `ValueError` text.
#[derive(Debug, thiserror::Error)]
pub enum BinaryReadError {
    #[error("Cannot read '{path}': {message}")]
    ReadFailed { path: String, message: String },

    #[error("Refused to read '{path}': {source}")]
    PngRefused { path: String, source: PngError },

    #[error("Height encoding '{name}' is not known; the encodings are: terrarium")]
    EncodingUnknown { name: String },

    #[error(
        "Terrarium heights are stored in 3 channels of 8 bits; \
         the shape given has {channels} channels of {bit_depth} bits"
    )]
    ShapeNotTerrarium { channels: u8, bit_depth: u8 },

    #[error("Cannot use the heights in '{path}': {source}")]
    HeightsRefused { path: String, source: ElevationError },
}

impl BinaryReadError {
    /// The failure class a caller branches on — the `error.type` of the reader result.
    /// `read_error`: the file could not be read. `parse_error` / `verification_error`:
    /// the class of the PNG error underneath. `rules_error`: the encoding, shape or
    /// height rules the caller stated cannot be used. `repair_error`: the tile holds
    /// no usable heights under those rules.
    pub fn error_type(&self) -> &'static str {
        match self {
            BinaryReadError::ReadFailed { .. } => "read_error",
            BinaryReadError::PngRefused { source, .. } => source.error_type(),
            BinaryReadError::EncodingUnknown { .. } | BinaryReadError::ShapeNotTerrarium { .. } => "rules_error",
            BinaryReadError::HeightsRefused { source, .. } => source.error_type(),
        }
    }
}

/// How the heights payload stores each number.
const PAYLOAD_FORMAT: &str = "float32_le";

/// What a read elevation tile holds: its heights as little-endian 32-bit
/// floats, row by row from the top, and the JSON that describes them.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightsRead {
    pub heights_bytes: Vec<u8>,
    pub report_json: String,
}

fn report_json(grid: &HeightGrid) -> String {
    json!({
        "width": grid.width,
        "height": grid.height,
        "payload_format": PAYLOAD_FORMAT,
        "lowest_m": grid.lowest_m,
        "highest_m": grid.highest_m,
        "pixels_repaired": {
            "outside_height_range": grid.repaired.outside_height_range,
            "below_sea_level": grid.repaired.below_sea_level,
            "lone_pixel": grid.repaired.lone_pixel,
        },
    })
    .to_string()
}

/// Read the PNG elevation tile at `file_path`, verify it has exactly the
/// expected shape, decode its heights, and repair its bad pixels under `rules`.
pub fn read_png_verify_heights(
    file_path: &str,
    expectation: &PngExpectation,
    encoding: HeightEncoding,
    rules: &HeightRules,
) -> Result<HeightsRead, BinaryReadError> {
    let HeightEncoding::Terrarium = encoding;
    if expectation.channels != 3 || expectation.bit_depth != 8 {
        return Err(BinaryReadError::ShapeNotTerrarium {
            channels: expectation.channels,
            bit_depth: expectation.bit_depth,
        });
    }
    let file_bytes = fs::read(file_path).map_err(|io_err| BinaryReadError::ReadFailed {
        path: file_path.to_string(),
        message: io_err.to_string(),
    })?;
    let samples = decode_png(&file_bytes, expectation)
        .map_err(|source| BinaryReadError::PngRefused { path: file_path.to_string(), source })?;
    let grid = repair_heights(
        expectation.width as usize,
        expectation.height as usize,
        decode_terrarium(&samples),
        rules,
    )
    .map_err(|source| BinaryReadError::HeightsRefused { path: file_path.to_string(), source })?;
    let heights_bytes = grid.heights.iter().flat_map(|height_m| height_m.to_le_bytes()).collect();
    Ok(HeightsRead { heights_bytes, report_json: report_json(&grid) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const SHAPE: PngExpectation = PngExpectation { width: 3, height: 3, channels: 3, bit_depth: 8 };
    const LAND: HeightRules =
        HeightRules { minimum_m: -430.0, maximum_m: 8849.0, sea_level_floor: true, lone_pixel_threshold_m: 20.0 };

    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_binary_reader_io_{}", test_name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Terrarium RGB for a whole number of metres.
    fn terrarium(height_m: i32) -> [u8; 3] {
        let offset = height_m + 32768;
        [(offset / 256) as u8, (offset % 256) as u8, 0]
    }

    /// Write a 3x3 RGB PNG whose nine pixels encode `heights_m`.
    fn write_tile(dir: &PathBuf, heights_m: [i32; 9]) -> String {
        let path = dir.join("tile.png");
        let samples: Vec<u8> = heights_m.iter().flat_map(|height_m| terrarium(*height_m)).collect();
        let mut encoder = png::Encoder::new(fs::File::create(&path).unwrap(), 3, 3);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&samples).unwrap();
        writer.finish().unwrap();
        path.to_str().unwrap().to_string()
    }

    fn heights_of(read: &HeightsRead) -> Vec<f32> {
        read.heights_bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect()
    }

    #[test]
    fn clean_tile_reads_as_its_heights() {
        let dir = temp_dir("clean");
        let file = write_tile(&dir, [10, 11, 12, 13, 14, 15, 16, 17, 18]);
        let read = read_png_verify_heights(&file, &SHAPE, HeightEncoding::Terrarium, &LAND).unwrap();
        assert_eq!(heights_of(&read), vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0]);
        let report: serde_json::Value = serde_json::from_str(&read.report_json).unwrap();
        assert_eq!(report["payload_format"], "float32_le");
        assert_eq!((report["width"].as_u64(), report["height"].as_u64()), (Some(3), Some(3)));
        assert_eq!(report["lowest_m"], 10.0);
        assert_eq!(report["highest_m"], 18.0);
        assert_eq!(report["pixels_repaired"]["lone_pixel"], 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lone_pixel_is_repaired_and_counted() {
        let dir = temp_dir("lone");
        let file = write_tile(&dir, [26, 26, 26, 26, 0, 26, 26, 26, 26]);
        let read = read_png_verify_heights(&file, &SHAPE, HeightEncoding::Terrarium, &LAND).unwrap();
        assert_eq!(heights_of(&read), vec![26.0; 9]);
        let report: serde_json::Value = serde_json::from_str(&read.report_json).unwrap();
        assert_eq!(report["pixels_repaired"]["lone_pixel"], 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tile_of_another_size_is_refused() {
        let dir = temp_dir("wrong_size");
        let file = write_tile(&dir, [1; 9]);
        let bigger = PngExpectation { width: 256, height: 256, ..SHAPE };
        let result = read_png_verify_heights(&file, &bigger, HeightEncoding::Terrarium, &LAND);
        assert!(matches!(result, Err(BinaryReadError::PngRefused { .. })), "{result:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_that_is_not_an_image_is_refused() {
        let dir = temp_dir("not_image");
        let path = dir.join("tile.png");
        fs::write(&path, b"not an image").unwrap();
        let result = read_png_verify_heights(path.to_str().unwrap(), &SHAPE, HeightEncoding::Terrarium, &LAND);
        assert!(matches!(result, Err(BinaryReadError::PngRefused { .. })), "{result:?}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_is_reported() {
        let result = read_png_verify_heights("/absolutely/does/not/exist.png", &SHAPE, HeightEncoding::Terrarium, &LAND);
        assert!(matches!(result, Err(BinaryReadError::ReadFailed { .. })), "{result:?}");
        assert_eq!(result.unwrap_err().error_type(), "read_error");
    }

    #[test]
    fn failure_classes_follow_the_error_underneath() {
        let dir = temp_dir("classes");
        let file = write_tile(&dir, [1; 9]);
        let bigger = PngExpectation { width: 256, height: 256, ..SHAPE };
        let wrong_shape = read_png_verify_heights(&file, &bigger, HeightEncoding::Terrarium, &LAND).unwrap_err();
        assert_eq!(wrong_shape.error_type(), "verification_error");
        let nothing_believable = HeightRules { minimum_m: 5000.0, ..LAND };
        let no_heights = read_png_verify_heights(&file, &SHAPE, HeightEncoding::Terrarium, &nothing_believable).unwrap_err();
        assert_eq!(no_heights.error_type(), "repair_error");
        let grey = PngExpectation { channels: 1, ..SHAPE };
        let wrong_rules = read_png_verify_heights(&file, &grey, HeightEncoding::Terrarium, &LAND).unwrap_err();
        assert_eq!(wrong_rules.error_type(), "rules_error");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_encoding_name_is_refused() {
        assert!(matches!(HeightEncoding::from_name("mapbox"), Err(BinaryReadError::EncodingUnknown { .. })));
        assert_eq!(HeightEncoding::from_name("terrarium").unwrap(), HeightEncoding::Terrarium);
    }

    #[test]
    fn shape_that_cannot_hold_terrarium_is_refused() {
        let grey = PngExpectation { channels: 1, ..SHAPE };
        let result = read_png_verify_heights("/unused.png", &grey, HeightEncoding::Terrarium, &LAND);
        assert!(matches!(result, Err(BinaryReadError::ShapeNotTerrarium { .. })), "{result:?}");
    }
}
