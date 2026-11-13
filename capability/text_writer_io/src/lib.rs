//! Shared text-writer IO — validate structured text against a schema, then write it.
//!
//! The writing sibling of `reader_io`. A text writer takes JSON text a Python
//! stage already holds, validates it against a JSON Schema loaded BY NAME from a
//! directory at call time — the same schema loading and cache the readers use —
//! converts it to the file's format through `format_core`, and only then writes
//! the file, atomically. Each `text_writers/write_json_validate_{format}` crate is
//! a thin PyO3 wrapper over `validate_write` with its format fixed.
//!
//! Nothing is written without a schema: text that is not JSON, a schema that
//! cannot be read or compiled, data that does not match it, and data the format
//! cannot express each stop the write before the disk is touched.
//!
//! Flow: parse → load+compile schema (cached per process) → validate → convert
//! → create the parent directory → write temp, fsync, rename; or a typed error.

use std::fs;
use std::path::Path;

use serde_json::Value;

use format_core::serialize::{to_markdown_document, to_toml, to_toon, to_xml, to_yaml};
use reader_io::{validate_against_named_schema, ReaderError};

/// Which file format a text writer writes — the writing twin of `reader_io::SourceFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    /// The JSON text exactly as given.
    Json,
    Toml,
    Yaml,
    Toon,
    /// The `{"@attr": …, "#text": …}` convention, under one root key.
    Xml,
    /// The envelope `{"frontmatter": {...}, "body": "..."}` the markdown reader returns.
    Markdown,
}

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

    /// Valid, schema-conforming data the file's format cannot express — a null in TOML,
    /// an XML document without its one root key, a markdown envelope without its parts.
    #[error("Refused to write '{path}': {message}")]
    NotExpressible { path: String, message: String },

    #[error("Cannot write '{path}': {message}")]
    WriteFailed { path: String, message: String },
}

impl TextWriteError {
    /// The failure class a caller branches on — the `error.type` of the writer result.
    pub fn error_type(&self) -> &'static str {
        match self {
            TextWriteError::NotJson { .. } => "parse_error",
            TextWriteError::SchemaRefused { source, .. } => source.error_type(),
            TextWriteError::NotExpressible { .. } => "conversion_error",
            TextWriteError::WriteFailed { .. } => "write_error",
        }
    }
}

