//! Format conversions with educational error diagnostics.
//!
//! Fast path first, diagnostics on failure:
//! 1. Try the conversion (lean)
//! 2. On success: return immediately
//! 3. On failure: analyze for educational errors
//! 4. Return helpful error with solution if possible

use error_core::FormatError;

use crate::{parse, serialize};

// =============================================================================
// JSON source conversions
// =============================================================================

/// Convert JSON string to pretty-printed JSON (canonicalize).
pub fn json_to_json(content: &str) -> Result<String, FormatError> {
    let value = parse::json(content)?;
    serialize::to_json(&value)
}

/// Convert JSON string to YAML.
pub fn json_to_yaml(content: &str) -> Result<String, FormatError> {
    let value = parse::json(content)?;
    serialize::to_yaml(&value)
}

/// Convert JSON string to TOML with educational diagnostics on failure.
pub fn json_to_toml(content: &str) -> Result<String, FormatError> {
    let value = parse::json(content)?;
    value_to_toml(&value)
}

/// Convert JSON string to TOON.
pub fn json_to_toon(content: &str) -> Result<String, FormatError> {
    let value = parse::json(content)?;
    serialize::to_toon(&value)
}

// =============================================================================
// YAML source conversions
// =============================================================================

/// Convert YAML string to JSON.
pub fn yaml_to_json(content: &str) -> Result<String, FormatError> {
    let value = parse::yaml(content)?;
    serialize::to_json(&value)
}

/// Convert YAML string to TOML with educational diagnostics on failure.
pub fn yaml_to_toml(content: &str) -> Result<String, FormatError> {
    let value = parse::yaml(content)?;
    value_to_toml(&value)
}

/// Convert YAML string to TOON.
pub fn yaml_to_toon(content: &str) -> Result<String, FormatError> {
    let value = parse::yaml(content)?;
    serialize::to_toon(&value)
}

// =============================================================================
// TOML source conversions
// =============================================================================

/// Convert TOML string to pretty-printed JSON.
pub fn toml_to_json(content: &str) -> Result<String, FormatError> {
    let value = parse::toml(content)?;
    serialize::to_json(&value)
}

/// Convert TOML string to YAML.
pub fn toml_to_yaml(content: &str) -> Result<String, FormatError> {
    let value = parse::toml(content)?;
    serialize::to_yaml(&value)
}

/// Convert TOML string to TOON.
pub fn toml_to_toon(content: &str) -> Result<String, FormatError> {
    let value = parse::toml(content)?;
    serialize::to_toon(&value)
}

// =============================================================================
// TOON source conversions
// =============================================================================

/// Convert TOON string to JSON.
pub fn toon_to_json(content: &str) -> Result<String, FormatError> {
    let value = parse::toon(content)?;
    serialize::to_json(&value)
}

/// Convert TOON string to YAML.
pub fn toon_to_yaml(content: &str) -> Result<String, FormatError> {
    let value = parse::toon(content)?;
    serialize::to_yaml(&value)
}

/// Convert TOON string to TOML with educational diagnostics on failure.
pub fn toon_to_toml(content: &str) -> Result<String, FormatError> {
    let value = parse::toon(content)?;
    value_to_toml(&value)
}

// =============================================================================
// XML source conversions
// =============================================================================

/// Convert XML string to pretty-printed JSON.
pub fn xml_to_json(content: &str) -> Result<String, FormatError> {
    let value = parse::xml(content)?;
    serialize::to_json(&value)
}

/// Convert XML string to YAML.
pub fn xml_to_yaml(content: &str) -> Result<String, FormatError> {
    let value = parse::xml(content)?;
    serialize::to_yaml(&value)
}

/// Convert XML string to TOML with educational diagnostics on failure.
pub fn xml_to_toml(content: &str) -> Result<String, FormatError> {
    let value = parse::xml(content)?;
    value_to_toml(&value)
}

/// Convert XML string to TOON.
pub fn xml_to_toon(content: &str) -> Result<String, FormatError> {
    let value = parse::xml(content)?;
    serialize::to_toon(&value)
}

// =============================================================================
// Markdown source conversions
// =============================================================================

