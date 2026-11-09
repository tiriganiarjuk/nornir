//! Nornir error types and educational formatting for pipeline validation.
//!
//! Adopts the educational error philosophy from `validator_errors`:
//! - **What** is wrong (rule violated)
//! - **Where** the problem is (JSON path)
//! - **What was found** (the invalid value)
//! - **What was expected** (the schema constraint)
//! - **How to fix it** (actionable suggestion)

use jsonschema::ValidationError;
use serde::Serialize;
use thiserror::Error;

// =============================================================================
// Top-level error enum
// =============================================================================

/// Top-level error type for all Nornir operations.
#[derive(Debug, Error)]
pub enum NornirError {
    #[error("Format error: {0}")]
    Format(#[from] FormatError),

    #[error("Schema error: {0}")]
    Schema(#[from] SchemaError),

    #[error("Path error: {0}")]
    Path(#[from] PathError),

    #[error("IO error: {0}")]
    Io(#[from] IoError),
}

// =============================================================================
// Specific error types
// =============================================================================

/// Format parse, serialize, or conversion errors.
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("TOML parse error: {0}")]
    TomlParse(String),

    #[error("JSON parse error: {0}")]
    JsonParse(String),

    #[error("YAML parse error: {0}")]
    YamlParse(String),

    #[error("TOON parse error: {0}")]
    ToonParse(String),

    #[error("TOMLX parse error: {0}")]
    TomlxParse(String),

    #[error("XML parse error: {0}")]
    XmlParse(String),

    #[error("Markdown parse error: {0}")]
    MarkdownParse(String),

    #[error("Conversion error: {0}")]
    Conversion(String),

    #[error("{message}")]
    Educational { message: String },
}

/// Schema validation errors — invalid input or schema compilation failures.
#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("Invalid JSON input: {0}")]
    InvalidInput(String),

    #[error("Schema compilation error: {0}")]
    SchemaCompilation(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

/// Missing filesystem paths.
#[derive(Debug, Error)]
#[error("Missing paths: {}", format_missing_paths(&self.missing))]
pub struct PathError {
    pub missing: Vec<PathField>,
}

/// A path field extracted from data via schema annotation.
#[derive(Debug, Clone, Serialize)]
pub struct PathField {
    /// JSON pointer to the field (e.g., "/security/schemas/security_input_schema")
    pub json_pointer: String,
    /// The path value from the data
    pub value: String,
}

/// IO read/write failures.
#[derive(Debug, Error)]
pub enum IoError {
    #[error("Cannot read '{path}': {message}")]
    ReadFailed { path: String, message: String },

    #[error("Cannot write '{path}': {message}")]
    WriteFailed { path: String, message: String },

    #[error("Cannot read stdin: {0}")]
    StdinFailed(String),
}

// =============================================================================
// ValidationIssue — educational error context
// =============================================================================

/// A single validation issue with educational context.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    /// JSON path to the invalid value (e.g., "/users/0/email")
    pub path: String,
    /// What rule was violated
    pub rule: String,
    /// The invalid value (truncated if too long)
    pub found: String,
    /// What was expected
    pub expected: String,
    /// Actionable fix suggestion
    pub fix: String,
}

impl ValidationIssue {
    /// Create a `ValidationIssue` from a `jsonschema::ValidationError`.
    pub fn from_schema_error(error: &ValidationError) -> Self {
        let path = error.instance_path.to_string();
        let path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };

        let (rule, expected, fix) = categorize_error(error);
        let found = truncate_value(&error.instance.to_string(), 50);

        Self {
            path,
            rule,
            found,
            expected,
            fix,
        }
    }
}

// =============================================================================
// Error categorization (adopted from validator_errors)
// =============================================================================

/// Categorize a schema validation error and provide educational context.
fn categorize_error(error: &ValidationError) -> (String, String, String) {
    let msg = error.to_string();

    if msg.contains("is not of type") {
        return categorize_type_mismatch(&msg);
    }
    if msg.contains("is a required property") {
        return categorize_missing_required(&msg);
    }
    if msg.contains("Additional properties are not allowed") {
        return categorize_additional_property();
    }
    if msg.contains("is not one of") {
        return categorize_invalid_enum(&msg);
    }
    if msg.contains("does not match") {
        return categorize_pattern_mismatch(&msg);
    }
    if msg.contains("is less than") || msg.contains("is greater than") {
        return categorize_range_violation(&msg);
    }
    if msg.contains("is shorter than") || msg.contains("is longer than") {
        return categorize_length_violation();
    }
    if msg.contains("has less than") || msg.contains("has more than") {
        return categorize_array_length();
    }
    if msg.contains("is not a")
        && (msg.contains("email") || msg.contains("uri") || msg.contains("date"))
    {
        return categorize_format_invalid();
    }
    if msg.contains("not valid under any of the schemas listed in the") {
        return categorize_variant_mismatch(&msg, error);
    }

    ("validation_failed".into(), msg, "Check the value against the schema requirements".into())
}

fn categorize_type_mismatch(error_message: &str) -> (String, String, String) {
    let expected = extract_expected_type(error_message);
    let fix = format!("Change the value to match the expected type: {}", expected);
    ("type_mismatch".into(), expected, fix)
}

fn categorize_missing_required(error_message: &str) -> (String, String, String) {
    let prop = extract_property_name(error_message);
    (
        "missing_required".into(),
        format!("Property '{}' must be present", prop),
        format!("Add the required property '{}'", prop),
    )
}

fn categorize_additional_property() -> (String, String, String) {
    (
        "additional_property".into(),
        "No additional properties allowed".into(),
        "Remove the unexpected property, or update the schema to allow it".into(),
    )
}

fn categorize_invalid_enum(error_message: &str) -> (String, String, String) {
    let expected = extract_enum_values(error_message);
    let fix = format!("Use one of the allowed values: {}", expected);
    ("invalid_enum".into(), expected, fix)
}

fn categorize_pattern_mismatch(error_message: &str) -> (String, String, String) {
    let pattern = extract_pattern(error_message);
    (
        "pattern_mismatch".into(),
        format!("Value must match pattern: {}", pattern),
        format!(
            "The value does not match the required format.\n\
             Pattern: {}\n\
             Review the schema definition for this field to see valid examples.",
            pattern
        ),
    )
}

fn categorize_range_violation(error_message: &str) -> (String, String, String) {
    let expected = extract_range(error_message);
    let fix = format!("Adjust the value to be within the allowed range: {}", expected);
    ("range_violation".into(), expected, fix)
}

fn categorize_length_violation() -> (String, String, String) {
    (
        "length_violation".into(),
        "String length out of bounds".into(),
        "Adjust the string length to meet requirements".into(),
    )
}

fn categorize_array_length() -> (String, String, String) {
    (
        "array_length".into(),
        "Array length out of bounds".into(),
        "Adjust the number of array items".into(),
    )
}

fn categorize_format_invalid() -> (String, String, String) {
    (
        "format_invalid".into(),
        "Value must match the specified format".into(),
        "Ensure the value matches the required format".into(),
    )
}

fn categorize_variant_mismatch(error_message: &str, error: &ValidationError) -> (String, String, String) {
    let keyword = if error_message.contains("'oneOf'") { "oneOf" } else { "anyOf" };
    let schema_path = format_schema_path(error);
    (
        format!("{}_mismatch", keyword),
        format!("Value must match one of the allowed variants ({})", keyword),
        format!(
            "The value doesn't match any of the allowed forms for this field.\n\
             Schema location: {}\n\
             Check the schema to see what variants are accepted.\n\
             Common causes: wrong string format, missing required fields in an object, \
             or using an include reference where inline content is expected (or vice versa).",
            schema_path
        ),
    )
}

// =============================================================================
// Formatting helpers
// =============================================================================

fn extract_expected_type(message: &str) -> String {
    if let Some(start) = message.find("type \"") {
        let rest = &message[start + 6..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    if let Some(start) = message.find("type '") {
        let rest = &message[start + 6..];
        if let Some(end) = rest.find('\'') {
            return rest[..end].to_string();
        }
    }
    "unknown".to_string()
}

fn extract_property_name(message: &str) -> String {
    if let Some(start) = message.find('"') {
        let rest = &message[start + 1..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    if let Some(start) = message.find('\'') {
        let rest = &message[start + 1..];
        if let Some(end) = rest.find('\'') {
            return rest[..end].to_string();
        }
    }
    "unknown".to_string()
}

fn extract_enum_values(message: &str) -> String {
    if let Some(start) = message.find('[') {
        if let Some(end) = message.find(']') {
            return message[start..=end].to_string();
        }
    }
    "allowed values".to_string()
}

fn extract_range(message: &str) -> String {
    message.to_string()
}

/// Extract the pattern regex from a "does not match" error message.
/// Format: `"value" does not match "^pattern$"`
fn extract_pattern(message: &str) -> String {
    // The pattern is in the last quoted string in the message
    if let Some(idx) = message.rfind('"') {
        let before = &message[..idx];
        if let Some(start) = before.rfind('"') {
            return message[start + 1..idx].to_string();
        }
    }
    "unknown pattern".to_string()
}

/// Format the schema path from a validation error for diagnostic context.
fn format_schema_path(error: &ValidationError) -> String {
    let path = error.schema_path.to_string();
    if path.is_empty() { "/".to_string() } else { path }
}

/// Truncate a value string for display (UTF-8 safe).
pub fn truncate_value(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        value.to_string()
    } else {
        let truncated: String = value.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// Format all issues into a human-readable educational message.
pub fn format_educational_message(schema_name: &str, issues: &[ValidationIssue]) -> String {
    if issues.is_empty() {
        return format!("{} validation passed", schema_name);
    }

    let mut lines = vec![
        format!(
            "{} validation failed — {} issue(s) found:",
            schema_name,
            issues.len()
        ),
        String::new(),
    ];

    for (i, issue) in issues.iter().enumerate() {
        lines.push(format!("  {}. Path: {}", i + 1, issue.path));
        lines.push(format!("     Rule: {}", issue.rule));
        lines.push(format!("     Found: {}", issue.found));
        lines.push(format!("     Expected: {}", issue.expected));
        lines.push(format!("     Fix: {}", issue.fix));
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Format path verification errors into a human-readable message.
pub fn format_path_errors(missing: &[PathField]) -> String {
    if missing.is_empty() {
        return "All referenced paths exist".to_string();
    }

    let mut lines = vec![format!(
        "Path verification failed — {} missing path(s):",
        missing.len()
    )];
    lines.push(String::new());

    for (i, field) in missing.iter().enumerate() {
        lines.push(format!("  {}. Field: {}", i + 1, field.json_pointer));
        lines.push(format!("     Path: {}", field.value));
        lines.push("     Fix: Create the file or correct the path".to_string());
        lines.push(String::new());
    }

    lines.join("\n")
}

fn format_missing_paths(missing: &[PathField]) -> String {
    missing
        .iter()
        .map(|p| format!("{} ({})", p.value, p.json_pointer))
        .collect::<Vec<_>>()
        .join(", ")
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_value_short() {
        assert_eq!(truncate_value("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_value_long() {
        let result = truncate_value("a very long string that exceeds the limit", 10);
        assert_eq!(result, "a very lon...");
    }

    #[test]
    fn test_truncate_value_multibyte_utf8() {
        let s = "MUST produce exactly one sentence per exchange — no more, no less";
        let result = truncate_value(s, 50);
        assert!(result.ends_with("..."));
        assert!(result.len() <= s.len());
    }

    #[test]
    fn test_format_educational_message_empty() {
        let msg = format_educational_message("agent-definition", &[]);
        assert_eq!(msg, "agent-definition validation passed");
    }

    #[test]
    fn test_format_educational_message_with_issues() {
        let issues = vec![ValidationIssue {
            path: "/name".to_string(),
            rule: "type_mismatch".to_string(),
            found: "123".to_string(),
            expected: "string".to_string(),
            fix: "Change the value to match the expected type: string".to_string(),
        }];
        let msg = format_educational_message("agent-definition", &issues);
        assert!(msg.contains("agent-definition validation failed"));
        assert!(msg.contains("1 issue(s) found"));
        assert!(msg.contains("Path: /name"));
        assert!(msg.contains("Rule: type_mismatch"));
        assert!(msg.contains("Fix:"));
    }

    #[test]
    fn test_validation_issue_from_schema_error() {
        let schema: serde_json::Value =
            serde_json::from_str(r#"{"type": "string"}"#).unwrap();
        let validator = jsonschema::Validator::new(&schema).unwrap();
        let data: serde_json::Value = serde_json::from_str("123").unwrap();

        let errors: Vec<_> = validator.iter_errors(&data).collect();
        assert!(!errors.is_empty());

        let issue = ValidationIssue::from_schema_error(&errors[0]);
        assert_eq!(issue.rule, "type_mismatch");
        assert_eq!(issue.path, "/");
    }

    #[test]
    fn test_required_property_error() {
        let schema: serde_json::Value = serde_json::from_str(
            r#"{"type": "object", "required": ["name", "age"]}"#,
        )
        .unwrap();
        let validator = jsonschema::Validator::new(&schema).unwrap();
        let data: serde_json::Value = serde_json::from_str("{}").unwrap();

        let errors: Vec<_> = validator.iter_errors(&data).collect();
        assert!(!errors.is_empty());

        let issue = ValidationIssue::from_schema_error(&errors[0]);
        assert_eq!(issue.rule, "missing_required");
        assert_ne!(
            issue.expected,
            "Property 'unknown' must be present",
            "Property name extraction failed"
        );
    }

    #[test]
    fn test_format_path_errors_empty() {
        assert_eq!(format_path_errors(&[]), "All referenced paths exist");
    }

    #[test]
    fn test_format_path_errors_with_missing() {
        let missing = vec![PathField {
            json_pointer: "/security/schemas/input".to_string(),
            value: "/path/to/missing.json".to_string(),
        }];
        let msg = format_path_errors(&missing);
        assert!(msg.contains("1 missing path(s)"));
        assert!(msg.contains("/security/schemas/input"));
        assert!(msg.contains("/path/to/missing.json"));
    }

    #[test]
    fn test_nornir_error_display() {
        let err = NornirError::Format(FormatError::TomlParse("bad toml".into()));
        assert!(err.to_string().contains("bad toml"));

        let err = NornirError::Path(PathError {
            missing: vec![PathField {
                json_pointer: "/foo".into(),
                value: "/no/such/file".into(),
            }],
        });
        assert!(err.to_string().contains("/no/such/file"));
    }
}