/// The text to write: the JSON exactly as given, or the data in the file's format.
fn rendered(format: TextFormat, json_data: &str, data: &Value) -> Result<String, String> {
    let converted = match format {
        TextFormat::Json => return Ok(json_data.to_string()),
        TextFormat::Toml => to_toml(data),
        TextFormat::Yaml => to_yaml(data),
        TextFormat::Toon => to_toon(data),
        TextFormat::Xml => to_xml(data),
        TextFormat::Markdown => to_markdown_document(data),
    };
    converted.map_err(|format_err| format_err.to_string())
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

/// Validate `json_data` against `{schema_dir}/{schema_name}.schema.json`, convert
/// it to `format`, then write it to `file_path`, with a final newline.
pub fn validate_write(
    format: TextFormat,
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
    let text = rendered(format, json_data, &data)
        .map_err(|message| TextWriteError::NotExpressible { path: file_path.to_string(), message })?;
    write_validated_text(file_path, &text)
}

/// Validate `json_data` against `{schema_dir}/{schema_name}.schema.json`, then
/// write it to `file_path` exactly as given, with a final newline.
pub fn validate_write_json(
    json_data: &str,
    file_path: &str,
    schema_name: &str,
    schema_dir: &str,
) -> Result<(), TextWriteError> {
    validate_write(TextFormat::Json, json_data, file_path, schema_name, schema_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_io::{read_validate_json, SourceFormat};
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

    // -- every format reads back, through its reader, as the data written --

    fn read_back(format: SourceFormat, file: &Path, schema_name: &str, dir: &Path) -> Value {
        let json = read_validate_json(format, file.to_str().unwrap(), schema_name, dir_str(dir)).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn toml_yaml_and_toon_read_back_as_written() {
        let dir = temp_dir("round_trip");
        write_schema(&dir, "round_trip", NAME_AGE_SCHEMA);
        let data = r#"{"name": "x", "age": 3}"#;
        let expected: Value = serde_json::from_str(data).unwrap();
        for (text_format, source_format, file_name) in [
            (TextFormat::Toml, SourceFormat::Toml, "out.toml"),
            (TextFormat::Yaml, SourceFormat::Yaml, "out.yaml"),
            (TextFormat::Toon, SourceFormat::Toon, "out.toon"),
        ] {
            let file = dir.join(file_name);
            validate_write(text_format, data, file.to_str().unwrap(), "round_trip", dir_str(&dir)).unwrap();
            assert_eq!(read_back(source_format, &file, "round_trip", &dir), expected, "{file_name}");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xml_reads_back_as_written() {
        let dir = temp_dir("xml_round_trip");
        write_schema(&dir, "xml_round_trip", r#"{"type": "object", "required": ["root"]}"#);
        let data = r#"{"root": {"name": "x"}}"#;
        let file = dir.join("out.xml");
        validate_write(TextFormat::Xml, data, file.to_str().unwrap(), "xml_round_trip", dir_str(&dir)).unwrap();
        let expected: Value = serde_json::from_str(data).unwrap();
        assert_eq!(read_back(SourceFormat::Xml, &file, "xml_round_trip", &dir), expected);
        fs::remove_dir_all(&dir).ok();
    }

    const ENVELOPE_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["frontmatter", "body"],
        "properties": {"frontmatter": {"type": "object"}, "body": {"type": "string"}}
    }"#;

    #[test]
    fn markdown_writes_fenced_frontmatter_in_key_order_and_reads_back() {
        let dir = temp_dir("markdown");
        write_schema(&dir, "markdown", ENVELOPE_SCHEMA);
        let data = r#"{"frontmatter": {"title": "A page", "tags": ["B", "A"]}, "body": "\nThe body.\n"}"#;
        let file = dir.join("page.md");
        validate_write(TextFormat::Markdown, data, file.to_str().unwrap(), "markdown", dir_str(&dir)).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "---\ntitle: A page\ntags:\n- B\n- A\n---\n\nThe body.\n");
        let expected: Value = serde_json::from_str(data).unwrap();
        assert_eq!(read_back(SourceFormat::Markdown, &file, "markdown", &dir), expected);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn markdown_with_empty_frontmatter_is_the_body_alone() {
        let dir = temp_dir("markdown_bare");
        write_schema(&dir, "markdown_bare", ENVELOPE_SCHEMA);
        let file = dir.join("bare.md");
        let data = r#"{"frontmatter": {}, "body": "Only a body.\n"}"#;
        validate_write(TextFormat::Markdown, data, file.to_str().unwrap(), "markdown_bare", dir_str(&dir)).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "Only a body.\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn markdown_body_that_opens_with_a_fence_reads_back_as_body() {
        let dir = temp_dir("markdown_fence");
        write_schema(&dir, "markdown_fence", ENVELOPE_SCHEMA);
        let file = dir.join("fence.md");
        let data = r#"{"frontmatter": {}, "body": "---\nnot frontmatter\n---\nrest\n"}"#;
        validate_write(TextFormat::Markdown, data, file.to_str().unwrap(), "markdown_fence", dir_str(&dir)).unwrap();
        let expected: Value = serde_json::from_str(data).unwrap();
        assert_eq!(read_back(SourceFormat::Markdown, &file, "markdown_fence", &dir), expected);
        fs::remove_dir_all(&dir).ok();
    }

    // -- data the format cannot express writes nothing --

    #[test]
    fn data_the_format_cannot_express_is_a_conversion_error() {
        let dir = temp_dir("not_expressible");
        write_schema(&dir, "not_expressible", r#"{"type": "object"}"#);
        for (text_format, data, file_name) in [
            (TextFormat::Toml, r#"{"name": null}"#, "out.toml"),
            (TextFormat::Xml, r#"{"first": 1, "second": 2}"#, "out.xml"),
            (TextFormat::Markdown, r#"{"body": "no frontmatter"}"#, "out.md"),
        ] {
            let file = dir.join(file_name);
            let error = validate_write(text_format, data, file.to_str().unwrap(), "not_expressible", dir_str(&dir))
                .unwrap_err();
            assert_eq!(error.error_type(), "conversion_error", "{file_name}");
            assert!(!file.exists(), "{file_name}");
        }
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
