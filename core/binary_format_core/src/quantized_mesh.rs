//! Quantized-mesh terrain tile verification (quantized-mesh-1.0, uncompressed bytes).
//!
//! A tile passes when its sections follow one another exactly as the format
//! lays them out and the data ends where the last section ends: the 88-byte
//! header, the vertex arrays, the triangle indices, the four edge lists, then
//! any extensions. Every count must match the bytes that follow it, every
//! decoded coordinate must lie in the format's 0..=32767 range, and every index
//! must name a vertex that exists.
//!
//! The extensions accepted are the two of fixed size: per-vertex normals and
//! the water mask. The metadata extension is refused: it holds free-form text.

const HEADER_BYTES: usize = 88;
const COORDINATE_MAX: i32 = 32767;
const NARROW_INDEX_VERTEX_LIMIT: u32 = 65536;
const EXTENSION_VERTEX_NORMALS: u8 = 1;
const EXTENSION_WATER_MASK: u8 = 2;
const WATER_MASK_GRID_BYTES: usize = 256 * 256;
const EDGE_NAMES: [&str; 4] = ["west", "south", "east", "north"];

/// What a passing tile holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshFacts {
    pub vertex_count: u32,
    pub triangle_count: u32,
    pub has_vertex_normals: bool,
    pub has_water_mask: bool,
}

/// Why bytes were refused as a quantized-mesh tile.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    #[error("quantized-mesh tile is cut short: the {section} needs {needed} more bytes but {remaining} remain")]
    Truncated {
        section: &'static str,
        needed: usize,
        remaining: usize,
    },

    #[error("quantized-mesh header is invalid: {0}")]
    HeaderInvalid(String),

    #[error("quantized-mesh vertex data is invalid: {0}")]
    VertexData(String),

    #[error("quantized-mesh triangle indices are invalid: {0}")]
    IndexData(String),

    #[error("quantized-mesh {edge} edge list is invalid: {reason}")]
    EdgeData { edge: &'static str, reason: String },

    #[error("quantized-mesh extension is not accepted: {0}")]
    Extension(String),
}

impl MeshError {
    /// The failure class a caller branches on — the `error.type` of a result.
    /// `parse_error`: the bytes are not a whole, well-formed tile.
    /// `verification_error`: a well-formed tile carrying an extension that may
    /// not be written.
    pub fn error_type(&self) -> &'static str {
        match self {
            MeshError::Truncated { .. }
            | MeshError::HeaderInvalid(_)
            | MeshError::VertexData(_)
            | MeshError::IndexData(_)
            | MeshError::EdgeData { .. } => "parse_error",
            MeshError::Extension(_) => "verification_error",
        }
    }
}

/// Reads little-endian values off the front of the data, naming the section on a short read.
struct ByteReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    fn take(&mut self, count: usize, section: &'static str) -> Result<&'a [u8], MeshError> {
        if count > self.remaining() {
            return Err(MeshError::Truncated { section, needed: count, remaining: self.remaining() });
        }
        let bytes = &self.data[self.position..self.position + count];
        self.position += count;
        Ok(bytes)
    }

    fn read_u32(&mut self, section: &'static str) -> Result<u32, MeshError> {
        let bytes = self.take(4, section)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read `count` indices of the tile's index width, widened to u32.
    fn read_indices(&mut self, count: usize, wide: bool, section: &'static str) -> Result<Vec<u32>, MeshError> {
        let width = if wide { 4 } else { 2 };
        let bytes = self.take(count.saturating_mul(width), section)?;
        Ok(bytes
            .chunks_exact(width)
            .map(|index| match index {
                [low, high] => u32::from(u16::from_le_bytes([*low, *high])),
                [first, second, third, fourth] => u32::from_le_bytes([*first, *second, *third, *fourth]),
                _ => 0,
            })
            .collect())
    }
}

/// Verify that `data` is one whole quantized-mesh tile and nothing else.
pub fn verify_quantized_mesh(data: &[u8]) -> Result<MeshFacts, MeshError> {
    let mut reader = ByteReader { data, position: 0 };
    check_header(reader.take(HEADER_BYTES, "header")?)?;
    let vertex_count = check_vertex_data(&mut reader)?;
    let wide = vertex_count > NARROW_INDEX_VERTEX_LIMIT;
    check_index_alignment(&mut reader, wide)?;
    let triangle_count = check_triangles(&mut reader, vertex_count, wide)?;
    for edge in EDGE_NAMES {
        check_edge(&mut reader, vertex_count, wide, edge)?;
    }
    let (has_vertex_normals, has_water_mask) = check_extensions(&mut reader, vertex_count)?;
    Ok(MeshFacts { vertex_count, triangle_count, has_vertex_normals, has_water_mask })
}

/// The header's twelve numbers, in order, widened to f64: three doubles, two floats, seven doubles.
fn header_values(header: &[u8]) -> Vec<f64> {
    let double_at = |offset: usize| {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&header[offset..offset + 8]);
        f64::from_le_bytes(bytes)
    };
    let float_at = |offset: usize| {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&header[offset..offset + 4]);
        f64::from(f32::from_le_bytes(bytes))
    };
    let mut values: Vec<f64> = (0..3).map(|index| double_at(index * 8)).collect();
    values.push(float_at(24));
    values.push(float_at(28));
    values.extend((0..7).map(|index| double_at(32 + index * 8)));
    values
}

