//! The gate patterns, as constraints over the one read and write engines.
//!
//! A gate is a generated variant (`HOW_A_NORNIR_TOOL_IS_MADE.md`): the engine with
//! its schema embedded in the binary and its format fixed to TOML. Reading,
//! converting and validating is `reader_io`; validating, converting and writing is
//! `text_writer_io`. This crate only fixes those choices:
//! - Input gates: read TOML file → validate → return JSON
//! - Output gates: validate JSON → write TOML file
//! - Passthrough gates: read TOML → validate → verify paths → write TOML

use reader_io::{ReaderError, SchemaSource, SourceFormat};
use schema_core::EmbeddedValidator;
use text_writer_io::{TextFormat, TextWriteError};

/// Every failure a gate can produce: the read engine's or the write engine's, as it is.
#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error(transparent)]
    Read(#[from] ReaderError),
    #[error(transparent)]
    Write(#[from] TextWriteError),
}

impl GateError {
    /// The failure class a caller branches on — the `error.type` of the gate result.
    pub fn error_type(&self) -> &'static str {
        match self {
            GateError::Read(error) => error.error_type(),
            GateError::Write(error) => error.error_type(),
        }
    }
}

fn embedded(schema: &EmbeddedValidator, verify_paths: bool) -> SchemaSource<'_> {
    SchemaSource::Embedded { validator: schema, verify_paths }
}

/// Input gate: read TOML from disk, validate against the embedded schema, return JSON.
pub fn read_and_validate(schema: &EmbeddedValidator, path: &str, verify_paths: bool) -> Result<String, GateError> {
    Ok(reader_io::read_validate(SourceFormat::Toml, path, &embedded(schema, verify_paths))?)
}

/// Output gate: validate JSON against the embedded schema, convert to TOML, write to disk.
pub fn validate_and_write(schema: &EmbeddedValidator, data: &str, path: &str, verify_paths: bool) -> Result<(), GateError> {
    Ok(text_writer_io::validate_write_with(TextFormat::Toml, data, path, &embedded(schema, verify_paths))?)
}

/// Passthrough gate: read TOML, validate, verify paths, write TOML.
pub fn read_validate_write(schema: &EmbeddedValidator, input_path: &str, output_path: &str) -> Result<(), GateError> {
    let json = read_and_validate(schema, input_path, true)?;
    validate_and_write(schema, &json, output_path, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    static SCHEMA: EmbeddedValidator = EmbeddedValidator::new(
        r#"{"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}, "source": {"type": "string", "format": "path_exists_absolute"}}}"#,
        "test-gate",
    );

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_gate_io_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_missing_input_file_is_a_read_error() {
        let error = read_and_validate(&SCHEMA, "/absolutely/does/not/exist.toml", false).unwrap_err();
        assert_eq!(error.error_type(), "read_error");
        assert!(error.to_string().contains("/absolutely/does/not/exist.toml"));
    }

    #[test]
    fn data_that_does_not_match_the_schema_is_a_validation_error() {
        let dir = scratch("invalid");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("in.toml");
        fs::write(&file, "other = 1\n").unwrap();
        let error = read_and_validate(&SCHEMA, file.to_str().unwrap(), false).unwrap_err();
        assert_eq!(error.error_type(), "validation_error");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_named_path_is_a_path_error() {
        let dir = scratch("paths");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("in.toml");
        fs::write(&file, "name = \"x\"\nsource = \"/absolutely/does/not/exist.txt\"\n").unwrap();
        let error = read_and_validate(&SCHEMA, file.to_str().unwrap(), true).unwrap_err();
        assert_eq!(error.error_type(), "path_error");
        assert!(read_and_validate(&SCHEMA, file.to_str().unwrap(), false).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_output_gate_creates_the_parent_and_ends_with_a_newline() {
        let dir = scratch("write");
        let file = dir.join("sub").join("out.toml");
        validate_and_write(&SCHEMA, r#"{"name": "test"}"#, file.to_str().unwrap(), false).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "name = \"test\"\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_null_the_output_gate_cannot_write_is_a_conversion_error_naming_it() {
        let dir = scratch("null");
        let file = dir.join("out.toml");
        let error = validate_and_write(&SCHEMA, r#"{"name": "test", "note": null}"#, file.to_str().unwrap(), false).unwrap_err();
        assert_eq!(error.error_type(), "conversion_error");
        assert!(error.to_string().contains("note"));
        assert!(!file.exists());
    }

    #[test]
    fn the_passthrough_gate_reads_validates_and_writes() {
        let dir = scratch("passthrough");
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.toml");
        let output = dir.join("out").join("out.toml");
        fs::write(&input, "name = \"carried\"\n").unwrap();
        read_validate_write(&SCHEMA, input.to_str().unwrap(), output.to_str().unwrap()).unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "name = \"carried\"\n");
        fs::remove_dir_all(&dir).ok();
    }
}
