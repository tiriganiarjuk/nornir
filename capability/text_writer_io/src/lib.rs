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

use format_core::convert::json_to_toml;
use format_core::serialize::{to_markdown_document, to_toon, to_xml, to_yaml};
use failure_mode_core::Failure;
use reader_io::{validate_against, ReaderError, SchemaSource};

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

impl TextFormat {
    /// The format a name denotes — the `{format}` slot of `write_json_validate_{format}`.
    pub fn from_name(name: &str) -> Option<TextFormat> {
        match name {
            "json" => Some(TextFormat::Json),
            "toml" => Some(TextFormat::Toml),
            "yaml" => Some(TextFormat::Yaml),
            "toon" => Some(TextFormat::Toon),
            "xml" => Some(TextFormat::Xml),
            "markdown" => Some(TextFormat::Markdown),
            _ => None,
        }
    }
}

/// Every failure a text writer can produce. Its class and message come from the error
/// registry (`failure_mode_core`): `error_type()` is the `error.type` a writer returns,
/// the Display string is the `error.message`.
#[derive(Debug)]
pub enum TextWriteError {
    NotJson { path: String, message: String },
    /// The schema could not be loaded, or the data does not match it. Carries the
    /// readers' error, so both directions report these failures alike.
    SchemaRefused { path: String, source: ReaderError },
    /// Valid, schema-conforming data the file's format cannot express — a null in TOML,
    /// an XML document without its one root key, a markdown envelope without its parts,
    /// an integer beyond ±2^53 in YAML.
    NotExpressible { path: String, message: String },
    WriteFailed { path: String, message: String },
}

impl TextWriteError {
    /// The registry's failure mode for this error, its slots filled. A schema refusal is
    /// the reader engine's own mode, with the file it would have written named first.
    pub fn failure(&self) -> Failure {
        match self {
            TextWriteError::NotJson { path, message } => failure_mode_core::text_not_json(path, message),
            TextWriteError::SchemaRefused { path, source } => {
                let mut failure = source.failure();
                failure.standard = format!("Refused to write '{path}': {}", failure.standard);
                failure
            }
            TextWriteError::NotExpressible { path, message } => failure_mode_core::data_not_expressible(path, message),
            TextWriteError::WriteFailed { path, message } => failure_mode_core::file_unwritable(path, message),
        }
    }

    /// The failure class a caller branches on — the `error.type` of the writer result.
    pub fn error_type(&self) -> &'static str {
        self.failure().class
    }
}

impl std::fmt::Display for TextWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.failure())
    }
}

impl std::error::Error for TextWriteError {}

/// The text to write: the JSON exactly as given, or the data in the file's format.
fn rendered(format: TextFormat, json_data: &str, data: &Value) -> Result<String, String> {
    let converted = match format {
        TextFormat::Json => return Ok(json_data.to_string()),
        // json_to_toml names every null and its fix when the data cannot be TOML.
        TextFormat::Toml => json_to_toml(json_data),
        TextFormat::Yaml => to_yaml(data),
        TextFormat::Toon => to_toon(data),
        TextFormat::Xml => to_xml(data),
        TextFormat::Markdown => to_markdown_document(data),
    };
    converted.map_err(|format_err| format_err.to_string())
}

