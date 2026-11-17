//! Extract `path_exists_absolute` fields from data using schema as a map.
//!
//! Pure function — no filesystem access, extraction only. Walks the schema
//! recursively through `properties`, `allOf`, `oneOf`, `anyOf`, `if/then/else`,
//! and `items` to find all fields annotated with `format: path_exists_absolute`,
//! then extracts the corresponding values from the data using JSON pointers.
//!
//! Also the two rules about paths every tool shares: what makes a name one
//! plain path segment (`validate_path_segment`), and where the ai home is under
//! a home directory (`ai_home_under`).

use error_core::{PathField, SchemaError};
use serde_json::Value;

/// Extract all path fields annotated with `format: path_exists_absolute` from
/// the data, using the schema as a map to locate them.
///
/// Returns a `Vec<PathField>` containing each field's JSON pointer and value.
/// Fields that exist in the schema but are missing from the data are silently
/// skipped (the schema validator catches missing required fields separately).
pub fn extract_path_fields(
    schema_json: &str,
    data_json: &str,
) -> Result<Vec<PathField>, SchemaError> {
    let schema: Value = serde_json::from_str(schema_json)
        .map_err(|e| SchemaError::InvalidInput(format!("Invalid schema JSON: {}", e)))?;
    let data: Value = serde_json::from_str(data_json)
        .map_err(|e| SchemaError::InvalidInput(format!("Invalid data JSON: {}", e)))?;

    let mut fields = Vec::new();
    walk_schema(&schema, &data, "", &mut fields);
    Ok(fields)
}

/// Recursively walk the schema, accumulating path fields found in the data.
fn walk_schema(schema: &Value, data: &Value, pointer: &str, fields: &mut Vec<PathField>) {
    let schema_map = match schema.as_object() {
        Some(m) => m,
        None => return,
    };

    if try_extract_leaf(schema_map, data, pointer, fields) {
        return;
    }

    walk_properties(schema_map, data, pointer, fields);
    walk_combinators(schema_map, data, pointer, fields);
    walk_conditionals(schema_map, data, pointer, fields);
    walk_items(schema_map, data, pointer, fields);
}

/// If this schema node is a path leaf (format: path_exists_absolute), extract the value.
/// Returns true if it was a leaf (caller should stop recursing).
fn try_extract_leaf(
    schema_map: &serde_json::Map<String, Value>,
    data: &Value,
    pointer: &str,
    fields: &mut Vec<PathField>,
) -> bool {
    let fmt = match schema_map.get("format").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => return false,
    };
    if fmt != "path_exists_absolute" {
        return false;
    }
    if let Some(Value::String(path_value)) = resolve_pointer(data, pointer) {
        let json_pointer = if pointer.is_empty() { "/" } else { pointer };
        fields.push(PathField {
            json_pointer: json_pointer.to_string(),
            value: path_value.to_string(),
        });
    }
    true
}

/// Walk `properties` — each property gets a child pointer segment.
fn walk_properties(
    schema_map: &serde_json::Map<String, Value>,
    data: &Value,
    pointer: &str,
    fields: &mut Vec<PathField>,
) {
    let props = match schema_map.get("properties").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return,
    };
    for (key, prop_schema) in props {
        let child_pointer = format!("{}/{}", pointer, key);
        walk_schema(prop_schema, data, &child_pointer, fields);
    }
}

/// Walk `allOf`, `oneOf`, `anyOf` — same pointer, multiple sub-schemas.
fn walk_combinators(
    schema_map: &serde_json::Map<String, Value>,
    data: &Value,
    pointer: &str,
    fields: &mut Vec<PathField>,
) {
    for key in &["allOf", "oneOf", "anyOf"] {
        if let Some(Value::Array(schemas)) = schema_map.get(*key) {
            for sub_schema in schemas {
                walk_schema(sub_schema, data, pointer, fields);
            }
        }
    }
}

/// Walk `if/then/else` — same pointer through conditional branches.
fn walk_conditionals(
    schema_map: &serde_json::Map<String, Value>,
    data: &Value,
    pointer: &str,
    fields: &mut Vec<PathField>,
) {
    if let Some(then_schema) = schema_map.get("then") {
        walk_schema(then_schema, data, pointer, fields);
    }
    if let Some(else_schema) = schema_map.get("else") {
        walk_schema(else_schema, data, pointer, fields);
    }
}

