//! Annotation extraction from TOML comments.
//!
//! Two-pass parsing:
//! 1. Parse TOML structure with `toml` crate
//! 2. Scan original text for annotation comments (line-by-line)

use std::collections::HashMap;

use super::types::{
    ExpandMode, FieldAnnotation, FieldAnnotationType, SectionAnnotation, TargetFamily,
    TypeAnnotation, parse_target, parse_type_annotation, RESERVED_PATH_KEYS,
};
use super::units::target_names_summary;

/// Result of parsing a section header line.
#[derive(Debug)]
pub enum SectionHeaderParse {
    NoAnnotation { section_path: String },
    Annotated(SectionAnnotation),
    Error { section_path: String, raw: String, reason: String },
}

/// Result of parsing a field line.
#[derive(Debug)]
pub enum FieldLineParse {
    NoAnnotation { field_name: String },
    Annotated(FieldAnnotation),
    Error { field_name: String, raw: String, reason: String },
    NotAField,
}

/// Parse a section header line for annotations.
pub fn parse_section_header(line: &str, line_num: usize) -> Option<SectionHeaderParse> {
    let line = line.trim();

    if !line.starts_with('[') {
        return None;
    }

    if line.starts_with("[[") {
        return Some(parse_table_list_header(line));
    }

    let bracket_end = line.find(']')?;
    let section_path = line[1..bracket_end].trim().to_string();

    let after_bracket = &line[bracket_end + 1..];
    let comment_start = after_bracket.find('#');

    match comment_start {
        None => Some(SectionHeaderParse::NoAnnotation { section_path }),
        Some(pos) => {
            let comment = after_bracket[pos + 1..].trim();
            match parse_section_annotation(comment, &section_path, line_num) {
                Ok(annotation) => Some(SectionHeaderParse::Annotated(annotation)),
                Err(reason) => Some(SectionHeaderParse::Error {
                    section_path,
                    raw: comment.to_string(),
                    reason,
                }),
            }
        }
    }
}

/// Parse a header that starts an item in a list of tables: `[[section]]`. It carries
/// no annotation. A comment on it is for people; one that reads as an annotation is
/// refused, because nothing would apply it.
fn parse_table_list_header(line: &str) -> SectionHeaderParse {
    let close = line.find("]]").unwrap_or(line.len());
    let section_path = line[2..close].trim().to_string();
    let comment = line[close..].find('#').map(|position| line[close + position + 1..].trim());

    match comment {
        Some(comment) if parse_section_annotation(comment, &section_path, 0).is_ok() => {
            SectionHeaderParse::Error {
                section_path,
                raw: comment.to_string(),
                reason: "a list of tables cannot carry an annotation".to_string(),
            }
        }
        _ => SectionHeaderParse::NoAnnotation { section_path },
    }
}

/// Parse section annotation content (after the #).
fn parse_section_annotation(
    comment: &str,
    section_path: &str,
    line_num: usize,
) -> Result<SectionAnnotation, String> {
    let pairs: Vec<&str> = comment.split(',').map(|s| s.trim()).collect();

    if pairs.is_empty() {
        return Err("Empty annotation".into());
    }

    let mut builder = AnnotationBuilder::default();

    for pair in pairs {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid key=value pair: '{}'", pair));
        }

        let key = parts[0].trim().to_lowercase();
        let value = parts[1].trim();

        builder.apply_key(&key, value)?;
    }

    validate_annotation_constraints(&builder)?;

    Ok(SectionAnnotation {
        section_path: section_path.to_string(),
        line: line_num,
        target: builder.target,
        type_annotation: builder.type_annotation,
        path_bases: builder.path_bases,
        expand_mode: builder.expand_mode,
    })
}

/// In-progress annotation state accumulated from key=value pairs.
#[derive(Default)]
struct AnnotationBuilder {
    target: Option<TargetFamily>,
    type_annotation: Option<TypeAnnotation>,
    path_bases: HashMap<String, String>,
    expand_mode: ExpandMode,
}

