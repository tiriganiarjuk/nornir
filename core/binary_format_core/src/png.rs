//! PNG verification.
//!
//! A PNG passes when: its signature is right; every chunk is whole, has a
//! correct CRC, is of a kind this verifier knows, and is within that kind's
//! size limit; the header states exactly the width, height, channel count and
//! bit depth the caller expects; the compressed pixel data inflates to exactly
//! the image the header describes, with nothing left over; and the file ends at
//! the end chunk.
//!
//! The chunk kinds accepted are the ones that carry the image and fixed-size
//! facts about its colour. Text, metadata, colour-profile and private chunks
//! are refused: they hold free-form bytes.

use flate2::{Crc, Decompress, FlushDecompress, Status};

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const CHUNK_OVERHEAD: usize = 12;
const INFLATE_BUFFER_BYTES: usize = 32 * 1024;

/// Adam7 passes as (first column, first row, column step, row step).
const ADAM7_PASSES: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// The exact shape a PNG must have to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngExpectation {
    pub width: u32,
    pub height: u32,
    /// Samples per pixel: 1 grey or palette, 2 grey+alpha, 3 RGB, 4 RGBA.
    pub channels: u8,
    /// Bits per sample.
    pub bit_depth: u8,
}

/// Why bytes were refused as a PNG. Each message says what was found and what a
/// passing file has instead.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PngError {
    #[error("not a PNG: the first 8 bytes are not the PNG signature")]
    SignatureMissing,

    #[error("PNG is cut short: a chunk starting at byte {offset} runs past the end of the data")]
    ChunkTruncated { offset: usize },

    #[error("PNG chunk '{kind}' is corrupt: its CRC does not match its content")]
    ChunkCrcMismatch { kind: String },

    #[error(
        "PNG chunk '{kind}' is not accepted: only image data and fixed-size colour facts may be written \
         (IHDR, PLTE, IDAT, IEND, tRNS, gAMA, cHRM, sRGB, sBIT, bKGD, pHYs). \
         Re-encode the image without text, metadata or profile chunks."
    )]
    ChunkNotAccepted { kind: String },

    #[error("PNG chunk '{kind}' holds {length} bytes; that kind holds at most {limit}")]
    ChunkTooLarge { kind: String, length: usize, limit: usize },

    #[error("PNG chunks are out of order: {0}")]
    ChunkOrder(String),

    #[error("PNG has {bytes} bytes after its end chunk; a passing file ends at the end chunk")]
    TrailingData { bytes: usize },

    #[error("PNG header is invalid: {0}")]
    HeaderInvalid(String),

    #[error("PNG {field} is {found}; this writer was told to expect {expected}")]
    ExpectationMismatch {
        field: &'static str,
        expected: u32,
        found: u32,
    },

    #[error("PNG pixel data is invalid: {0}")]
    PixelData(String),

    #[error("PNG pixel data holds more than the image: {0}")]
    PixelDataExcess(String),

    #[error("PNG cannot be decoded: {0}")]
    DecodeFailed(String),
}

impl PngError {
    /// The failure class a caller branches on — the `error.type` of a result.
    /// `parse_error`: the bytes are not a whole, well-formed PNG.
    /// `verification_error`: a well-formed PNG that is not exactly the stated
    /// shape, or carries something beyond it.
    pub fn error_type(&self) -> &'static str {
        match self {
            PngError::SignatureMissing
            | PngError::ChunkTruncated { .. }
            | PngError::ChunkCrcMismatch { .. }
            | PngError::ChunkOrder(_)
            | PngError::HeaderInvalid(_)
            | PngError::PixelData(_)
            | PngError::DecodeFailed(_) => "parse_error",
            PngError::ChunkNotAccepted { .. }
            | PngError::ChunkTooLarge { .. }
            | PngError::TrailingData { .. }
            | PngError::ExpectationMismatch { .. }
            | PngError::PixelDataExcess(_) => "verification_error",
        }
    }
}

struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
}

struct Header {
    width: u32,
    height: u32,
    bit_depth: u8,
    channels: u8,
    interlaced: bool,
}

/// Rows of one run: every row in it has the same length in bytes, filter byte included.
struct RowRun {
    row_bytes: usize,
    rows: usize,
}

