//! Educational error diagnostics for .tomlx parsing.
//!
//! Error format:
//! ```text
//! ERROR: <file>:<line>
//!   <What is wrong>
//!   Rule: <The constraint violated>
//!   Fix: <Actionable guidance>
//! ```

use std::fmt;

use super::angle_parse::AngleNotation;
use super::units::supported_units_summary;

#[derive(Debug, Clone)]
pub struct Location {
    pub file: Option<String>,
    pub line: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}:{}", file, self.line),
            None => write!(f, "line {}", self.line),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MalformedSectionAnnotation {
    pub location: Location,
    pub section_name: String,
    pub raw_annotation: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct MalformedFieldAnnotation {
    pub location: Location,
    pub field_name: String,
    pub raw_annotation: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct OrphanUnitAnnotation {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub section_name: String,
}

#[derive(Debug, Clone)]
pub struct OrphanPathAnnotation {
    pub location: Location,
    pub field_name: String,
    pub path_ref: String,
    pub section_name: String,
}

#[derive(Debug, Clone)]
pub struct MissingFieldAnnotation {
    pub location: Location,
    pub field_name: String,
    pub section_name: String,
    pub expected_type: String,
}

#[derive(Debug, Clone)]
pub struct BaseRedeclaration {
    pub location: Location,
    pub section_name: String,
    pub base_name: String,
    pub original_line: usize,
    pub original_value: String,
}

#[derive(Debug, Clone)]
pub struct UnknownUnit {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IncompatibleUnitFamily {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub unit_family: String,
    /// The target a section holding this unit's family converts to.
    pub unit_family_target: String,
    pub target: String,
    pub target_family: String,
}

/// A value its unit annotation cannot convert.
#[derive(Debug, Clone)]
pub struct UnconvertibleValue {
    pub location: Location,
    pub field_name: String,
    pub unit: String,
    pub found: String,
    pub reason: String,
}

/// A value its path annotation cannot expand.
#[derive(Debug, Clone)]
pub struct UnexpandablePath {
    pub location: Location,
    pub field_name: String,
    pub reference: String,
    pub found: String,
    pub reason: String,
}

/// An annotation on a section or a field that is not in the file's data where its name says.
#[derive(Debug, Clone)]
pub struct UnresolvedAnnotation {
    pub location: Location,
    /// What carries the annotation, as a message names it: `Section [a.b]`, `Field 'a.b.c'`.
    pub annotated: String,
}

#[derive(Debug, Clone)]
pub struct UndefinedPathReference {
    pub location: Location,
    pub field_name: String,
    pub reference: String,
    pub defined_refs: Vec<String>,
}

/// Collection of all .tomlx parsing issues.
#[derive(Debug, Default)]
pub struct TomlxIssues {
    pub file: Option<String>,
    pub malformed_section_annotations: Vec<MalformedSectionAnnotation>,
    pub malformed_field_annotations: Vec<MalformedFieldAnnotation>,
    pub orphan_unit_annotations: Vec<OrphanUnitAnnotation>,
    pub orphan_path_annotations: Vec<OrphanPathAnnotation>,
    pub missing_field_annotations: Vec<MissingFieldAnnotation>,
    pub base_redeclarations: Vec<BaseRedeclaration>,
    pub unknown_units: Vec<UnknownUnit>,
    pub incompatible_unit_families: Vec<IncompatibleUnitFamily>,
    pub unconvertible_values: Vec<UnconvertibleValue>,
    pub unexpandable_paths: Vec<UnexpandablePath>,
    pub unresolved_annotations: Vec<UnresolvedAnnotation>,
    pub undefined_path_references: Vec<UndefinedPathReference>,
}

impl TomlxIssues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(file: impl Into<String>) -> Self {
        Self { file: Some(file.into()), ..Self::default() }
    }

    pub fn loc(&self, line: usize) -> Location {
        Location { file: self.file.clone(), line }
    }

    pub fn has_issues(&self) -> bool {
        !self.malformed_section_annotations.is_empty()
            || !self.malformed_field_annotations.is_empty()
            || !self.orphan_unit_annotations.is_empty()
            || !self.orphan_path_annotations.is_empty()
            || !self.missing_field_annotations.is_empty()
            || !self.base_redeclarations.is_empty()
            || !self.unknown_units.is_empty()
            || !self.incompatible_unit_families.is_empty()
            || !self.unconvertible_values.is_empty()
            || !self.unexpandable_paths.is_empty()
            || !self.unresolved_annotations.is_empty()
            || !self.undefined_path_references.is_empty()
    }

    pub fn count(&self) -> usize {
        self.malformed_section_annotations.len()
            + self.malformed_field_annotations.len()
            + self.orphan_unit_annotations.len()
            + self.orphan_path_annotations.len()
            + self.missing_field_annotations.len()
            + self.base_redeclarations.len()
            + self.unknown_units.len()
            + self.incompatible_unit_families.len()
            + self.unconvertible_values.len()
            + self.unexpandable_paths.len()
            + self.unresolved_annotations.len()
            + self.undefined_path_references.len()
    }

    /// Format all issues as an educational error message.
    pub fn format(&self) -> String {
        let mut sections = Vec::new();

        sections.extend(self.malformed_section_annotations.iter().map(format_malformed_section));
        sections.extend(self.malformed_field_annotations.iter().map(format_malformed_field));
        sections.extend(self.orphan_unit_annotations.iter().map(format_orphan_unit));
        sections.extend(self.orphan_path_annotations.iter().map(format_orphan_path));
        sections.extend(self.missing_field_annotations.iter().map(format_missing_field));
        sections.extend(self.base_redeclarations.iter().map(format_base_redeclaration));
        sections.extend(self.unknown_units.iter().map(format_unknown_unit));
        sections.extend(self.incompatible_unit_families.iter().map(format_incompatible_family));
        sections.extend(self.unconvertible_values.iter().map(format_unconvertible_value));
        sections.extend(self.unexpandable_paths.iter().map(format_unexpandable_path));
        sections.extend(self.unresolved_annotations.iter().map(format_unresolved_annotation));
        sections.extend(self.undefined_path_references.iter().map(format_undefined_path_ref));

        if sections.is_empty() {
            "No issues found.".into()
        } else {
            sections.join("\n\n---\n\n")
        }
    }
}

// -------------------------------------------------------------------------
// Per-issue-type formatters
// -------------------------------------------------------------------------

fn format_malformed_section(issue: &MalformedSectionAnnotation) -> String {
    format!(
        "ERROR: {}\n  \
         Section [{}] has malformed annotation.\n\n  \
         Found: {}\n  \
         Problem: {}\n\n  \
         Fix: Use format: [section]  # target=seconds\n       \
              Or: [section]  # target=path, base=~/app/",
        issue.location, issue.section_name, issue.raw_annotation, issue.reason
    )
}

fn format_malformed_field(issue: &MalformedFieldAnnotation) -> String {
    format!(
        "ERROR: {}\n  \
         Field '{}' has malformed annotation.\n\n  \
         Found: {}\n  \
         Problem: {}\n\n  \
         Fix: Use format: field = value  # unit=seconds\n       \
              Or: field = \"path/\"  # path=base",
        issue.location, issue.field_name, issue.raw_annotation, issue.reason
    )
}

fn format_orphan_unit(issue: &OrphanUnitAnnotation) -> String {
    format!(
        "ERROR: {}\n  \
         Field '{}' has unit annotation but section [{}] has no target.\n\n  \
         Rule: Unit annotations require a section target declaration.\n\n  \
         Fix: Add target to section: [{}]  # target=seconds\n       \
              Or remove the annotation if no conversion needed.",
        issue.location, issue.field_name, issue.section_name, issue.section_name
    )
}

fn format_orphan_path(issue: &OrphanPathAnnotation) -> String {
    format!(
        "ERROR: {}\n  \
         Field '{}' has path annotation but section [{}] has no target.\n\n  \
         Rule: Path annotations require a section target=path declaration.\n\n  \
         Fix: Add target to section: [{}]  # target=path, base=~/app/\n       \
              Or remove the annotation if no path expansion needed.",
        issue.location, issue.field_name, issue.section_name, issue.section_name
    )
}

fn format_missing_field(issue: &MissingFieldAnnotation) -> String {
    format!(
        "ERROR: {}\n  \
         Section [{}] has target but field '{}' has no {} annotation.\n\n  \
         Rule: All fields in targeted sections must have annotations.\n       \
               All units must be convertible to the section target.\n\n  \
         Fix: Add annotation: {} = value  # {}=...\n       \
              Or move to a non-targeted section if this field doesn't need conversion.",
        issue.location, issue.section_name, issue.field_name, issue.expected_type,
        issue.field_name, issue.expected_type
    )
}

fn format_base_redeclaration(issue: &BaseRedeclaration) -> String {
    format!(
        "ERROR: {}\n  \
         Section [{}] attempts to redefine '{}' but path bases are document-level constants.\n\n  \
         Rule: Path bases must be declared in the FIRST target=path section only.\n       \
               Subsequent target=path sections inherit these bases.\n\n  \
         '{}' was defined at line {}: {}={}\n\n  \
         Fix: Remove '{}=...' from this section header.\n       \
              Use: [{}]  # target=path",
        issue.location, issue.section_name, issue.base_name,
        issue.base_name, issue.original_line, issue.base_name, issue.original_value,
        issue.base_name, issue.section_name
    )
}

fn format_unknown_unit(issue: &UnknownUnit) -> String {
    let suggestions = if issue.suggestions.is_empty() {
        String::new()
    } else {
        format!("\n\n  Did you mean: {}?", issue.suggestions.join(", "))
    };
    format!(
        "ERROR: {}\n  \
         Unknown unit '{}' on field '{}'.{}\n\n  \
         {}",
        issue.location, issue.unit, issue.field_name, suggestions, supported_units_summary()
    )
}

fn format_unconvertible_value(issue: &UnconvertibleValue) -> String {
    let fix = match AngleNotation::from_name(&issue.unit) {
        Some(notation) => format!(
            "Write it as: {} = {}  # unit={}\n       \
                  Or give a number: {} = 56.54  # unit=deg",
            issue.field_name, toml_string(notation.example()), notation.name(), issue.field_name
        ),
        None => format!(
            "Give a number: {} = 5  # unit={}\n       \
                  Or a list: {} = [2, 10, 50]  # unit={}\n       \
                  Or move the field to a section without a target if it needs no conversion.",
            issue.field_name, issue.unit, issue.field_name, issue.unit
        ),
    };
    format!(
        "ERROR: {}\n  \
         Field '{}' has a value its unit cannot convert.\n\n  \
         Found: {}  # unit={}\n  \
         Problem: {}\n\n  \
         Rule: A unit applies to a number, or to a list of numbers written on one line.\n       \
               Under an angle target, unit=dms and unit=dm apply to a quoted string.\n\n  \
         Fix: {}",
        issue.location, issue.field_name, issue.found, issue.unit, issue.reason, fix
    )
}

fn format_unexpandable_path(issue: &UnexpandablePath) -> String {
    format!(
        "ERROR: {}\n  \
         Field '{}' has a value its path annotation cannot expand.\n\n  \
         Found: {}  # path={}\n  \
         Problem: {}\n\n  \
         Rule: A path annotation applies to a quoted string, and every part of the path\n       \
               must resolve: its base, the home directory, each environment variable.\n\n  \
         Fix: Give the path as a string: {} = \"subdir/\"  # path={}\n       \
              Or move the field to a section without a target if it is not a path.",
        issue.location, issue.field_name, issue.found, issue.reference, issue.reason,
        issue.field_name, issue.reference
    )
}

fn format_unresolved_annotation(issue: &UnresolvedAnnotation) -> String {
    format!(
        "ERROR: {}\n  \
         {} carries an annotation, but is not in the file's data where its name says.\n\n  \
         Rule: An annotation applies to a plain table reached by its name, and to a field\n       \
               written in that table with a bare key. A table inside a list of tables,\n       \
               a dotted key and a quoted key cannot carry one.\n\n  \
         Fix: Put the values in a plain section: [section]  # target=meters\n       \
              Several values of one kind can be a list: field = [2, 10, 50]  # unit=km",
        issue.location, issue.annotated
    )
}

/// A string as a TOML file writes it.
fn toml_string(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\\\""))
}