fn check_header(header: &[u8]) -> Result<(), MeshError> {
    let values = header_values(header);
    if values.iter().any(|value| !value.is_finite()) {
        return Err(MeshError::HeaderInvalid("every header number must be finite".to_string()));
    }
    let (minimum_height, maximum_height, bounding_radius) = (values[3], values[4], values[8]);
    if minimum_height > maximum_height {
        return Err(MeshError::HeaderInvalid(format!(
            "minimum height {minimum_height} is above maximum height {maximum_height}"
        )));
    }
    if bounding_radius < 0.0 {
        return Err(MeshError::HeaderInvalid(format!("bounding sphere radius {bounding_radius} is negative")));
    }
    Ok(())
}

fn zigzag_decode(value: u16) -> i32 {
    let value = i32::from(value);
    (value >> 1) ^ -(value & 1)
}

/// Decode one delta-and-zigzag coded array and check every coordinate is in range.
fn check_coordinates(coded: &[u8], name: &str) -> Result<(), MeshError> {
    let mut coordinate = 0i32;
    for pair in coded.chunks_exact(2) {
        coordinate += zigzag_decode(u16::from_le_bytes([pair[0], pair[1]]));
        if !(0..=COORDINATE_MAX).contains(&coordinate) {
            return Err(MeshError::VertexData(format!(
                "a decoded {name} value is {coordinate}; values run 0 to {COORDINATE_MAX}"
            )));
        }
    }
    Ok(())
}

fn check_vertex_data(reader: &mut ByteReader<'_>) -> Result<u32, MeshError> {
    let vertex_count = reader.read_u32("vertex count")?;
    if vertex_count < 3 {
        return Err(MeshError::VertexData(format!(
            "the tile has {vertex_count} vertices; a mesh needs at least 3"
        )));
    }
    let array_bytes = (vertex_count as usize).saturating_mul(2);
    for name in ["u", "v", "height"] {
        check_coordinates(reader.take(array_bytes, "vertex arrays")?, name)?;
    }
    Ok(vertex_count)
}

/// Wide (32-bit) indices start on a 4-byte boundary; the padding before them must be zero.
fn check_index_alignment(reader: &mut ByteReader<'_>, wide: bool) -> Result<(), MeshError> {
    let padding = if wide { (4 - reader.position % 4) % 4 } else { 0 };
    let padding_bytes = reader.take(padding, "index alignment padding")?;
    if padding_bytes.iter().any(|byte| *byte != 0) {
        return Err(MeshError::IndexData("the alignment padding before the indices is not zero".to_string()));
    }
    Ok(())
}

/// Decode high-water-mark coded indices, checking each names an existing vertex.
fn check_high_water_mark(codes: &[u32], vertex_count: u32) -> Result<(), MeshError> {
    let mut highest = 0u32;
    for code in codes {
        let index = highest.checked_sub(*code).ok_or_else(|| {
            MeshError::IndexData(format!("code {code} is above the high-water mark {highest}"))
        })?;
        if index >= vertex_count {
            return Err(MeshError::IndexData(format!(
                "index {index} names a vertex, but the tile has only {vertex_count}"
            )));
        }
        if *code == 0 {
            highest += 1;
        }
    }
    Ok(())
}

