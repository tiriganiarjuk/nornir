//! Verification of binary file formats, on bytes in memory.
//!
//! Each module answers one question about one format: are these bytes a file
//! of this format, of exactly the shape the caller states, carrying nothing
//! beyond what that shape requires? A file that passes holds its declared
//! content and has no room for anything else — no trailing bytes, no free-form
//! chunks or extensions, no slack inside a compressed stream.
//!
//! Pure: bytes in, verdict out. The `binary_writer_io` capability puts these
//! verdicts in front of the disk.

pub mod png;
pub mod quantized_mesh;