/// Walk `items` — each array element gets an indexed pointer segment.
fn walk_items(
    schema_map: &serde_json::Map<String, Value>,
    data: &Value,
    pointer: &str,
    fields: &mut Vec<PathField>,
) {
    let items_schema = match schema_map.get("items") {
        Some(s) => s,
        None => return,
    };
    let data_items = match resolve_pointer(data, pointer).and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return,
    };
    match items_schema {
        Value::Object(_) => {
            for (i, _item) in data_items.iter().enumerate() {
                let item_pointer = format!("{}/{}", pointer, i);
                walk_schema(items_schema, data, &item_pointer, fields);
            }
        }
        Value::Array(item_schemas) => {
            for (i, item_schema) in item_schemas.iter().enumerate() {
                if i < data_items.len() {
                    let item_pointer = format!("{}/{}", pointer, i);
                    walk_schema(item_schema, data, &item_pointer, fields);
                }
            }
        }
        _ => {}
    }
}

/// Resolve a JSON pointer path against a Value.
/// Returns `None` if the path doesn't exist in the data.
fn resolve_pointer<'a>(data: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(data);
    }
    data.pointer(pointer)
}

// =============================================================================
// Filename component validation
// =============================================================================

/// Validate an untrusted filename component (single path segment).
///
/// Rejects path traversal (`..`, `/`, `\`, null bytes), hidden files
/// (leading `.`), flag injection (leading `-`), and empty strings.
///
/// Use this for any LLM-provided or user-provided filename, directory name,
/// or path segment that will be joined into a filesystem path.
pub fn validate_path_segment(input: &str) -> Result<(), String> {
    if input.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if input.contains("..") {
        return Err(format!("\"..\" forbidden in name: \"{input}\""));
    }
    if input.contains('/') || input.contains('\\') {
        return Err(format!("path separators forbidden in name: \"{input}\""));
    }
    if input.contains('\0') {
        return Err(format!("null byte in name: \"{input}\""));
    }
    if input.starts_with('.') {
        return Err(format!("name cannot start with \".\" (no hidden files): \"{input}\""));
    }
    if input.starts_with('-') {
        return Err(format!("name cannot start with \"-\" (no flag injection): \"{input}\""));
    }
    Ok(())
}

// =============================================================================
// The ai home
// =============================================================================