fn check_triangles(reader: &mut ByteReader<'_>, vertex_count: u32, wide: bool) -> Result<u32, MeshError> {
    let triangle_count = reader.read_u32("triangle count")?;
    if triangle_count == 0 {
        return Err(MeshError::IndexData("the tile has no triangles".to_string()));
    }
    let index_count = (triangle_count as usize).saturating_mul(3);
    let codes = reader.read_indices(index_count, wide, "triangle indices")?;
    check_high_water_mark(&codes, vertex_count)?;
    Ok(triangle_count)
}

fn check_edge(reader: &mut ByteReader<'_>, vertex_count: u32, wide: bool, edge: &'static str) -> Result<(), MeshError> {
    let edge_count = reader.read_u32("edge vertex count")?;
    if edge_count > vertex_count {
        return Err(MeshError::EdgeData {
            edge,
            reason: format!("it lists {edge_count} vertices, but the tile has only {vertex_count}"),
        });
    }
    let indices = reader.read_indices(edge_count as usize, wide, "edge indices")?;
    match indices.iter().find(|index| **index >= vertex_count) {
        Some(index) => Err(MeshError::EdgeData {
            edge,
            reason: format!("index {index} names a vertex, but the tile has only {vertex_count}"),
        }),
        None => Ok(()),
    }
}

fn check_extension_length(extension_id: u8, length: usize, vertex_count: u32) -> Result<(), MeshError> {
    let normals_bytes = (vertex_count as usize).saturating_mul(2);
    match extension_id {
        EXTENSION_VERTEX_NORMALS if length == normals_bytes => Ok(()),
        EXTENSION_VERTEX_NORMALS => Err(MeshError::Extension(format!(
            "vertex normals hold {length} bytes; {vertex_count} vertices need exactly {normals_bytes}"
        ))),
        EXTENSION_WATER_MASK if length == 1 || length == WATER_MASK_GRID_BYTES => Ok(()),
        EXTENSION_WATER_MASK => Err(MeshError::Extension(format!(
            "the water mask holds {length} bytes; it is either 1 byte or {WATER_MASK_GRID_BYTES}"
        ))),
        other => Err(MeshError::Extension(format!(
            "extension id {other}; only vertex normals (1) and the water mask (2) may be written"
        ))),
    }
}