impl AnnotationBuilder {
    /// Dispatch a single key=value pair into the builder state.
    fn apply_key(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "target" => {
                self.target = Some(parse_target(value).ok_or_else(|| {
                    format!("Unknown target '{}'. Use: {}, or path", value, target_names_summary())
                })?);
            }
            "type" => {
                self.type_annotation = Some(parse_type_annotation(value).ok_or_else(|| {
                    format!("Unknown type '{}'. Use: int, float, str, bool, or list[T]", value)
                })?);
            }
            "expand" => {
                self.expand_mode = ExpandMode::from_str(value).ok_or_else(|| {
                    format!("Unknown expand mode '{}'. Use: user, env, all, or none", value)
                })?;
            }
            _ => {
                if RESERVED_PATH_KEYS.contains(&key) && key != "target" && key != "expand" && key != "type" {
                    return Err(format!("Reserved key '{}' cannot be used as base name", key));
                }
                self.path_bases.insert(key.to_string(), value.to_string());
            }
        }
        Ok(())
    }
}

/// Validate cross-field constraints after all pairs are parsed.
fn validate_annotation_constraints(builder: &AnnotationBuilder) -> Result<(), String> {
    if builder.target.is_none() && builder.type_annotation.is_none() {
        return Err("Missing required annotation: at least one of 'target=' or 'type=' required".into());
    }

    if !builder.path_bases.is_empty() {
        match &builder.target {
            Some(TargetFamily::Path) => {}
            Some(_) => {
                return Err(format!(
                    "Path bases ({}) only valid for target=path",
                    builder.path_bases.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            None => {
                return Err(format!(
                    "Path bases ({}) require target=path",
                    builder.path_bases.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    Ok(())
}

/// Parse a field line for annotations.
pub fn parse_field_line(line: &str, line_num: usize, section_path: &str) -> FieldLineParse {
    let line = line.trim();

    if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
        return FieldLineParse::NotAField;
    }

    let eq_pos = match line.find('=') {
        Some(pos) => pos,
        None => return FieldLineParse::NotAField,
    };

    let field_name = line[..eq_pos].trim().to_string();
    if field_name.is_empty() {
        return FieldLineParse::NotAField;
    }

    let comment_start = find_comment_start(line);

    match comment_start {
        None => FieldLineParse::NoAnnotation { field_name },
        Some(pos) => {
            let comment = line[pos + 1..].trim();
            let field_path = if section_path.is_empty() {
                field_name.clone()
            } else {
                format!("{}.{}", section_path, field_name)
            };

            match parse_field_annotation(comment, &field_path, line_num) {
                Ok(annotation) => FieldLineParse::Annotated(annotation),
                Err(reason) => FieldLineParse::Error {
                    field_name,
                    raw: comment.to_string(),
                    reason,
                },
            }
        }
    }
}

/// Find the start of a comment, avoiding matches inside strings.
fn find_comment_start(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escape_next = false;
    let mut string_char = '"';

    for (i, c) in line.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' | '\'' => {
                if in_string {
                    if c == string_char {
                        in_string = false;
                    }
                } else {
                    in_string = true;
                    string_char = c;
                }
            }
            '#' if !in_string => {
                return Some(i);
            }
            _ => {}
        }
    }

    None
}

/// Parse field annotation content (after the #).
fn parse_field_annotation(
    comment: &str,
    field_path: &str,
    line_num: usize,
) -> Result<FieldAnnotation, String> {
    // Split on ` // ` to separate machine annotation from human comment
    let (annotation_part, human_comment) = match comment.find(" // ") {
        Some(pos) => {
            let ann = comment[..pos].trim();
            let human = comment[pos + 4..].trim();
            (ann, Some(human.to_string()))
        }
        None => (comment, None),
    };

    let parts: Vec<&str> = annotation_part.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid annotation format: '{}'. Expected key=value", annotation_part));
    }

    let key = parts[0].trim().to_lowercase();
    let value = parts[1].trim().to_string();

    let annotation_type = match key.as_str() {
        "unit" => FieldAnnotationType::Unit(value),
        "path" => FieldAnnotationType::Path(value),
        _ => {
            return Err(format!(
                "Unknown annotation type '{}'. Use: unit or path",
                key
            ));
        }
    };

    Ok(FieldAnnotation {
        field_path: field_path.to_string(),
        line: line_num,
        annotation_type,
        human_comment,
    })
}

/// Extract all section headers and their annotations from source text.
pub fn extract_section_annotations(source: &str) -> Vec<(usize, SectionHeaderParse)> {
    source
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let line_num = i + 1;
            parse_section_header(line, line_num).map(|result| (line_num, result))
        })
        .collect()
}

/// Extract all field annotations from source text within a section.
pub fn extract_field_annotations(
    source: &str,
    section_start: usize,
    section_end: Option<usize>,
    section_path: &str,
) -> Vec<(usize, FieldLineParse)> {
    let end = section_end.unwrap_or(usize::MAX);

    source
        .lines()
        .enumerate()
        .filter(|(i, _)| *i + 1 > section_start && *i + 1 < end)
        .filter_map(|(i, line)| {
            let line_num = i + 1;
            let result = parse_field_line(line, line_num, section_path);
            match result {
                FieldLineParse::NotAField => None,
                _ => Some((line_num, result)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_section_no_annotation() {
        let result = parse_section_header("[settings]", 1).unwrap();
        assert!(matches!(result, SectionHeaderParse::NoAnnotation { .. }));
    }

    #[test]
    fn test_parse_section_with_target() {
        let result = parse_section_header("[ttl]  # target=seconds", 1).unwrap();
        if let SectionHeaderParse::Annotated(ann) = result {
            assert_eq!(ann.section_path, "ttl");
            assert!(matches!(ann.target, Some(TargetFamily::Unit(_))));
        } else {
            panic!("Expected Annotated");
        }
    }

    #[test]
    fn test_parse_section_with_path_and_bases() {
        let result = parse_section_header(
            "[paths]  # target=path, base=~/ai/phoenix/, logs=/var/log/",
            5,
        ).unwrap();

        if let SectionHeaderParse::Annotated(ann) = result {
            assert!(matches!(ann.target, Some(TargetFamily::Path)));
            assert_eq!(ann.path_bases.get("base"), Some(&"~/ai/phoenix/".to_string()));
            assert_eq!(ann.path_bases.get("logs"), Some(&"/var/log/".to_string()));
        } else {
            panic!("Expected Annotated");
        }
    }

    #[test]
    fn test_parse_section_type_only() {
        let result = parse_section_header("[features]  # type=bool", 1).unwrap();
        if let SectionHeaderParse::Annotated(ann) = result {
            assert!(ann.target.is_none());
            assert!(matches!(ann.type_annotation, Some(TypeAnnotation::Bool)));
        } else {
            panic!("Expected Annotated");
        }
    }

    #[test]
    fn test_parse_field_with_unit() {
        let result = parse_field_line("session = 15  # unit=minutes", 1, "ttl");
        if let FieldLineParse::Annotated(ann) = result {
            assert_eq!(ann.field_path, "ttl.session");
            assert!(matches!(ann.annotation_type, FieldAnnotationType::Unit(_)));
        } else {
            panic!("Expected Annotated");
        }
    }

    #[test]
    fn test_parse_field_with_human_comment() {
        let result = parse_field_line("session = 15  # unit=minutes // idle timeout", 1, "ttl");
        if let FieldLineParse::Annotated(ann) = result {
            assert_eq!(ann.human_comment, Some("idle timeout".to_string()));
        } else {
            panic!("Expected Annotated");
        }
    }

    #[test]
    fn test_parse_field_hash_in_string() {
        let result = parse_field_line("pattern = \"#hashtag\"  # unit=seconds", 1, "");
        assert!(matches!(result, FieldLineParse::Annotated(_)));
    }

    #[test]
    fn test_extract_section_annotations() {
        let source = "[settings]\nname = \"test\"\n\n[ttl]  # target=seconds\nsession = 15\n";
        let sections = extract_section_annotations(source);
        assert_eq!(sections.len(), 2);
        assert!(matches!(sections[0].1, SectionHeaderParse::NoAnnotation { .. }));
        assert!(matches!(sections[1].1, SectionHeaderParse::Annotated(_)));
    }
}