/// Verify that `data` is a PNG of exactly the expected shape, carrying nothing else.
pub fn verify_png(data: &[u8], expectation: &PngExpectation) -> Result<(), PngError> {
    let chunks = read_chunks(data)?;
    check_chunk_order(&chunks)?;
    let header = parse_header(chunks[0].data)?;
    check_expectation(&header, expectation)?;
    let compressed: Vec<u8> = chunks
        .iter()
        .filter(|chunk| &chunk.kind == b"IDAT")
        .flat_map(|chunk| chunk.data.iter().copied())
        .collect();
    check_pixel_data(&compressed, &row_runs(&header))
}

/// Verify `data` exactly as `verify_png` does, then decode it to its samples:
/// row by row from the top, pixels left to right, channels interleaved. Samples
/// deeper than 8 bits are two bytes each, high byte first, as PNG stores them.
pub fn decode_png(data: &[u8], expectation: &PngExpectation) -> Result<Vec<u8>, PngError> {
    verify_png(data, expectation)?;
    let decode_failed = |decode_error: png::DecodingError| PngError::DecodeFailed(decode_error.to_string());
    let mut reader = png::Decoder::new(std::io::Cursor::new(data)).read_info().map_err(decode_failed)?;
    let buffer_bytes = reader
        .output_buffer_size()
        .ok_or_else(|| PngError::DecodeFailed("the image is too large to hold in memory".to_string()))?;
    let mut samples = vec![0u8; buffer_bytes];
    let frame = reader.next_frame(&mut samples).map_err(decode_failed)?;
    samples.truncate(frame.buffer_size());
    Ok(samples)
}

fn kind_name(kind: [u8; 4]) -> String {
    String::from_utf8_lossy(&kind).into_owned()
}

/// The most bytes a chunk of this kind may hold, or `None` for a kind that is not accepted.
fn chunk_length_limit(kind: [u8; 4]) -> Option<usize> {
    match &kind {
        b"IHDR" => Some(13),
        b"PLTE" => Some(768),
        b"IDAT" => Some(u32::MAX as usize),
        b"IEND" => Some(0),
        b"tRNS" => Some(256),
        b"gAMA" => Some(4),
        b"cHRM" => Some(32),
        b"sRGB" => Some(1),
        b"sBIT" => Some(4),
        b"bKGD" => Some(6),
        b"pHYs" => Some(9),
        _ => None,
    }
}

fn read_u32_be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_chunk(data: &[u8], offset: usize) -> Result<Chunk<'_>, PngError> {
    let truncated = PngError::ChunkTruncated { offset };
    let head = data.get(offset..offset + 8).ok_or(truncated)?;
    let length = read_u32_be(head) as usize;
    let kind = [head[4], head[5], head[6], head[7]];
    let body_end = offset + 8 + length;
    let body = data.get(offset + 8..body_end).ok_or(PngError::ChunkTruncated { offset })?;
    let stored_crc = data.get(body_end..body_end + 4).ok_or(PngError::ChunkTruncated { offset })?;

    let limit = chunk_length_limit(kind).ok_or_else(|| PngError::ChunkNotAccepted { kind: kind_name(kind) })?;
    if length > limit {
        return Err(PngError::ChunkTooLarge { kind: kind_name(kind), length, limit });
    }
    let mut crc = Crc::new();
    crc.update(&kind);
    crc.update(body);
    if crc.sum() != read_u32_be(stored_crc) {
        return Err(PngError::ChunkCrcMismatch { kind: kind_name(kind) });
    }
    Ok(Chunk { kind, data: body })
}

fn read_chunks(data: &[u8]) -> Result<Vec<Chunk<'_>>, PngError> {
    if data.get(..SIGNATURE.len()) != Some(&SIGNATURE[..]) {
        return Err(PngError::SignatureMissing);
    }
    let mut chunks = Vec::new();
    let mut offset = SIGNATURE.len();
    while offset < data.len() {
        let chunk = read_chunk(data, offset)?;
        offset += CHUNK_OVERHEAD + chunk.data.len();
        let is_end = &chunk.kind == b"IEND";
        chunks.push(chunk);
        if is_end {
            break;
        }
    }
    if offset != data.len() {
        return Err(PngError::TrailingData { bytes: data.len() - offset });
    }
    Ok(chunks)
}