/// Read extensions to the end of the data. Returns (has vertex normals, has water mask).
fn check_extensions(reader: &mut ByteReader<'_>, vertex_count: u32) -> Result<(bool, bool), MeshError> {
    let (mut has_vertex_normals, mut has_water_mask) = (false, false);
    while reader.remaining() > 0 {
        let extension_id = reader.take(1, "extension header")?[0];
        let length = reader.read_u32("extension header")? as usize;
        check_extension_length(extension_id, length, vertex_count)?;
        reader.take(length, "extension data")?;
        let seen = if extension_id == EXTENSION_VERTEX_NORMALS { &mut has_vertex_normals } else { &mut has_water_mask };
        if *seen {
            return Err(MeshError::Extension(format!("extension id {extension_id} appears twice")));
        }
        *seen = true;
    }
    Ok((has_vertex_normals, has_water_mask))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header: centre, min and max height, bounding sphere, horizon occlusion point.
    fn header(minimum_height: f32, maximum_height: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&minimum_height.to_le_bytes());
        bytes.extend_from_slice(&maximum_height.to_le_bytes());
        for value in [1.0f64, 2.0, 3.0, 10.0, 4.0, 5.0, 6.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn zigzag_encode(delta: i32) -> u16 {
        ((delta << 1) ^ (delta >> 31)) as u16
    }

    fn coded(values: &[i32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut previous = 0;
        for value in values {
            bytes.extend_from_slice(&zigzag_encode(value - previous).to_le_bytes());
            previous = *value;
        }
        bytes
    }

    fn list_u16(values: &[u16]) -> Vec<u8> {
        let mut bytes = (values.len() as u32).to_le_bytes().to_vec();
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// A flat square tile: four corner vertices, two triangles, one vertex pair per edge.
    fn square_tile() -> Vec<u8> {
        let mut bytes = header(0.0, 100.0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend(coded(&[0, 32767, 0, 32767]));
        bytes.extend(coded(&[0, 0, 32767, 32767]));
        bytes.extend(coded(&[0, 0, 0, 0]));
        // Triangles (0,1,2) and (1,3,2), high-water-mark coded.
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for code in [0u16, 0, 0, 2, 0, 2] {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        bytes.extend(list_u16(&[0, 2]));
        bytes.extend(list_u16(&[0, 1]));
        bytes.extend(list_u16(&[1, 3]));
        bytes.extend(list_u16(&[2, 3]));
        bytes
    }

    fn extension(extension_id: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![extension_id];
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    const SQUARE: MeshFacts =
        MeshFacts { vertex_count: 4, triangle_count: 2, has_vertex_normals: false, has_water_mask: false };

    // -- accepted --

    #[test]
    fn plain_tile_passes() {
        assert_eq!(verify_quantized_mesh(&square_tile()), Ok(SQUARE));
    }

    #[test]
    fn tile_with_vertex_normals_passes() {
        let mut bytes = square_tile();
        bytes.extend(extension(EXTENSION_VERTEX_NORMALS, &[0u8; 8]));
        assert_eq!(verify_quantized_mesh(&bytes), Ok(MeshFacts { has_vertex_normals: true, ..SQUARE }));
    }

    #[test]
    fn tile_with_normals_and_single_byte_water_mask_passes() {
        let mut bytes = square_tile();
        bytes.extend(extension(EXTENSION_VERTEX_NORMALS, &[0u8; 8]));
        bytes.extend(extension(EXTENSION_WATER_MASK, &[255]));
        assert_eq!(
            verify_quantized_mesh(&bytes),
            Ok(MeshFacts { has_vertex_normals: true, has_water_mask: true, ..SQUARE })
        );
    }

    // -- refused: not a whole tile --

    #[test]
    fn other_bytes_are_refused() {
        assert!(matches!(
            verify_quantized_mesh(b"plain text, not a terrain tile"),
            Err(MeshError::Truncated { section: "header", .. })
        ));
    }

    #[test]
    fn cut_short_tile_is_refused() {
        let whole = square_tile();
        assert!(matches!(
            verify_quantized_mesh(&whole[..whole.len() - 3]),
            Err(MeshError::Truncated { .. })
        ));
    }

    #[test]
    fn inverted_height_range_is_refused() {
        let mut bytes = square_tile();
        bytes.splice(..HEADER_BYTES, header(100.0, 0.0));
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::HeaderInvalid(_))));
    }

    #[test]
    fn coordinate_out_of_range_is_refused() {
        let mut bytes = header(0.0, 100.0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend(coded(&[32767, 32800, 32800, 32800]));
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::VertexData(_))));
    }

    #[test]
    fn triangle_naming_a_missing_vertex_is_refused() {
        // Four zero codes introduce vertices 0..=3; a fifth names vertex 4, which a 4-vertex tile lacks.
        let mut bytes = header(0.0, 100.0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        for _ in 0..3 {
            bytes.extend(coded(&[0, 0, 0, 0]));
        }
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for code in [0u16, 0, 0, 0, 0, 1] {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::IndexData(_))));
    }

    #[test]
    fn edge_naming_a_missing_vertex_is_refused() {
        let mut bytes = square_tile();
        let last_edge_index = bytes.len() - 2;
        bytes[last_edge_index] = 9;
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::EdgeData { edge: "north", .. })));
    }

    // -- refused: carrying something beyond the mesh --

    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = square_tile();
        bytes.extend_from_slice(b"appended payload");
        assert!(verify_quantized_mesh(&bytes).is_err());
    }

    #[test]
    fn metadata_extension_is_refused() {
        let mut bytes = square_tile();
        let json = br#"{"anything":"at all"}"#;
        let mut body = (json.len() as u32).to_le_bytes().to_vec();
        body.extend_from_slice(json);
        bytes.extend(extension(4, &body));
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::Extension(_))));
    }

    #[test]
    fn vertex_normals_of_the_wrong_length_are_refused() {
        let mut bytes = square_tile();
        bytes.extend(extension(EXTENSION_VERTEX_NORMALS, &[0u8; 64]));
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::Extension(_))));
    }

    #[test]
    fn failure_classes_separate_malformed_from_not_accepted() {
        let truncated = MeshError::Truncated { section: "header", needed: 88, remaining: 3 };
        assert_eq!(truncated.error_type(), "parse_error");
        assert_eq!(MeshError::Extension("metadata".to_string()).error_type(), "verification_error");
    }

    #[test]
    fn repeated_extension_is_refused() {
        let mut bytes = square_tile();
        bytes.extend(extension(EXTENSION_VERTEX_NORMALS, &[0u8; 8]));
        bytes.extend(extension(EXTENSION_VERTEX_NORMALS, &[0u8; 8]));
        assert!(matches!(verify_quantized_mesh(&bytes), Err(MeshError::Extension(_))));
    }
}