fn format_incompatible_family(issue: &IncompatibleUnitFamily) -> String {
    format!(
        "ERROR: {}\n  \
         Field '{}' has unit={} but section has target={}.\n\n  \
         Rule: Unit must be convertible to section target.\n\n  \
         {} units ({}) cannot convert to {} units.\n  \
         Move this field to a section with target={}.",
        issue.location, issue.field_name, issue.unit, issue.target,
        issue.unit_family.to_uppercase(), issue.unit, issue.target_family,
        issue.unit_family_target
    )
}

fn format_undefined_path_ref(issue: &UndefinedPathReference) -> String {
    let defined = if issue.defined_refs.is_empty() {
        "none".into()
    } else {
        issue.defined_refs.join(", ")
    };
    format!(
        "ERROR: {}\n  \
         Field '{}' uses path={} but '{}' is not defined in section header.\n\n  \
         Defined references: {}\n  \
         Built-in references: home, absolute, config\n\n  \
         Fix: Add to section header: [section]  # target=path, base=..., {}=/path/\n       \
              Or use a defined reference: # path=base",
        issue.location, issue.field_name, issue.reference, issue.reference,
        defined, issue.reference
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_issues() {
        let issues = TomlxIssues::new();
        assert!(!issues.has_issues());
        assert_eq!(issues.count(), 0);
    }

    #[test]
    fn test_orphan_unit_format() {
        let mut issues = TomlxIssues::with_file("config.tomlx");
        issues.orphan_unit_annotations.push(OrphanUnitAnnotation {
            location: issues.loc(7),
            field_name: "timeout".to_string(),
            unit: "minutes".to_string(),
            section_name: "settings".to_string(),
        });

        assert!(issues.has_issues());
        let formatted = issues.format();
        assert!(formatted.contains("config.tomlx:7"));
        assert!(formatted.contains("timeout"));
        assert!(formatted.contains("Rule:"));
        assert!(formatted.contains("Fix:"));
    }
}