fn check_chunk_order(chunks: &[Chunk<'_>]) -> Result<(), PngError> {
    let kinds: Vec<&[u8; 4]> = chunks.iter().map(|chunk| &chunk.kind).collect();
    if kinds.first() != Some(&b"IHDR") {
        return Err(PngError::ChunkOrder("the first chunk must be IHDR".to_string()));
    }
    if kinds.last() != Some(&b"IEND") {
        return Err(PngError::ChunkOrder("the last chunk must be IEND".to_string()));
    }
    if kinds.iter().filter(|kind| **kind == b"IHDR").count() != 1 {
        return Err(PngError::ChunkOrder("there must be exactly one IHDR".to_string()));
    }
    let first_data = kinds.iter().position(|kind| *kind == b"IDAT");
    let last_data = kinds.iter().rposition(|kind| *kind == b"IDAT");
    match (first_data, last_data) {
        (Some(first), Some(last)) if kinds[first..=last].iter().all(|kind| *kind == b"IDAT") => Ok(()),
        (Some(_), Some(_)) => Err(PngError::ChunkOrder("IDAT chunks must be consecutive".to_string())),
        _ => Err(PngError::ChunkOrder("there is no IDAT chunk, so no image".to_string())),
    }
}

fn channels_for(colour_type: u8, bit_depth: u8) -> Result<u8, PngError> {
    let (channels, depths): (u8, &[u8]) = match colour_type {
        0 => (1, &[1, 2, 4, 8, 16]),
        2 => (3, &[8, 16]),
        3 => (1, &[1, 2, 4, 8]),
        4 => (2, &[8, 16]),
        6 => (4, &[8, 16]),
        other => return Err(PngError::HeaderInvalid(format!("colour type {other} does not exist"))),
    };
    if depths.contains(&bit_depth) {
        Ok(channels)
    } else {
        Err(PngError::HeaderInvalid(format!(
            "bit depth {bit_depth} is not allowed with colour type {colour_type}"
        )))
    }
}

fn parse_header(body: &[u8]) -> Result<Header, PngError> {
    if body.len() != 13 {
        return Err(PngError::HeaderInvalid(format!("IHDR holds {} bytes, not 13", body.len())));
    }
    let width = read_u32_be(&body[0..4]);
    let height = read_u32_be(&body[4..8]);
    if width == 0 || height == 0 {
        return Err(PngError::HeaderInvalid("width and height must both be above zero".to_string()));
    }
    let (bit_depth, colour_type, compression, filter, interlace) = (body[8], body[9], body[10], body[11], body[12]);
    if compression != 0 || filter != 0 || interlace > 1 {
        return Err(PngError::HeaderInvalid(
            "compression and filter methods must be 0 and interlace 0 or 1".to_string(),
        ));
    }
    let channels = channels_for(colour_type, bit_depth)?;
    Ok(Header { width, height, bit_depth, channels, interlaced: interlace == 1 })
}

fn check_expectation(header: &Header, expectation: &PngExpectation) -> Result<(), PngError> {
    let pairs = [
        ("width", expectation.width, header.width),
        ("height", expectation.height, header.height),
        ("channel count", u32::from(expectation.channels), u32::from(header.channels)),
        ("bit depth", u32::from(expectation.bit_depth), u32::from(header.bit_depth)),
    ];
    match pairs.iter().find(|(_, expected, found)| expected != found) {
        Some(&(field, expected, found)) => Err(PngError::ExpectationMismatch { field, expected, found }),
        None => Ok(()),
    }
}

/// Bytes in one row of `width` pixels, filter byte included.
fn row_bytes(header: &Header, width: u32) -> usize {
    let bits = width as usize * usize::from(header.channels) * usize::from(header.bit_depth);
    1 + bits.div_ceil(8)
}

/// How many of `size` columns or rows a pass starting at `first` with `step` covers.
fn pass_extent(size: u32, first: u32, step: u32) -> u32 {
    if size > first {
        (size - first).div_ceil(step)
    } else {
        0
    }
}

/// The rows the inflated pixel data must consist of, in order.
fn row_runs(header: &Header) -> Vec<RowRun> {
    if !header.interlaced {
        return vec![RowRun { row_bytes: row_bytes(header, header.width), rows: header.height as usize }];
    }
    ADAM7_PASSES
        .iter()
        .map(|&(first_column, first_row, column_step, row_step)| {
            (
                pass_extent(header.width, first_column, column_step),
                pass_extent(header.height, first_row, row_step),
            )
        })
        .filter(|&(pass_width, pass_height)| pass_width > 0 && pass_height > 0)
        .map(|(pass_width, pass_height)| RowRun {
            row_bytes: row_bytes(header, pass_width),
            rows: pass_height as usize,
        })
        .collect()
}

/// Walks inflated bytes row by row, checking each row's filter byte.
struct RowWalk<'a> {
    runs: &'a [RowRun],
    run_index: usize,
    rows_done: usize,
    position_in_row: usize,
}

