//! Shared binary-writer IO — verify bytes against their format, then write them.
//!
//! A binary writer takes bytes a Python stage already holds, has
//! `binary_format_core` verify that they are a file of one named format, of the
//! exact shape the caller states, carrying nothing else — and only then writes
//! them, atomically. Bytes that do not pass never reach the disk. Each
//! `binary_writers/write_bytes_verify_{format}` crate is a thin PyO3 wrapper
//! over one function here.
//!
//! Flow: verify → create the parent directory → write temp, fsync, rename →
//! done, or a typed error: `error_type()` names the failure class and the
//! message says what was found and what passes.

use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;

use binary_format_core::png::{verify_png, PngError};
use binary_format_core::quantized_mesh::{verify_quantized_mesh, MeshError};

pub use binary_format_core::png::PngExpectation;

/// Every failure a binary writer can produce. Display strings are the
/// `error.message` the Python caller receives.
#[derive(Debug, thiserror::Error)]
pub enum BinaryWriteError {
    #[error("Refused to write '{path}': {source}")]
    PngRefused { path: String, source: PngError },

    #[error("Refused to write '{path}': {source}")]
    QuantizedMeshRefused { path: String, source: MeshError },

    #[error("Cannot compress '{path}': {message}")]
    CompressFailed { path: String, message: String },

    #[error("Cannot write '{path}': {message}")]
    WriteFailed { path: String, message: String },
}

impl BinaryWriteError {
    /// The failure class a caller branches on — the `error.type` of the writer result.
    /// A refusal carries the class of the format error underneath it:
    /// `parse_error` (the bytes are not a well-formed file of the format) or
    /// `verification_error` (well-formed, but not exactly the stated shape).
    pub fn error_type(&self) -> &'static str {
        match self {
            BinaryWriteError::PngRefused { source, .. } => source.error_type(),
            BinaryWriteError::QuantizedMeshRefused { source, .. } => source.error_type(),
            BinaryWriteError::CompressFailed { .. } => "compression_error",
            BinaryWriteError::WriteFailed { .. } => "write_error",
        }
    }
}

/// Create the parent directory if needed, then write `bytes` to `file_path` atomically.
fn write_verified_bytes(file_path: &str, bytes: &[u8]) -> Result<usize, BinaryWriteError> {
    let write_failed = |message: String| BinaryWriteError::WriteFailed { path: file_path.to_string(), message };
    let path = Path::new(file_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|io_err| write_failed(format!("create directory: {io_err}")))?;
    }
    write_engine::write_bytes_atomic(path, bytes).map_err(write_failed)?;
    Ok(bytes.len())
}

fn compress_gzip(file_path: &str, bytes: &[u8]) -> Result<Vec<u8>, BinaryWriteError> {
    let compress_failed = |io_err: std::io::Error| BinaryWriteError::CompressFailed {
        path: file_path.to_string(),
        message: io_err.to_string(),
    };
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).map_err(compress_failed)?;
    encoder.finish().map_err(compress_failed)
}

/// Verify `data` is a PNG of exactly the expected shape, then write it to
/// `file_path`. Returns the number of bytes written.
pub fn verify_write_png(data: &[u8], file_path: &str, expectation: &PngExpectation) -> Result<usize, BinaryWriteError> {
    verify_png(data, expectation)
        .map_err(|source| BinaryWriteError::PngRefused { path: file_path.to_string(), source })?;
    write_verified_bytes(file_path, data)
}

/// Verify `data` is one whole uncompressed quantized-mesh tile, then write it to
/// `file_path` — gzip-compressed when `gzip_compress` is set, since tiles are
/// served gzipped. Returns the number of bytes written.
pub fn verify_write_quantized_mesh(data: &[u8], file_path: &str, gzip_compress: bool) -> Result<usize, BinaryWriteError> {
    verify_quantized_mesh(data)
        .map_err(|source| BinaryWriteError::QuantizedMeshRefused { path: file_path.to_string(), source })?;
    if gzip_compress {
        write_verified_bytes(file_path, &compress_gzip(file_path, data)?)
    } else {
        write_verified_bytes(file_path, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;

    use flate2::read::GzDecoder;

    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_binary_writer_io_{}", test_name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A flat square quantized-mesh tile: four vertices, two triangles.
    fn square_tile() -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        bytes.extend_from_slice(&100.0f32.to_le_bytes());
        for value in [1.0f64, 2.0, 3.0, 10.0, 4.0, 5.0, 6.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&4u32.to_le_bytes());
        // u, v, height: delta-and-zigzag coded corners (0,0) (32767,0) (0,32767) (32767,32767), flat.
        for code in [0u16, 65534, 65533, 65534, 0, 0, 65534, 0, 0, 0, 0, 0] {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for code in [0u16, 0, 0, 2, 0, 2] {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        for edge in [[0u16, 2], [0, 1], [1, 3], [2, 3]] {
            bytes.extend_from_slice(&2u32.to_le_bytes());
            for index in edge {
                bytes.extend_from_slice(&index.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn verified_tile_is_written_into_a_new_directory() {
        let dir = temp_dir("plain_tile");
        let file = dir.join("9/12/34.terrain");
        let tile = square_tile();
        let written = verify_write_quantized_mesh(&tile, file.to_str().unwrap(), false).unwrap();
        assert_eq!(written, tile.len());
        assert_eq!(fs::read(&file).unwrap(), tile);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gzip_compressed_tile_unpacks_to_the_verified_bytes() {
        let dir = temp_dir("gzip_tile");
        let file = dir.join("tile.terrain");
        let tile = square_tile();
        let written = verify_write_quantized_mesh(&tile, file.to_str().unwrap(), true).unwrap();
        let on_disk = fs::read(&file).unwrap();
        assert_eq!(written, on_disk.len());
        let mut unpacked = Vec::new();
        GzDecoder::new(&on_disk[..]).read_to_end(&mut unpacked).unwrap();
        assert_eq!(unpacked, tile);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refused_bytes_never_reach_the_disk() {
        let dir = temp_dir("refused");
        let file = dir.join("tile.terrain");
        let result = verify_write_quantized_mesh(b"not a terrain tile", file.to_str().unwrap(), false);
        assert!(matches!(result, Err(BinaryWriteError::QuantizedMeshRefused { .. })), "{result:?}");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refused_png_names_the_file_and_the_reason() {
        let dir = temp_dir("refused_png");
        let file = dir.join("tile.png");
        let expectation = PngExpectation { width: 256, height: 256, channels: 3, bit_depth: 8 };
        let error = verify_write_png(b"not an image", file.to_str().unwrap(), &expectation).unwrap_err();
        let message = error.to_string();
        assert!(message.starts_with("Refused to write"), "{message}");
        assert!(message.contains("not a PNG"), "{message}");
        assert_eq!(error.error_type(), "parse_error");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unwritable_destination_is_a_write_error() {
        let dir = temp_dir("unwritable");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"a file where a directory is needed").unwrap();
        let file = blocker.join("tile.terrain");
        let error = verify_write_quantized_mesh(&square_tile(), file.to_str().unwrap(), false).unwrap_err();
        assert_eq!(error.error_type(), "write_error");
        fs::remove_dir_all(&dir).ok();
    }
}
