//! Main processing pipeline for .tomlx files.
//!
//! Orchestrates: parse -> validate -> convert -> output.

use std::collections::HashMap;
use std::path::Path;

use error_core::FormatError;
use serde_json::{Map, Value};

use super::angle_parse::{parse_angle, AngleNotation};
use super::diagnostics::{
    IncompatibleUnitFamily, MalformedFieldAnnotation, TomlxIssues, UnconvertibleValue,
    UndefinedPathReference, UnexpandablePath, UnknownUnit, UnresolvedAnnotation,
};
use super::paths::{expand_path, expand_registry_bases};
use super::types::{
    FieldAnnotation, FieldAnnotationType, PathRegistry, SectionAnnotation, SectionPathInfo,
    TargetFamily, TomlxOutput, TypeAnnotation,
};
use super::units::{
    convert_unit, lookup_unit, other_family_of_unit, suggest_units, target_unit_name, UnitFamily, UnitTarget,
};
use super::validation::{validate_tomlx, ValidatedSection, ValidationResult};

/// Identity of a field being processed (name + source line).
struct FieldRef<'a> {
    name: &'a str,
    line: usize,
}

/// Context for path resolution operations.
struct PathContext<'a> {
    registry: &'a PathRegistry,
    config_dir: Option<&'a Path>,
    resolve_env: &'a dyn Fn(&str) -> Option<String>,
}

/// Parse a .tomlx string and return the converted output.
///
/// `resolve_env` resolves environment variable names to values. Pass
/// `|name| std::env::var(name).ok()` for live resolution.
pub fn parse_tomlx(
    source: &str,
    file: Option<&str>,
    config_dir: Option<&Path>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<TomlxOutput, FormatError> {
    // Step 1: Parse as TOML first (fast path)
    let toml_value: toml::Value = toml::from_str(source)
        .map_err(|e| FormatError::TomlxParse(format!("Invalid TOML: {}", e)))?;

    let json_value = toml_to_json_value(&toml_value);

    // Step 2: Validate annotations
    let validation = validate_tomlx(source, file);

    if validation.issues.has_issues() {
        return Err(FormatError::Educational {
            message: validation.issues.format(),
        });
    }

    // Step 3: Process sections
    process_sections(json_value, validation, config_dir, resolve_env)
}

fn toml_to_json_value(toml: &toml::Value) -> Value {
    match toml {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => Value::Array(arr.iter().map(toml_to_json_value).collect()),
        toml::Value::Table(table) => {
            let map: Map<String, Value> = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json_value(v)))
                .collect();
            Value::Object(map)
        }
    }
}

fn process_sections(
    mut data: Value,
    validation: ValidationResult,
    config_dir: Option<&Path>,
    resolve_env: &dyn Fn(&str) -> Option<String>,
) -> Result<TomlxOutput, FormatError> {
    let mut section_units: HashMap<String, String> = HashMap::new();
    let mut section_paths: HashMap<String, SectionPathInfo> = HashMap::new();
    let mut section_types: HashMap<String, String> = HashMap::new();
    let mut typed_sections: HashMap<String, (TypeAnnotation, Vec<String>)> = HashMap::new();
    let mut issues = validation.issues;
    let mut path_registry = validation.path_registry;

    expand_registry_bases(&mut path_registry, config_dir, resolve_env);

    for (section_path, section) in &validation.sections {
        if section_path.is_empty() {
            if let Some(annotation) = &section.annotation {
                let paths = PathContext { registry: &path_registry, config_dir, resolve_env };
                process_root_fields(&mut data, section, annotation, &paths, &mut issues)?;
            }
            continue;
        }

        let annotation = match &section.annotation {
            Some(annotation) => annotation,
            None => continue,
        };

        // An annotated section is a table reached by its name, or the file is refused.
        let section_data = match get_section_mut(&mut data, section_path) {
            Some(data) if data.is_object() => data,
            _ => {
                issues.unresolved_annotations.push(UnresolvedAnnotation {
                    location: issues.loc(annotation.line),
                    annotated: format!("Section [{}]", section_path),
                });
                continue;
            }
        };

        record_type_metadata(section_path, section_data, annotation, &mut section_types, &mut typed_sections);

        let paths = PathContext { registry: &path_registry, config_dir, resolve_env };
        match apply_target_conversion(section_data, section, annotation, &paths, &mut issues)? {
            TargetResult::Unit(name) => { section_units.insert(section_path.clone(), name); }
            TargetResult::Path(info) => { section_paths.insert(section_path.clone(), info); }
            TargetResult::None => {}
        }
    }

    if issues.has_issues() {
        return Err(FormatError::Educational { message: issues.format() });
    }

    let schema = if typed_sections.is_empty() {
        None
    } else {
        Some(build_document_schema(&typed_sections))
    };

    Ok(TomlxOutput { data, schema, section_types, section_units, section_paths })
}

