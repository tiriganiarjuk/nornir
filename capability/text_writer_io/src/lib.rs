//! Shared text-writer IO — validate structured text against a schema, then write it.
//!
//! The writing sibling of `reader_io`. A text writer takes JSON text a Python
//! stage already holds, validates it against a JSON Schema loaded BY NAME from a
//! directory at call time — the same schema loading and cache the readers use —
//! and only then writes the file, atomically. Each
//! `text_writers/write_json_validate_{format}` crate is a thin PyO3 wrapper over
//! one function here.
//!
//! Nothing is written without a schema: text that is not JSON, a schema that
//! cannot be read or compiled, and data that does not match it each stop the
//! write before the disk is touched.
//!
//! Flow: parse → load+compile schema (cached per process) → validate → create
//! the parent directory → write temp, fsync, rename; or a typed error.

use std::fs;
use std::path::Path;

use serde_json::Value;

use reader_io::{validate_against_named_schema, ReaderError};

/// Every failure a text writer can produce. `error_type()` is the `error.type`
/// a writer returns to Python; the Display string is the `error.message`.
#[derive(Debug, thiserror::Error)]
pub enum TextWriteError {
    #[error("Refused to write '{path}': the text is not valid JSON: {message}")]
    NotJson { path: String, message: String },

    /// The schema could not be loaded, or the data does not match it. Carries
    /// the readers' error, so both directions report these failures alike.
    #[error("Refused to write '{path}': {source}")]
    SchemaRefused { path: String, source: ReaderError },

    #[error("Cannot write '{path}': {message}")]
    WriteFailed { path: String, message: String },
}

impl TextWriteError {
    /// The failure class a caller branches on — the `error.type` of the writer result.
    pub fn error_type(&self) -> &'static str {
        match self {
            TextWriteError::NotJson { .. } => "parse_error",
            TextWriteError::SchemaRefused { source, .. } => source.error_type(),
            TextWriteError::WriteFailed { .. } => "write_error",
        }
    }
}

fn with_final_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

fn write_validated_text(file_path: &str, text: &str) -> Result<(), TextWriteError> {
    let write_failed = |message: String| TextWriteError::WriteFailed { path: file_path.to_string(), message };
    let path = Path::new(file_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|io_err| write_failed(format!("create directory: {io_err}")))?;
    }
    write_engine::write_file_atomic(path, &with_final_newline(text)).map_err(write_failed)
}

/// Validate `json_data` against `{schema_dir}/{schema_name}.schema.json`, then
/// write it to `file_path` exactly as given, with a final newline.
pub fn validate_write_json(
    json_data: &str,
    file_path: &str,
    schema_name: &str,
    schema_dir: &str,
) -> Result<(), TextWriteError> {
    let data: Value = serde_json::from_str(json_data).map_err(|json_err| TextWriteError::NotJson {
        path: file_path.to_string(),
        message: json_err.to_string(),
    })?;
    validate_against_named_schema(&data, schema_name, schema_dir)
        .map_err(|source| TextWriteError::SchemaRefused { path: file_path.to_string(), source })?;
    write_validated_text(file_path, json_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NAME_AGE_SCHEMA: &str = r#"{
        "type": "object",
        "additionalProperties": false,
        "required": ["name"],
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"}
        }
    }"#;

    /// Fresh temp dir per test. The validator cache is process-global and keyed
    /// by schema path, so every test uses its own directory AND schema name.
    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_text_writer_io_{}", test_name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_schema(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(format!("{}.schema.json", name)), body).unwrap();
    }

    fn dir_str(dir: &Path) -> &str {
        dir.to_str().unwrap()
    }

    // -- written --

    #[test]
    fn valid_json_is_written_as_given_into_a_new_directory() {
        let dir = temp_dir("valid");
        write_schema(&dir, "valid", NAME_AGE_SCHEMA);
        let file = dir.join("terrain/layer.json");
        let text = "{\n  \"name\": \"x\",\n  \"age\": 3\n}";
        validate_write_json(text, file.to_str().unwrap(), "valid", dir_str(&dir)).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), format!("{text}\n"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn existing_file_is_replaced() {
        let dir = temp_dir("replace");
        write_schema(&dir, "replace", NAME_AGE_SCHEMA);
        let file = dir.join("out.json");
        fs::write(&file, "old content").unwrap();
        validate_write_json(r#"{"name": "new"}"#, file.to_str().unwrap(), "replace", dir_str(&dir)).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"name\": \"new\"}\n");
        fs::remove_dir_all(&dir).ok();
    }

    // -- nothing written without a schema --

    #[test]
    fn missing_schema_writes_nothing() {
        let dir = temp_dir("no_schema");
        let file = dir.join("out.json");
        let error = validate_write_json(r#"{"name": "x"}"#, file.to_str().unwrap(), "absent", dir_str(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "schema_error");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_that_is_not_json_writes_nothing() {
        let dir = temp_dir("schema_not_json");
        write_schema(&dir, "schema_not_json", "not json at all");
        let file = dir.join("out.json");
        let error =
            validate_write_json(r#"{"name": "x"}"#, file.to_str().unwrap(), "schema_not_json", dir_str(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "schema_error");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_that_is_not_a_json_schema_writes_nothing() {
        let dir = temp_dir("schema_bad");
        write_schema(&dir, "schema_bad", r#"{"type": 42}"#);
        let file = dir.join("out.json");
        let error = validate_write_json(r#"{"name": "x"}"#, file.to_str().unwrap(), "schema_bad", dir_str(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "schema_error");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    // -- refused --

    #[test]
    fn data_that_does_not_match_the_schema_writes_nothing() {
        let dir = temp_dir("mismatch");
        write_schema(&dir, "mismatch", NAME_AGE_SCHEMA);
        let file = dir.join("out.json");
        let error =
            validate_write_json(r#"{"name": 7, "extra": true}"#, file.to_str().unwrap(), "mismatch", dir_str(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "validation_error");
        assert!(error.to_string().starts_with("Refused to write"), "{error}");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn text_that_is_not_json_writes_nothing() {
        let dir = temp_dir("not_json");
        write_schema(&dir, "not_json", NAME_AGE_SCHEMA);
        let file = dir.join("out.json");
        let error = validate_write_json("name = \"x\"", file.to_str().unwrap(), "not_json", dir_str(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "parse_error");
        assert!(!file.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refused_data_leaves_an_existing_file_untouched() {
        let dir = temp_dir("untouched");
        write_schema(&dir, "untouched", NAME_AGE_SCHEMA);
        let file = dir.join("out.json");
        fs::write(&file, "kept").unwrap();
        let result = validate_write_json(r#"{"age": 3}"#, file.to_str().unwrap(), "untouched", dir_str(&dir));
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "kept");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unwritable_destination_is_a_write_error() {
        let dir = temp_dir("unwritable");
        write_schema(&dir, "unwritable", NAME_AGE_SCHEMA);
        let blocker = dir.join("blocker");
        fs::write(&blocker, "a file where a directory is needed").unwrap();
        let file = blocker.join("out.json");
        let error = validate_write_json(r#"{"name": "x"}"#, file.to_str().unwrap(), "unwritable", dir_str(&dir)).unwrap_err();
        assert_eq!(error.error_type(), "write_error");
        fs::remove_dir_all(&dir).ok();
    }
}
