//! Type definitions for .tomlx parsing.
//!
//! Defines the AST and intermediate representations for parsing .tomlx files.

use std::collections::HashMap;

use super::units::{lookup_target, UnitTarget};

/// Target processing mode for a section.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetFamily {
    /// Unit conversion to one unit of a family (seconds, bytes, meters, etc.)
    Unit(UnitTarget),
    /// Path expansion mode.
    Path,
}

/// Parse a target string into a TargetFamily.
pub fn parse_target(target: &str) -> Option<TargetFamily> {
    let target = target.trim().to_lowercase();
    if target == "path" {
        return Some(TargetFamily::Path);
    }
    lookup_target(&target).map(TargetFamily::Unit)
}

/// Section annotation parsed from a section header comment.
#[derive(Debug, Clone)]
pub struct SectionAnnotation {
    /// Full section path (e.g., "parent.child")
    pub section_path: String,
    /// Line number in source (1-indexed)
    pub line: usize,
    /// The target mode for this section
    pub target: Option<TargetFamily>,
    /// Type annotation for this section
    pub type_annotation: Option<TypeAnnotation>,
    /// For path mode: base definitions (first section only)
    pub path_bases: HashMap<String, String>,
    /// For path mode: expansion mode
    pub expand_mode: ExpandMode,
}

/// Path expansion modes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ExpandMode {
    /// Expand ~ to home directory (default)
    #[default]
    User,
    /// Expand $VAR and ${VAR}
    Env,
    /// Both user and env expansion
    All,
    /// No expansion
    None,
}

impl ExpandMode {
    /// Parse expand mode from string.
    pub fn from_str(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "user" => Some(ExpandMode::User),
            "env" => Some(ExpandMode::Env),
            "all" => Some(ExpandMode::All),
            "none" => Some(ExpandMode::None),
            _ => None,
        }
    }

    /// Convert to string for output.
    pub fn as_str(&self) -> &'static str {
        match self {
            ExpandMode::User => "user",
            ExpandMode::Env => "env",
            ExpandMode::All => "all",
            ExpandMode::None => "none",
        }
    }
}

/// Type annotation for JSON Schema generation.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotation {
    Int,
    Float,
    Str,
    Bool,
    List(Box<TypeAnnotation>),
}

impl TypeAnnotation {
    /// Convert to string representation for metadata output.
    pub fn as_str(&self) -> String {
        match self {
            TypeAnnotation::Int => "int".to_string(),
            TypeAnnotation::Float => "float".to_string(),
            TypeAnnotation::Str => "str".to_string(),
            TypeAnnotation::Bool => "bool".to_string(),
            TypeAnnotation::List(inner) => format!("list[{}]", inner.as_str()),
        }
    }
}

/// Parse a type annotation string.
pub fn parse_type_annotation(type_str: &str) -> Option<TypeAnnotation> {
    let type_str = type_str.trim().to_lowercase();

    if type_str.starts_with("list[") && type_str.ends_with(']') {
        let inner = type_str[5..type_str.len() - 1].trim();
        let inner_type = parse_scalar_type(inner)?;
        return Some(TypeAnnotation::List(Box::new(inner_type)));
    }

    parse_scalar_type(&type_str)
}

fn parse_scalar_type(type_str: &str) -> Option<TypeAnnotation> {
    match type_str {
        "int" | "integer" => Some(TypeAnnotation::Int),
        "float" | "number" => Some(TypeAnnotation::Float),
        "str" | "string" => Some(TypeAnnotation::Str),
        "bool" | "boolean" => Some(TypeAnnotation::Bool),
        _ => None,
    }
}

/// Field annotation parsed from a field comment.
#[derive(Debug, Clone)]
pub struct FieldAnnotation {
    /// Full field path (e.g., "ttl.session")
    pub field_path: String,
    /// Line number in source (1-indexed)
    pub line: usize,
    /// The annotation type
    pub annotation_type: FieldAnnotationType,
    /// Human comment (after ` // `, if present)
    pub human_comment: Option<String>,
}

/// Type of field annotation.
#[derive(Debug, Clone)]
pub enum FieldAnnotationType {
    /// Unit annotation: `# unit=minutes`
    Unit(String),
    /// Path reference: `# path=base`
    Path(String),
}

/// Registry of path bases for a document.
#[derive(Debug, Clone, Default)]
pub struct PathRegistry {
    /// User-defined bases from first path section.
    pub user_defined: HashMap<String, String>,
    /// Expansion mode for the document.
    pub expand_mode: ExpandMode,
    /// Line where bases were declared.
    pub declared_at: Option<usize>,
}

impl PathRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_builtin(reference: &str) -> bool {
        matches!(reference, "home" | "absolute" | "config")
    }

    pub fn is_valid_reference(&self, reference: &str) -> bool {
        Self::is_builtin(reference) || self.user_defined.contains_key(reference)
    }

    pub fn get_base(&self, reference: &str) -> Option<&str> {
        self.user_defined.get(reference).map(|s| s.as_str())
    }
}

/// Reserved keys that cannot be used as path base names.
pub const RESERVED_PATH_KEYS: &[&str] = &[
    "target",
    "type",
    "expand",
    "schema",
    "section_types",
    "section_units",
    "section_paths",
    "home",
    "absolute",
    "config",
];

