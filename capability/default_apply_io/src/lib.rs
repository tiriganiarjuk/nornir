//! Read TOML, apply conditional defaults, validate, write TOML.
//!
//! Composes `default_apply_core` (pure logic) with the one read and write engines:
//! `reader_io` reads and converts the file, the defaults are applied, and
//! `text_writer_io` validates against the embedded schema and writes the result.
//! A failure is the engines' own, as a `gate_io::GateError`.

use gate_io::GateError;
use reader_io::{ReaderError, SchemaSource, SourceFormat};
use schema_core::EmbeddedValidator;
use text_writer_io::TextFormat;

/// Read raw TOML, apply conditional defaults, validate, write clean TOML.
/// Returns the modified JSON string for downstream use.
pub fn read_apply_validate_write(
    schema: &EmbeddedValidator,
    input_path: &str,
    output_path: &str,
) -> Result<String, GateError> {
    let (_, mut data) = reader_io::read_converted(SourceFormat::Toml, input_path)?;
    let unusable = |message: String| ReaderError::EmbeddedSchemaInvalid { name: schema.schema_name().to_string(), message };

    let schema_value: serde_json::Value =
        serde_json::from_str(schema.schema_json()).map_err(|json_err| unusable(json_err.to_string()))?;
    default_apply_core::apply_conditional_defaults(&schema_value, &mut data);

    let modified_json = serde_json::to_string(&data).map_err(|json_err| ReaderError::ConvertedJsonInvalid {
        format: "JSON",
        message: json_err.to_string(),
    })?;

    let embedded = SchemaSource::Embedded { validator: schema, verify_paths: false };
    text_writer_io::validate_write_with(TextFormat::Toml, &modified_json, output_path, &embedded)?;
    Ok(modified_json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    static SCHEMA: EmbeddedValidator = EmbeddedValidator::new(
        r#"{"type": "object", "required": ["name", "mode"], "properties": {"name": {"type": "string"}, "mode": {"type": "string", "default": "plain"}}}"#,
        "test-defaults",
    );

    #[test]
    fn defaults_are_applied_before_validation_and_written() {
        let dir = std::env::temp_dir().join(format!("nornir_default_apply_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("raw.toml");
        let output = dir.join("out").join("defaulted.toml");
        fs::write(&input, "name = \"agent\"\n").unwrap();

        let json = read_apply_validate_write(&SCHEMA, input.to_str().unwrap(), output.to_str().unwrap()).unwrap();
        let data: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(data["mode"], "plain");
        assert!(fs::read_to_string(&output).unwrap().contains("mode = \"plain\""));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_input_is_a_read_error() {
        let error = read_apply_validate_write(&SCHEMA, "/absolutely/not/here.toml", "/tmp/unused.toml").unwrap_err();
        assert_eq!(error.error_type(), "read_error");
    }
}