/// The file's text: `text` ended with a line break. A Markdown document is the one
/// exception — its body is data, and is written to its last byte exactly as given.
fn file_text(format: TextFormat, text: String) -> String {
    if format == TextFormat::Markdown || text.ends_with('\n') {
        text
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
    write_engine::write_file_atomic(path, text).map_err(write_failed)
}

/// Validate `json_data` against `{schema_dir}/{schema_name}.schema.json` and convert
/// it to `format`: the exact text a write of it puts on disk, its final line break
/// included. `file_path` names the file in the errors; nothing is written.
pub fn validate_render(
    format: TextFormat,
    json_data: &str,
    file_path: &str,
    schema_name: &str,
    schema_dir: &str,
) -> Result<String, TextWriteError> {
    validate_render_with(format, json_data, file_path, &SchemaSource::Named { name: schema_name, dir: schema_dir })
}

/// Validate `json_data` against `schema` and convert it to `format`: the exact text a
/// write of it puts on disk, its final line break included. Nothing is written.
pub fn validate_render_with(
    format: TextFormat,
    json_data: &str,
    file_path: &str,
    schema: &SchemaSource<'_>,
) -> Result<String, TextWriteError> {
    let data: Value = serde_json::from_str(json_data).map_err(|json_err| TextWriteError::NotJson {
        path: file_path.to_string(),
        message: json_err.to_string(),
    })?;
    validate_against(&data, json_data, schema)
        .map_err(|source| TextWriteError::SchemaRefused { path: file_path.to_string(), source })?;
    let text = rendered(format, json_data, &data)
        .map_err(|message| TextWriteError::NotExpressible { path: file_path.to_string(), message })?;
    Ok(file_text(format, text))
}

/// Validate `json_data` against `{schema_dir}/{schema_name}.schema.json`, convert
/// it to `format`, then write it to `file_path`. Every format but Markdown ends with a
/// line break; a Markdown document ends where its body ends.
pub fn validate_write(
    format: TextFormat,
    json_data: &str,
    file_path: &str,
    schema_name: &str,
    schema_dir: &str,
) -> Result<(), TextWriteError> {
    validate_write_with(format, json_data, file_path, &SchemaSource::Named { name: schema_name, dir: schema_dir })
}

/// Validate `json_data` against `schema`, convert it to `format`, then write it to
/// `file_path`.
pub fn validate_write_with(
    format: TextFormat,
    json_data: &str,
    file_path: &str,
    schema: &SchemaSource<'_>,
) -> Result<(), TextWriteError> {
    let text = validate_render_with(format, json_data, file_path, schema)?;
    write_validated_text(file_path, &text)
}

/// Validate `json_data` against `{schema_dir}/{schema_name}.schema.json`, then
/// write it to `file_path` exactly as given, ended with a line break.
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
    fn yaml_and_frontmatter_quote_what_another_reader_would_mistype_and_refuse_what_it_would_round() {
        let dir = temp_dir("faithful_yaml");
        write_schema(&dir, "faithful_yaml", r#"{"type": "object"}"#);
        let schema_dir = dir_str(&dir);

        let yaml_file = dir.join("out.yaml");
        let data = r#"{"date": "1999-04-01", "answer": "yes", "ratio": 1e-7}"#;
        validate_write(TextFormat::Yaml, data, yaml_file.to_str().unwrap(), "faithful_yaml", schema_dir).unwrap();
        let written = fs::read_to_string(&yaml_file).unwrap();
        assert_eq!(written, "date: \"1999-04-01\"\nanswer: \"yes\"\nratio: 1.0e-7\n");
        let expected: Value = serde_json::from_str(data).unwrap();
        assert_eq!(read_back(SourceFormat::Yaml, &yaml_file, "faithful_yaml", &dir), expected);

        let page = dir.join("page.md");
        let envelope = r#"{"frontmatter": {"occurrence/effective": "1971-12-18", "note": "l1\n"}, "body": "Body.\n"}"#;
        validate_write(TextFormat::Markdown, envelope, page.to_str().unwrap(), "faithful_yaml", schema_dir).unwrap();
        let page_text = fs::read_to_string(&page).unwrap();
        assert_eq!(page_text, "---\noccurrence/effective: \"1971-12-18\"\nnote: |\n  l1\n---\nBody.\n");
        let read = read_back(SourceFormat::Markdown, &page, "faithful_yaml", &dir);
        assert_eq!(read, serde_json::from_str::<Value>(envelope).unwrap());
        let rewritten = validate_render(TextFormat::Markdown, &read.to_string(), "page.md", "faithful_yaml", schema_dir);
        assert_eq!(rewritten.unwrap(), page_text);

        let big = dir.join("big.yaml");
        let error = validate_write(TextFormat::Yaml, r#"{"id": 9007199254740993}"#, big.to_str().unwrap(), "faithful_yaml", schema_dir)
            .unwrap_err();
        assert_eq!(error.error_type(), "conversion_error");
        assert!(!big.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn markdown_ends_where_its_body_ends() {
        let dir = temp_dir("markdown_last_byte");
        write_schema(&dir, "markdown_last_byte", ENVELOPE_SCHEMA);
        for (name, body) in [("open.md", "No final line break"), ("empty.md", ""), ("fence.md", "---")] {
            let file = dir.join(name);
            let data = serde_json::json!({"frontmatter": {"title": "x"}, "body": body}).to_string();
            validate_write(TextFormat::Markdown, &data, file.to_str().unwrap(), "markdown_last_byte", dir_str(&dir)).unwrap();
            assert_eq!(fs::read_to_string(&file).unwrap(), format!("---\ntitle: x\n---\n{body}"));
            let expected: Value = serde_json::from_str(&data).unwrap();
            assert_eq!(read_back(SourceFormat::Markdown, &file, "markdown_last_byte", &dir), expected, "{name}");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn toon_and_xml_refuse_data_they_would_hand_back_changed() {
        let dir = temp_dir("verified");
        write_schema(&dir, "verified", r#"{"type": "object"}"#);
        for (text_format, data, file_name) in [
            (TextFormat::Toon, r#"{"big": 18446744073709551615}"#, "out.toon"),
            (TextFormat::Toon, r#"{"huge": 1e300}"#, "huge.toon"),
            (TextFormat::Xml, r#"{"root": {"count": 3}}"#, "out.xml"),
            (TextFormat::Xml, r#"{"root": {"a b": "x"}}"#, "name.xml"),
            (TextFormat::Toml, r#"{"big": 9223372036854775808}"#, "out.toml"),
        ] {
            let file = dir.join(file_name);
            let error = validate_write(text_format, data, file.to_str().unwrap(), "verified", dir_str(&dir)).unwrap_err();
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
