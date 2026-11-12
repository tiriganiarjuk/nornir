//! Shared reader IO — the runtime-schema sibling of `gate_io`.
//!
//! A reader reads one structured-file format from disk, converts it to JSON
//! through `format_core`, validates it against a JSON Schema loaded BY NAME
//! from a directory at call time, and returns the validated JSON string. The
//! schema is a parameter, not a compile-time embed: the consuming project owns
//! its schemas. Each `readers/read_{format}_validate_json` crate is a thin PyO3
//! wrapper over `read_validate_json` with its format fixed.
//!
//! Also home to the tomlx readers: a `.tomlx` file carries its own schema (derived
//! from its annotations by `format_core::tomlx`), so those take no schema parameter.
//!
//! Flow: read file → format→JSON → load+compile schema (cached per process) →
//! validate → JSON string, or a typed `ReaderError` with an educational message.
//! The readers hand both to Python in the gate contract:
//! `{"ok": bool, "data": str | None, "error": {"type": str, "message": str} | None}`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use jsonschema::Validator;
use serde_json::Value;

use error_core::{format_educational_message, FormatError, ValidationIssue};
use format_core::convert::{
    json_to_json, markdown_to_json, toml_to_json, toon_to_json, xml_to_json, yaml_to_json,
};
use format_core::serialize::to_json;
use format_core::tomlx::{parse_tomlx, TomlxOutput};

// =============================================================================
// Source formats
// =============================================================================

/// Which decoder `format_core` applies before validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Json,
    Yaml,
    Toml,
    Toon,
    Xml,
    /// A Markdown document with optional YAML frontmatter; arrives as the
    /// envelope `{"frontmatter": {...}, "body": "..."}` — the schema targets that shape.
    Markdown,
}

impl SourceFormat {
    /// Upper-case label used in error messages.
    pub fn label(self) -> &'static str {
        match self {
            SourceFormat::Json => "JSON",
            SourceFormat::Yaml => "YAML",
            SourceFormat::Toml => "TOML",
            SourceFormat::Toon => "TOON",
            SourceFormat::Xml => "XML",
            SourceFormat::Markdown => "MARKDOWN",
        }
    }

    fn to_json(self, content: &str) -> Result<String, FormatError> {
        match self {
            SourceFormat::Json => json_to_json(content),
            SourceFormat::Yaml => yaml_to_json(content),
            SourceFormat::Toml => toml_to_json(content),
            SourceFormat::Toon => toon_to_json(content),
            SourceFormat::Xml => xml_to_json(content),
            SourceFormat::Markdown => markdown_to_json(content),
        }
    }
}

// =============================================================================
// Errors
// =============================================================================

/// Every failure a reader can produce. `error_type()` is the `error.type` a reader
/// returns to Python; the Display string is the `error.message`.
#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("Cannot read '{path}': {message}")]
    ReadFailed { path: String, message: String },

    #[error("{format} parse error in '{path}': {message}")]
    ParseFailed {
        format: &'static str,
        path: String,
        message: String,
    },

    #[error("Invalid JSON from {format} conversion: {message}")]
    ConvertedJsonInvalid {
        format: &'static str,
        message: String,
    },

    #[error("Cannot read schema '{path}': {message}")]
    SchemaReadFailed { path: String, message: String },

    #[error("Schema '{path}' is not valid JSON: {message}")]
    SchemaNotJson { path: String, message: String },

    #[error("Schema '{path}' is not valid JSON Schema: {message}")]
    SchemaCompileFailed { path: String, message: String },

    /// The educational message from `error_core`, verbatim.
    #[error("{0}")]
    ValidationFailed(String),

    #[error("Validator cache poisoned: {0}")]
    CachePoisoned(String),

    #[error("No type annotations in '{path}' — read_tomlx_json_schema needs at least one annotated section")]
    TomlxNoSchema { path: String },
}

impl ReaderError {
    /// The failure class a caller branches on — the `error.type` of the reader result.
    pub fn error_type(&self) -> &'static str {
        match self {
            ReaderError::ReadFailed { .. } => "read_error",
            ReaderError::ParseFailed { .. } => "parse_error",
            ReaderError::ConvertedJsonInvalid { .. } => "conversion_error",
            ReaderError::SchemaReadFailed { .. }
            | ReaderError::SchemaNotJson { .. }
            | ReaderError::SchemaCompileFailed { .. }
            | ReaderError::TomlxNoSchema { .. } => "schema_error",
            ReaderError::ValidationFailed(_) => "validation_error",
            ReaderError::CachePoisoned(_) => "cache_error",
        }
    }
}

// =============================================================================
// Schema cache (compiled once per schema path, per process)
// =============================================================================