/// Output structure for parsed .tomlx.
#[derive(Debug, Clone)]
pub struct TomlxOutput {
    /// The converted data (sections with values).
    pub data: serde_json::Value,
    /// Generated JSON Schema (if any type annotations present).
    pub schema: Option<serde_json::Value>,
    /// Section type annotations metadata.
    pub section_types: HashMap<String, String>,
    /// Section units metadata.
    pub section_units: HashMap<String, String>,
    /// Section paths metadata.
    pub section_paths: HashMap<String, SectionPathInfo>,
}

/// Path metadata for a section.
#[derive(Debug, Clone)]
pub struct SectionPathInfo {
    pub bases: HashMap<String, String>,
    pub expand: ExpandMode,
}

impl TomlxOutput {
    /// Convert to JSON with metadata sections.
    pub fn to_json(&self) -> serde_json::Value {
        let mut output = self.data.clone();

        if let Some(ref schema) = self.schema {
            if let serde_json::Value::Object(ref mut map) = output {
                map.insert("schema".to_string(), schema.clone());
            }
        }

        if !self.section_types.is_empty() {
            if let serde_json::Value::Object(ref mut map) = output {
                map.insert(
                    "section_types".to_string(),
                    serde_json::to_value(&self.section_types).unwrap_or_default(),
                );
            }
        }

        if !self.section_units.is_empty() {
            if let serde_json::Value::Object(ref mut map) = output {
                map.insert(
                    "section_units".to_string(),
                    serde_json::to_value(&self.section_units).unwrap_or_default(),
                );
            }
        }

        if !self.section_paths.is_empty() {
            if let serde_json::Value::Object(ref mut map) = output {
                let paths_value: HashMap<String, serde_json::Value> = self
                    .section_paths
                    .iter()
                    .map(|(k, v)| {
                        let mut section_map: HashMap<String, serde_json::Value> = v
                            .bases
                            .iter()
                            .map(|(bk, bv)| (bk.clone(), serde_json::Value::String(bv.clone())))
                            .collect();
                        section_map.insert(
                            "expand".to_string(),
                            serde_json::Value::String(v.expand.as_str().to_string()),
                        );
                        (k.clone(), serde_json::to_value(section_map).unwrap_or_default())
                    })
                    .collect();
                map.insert(
                    "section_paths".to_string(),
                    serde_json::to_value(paths_value).unwrap_or_default(),
                );
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_target() {
        let unit_target_name = |target: &str| match parse_target(target) {
            Some(TargetFamily::Unit(unit_target)) => Some(unit_target.name),
            _ => None,
        };
        assert_eq!(unit_target_name("seconds"), Some("seconds"));
        assert_eq!(unit_target_name("ms"), Some("milliseconds"));
        assert_eq!(unit_target_name("bytes"), Some("bytes"));
        assert_eq!(unit_target_name("meters"), Some("meters"));
        assert_eq!(unit_target_name("metres"), Some("meters"));
        assert_eq!(unit_target_name("m"), Some("meters"));
        assert_eq!(unit_target_name("KG"), Some("kilograms"));
        assert!(matches!(parse_target("path"), Some(TargetFamily::Path)));
        assert!(parse_target("unknown").is_none());
    }

    #[test]
    fn test_expand_mode() {
        assert_eq!(ExpandMode::from_str("user"), Some(ExpandMode::User));
        assert_eq!(ExpandMode::from_str("ENV"), Some(ExpandMode::Env));
        assert_eq!(ExpandMode::from_str("all"), Some(ExpandMode::All));
        assert_eq!(ExpandMode::from_str("none"), Some(ExpandMode::None));
        assert_eq!(ExpandMode::from_str("invalid"), None);
    }

    #[test]
    fn test_parse_type_annotation_scalars() {
        assert_eq!(parse_type_annotation("int"), Some(TypeAnnotation::Int));
        assert_eq!(parse_type_annotation("integer"), Some(TypeAnnotation::Int));
        assert_eq!(parse_type_annotation("float"), Some(TypeAnnotation::Float));
        assert_eq!(parse_type_annotation("str"), Some(TypeAnnotation::Str));
        assert_eq!(parse_type_annotation("bool"), Some(TypeAnnotation::Bool));
    }

    #[test]
    fn test_parse_type_annotation_lists() {
        assert_eq!(
            parse_type_annotation("list[int]"),
            Some(TypeAnnotation::List(Box::new(TypeAnnotation::Int)))
        );
        assert_eq!(
            parse_type_annotation("list[str]"),
            Some(TypeAnnotation::List(Box::new(TypeAnnotation::Str)))
        );
    }

    #[test]
    fn test_parse_type_annotation_invalid() {
        assert_eq!(parse_type_annotation("dict"), None);
        assert_eq!(parse_type_annotation("list"), None);
        assert_eq!(parse_type_annotation("list[]"), None);
    }

    #[test]
    fn test_type_annotation_as_str() {
        assert_eq!(TypeAnnotation::Int.as_str(), "int");
        assert_eq!(TypeAnnotation::List(Box::new(TypeAnnotation::Str)).as_str(), "list[str]");
    }

    #[test]
    fn test_path_registry() {
        let mut registry = PathRegistry::new();
        registry.user_defined.insert("base".to_string(), "/app/".to_string());
        assert!(registry.is_valid_reference("base"));
        assert!(registry.is_valid_reference("home"));
        assert!(!registry.is_valid_reference("undefined"));
    }
}