/// Convert a Markdown document (optional YAML frontmatter + body) to the
/// pretty-printed JSON envelope `{"frontmatter": {...}, "body": "..."}`.
pub fn markdown_to_json(content: &str) -> Result<String, FormatError> {
    let value = parse::markdown(content)?;
    serialize::to_json(&value)
}

// =============================================================================
// *-to-XML conversions
// =============================================================================

/// Convert JSON string to XML.
pub fn json_to_xml(content: &str) -> Result<String, FormatError> {
    let value = parse::json(content)?;
    serialize::to_xml(&value)
}

/// Convert YAML string to XML.
pub fn yaml_to_xml(content: &str) -> Result<String, FormatError> {
    let value = parse::yaml(content)?;
    serialize::to_xml(&value)
}

/// Convert TOML string to XML.
pub fn toml_to_xml(content: &str) -> Result<String, FormatError> {
    let value = parse::toml(content)?;
    serialize::to_xml(&value)
}

/// Convert TOON string to XML.
pub fn toon_to_xml(content: &str) -> Result<String, FormatError> {
    let value = parse::toon(content)?;
    serialize::to_xml(&value)
}

// =============================================================================
// Null stripping
// =============================================================================

/// Recursively remove null values from a JSON Value.
///
/// - Object keys with null values are removed
/// - Null elements in arrays are removed
/// - All other values pass through unchanged
pub fn strip_nulls(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let cleaned: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, strip_nulls(v)))
                .collect();
            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(arr) => {
            let cleaned: Vec<serde_json::Value> = arr
                .into_iter()
                .filter(|v| !v.is_null())
                .map(strip_nulls)
                .collect();
            serde_json::Value::Array(cleaned)
        }
        other => other,
    }
}

// =============================================================================
// Educational TOML conversion
// =============================================================================

/// Convert a Value to TOML with lazy educational diagnostics.
///
/// Fast path first: try serialization. On failure, analyze for known
/// issues and return actionable error messages.
fn value_to_toml(value: &serde_json::Value) -> Result<String, FormatError> {
    match serialize::to_toml(value) {
        Ok(result) => Ok(result),
        Err(_raw_err) => {
            // Analyze for educational diagnostics
            let mut issues = Vec::new();

            // Check for null values
            let nulls = find_null_paths(value, "");
            if !nulls.is_empty() {
                issues.push(format!(
                    "{} null value(s) at: {}\n  Solution: Remove fields or use empty string \"\"",
                    nulls.len(),
                    nulls.join(", ")
                ));
            }

            // Check for top-level array
            if value.is_array() {
                issues.push(
                    "Top-level array — TOML requires a root table (object).\n  \
                     Solution: Wrap in an object: {\"items\": [...]}"
                        .to_string(),
                );
            }

            if issues.is_empty() {
                Err(FormatError::Conversion(
                    "TOML serialization failed".to_string(),
                ))
            } else {
                Err(FormatError::Educational {
                    message: issues.join("\n\n"),
                })
            }
        }
    }
}