static VALIDATOR_CACHE: Mutex<Option<HashMap<String, Validator>>> = Mutex::new(None);

/// Build the schema file path from a name and a directory:
/// `{schema_dir}/{schema_name}.schema.json`. Pass an absolute `schema_dir`.
pub fn schema_file_path(schema_name: &str, schema_dir: &str) -> String {
    format!("{}/{}.schema.json", schema_dir, schema_name)
}

fn compile_and_cache_schema(
    cache: &mut HashMap<String, Validator>,
    schema_path: &str,
) -> Result<(), ReaderError> {
    let schema_str = fs::read_to_string(schema_path).map_err(|io_err| ReaderError::SchemaReadFailed {
        path: schema_path.to_string(),
        message: io_err.to_string(),
    })?;

    let schema_value: Value = serde_json::from_str(&schema_str).map_err(|json_err| ReaderError::SchemaNotJson {
        path: schema_path.to_string(),
        message: json_err.to_string(),
    })?;

    let validator = Validator::new(&schema_value).map_err(|schema_err| ReaderError::SchemaCompileFailed {
        path: schema_path.to_string(),
        message: schema_err.to_string(),
    })?;

    cache.insert(schema_path.to_string(), validator);
    Ok(())
}

fn read_file(path: &str) -> Result<String, ReaderError> {
    fs::read_to_string(path).map_err(|io_err| ReaderError::ReadFailed {
        path: path.to_string(),
        message: io_err.to_string(),
    })
}

// =============================================================================
// Schema-parametric read
// =============================================================================

/// Read `file_path` as `format`, convert to JSON, validate against
/// `{schema_dir}/{schema_name}.schema.json`, return the validated JSON string.
pub fn read_validate_json(
    format: SourceFormat,
    file_path: &str,
    schema_name: &str,
    schema_dir: &str,
) -> Result<String, ReaderError> {
    let schema_path = schema_file_path(schema_name, schema_dir);

    let content = read_file(file_path)?;

    let json_str = format.to_json(&content).map_err(|format_err| ReaderError::ParseFailed {
        format: format.label(),
        path: file_path.to_string(),
        message: format_err.to_string(),
    })?;

    let data: Value = serde_json::from_str(&json_str).map_err(|json_err| ReaderError::ConvertedJsonInvalid {
        format: format.label(),
        message: json_err.to_string(),
    })?;

    let mut cache_guard = VALIDATOR_CACHE
        .lock()
        .map_err(|poison_err| ReaderError::CachePoisoned(poison_err.to_string()))?;
    let cache = cache_guard.get_or_insert_with(HashMap::new);

    if !cache.contains_key(&schema_path) {
        compile_and_cache_schema(cache, &schema_path)?;
    }

    let validator = cache.get(&schema_path).ok_or_else(|| ReaderError::SchemaCompileFailed {
        path: schema_path.clone(),
        message: "not in cache after compilation".to_string(),
    })?;

    let errors: Vec<_> = validator.iter_errors(&data).collect();
    if errors.is_empty() {
        return Ok(json_str);
    }

    let issues: Vec<_> = errors.iter().map(ValidationIssue::from_schema_error).collect();
    Err(ReaderError::ValidationFailed(format_educational_message(schema_name, &issues)))
}

// =============================================================================
// tomlx (schema carried in the file)
// =============================================================================

/// Read and parse a `.tomlx` file with live environment resolution. The file's
/// parent directory serves `base=config` references.
pub fn read_tomlx(file_path: &str) -> Result<TomlxOutput, ReaderError> {
    let content = read_file(file_path)?;
    let config_dir = Path::new(file_path).parent();
    let resolve_env = |name: &str| std::env::var(name).ok();
    parse_tomlx(&content, Some(file_path), config_dir, &resolve_env).map_err(|format_err| ReaderError::ParseFailed {
        format: "TOMLX",
        path: file_path.to_string(),
        message: format_err.to_string(),
    })
}

fn serialize_value(value: &Value) -> Result<String, ReaderError> {
    to_json(value).map_err(|format_err| ReaderError::ConvertedJsonInvalid {
        format: "TOMLX",
        message: format_err.to_string(),
    })
}

/// Read a `.tomlx` file and return its converted data as a JSON string. Data
/// only — the derived schema is not merged in.
pub fn read_tomlx_json(file_path: &str) -> Result<String, ReaderError> {
    let output = read_tomlx(file_path)?;
    serialize_value(&output.data)
}