/// Record type annotation metadata for a section (types map + schema fields).
fn record_type_metadata(
    section_path: &str,
    section_data: &Value,
    annotation: &SectionAnnotation,
    section_types: &mut HashMap<String, String>,
    typed_sections: &mut HashMap<String, (TypeAnnotation, Vec<String>)>,
) {
    if let Some(type_ann) = &annotation.type_annotation {
        section_types.insert(section_path.to_string(), type_ann.as_str());
        let field_names: Vec<String> = match section_data {
            Value::Object(map) => map.keys().cloned().collect(),
            _ => vec![],
        };
        typed_sections.insert(section_path.to_string(), (type_ann.clone(), field_names));
    }
}

/// Result of target-based conversion: what metadata to record.
enum TargetResult {
    Unit(String),
    Path(SectionPathInfo),
    None,
}

/// Apply target-based conversion (unit normalization or path expansion) to a section.
fn apply_target_conversion(
    section_data: &mut Value,
    section: &ValidatedSection,
    annotation: &SectionAnnotation,
    paths: &PathContext,
    issues: &mut TomlxIssues,
) -> Result<TargetResult, FormatError> {
    match &annotation.target {
        Some(TargetFamily::Unit(_)) => {
            process_unit_section(section_data, section, annotation, issues)?;
            let unit_name = annotation.target.as_ref()
                .map(|target| target_unit_name(target).to_string())
                .unwrap_or_default();
            Ok(TargetResult::Unit(unit_name))
        }
        Some(TargetFamily::Path) => {
            process_path_section(section_data, section, annotation, paths, issues)?;
            Ok(TargetResult::Path(snapshot_path_bases(paths.registry)))
        }
        None => Ok(TargetResult::None),
    }
}

/// Snapshot current path bases from the registry into a SectionPathInfo.
fn snapshot_path_bases(registry: &PathRegistry) -> SectionPathInfo {
    let bases: HashMap<String, String> = registry
        .user_defined
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    SectionPathInfo { bases, expand: registry.expand_mode }
}

fn build_document_schema(typed_sections: &HashMap<String, (TypeAnnotation, Vec<String>)>) -> Value {
    let mut properties = Map::new();

    for (section_path, (type_ann, field_names)) in typed_sections {
        let field_schema = type_annotation_to_json_schema(type_ann);
        let section_schema = build_section_schema(&field_schema, field_names);
        insert_nested_schema(&mut properties, section_path, section_schema);
    }

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    Value::Object(schema)
}

fn type_annotation_to_json_schema(type_ann: &TypeAnnotation) -> Value {
    let mut schema = Map::new();
    match type_ann {
        TypeAnnotation::Int => { schema.insert("type".to_string(), Value::String("integer".to_string())); }
        TypeAnnotation::Float => { schema.insert("type".to_string(), Value::String("number".to_string())); }
        TypeAnnotation::Str => { schema.insert("type".to_string(), Value::String("string".to_string())); }
        TypeAnnotation::Bool => { schema.insert("type".to_string(), Value::String("boolean".to_string())); }
        TypeAnnotation::List(inner) => {
            schema.insert("type".to_string(), Value::String("array".to_string()));
            schema.insert("items".to_string(), type_annotation_to_json_schema(inner));
        }
    }
    Value::Object(schema)
}

fn build_section_schema(field_schema: &Value, field_names: &[String]) -> Value {
    let mut props = Map::new();
    for name in field_names {
        props.insert(name.clone(), field_schema.clone());
    }

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(props));
    Value::Object(schema)
}

