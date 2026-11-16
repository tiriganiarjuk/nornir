//! Reader: read_markdown_validate_json
//! Reads a Markdown file with optional YAML frontmatter, validates the envelope
//! `{"frontmatter": {...}, "body": "..."}` against `{schema_dir}/{schema_name}.schema.json`
//! (loaded at call time, compiled once per process), and returns the validated JSON.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `read` is the tool;
//! the CLI face (`main.rs`) and the PyO3 face (below, feature "python") hand its result
//! to `tool_face_io`.

pub use reader_io::ReaderError;

pub const USAGE: &str = "usage: read_markdown_validate_json <file_path> <schema_name> <schema_dir>\n\
                         Pass the three arguments in that order; schema_dir is an absolute path.";

/// Read a Markdown file and validate its {frontmatter, body} envelope against the named schema. Pass an absolute schema_dir.
pub fn read(file_path: &str, schema_name: &str, schema_dir: &str) -> Result<Option<String>, ReaderError> {
    reader_io::read_validate_json(reader_io::SourceFormat::Markdown, file_path, schema_name, schema_dir).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn read(py: Python<'_>, file_path: &str, schema_name: &str, schema_dir: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::read(file_path, schema_name, schema_dir), super::ReaderError::error_type)
    }

    #[pymodule]
    fn read_markdown_validate_json(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(read, m)?)?;
        Ok(())
    }
}