/// Read a `.tomlx` file and return one JSON object string, `{"data": …, "schema": …}`:
/// the converted data and the JSON Schema derived from its annotations. Errors with
/// `TomlxNoSchema` when the file has no type annotations to derive a schema from.
pub fn read_tomlx_json_schema(file_path: &str) -> Result<String, ReaderError> {
    let output = read_tomlx(file_path)?;
    let schema = output.schema.as_ref().ok_or_else(|| ReaderError::TomlxNoSchema {
        path: file_path.to_string(),
    })?;
    serialize_value(&serde_json::json!({"data": output.data, "schema": schema}))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NAME_AGE_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["name"],
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"}
        }
    }"#;

    /// Fresh temp dir per test. The validator cache is process-global and keyed
    /// by schema path, so every test uses its own directory AND schema name.
    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nornir_reader_io_{}", test_name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_schema(dir: &Path, name: &str, body: &str) -> String {
        let path = dir.join(format!("{}.schema.json", name));
        fs::write(&path, body).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn write_data(dir: &Path, file_name: &str, body: &str) -> String {
        let path = dir.join(file_name);
        fs::write(&path, body).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn dir_str(dir: &Path) -> &str {
        dir.to_str().unwrap()
    }

    // -- schema_file_path --

    #[test]
    fn schema_file_path_joins_dir_name_suffix() {
        assert_eq!(schema_file_path("atom", "/schemas"), "/schemas/atom.schema.json");
    }

    // -- read_validate_json: one valid read per format --

    #[test]
    fn valid_toml() {
        let dir = temp_dir("valid_toml");
        write_schema(&dir, "valid_toml", NAME_AGE_SCHEMA);
        let file = write_data(&dir, "data.toml", "name = \"x\"\nage = 3\n");
        let json = read_validate_json(SourceFormat::Toml, &file, "valid_toml", dir_str(&dir)).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["name"], "x");
        assert_eq!(value["age"], 3);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_yaml() {
        let dir = temp_dir("valid_yaml");
        write_schema(&dir, "valid_yaml", NAME_AGE_SCHEMA);
        let file = write_data(&dir, "data.yaml", "name: x\nage: 3\n");
        let json = read_validate_json(SourceFormat::Yaml, &file, "valid_yaml", dir_str(&dir)).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["name"], "x");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_json() {
        let dir = temp_dir("valid_json");
        write_schema(&dir, "valid_json", NAME_AGE_SCHEMA);
        let file = write_data(&dir, "data.json", r#"{"name": "x", "age": 3}"#);
        let json = read_validate_json(SourceFormat::Json, &file, "valid_json", dir_str(&dir)).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["age"], 3);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_toon() {
        let dir = temp_dir("valid_toon");
        write_schema(&dir, "valid_toon", NAME_AGE_SCHEMA);
        let file = write_data(&dir, "data.toon", "name: x\n");
        let json = read_validate_json(SourceFormat::Toon, &file, "valid_toon", dir_str(&dir)).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["name"], "x");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_xml() {
        // XML arrives as {"root": {...}} — the schema targets that shape.
        let dir = temp_dir("valid_xml");
        let schema = r#"{"type": "object", "required": ["root"], "properties": {"root": {"type": "object"}}}"#;
        write_schema(&dir, "valid_xml", schema);
        let file = write_data(&dir, "data.xml", "<root><name>x</name></root>");
        let json = read_validate_json(SourceFormat::Xml, &file, "valid_xml", dir_str(&dir)).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["root"]["name"], "x");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn valid_markdown() {
        // Markdown arrives as {"frontmatter": {...}, "body": "..."} — the schema targets that shape.
        let dir = temp_dir("valid_markdown");
        let schema = r#"{"type": "object", "required": ["frontmatter", "body"],
            "properties": {"frontmatter": {"type": "object", "required": ["name"]},
                           "body": {"type": "string"}}}"#;
        write_schema(&dir, "valid_markdown", schema);
        let file = write_data(&dir, "data.md", "---\nname: x\n---\nThe body.\n");
        let json = read_validate_json(SourceFormat::Markdown, &file, "valid_markdown", dir_str(&dir)).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["frontmatter"]["name"], "x");
        assert_eq!(value["body"], "The body.\n");
        fs::remove_dir_all(&dir).ok();
    }

    // -- read_validate_json: failures --

    #[test]
    fn schema_violation_is_educational() {
        let dir = temp_dir("schema_violation");
        write_schema(&dir, "schema_violation", NAME_AGE_SCHEMA);
        let file = write_data(&dir, "data.toml", "name = 7\n");
        let err = read_validate_json(SourceFormat::Toml, &file, "schema_violation", dir_str(&dir)).unwrap_err();
        match err {
            ReaderError::ValidationFailed(msg) => {
                assert!(msg.starts_with("schema_violation validation failed"), "{msg}");
                assert!(msg.contains("type_mismatch"), "{msg}");
            }
            other => panic!("Expected ValidationFailed, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_schema_file() {
        let dir = temp_dir("missing_schema");
        let file = write_data(&dir, "data.toml", "name = \"x\"\n");
        let err = read_validate_json(SourceFormat::Toml, &file, "missing", dir_str(&dir)).unwrap_err();
        match err {
            ReaderError::SchemaReadFailed { path, .. } => assert!(path.ends_with("missing.schema.json")),
            other => panic!("Expected SchemaReadFailed, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_not_json() {
        let dir = temp_dir("schema_not_json");
        write_schema(&dir, "schema_not_json", "not json at all");
        let file = write_data(&dir, "data.toml", "name = \"x\"\n");
        let err = read_validate_json(SourceFormat::Toml, &file, "schema_not_json", dir_str(&dir)).unwrap_err();
        assert!(matches!(err, ReaderError::SchemaNotJson { .. }), "{:?}", err);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn schema_not_json_schema() {
        let dir = temp_dir("schema_bad");
        write_schema(&dir, "schema_bad", r#"{"type": 42}"#);
        let file = write_data(&dir, "data.toml", "name = \"x\"\n");
        let err = read_validate_json(SourceFormat::Toml, &file, "schema_bad", dir_str(&dir)).unwrap_err();
        assert!(matches!(err, ReaderError::SchemaCompileFailed { .. }), "{:?}", err);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bad_format_parse() {
        let dir = temp_dir("bad_parse");
        write_schema(&dir, "bad_parse", NAME_AGE_SCHEMA);
        let file = write_data(&dir, "data.toml", "name = \"unterminated\n");
        let err = read_validate_json(SourceFormat::Toml, &file, "bad_parse", dir_str(&dir)).unwrap_err();
        match err {
            ReaderError::ParseFailed { format, .. } => assert_eq!(format, "TOML"),
            other => panic!("Expected ParseFailed, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_data_file() {
        let dir = temp_dir("missing_data");
        write_schema(&dir, "missing_data", NAME_AGE_SCHEMA);
        let err = read_validate_json(
            SourceFormat::Toml,
            "/absolutely/does/not/exist.toml",
            "missing_data",
            dir_str(&dir),
        )
        .unwrap_err();
        match err {
            ReaderError::ReadFailed { path, .. } => assert_eq!(path, "/absolutely/does/not/exist.toml"),
            other => panic!("Expected ReadFailed, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cache_reuse_survives_schema_file_deletion() {
        // Proves the compiled validator is reused: delete the schema file after the
        // first call; the second call against the same name/dir still validates.
        let dir = temp_dir("cache_reuse");
        let schema_path = write_schema(&dir, "cache_reuse", NAME_AGE_SCHEMA);
        let first = write_data(&dir, "first.toml", "name = \"a\"\n");
        read_validate_json(SourceFormat::Toml, &first, "cache_reuse", dir_str(&dir)).unwrap();

        fs::remove_file(&schema_path).unwrap();

        let second = write_data(&dir, "second.toml", "name = \"b\"\n");
        let json = read_validate_json(SourceFormat::Toml, &second, "cache_reuse", dir_str(&dir)).unwrap();
        assert!(json.contains("\"b\""));
        fs::remove_dir_all(&dir).ok();
    }

    // -- tomlx --

    #[test]
    fn tomlx_json_returns_data_only() {
        let dir = temp_dir("tomlx_data");
        let file = write_data(&dir, "config.tomlx", "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\n");
        let json = read_tomlx_json(&file).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["ttl"]["session"], 900);
        assert!(value.get("schema").is_none(), "{json}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tomlx_json_schema_returns_data_and_schema_object() {
        let dir = temp_dir("tomlx_schema");
        let file = write_data(&dir, "config.tomlx", "\n[features]  # type=bool\ndark_mode = true\n");
        let json = read_tomlx_json_schema(&file).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["data"]["features"]["dark_mode"], true);
        assert_eq!(value["schema"]["type"], "object");
        assert!(value["schema"]["properties"].is_object(), "{json}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tomlx_json_schema_without_annotations_errors() {
        let dir = temp_dir("tomlx_no_schema");
        let file = write_data(&dir, "plain.tomlx", "\n[metadata]\nname = \"test\"\n");
        let err = read_tomlx_json_schema(&file).unwrap_err();
        assert!(matches!(err, ReaderError::TomlxNoSchema { .. }), "{:?}", err);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tomlx_parse_error_maps() {
        let dir = temp_dir("tomlx_bad");
        let file = write_data(&dir, "bad.tomlx", "name = \"unterminated\n");
        let err = read_tomlx_json(&file).unwrap_err();
        match err {
            ReaderError::ParseFailed { format, .. } => assert_eq!(format, "TOMLX"),
            other => panic!("Expected ParseFailed, got: {:?}", other),
        }
        fs::remove_dir_all(&dir).ok();
    }
}