impl RowWalk<'_> {
    fn finished(&self) -> bool {
        self.run_index == self.runs.len()
    }

    fn step_row(&mut self) {
        self.position_in_row = 0;
        self.rows_done += 1;
        if self.rows_done == self.runs[self.run_index].rows {
            self.rows_done = 0;
            self.run_index += 1;
        }
    }

    fn consume(&mut self, mut bytes: &[u8]) -> Result<(), PngError> {
        while let Some(&first) = bytes.first() {
            if self.finished() {
                return Err(PngError::PixelDataExcess("it inflates to more bytes than the image has pixels for".to_string()));
            }
            if self.position_in_row == 0 && first > 4 {
                return Err(PngError::PixelData(format!("a row starts with filter type {first}; types run 0 to 4")));
            }
            let row_remaining = self.runs[self.run_index].row_bytes - self.position_in_row;
            let taken = row_remaining.min(bytes.len());
            self.position_in_row += taken;
            bytes = &bytes[taken..];
            if taken == row_remaining {
                self.step_row();
            }
        }
        Ok(())
    }
}

fn check_pixel_data(compressed: &[u8], runs: &[RowRun]) -> Result<(), PngError> {
    let mut inflater = Decompress::new(true);
    let mut buffer = vec![0u8; INFLATE_BUFFER_BYTES];
    let mut walk = RowWalk { runs, run_index: 0, rows_done: 0, position_in_row: 0 };
    loop {
        let consumed_before = inflater.total_in() as usize;
        let produced_before = inflater.total_out() as usize;
        let status = inflater
            .decompress(&compressed[consumed_before..], &mut buffer, FlushDecompress::None)
            .map_err(|inflate_error| PngError::PixelData(format!("it does not inflate: {inflate_error}")))?;
        let produced = inflater.total_out() as usize - produced_before;
        walk.consume(&buffer[..produced])?;
        let stalled = produced == 0 && inflater.total_in() as usize == consumed_before;
        match status {
            Status::StreamEnd => break,
            _ if stalled => return Err(PngError::PixelData("the compressed stream is cut short".to_string())),
            _ => {}
        }
    }
    if inflater.total_in() as usize != compressed.len() {
        return Err(PngError::PixelDataExcess(
            "bytes follow the end of the compressed stream inside the IDAT chunks".to_string(),
        ));
    }
    if !walk.finished() || walk.position_in_row != 0 {
        return Err(PngError::PixelData("it holds fewer bytes than the image has pixels for".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    const RGB_8: PngExpectation = PngExpectation { width: 4, height: 3, channels: 3, bit_depth: 8 };

    fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut crc = Crc::new();
        crc.update(kind);
        crc.update(body);
        let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc.sum().to_be_bytes());
        bytes
    }

    fn header_body(width: u32, height: u32, bit_depth: u8, colour_type: u8, interlace: u8) -> Vec<u8> {
        let mut body = width.to_be_bytes().to_vec();
        body.extend_from_slice(&height.to_be_bytes());
        body.extend_from_slice(&[bit_depth, colour_type, 0, 0, interlace]);
        body
    }

    fn deflate(raw: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }

    /// A PNG from a header body, the inflated pixel bytes, and any chunks to place before IDAT.
    fn png(header: &[u8], raw_pixels: &[u8], extra_chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend(chunk(b"IHDR", header));
        for extra in extra_chunks {
            bytes.extend_from_slice(extra);
        }
        bytes.extend(chunk(b"IDAT", &deflate(raw_pixels)));
        bytes.extend(chunk(b"IEND", &[]));
        bytes
    }

    /// 4x3 RGB 8-bit: three rows of one filter byte plus twelve sample bytes.
    fn rgb_pixels() -> Vec<u8> {
        vec![0u8; 3 * (1 + 4 * 3)]
    }

    fn rgb_png() -> Vec<u8> {
        png(&header_body(4, 3, 8, 2, 0), &rgb_pixels(), &[])
    }

    // -- accepted --

    #[test]
    fn plain_rgb_image_passes() {
        assert_eq!(verify_png(&rgb_png(), &RGB_8), Ok(()));
    }

    #[test]
    fn fixed_size_colour_chunk_passes() {
        let with_gamma = png(&header_body(4, 3, 8, 2, 0), &rgb_pixels(), &[chunk(b"gAMA", &[0, 0, 177, 143])]);
        assert_eq!(verify_png(&with_gamma, &RGB_8), Ok(()));
    }

    #[test]
    fn image_split_across_data_chunks_passes() {
        let compressed = deflate(&rgb_pixels());
        let (first_half, second_half) = compressed.split_at(compressed.len() / 2);
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend(chunk(b"IHDR", &header_body(4, 3, 8, 2, 0)));
        bytes.extend(chunk(b"IDAT", first_half));
        bytes.extend(chunk(b"IDAT", second_half));
        bytes.extend(chunk(b"IEND", &[]));
        assert_eq!(verify_png(&bytes, &RGB_8), Ok(()));
    }

    #[test]
    fn interlaced_image_passes() {
        // 4x3 grey 8-bit, Adam7. Passes cover (columns x rows): 1x1, 0x1 (skipped), 1x0 (skipped),
        // 1x1, 2x1, 2x2, 4x1 — so rows of 2, 2, 3, 3, 3, 5 bytes.
        let raw = vec![0u8; 2 + 2 + 3 + 3 + 3 + 5];
        let interlaced = png(&header_body(4, 3, 8, 0, 1), &raw, &[]);
        let grey = PngExpectation { width: 4, height: 3, channels: 1, bit_depth: 8 };
        assert_eq!(verify_png(&interlaced, &grey), Ok(()));
    }

    // -- decoded --

    #[test]
    fn decoding_returns_the_samples_row_by_row() {
        // 2x2 grey 8-bit, filter type 0 on both rows: samples 10 20 / 30 40.
        let grey = png(&header_body(2, 2, 8, 0, 0), &[0, 10, 20, 0, 30, 40], &[]);
        let expectation = PngExpectation { width: 2, height: 2, channels: 1, bit_depth: 8 };
        assert_eq!(decode_png(&grey, &expectation), Ok(vec![10, 20, 30, 40]));
    }

    #[test]
    fn decoding_refuses_what_verification_refuses() {
        let mut bytes = rgb_png();
        bytes.extend_from_slice(b"appended payload");
        assert_eq!(decode_png(&bytes, &RGB_8), Err(PngError::TrailingData { bytes: 16 }));
    }

    // -- refused: not the expected shape --

    #[test]
    fn wrong_width_is_refused() {
        let expectation = PngExpectation { width: 256, ..RGB_8 };
        assert_eq!(
            verify_png(&rgb_png(), &expectation),
            Err(PngError::ExpectationMismatch { field: "width", expected: 256, found: 4 })
        );
    }

    #[test]
    fn wrong_channel_count_is_refused() {
        let expectation = PngExpectation { channels: 4, ..RGB_8 };
        assert_eq!(
            verify_png(&rgb_png(), &expectation),
            Err(PngError::ExpectationMismatch { field: "channel count", expected: 4, found: 3 })
        );
    }

    // -- refused: not a whole PNG --

    #[test]
    fn other_bytes_are_refused() {
        assert_eq!(verify_png(b"plain text, not an image", &RGB_8), Err(PngError::SignatureMissing));
    }

    #[test]
    fn cut_short_file_is_refused() {
        let whole = rgb_png();
        let cut = &whole[..whole.len() - 6];
        assert!(matches!(verify_png(cut, &RGB_8), Err(PngError::ChunkTruncated { .. })));
    }

    #[test]
    fn corrupt_chunk_is_refused() {
        let mut bytes = rgb_png();
        let last_data_byte = bytes.len() - 12 - 5;
        bytes[last_data_byte] ^= 0xFF;
        assert_eq!(verify_png(&bytes, &RGB_8), Err(PngError::ChunkCrcMismatch { kind: "IDAT".to_string() }));
    }

    #[test]
    fn too_few_pixel_bytes_is_refused() {
        let short = png(&header_body(4, 3, 8, 2, 0), &vec![0u8; 2 * 13], &[]);
        assert!(matches!(verify_png(&short, &RGB_8), Err(PngError::PixelData(_))));
    }

    // -- refused: carrying something beyond the image --

    #[test]
    fn text_chunk_is_refused() {
        let with_text = png(&header_body(4, 3, 8, 2, 0), &rgb_pixels(), &[chunk(b"tEXt", b"Comment\0anything at all")]);
        assert_eq!(verify_png(&with_text, &RGB_8), Err(PngError::ChunkNotAccepted { kind: "tEXt".to_string() }));
    }

    #[test]
    fn private_chunk_is_refused() {
        let with_private = png(&header_body(4, 3, 8, 2, 0), &rgb_pixels(), &[chunk(b"prVt", &[1, 2, 3])]);
        assert_eq!(verify_png(&with_private, &RGB_8), Err(PngError::ChunkNotAccepted { kind: "prVt".to_string() }));
    }

    #[test]
    fn oversized_colour_chunk_is_refused() {
        let with_big_gamma = png(&header_body(4, 3, 8, 2, 0), &rgb_pixels(), &[chunk(b"gAMA", &[0u8; 64])]);
        assert_eq!(
            verify_png(&with_big_gamma, &RGB_8),
            Err(PngError::ChunkTooLarge { kind: "gAMA".to_string(), length: 64, limit: 4 })
        );
    }

    #[test]
    fn bytes_after_the_end_chunk_are_refused() {
        let mut bytes = rgb_png();
        bytes.extend_from_slice(b"appended payload");
        assert_eq!(verify_png(&bytes, &RGB_8), Err(PngError::TrailingData { bytes: 16 }));
    }

    #[test]
    fn bytes_after_the_compressed_stream_are_refused() {
        let mut compressed = deflate(&rgb_pixels());
        compressed.extend_from_slice(b"hidden after the stream");
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend(chunk(b"IHDR", &header_body(4, 3, 8, 2, 0)));
        bytes.extend(chunk(b"IDAT", &compressed));
        bytes.extend(chunk(b"IEND", &[]));
        assert!(matches!(verify_png(&bytes, &RGB_8), Err(PngError::PixelDataExcess(_))));
    }

    #[test]
    fn more_pixel_bytes_than_the_image_holds_are_refused() {
        let long = png(&header_body(4, 3, 8, 2, 0), &vec![0u8; 4 * 13], &[]);
        assert!(matches!(verify_png(&long, &RGB_8), Err(PngError::PixelDataExcess(_))));
    }

    // -- failure classes --

    #[test]
    fn malformed_bytes_are_a_parse_error() {
        assert_eq!(PngError::SignatureMissing.error_type(), "parse_error");
        assert_eq!(PngError::ChunkTruncated { offset: 8 }.error_type(), "parse_error");
    }

    #[test]
    fn well_formed_image_of_the_wrong_shape_or_with_extras_is_a_verification_error() {
        let mismatch = PngError::ExpectationMismatch { field: "width", expected: 256, found: 4 };
        assert_eq!(mismatch.error_type(), "verification_error");
        assert_eq!(PngError::TrailingData { bytes: 16 }.error_type(), "verification_error");
        assert_eq!(PngError::ChunkNotAccepted { kind: "tEXt".to_string() }.error_type(), "verification_error");
    }
}