/// The ai home, `~/ai`, under the home directory `home`.
///
/// `home` is the value of `$HOME` as the caller read it. With none, or an empty
/// one, the ai home is under `/tmp`.
pub fn ai_home_under(home: Option<&str>) -> std::path::PathBuf {
    let home = home.filter(|named| !named.is_empty()).unwrap_or("/tmp");
    std::path::Path::new(home).join("ai")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ai_home_is_under_the_home_directory() {
        assert_eq!(ai_home_under(Some("/Users/u")), std::path::Path::new("/Users/u/ai"));
    }

    #[test]
    fn without_a_home_directory_the_ai_home_is_under_tmp() {
        assert_eq!(ai_home_under(None), std::path::Path::new("/tmp/ai"));
        assert_eq!(ai_home_under(Some("")), std::path::Path::new("/tmp/ai"));
    }

    #[test]
    fn test_simple_flat_path_field() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "config_path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                }
            }
        }"#;
        let data = r#"{"config_path": "/etc/config.json"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].json_pointer, "/config_path");
        assert_eq!(fields[0].value, "/etc/config.json");
    }

    #[test]
    fn test_nested_properties() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "security": {
                    "type": "object",
                    "properties": {
                        "schemas": {
                            "type": "object",
                            "properties": {
                                "input_schema": {
                                    "type": "string",
                                    "format": "path_exists_absolute"
                                }
                            }
                        }
                    }
                }
            }
        }"#;
        let data = r#"{"security": {"schemas": {"input_schema": "/path/to/schema.json"}}}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].json_pointer,
            "/security/schemas/input_schema"
        );
        assert_eq!(fields[0].value, "/path/to/schema.json");
    }

    #[test]
    fn test_allof_traversal() {
        let schema = r#"{
            "type": "object",
            "allOf": [
                {
                    "properties": {
                        "path_a": {
                            "type": "string",
                            "format": "path_exists_absolute"
                        }
                    }
                },
                {
                    "properties": {
                        "path_b": {
                            "type": "string",
                            "format": "path_exists_absolute"
                        }
                    }
                }
            ]
        }"#;
        let data = r#"{"path_a": "/a/path", "path_b": "/b/path"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn test_oneof_traversal() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "include": {
                                        "type": "string",
                                        "format": "path_exists_absolute"
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "inline": {"type": "string"}
                                }
                            }
                        ]
                    }
                }
            }
        }"#;
        let data = r#"{"items": [{"include": "/path/to/file.md"}, {"inline": "text"}]}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].json_pointer, "/items/0/include");
        assert_eq!(fields[0].value, "/path/to/file.md");
    }

    #[test]
    fn test_if_then_else_traversal() {
        let schema = r#"{
            "type": "object",
            "allOf": [
                {
                    "if": {
                        "properties": {"type": {"const": "file"}}
                    },
                    "then": {
                        "properties": {
                            "source": {
                                "type": "string",
                                "format": "path_exists_absolute"
                            }
                        }
                    },
                    "else": {
                        "properties": {
                            "url": {"type": "string"}
                        }
                    }
                }
            ]
        }"#;
        let data = r#"{"type": "file", "source": "/data/input.csv"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].json_pointer, "/source");
    }

    #[test]
    fn test_missing_data_fields_skipped() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "required_path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                },
                "optional_path": {
                    "type": "string",
                    "format": "path_exists_absolute"
                }
            }
        }"#;
        let data = r#"{"required_path": "/exists"}"#;
        let fields = extract_path_fields(schema, data).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value, "/exists");
    }

    #[test]
    fn test_real_schema_structure() {
        // Mimics the real agent-paths-resolved schema structure:
        // top-level properties -> nested objects -> allOf with if/then/else
        let schema = r#"{
            "type": "object",
            "properties": {
                "security": {
                    "type": "object",
                    "properties": {
                        "schemas": {
                            "type": "object",
                            "properties": {
                                "security_input_schema": {
                                    "type": "string",
                                    "format": "path_exists_absolute"
                                },
                                "security_output_schema": {
                                    "type": "string",
                                    "format": "path_exists_absolute"
                                }
                            }
                        }
                    },
                    "allOf": [
                        {
                            "if": {
                                "properties": { "has_tools": { "const": true } }
                            },
                            "then": {
                                "properties": {
                                    "tool_config": {
                                        "type": "string",
                                        "format": "path_exists_absolute"
                                    }
                                }
                            },
                            "else": {}
                        }
                    ]
                },
                "execution": {
                    "type": "object",
                    "properties": {
                        "instructions": {
                            "type": "array",
                            "items": {
                                "oneOf": [
                                    {
                                        "type": "object",
                                        "properties": {
                                            "include": {
                                                "type": "string",
                                                "format": "path_exists_absolute"
                                            }
                                        }
                                    },
                                    {
                                        "type": "object",
                                        "properties": {
                                            "text": { "type": "string" }
                                        }
                                    }
                                ]
                            }
                        }
                    }
                }
            }
        }"#;
        let data = r#"{
            "security": {
                "has_tools": true,
                "schemas": {
                    "security_input_schema": "/schemas/input.json",
                    "security_output_schema": "/schemas/output.json"
                },
                "tool_config": "/config/tools.json"
            },
            "execution": {
                "instructions": [
                    {"include": "/instructions/step1.md"},
                    {"text": "inline instruction"},
                    {"include": "/instructions/step3.md"}
                ]
            }
        }"#;
        let fields = extract_path_fields(schema, data).unwrap();
        // Should find: input_schema, output_schema, tool_config, step1.md, step3.md
        assert_eq!(fields.len(), 5);
        let values: Vec<&str> = fields.iter().map(|f| f.value.as_str()).collect();
        assert!(values.contains(&"/schemas/input.json"));
        assert!(values.contains(&"/schemas/output.json"));
        assert!(values.contains(&"/config/tools.json"));
        assert!(values.contains(&"/instructions/step1.md"));
        assert!(values.contains(&"/instructions/step3.md"));
    }

    // ── validate_path_segment ───────────────────────────────────────

    #[test]
    fn path_segment_valid() {
        assert!(super::validate_path_segment("entry-123").is_ok());
        assert!(super::validate_path_segment("my_file_name").is_ok());
        assert!(super::validate_path_segment("report2024").is_ok());
    }

    #[test]
    fn path_segment_empty() {
        assert!(super::validate_path_segment("").is_err());
    }

    #[test]
    fn path_segment_traversal() {
        assert!(super::validate_path_segment("..").is_err());
        assert!(super::validate_path_segment("../etc/passwd").is_err());
        assert!(super::validate_path_segment("foo/../bar").is_err());
    }

    #[test]
    fn path_segment_separators() {
        assert!(super::validate_path_segment("foo/bar").is_err());
        assert!(super::validate_path_segment("foo\\bar").is_err());
    }

    #[test]
    fn path_segment_null() {
        assert!(super::validate_path_segment("foo\0bar").is_err());
    }

    #[test]
    fn path_segment_hidden() {
        assert!(super::validate_path_segment(".hidden").is_err());
        assert!(super::validate_path_segment(".").is_err());
    }

    #[test]
    fn path_segment_flag_injection() {
        assert!(super::validate_path_segment("-flag").is_err());
        assert!(super::validate_path_segment("--verbose").is_err());
    }
}