/// Recursively find paths to null values in a JSON Value.
fn find_null_paths(value: &serde_json::Value, prefix: &str) -> Vec<String> {
    let mut paths = Vec::new();
    match value {
        serde_json::Value::Null => {
            paths.push(if prefix.is_empty() {
                "(root)".to_string()
            } else {
                prefix.to_string()
            });
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                paths.extend(find_null_paths(v, &path));
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let path = format!("{}[{}]", prefix, i);
                paths.extend(find_null_paths(v, &path));
            }
        }
        _ => {}
    }
    paths
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_to_yaml() {
        let result = json_to_yaml(r#"{"key": "value"}"#).unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_yaml_to_json() {
        let result = yaml_to_json("key: value").unwrap();
        assert!(result.contains("\"key\""));
    }

    #[test]
    fn test_toml_to_json_simple() {
        let toml = r#"
name = "test"
count = 42
active = true
"#;
        let json = toml_to_json(toml).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["name"], "test");
        assert_eq!(parsed["count"], 42);
        assert_eq!(parsed["active"], true);
    }

    #[test]
    fn test_json_to_toml_simple() {
        let json = r#"{"name": "test", "count": 42, "active": true}"#;
        let toml_str = json_to_toml(json).unwrap();
        assert!(toml_str.contains("name = \"test\""));
        assert!(toml_str.contains("count = 42"));
        assert!(toml_str.contains("active = true"));
    }

    #[test]
    fn test_json_to_toon() {
        let result = json_to_toon(r#"{"key": "value"}"#).unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_toon_to_json() {
        let result = toon_to_json("key: value").unwrap();
        assert!(result.contains("\"key\""));
    }

    #[test]
    fn test_toml_to_toon() {
        let result = toml_to_toon("key = \"value\"").unwrap();
        assert!(result.contains("key:"));
    }

    #[test]
    fn test_json_to_toml_null_educational() {
        let err = json_to_toml(r#"{"email": null}"#).unwrap_err();
        match err {
            FormatError::Educational { message } => {
                assert!(message.contains("null value"));
                assert!(message.contains("Solution:"));
            }
            _ => panic!("Expected educational error, got: {:?}", err),
        }
    }

    #[test]
    fn test_json_to_toml_top_level_array() {
        let err = json_to_toml("[1, 2, 3]").unwrap_err();
        match err {
            FormatError::Educational { message } => {
                assert!(message.contains("Top-level array"));
                assert!(message.contains("Solution:"));
            }
            _ => panic!("Expected educational error, got: {:?}", err),
        }
    }

    #[test]
    fn test_roundtrip_json_yaml_json() {
        let original = r#"{"key": "value", "number": 42}"#;
        let yaml = json_to_yaml(original).unwrap();
        let back = yaml_to_json(&yaml).unwrap();
        let orig_val: serde_json::Value = serde_json::from_str(original).unwrap();
        let back_val: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(orig_val, back_val);
    }

    #[test]
    fn test_roundtrip_json_toml_json() {
        let original = r#"{"name": "test", "nested": {"key": "value"}, "list": [1, 2, 3]}"#;
        let toml_str = json_to_toml(original).unwrap();
        let back = toml_to_json(&toml_str).unwrap();
        let orig_val: serde_json::Value = serde_json::from_str(original).unwrap();
        let back_val: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(orig_val, back_val);
    }

    #[test]
    fn test_roundtrip_json_toon_json() {
        let original = r#"{"name": "test", "count": 42}"#;
        let toon = json_to_toon(original).unwrap();
        let back = toon_to_json(&toon).unwrap();
        let orig_val: serde_json::Value = serde_json::from_str(original).unwrap();
        let back_val: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(orig_val, back_val);
    }

    #[test]
    fn test_strip_nulls_object() {
        let value = serde_json::json!({"name": "test", "email": null, "age": 30});
        let stripped = strip_nulls(value);
        assert_eq!(stripped, serde_json::json!({"name": "test", "age": 30}));
    }

    #[test]
    fn test_strip_nulls_nested() {
        let value = serde_json::json!({"a": {"b": null, "c": 1}, "d": null});
        let stripped = strip_nulls(value);
        assert_eq!(stripped, serde_json::json!({"a": {"c": 1}}));
    }

    #[test]
    fn test_strip_nulls_array() {
        let value = serde_json::json!({"items": [1, null, 3]});
        let stripped = strip_nulls(value);
        assert_eq!(stripped, serde_json::json!({"items": [1, 3]}));
    }

    #[test]
    fn test_strip_nulls_clean_passthrough() {
        let value = serde_json::json!({"name": "test", "count": 42});
        let stripped = strip_nulls(value.clone());
        assert_eq!(stripped, value);
    }

    #[test]
    fn test_roundtrip_toml_json_toml() {
        let original = r#"
[metadata]
name = "test-agent"
version = "1.0"

[settings]
enabled = true
count = 5

[[items]]
id = 1
label = "first"

[[items]]
id = 2
label = "second"
"#;
        let json = toml_to_json(original).unwrap();
        let back = json_to_toml(&json).unwrap();
        let orig_val: toml::Value = toml::from_str(original).unwrap();
        let back_val: toml::Value = toml::from_str(&back).unwrap();
        assert_eq!(orig_val, back_val);
    }

    #[test]
    fn test_nested_arrays_of_tables() {
        let toml = r#"
[[servers]]
name = "alpha"
port = 8080

[[servers]]
name = "beta"
port = 9090
"#;
        let json = toml_to_json(toml).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["servers"][0]["name"], "alpha");
        assert_eq!(parsed["servers"][1]["port"], 9090);
    }
}
