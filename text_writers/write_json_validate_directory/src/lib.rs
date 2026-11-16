//! Text writer: write_json_validate_directory
//! Replaces a generated directory whole. Takes a request, `{"files": [...]}` per
//! `schemas/tools/directory-write-request.schema.json`: each text entry is validated
//! against its named schema and written in its format, each copy entry is an existing
//! file's bytes. Everything is checked before anything is written; the new directory is
//! built beside the old and swapped in; a byte-identical file is kept with its time. A
//! folder without the `.generated_directory.json` marker is refused.
//! Its data is the run's accounting, `{"written": int, "unchanged": int, "removed": int}`,
//! validated against `schemas/tools/directory-write-report.schema.json` — the directory
//! writer's approved extension of the contract.
//!
//! One tool, two faces (`NORNIR_CONVENTIONS.md` § Tool Structure). `write` is the tool;
//! the CLI face (`main.rs`, the request on stdin) and the PyO3 face (below, feature
//! "python") hand its result to `tool_face_io`.

pub use directory_write_io::DirectoryWriteError;

pub const USAGE: &str = "usage: write_json_validate_directory <directory_root> <schema_dir>\n\
                         Pipe the request, {\"files\": [...]}, on stdin; schema_dir is an absolute path.";

/// Replace the directory at directory_root with exactly the files the request lists. Pass an absolute schema_dir.
pub fn write(directory_root: &str, request_json: &str, schema_dir: &str) -> Result<Option<String>, DirectoryWriteError> {
    directory_write_io::write_directory_json(directory_root, request_json, schema_dir).map(Some)
}

#[cfg(feature = "python")]
mod python_face {
    use pyo3::prelude::*;

    #[pyfunction]
    fn write(py: Python<'_>, directory_root: &str, request_json: &str, schema_dir: &str) -> PyResult<PyObject> {
        tool_face_io::py_contract(py, super::write(directory_root, request_json, schema_dir), super::DirectoryWriteError::error_type)
    }

    #[pymodule]
    fn write_json_validate_directory(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(write, m)?)?;
        Ok(())
    }
}