fn insert_nested_schema(properties: &mut Map<String, Value>, path: &str, schema: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() { return; }

    if parts.len() == 1 {
        properties.insert(parts[0].to_string(), schema);
        return;
    }

    let first = parts[0];
    let rest = parts[1..].join(".");

    let entry = properties.entry(first.to_string()).or_insert_with(|| {
        let mut obj = Map::new();
        obj.insert("type".to_string(), Value::String("object".to_string()));
        obj.insert("properties".to_string(), Value::Object(Map::new()));
        Value::Object(obj)
    });

    if let Value::Object(ref mut obj) = entry {
        if let Some(Value::Object(ref mut nested_props)) = obj.get_mut("properties") {
            insert_nested_schema(nested_props, &rest, schema);
        }
    }
}

fn get_section_mut<'a>(data: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = data;
    for part in parts {
        current = current.get_mut(part)?;
    }
    Some(current)
}

fn process_root_fields(
    data: &mut Value,
    section: &ValidatedSection,
    annotation: &super::types::SectionAnnotation,
    paths: &PathContext,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    if let Value::Object(map) = data {
        for field in &section.fields {
            let field_ref = FieldRef {
                name: field.field_path.rsplit('.').next().unwrap_or(&field.field_path),
                line: field.line,
            };
            if let Some(value) = map.get_mut(field_ref.name) {
                match (&annotation.target, &field.annotation_type) {
                    (Some(TargetFamily::Unit(target)), FieldAnnotationType::Unit(unit)) => {
                        convert_field_unit(value, unit, target, &field_ref, issues)?;
                    }
                    (Some(TargetFamily::Path), FieldAnnotationType::Path(reference)) => {
                        convert_field_path(value, reference, paths, &field_ref, issues)?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn process_unit_section(
    section_data: &mut Value,
    section: &ValidatedSection,
    annotation: &super::types::SectionAnnotation,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    let target = match &annotation.target {
        Some(TargetFamily::Unit(unit_target)) => unit_target,
        _ => return Ok(()),
    };

    if let Value::Object(map) = section_data {
        for field in &section.fields {
            let field_ref = FieldRef {
                name: field.field_path.rsplit('.').next().unwrap_or(&field.field_path),
                line: field.line,
            };
            match (map.get_mut(field_ref.name), &field.annotation_type) {
                (Some(value), FieldAnnotationType::Unit(unit)) => {
                    convert_field_unit(value, unit, target, &field_ref, issues)?;
                }
                (Some(_), FieldAnnotationType::Path(_)) => report_wrong_annotation_kind(field, "unit", issues),
                (None, _) => report_unresolved_field(field, issues),
            }
        }
    }
    Ok(())
}

fn convert_field_unit(
    value: &mut Value,
    unit: &str,
    target: &UnitTarget,
    field: &FieldRef,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    // A unit name is read within the family of the section's target.
    let expected = target.family;

    // An angle may be written in parts, as a quoted string in a named notation.
    let notation = match expected {
        UnitFamily::Angle => AngleNotation::from_name(unit),
        _ => None,
    };

    if notation.is_none() && lookup_unit(unit, expected).is_none() {
        match other_family_of_unit(unit, expected) {
            Some(other) => issues.incompatible_unit_families.push(IncompatibleUnitFamily {
                location: issues.loc(field.line),
                field_name: field.name.to_string(),
                unit: unit.to_string(),
                unit_family: other.name().to_string(),
                unit_family_target: other.base_target().to_string(),
                target: target.name.to_string(),
                target_family: expected.name().to_string(),
            }),
            None => issues.unknown_units.push(UnknownUnit {
                location: issues.loc(field.line),
                field_name: field.name.to_string(),
                unit: unit.to_string(),
                suggestions: suggest_units(unit, expected),
            }),
        }
        return Ok(());
    }

    // The unit applies to the value, or to every item of a list. Anything else is refused.
    let outcome = match &mut *value {
        Value::Array(items) => items.iter_mut().try_for_each(|item| convert_value(item, unit, notation, target)),
        single => convert_value(single, unit, notation, target),
    };

    if let Err(reason) = outcome {
        issues.unconvertible_values.push(UnconvertibleValue {
            location: issues.loc(field.line),
            field_name: field.name.to_string(),
            unit: unit.to_string(),
            found: value.to_string(),
            reason,
        });
    }

    Ok(())
}

/// Convert one value in place: a number in its unit, or an angle written in its notation.
fn convert_value(
    value: &mut Value,
    unit: &str,
    notation: Option<AngleNotation>,
    target: &UnitTarget,
) -> Result<(), String> {
    let converted = match (notation, &*value) {
        (Some(notation), Value::String(text)) => {
            parse_angle(text, notation).and_then(|degrees| convert_unit(degrees, "deg", target))?
        }
        (Some(notation), _) => {
            return Err(format!(
                "the value is not a quoted string; unit={} reads one such as {}",
                notation.name(), notation.example()
            ));
        }
        (None, Value::Number(number)) => {
            let number = number.as_f64().ok_or_else(|| "the number is out of range".to_string())?;
            convert_unit(number, unit, target)?
        }
        (None, Value::String(_)) if target.family == UnitFamily::Angle => {
            return Err(format!("a quoted string needs unit=dms or unit=dm, found unit={}", unit));
        }
        (None, _) => return Err(format!("unit={} needs a number", unit)),
    };

    store_number(value, converted);
    Ok(())
}

/// Report a field whose annotation is of the kind its section's target does not take.
fn report_wrong_annotation_kind(field: &FieldAnnotation, expected: &str, issues: &mut TomlxIssues) {
    let (found, named) = match &field.annotation_type {
        FieldAnnotationType::Unit(unit) => ("unit", unit),
        FieldAnnotationType::Path(reference) => ("path", reference),
    };
    issues.malformed_field_annotations.push(MalformedFieldAnnotation {
        location: issues.loc(field.line),
        field_name: field.field_path.clone(),
        raw_annotation: format!("{}={}", found, named),
        reason: format!("this section's target takes {}= annotations, not {}=", expected, found),
    });
}

/// Report an annotated field that is not in its section's table under its name.
fn report_unresolved_field(field: &FieldAnnotation, issues: &mut TomlxIssues) {
    issues.unresolved_annotations.push(UnresolvedAnnotation {
        location: issues.loc(field.line),
        annotated: format!("Field '{}'", field.field_path),
    });
}

/// Put a converted number in place of a field's value: whole numbers as integers.
fn store_number(value: &mut Value, converted: f64) {
    if converted.fract() == 0.0 && converted.abs() < i64::MAX as f64 {
        *value = Value::Number((converted as i64).into());
    } else if let Some(n) = serde_json::Number::from_f64(converted) {
        *value = Value::Number(n);
    }
}

fn process_path_section(
    section_data: &mut Value,
    section: &ValidatedSection,
    _ann: &super::types::SectionAnnotation,
    paths: &PathContext,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    if let Value::Object(map) = section_data {
        for field in &section.fields {
            let field_ref = FieldRef {
                name: field.field_path.rsplit('.').next().unwrap_or(&field.field_path),
                line: field.line,
            };
            match (map.get_mut(field_ref.name), &field.annotation_type) {
                (Some(value), FieldAnnotationType::Path(reference)) => {
                    convert_field_path(value, reference, paths, &field_ref, issues)?;
                }
                (Some(_), FieldAnnotationType::Unit(_)) => report_wrong_annotation_kind(field, "path", issues),
                (None, _) => report_unresolved_field(field, issues),
            }
        }
    }
    Ok(())
}

fn convert_field_path(
    value: &mut Value,
    reference: &str,
    paths: &PathContext,
    field: &FieldRef,
    issues: &mut TomlxIssues,
) -> Result<(), FormatError> {
    let path_str = match &*value {
        Value::String(s) => s.clone(),
        other => {
            issues.unexpandable_paths.push(UnexpandablePath {
                location: issues.loc(field.line),
                field_name: field.name.to_string(),
                reference: reference.to_string(),
                found: other.to_string(),
                reason: "the value is not a quoted string".to_string(),
            });
            return Ok(());
        }
    };

    if !PathRegistry::is_builtin(reference) && !paths.registry.user_defined.contains_key(reference) {
        issues.undefined_path_references.push(UndefinedPathReference {
            location: issues.loc(field.line),
            field_name: field.name.to_string(),
            reference: reference.to_string(),
            defined_refs: paths.registry.user_defined.keys().cloned().collect(),
        });
        return Ok(());
    }

    match expand_path(&path_str, reference, paths.registry, paths.config_dir, paths.resolve_env) {
        Ok(expanded) => { *value = Value::String(expanded); }
        Err(reason) => issues.unexpandable_paths.push(UnexpandablePath {
            location: issues.loc(field.line),
            field_name: field.name.to_string(),
            reference: reference.to_string(),
            found: value.to_string(),
            reason,
        }),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_env(name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/Users/test".to_string()),
            _ => None,
        }
    }

    #[test]
    fn test_parse_unit_conversion() {
        let source = "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\ncache = 4     # unit=hours\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let ttl = result.data.get("ttl").unwrap();
        assert_eq!(ttl.get("session").unwrap(), 900);
        assert_eq!(ttl.get("cache").unwrap(), 14400);
        assert_eq!(result.section_units.get("ttl"), Some(&"seconds".to_string()));
    }

    #[test]
    fn test_parse_path_expansion() {
        let source = "\n[paths]  # target=path, base=~/ai/phoenix/, expand=user\ncli = \"cli/\"    # path=base\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let paths = result.data.get("paths").unwrap();
        assert_eq!(paths.get("cli").unwrap().as_str().unwrap(), "/Users/test/ai/phoenix/cli");
    }

    #[test]
    fn test_parse_mixed() {
        let source = "\n[metadata]\nname = \"test\"\n\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("metadata").unwrap().get("name").unwrap(), "test");
        assert_eq!(result.data.get("ttl").unwrap().get("session").unwrap(), 900);
    }

    #[test]
    fn test_orphan_error() {
        let source = "\n[settings]\ntimeout = 30  # unit=seconds\n";
        let result = parse_tomlx(source, Some("config.tomlx"), None, &test_env);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timeout"));
    }

    #[test]
    fn test_size_units() {
        let source = "\n[limits]  # target=bytes\nmax_size = 500  # unit=mb\nbuffer = 64     # unit=kib\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let limits = result.data.get("limits").unwrap();
        assert_eq!(limits.get("max_size").unwrap(), 500_000_000i64);
        assert_eq!(limits.get("buffer").unwrap(), 65536i64);
    }

    #[test]
    fn test_length_units() {
        let source = "\n[margin]  # target=meters\nwest = 2      # unit=km\neast = 5000   # unit=m\nstep = 250    # unit=cm\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let margin = result.data.get("margin").unwrap();
        assert_eq!(margin.get("west").unwrap(), 2000i64);
        assert_eq!(margin.get("east").unwrap(), 5000i64);
        assert_eq!(margin.get("step").unwrap(), 2.5);
        assert_eq!(result.section_units.get("margin"), Some(&"meters".to_string()));
    }

    #[test]
    fn test_length_target_in_either_spelling() {
        let source = "\n[margin]  # target=metres\nnorth = 10  # unit=kilometres\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("margin").unwrap().get("north").unwrap(), 10_000i64);
        assert_eq!(result.section_units.get("margin"), Some(&"meters".to_string()));
    }

    #[test]
    fn test_m_follows_the_section_target() {
        let source = "\n[ttl]  # target=seconds\nsession = 2  # unit=m\n\n[margin]  # target=meters\nwest = 2  # unit=m\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("ttl").unwrap().get("session").unwrap(), 120);
        assert_eq!(result.data.get("margin").unwrap().get("west").unwrap(), 2);
    }

    #[test]
    fn test_length_unit_under_time_target_is_refused() {
        let source = "\n[ttl]  # target=seconds\nsession = 5  # unit=km\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("session"));
        assert!(err.contains("LENGTH"));
        assert!(err.contains("target=meters"));
    }

    #[test]
    fn test_time_unit_under_length_target_is_refused() {
        let source = "\n[margin]  # target=meters\nwest = 5  # unit=hours\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("west"));
        assert!(err.contains("TIME"));
        assert!(err.contains("target=seconds"));
    }

    #[test]
    fn test_unknown_length_unit_suggests_length_units() {
        let source = "\n[margin]  # target=meters\nwest = 5  # unit=kilometrs\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("Unknown unit 'kilometrs'"));
        assert!(err.contains("Did you mean"));
    }

    #[test]
    fn test_mass_speed_and_pressure_sections() {
        let source = "\n[load]  # target=kg\npack = 18500  # unit=g\nboat = 0.5    # unit=t\n\n[pace]  # target=m/s\nhiking = 18  # unit=km/h\n\n[weather]  # target=hpa\nstorm = 1         # unit=bar\nstation = 101325  # unit=pa\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("load").unwrap().get("pack").unwrap(), 18.5);
        assert_eq!(result.data.get("load").unwrap().get("boat").unwrap(), 500);
        assert_eq!(result.data.get("pace").unwrap().get("hiking").unwrap(), 5);
        assert_eq!(result.data.get("weather").unwrap().get("storm").unwrap(), 1000);
        assert_eq!(result.data.get("weather").unwrap().get("station").unwrap(), 1013.25);
        assert_eq!(result.section_units.get("load"), Some(&"kilograms".to_string()));
        assert_eq!(result.section_units.get("pace"), Some(&"meters_per_second".to_string()));
        assert_eq!(result.section_units.get("weather"), Some(&"hectopascals".to_string()));
    }

    #[test]
    fn test_area_volume_and_frequency_sections() {
        let source = "\n[lake]  # target=m2\nsurface = 2.5  # unit=ha\n\n[water]  # target=liters\nbottle = 750  # unit=ml\n\n[radio]  # target=hz\nchannel = 156.8  # unit=mhz\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("lake").unwrap().get("surface").unwrap(), 25000);
        assert_eq!(result.data.get("water").unwrap().get("bottle").unwrap(), 0.75);
        assert_eq!(result.data.get("radio").unwrap().get("channel").unwrap(), 156_800_000i64);
        assert_eq!(result.section_units.get("lake"), Some(&"square_meters".to_string()));
    }

    #[test]
    fn test_angles_written_in_parts() {
        let source = "\n[camp]  # target=degrees\nlatitude = \"56°32'24.5\\\"N\"  # unit=dms\nlongitude = \"61°42.654'W\"   # unit=dm // from the GPS\nbearing = 270                # unit=deg\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let camp = result.data.get("camp").unwrap();
        let latitude = (56.0 * 3600.0 + 32.0 * 60.0 + 24.5) / 3600.0;
        let longitude = -(61.0 * 60.0 + 42.654) / 60.0;
        assert_eq!(camp.get("latitude").unwrap().as_f64().unwrap(), latitude);
        assert_eq!(camp.get("longitude").unwrap().as_f64().unwrap(), longitude);
        assert_eq!(camp.get("bearing").unwrap(), 270);
        assert_eq!(result.section_units.get("camp"), Some(&"degrees".to_string()));
    }

    #[test]
    fn test_angles_to_radians() {
        let source = "\n[sun]  # target=radians\nazimuth = 180        # unit=deg\nnoon = \"180°0'0\\\"\"  # unit=dms\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let sun = result.data.get("sun").unwrap();
        assert!((sun.get("azimuth").unwrap().as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-12);
        assert!((sun.get("noon").unwrap().as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn test_angle_in_parts_that_is_not_a_string_is_refused() {
        let source = "\n[camp]  # target=degrees\nlatitude = 56.54  # unit=dms\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("latitude"));
        assert!(err.contains("not a quoted string"));
        assert!(err.contains("Fix:"));
    }

    #[test]
    fn test_angle_missing_a_part_is_refused() {
        let source = "\n[camp]  # target=degrees\nlatitude = \"56°32'N\"  # unit=dms\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("no seconds"));
    }

    #[test]
    fn test_quoted_angle_without_a_notation_is_refused() {
        let source = "\n[camp]  # target=degrees\nlatitude = \"56°32'24\\\"N\"  # unit=deg\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("needs unit=dms or unit=dm"));
    }

    #[test]
    fn test_angle_notation_under_time_target_is_refused() {
        let source = "\n[ttl]  # target=seconds\nsession = 5  # unit=dms\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("ANGLE"));
        assert!(err.contains("target=degrees"));
    }

    #[test]
    fn test_unknown_target_lists_the_targets() {
        let source = "\n[load]  # target=stones\npack = 3  # unit=kg\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("Unknown target 'stones'"));
        assert!(err.contains("kilograms"));
    }

    #[test]
    fn test_unit_on_a_list_converts_every_item() {
        let source = "\n[area.margin]  # target=meters\nwest = [2, 10, 50]      # unit=km\neast = [500, 2.5, 0]    # unit=m\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        let margin = result.data.get("area").unwrap().get("margin").unwrap();
        assert_eq!(margin.get("west").unwrap(), &serde_json::json!([2000, 10000, 50000]));
        assert_eq!(margin.get("east").unwrap(), &serde_json::json!([500, 2.5, 0]));
    }

    #[test]
    fn test_list_of_angles_in_parts() {
        let source = "\n[camps]  # target=degrees\nlatitudes = [\"56°30'N\", \"57°15'N\"]  # unit=dm\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("camps").unwrap().get("latitudes").unwrap(), &serde_json::json!([56.5, 57.25]));
    }

    #[test]
    fn test_list_holding_a_non_number_is_refused() {
        let source = "\n[margin]  # target=meters\nwest = [2, \"ten\", 50]  # unit=km\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("west"));
        assert!(err.contains("needs a number"));
    }

    #[test]
    fn test_unit_on_a_string_is_refused() {
        let source = "\n[ttl]  # target=seconds\nname = \"session\"  # unit=seconds\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("name"));
        assert!(err.contains("needs a number"));
    }

    #[test]
    fn test_annotated_section_inside_a_list_of_tables_is_refused() {
        let source = "\n[[area.zones]]\nzoom_levels_coarser = 2\n\n[area.zones.margin]  # target=meters\nwest = 10  # unit=km\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("Section [area.zones.margin]"));
        assert!(err.contains("not in the file's data"));
    }

    #[test]
    fn test_annotation_on_a_list_of_tables_header_is_refused() {
        let source = "\n[[zones]]  # target=meters\nwest = 10\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("a list of tables cannot carry an annotation"));
    }

    #[test]
    fn test_list_of_tables_passes_through_unannotated() {
        let source = "\n[[zones]]  # the innermost\nname = \"inner\"\n\n[[zones]]\nname = \"outer\"\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("zones").unwrap(), &serde_json::json!([{"name": "inner"}, {"name": "outer"}]));
    }

    #[test]
    fn test_unit_on_a_dotted_key_is_refused() {
        let source = "\n[margin]  # target=meters\nwest.inner = 2  # unit=km\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("Field 'margin.west.inner'"));
        assert!(err.contains("not in the file's data"));
    }

    #[test]
    fn test_path_annotation_in_a_unit_section_is_refused() {
        let source = "\n[ttl]  # target=seconds\nsession = \"logs/\"  # path=home\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("takes unit= annotations, not path="));
    }

    #[test]
    fn test_path_on_a_value_that_is_not_a_string_is_refused() {
        let source = "\n[paths]  # target=path, base=~/app/\nretries = 3  # path=base\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("retries"));
        assert!(err.contains("not a quoted string"));
    }

    #[test]
    fn test_path_naming_an_unset_variable_is_refused() {
        let source = "\n[paths]  # target=path, base=$MISSING_TOMLX_VAR/app/, expand=env\ncli = \"cli/\"  # path=base\n";
        let err = parse_tomlx(source, Some("config.tomlx"), None, &test_env).unwrap_err().to_string();

        assert!(err.contains("cli"));
        assert!(err.contains("MISSING_TOMLX_VAR"));
    }

    #[test]
    fn test_type_only_section() {
        let source = "\n[features]  # type=bool\ndark_mode = true\nnotifications = false\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("features").unwrap().get("dark_mode").unwrap(), true);
        assert_eq!(result.section_types.get("features"), Some(&"bool".to_string()));
        assert!(result.schema.is_some());
    }

    #[test]
    fn test_type_with_target() {
        let source = "\n[ttl]  # type=int, target=seconds\nsession = 15  # unit=minutes\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.data.get("ttl").unwrap().get("session").unwrap(), 900);
        assert_eq!(result.section_types.get("ttl"), Some(&"int".to_string()));
        assert_eq!(result.section_units.get("ttl"), Some(&"seconds".to_string()));
        assert!(result.schema.is_some());
    }

    #[test]
    fn test_list_type_schema() {
        let source = "\n[tags]  # type=list[str]\ncategories = [\"news\", \"tech\"]\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();

        assert_eq!(result.section_types.get("tags"), Some(&"list[str]".to_string()));
        let schema = result.schema.unwrap();
        let props = schema.get("properties").unwrap();
        let tags_schema = props.get("tags").unwrap();
        let tags_props = tags_schema.get("properties").unwrap();
        let cat_schema = tags_props.get("categories").unwrap();
        assert_eq!(cat_schema.get("type").unwrap(), "array");
        assert_eq!(cat_schema.get("items").unwrap().get("type").unwrap(), "string");
    }

    #[test]
    fn test_output_to_json() {
        let source = "\n[ttl]  # target=seconds\nsession = 15  # unit=minutes\n";
        let result = parse_tomlx(source, None, None, &test_env).unwrap();
        let json = result.to_json();
        assert_eq!(json.get("section_units").unwrap().get("ttl").unwrap(), "seconds");
    }
}
